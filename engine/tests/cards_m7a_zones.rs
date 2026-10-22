//! M7a: cards exiled with a permanent (CR 607.2a, 610.3), cost
//! reductions (601.2f), and later zone mechanics.

mod common;

use common::db;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 10]).library(1, &["Forest"; 10])
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
        g.drive_until(at_priority);
    }
}

fn cast(g: &mut Game, p: PlayerId, name: &str) {
    let c = g.card(p, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(at_priority);
    pay(g);
}

/// Run the stack down; `answer` answers what is asked (None: the first
/// option).
fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..200 {
        g.drive_until(|g| (g.stack().is_empty() && at_priority(g)) || !at_priority(g));
        if g.stack().is_empty() && at_priority(g) {
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

#[test]
fn cr_610_3_portable_hole_exiles_until_it_leaves_then_the_permanent_returns() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Mountain"])
        .hand(0, &["Portable Hole"])
        .battlefield(1, &["Sol Ring", "Mountain", "Mountain"])
        .hand(1, &["Abrade"])
        .at(Step::Main1, 0)
        .start();
    let ring = bf(&g, 1, "Sol Ring");
    cast(&mut g, 0, "Portable Hole");
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(ring).zone, Zone::Exile);
    // P1 destroys the Hole (Abrade: destroy target artifact).
    let hole = bf(&g, 0, "Portable Hole");
    g.pass();
    let abrade = g.card(1, "Abrade", Zone::Hand);
    g.choose(Choice::Cast(abrade));
    g.drive_until(|g| !at_priority(g) || g.stack().len() == 1);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => Some(Choice::Number(1)),
        Some(DecisionKind::Target) if g.has_option(g.tgt(hole)) => Some(g.tgt(hole)),
        _ => None,
    });
    assert_eq!(g.obj(hole).zone, Zone::Graveyard, "{}", g.log_lines().join("\n"));
    let ring = g.card(1, "Sol Ring", Zone::Battlefield);
    assert_eq!(g.obj(ring).controller, 1, "back under its owner's control");
}

#[test]
fn cr_610_3c_deep_cavern_bat_gone_before_its_trigger_resolves_exiles_nothing() {
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp"])
        .hand(0, &["Deep-Cavern Bat"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt", "Counterspell"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Deep-Cavern Bat");
    // The creature spell resolves; its trigger goes on the stack.
    g.pass();
    g.pass();
    g.drive_until(|g| at_priority(g) && !g.stack().is_empty());
    let bat = bf(&g, 0, "Deep-Cavern Bat");
    // P1 kills the Bat in response.
    g.pass();
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || at_priority(g));
    if g.kind() == Some(DecisionKind::Target) {
        g.choose(g.tgt(bat));
    }
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(bat).zone, Zone::Graveyard);
    assert_eq!(g.count_in(1, "Counterspell", Zone::Hand), 1, "not exiled (610.3c)");
}

#[test]
fn tidehollow_sculler_exiles_from_the_revealed_hand_and_its_leave_trigger_returns_it() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Swamp"])
        .hand(0, &["Tidehollow Sculler"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt", "Lightning Bolt", "Forest"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Tidehollow Sculler");
    resolve_with(&mut g, |_| None);
    assert_eq!(g.count_in(1, "Lightning Bolt", Zone::Exile), 1);
    assert_eq!(g.count_in(1, "Forest", Zone::Hand), 1, "a nonland card");
    let sculler = bf(&g, 0, "Tidehollow Sculler");
    // P1 bolts it: its leave trigger returns the exiled card to hand.
    g.pass();
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || at_priority(g));
    if g.kind() == Some(DecisionKind::Target) {
        g.choose(g.tgt(sculler));
    }
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(sculler).zone, Zone::Graveyard);
    assert_eq!(g.count_in(1, "Lightning Bolt", Zone::Hand), 1, "returned");
    assert_eq!(g.count_in(1, "Lightning Bolt", Zone::Exile), 0);
}

