//! M7b part 2 (ENGINE.md 25): rules of cards with more than one face, on
//! custom double-faced test cards (`tests/common`): transforming (CR
//! 701.27), the same object (712.18), what a face is where (712.8), a
//! creature that transforms into a planeswalker (306.5b, 704.5i).

mod common;

use common::db;
use engine::db::{AbilitySlot, Counter};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Zone};

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 10]).library(1, &["Forest"; 10])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn act(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(i))
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn pay(g: &mut Game) {
    g.drive_until(|g| at_priority(g) || g.kind() != Some(DecisionKind::PayMana));
    while g.kind() == Some(DecisionKind::PayMana) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
        g.drive_until(|g| at_priority(g) || g.kind() != Some(DecisionKind::PayMana));
    }
}

fn settle(g: &mut Game) {
    for _ in 0..300 {
        g.drive_until(|g| (g.stack().is_empty() && at_priority(g)) || !at_priority(g));
        if g.stack().is_empty() && at_priority(g) {
            return;
        }
        if at_priority(g) {
            g.pass();
            continue;
        }
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    panic!("settle: no progress");
}

const FLIPPER: &str = "Test Flipper";

#[test]
fn transform_keeps_the_object_and_what_applies_to_it_cr_712_18() {
    let mut g = scen()
        .battlefield(0, &[FLIPPER, "Forest", "Forest", "Mountain"])
        .counters(0, FLIPPER, Counter::P1P1, 1)
        .hand(0, &["Test Phyrexian Shock"])
        .start();
    let f = bf(&g, 0, FLIPPER);
    let r = g.obj_ref(f);
    assert_eq!(g.pt(f), (3, 3));
    // 2 damage marked on it first.
    let shock = g.card(0, "Test Phyrexian Shock", Zone::Hand);
    g.choose(Choice::Cast(shock));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(f));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayPhyrexian));
    if g.kind() == Some(DecisionKind::PayPhyrexian) {
        g.choose(Choice::Number(0));
    }
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(f).damage, 2);
    // Its counter and its damage stay.
    g.choose(act(f, 0));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj_ref(f), r, "not a new object");
    assert_eq!(g.name(f), "Test Flipped");
    assert_eq!(g.counters_on(f, Counter::P1P1), 1);
    assert_eq!(g.obj(f).damage, 2);
    assert_eq!(g.pt(f), (4, 4));
    // CR 712.8e: the front face's mana value; the colour indicator's colour.
    assert_eq!(g.def_of(f).mana_value(), 2);
    assert_eq!(g.colors_of(f), 1 << 4);
    // And back again (its back face transforms too).
    g.choose(act(f, 0));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.name(f), FLIPPER);
    assert_eq!(g.obj_ref(f), r);
}

#[test]
fn an_ability_doesnt_transform_a_permanent_that_transformed_since_cr_701_27f() {
    let mut g = scen().battlefield(0, &[FLIPPER, "Forest", "Forest"]).start();
    let f = bf(&g, 0, FLIPPER);
    // Two activations on the stack: the first to resolve transforms it; the
    // second (put on the stack before that) does nothing.
    g.choose(act(f, 0));
    pay(&mut g);
    g.choose(act(f, 0));
    pay(&mut g);
    assert_eq!(g.stack().len(), 2);
    settle(&mut g);
    assert_eq!(g.name(f), "Test Flipped", "transformed once, not back");
}

#[test]
fn transforming_a_single_faced_permanent_does_nothing_cr_701_27c() {
    let mut g = scen().battlefield(0, &["Test Not Flipper", "Forest"]).start();
    let f = bf(&g, 0, "Test Not Flipper");
    g.choose(act(f, 0));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.name(f), "Test Not Flipper");
}

#[test]
fn a_creature_transformed_into_a_planeswalker_has_no_loyalty_cr_306_5b_704_5i() {
    // Loyalty counters come as a planeswalker enters (CR 306.5b), not as a
    // permanent transforms: with 0 loyalty it is put into its owner's
    // graveyard (704.5i), as its front face (712.8a).
    let mut g = scen().battlefield(0, &["Test Walker Flip", "Island"]).start();
    let w = bf(&g, 0, "Test Walker Flip");
    g.choose(act(w, 0));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(w).zone, Zone::Graveyard);
    assert_eq!(g.name(w), "Test Walker Flip");
    assert!(!db().is_face(g.obj(w).def));
}

#[test]
fn a_dying_back_face_is_its_front_card_in_the_graveyard_cr_712_8a() {
    let mut g = scen()
        .battlefield(1, &[FLIPPER])
        .transformed(1, FLIPPER)
        .battlefield(0, &["Mountain"])
        .hand(0, &["Lightning Bolt"])
        .start();
    let f = bf(&g, 1, "Test Flipped");
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(f));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(f).zone, Zone::Graveyard);
    assert_eq!(g.name(f), FLIPPER);
    assert_eq!(g.def_of(f).power, Some(2));
}

#[test]
fn a_transform_is_public_and_keeps_the_hidden_state_cr_712_6() {
    // Transforming reveals nothing hidden: no chance key.
    let mut g = scen().battlefield(0, &[FLIPPER, "Forest"]).start();
    let f = bf(&g, 0, FLIPPER);
    g.choose(act(f, 0));
    pay(&mut g);
    g.pass();
    let keys = g.pass();
    assert!(keys.is_empty(), "{keys:?}");
    assert_eq!(g.name(f), "Test Flipped");
    for seed in 0..5 {
        let mut w = g.clone();
        w.determinize(1, &mut engine::rng::Rng::new(seed));
        assert_eq!(w.view_of(1), g.view_of(1));
        assert_eq!(w.obj(f).def, g.obj(f).def);
    }
}

#[test]
fn a_daybound_permanent_transforms_only_through_day_and_night_cr_702_145b() {
    let mut g = scen().battlefield(0, &["Test Werewolf", "Forest"]).day(Some(true)).start();
    let w = bf(&g, 0, "Test Werewolf");
    g.choose(act(w, 0));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.name(w), "Test Werewolf", "its transform ability does nothing");
    // Night (P0 casts nothing this turn, P1 nothing in its turn).
    for _ in 0..200 {
        if g.day() == Some(false) {
            break;
        }
        let c = match g.kind() {
            Some(DecisionKind::Priority) | Some(DecisionKind::DeclareAttackers) | Some(DecisionKind::DeclareBlockers) => Choice::Pass,
            _ => g.pending_decision().unwrap().options[0].choice,
        };
        g.choose(c);
    }
    assert_eq!(g.day(), Some(false));
    assert_eq!(g.name(w), "Test Nightwolf");
}
