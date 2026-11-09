//! W6's Mm'menon, the Right Hand (ENGINE.md 99.16): "Artifacts you control
//! have '{T}: Add {U}. Spend this mana only to cast a spell from anywhere
//! other than your hand.'" (CR 613.1f, 605.1a, 106.6), a payment source and
//! a mana ability with priority through B2's granted sources (100.4.5).

mod eoe_w6_kit;

use engine::{Choice, DecisionKind, Game, Step, Zone};
use eoe_w6_kit::*;

fn top(g: &Game, p: u8) -> engine::CardIdx {
    *g.player(p).library.last().expect("a library")
}

/// "Artifacts you control have '{T}: Add {U}. Spend this mana only to cast a
/// spell from anywhere other than your hand.'" With Mm'menon and a Nutrient
/// Block and no land, the artifact spell on top of the library (Mm'menon's
/// own permission, CR 601.3) can be cast with the Block's {U}; the Nutrient
/// Block in hand can't (that mana can't pay for it, 106.6).
#[test]
fn cr_613_1f_your_artifacts_tap_for_blue_to_cast_from_outside_your_hand() {
    let mut g = scen()
        .battlefield(0, &["Mm'menon, the Right Hand", "Nutrient Block"])
        .hand(0, &["Nutrient Block"])
        .library(0, &["Nutrient Block", "Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    let on_top = top(&g, 0);
    let in_hand = g.card(0, "Nutrient Block", Zone::Hand);
    let source = bf(&g, 0, "Nutrient Block");
    assert!(g.has_option(Choice::Cast(on_top)), "{:?}", g.options_desc());
    assert!(!g.has_option(Choice::Cast(in_hand)), "{:?}", g.options_desc());
    start_with(&mut g, Choice::Cast(on_top), |_| None);
    settle(&mut g);
    assert_eq!(g.obj(on_top).zone, Zone::Battlefield, "{}", log(&g));
    assert!(g.obj(source).tapped);
}

/// Only artifacts Mm'menon's controller controls, and only while Mm'menon is
/// on the battlefield (CR 613.1f): with Mm'menon and the opponent's Nutrient
/// Block, or with a Nutrient Block and no Mm'menon, its player has no mana
/// ability to activate (CR 605.3a: no `YES` at priority), and Mm'menon (a
/// creature) has none either.
#[test]
fn cr_613_1f_only_your_artifacts_and_only_with_mmmenon() {
    let g = scen()
        .battlefield(0, &["Mm'menon, the Right Hand"])
        .battlefield(1, &["Nutrient Block"])
        .library(0, &["Nutrient Block", "Island"])
        .at(Step::Main1, 0)
        .start();
    assert!(!g.has_option(Choice::Yes), "{:?}", g.options_desc());
    assert!(!g.has_option(Choice::Cast(top(&g, 0))), "{:?}", g.options_desc());
    let g = scen().battlefield(0, &["Nutrient Block"]).at(Step::Main1, 0).start();
    assert!(!g.has_option(Choice::Yes), "{:?}", g.options_desc());
}

/// The granted ability is a mana ability (CR 605.1a), activated with
/// priority (605.3a): the {U} floats with its restriction (106.6), paying
/// for the artifact on top of the library and not for the one in hand.
#[test]
fn cr_605_3a_the_granted_ability_floats_restricted_blue() {
    let mut g = scen()
        .battlefield(0, &["Mm'menon, the Right Hand", "Nutrient Block"])
        .hand(0, &["Nutrient Block"])
        .library(0, &["Nutrient Block", "Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    let source = bf(&g, 0, "Nutrient Block");
    let on_top = top(&g, 0);
    let in_hand = g.card(0, "Nutrient Block", Zone::Hand);
    g.choose(Choice::Yes);
    assert_eq!(g.kind(), Some(DecisionKind::ActivateMana), "{:?}", g.options_desc());
    let srcs: Vec<Choice> = g.pending_decision().unwrap().options.iter().map(|o| o.choice).collect();
    assert_eq!(srcs, vec![Choice::Card(source)], "only the Block has a mana ability");
    g.choose(Choice::Card(source));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ActivateMana) || at_priority(g));
    if g.kind() == Some(DecisionKind::ActivateMana) {
        g.choose(Choice::Pass);
    }
    assert!(at_priority(&g), "{}", log(&g));
    assert!(g.obj(source).tapped);
    assert_eq!(g.player(0).pool.total(), 1, "{:?}", g.player(0).pool);
    assert!(!g.has_option(Choice::Cast(in_hand)), "{:?}", g.options_desc());
    assert!(g.has_option(Choice::Cast(on_top)), "{:?}", g.options_desc());
    start_with(&mut g, Choice::Cast(on_top), |_| None);
    settle(&mut g);
    assert_eq!(g.obj(on_top).zone, Zone::Battlefield, "{}", log(&g));
    assert_eq!(g.player(0).pool.total(), 0);
}