#[test]
fn cr_601_2f_leyline_binding_costs_one_less_per_basic_land_type() {
    let g = scen()
        .battlefield(0, &["Plains", "Island", "Swamp", "Mountain", "Forest"])
        .hand(0, &["Leyline Binding"])
        .battlefield(1, &["Sol Ring"])
        .at(Step::Main1, 0)
        .start();
    // {5}{W} - 5 = {W}: castable with... the five lands (one is enough).
    let lb = g.card(0, "Leyline Binding", Zone::Hand);
    assert!(g.has_option(Choice::Cast(lb)));
    let g = scen()
        .battlefield(0, &["Plains", "Plains"])
        .hand(0, &["Leyline Binding"])
        .battlefield(1, &["Sol Ring"])
        .at(Step::Main1, 0)
        .start();
    let lb = g.card(0, "Leyline Binding", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(lb)), "{{4}}{{W}} with two Plains");
    let _ = Target::Player(0);
}

/// Pass priority and answer forced decisions until `stop` (no attacks).
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
fn cr_603_7_flickerwisp_the_card_returns_at_the_next_end_step_as_a_new_object() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains"])
        .hand(0, &["Flickerwisp"])
        .battlefield(1, &["Sol Ring"])
        .at(Step::Main1, 0)
        .start();
    let ring = bf(&g, 1, "Sol Ring");
    cast(&mut g, 0, "Flickerwisp");
    resolve_with(&mut g, |g| {
        if g.kind() == Some(DecisionKind::Target) && g.has_option(g.tgt(ring)) {
            Some(g.tgt(ring))
        } else {
            None
        }
    });
    assert_eq!(g.obj(ring).zone, Zone::Exile);
    quiet_until(&mut g, |g| g.turn().step == Step::End && at_priority(g) && g.stack().is_empty());
    assert_eq!(g.obj(ring).zone, Zone::Battlefield, "{}", g.log_lines().join("
"));
    assert_eq!(g.obj(ring).controller, 1);
}

#[test]
fn sneak_attack_the_creature_has_haste_and_is_sacrificed_at_the_end_step() {
    let mut g = scen()
        .battlefield(0, &["Sneak Attack", "Mountain"])
        .hand(0, &["Griselbrand"])
        .at(Step::Main1, 0)
        .start();
    let sa = bf(&g, 0, "Sneak Attack");
    g.choose(Choice::Ability(sa, engine::db::AbilitySlot::Activated(0)));
    g.drive_until(at_priority);
    pay(&mut g);
    resolve_with(&mut g, |g| {
        if g.kind() == Some(DecisionKind::ChooseHandCard) {
            g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|c| *c != Choice::Pass)
        } else {
            None
        }
    });
    let gris = bf(&g, 0, "Griselbrand");
    assert!(g.has_kw(gris, engine::db::Kw::Haste));
    quiet_until(&mut g, |g| g.turn().step == Step::End && at_priority(g) && g.stack().is_empty());
    assert_eq!(g.obj(gris).zone, Zone::Graveyard);
}

#[test]
fn mana_drain_adds_colorless_at_the_beginning_of_your_next_main_phase() {
    let mut g = scen()
        .battlefield(0, &["Mountain"])
        .hand(0, &["Lightning Bolt"])
        .battlefield(1, &["Island", "Island"])
        .hand(1, &["Mana Drain"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Lightning Bolt");
    g.choose(Choice::Target(Target::Player(1)));
    g.drive_until(at_priority);
    pay(&mut g);
    g.pass();
    cast(&mut g, 1, "Mana Drain");
    resolve_with(&mut g, |_| None);
    assert_eq!(g.life(1), 20);
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
    // The delayed trigger resolves at P1's first main phase: {C} for the
    // Bolt's mana value 1.
    resolve_with(&mut g, |_| None);
    assert_eq!(g.player(1).pool.amt[5], 1, "{}", g.log_lines().join("
"));
}

#[test]
fn shallow_grave_the_top_creature_card_returns_with_haste_and_is_exiled_at_end_step() {
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp"])
        .graveyard(0, &["Griselbrand", "Test Bear", "Island"])
        .hand(0, &["Shallow Grave"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Shallow Grave");
    resolve_with(&mut g, |_| None);
    // Island is on top; the top creature card is Test Bear.
    let bear = bf(&g, 0, "Test Bear");
    assert!(g.has_kw(bear, engine::db::Kw::Haste));
    quiet_until(&mut g, |g| g.turn().step == Step::End && at_priority(g) && g.stack().is_empty());
    assert_eq!(g.count_in(0, "Test Bear", Zone::Exile), 1);
}
