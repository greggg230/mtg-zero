//! Legacy Doomsday, lane A (ENGINE.md 124.1): Doomsday, Pact of Negation,
//! Personal Tutor and Flow State against their Oracle text, the
//! Comprehensive Rules and their official rulings (`// ruling: <hash8>`,
//! `data/rulings/legacy.rulings.json`), and their interactions with the
//! deck's other cards. They are in the card table of constructed play only
//! (`CardDb::constructed`).
//!
//! Doomsday {B}{B}{B}, Sorcery: "Search your library and graveyard for five
//! cards and exile the rest. Put the chosen cards on top of your library in
//! any order. You lose half your life, rounded up."
//!
//! Pact of Negation {0}, Instant (blue: a colour indicator): "Counter target
//! spell. At the beginning of your next upkeep, pay {3}{U}{U}. If you don't,
//! you lose the game."
//!
//! Personal Tutor {U}, Sorcery: "Search your library for a sorcery card,
//! reveal it, then shuffle and put that card on top."
//!
//! Flow State {1}{U}, Sorcery: "Look at the top three cards of your library.
//! Put one of them into your hand and the rest on the bottom of your library
//! in any order. If there is an instant card and a sorcery card in your
//! graveyard, instead put two of them into your hand and the rest on the
//! bottom of your library in any order."

use engine::db::CardDb;
use engine::game::EndReason;
use engine::hidden::{key_kind, key_payload, KEY_REVEAL};
use engine::rng::Rng;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};
use std::collections::BTreeSet;

const DD: &str = "Doomsday";
const PACT: &str = "Pact of Negation";
const PT: &str = "Personal Tutor";
const FS: &str = "Flow State";
const ORACLE: &str = "Thassa's Oracle";

fn db() -> &'static CardDb {
    CardDb::constructed()
}

fn def(name: &str) -> u16 {
    db().id(name).unwrap_or_else(|| panic!("no card {name}"))
}

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false)
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn opts(g: &Game) -> String {
    g.options_desc().join(" | ")
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

/// `p`'s library, top first, by name.
fn library(g: &Game, p: PlayerId) -> Vec<&'static str> {
    g.player(p).library.iter().rev().map(|&c| g.name(c)).collect()
}

fn names(g: &Game, cards: &[CardIdx]) -> Vec<&'static str> {
    cards.iter().map(|&c| g.name(c)).collect()
}

/// Cast `name` from `p`'s hand: `target` for a target, the first option for
/// the rest (mana payments), until priority.
fn cast(g: &mut Game, p: PlayerId, name: &str, target: Option<Choice>) {
    let c = g.card(p, name, Zone::Hand);
    assert!(g.has_option(Choice::Cast(c)), "{name} castable: {}", opts(g));
    g.choose(Choice::Cast(c));
    for _ in 0..100 {
        if at_priority(g) {
            return;
        }
        let d = g.pending_decision().expect("a decision");
        let ch = match d.kind {
            DecisionKind::Target => target.unwrap_or_else(|| panic!("a target for {name}: {}", opts(g))),
            _ => d.options[0].choice,
        };
        g.choose(ch);
    }
    panic!("cast {name}: no priority\n{}", log(g));
}

/// Pass priority until another decision is asked or the stack is empty.
fn pass_on(g: &mut Game) {
    g.pass_until(|g| g.stack().is_empty());
}

