//! M7a: additional land drops (CR 305.2), Atraxa's one card per card type,
//! Barrowgoyf's mill, cards exiled with Ghost Vacuum (607.2a), a random
//! hand card looked at, Arwen's counter cost.

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

#[test]
fn cr_305_2_icetill_explorer_gives_an_additional_land_drop() {
    let mut g = scen()
        .battlefield(0, &["Icetill Explorer"])
        .hand(0, &["Forest", "Island", "Mountain"])
        .at(Step::Main1, 0)
        .start();
    for name in ["Forest", "Island"] {
        let c = g.card(0, name, Zone::Hand);
        assert!(g.has_option(Choice::Play(c)));
        g.choose(Choice::Play(c));
        resolve_with(&mut g, |_| None);
    }
    let m = g.card(0, "Mountain", Zone::Hand);
    assert!(!g.has_option(Choice::Play(m)), "two land drops");
}

#[test]
fn atraxa_takes_one_card_per_card_type() {
    let lib = ["Test Bear", "Test Flyer", "Island", "Sol Ring", "Lightning Bolt", "Forest", "Counterspell", "Island", "Island", "Island", "Plains"];
    let mut g = base()
        .library(0, &lib)
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Plains", "Island", "Swamp", "Forest", "Plains", "Island", "Swamp"])
        .hand(0, &["Atraxa, Grand Unifier"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Atraxa, Grand Unifier", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
    pay(&mut g);
    let mut taken = 0;
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::PutInHand) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|c| *c != Choice::Pass),
        _ => None,
    });
    taken += g.player(0).hand.len();
    // Creature, land, artifact, instant: four (two creatures, two instants,
    // several lands: one each).
    assert_eq!(taken, 4, "{:?}", g.player(0).hand.iter().map(|&c| g.name(c)).collect::<Vec<_>>());
    assert_eq!(g.player(0).library.len(), 11 - 4);
}

#[test]
fn barrowgoyf_mills_the_damage_and_may_take_a_creature() {
    let mut g = base()
        .library(0, &["Island", "Test Bear", "Island", "Island", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Barrowgoyf"])
        .graveyard(1, &["Island", "Lightning Bolt"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let bg = bf(&g, 0, "Barrowgoyf");
    assert_eq!(g.pt(bg), (2, 3));
    g.choose(Choice::Card(bg));
    g.choose(Choice::Pass);
    let bear = g.card(0, "Test Bear", Zone::Library);
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::YesNo));
    g.choose(Choice::Yes);
    g.drive_until(|g| !at_priority(g));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseGraveyardCard));
    g.choose(Choice::Card(bear));
    assert_eq!(g.obj(bear).zone, Zone::Hand);
    assert_eq!(g.life(0), 22, "lifelink");
}

#[test]
fn ghost_vacuum_returns_the_creature_cards_exiled_with_it_as_1_1_flying_spirits() {
    let mut g = scen()
        .battlefield(0, &["Ghost Vacuum", "Island", "Island", "Island", "Island", "Island", "Island"])
        .graveyard(1, &["Test Giant", "Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let gv = bf(&g, 0, "Ghost Vacuum");
    let giant = g.card(1, "Test Giant", Zone::Graveyard);
    g.choose(Choice::Ability(gv, AbilitySlot::Activated(0)));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(g.tgt(giant)),
        _ => None,
    });
    assert_eq!(g.obj(giant).zone, Zone::Exile);
    let t0 = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number > t0 + 1 && g.turn().step == Step::Main1 && at_priority(g));
    g.choose(Choice::Ability(gv, AbilitySlot::Activated(1)));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
    pay(&mut g);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(giant).zone, Zone::Battlefield);
    assert_eq!(g.obj(giant).controller, 0);
    assert_eq!(g.pt(giant), (1, 1));
    assert!(g.has_kw(giant, Kw::Flying));
}

#[test]
fn urzas_bauble_looks_at_a_random_hand_card() {
    let mut g = scen().battlefield(0, &["Urza's Bauble"]).hand(1, &["Lightning Bolt"]).at(Step::Main1, 0).start();
    let ub = bf(&g, 0, "Urza's Bauble");
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    assert!(!g.knows(0, bolt));
    g.choose(Choice::Ability(ub, AbilitySlot::Activated(0)));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(Choice::Target(Target::Player(1))),
        _ => None,
    });
    assert!(g.knows(0, bolt), "the only card: seen");
}

#[test]
fn arwen_removes_her_indestructible_counter_to_share_it() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest", "Plains", "Test Bear"])
        .hand(0, &["Arwen, Mortal Queen"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Arwen, Mortal Queen", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
    pay(&mut g);
    resolve_with(&mut g, |_| None);
    let arwen = bf(&g, 0, "Arwen, Mortal Queen");
    let bear = bf(&g, 0, "Test Bear");
    assert!(g.has_kw(arwen, Kw::Indestructible));
    // ({1} left? No: three lands spent. Next turn.)
    let t0 = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number > t0 + 1 && g.turn().step == Step::Main1 && at_priority(g));
    g.choose(Choice::Ability(arwen, AbilitySlot::Activated(0)));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(g.tgt(bear)),
        _ => None,
    });
    assert!(!g.has_kw(arwen, Kw::Indestructible), "the counter is gone");
    assert!(g.has_kw(bear, Kw::Indestructible) && g.has_kw(bear, Kw::Lifelink));
    assert_eq!(g.counters_on(bear, Counter::P1P1), 1);
    assert_eq!(g.counters_on(arwen, Counter::P1P1), 1);
    assert!(g.has_kw(arwen, Kw::Lifelink));
    let arwen_again = Choice::Ability(arwen, AbilitySlot::Activated(0));
    assert!(!g.has_option(arwen_again), "no indestructible counter left");
}
