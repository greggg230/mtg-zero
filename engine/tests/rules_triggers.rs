//! Triggered abilities (CR 603).

mod common;

use common::db;
use engine::game::StackKind;
use engine::testkit::Scenario;
use engine::{Choice, DecisionKind, Step, Target, Zone};

const LIB: &[&str] = &["Island"; 5];

fn scen() -> Scenario {
    Scenario::new(db()).library(0, LIB).library(1, LIB)
}

#[test]
fn cr_603_3_etb_trigger_goes_on_the_stack_before_priority() {
    let mut g = scen()
        .battlefield(0, &["Island", "Swamp", "Mountain"])
        .hand(0, &["Baleful Strix", "Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let strix = g.card(0, "Baleful Strix", Zone::Hand);
    g.choose(Choice::Cast(strix));
    g.pass(); // P0 (could Bolt)
    // P1 passed automatically; Strix resolved; its ETB trigger is on the
    // stack when P0 next receives priority (117.3b, 603.3).
    assert_eq!(g.kind(), Some(DecisionKind::Priority));
    assert_eq!(g.to_move(), Some(0));
    assert_eq!(g.stack().len(), 1);
    assert!(matches!(g.stack()[0].kind, StackKind::Ability { .. }));
    let hand = g.player(0).hand.len();
    g.pass();
    assert_eq!(g.player(0).hand.len(), hand + 1, "drew a card");
}

#[test]
fn cr_603_3d_targets_chosen_as_the_trigger_is_put_on_the_stack() {
    // ruling: 9ef5a7f0
    // Flametongue Kavu must target a creature: with no other, itself.
    let mut g = scen().battlefield(0, &["Mountain"; 4]).hand(0, &["Flametongue Kavu"]).at(Step::Main1, 0).start();
    let ftk = g.card(0, "Flametongue Kavu", Zone::Hand);
    g.choose(Choice::Cast(ftk));
    g.pass_until(|g| g.stack().is_empty());
    assert_eq!(g.count_in(0, "Flametongue Kavu", Zone::Graveyard), 1);

    let mut g = scen()
        .battlefield(0, &["Mountain"; 4])
        .hand(0, &["Flametongue Kavu"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let ftk = g.card(0, "Flametongue Kavu", Zone::Hand);
    g.choose(Choice::Cast(ftk));
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    assert_eq!(g.pending_decision().unwrap().options.len(), 2);
    let bear = g.card(1, "Test Bear", Zone::Battlefield);
    let t = g.tgt(bear);
    g.choose(t);
    g.pass_until(|g| g.stack().is_empty());
    assert_eq!(g.count_in(1, "Test Bear", Zone::Graveyard), 1);
    assert_eq!(g.count_in(0, "Flametongue Kavu", Zone::Battlefield), 1);
}

#[test]
fn cr_603_3d_trigger_without_a_legal_target_is_removed() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Test Dies Shock"])
        .hand(0, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let shock = g.card(0, "Test Dies Shock", Zone::Battlefield);
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    let t = g.tgt(shock);
    g.choose(t);
    g.pass_until(|g| g.stack().is_empty());
    assert_eq!(g.obj(shock).zone, Zone::Graveyard);
    assert!(g.stack().is_empty());
    assert!(g.waiting_triggers().is_empty());
}

#[test]
fn cr_603_3b_apnap_order_and_cr_603_10a_dies_triggers_look_back() {
    // P0 holds a Bolt so that its priority is a real decision.
    let mut g = scen()
        .battlefield(0, &["Test Dies Draw", "Mountain"])
        .hand(0, &["Lightning Bolt"])
        .battlefield(1, &["Test Dies Ping"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let a = g.card(0, "Test Dies Draw", Zone::Battlefield);
    g.choose(Choice::Card(a));
    g.pass_until(|g| g.kind() == Some(DecisionKind::DeclareBlockers));
    g.choose(Choice::Card(a)); // P1 blocks: both die
    g.pass_until(|_| false); // to the next non-priority decision
    // AP's trigger went on the stack first; P1 now targets with theirs.
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    assert_eq!(g.to_move(), Some(1));
    assert_eq!(g.stack().len(), 2);
    assert_eq!((g.stack()[0].controller, g.stack()[1].controller), (0, 1));
    let hand = g.player(0).hand.len();
    g.choose(Choice::Target(Target::Player(0)));
    assert_eq!(g.to_move(), Some(0));
    // The non-active player's trigger (on top) resolves first.
    g.pass();
    assert_eq!(g.stack().len(), 1);
    assert_eq!(g.life(0), 19);
    assert_eq!(g.player(0).hand.len(), hand);
    g.pass();
    assert_eq!(g.player(0).hand.len(), hand + 1);
}

#[test]
fn cr_603_3b_controller_orders_simultaneous_triggers() {
    let mut g = scen()
        .battlefield(0, &["Test Dies Draw", "Test Dies Ping", "Mountain"])
        .hand(0, &["Lightning Bolt"])
        .battlefield(1, &["Test Bear", "Test Bear"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let dd = g.card(0, "Test Dies Draw", Zone::Battlefield);
    let dp = g.card(0, "Test Dies Ping", Zone::Battlefield);
    g.choose(Choice::Card(dd));
    g.choose(Choice::Card(dp));
    g.pass_until(|g| g.kind() == Some(DecisionKind::DeclareBlockers));
    g.choose(Choice::Card(dd)); // first Bear blocks Dies Draw
    g.choose(Choice::Card(dp)); // second Bear blocks Dies Ping
    g.pass_until(|_| false);
    assert_eq!(g.kind(), Some(DecisionKind::OrderTriggers));
    assert_eq!(g.pending_decision().unwrap().options.len(), 2);
    // Waiting triggers are listed in event order (Dies Draw died first).
    g.choose(Choice::Trigger(1)); // the ping goes on the stack first
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    g.choose(Choice::Target(Target::Player(1)));
    assert_eq!(g.stack().len(), 2);
    let top = &g.stack()[1];
    let StackKind::Ability { ability, .. } = top.kind else { panic!() };
    assert_eq!(g.db().def(ability.def).name, "Test Dies Draw");
}