/// Answer the decisions of a resolving Doomsday: the graveyard picks
/// `gy`, then `PASS` while allowed; the library picks `lib` (definitions,
/// picked in non-decreasing definition order, as the search asks); the
/// order `top` (top first).
fn answer_doomsday(g: &mut Game, gy: &[CardIdx], lib: &[&str], top: &[&str]) {
    let mut lib: Vec<&str> = lib.to_vec();
    lib.sort_by_key(|n| def(n));
    // (The graveyard picks in increasing position, as the search asks.)
    let mut gy: Vec<CardIdx> = gy.to_vec();
    gy.sort_by_key(|&c| g.player(0).graveyard.iter().position(|&x| x == c));
    let mut gi = 0;
    while g.kind() == Some(DecisionKind::ChooseGraveyardCard) {
        if gi < gy.len() {
            g.choose(Choice::Card(gy[gi]));
            gi += 1;
        } else {
            g.pass();
        }
    }
    assert_eq!(gi, gy.len(), "graveyard picks: {}", opts(g));
    let mut li = 0;
    while g.kind() == Some(DecisionKind::SearchLibrary) {
        let d = def(lib.get(li).unwrap_or_else(|| panic!("a library pick asked past {lib:?}: {}", opts(g))));
        g.choose(Choice::Def(d));
        li += 1;
    }
    assert_eq!(li, lib.len(), "library picks: {}\n{}", opts(g), log(g));
    // The pile to the top: the last card chosen ends on top.
    let mut k = top.len();
    while g.kind() == Some(DecisionKind::PutOnTop) {
        k -= 1;
        let want = top[k];
        let pick = g
            .pending_decision()
            .unwrap()
            .options
            .iter()
            .find_map(|o| match o.choice {
                Choice::Card(c) if g.name(c) == want => Some(o.choice),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{want} on top: {}", opts(g)));
        g.choose(pick);
    }
    assert_eq!(k, 0, "the order: {}", opts(g));
}

/// P0 at its precombat main phase with three Swamps and Doomsday in hand.
fn dd_scen(lib: &[&str], gy: &[&str]) -> Scenario {
    scen().battlefield(0, &["Swamp", "Swamp", "Swamp"]).hand(0, &[DD]).library(0, lib).graveyard(0, gy).library(1, &["Island"; 10]).at(Step::Main1, 0)
}

/// Cast Doomsday and pass until its first choice.
fn cast_doomsday(g: &mut Game) {
    cast(g, 0, DD, None);
    pass_on(g);
}

const LIB10: &[&str] = &["Brainstorm", "Ponder", "Consider", "Daze", ORACLE, "Dark Ritual", "Lotus Petal", "Island", "Underground Sea", "Polluted Delta"];

// ---- Doomsday ------------------------------------------------------------------

#[test]
fn doomsday_is_a_black_sorcery_of_mana_value_3() {
    let g = dd_scen(&[], &[]).start();
    let d = db().def(def(DD));
    assert_eq!(d.mana_value(), 3);
    assert!(d.types & engine::db::types::SORCERY != 0);
    let c = g.card(0, DD, Zone::Hand);
    assert!(g.has_option(Choice::Cast(c)), "{}", opts(&g));
}

#[test]
fn cr_701_23d_doomsday_must_find_five_of_its_library_and_graveyard() {
    // ruling: 4c669d8c
    // "Five cards": a quantity without a stated quality, so the search must
    // find five when the two zones hold five or more (CR 701.23d).
    let mut g = dd_scen(LIB10, &["Force of Will", "Thoughtseize"]).start();
    cast_doomsday(&mut g);
    // The graveyard first: each card, in increasing position, and PASS (the
    // library alone holds five).
    assert_eq!(g.kind(), Some(DecisionKind::ChooseGraveyardCard), "{}", opts(&g));
    assert!(g.has_option(Choice::Pass));
    let fow = g.card(0, "Force of Will", Zone::Graveyard);
    let ts = g.card(0, "Thoughtseize", Zone::Graveyard);
    assert!(g.has_option(Choice::Card(fow)) && g.has_option(Choice::Card(ts)));
    g.choose(Choice::Card(fow));
    // Above Force of Will only: Thoughtseize, or PASS.
    assert!(!g.has_option(Choice::Card(fow)) && g.has_option(Choice::Card(ts)) && g.has_option(Choice::Pass));
    g.pass();
    // Four from the library: no PASS (CR 701.23d). One definition per
    // decision in non-decreasing order (each multiset once): the first pick
    // offers the definitions that leave at least three cards after them.
    assert_eq!(g.kind(), Some(DecisionKind::SearchLibrary));
    assert!(!g.has_option(Choice::Pass), "{}", opts(&g));
    let mut sorted: Vec<u16> = LIB10.iter().map(|n| def(n)).collect();
    sorted.sort();
    for (i, &d) in sorted.iter().enumerate() {
        assert_eq!(g.has_option(Choice::Def(d)), sorted.len() - i >= 4, "{}: {}", db().def(d).name, opts(&g));
    }
    answer_doomsday(&mut g, &[], &["Brainstorm", "Dark Ritual", "Lotus Petal", ORACLE], &[ORACLE, "Force of Will", "Brainstorm", "Lotus Petal", "Dark Ritual"]);
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    // The chosen five on top in the chosen order; the rest of both zones
    // exiled; Doomsday in the graveyard; 20 -> 10 life.
    assert_eq!(library(&g, 0), vec![ORACLE, "Force of Will", "Brainstorm", "Lotus Petal", "Dark Ritual"]);
    assert_eq!(names(&g, &g.player(0).graveyard.clone()), vec![DD]);
    for n in ["Ponder", "Consider", "Daze", "Island", "Underground Sea", "Polluted Delta", "Thoughtseize"] {
        assert_eq!(g.count_in(0, n, Zone::Exile), 1, "{n} exiled\n{}", log(&g));
    }
    assert_eq!(g.life(0), 10);
    // P0 knows its five exactly, at their places.
    assert!(g.player(0).library.iter().all(|&c| g.knows(0, c)));
    g.check_invariants().unwrap();
    g.check_knowledge().unwrap();
}

#[test]
fn cr_701_23d_with_fewer_than_five_cards_all_of_them_go_on_top() {
    // ruling: 6c6f247d
    let mut g = dd_scen(&["Brainstorm", "Ponder"], &["Dark Ritual"]).start();
    cast_doomsday(&mut g);
    // The graveyard card must be found (the library holds only two): no PASS.
    let dr = g.card(0, "Dark Ritual", Zone::Graveyard);
    assert_eq!(g.kind(), Some(DecisionKind::ChooseGraveyardCard));
    assert!(!g.has_option(Choice::Pass) && g.has_option(Choice::Card(dr)), "{}", opts(&g));
    answer_doomsday(&mut g, &[dr], &["Brainstorm", "Ponder"], &["Ponder", "Dark Ritual", "Brainstorm"]);
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(library(&g, 0), vec!["Ponder", "Dark Ritual", "Brainstorm"]);
    assert_eq!(g.player(0).graveyard.len(), 1, "Doomsday only");
    assert_eq!(g.life(0), 10);
}

#[test]
fn doomsday_with_an_empty_library_and_graveyard_finds_nothing_and_costs_half_the_life() {
    let mut g = dd_scen(&[], &[]).life(0, 7).start();
    cast_doomsday(&mut g);
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert!(g.player(0).library.is_empty());
    assert_eq!(names(&g, &g.player(0).graveyard.clone()), vec![DD]);
    // CR 107.1a: half of 7 rounded up is 4.
    assert_eq!(g.life(0), 3);
}

#[test]
fn doomsday_from_the_graveyard_alone_must_take_five_in_increasing_position() {
    // An empty library: five of the seven graveyard cards, chosen in
    // increasing position (each set once), no PASS before five, and no pick
    // that leaves too few cards above it.
    let gy = ["Brainstorm", "Ponder", "Consider", "Daze", "Dark Ritual", "Lotus Petal", ORACLE];
    let mut g = dd_scen(&[], &gy).start();
    cast_doomsday(&mut g);
    let cards = g.player(0).graveyard.clone();
    // First pick: positions 0..=2 (from 3 on, fewer than five cards remain).
    let offered = |g: &Game| -> Vec<CardIdx> {
        g.pending_decision()
            .unwrap()
            .options
            .iter()
            .filter_map(|o| match o.choice {
                Choice::Card(c) => Some(c),
                _ => None,
            })
            .collect()
    };
    assert_eq!(offered(&g), cards[0..3].to_vec());
    assert!(!g.has_option(Choice::Pass));
    g.choose(Choice::Card(cards[1]));
    assert_eq!(offered(&g), cards[2..4].to_vec());
    g.choose(Choice::Card(cards[3]));
    g.choose(Choice::Card(cards[4]));
    g.choose(Choice::Card(cards[5]));
    assert_eq!(offered(&g), vec![cards[6]], "the last pick is forced");
    g.choose(Choice::Card(cards[6]));
    // No library pick; the order of the five.
    assert_eq!(g.kind(), Some(DecisionKind::PutOnTop), "{}", opts(&g));
    answer_doomsday(&mut g, &[], &[], &[ORACLE, "Ponder", "Daze", "Dark Ritual", "Lotus Petal"]);
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(library(&g, 0), vec![ORACLE, "Ponder", "Daze", "Dark Ritual", "Lotus Petal"]);
    assert_eq!(g.count_in(0, "Brainstorm", Zone::Exile) + g.count_in(0, "Consider", Zone::Exile), 2);
}

/// Every decision path of a resolving Doomsday from `g` (P0 deciding):
/// (paths, the distinct libraries and exiles it can leave, top first).
fn doomsday_outcomes(g: &Game) -> (usize, BTreeSet<(Vec<&'static str>, Vec<&'static str>)>) {
    let mut out = BTreeSet::new();
    let mut paths = 0;
    let mut stack = vec![g.clone()];
    while let Some(g) = stack.pop() {
        if g.stack().is_empty() && at_priority(&g) {
            let mut ex: Vec<&'static str> = Vec::new();
            for p in 0..1 {
                for n in [
                    "Island",
                    "Brainstorm",
                    "Ponder",
                    "Consider",
                    "Daze",
                    "Dark Ritual",
                    "Lotus Petal",
                    ORACLE,
                ] {
                    for _ in 0..g.count_in(p, n, Zone::Exile) {
                        ex.push(n);
                    }
                }
            }
            paths += 1;
            out.insert((library(&g, 0), ex));
            continue;
        }
        let d = g.pending_decision().unwrap().clone();
        assert_eq!(d.player, 0, "{:?}", d.kind);
        for o in &d.options {
            let mut h = g.clone();
            h.choose(o.choice);
            if at_priority(&h) && !h.stack().is_empty() {
                pass_on(&mut h);
            }
            stack.push(h);
        }
    }
    (paths, out)
}

#[test]
fn every_pile_and_every_order_is_reachable_once() {
    // Six different cards, three in the library and three in the graveyard:
    // any five of them (6 ways) in any order (120): 720 outcomes, each
    // reached by exactly one sequence of decisions.
    let mut g = dd_scen(&["Brainstorm", "Ponder", "Consider"], &["Daze", "Dark Ritual", "Lotus Petal"]).start();
    cast_doomsday(&mut g);
    let (paths, out) = doomsday_outcomes(&g);
    assert_eq!(out.len(), 720);
    assert_eq!(paths, 720);
    // With equal cards: two Islands in the library. Leaving out an Island,
    // the others are five different cards (120 orders); leaving out another
    // card, four kinds with a pair (60 orders each, 4 such cards): 360.
    let mut g = dd_scen(&["Island", "Island", "Brainstorm"], &["Ponder", "Consider", "Daze"]).start();
    cast_doomsday(&mut g);
    let (paths, out) = doomsday_outcomes(&g);
    assert_eq!(out.len(), 360);
    assert_eq!(paths, 360);
}

#[test]
fn doomsday_at_odd_life_loses_half_rounded_up_and_at_1_life_its_caster_loses() {
    // CR 107.1a: 21 -> 10 (11 lost).
    let mut g = dd_scen(LIB10, &[]).life(0, 21).start();
    cast_doomsday(&mut g);
    answer_doomsday(&mut g, &[], &["Brainstorm", "Ponder", "Consider", "Daze", ORACLE], &["Brainstorm", "Ponder", "Consider", "Daze", ORACLE]);
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(g.life(0), 10);
    // At 1 life: 1 lost, 0 life, the state-based action (CR 104.3b).
    let mut g = dd_scen(LIB10, &[]).life(0, 1).start();
    cast_doomsday(&mut g);
    answer_doomsday(&mut g, &[], &["Brainstorm", "Ponder", "Consider", "Daze", ORACLE], &["Brainstorm", "Ponder", "Consider", "Daze", ORACLE]);
    g.drive_until(|g| g.is_over());
    assert_eq!(g.life(0), 0);
    let o = g.outcome().unwrap();
    assert_eq!((o.winner, o.reason), (Some(1), EndReason::Sba));
}

#[test]
fn doomsday_reveals_the_exiled_library_cards_in_definition_order() {
    // The library's order was hidden: its exiled cards are revealed sorted
    // by definition, so the keys say only which cards they were.
    let mut g = dd_scen(LIB10, &[]).start();
    cast_doomsday(&mut g);
    let mut keys = Vec::new();
    let mut picks: Vec<u16> = ["Brainstorm", "Ponder", "Consider", "Daze", ORACLE].map(def).to_vec();
    picks.sort();
    for d in picks {
        keys.extend(g.choose(Choice::Def(d)));
    }
    let revealed: Vec<u16> = keys.iter().filter(|&&k| key_kind(k) == KEY_REVEAL).map(|&k| key_payload(k) as u16).collect();
    let mut want: Vec<u16> = ["Dark Ritual", "Lotus Petal", "Island", "Underground Sea", "Polluted Delta"].map(def).to_vec();
    want.sort();
    assert_eq!(revealed, want);
}

// ---- Doomsday: who knows what ------------------------------------------------------

/// The definitions of `p`'s hand and library in `g`.
fn defs_in(g: &Game, cards: &[CardIdx]) -> Vec<u16> {
    cards.iter().map(|&c| g.obj(c).def).collect()
}

#[test]
fn doomsday_the_opponent_knows_the_graveyard_cards_are_among_the_five_but_not_where() {
    // Force of Will is P0's only copy: from the graveyard into the library,
    // in an order P0 chooses in private.
    let mut g = dd_scen(LIB10, &["Force of Will"]).hand(0, &["Island", "Daze"]).start();
    cast_doomsday(&mut g);
    let fow = g.card(0, "Force of Will", Zone::Graveyard);
    answer_doomsday(&mut g, &[fow], &["Brainstorm", "Ponder", "Consider", ORACLE], &[ORACLE, "Brainstorm", "Force of Will", "Ponder", "Consider"]);
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    let five = g.player(0).library.clone();
    // P0 knows its five, in order: every world it samples keeps them.
    for s in 0..20 {
        let mut d = g.clone();
        d.determinize(0, &mut Rng::new(s));
        assert_eq!(defs_in(&d, &five), defs_in(&g, &five), "P0's own library order");
    }
    // P1 saw Force of Will go into the library and saw what was exiled: in
    // every world it samples, Force of Will is among P0's five library cards,
    // never in P0's hand; where, it doesn't know.
    let cloud = g.clouds().iter().find(|k| k.viewer == 1 && k.owner == 0).expect("P1's cloud over P0's library");
    assert!(!cloud.hand && (cloud.library || five.iter().all(|c| cloud.slots.contains(c))), "{cloud:?}");
    assert_eq!(cloud.defs, vec![def("Force of Will")], "{cloud:?}");
    let mut places = BTreeSet::new();
    let mut hands_differ = false;
    for s in 0..60 {
        let mut d = g.clone();
        d.determinize(1, &mut Rng::new(s));
        d.check_knowledge().unwrap();
        let lib = defs_in(&d, &d.player(0).library.clone());
        let hand = defs_in(&d, &d.player(0).hand.clone());
        assert_eq!(lib.iter().filter(|&&x| x == def("Force of Will")).count(), 1, "seed {s}: {lib:?}");
        assert!(!hand.contains(&def("Force of Will")), "seed {s}: never in P0's hand");
        places.insert(lib.iter().position(|&x| x == def("Force of Will")).unwrap());
        hands_differ |= hand != defs_in(&g, &g.player(0).hand.clone());
        // The exiled cards are public.
        assert_eq!(d.count_in(0, "Daze", Zone::Exile), 1);
    }
    assert!(places.len() > 1, "P1 doesn't know where Force of Will is: {places:?}");
    assert!(hands_differ, "P0's hand and the four library cards it found are hidden from P1");
}

/// P0 casts Personal Tutor for Doomsday (revealed, on top: both players know
/// it), then Doomsday from its hand, keeping `keep` library cards.
fn tutor_then_doomsday(lib: &[&str], keep: &[&str]) -> Game {
    let mut g = scen()
        .battlefield(0, &["Island", "Swamp", "Swamp", "Swamp"])
        .hand(0, &[PT, DD, "Brainstorm", "Ponder"])
        .library(0, lib)
        .library(1, &["Island"; 10])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, PT, None);
    pass_on(&mut g);
    assert_eq!(g.kind(), Some(DecisionKind::SearchLibrary));
    g.choose(Choice::Def(def(DD)));
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    let top = *g.player(0).library.last().unwrap();
    assert_eq!(g.name(top), DD);
    assert!(g.knows(0, top) && g.knows(1, top), "revealed on top");
    let hand_dd = g.card(0, DD, Zone::Hand);
    g.choose(Choice::Cast(hand_dd));
    g.drive_until(at_priority);
    pass_on(&mut g);
    let order: Vec<&str> = keep.to_vec();
    answer_doomsday(&mut g, &[], keep, &order);
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    g
}

#[test]
fn doomsday_keeping_a_library_card_the_opponent_knew_keeps_it_among_the_five() {
    // P1 knew the top card was Doomsday (Personal Tutor revealed it); the
    // search is private, but no Doomsday was exiled: it is among the five.
    let lib = ["Island", DD, "Consider", "Daze", ORACLE, "Dark Ritual", "Lotus Petal"];
    let g = tutor_then_doomsday(&lib, &[DD, "Consider", "Daze", ORACLE, "Dark Ritual"]);
    for s in 0..40 {
        let mut d = g.clone();
        d.determinize(1, &mut Rng::new(s));
        d.check_knowledge().unwrap();
        assert!(defs_in(&d, &d.player(0).library.clone()).contains(&def(DD)), "seed {s}");
        assert!(!defs_in(&d, &d.player(0).hand.clone()).contains(&def(DD)), "seed {s}");
    }
}

#[test]
fn doomsday_exiling_a_copy_of_a_known_card_leaves_the_opponent_unsure_where_the_other_is() {
    // Two Doomsdays in the library, one known on top (Personal Tutor). P0
    // keeps one and exiles the other: P1 sees "a Doomsday" exiled and can't
    // tell which, so it can't know the kept one is in the library rather
    // than P0's hand: no world-keeping claim (the cloud lost it).
    let lib = ["Island", DD, "Consider", "Daze", ORACLE, "Dark Ritual", "Lotus Petal", DD];
    let g = tutor_then_doomsday(&lib, &[DD, "Consider", "Daze", ORACLE, "Dark Ritual"]);
    assert_eq!(g.count_in(0, DD, Zone::Exile), 1);
    assert!(g.clouds().iter().all(|k| !(k.viewer == 1 && k.owner == 0 && k.defs.contains(&def(DD)))), "{:?}", g.clouds());
    let in_hand = (0..80).any(|s| {
        let mut d = g.clone();
        d.determinize(1, &mut Rng::new(s));
        defs_in(&d, &d.player(0).hand.clone()).contains(&def(DD))
    });
    assert!(in_hand, "a world with the other Doomsday in P0's hand");
}

#[test]
fn doomsday_exiling_the_known_card_makes_it_public() {
    let lib = ["Island", DD, "Consider", "Daze", ORACLE, "Dark Ritual", "Lotus Petal"];
    let g = tutor_then_doomsday(&lib, &["Island", "Consider", "Daze", ORACLE, "Dark Ritual"]);
    assert_eq!(g.count_in(0, DD, Zone::Exile), 1);
    assert!(g.clouds().iter().all(|k| !(k.viewer == 1 && k.owner == 0 && k.defs.contains(&def(DD)))));
}

// ---- Pact of Negation --------------------------------------------------------------

/// Pass priority and decline attacks and blocks until `stop`.
fn quiet_until(g: &mut Game, stop: impl Fn(&Game) -> bool) {
    for _ in 0..10_000 {
        if stop(g) {
            return;
        }
        let d = g.pending_decision().unwrap_or_else(|| panic!("game over: {:?}\n{}", g.outcome(), log(g)));
        let c = match d.kind {
            DecisionKind::Priority | DecisionKind::DeclareAttackers | DecisionKind::DeclareBlockers => Choice::Pass,
            DecisionKind::OrderTriggers | DecisionKind::PayMana | DecisionKind::ChooseMana => d.options[0].choice,
            k => panic!("quiet_until: unexpected {k:?}: {}\n{}", opts(g), log(g)),
        };
        g.choose(c);
    }
    panic!("quiet_until: no progress");
}

/// Pact of Negation's trigger asks its controller (a `YesNo`), or the game
/// is over.
fn pact_asks(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::YesNo) || g.is_over()
}

