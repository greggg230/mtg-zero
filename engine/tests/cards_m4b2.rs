//! Cube cards supported in M4b-2: hand reveals and chosen discards, modal
//! spells, tokens (library searches and looks: `rules_library.rs`).

mod common;

use common::db;
use engine::decision::act;
use engine::hidden::{key_kind, key_payload, key_player, HIDDEN, KEY_LOOK, KEY_REVEAL};
use engine::rng::Rng;
use engine::testkit::Scenario;
use engine::{Choice, DecisionKind, Game, PlayerId, Step, Zone};

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 8]).library(1, &["Forest"; 8])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn def(name: &str) -> u16 {
    db().id(name).unwrap()
}

/// Cast from hand, answering forced decisions, until the next real one.
fn cast(g: &mut Game, p: PlayerId, name: &str) {
    let c = g.card(p, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(at_priority);
}

/// Pass priority until something other than priority is asked or the stack
/// is empty; returns the chance keys.
fn resolve(g: &mut Game) -> Vec<u64> {
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty())
}

fn offered_names(g: &Game) -> Vec<String> {
    g.pending_decision()
        .unwrap()
        .options
        .iter()
        .map(|o| match o.choice {
            Choice::Card(c) => g.name(c).to_string(),
            Choice::Pass => "pass".to_string(),
            other => format!("{other:?}"),
        })
        .collect()
}

fn pick(g: &mut Game, name: &str) -> Vec<u64> {
    let c = g
        .pending_decision()
        .unwrap()
        .options
        .iter()
        .map(|o| o.choice)
        .find(|&c| matches!(c, Choice::Card(x) if g.name(x) == name))
        .unwrap_or_else(|| panic!("{name} not offered: {:?}", g.options_desc()));
    g.choose(c)
}

const P1_HAND: &[&str] = &["Test Bear", "Lightning Bolt", "Counterspell", "Forest", "Time Walk"];

#[test]
fn duress_reveals_the_hand_and_the_caster_picks_a_noncreature_nonland_card() {
    let mut g = scen().battlefield(0, &["Swamp"]).hand(0, &["Duress"]).hand(1, P1_HAND).at(Step::Main1, 0).start();
    // "Target opponent": P0 itself is not a legal target, so the single
    // target is chosen without a decision.
    cast(&mut g, 0, "Duress");
    let keys = resolve(&mut g);
    // Every card of the hand is revealed to everyone, in definition order.
    let mut want: Vec<u16> = P1_HAND.iter().map(|n| def(n)).collect();
    want.sort();
    let revealed: Vec<u16> = keys
        .iter()
        .filter(|&&k| key_kind(k) == KEY_REVEAL && key_player(k) == 1)
        .map(|&k| key_payload(k) as u16)
        .collect();
    assert_eq!(revealed, want);
    assert!(g.player(1).hand.iter().all(|&c| g.knows(0, c)));
    assert_eq!((g.kind(), g.to_move()), (Some(DecisionKind::ChooseDiscard), Some(0)));
    let mut names = offered_names(&g);
    names.sort();
    assert_eq!(names, vec!["Counterspell", "Lightning Bolt", "Time Walk"]);
    // Slots: the card's place in that hand sorted by definition.
    let hand = g.hand_sorted(1);
    for o in &g.pending_decision().unwrap().options {
        let Choice::Card(c) = o.choice else { unreachable!() };
        assert_eq!(o.action, act::LIBRARY + hand.iter().position(|&x| x == c).unwrap() as u16);
    }
    pick(&mut g, "Time Walk");
    assert_eq!(g.count_in(1, "Time Walk", Zone::Graveyard), 1);
    // The rest stays known to P0 until it changes.
    for s in 0..10 {
        let mut d = g.clone();
        d.determinize(0, &mut Rng::new(s));
        let mut h: Vec<&str> = d.player(1).hand.iter().map(|&c| d.name(c)).collect();
        h.sort();
        assert_eq!(h, vec!["Counterspell", "Forest", "Lightning Bolt", "Test Bear"]);
    }
    // A card P1 draws later is not known to P0.
    g.drive_until(|g| g.turn().number == 4 && g.turn().step == Step::Main1);
    let drawn = g.player(1).hand.iter().copied().find(|&c| !g.knows(0, c)).expect("a new card");
    assert_eq!(g.player(1).hand.len(), 5);
    assert!(g.knows(1, drawn));
}

#[test]
fn duress_with_nothing_to_take_only_reveals() {
    let mut g = scen().battlefield(0, &["Swamp"]).hand(0, &["Duress"]).hand(1, &["Test Bear", "Forest"]).at(Step::Main1, 0).start();
    cast(&mut g, 0, "Duress");
    resolve(&mut g);
    assert!(g.stack().is_empty() && at_priority(&g));
    assert_eq!(g.player(1).hand.len(), 2);
    assert!(g.player(1).hand.iter().all(|&c| g.knows(0, c)));
}

