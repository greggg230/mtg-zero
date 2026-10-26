//! M7a: X (CR 107.3, 601.2b, 202.3e), counters removed as a cost (118.3),
//! crew (702.122), "whenever this blocks" (509.1i).

mod common;

use common::db;
use engine::db::{AbilitySlot, Counter, Kw};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

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

fn player(p: PlayerId) -> Choice {
    Choice::Target(Target::Player(p))
}

#[test]
fn cr_107_3_mind_twist_x_is_announced_and_paid() {
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp", "Swamp", "Swamp"])
        .hand(0, &["Mind Twist"])
        .hand(1, &["Forest", "Forest", "Forest", "Forest", "Forest"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Mind Twist", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| !at_priority(g));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseMode));
    assert!(g.has_option(Choice::Number(3)) && !g.has_option(Choice::Number(4)), "four Swamps: X at most 3");
    g.choose(Choice::Number(3));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(player(1)),
        _ => None,
    });
    assert_eq!(g.player(1).hand.len(), 2);
    assert!(g.lands_of(0).all(|c| g.obj(c).tapped));
}

#[test]
fn traumatic_critique_deals_x_and_loots() {
    let mut g = scen()
        .battlefield(0, &["Volcanic Island", "Volcanic Island", "Mountain", "Mountain"])
        .hand(0, &["Traumatic Critique"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Traumatic Critique", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| !at_priority(g));
    g.choose(Choice::Number(2));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(player(1)),
        _ => None,
    });
    assert_eq!(g.life(1), 18);
    assert_eq!(g.player(0).hand.len(), 1, "drew two, discarded one");
}

#[test]
fn unexpectedly_absent_puts_it_beneath_the_top_x() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains", "Plains"])
        .hand(0, &["Unexpectedly Absent"])
        .battlefield(1, &["Sol Ring"])
        .at(Step::Main1, 0)
        .start();
    let ring = bf(&g, 1, "Sol Ring");
    let c = g.card(0, "Unexpectedly Absent", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| !at_priority(g));
    g.choose(Choice::Number(2));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(g.tgt(ring)),
        _ => None,
    });
    let lib = &g.player(1).library;
    assert_eq!(lib[lib.len() - 3], ring, "third from the top");
}

#[test]
fn walking_ballista_enters_with_x_counters_and_removes_them_to_ping() {
    // ruling: 27f2af67
    let mut g = scen()
        .battlefield(0, &["Sol Ring", "Mountain", "Mountain"])
        .hand(0, &["Walking Ballista"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Walking Ballista", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| !at_priority(g));
    assert!(g.has_option(Choice::Number(2)), "{{X}}{{X}} with four mana");
    g.choose(Choice::Number(2));
    resolve_with(&mut g, |_| None);
    let wb = bf(&g, 0, "Walking Ballista");
    assert_eq!(g.counters_on(wb, Counter::P1P1), 2);
    assert_eq!(g.pt(wb), (2, 2));
    g.choose(Choice::Ability(wb, AbilitySlot::Activated(1)));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(player(1)),
        _ => None,
    });
    assert_eq!(g.life(1), 19);
    assert_eq!(g.counters_on(wb, Counter::P1P1), 1);
}

#[test]
fn cr_702_122_crew_taps_creatures_with_enough_power_and_the_copter_loots() {
    // ruling: 429c5474
    let mut g = scen()
        .battlefield(0, &["Smuggler's Copter", "Test Bear"])
        .hand(0, &["Island"])
        .at(Step::Main1, 0)
        .start();
    let copter = bf(&g, 0, "Smuggler's Copter");
    let bear = bf(&g, 0, "Test Bear");
    assert!(!g.is_creature(copter));
    g.choose(Choice::Ability(copter, AbilitySlot::Activated(0)));
    g.drive_until(|g| !at_priority(g) || !g.stack().is_empty());
    if g.kind() == Some(DecisionKind::ChoosePermanent) {
        assert!(!g.has_option(Choice::Card(copter)), "not itself");
        g.choose(Choice::Card(bear));
    }
    resolve_with(&mut g, |_| None);
    assert!(g.obj(bear).tapped);
    assert!(g.is_creature(copter));
    assert!(g.has_kw(copter, Kw::Flying));
    // Attack: may draw, then discard.
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(copter));
    g.choose(Choice::Pass);
    let hand = g.player(0).hand.len();
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        _ => None,
    });
    assert_eq!(g.player(0).hand.len(), hand, "drew one, discarded one");
    assert_eq!(g.player(0).graveyard.len(), 1);
    quiet_until(&mut g, |g| g.turn().active == 1);
    assert!(!g.is_creature(copter), "until end of turn");
}

#[test]
fn the_copter_blocks_and_loots() {
    let mut g = scen()
        .battlefield(0, &["Test Bear"])
        .battlefield(1, &["Smuggler's Copter", "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let copter = bf(&g, 1, "Smuggler's Copter");
    let bear1 = bf(&g, 1, "Test Bear");
    let bear0 = bf(&g, 0, "Test Bear");
    // P1 crews in P0's beginning of combat.
    quiet_until(&mut g, |g| g.turn().step == Step::BeginCombat && at_priority(g) && g.turn().priority == 1);
    g.choose(Choice::Ability(copter, AbilitySlot::Activated(0)));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChoosePermanent) => Some(Choice::Card(bear1)),
        _ => None,
    });
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(bear0));
    g.choose(Choice::Pass);
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareBlockers) || g.turn().step == Step::DeclareBlockers);
    while at_priority(&g) && g.kind() != Some(DecisionKind::DeclareBlockers) {
        g.pass();
    }
    g.choose(Choice::Card(bear0));
    let mut asked = false;
    for _ in 0..50 {
        g.drive_until(|g| !at_priority(g) || g.turn().step != Step::DeclareBlockers);
        if g.turn().step != Step::DeclareBlockers {
            break;
        }
        match g.kind() {
            Some(DecisionKind::YesNo) => {
                asked = true;
                g.choose(Choice::Yes);
            }
            Some(DecisionKind::Priority) => {
                g.pass();
            }
            _ => {
                let c = g.pending_decision().unwrap().options[0].choice;
                g.choose(c);
            }
        }
    }
    assert!(asked, "whenever this Vehicle blocks: may draw");
}

#[test]
fn the_fantasticar_may_become_a_creature_on_a_noncreature_spell() {
    let mut g = scen()
        .battlefield(0, &["The Fantasticar", "Mountain"])
        .hand(0, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let car = bf(&g, 0, "The Fantasticar");
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.choose(player(1));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
    pay(&mut g);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        _ => None,
    });
    assert!(g.is_creature(car));
    assert_eq!(g.pt(car), (4, 4));
}