/// Cast `name` from `p`'s hand with its first option for everything but its
/// target (`target`) and its alternative cost (`alt`: the last option).
fn cast_with(g: &mut Game, p: PlayerId, name: &str, target: Choice, alt: bool) {
    let c = g.card(p, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    while !at_priority(g) {
        let d = g.pending_decision().unwrap();
        let ch = match d.kind {
            DecisionKind::Target => target,
            DecisionKind::ChooseAltCost if alt => d.options.last().unwrap().choice,
            _ => d.options[0].choice,
        };
        g.choose(ch);
    }
}

/// P1's turn 3: P1 casts Lightning Bolt at P0, P0 answers with Pact of
/// Negation; both resolve. P0's `lands`.
fn bolt_then_pact(lands: &[&str]) -> Game {
    let mut g = scen()
        .battlefield(0, lands)
        .hand(0, &[PACT])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
        .at(Step::Main1, 1)
        .start();
    cast(&mut g, 1, "Lightning Bolt", Some(Choice::Target(Target::Player(0))));
    g.pass();
    let bolt = g.card(1, "Lightning Bolt", Zone::Stack);
    let t = g.tgt(bolt);
    cast(&mut g, 0, PACT, Some(t));
    pass_on(&mut g);
    assert!(g.stack().is_empty(), "{}", log(&g));
    assert_eq!(g.obj(bolt).zone, Zone::Graveyard, "countered");
    assert_eq!(g.life(0), 20);
    g
}

const FIVE: &[&str] = &["Island", "Island", "Swamp", "Swamp", "Swamp"];

#[test]
fn pact_of_negation_is_a_blue_zero_mana_instant() {
    // ruling: 0dfaac8c
    // CR 204, 202.2e: its colour indicator makes it blue (its mana cost is
    // {0}).
    let d = db().def(def(PACT));
    assert_eq!(d.mana_value(), 0);
    assert!(d.types & engine::db::types::INSTANT != 0);
    let g = scen().hand(0, &[PACT]).start();
    let c = g.card(0, PACT, Zone::Hand);
    assert_eq!(g.colors_of(c), 1 << 1, "blue only (W U B R G)");
    // A colour-changing effect still applies (the ruling): Painter's
    // Servant naming red makes it blue and red (CR 613.1e).
    let mut g = scen().battlefield(0, &["Island", "Island"]).hand(0, &["Painter's Servant", PACT]).at(Step::Main1, 0).start();
    let ps = g.card(0, "Painter's Servant", Zone::Hand);
    g.choose(Choice::Cast(ps));
    while g.kind() != Some(DecisionKind::ChooseColor) {
        let c = if at_priority(&g) { Choice::Pass } else { g.pending_decision().unwrap().options[0].choice };
        g.choose(c);
    }
    g.choose(Choice::Number(3)); // red
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    let c = g.card(0, PACT, Zone::Hand);
    assert_eq!(g.colors_of(c), (1 << 1) | (1 << 3), "blue and red");
}

#[test]
fn cr_603_7_pact_counters_and_its_controller_pays_at_their_next_upkeep() {
    let mut g = bolt_then_pact(FIVE);
    // Nothing at P1's end step or anywhere before P0's upkeep.
    quiet_until(&mut g, |g| pact_asks(g) || (g.turn().active == 0 && g.turn().step == Step::Draw));
    assert_eq!(g.kind(), Some(DecisionKind::YesNo), "{}", log(&g));
    assert_eq!((g.turn().active, g.turn().step, g.turn().number), (0, Step::Upkeep, 4));
    assert!(g.has_option(Choice::Yes) && g.has_option(Choice::Pass), "may pay, may decline: {}", opts(&g));
    g.choose(Choice::Yes);
    quiet_until(&mut g, |g| g.stack().is_empty() && at_priority(g));
    assert!(!g.is_over());
    // {3}{U}{U}: the five lands are tapped.
    assert!(g.player(0).pool.amt.iter().all(|&m| m == 0));
    quiet_until(&mut g, |g| g.turn().step == Step::Main1);
    assert!(!g.is_over());
}

#[test]
fn cr_104_3e_declining_to_pay_loses_the_game_even_when_able() {
    let mut g = bolt_then_pact(FIVE);
    quiet_until(&mut g, pact_asks);
    assert!(g.has_option(Choice::Yes));
    g.pass();
    assert!(g.is_over());
    let o = g.outcome().unwrap();
    assert_eq!((o.winner, o.reason), (Some(1), EndReason::Effect));
    assert!(g.player(0).lost && !g.player(1).lost);
}

#[test]
fn pact_with_no_mana_to_pay_loses_the_game() {
    let mut g = bolt_then_pact(&["Island", "Island"]);
    quiet_until(&mut g, pact_asks);
    assert!(!g.has_option(Choice::Yes), "can't pay: {}", opts(&g));
    assert!(g.has_option(Choice::Pass));
    g.pass();
    let o = g.outcome().unwrap();
    assert_eq!((o.winner, o.reason), (Some(1), EndReason::Effect));
}

#[test]
fn cr_605_3a_mana_abilities_pay_pacts_trigger() {
    // Four lands and a Lotus Petal: the Petal is sacrificed for mana during
    // the payment.
    let mut g = bolt_then_pact(&["Island", "Swamp", "Swamp", "Swamp", "Lotus Petal"]);
    quiet_until(&mut g, pact_asks);
    assert!(g.has_option(Choice::Yes), "{}", opts(&g));
    g.choose(Choice::Yes);
    quiet_until(&mut g, |g| g.stack().is_empty() && at_priority(g));
    assert!(!g.is_over(), "{}", log(&g));
    assert_eq!(g.count_in(0, "Lotus Petal", Zone::Graveyard), 1);
}

#[test]
fn cr_603_7_pact_cast_on_its_controllers_turn_waits_for_their_next_upkeep() {
    // P0's main phase, Pact on its own Brainstorm: not P1's upkeep (turn
    // 4), P0's (turn 5).
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Swamp", "Swamp", "Swamp", "Island"])
        .hand(0, &["Brainstorm", PACT])
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Brainstorm", None);
    let bs = g.card(0, "Brainstorm", Zone::Stack);
    let t = g.tgt(bs);
    cast(&mut g, 0, PACT, Some(t));
    pass_on(&mut g);
    assert_eq!(g.obj(bs).zone, Zone::Graveyard);
    quiet_until(&mut g, |g| pact_asks(g) || (g.turn().number == 5 && g.turn().step == Step::Draw));
    assert_eq!(g.kind(), Some(DecisionKind::YesNo));
    assert_eq!((g.turn().active, g.turn().number, g.turn().step), (0, 5, Step::Upkeep));
}

#[test]
fn cr_603_7_pact_cast_during_its_controllers_upkeep_waits_for_the_next_one() {
    // The upkeep had begun: "your next upkeep" is the one after it.
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Swamp", "Swamp", "Swamp", "Island"])
        .hand(0, &["Brainstorm", PACT])
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
        .at(Step::Upkeep, 0)
        .start();
    assert_eq!(g.turn().step, Step::Upkeep);
    cast(&mut g, 0, "Brainstorm", None);
    let bs = g.card(0, "Brainstorm", Zone::Stack);
    let t = g.tgt(bs);
    cast(&mut g, 0, PACT, Some(t));
    pass_on(&mut g);
    quiet_until(&mut g, |g| pact_asks(g) || (g.turn().number == 5 && g.turn().step == Step::Draw));
    assert_eq!(g.kind(), Some(DecisionKind::YesNo));
    assert_eq!((g.turn().active, g.turn().number, g.turn().step), (0, 5, Step::Upkeep));
}

#[test]
fn pact_whose_target_is_gone_doesnt_resolve_and_makes_no_trigger() {
    // ruling: 4c4f55a3
    // Two Pacts on one Bolt: the second resolves first and counters it; the
    // first's target is gone (CR 608.2b): one trigger, which P0 pays.
    let mut g = scen()
        .battlefield(0, FIVE)
        .hand(0, &[PACT, PACT])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
        .at(Step::Main1, 1)
        .start();
    cast(&mut g, 1, "Lightning Bolt", Some(Choice::Target(Target::Player(0))));
    g.pass();
    let bolt = g.card(1, "Lightning Bolt", Zone::Stack);
    let t = g.tgt(bolt);
    cast(&mut g, 0, PACT, Some(t));
    let t = g.tgt(bolt);
    cast(&mut g, 0, PACT, Some(t));
    pass_on(&mut g);
    assert!(g.stack().is_empty());
    assert_eq!(g.count_in(0, PACT, Zone::Graveyard), 2);
    quiet_until(&mut g, pact_asks);
    g.choose(Choice::Yes);
    quiet_until(&mut g, |g| g.is_over() || g.kind() == Some(DecisionKind::YesNo) || g.turn().step == Step::Main1);
    assert!(!g.is_over() && g.kind() != Some(DecisionKind::YesNo), "one trigger, paid\n{}", log(&g));
}

#[test]
fn two_pacts_are_two_triggers_each_paid_or_lost() {
    // Two Pacts on two Bolts: two triggers at the next upkeep, each its own
    // payment; five lands pay one, not both.
    let mut g = scen()
        .battlefield(0, FIVE)
        .hand(0, &[PACT, PACT])
        .battlefield(1, &["Mountain", "Mountain"])
        .hand(1, &["Lightning Bolt", "Lightning Bolt"])
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
        .at(Step::Main1, 1)
        .start();
    for _ in 0..2 {
        cast_with(&mut g, 1, "Lightning Bolt", Choice::Target(Target::Player(0)), false);
        g.pass();
        let bolt = g.card(1, "Lightning Bolt", Zone::Stack);
        let t = g.tgt(bolt);
    cast(&mut g, 0, PACT, Some(t));
        pass_on(&mut g);
        assert!(g.stack().is_empty());
    }
    assert_eq!(g.life(0), 20);
    quiet_until(&mut g, pact_asks);
    // The first: paid.
    g.choose(Choice::Yes);
    quiet_until(&mut g, pact_asks);
    assert!(!g.is_over());
    // The second: the lands are tapped; it can't be paid.
    assert!(!g.has_option(Choice::Yes), "{}", opts(&g));
    g.pass();
    assert_eq!(g.outcome().unwrap().winner, Some(1));
    // Ten lands pay both.
    let mut g = scen()
        .battlefield(0, &["Island"; 10])
        .hand(0, &[PACT, PACT])
        .battlefield(1, &["Mountain", "Mountain"])
        .hand(1, &["Lightning Bolt", "Lightning Bolt"])
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
        .at(Step::Main1, 1)
        .start();
    for _ in 0..2 {
        cast_with(&mut g, 1, "Lightning Bolt", Choice::Target(Target::Player(0)), false);
        g.pass();
        let bolt = g.card(1, "Lightning Bolt", Zone::Stack);
        let t = g.tgt(bolt);
    cast(&mut g, 0, PACT, Some(t));
        pass_on(&mut g);
    }
    quiet_until(&mut g, pact_asks);
    g.choose(Choice::Yes);
    quiet_until(&mut g, pact_asks);
    assert!(g.has_option(Choice::Yes), "{}", opts(&g));
    g.choose(Choice::Yes);
    quiet_until(&mut g, |g| g.is_over() || g.turn().step == Step::Main1);
    assert!(!g.is_over());
}

#[test]
fn pact_that_resolves_but_cant_counter_still_makes_its_trigger() {
    // ruling: 3e504e2e
    // Abrupt Decay can't be countered: Pact resolves with a legal target,
    // counters nothing, and its delayed trigger still comes.
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Swamp", "Swamp", "Swamp", "Lotus Petal"])
        .hand(0, &[PACT])
        .battlefield(1, &["Bayou", "Bayou"])
        .hand(1, &["Abrupt Decay"])
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
        .at(Step::Main1, 1)
        .start();
    let petal = g.card(0, "Lotus Petal", Zone::Battlefield);
    let t = g.tgt(petal);
    cast(&mut g, 1, "Abrupt Decay", Some(t));
    g.pass();
    let decay = g.card(1, "Abrupt Decay", Zone::Stack);
    let t = g.tgt(decay);
    cast(&mut g, 0, PACT, Some(t));
    pass_on(&mut g);
    assert!(g.stack().is_empty());
    assert_eq!(g.count_in(0, "Lotus Petal", Zone::Graveyard), 1, "Decay resolved\n{}", log(&g));
    quiet_until(&mut g, |g| pact_asks(g) || (g.turn().active == 0 && g.turn().step == Step::Draw));
    assert_eq!(g.kind(), Some(DecisionKind::YesNo), "the trigger still comes\n{}", log(&g));
}

#[test]
fn pact_countered_by_daze_makes_no_trigger_unless_daze_is_paid() {
    // ruling: 4c4f55a3
    for pay in [false, true] {
        let mut g = scen()
            .battlefield(0, &["Island", "Island", "Swamp", "Swamp", "Swamp", "Island"])
            .hand(0, &[PACT])
            .battlefield(1, &["Mountain", "Island"])
            .hand(1, &["Lightning Bolt", "Daze"])
            .library(0, &["Island"; 10])
            .library(1, &["Island"; 10])
            .at(Step::Main1, 1)
            .start();
        cast(&mut g, 1, "Lightning Bolt", Some(Choice::Target(Target::Player(0))));
        g.pass();
        let bolt = g.card(1, "Lightning Bolt", Zone::Stack);
        let t = g.tgt(bolt);
    cast(&mut g, 0, PACT, Some(t));
        g.pass();
        // P1 Dazes the Pact (returning its Island).
        let pact = g.card(0, PACT, Zone::Stack);
        let t = g.tgt(pact);
    cast_with(&mut g, 1, "Daze", t, true);
        assert_eq!(g.count_in(1, "Island", Zone::Hand), 1, "Daze's alternative cost\n{}", log(&g));
        // Daze resolves: P0 pays {1} or not.
        g.pass();
        g.pass();
        assert_eq!(g.kind(), Some(DecisionKind::YesNo), "{}", opts(&g));
        if pay {
            g.choose(Choice::Yes);
        } else {
            g.pass();
        }
        quiet_until(&mut g, |g| g.stack().is_empty() && at_priority(g));
        assert_eq!(g.obj(pact).zone, Zone::Graveyard);
        assert_eq!(g.life(0), if pay { 20 } else { 17 }, "{}", log(&g));
        quiet_until(&mut g, |g| pact_asks(g) || (g.turn().active == 0 && g.turn().step == Step::Draw));
        assert_eq!(g.kind() == Some(DecisionKind::YesNo), pay, "a trigger only if the Pact resolved\n{}", log(&g));
    }
}

#[test]
fn pact_countered_by_force_of_will_makes_no_trigger() {
    // ruling: 4c4f55a3
    let mut g = scen()
        .battlefield(0, FIVE)
        .hand(0, &[PACT])
        .battlefield(1, &["Mountain", "Island", "Island", "Island", "Island", "Island"])
        .hand(1, &["Lightning Bolt", "Force of Will"])
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
        .at(Step::Main1, 1)
        .start();
    cast(&mut g, 1, "Lightning Bolt", Some(Choice::Target(Target::Player(0))));
    g.pass();
    let bolt = g.card(1, "Lightning Bolt", Zone::Stack);
    let t = g.tgt(bolt);
    cast(&mut g, 0, PACT, Some(t));
    g.pass();
    let pact = g.card(0, PACT, Zone::Stack);
    let t = g.tgt(pact);
    cast_with(&mut g, 1, "Force of Will", t, false);
    quiet_until(&mut g, |g| g.stack().is_empty() && at_priority(g));
    assert_eq!(g.obj(pact).zone, Zone::Graveyard);
    assert_eq!(g.life(0), 17, "the Bolt resolved");
    quiet_until(&mut g, |g| pact_asks(g) || (g.turn().active == 0 && g.turn().step == Step::Draw));
    assert_ne!(g.kind(), Some(DecisionKind::YesNo), "no trigger\n{}", log(&g));
}

#[test]
fn pact_is_a_blue_card_force_of_will_can_exile() {
    // ruling: 0dfaac8c
    let mut g = scen()
        .hand(0, &["Force of Will", PACT])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
        .at(Step::Main1, 1)
        .start();
    cast(&mut g, 1, "Lightning Bolt", Some(Choice::Target(Target::Player(0))));
    g.pass();
    let bolt = g.card(1, "Lightning Bolt", Zone::Stack);
    let fow = g.card(0, "Force of Will", Zone::Hand);
    let pact = g.card(0, PACT, Zone::Hand);
    g.choose(Choice::Cast(fow));
    let mut exiled = false;
    while !at_priority(&g) {
        let d = g.pending_decision().unwrap();
        let c = match d.kind {
            DecisionKind::Target => g.tgt(bolt),
            DecisionKind::ExileCost => {
                assert!(g.has_option(Choice::Card(pact)), "{}", opts(&g));
                exiled = true;
                Choice::Card(pact)
            }
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    assert!(exiled, "{}", log(&g));
    assert_eq!(g.obj(pact).zone, Zone::Exile);
    pass_on(&mut g);
    assert_eq!(g.life(0), 19, "1 life for Force of Will, the Bolt countered");
}

// ---- Personal Tutor ------------------------------------------------------------------

/// The definitions a search decision offers.
fn offered_defs(g: &Game) -> Vec<u16> {
    let d = g.pending_decision().unwrap();
    assert_eq!(d.kind, DecisionKind::SearchLibrary);
    d.options
        .iter()
        .filter_map(|o| match o.choice {
            Choice::Def(x) => Some(x),
            _ => None,
        })
        .collect()
}

fn pt_scen(lib: &[&str]) -> Scenario {
    scen().battlefield(0, &["Island"]).hand(0, &[PT]).library(0, lib).library(1, &["Island"; 10]).at(Step::Main1, 0)
}

#[test]
fn cr_701_23b_personal_tutor_offers_the_sorcery_cards_and_may_find_none() {
    // ruling: ce207f9e
    let mut g = pt_scen(&["Island", "Brainstorm", "Ponder", DD, "Daze", FS, "Ponder"]).start();
    cast(&mut g, 0, PT, None);
    pass_on(&mut g);
    let mut want = vec![def("Ponder"), def(DD), def(FS)];
    want.sort();
    assert_eq!(offered_defs(&g), want, "sorceries only: {}", opts(&g));
    assert!(g.has_option(Choice::Pass), "a stated quality: may find nothing (CR 701.23b)");
    // Declining: nothing moves, the library is shuffled (nobody knows its
    // cards' places).
    g.pass();
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(g.player(0).library.len(), 7);
    assert!(g.player(0).library.iter().all(|&c| !g.knows(0, c) && !g.knows(1, c)));
    assert_eq!(g.count_in(0, PT, Zone::Graveyard), 1);
}

#[test]
fn personal_tutor_reveals_the_card_then_shuffles_and_puts_it_on_top() {
    let mut g = pt_scen(&["Island", "Brainstorm", "Ponder", DD, "Daze", FS]).start();
    cast(&mut g, 0, PT, None);
    pass_on(&mut g);
    let keys = g.choose(Choice::Def(def(DD)));
    let revealed: Vec<u16> = keys.iter().filter(|&&k| key_kind(k) == KEY_REVEAL).map(|&k| key_payload(k) as u16).collect();
    assert_eq!(revealed, vec![def(DD)]);
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    let top = *g.player(0).library.last().unwrap();
    assert_eq!(g.name(top), DD);
    assert!(g.knows(0, top) && g.knows(1, top));
    // The rest was shuffled: nobody knows where its cards are.
    assert!(g.player(0).library.iter().filter(|&&c| c != top).all(|&c| !g.knows(0, c) && !g.knows(1, c)));
    for v in 0..2 {
        for s in 0..20 {
            let mut d = g.clone();
            d.determinize(v, &mut Rng::new(s));
            assert_eq!(d.obj(top).def, def(DD), "the revealed top card stays for viewer {v}");
        }
    }
}

#[test]
fn personal_tutor_with_no_sorcery_finds_nothing() {
    let mut g = pt_scen(&["Island", "Brainstorm", "Daze"]).start();
    cast(&mut g, 0, PT, None);
    pass_on(&mut g);
    assert_eq!(g.kind(), Some(DecisionKind::SearchLibrary));
    assert_eq!(g.options_desc(), vec!["0=pass".to_string()]);
    g.pass();
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(g.player(0).library.len(), 3);
}

#[test]
fn personal_tutor_for_doomsday_then_draw_it() {
    // The deck's line: Personal Tutor puts Doomsday on top; the next draw is
    // that Doomsday (both players knew it).
    let mut g = pt_scen(&["Island", "Brainstorm", DD, "Daze", FS]).start();
    cast(&mut g, 0, PT, None);
    pass_on(&mut g);
    g.choose(Choice::Def(def(DD)));
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    let top = *g.player(0).library.last().unwrap();
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Main1 && g.turn().number == 5);
    assert_eq!(g.obj(top).zone, Zone::Hand);
    assert_eq!(g.name(top), DD);
    assert!(g.knows(1, top), "P1 knows the card P0 drew");
}

// ---- Flow State -------------------------------------------------------------------

fn fs_scen(lib: &[&str], gy: &[&str]) -> Scenario {
    scen().battlefield(0, &["Island", "Island"]).hand(0, &[FS]).library(0, lib).graveyard(0, gy).library(1, &["Island"; 10]).at(Step::Main1, 0)
}

/// The cards a pile decision offers.
fn pile_offered(g: &Game) -> Vec<&'static str> {
    let mut v: Vec<&'static str> = g
        .pending_decision()
        .unwrap()
        .options
        .iter()
        .filter_map(|o| match o.choice {
            Choice::Card(c) => Some(g.name(c)),
            _ => None,
        })
        .collect();
    v.sort();
    v
}

fn pick(g: &mut Game, name: &str) {
    let c = g
        .pending_decision()
        .unwrap()
        .options
        .iter()
        .find_map(|o| match o.choice {
            Choice::Card(c) if g.name(c) == name => Some(o.choice),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{name}: {}", opts(g)));
    g.choose(c);
}

#[test]
fn flow_state_puts_one_into_the_hand_and_two_on_the_bottom_in_any_order() {
    let mut g = fs_scen(&["Brainstorm", "Ponder", "Daze", "Island", "Swamp"], &[]).start();
    cast(&mut g, 0, FS, None);
    pass_on(&mut g);
    assert_eq!(g.kind(), Some(DecisionKind::PutInHand), "{}", opts(&g));
    assert_eq!(pile_offered(&g), vec!["Brainstorm", "Daze", "Ponder"]);
    assert!(!g.has_option(Choice::Pass), "one must be taken");
    pick(&mut g, "Ponder");
    // The rest on the bottom, the order chosen (the first chosen ends at the
    // bottom... each goes to the bottom in turn).
    assert_eq!(g.kind(), Some(DecisionKind::PutOnBottom));
    assert_eq!(pile_offered(&g), vec!["Brainstorm", "Daze"]);
    pick(&mut g, "Daze");
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(g.count_in(0, "Ponder", Zone::Hand), 1);
    assert_eq!(library(&g, 0), vec!["Island", "Swamp", "Daze", "Brainstorm"]);
    // The other order.
    let mut g = fs_scen(&["Brainstorm", "Ponder", "Daze", "Island", "Swamp"], &[]).start();
    cast(&mut g, 0, FS, None);
    pass_on(&mut g);
    pick(&mut g, "Ponder");
    pick(&mut g, "Brainstorm");
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(library(&g, 0), vec!["Island", "Swamp", "Brainstorm", "Daze"]);
}

#[test]
fn flow_state_the_caster_knows_the_bottom_cards_the_opponent_doesnt() {
    // As every look-and-bottom card (ENGINE.md 10): the looker put them there
    // and knows them; the other player saw cards go to the bottom.
    let mut g = fs_scen(&["Brainstorm", "Ponder", "Daze", "Island", "Swamp"], &[]).hand(0, &["Consider", "Dark Ritual"]).start();
    cast(&mut g, 0, FS, None);
    pass_on(&mut g);
    pick(&mut g, "Ponder");
    pick(&mut g, "Daze");
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    let bottom: Vec<CardIdx> = g.player(0).library[0..2].to_vec();
    assert!(bottom.iter().all(|&c| g.knows(0, c) && !g.knows(1, c)));
    for s in 0..10 {
        let mut d = g.clone();
        d.determinize(0, &mut Rng::new(s));
        assert_eq!(defs_in(&d, &bottom), defs_in(&g, &bottom));
    }
    let moved = (0..40).any(|s| {
        let mut d = g.clone();
        d.determinize(1, &mut Rng::new(s));
        defs_in(&d, &bottom) != defs_in(&g, &bottom)
    });
    assert!(moved, "P1 doesn't know them");
}

#[test]
fn flow_state_with_an_instant_and_a_sorcery_in_the_graveyard_takes_two() {
    let mut g = fs_scen(&["Brainstorm", "Ponder", "Daze", "Island"], &["Force of Will", DD]).start();
    cast(&mut g, 0, FS, None);
    pass_on(&mut g);
    pick(&mut g, "Brainstorm");
    assert_eq!(g.kind(), Some(DecisionKind::PutInHand), "a second card: {}", opts(&g));
    assert!(!g.has_option(Choice::Pass));
    pick(&mut g, "Daze");
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(g.player(0).hand.len(), 2);
    assert_eq!(library(&g, 0), vec!["Island", "Ponder"]);
}

#[test]
fn flow_state_needs_both_an_instant_and_a_sorcery_card() {
    // Two instants: one card. Flow State itself is on the stack as it
    // resolves, not in the graveyard (CR 608.2n puts it there after).
    for gy in [&["Force of Will", "Daze"][..], &[DD, "Ponder"][..], &["Force of Will"][..]] {
        let mut g = fs_scen(&["Brainstorm", "Ponder", "Daze", "Island"], gy).start();
        cast(&mut g, 0, FS, None);
        pass_on(&mut g);
        pick(&mut g, "Brainstorm");
        assert_eq!(g.kind(), Some(DecisionKind::PutOnBottom), "{gy:?}: one card only: {}", opts(&g));
    }
}

#[test]
fn flow_state_with_fewer_than_three_cards_looks_at_what_there_is() {
    // One card: it is taken (even with the condition, at most what there is).
    let mut g = fs_scen(&["Brainstorm"], &["Force of Will", DD]).start();
    cast(&mut g, 0, FS, None);
    pass_on(&mut g);
    assert_eq!(pile_offered(&g), vec!["Brainstorm"]);
    pick(&mut g, "Brainstorm");
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert!(g.player(0).library.is_empty());
    assert_eq!(g.count_in(0, "Brainstorm", Zone::Hand), 1);
    // Two cards and the condition: both (a set: picked in definition
    // order).
    let mut g = fs_scen(&["Brainstorm", "Daze"], &["Force of Will", DD]).start();
    cast(&mut g, 0, FS, None);
    pass_on(&mut g);
    assert_eq!(g.pending_decision().unwrap().options.len(), 1, "{}", opts(&g));
    let first = g.pending_decision().unwrap().options[0].choice;
    g.choose(first);
    assert_eq!(g.kind(), Some(DecisionKind::PutInHand), "the second: {}", opts(&g));
    let second = g.pending_decision().unwrap().options[0].choice;
    g.choose(second);
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert!(g.player(0).library.is_empty());
    assert_eq!(g.player(0).hand.len(), 2);
    // An empty library: nothing to look at; nothing asked.
    let mut g = fs_scen(&[], &[]).start();
    cast(&mut g, 0, FS, None);
    pass_on(&mut g);
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert!(g.stack().is_empty() && at_priority(&g));
    assert_eq!(g.count_in(0, FS, Zone::Graveyard), 1);
}

// ---- Interactions ----------------------------------------------------------------

#[test]
fn doomsday_then_thassas_oracle_wins_with_a_small_enough_library() {
    // A one-card pile: Thassa's Oracle's devotion to blue (2: its own {U}{U})
    // is at least the library's size, CR 104.2b (its Oracle text).
    let mut g = dd_scen(&["Island"], &[]).battlefield(0, &["Island", "Island"]).hand(0, &[ORACLE]).start();
    cast_doomsday(&mut g);
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(library(&g, 0), vec!["Island"]);
    cast(&mut g, 0, ORACLE, None);
    while !(g.is_over() || (g.stack().is_empty() && at_priority(&g))) {
        let c = if at_priority(&g) { Choice::Pass } else { g.pending_decision().unwrap().options[0].choice };
        g.choose(c);
    }
    let o = g.outcome().expect("the Oracle wins");
    assert_eq!((o.winner, o.reason), (Some(0), EndReason::Effect));
}

#[test]
fn doomsday_then_thassas_oracle_with_five_cards_looks_and_doesnt_win() {
    let mut g = dd_scen(LIB10, &[]).battlefield(0, &["Island", "Island"]).hand(0, &[ORACLE]).start();
    cast_doomsday(&mut g);
    answer_doomsday(&mut g, &[], &["Brainstorm", "Ponder", "Consider", "Daze", "Island"], &["Brainstorm", "Ponder", "Consider", "Daze", "Island"]);
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    cast(&mut g, 0, ORACLE, None);
    quiet_until(&mut g, |g| g.is_over() || (g.stack().is_empty() && at_priority(g)) || !matches!(g.kind(), Some(DecisionKind::Priority) | Some(DecisionKind::OrderTriggers)));
    while !(g.is_over() || (g.stack().is_empty() && at_priority(&g))) {
        let c = if at_priority(&g) { Choice::Pass } else { g.pending_decision().unwrap().options[0].choice };
        g.choose(c);
    }
    assert!(!g.is_over(), "devotion 2 < 5 cards");
    assert_eq!(g.player(0).library.len(), 5);
}

#[test]
fn doomsday_then_brainstorm_draws_three_of_the_pile_and_puts_two_back() {
    // P1 knew Force of Will was among the five; after the draw and the
    // private put-back, it may be in P0's hand or library.
    let mut g = dd_scen(LIB10, &["Force of Will"]).battlefield(0, &["Island"]).hand(0, &["Brainstorm"]).start();
    cast_doomsday(&mut g);
    let fow = g.card(0, "Force of Will", Zone::Graveyard);
    answer_doomsday(&mut g, &[fow], &[ORACLE, "Ponder", "Consider", "Daze"], &["Force of Will", ORACLE, "Ponder", "Consider", "Daze"]);
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    cast(&mut g, 0, "Brainstorm", None);
    pass_on(&mut g);
    // Draw three (Force of Will, Oracle, Ponder), put back two.
    assert_eq!(g.kind(), Some(DecisionKind::ChooseHandCard), "{}", opts(&g));
    let hand_fow = g.card(0, "Force of Will", Zone::Hand);
    let hand_ponder = g.card(0, "Ponder", Zone::Hand);
    g.choose(Choice::Card(hand_ponder));
    g.choose(Choice::Card(hand_fow));
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(library(&g, 0), vec!["Force of Will", "Ponder", "Consider", "Daze"]);
    assert!(g.player(0).library.iter().all(|&c| g.knows(0, c)), "P0 knows its library");
    let (mut lib, mut hand) = (false, false);
    for s in 0..60 {
        let mut d = g.clone();
        d.determinize(1, &mut Rng::new(s));
        d.check_knowledge().unwrap();
        lib |= defs_in(&d, &d.player(0).library.clone()).contains(&def("Force of Will"));
        hand |= defs_in(&d, &d.player(0).hand.clone()).contains(&def("Force of Will"));
    }
    assert!(lib && hand, "P1 can't tell where Force of Will went");
}

#[test]
fn doomsday_then_ponder_draws_the_chosen_card() {
    let mut g = dd_scen(LIB10, &[]).battlefield(0, &["Island"]).hand(0, &["Ponder"]).start();
    cast_doomsday(&mut g);
    answer_doomsday(&mut g, &[], &[ORACLE, "Brainstorm", "Consider", "Daze", "Island"], &["Daze", ORACLE, "Brainstorm", "Consider", "Island"]);
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    cast(&mut g, 0, "Ponder", None);
    pass_on(&mut g);
    // Ponder: the top three back in any order (Oracle on top), no shuffle,
    // draw.
    let mut guard = 0;
    while !(g.stack().is_empty() && at_priority(&g)) {
        guard += 1;
        assert!(guard < 50, "{}", opts(&g));
        match g.kind() {
            Some(DecisionKind::PutOnTop) => {
                // The last chosen ends on top: Brainstorm, Daze, then Oracle.
                let order = ["Brainstorm", "Daze", ORACLE];
                let n = g.pending_decision().unwrap().options.len();
                pick(&mut g, order[3 - n]);
            }
            Some(DecisionKind::YesNo) => {
                g.pass();
            }
            _ => {
                let c = if at_priority(&g) { Choice::Pass } else { g.pending_decision().unwrap().options[0].choice };
                g.choose(c);
            }
        }
    }
    assert_eq!(g.count_in(0, ORACLE, Zone::Hand), 1, "{}", log(&g));
    assert_eq!(library(&g, 0), vec!["Daze", "Brainstorm", "Consider", "Island"]);
}

#[test]
fn doomsday_then_consider_surveils_one_and_draws_the_next() {
    let mut g = dd_scen(LIB10, &[]).battlefield(0, &["Island"]).hand(0, &["Consider"]).start();
    cast_doomsday(&mut g);
    answer_doomsday(&mut g, &[], &[ORACLE, "Brainstorm", "Ponder", "Daze", "Island"], &["Island", ORACLE, "Brainstorm", "Ponder", "Daze"]);
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    cast(&mut g, 0, "Consider", None);
    pass_on(&mut g);
    // Surveil 1: the Island into the graveyard.
    assert_eq!(g.kind(), Some(DecisionKind::PutInGraveyard), "{}", opts(&g));
    pick(&mut g, "Island");
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(g.count_in(0, ORACLE, Zone::Hand), 1);
    assert_eq!(library(&g, 0), vec!["Brainstorm", "Ponder", "Daze"]);
}

#[test]
fn lions_eye_diamond_in_response_puts_the_hand_where_doomsday_finds_it() {
    // LED's mana ability with Doomsday on the stack: the hand is discarded,
    // and Doomsday's search of the graveyard finds those cards.
    let mut g = dd_scen(&["Island", "Swamp", "Daze"], &[]).battlefield(0, &["Lion's Eye Diamond"]).hand(0, &[ORACLE, "Brainstorm"]).start();
    cast(&mut g, 0, DD, None);
    let led = g.card(0, "Lion's Eye Diamond", Zone::Battlefield);
    let led_act = Choice::Ability(led, engine::db::AbilitySlot::Activated(0));
    assert!(g.has_option(led_act), "{}", opts(&g));
    g.choose(led_act);
    if g.kind() == Some(DecisionKind::ChooseMana) {
        g.choose(Choice::Number(2));
    }
    assert!(g.player(0).hand.is_empty());
    assert_eq!(g.count_in(0, ORACLE, Zone::Graveyard), 1);
    pass_on(&mut g);
    let oracle = g.card(0, ORACLE, Zone::Graveyard);
    let bs = g.card(0, "Brainstorm", Zone::Graveyard);
    // Five of three library cards and three graveyard cards (LED, the Oracle,
    // Brainstorm).
    answer_doomsday(&mut g, &[oracle, bs], &["Island", "Swamp", "Daze"], &[ORACLE, "Brainstorm", "Island", "Swamp", "Daze"]);
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(library(&g, 0), vec![ORACLE, "Brainstorm", "Island", "Swamp", "Daze"]);
    assert_eq!(g.count_in(0, "Lion's Eye Diamond", Zone::Exile), 1);
}

#[test]
fn doomsday_triggers_murktide_for_each_instant_and_sorcery_leaving_the_graveyard() {
    // CR 603.10a: each instant or sorcery card leaving the graveyard, to the
    // library or to exile.
    let mut g = dd_scen(LIB10, &["Brainstorm", "Ponder", "Island", "Daze"]).battlefield(0, &["Murktide Regent"]).start();
    let m = g.card(0, "Murktide Regent", Zone::Battlefield);
    cast_doomsday(&mut g);
    let bs = g.card(0, "Brainstorm", Zone::Graveyard);
    answer_doomsday(&mut g, &[bs], &[ORACLE, "Consider", "Dark Ritual", "Lotus Petal"], &[ORACLE, "Brainstorm", "Consider", "Dark Ritual", "Lotus Petal"]);
    quiet_until(&mut g, |g| g.stack().is_empty() && at_priority(g));
    // Brainstorm (to the library), Ponder and Daze (exiled): three; the
    // Island isn't an instant or sorcery.
    assert_eq!(g.counters_on(m, engine::db::Counter::P1P1), 3, "{}", log(&g));
}

#[test]
fn cr_106_4_pacts_trigger_paid_with_dark_ritual_mana_floated_in_response() {
    // With the trigger on the stack, Dark Ritual's {B}{B}{B} floats in the
    // pool (CR 106.4) and pays the {3}; two Islands pay {U}{U}.
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Swamp"])
        .hand(0, &[PACT, "Dark Ritual"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
        .at(Step::Main1, 1)
        .start();
    cast(&mut g, 1, "Lightning Bolt", Some(Choice::Target(Target::Player(0))));
    g.pass();
    let bolt = g.card(1, "Lightning Bolt", Zone::Stack);
    let t = g.tgt(bolt);
    cast(&mut g, 0, PACT, Some(t));
    pass_on(&mut g);
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Upkeep && at_priority(g) && !g.stack().is_empty());
    cast(&mut g, 0, "Dark Ritual", None);
    g.pass();
    g.pass();
    assert_eq!(g.player(0).pool.amt[2], 3, "{}", log(&g));
    // The Ritual resolved; the trigger next.
    pass_on(&mut g);
    assert_eq!(g.kind(), Some(DecisionKind::YesNo), "{}", log(&g));
    assert!(g.has_option(Choice::Yes), "{}", opts(&g));
    g.choose(Choice::Yes);
    quiet_until(&mut g, |g| g.stack().is_empty() && at_priority(g));
    assert!(!g.is_over(), "{}", log(&g));
    assert!(g.player(0).pool.amt.iter().all(|&m| m == 0));
}

#[test]
fn pyroblast_counters_pact_because_it_is_blue() {
    // ruling: 0dfaac8c
    // "Counter target spell if it's blue": Pact of Negation is (its colour
    // indicator); countered, it makes no trigger.
    let mut g = scen()
        .battlefield(0, FIVE)
        .hand(0, &[PACT])
        .battlefield(1, &["Mountain", "Mountain"])
        .hand(1, &["Lightning Bolt", "Pyroblast"])
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
        .at(Step::Main1, 1)
        .start();
    cast(&mut g, 1, "Lightning Bolt", Some(Choice::Target(Target::Player(0))));
    g.pass();
    let bolt = g.card(1, "Lightning Bolt", Zone::Stack);
    let t = g.tgt(bolt);
    cast(&mut g, 0, PACT, Some(t));
    g.pass();
    let pact = g.card(0, PACT, Zone::Stack);
    let t = g.tgt(pact);
    let pb = g.card(1, "Pyroblast", Zone::Hand);
    g.choose(Choice::Cast(pb));
    while !at_priority(&g) {
        let d = g.pending_decision().unwrap();
        let c = match d.kind {
            // The first mode: counter target spell if it's blue.
            DecisionKind::ChooseMode => d.options.iter().find(|o| o.choice != Choice::Pass).unwrap().choice,
            DecisionKind::Target => t,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    quiet_until(&mut g, |g| g.stack().is_empty() && at_priority(g));
    assert_eq!(g.obj(pact).zone, Zone::Graveyard);
    assert_eq!(g.life(0), 17, "the Bolt resolved\n{}", log(&g));
    quiet_until(&mut g, |g| pact_asks(g) || (g.turn().active == 0 && g.turn().step == Step::Draw));
    assert_ne!(g.kind(), Some(DecisionKind::YesNo), "no trigger\n{}", log(&g));
}

#[test]
fn doomsday_the_searchers_worlds_agree_on_what_it_knows() {
    // P0 doesn't know its library's order: two of its worlds deal it
    // differently. The same picks must leave the same view (which places it
    // knows, and what is there): the found cards wait on top in definition
    // order until P0 orders them (the fuzz's information-set check).
    let mut g = dd_scen(LIB10, &["Force of Will"]).start();
    cast_doomsday(&mut g);
    let fow = g.card(0, "Force of Will", Zone::Graveyard);
    // (The graveyard's one card: then the library.)
    g.choose(Choice::Card(fow));
    assert_eq!(g.kind(), Some(DecisionKind::SearchLibrary));
    let mut picks: Vec<u16> = ["Brainstorm", "Ponder", ORACLE, "Island"].map(def).to_vec();
    picks.sort();
    let mut views = BTreeSet::new();
    let mut orders = BTreeSet::new();
    for s in 0..12 {
        let mut d = g.clone();
        d.determinize(0, &mut Rng::new(s));
        orders.insert(defs_in(&d, &d.player(0).library.clone()));
        for &p in &picks {
            d.choose(Choice::Def(p));
        }
        assert_eq!(d.kind(), Some(DecisionKind::PutOnTop));
        views.insert(d.view_of(0));
    }
    assert!(orders.len() > 1, "P0's worlds deal its library differently");
    assert_eq!(views.len(), 1, "the same picks, the same view in every world of P0");
}