#[test]
fn thoughtseize_and_inquisition_filter_and_cost_life() {
    let mut g = scen().battlefield(0, &["Swamp"]).hand(0, &["Thoughtseize"]).hand(1, P1_HAND).at(Step::Main1, 0).start();
    let ts = g.card(0, "Thoughtseize", Zone::Hand);
    g.choose(Choice::Cast(ts));
    // "Target player": either.
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    g.choose(Choice::Target(engine::Target::Player(1)));
    g.drive_until(at_priority);
    resolve(&mut g);
    let mut names = offered_names(&g);
    names.sort();
    assert_eq!(names, vec!["Counterspell", "Lightning Bolt", "Test Bear", "Time Walk"], "nonland");
    pick(&mut g, "Test Bear");
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(g.life(0), 18);

    let mut g = scen().battlefield(0, &["Swamp"]).hand(0, &["Inquisition of Kozilek"]).hand(1, P1_HAND).at(Step::Main1, 0).start();
    let iok = g.card(0, "Inquisition of Kozilek", Zone::Hand);
    g.choose(Choice::Cast(iok));
    g.choose(Choice::Target(engine::Target::Player(1)));
    g.drive_until(at_priority);
    resolve(&mut g);
    let mut names = offered_names(&g);
    names.sort();
    // Mana value 3 or less: Time Walk is 2, all are.
    assert_eq!(names, vec!["Counterspell", "Lightning Bolt", "Test Bear", "Time Walk"]);
}

#[test]
fn gitaxian_probe_lets_only_its_caster_see_the_hand() {
    let mut g = scen().hand(0, &["Gitaxian Probe"]).hand(1, P1_HAND).at(Step::Main1, 0).start();
    let probe = g.card(0, "Gitaxian Probe", Zone::Hand);
    g.choose(Choice::Cast(probe));
    g.choose(Choice::Target(engine::Target::Player(1)));
    // {U/P} with no blue source: two life.
    g.drive_until(at_priority);
    assert_eq!(g.life(0), 18);
    let mut o = g.clone();
    let keys = resolve(&mut g);
    let looks: Vec<u64> = keys.iter().copied().filter(|&k| key_kind(k) == KEY_LOOK).collect();
    assert_eq!(looks.len(), 5);
    assert!(looks.iter().all(|&k| key_player(k) == 0));
    assert!(keys.iter().all(|&k| key_kind(k) != KEY_REVEAL || key_player(k) != 1), "nothing of P1's is revealed");
    assert!(g.player(1).hand.iter().all(|&c| g.knows(0, c)));
    // In P1's search the looks are keyed as hidden.
    o.determinize(1, &mut Rng::new(1));
    let keys = resolve(&mut o);
    assert!(keys.iter().filter(|&&k| key_kind(k) == KEY_LOOK).all(|&k| key_payload(k) == HIDDEN));
    // P0 drew a card off the Probe.
    assert_eq!(g.player(0).hand.len(), 1);
}

#[test]
fn a_seen_hand_partly_put_back_is_known_only_as_its_contents() {
    // P0 Probes P1, then P1 Brainstorms: P0 knows the old cards are in P1's
    // hand or the two put back, not which.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .hand(0, &["Gitaxian Probe"])
        .battlefield(1, &["Island"])
        .hand(1, &["Brainstorm", "Lightning Bolt", "Time Walk"])
        .library(0, &["Island"; 8])
        .library(1, &["Forest", "Forest", "Forest", "Plains", "Plains", "Plains", "Plains", "Plains"])
        .at(Step::Main1, 0)
        .start();
    let probe = g.card(0, "Gitaxian Probe", Zone::Hand);
    g.choose(Choice::Cast(probe));
    g.choose(Choice::Target(engine::Target::Player(1)));
    // P1's turn: Brainstorm.
    g.drive_until(|g| g.turn().number == 4 && g.turn().step == Step::Main1);
    let bs = g.card(1, "Brainstorm", Zone::Hand);
    g.choose(Choice::Cast(bs));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseHandCard));
    pick(&mut g, "Time Walk");
    pick(&mut g, "Forest");
    g.check_invariants().unwrap();
    let k = g.clouds().iter().find(|k| k.viewer == 0).expect("P0's partial knowledge");
    let mut want = vec![def("Lightning Bolt"), def("Time Walk")];
    want.sort();
    assert_eq!(k.defs, want);
    let tw = g.card(1, "Time Walk", Zone::Library);
    let (mut lib, mut hand) = (0, 0);
    for s in 0..100 {
        let mut d = g.clone();
        d.determinize(0, &mut Rng::new(s));
        d.check_invariants().unwrap();
        let top2: Vec<_> = d.player(1).library.iter().rev().take(2).copied().collect();
        let tw_top = top2.iter().any(|&c| d.obj(c).def == def("Time Walk"));
        let tw_hand = d.player(1).hand.iter().any(|&c| d.obj(c).def == def("Time Walk"));
        assert!(tw_top || tw_hand, "seed {s}");
        lib += tw_top as u32;
        hand += tw_hand as u32;
    }
    assert!(lib > 10 && hand > 10, "top {lib}, hand {hand}");
    assert!(!g.knows(0, tw));
}
