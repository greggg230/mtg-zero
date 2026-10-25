//! M7a: a token copy of the card itself (CR 707.9b), reveal until a
//! creature that enters attacking (508.4), "whenever that creature attacks
//! the monarch".

mod common;

use common::db;
use engine::db::Kw;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};

fn base() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false)
}

fn scen() -> Scenario {
    base().library(0, &["Island"; 10]).library(1, &["Forest"; 10])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn pay(g: &mut Game) {
    while g.kind() == Some(DecisionKind::PayMana) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
        g.drive_until(|g| at_priority(g) || g.kind() != Some(DecisionKind::PayMana));
    }
}

fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..300 {
        g.drive_until(|g| (g.stack().is_empty() && at_priority(g)) || !at_priority(g) || g.is_over());
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
    panic!("resolve_with: no progress");
}

fn quiet_until(g: &mut Game, stop: impl Fn(&Game) -> bool) {
    for _ in 0..10_000 {
        if stop(g) {
            return;
        }
        let d = g.pending_decision().expect("game running");
        let c = match d.kind {
            DecisionKind::Priority | DecisionKind::DeclareAttackers | DecisionKind::DeclareBlockers => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    panic!("quiet_until: no progress");
}

#[test]
fn cr_707_vaultborn_tyrant_dies_and_leaves_an_artifact_token_copy_once() {
    let mut g = scen()
        .battlefield(0, &["Vaultborn Tyrant"])
        .battlefield(1, &["Swamp", "Swamp", "Swamp", "Swamp", "Swamp", "Swamp", "Swamp", "Swamp"])
        .hand(1, &["Snuff Out", "Snuff Out"])
        .at(Step::Main1, 1)
        .start();
    let vt = bf(&g, 0, "Vaultborn Tyrant");
    let so = g.card(1, "Snuff Out", Zone::Hand);
    // (Snuff Out can't target a black creature: Vaultborn is green.)
    g.choose(Choice::Cast(so));
    g.drive_until(|g| !at_priority(g) || g.stack().len() == 1);
    if g.kind() == Some(DecisionKind::ChooseAltCost) {
        g.choose(Choice::Number(0));
    }
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(g.tgt(vt)),
        _ => None,
    });
    assert_eq!(g.obj(vt).zone, Zone::Graveyard);
    let copy = bf(&g, 0, "Vaultborn Tyrant");
    // (A token copy: the card's copiable values, CR 707.2, with the
    // exception "it's an artifact", 707.9b; ENGINE.md 26.)
    assert!(g.is_token(copy));
    assert!(g.is_type(copy, engine::db::types::ARTIFACT));
    assert!(g.is_copy(copy));
    assert_eq!(g.pt(copy), (6, 6));
    assert!(g.has_kw(copy, Kw::Trample));
    // The token dies: "if it's not a token": no further copy.
    let so2 = g.card(1, "Snuff Out", Zone::Hand);
    g.choose(Choice::Cast(so2));
    g.drive_until(|g| !at_priority(g) || g.stack().len() == 1);
    if g.kind() == Some(DecisionKind::ChooseAltCost) {
        g.choose(Choice::Number(0));
    }
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(g.tgt(copy)),
        _ => None,
    });
    assert_eq!(g.count_in(0, "Vaultborn Tyrant", Zone::Battlefield), 0);
}

#[test]
fn raph_and_mikey_reveal_a_creature_that_enters_attacking() {
    let mut g = base()
        .library(0, &["Island", "Lightning Bolt", "Test Giant", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Raph & Mikey, Troublemakers"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let rm = bf(&g, 0, "Raph & Mikey, Troublemakers");
    g.choose(Choice::Card(rm));
    g.choose(Choice::Pass);
    resolve_with(&mut g, |_| None);
    let giant = bf(&g, 0, "Test Giant");
    assert!(g.obj(giant).attacking && g.obj(giant).tapped);
    assert_eq!(g.player(0).library.len(), 3, "Island and Bolt on the bottom");
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert_eq!(g.life(1), 8, "7 + 5");
}

#[test]
fn okoye_the_countered_creature_gains_double_strike_attacking_the_monarch() {
    let mut g = scen()
        .battlefield(0, &["Okoye, Mighty and Adored", "Test Hasty", "Test Bear"])
        .battlefield(1, &["Plains", "Plains", "Plains", "Plains"])
        .hand(1, &["Palace Jailer"])
        .at(Step::Main1, 1)
        .start();
    let hasty = bf(&g, 0, "Test Hasty");
    let bear = bf(&g, 0, "Test Bear");
    // P1 becomes the monarch (and exiles the Bear).
    let pj = g.card(1, "Palace Jailer", Zone::Hand);
    g.choose(Choice::Cast(pj));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
    pay(&mut g);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(g.tgt(bear)) => Some(g.tgt(bear)),
        _ => None,
    });
    assert_eq!(g.obj(bear).zone, Zone::Exile);
    // P0's turn: Okoye's counter on the hasty creature, which attacks the
    // monarch: double strike and trample (a delayed triggered ability on
    // the stack as it attacks, CR 603.7).
    quiet_until(&mut g, |g| g.turn().active == 0 && g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(hasty));
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(hasty));
    g.choose(Choice::Pass);
    resolve_with(&mut g, |_| None);
    assert!(g.has_kw(hasty, Kw::DoubleStrike) && g.has_kw(hasty, Kw::Trample));
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert_eq!(g.life(1), 14, "3 first-strike and 3 regular damage");
    assert_eq!(g.obj(bear).zone, Zone::Battlefield, "P0 took the monarchy: the Bear is back");
}
