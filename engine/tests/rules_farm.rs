//! M6 (ENGINE.md 51): the fuzz farm's first bugs. The farm (ENGINE.md 50.2)
//! fuzzes main on the Mac for ever; each test here builds, on real cube
//! cards, the state one of its failures reached, and fails on main ab02574
//! (td1's engine) unless said otherwise:
//!
//! * 51.1 a world `determinize` sampled for a best-of-three match dealt a
//!   card twice and another not at all (the hidden pool's deal against a
//!   cloud while the opponent pays).

mod common;

use common::db;
use engine::az::matches::{Format, MatchState, Seat};
use engine::az::{sample, MtgAz};
use engine::db::{AbilitySlot};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};
use zero_search::game::SearchGame;

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| {
        s.auto_single = false;
        s.log = true;
    })
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn act(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(i))
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

/// Answer decisions (`answer`, else the first option) until a player has
/// priority.
fn finish(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..500 {
        if at_priority(g) || g.is_over() {
            return;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("finish: no progress\n{}", log(g));
}

/// Pass priority (and answer other decisions with `answer`, else the first
/// option) until the stack is empty and a player has priority.
fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..500 {
        if g.is_over() || (g.stack().is_empty() && at_priority(g)) {
            return;
        }
        if at_priority(g) {
            g.pass();
            continue;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("resolve_with: no progress\n{}", log(g));
}

// ---- 51.1: the hidden pool's deal and a payment's pinned hand -----------------

/// ENGINE.md 36.11, 10. In a best-of-three match `determinize(viewer)`
/// samples the opponent's deck and deals its unknown cards to the slots the
/// viewer does not know; `Game::determinize` then keeps the hand mana
/// sources of an opponent who is paying (pinned: the payment may count on
/// them) and deals the rest so each cloud's cards stay in its domain. The
/// hidden pool's deal ignored the clouds, so a card a cloud places in the
/// library could be dealt to a hand slot; a hand mana source there (Elvish
/// Spirit Guide) was then pinned, the cloud's card was missing from what
/// the second deal shared out and was dealt again: the world held it twice
/// and another card of the deck not at all (fuzz match 10009649: Elvish
/// Spirit Guide twice, no Jennifer Walters). Here the Guide is in P0's
/// graveyard; P0 discards Emrakul, the Aeons Torn to Psychic Frog, whose
/// trigger shuffles the graveyard into the library (P1 now knows the Guide
/// is somewhere in it: a cloud), and casts Lightning Bolt, paying. Every
/// world P1 samples holds P0's sampled deck exactly, the Guide in the
/// library.
#[test]
fn hidden_pool_deal_keeps_a_clouds_card_in_its_domain_while_its_owner_pays() {
    // Every card of P0's that P1 does not know sorts after the Guide
    // (definitions are in name order), so the deal that ignored the clouds
    // gave it the first hand slot.
    let mut p0_lib = vec!["Mountain"; 16];
    p0_lib.extend(["Forest"; 16]);
    let mut g = scen()
        .spec(|s| s.hidden_decks = true)
        .battlefield(0, &["Psychic Frog", "Mountain", "Mountain"])
        .hand(0, &["Emrakul, the Aeons Torn", "Lightning Bolt", "Mountain", "Forest"])
        .graveyard(0, &["Elvish Spirit Guide"])
        .library(0, &p0_lib)
        .battlefield(1, &["Island", "Island", "Island"])
        .library(1, &["Island"; 37])
        .at(Step::Main1, 0)
        .start();
    // The match: the decks are the game's 40 + 40 cards (objects 0..80).
    let deck = |g: &Game, p: PlayerId| {
        let mut v: Vec<u16> = g.objs().iter().filter(|o| o.owner == p).map(|o| o.def).collect();
        v.sort_unstable();
        v
    };
    assert_eq!((deck(&g, 0).len(), deck(&g, 1).len(), g.objs().len()), (40, 40, 80));
    let seat = |p| Seat { deck: deck(&g, p), sideboard: Vec::new(), first_picks: Vec::new(), elsewhere: Vec::new() };
    let m = MatchState::new(db(), Format::Bo3, *g.spec(), &[seat(0), seat(1)], 7).unwrap();
    let esg = g.card(0, "Elvish Spirit Guide", Zone::Graveyard);
    let emrakul = g.card(0, "Emrakul, the Aeons Torn", Zone::Hand);
    // The Frog's "discard a card": Emrakul; its trigger shuffles P0's
    // graveyard into the library.
    let frog = bf(&g, 0, "Psychic Frog");
    g.choose(act(frog, 0));
    finish(&mut g, |g| (g.kind() == Some(DecisionKind::DiscardCost)).then_some(Choice::Card(emrakul)));
    resolve_with(&mut g, |_| None);
    assert_eq!((g.obj(esg).zone, g.obj(emrakul).zone), (Zone::Library, Zone::Library), "{}", log(&g));
    assert!(!g.knows(1, esg));
    assert!(
        g.clouds().iter().any(|k| k.viewer == 1 && k.owner == 0 && k.library && !k.hand && k.defs.contains(&g.obj(esg).def)),
        "P1 knows the Guide is in P0's library: {:?}",
        g.clouds()
    );
    // P0 casts Lightning Bolt: the payment is P0's next decision.
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    for _ in 0..20 {
        match g.kind() {
            Some(DecisionKind::PayMana) => break,
            Some(DecisionKind::Target) => g.choose(Choice::Target(Target::Player(1))),
            _ => {
                let c = g.pending_decision().unwrap().options[0].choice;
                g.choose(c)
            }
        };
    }
    assert_eq!(g.kind(), Some(DecisionKind::PayMana), "{}", log(&g));
    assert!(g.player(0).hand.iter().all(|&c| !g.knows(1, c)) && !g.player(0).hand.is_empty());
    let esg_def = g.obj(esg).def;
    let az = MtgAz { g, m };
    for s in 0..64 {
        let mut d = az.clone();
        d.determinize(1, &mut zero_search::rng::Rng::new(s));
        let want = d.m.deck(0);
        assert_eq!(sample::game_cards(&d, 0), want, "world {s}: P0's cards are not the sampled deck");
        let guides: Vec<CardIdx> = (0..40).map(|i| CardIdx(i as u16)).filter(|&c| sample::card_of(&d.g, c) == esg_def).collect();
        assert_eq!(guides.len(), 1, "world {s}: one Elvish Spirit Guide");
        assert_eq!(d.g.obj(guides[0]).zone, Zone::Library, "world {s}: the cloud's Guide in the library");
        d.g.check_knowledge().unwrap_or_else(|e| panic!("world {s}: {e}"));
        d.g.check_invariants().unwrap_or_else(|e| panic!("world {s}: {e}"));
    }
}
