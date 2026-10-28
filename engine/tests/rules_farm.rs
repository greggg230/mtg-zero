//! M6 (ENGINE.md 51): the fuzz farm's first bugs. The farm (ENGINE.md 50.2)
//! fuzzes main on the Mac for ever; each test here builds, on real cube
//! cards, the state one of its failures reached, and fails on main ab02574
//! (td1's engine) unless said otherwise:
//!
//! * 51.1 a world `determinize` sampled for a best-of-three match dealt a
//!   card twice and another not at all (the hidden pool's deal against a
//!   cloud while the opponent pays);
//! * 51.2 `ChooseGraveyardCard with no options` (an opponent's Lotus Petal
//!   counted for "exile cards from your graveyard" and delve, CR 400.3);
//! * 51.3 a payment search that never ended (Gaea's Cradle's mana against
//!   its own waterbend tap: a matching kept in eight bits);
//! * 51.4 a world that changed its decider's encoding (Blightsteel Colossus
//!   surveiled into the graveyard shuffled the pile still to be placed).

mod common;

use common::db;
use engine::az::matches::{Format, MatchState, Seat};
use engine::az::{sample, MtgAz, NUM_FEATURES};
use engine::db::{AbilitySlot};
use engine::rng::Rng;
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

fn options(g: &Game) -> Vec<Choice> {
    g.pending_decision().map(|d| d.options.iter().map(|o| o.choice).collect()).unwrap_or_default()
}

fn encoding(g: &Game, viewer: PlayerId) -> Vec<f32> {
    let mut x = vec![0.0f32; NUM_FEATURES];
    g.encode_features(viewer, &mut x);
    x
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

/// Every path of the decisions from here to the next priority (each option
/// in turn, depth first, at most `budget` paths) reaches priority: none
/// panics (a decision without options panics as it is asked).
fn every_path_completes(g: &Game, budget: &mut usize) -> usize {
    if at_priority(g) || g.is_over() {
        *budget = budget.saturating_sub(1);
        return 1;
    }
    let opts = options(g);
    assert!(!opts.is_empty(), "a decision without options\n{}", log(g));
    let mut paths = 0;
    for c in opts {
        if *budget == 0 {
            break;
        }
        let mut h = g.clone();
        h.choose(c);
        paths += every_path_completes(&h, budget);
    }
    paths
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

// ---- 51.2: another player's Lotus Petal goes to its owner's graveyard ----------

/// CR 400.3: a card put into a graveyard goes to its owner's. A Lotus Petal
/// P0 controls but P1 owns (Dack Fayden's -2), sacrificed for mana, is a
/// card in P1's graveyard, not P0's: it can't be exiled by Psychic Frog's
/// "exile three cards from your graveyard" (two cards there) nor by delve.
/// Castability counted it (fuzz drafted match 10001565: Crabomination cast
/// P0's Petal for P1, then the Frog's ability was offered and asked
/// `ChooseGraveyardCard` with no options).
#[test]
fn cr_400_3_an_opponents_lotus_petal_does_not_feed_your_graveyard_costs() {
    let start = |mine: bool, hand: &[&str]| {
        let mut s = scen()
            .battlefield(0, &["Psychic Frog", "Dack Fayden", "Island", "Island"])
            .graveyard(0, &["Island", "Swamp"])
            .hand(0, hand)
            .library(0, &["Island"; 8])
            .library(1, &["Island"; 8]);
        s = if mine { s.battlefield(0, &["Lotus Petal"]) } else { s.battlefield(1, &["Lotus Petal"]) };
        let mut g = s.at(Step::Main1, 0).start();
        if !mine {
            // Dack Fayden's -2: gain control of target artifact.
            let dack = bf(&g, 0, "Dack Fayden");
            let petal = bf(&g, 1, "Lotus Petal");
            g.choose(act(dack, 1));
            finish(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(petal)));
            resolve_with(&mut g, |_| None);
            assert_eq!(g.obj(petal).controller, 0, "{}", log(&g));
        }
        g
    };
    let g = start(false, &[]);
    let frog = bf(&g, 0, "Psychic Frog");
    assert!(!g.has_option(act(frog, 1)), "two cards in P0's graveyard and P1's Petal: no exile of three\n{}", log(&g));
    let g = start(true, &[]);
    assert!(g.has_option(act(frog, 1)), "P0's own Petal: the Frog's ability");
    let mut budget = 200;
    assert!(every_path_completes(&{
        let mut h = g.clone();
        h.choose(act(frog, 1));
        h
    }, &mut budget) > 0);
    // Delve: Treasure Cruise ({7}{U}) with two Islands, the Petal and four
    // graveyard cards (two more added): the Petal's mana and, P0's own, its
    // card delved.
    let cruise = |mine: bool| {
        let mut s = scen()
            .battlefield(0, &["Island", "Island"])
            .graveyard(0, &["Island", "Swamp", "Forest", "Mountain"])
            .hand(0, &["Treasure Cruise"])
            .battlefield(0, &["Dack Fayden"])
            .library(0, &["Island"; 8])
            .library(1, &["Island"; 8]);
        s = if mine { s.battlefield(0, &["Lotus Petal"]) } else { s.battlefield(1, &["Lotus Petal"]) };
        let mut g = s.at(Step::Main1, 0).start();
        if !mine {
            let dack = bf(&g, 0, "Dack Fayden");
            let petal = bf(&g, 1, "Lotus Petal");
            g.choose(act(dack, 1));
            finish(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(petal)));
            resolve_with(&mut g, |_| None);
        }
        g
    };
    let g = cruise(false);
    let tc = g.card(0, "Treasure Cruise", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(tc)), "seven: P1's Petal is not delved\n{}", log(&g));
    let g = cruise(true);
    assert!(g.has_option(Choice::Cast(tc)), "eight with P0's own Petal delved");
    let mut h = g.clone();
    h.choose(Choice::Cast(tc));
    let mut budget = 500;
    assert!(every_path_completes(&h, &mut budget) > 0);
}

// ---- 51.3: a payment search that never ended ---------------------------------

/// CR 605.3a, 601.2g; Aang's waterbend (tap artifacts and creatures, each
/// paying {1}). A card's ways to pay are folded into one when one way makes
/// every other's mana (`pay::covers`): Gaea's Cradle animated by Nissa,
/// Who Shakes the World, a creature, either taps for {G} per creature or for
/// waterbend's {1}. The check matched the units of two ways with the used
/// ones in eight bits: past eight units every permutation was tried, and
/// with 28 creatures the castability check of Aang's ability never ended
/// (fuzz game 10049522 ran for over 50 minutes; a self-play worker would
/// hang its run). The check now ends at once (a bipartite matching).
#[test]
fn cr_605_3a_gaeas_cradle_with_many_creatures_tapped_for_waterbend_is_checked_at_once() {
    // (In a thread of its own, against a clock: on ab02574 the checks below
    // take minutes; here, milliseconds.)
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut names = vec!["Gaea's Cradle", "Nissa, Who Shakes the World", "Aang, Swift Savior // Aang and La, Ocean's Fury"];
        names.extend(["Llanowar Elves"; 26]);
        let mut g = scen().battlefield(0, &names).library(0, &["Forest"; 8]).library(1, &["Island"; 8]).at(Step::Main1, 0).start();
        let cradle = bf(&g, 0, "Gaea's Cradle");
        let nissa = bf(&g, 0, "Nissa, Who Shakes the World");
        // Nissa's +1: the Cradle a 0/0 creature with three counters, haste.
        g.choose(act(nissa, 0));
        finish(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(cradle)));
        resolve_with(&mut g, |_| None);
        assert!(g.is_creature(cradle));
        assert_eq!(g.creatures_of(0).count(), 28);
        // Aang's waterbend {8}: offered, and every payment path completes.
        let aang = bf(&g, 0, "Aang, Swift Savior // Aang and La, Ocean's Fury");
        let offered = g.has_option(act(aang, 0));
        let mut h = g.clone();
        h.choose(act(aang, 0));
        let mut budget = 64;
        let paths = every_path_completes(&h, &mut budget);
        let _ = tx.send((offered, paths));
    });
    let (offered, paths) = rx
        .recv_timeout(std::time::Duration::from_secs(30))
        .expect("the castability and payment of Aang's waterbend (28 creatures, Gaea's Cradle one of them) did not end in 30 s");
    assert!(offered, "waterbend {{8}}");
    assert!(paths > 0);
}

// ---- 51.4: a surveil's replacement shuffle -------------------------------------

/// CR 701.25a (surveil: put any number of the looked-at cards into the
/// graveyard and the rest on top, one action), 614.1a (Blightsteel
/// Colossus: "if it would be put into a graveyard from anywhere, reveal it
/// and shuffle it into its owner's library instead"). The engine asks the
/// cards one by one; the Colossus picked first shuffled the library at
/// once, the other looked-at card still to be placed in it: its player no
/// longer knew which card it was, a world `determinize` sampled for that
/// player dealt it again, and the decision's encoding changed (fuzz match
/// 10036882 with `FUZZ_POOL=interactions` at 075649c: Kaito's surveil). The
/// shuffle now follows the pile's placement.
#[test]
fn cr_701_25a_a_surveiled_blightsteel_colossus_shuffles_after_the_pile_is_placed() {
    let start = || {
        scen()
            .battlefield(0, &["Kaito, Bane of Nightmares"])
            .library(0, &["Blightsteel Colossus", "Screaming Nemesis", "Island", "Island", "Island", "Island", "Island", "Island"])
            .library(1, &["Island"; 8])
            .at(Step::Main1, 0)
            .start()
    };
    for to_graveyard in [false, true] {
        let mut g = start();
        let kaito = bf(&g, 0, "Kaito, Bane of Nightmares");
        let bs = g.card(0, "Blightsteel Colossus", Zone::Library);
        let nemesis = g.card(0, "Screaming Nemesis", Zone::Library);
        let nemesis_def = g.obj(nemesis).def;
        // Kaito's 0: surveil 2.
        g.choose(act(kaito, 1));
        finish(&mut g, |_| None);
        g.pass();
        g.pass();
        assert_eq!(g.kind(), Some(DecisionKind::PutInGraveyard), "{}", log(&g));
        g.choose(Choice::Card(bs));
        assert_eq!(g.kind(), Some(DecisionKind::PutInGraveyard), "{}", log(&g));
        assert!(options(&g).contains(&Choice::Card(nemesis)));
        assert!(g.knows(0, nemesis), "P0 still knows the looked-at Screaming Nemesis");
        // Every world P0 samples keeps it, and P0's encoding.
        let x = encoding(&g, 0);
        for s in 0..32 {
            let mut d = g.clone();
            d.determinize(0, &mut Rng::new(s));
            assert_eq!(d.ident(nemesis), nemesis_def, "world {s}");
            assert_eq!(encoding(&d, 0), x, "world {s}: P0's encoding");
        }
        // Then the Nemesis into the graveyard or on top; the library is
        // shuffled after (the Colossus in it, revealed).
        if to_graveyard {
            g.choose(Choice::Card(nemesis));
        } else {
            g.choose(Choice::Pass);
        }
        resolve_with(&mut g, |_| None);
        assert_eq!(g.obj(bs).zone, Zone::Library, "{}", log(&g));
        assert_eq!(g.obj(nemesis).zone, if to_graveyard { Zone::Graveyard } else { Zone::Library });
        assert_eq!(g.player(0).library.len(), if to_graveyard { 7 } else { 8 });
        if !to_graveyard {
            assert!(!g.knows(0, nemesis), "shuffled: nobody knows where it is");
        }
        g.check_knowledge().unwrap();
    }
}
