//! M7a: the monarch (CR 724), escalate (702.120), level up (711), "if it
//! would die this turn, exile it instead", one spell each turn, opponents'
//! permanents entering tapped, casting a graveyard card this turn, token
//! creation events, counters as they last existed (608.2h).

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

fn cast(g: &mut Game, p: PlayerId, name: &str) {
    let c = g.card(p, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
    pay(g);
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
fn cr_724_palace_jailer_exiles_until_an_opponent_becomes_the_monarch() {
    let mut g = scen()
        .battlefield(0, &["Plains"; 4])
        .hand(0, &["Palace Jailer"])
        .battlefield(1, &["Test Bear", "Test Hasty"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 1, "Test Bear");
    cast(&mut g, 0, "Palace Jailer");
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(g.tgt(bear))).then(|| g.tgt(bear)));
    assert_eq!(g.obj(bear).zone, Zone::Exile);
    // The monarch draws at the beginning of their end step.
    let hand = g.player(0).hand.len();
    quiet_until(&mut g, |g| g.turn().step == Step::End && at_priority(g));
    assert_eq!(g.player(0).hand.len(), hand + 1);
    // P1 attacks with the hasty creature: combat damage to the monarch.
    quiet_until(&mut g, |g| g.turn().active == 1 && g.kind() == Some(DecisionKind::DeclareAttackers));
    let hasty = bf(&g, 1, "Test Hasty");
    g.choose(Choice::Card(hasty));
    g.choose(Choice::Pass);
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert_eq!(g.obj(bear).zone, Zone::Battlefield, "back when P1 became the monarch");
    assert_eq!(g.obj(bear).controller, 1);
}

#[test]
fn aragorn_as_monarch_no_creature_can_block() {
    let mut g = scen()
        .battlefield(0, &["Island", "Mountain", "Plains", "Plains"])
        .hand(0, &["Aragorn, King of Gondor"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Aragorn, King of Gondor");
    resolve_with(&mut g, |_| None);
    let aragorn = bf(&g, 0, "Aragorn, King of Gondor");
    let t0 = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number > t0 + 1 && g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(aragorn));
    g.choose(Choice::Pass);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(Choice::Pass),
        _ => None,
    });
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert_eq!(g.life(1), 16, "unblocked");
}

#[test]
fn cr_702_120_collective_brutality_escalates_with_discards() {
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp"])
        .hand(0, &["Collective Brutality", "Island", "Forest"])
        .battlefield(1, &["Test Bear"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 1, "Test Bear");
    let cb = g.card(0, "Collective Brutality", Zone::Hand);
    g.choose(Choice::Cast(cb));
    g.drive_until(|g| !at_priority(g));
    // Three modes, two discards.
    g.choose(Choice::Number(0));
    g.choose(Choice::Number(1));
    g.choose(Choice::Number(2));
    let mut discards = 0;
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(g.tgt(bear)) => Some(g.tgt(bear)),
        Some(DecisionKind::Target) => Some(Choice::Target(Target::Player(1))),
        _ => None,
    });
    discards += g.player(0).graveyard.iter().filter(|&&c| g.name(c) != "Collective Brutality").count();
    assert_eq!(discards, 2);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard, "-2/-2");
    assert_eq!(g.count_in(1, "Lightning Bolt", Zone::Graveyard), 1);
    assert_eq!((g.life(0), g.life(1)), (22, 18));
}

#[test]
fn staff_of_the_storyteller_counts_token_creation_once_per_effect() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Swamp", "Swamp", "Plains"])
        .hand(0, &["Staff of the Storyteller", "Lingering Souls"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Staff of the Storyteller");
    resolve_with(&mut g, |_| None);
    let staff = bf(&g, 0, "Staff of the Storyteller");
    assert_eq!(g.counters_on(staff, Counter::Named(db().subtype_id("counter:story").unwrap())), 1, "its own Spirit");
    cast(&mut g, 0, "Lingering Souls");
    resolve_with(&mut g, |_| None);
    assert_eq!(g.counters_on(staff, Counter::Named(db().subtype_id("counter:story").unwrap())), 2, "two Spirits: one event");
}

#[test]
fn time_spiral_exiles_itself_and_untaps_up_to_six_lands() {
    let mut g = scen()
        .battlefield(0, &["Island"; 6])
        .hand(0, &["Time Spiral"])
        .at(Step::Main1, 0)
        .start();
    let ts = g.card(0, "Time Spiral", Zone::Hand);
    g.choose(Choice::Cast(ts));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
    pay(&mut g);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChoosePermanent) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|c| *c != Choice::Pass),
        _ => None,
    });
    assert_eq!(g.obj(ts).zone, Zone::Exile);
    assert_eq!(g.player(0).hand.len(), 7);
    assert!(g.lands_of(0).all(|c| !g.obj(c).tapped));
}

#[test]
fn cr_711_hexdrinker_levels() {
    let mut g = scen().battlefield(0, &["Hexdrinker"]).battlefield(0, &["Island"; 9]).at(Step::Main1, 0).start();
    let hd = bf(&g, 0, "Hexdrinker");
    assert_eq!(g.pt(hd), (2, 1));
    for _ in 0..3 {
        g.choose(Choice::Ability(hd, AbilitySlot::Activated(0)));
        g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
        pay(&mut g);
        resolve_with(&mut g, |_| None);
    }
    assert_eq!(g.pt(hd), (4, 4));
    let p = g.protection_of(hd);
    assert!(p.types & engine::db::types::INSTANT != 0, "protection from instants");
    let _ = Kw::Flying;
}

#[test]
fn suplex_exiles_the_creature_if_it_would_die_this_turn() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain"])
        .hand(0, &["Suplex"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 1, "Test Bear");
    let s = g.card(0, "Suplex", Zone::Hand);
    g.choose(Choice::Cast(s));
    g.drive_until(|g| !at_priority(g));
    g.choose(Choice::Number(0));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(g.tgt(bear)),
        _ => None,
    });
    assert_eq!(g.obj(bear).zone, Zone::Exile);
}

#[test]
fn archon_of_emeria_one_spell_each_turn_and_opponents_nonbasics_enter_tapped() {
    let mut g = scen()
        .battlefield(0, &["Archon of Emeria", "Mountain", "Mountain"])
        .hand(0, &["Lightning Bolt", "Lightning Bolt"])
        .hand(1, &["Steam Vents", "Island"])
        .at(Step::Main1, 0)
        .start();
    let b1 = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b1));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(Choice::Target(Target::Player(1))),
        _ => None,
    });
    let b2 = g.card(0, "Lightning Bolt", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(b2)), "one spell this turn");
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
    let sv = g.card(1, "Steam Vents", Zone::Hand);
    g.choose(Choice::Play(sv));
    resolve_with(&mut g, |_| None);
    assert!(g.obj(sv).tapped, "a nonbasic land of an opponent");
}

#[test]
fn spider_woman_opponents_creatures_enter_tapped() {
    let mut g = scen()
        .battlefield(0, &["Spider-Woman, Stunning Savior"])
        .battlefield(1, &["Forest", "Forest"])
        .hand(1, &["Test Bear"])
        .at(Step::Main1, 1)
        .start();
    cast(&mut g, 1, "Test Bear");
    resolve_with(&mut g, |_| None);
    let bear = bf(&g, 1, "Test Bear");
    assert!(g.obj(bear).tapped);
}

#[test]
fn emry_casts_an_artifact_from_the_graveyard_this_turn() {
    let mut g = scen()
        .battlefield(0, &["Emry, Lurker of the Loch", "Island"])
        .graveyard(0, &["Sol Ring"])
        .at(Step::Main1, 0)
        .start();
    let emry = bf(&g, 0, "Emry, Lurker of the Loch");
    let ring = g.card(0, "Sol Ring", Zone::Graveyard);
    assert!(!g.has_option(Choice::Cast(ring)));
    g.choose(Choice::Ability(emry, AbilitySlot::Activated(0)));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(g.tgt(ring)),
        _ => None,
    });
    assert!(g.has_option(Choice::Cast(ring)));
    g.choose(Choice::Cast(ring));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
    pay(&mut g);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(ring).zone, Zone::Battlefield);
}

#[test]
fn reckoner_bankbuster_draws_and_makes_a_pilot_when_its_last_counter_goes() {
    let mut g = scen()
        .battlefield(0, &["Sol Ring", "Sol Ring", "Sol Ring", "Island", "Island", "Island"])
        .hand(0, &["Reckoner Bankbuster"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Reckoner Bankbuster");
    resolve_with(&mut g, |_| None);
    let rb = bf(&g, 0, "Reckoner Bankbuster");
    let charge = Counter::Named(db().subtype_id("counter:charge").unwrap());
    assert_eq!(g.counters_on(rb, charge), 3);
    let t0 = g.turn().number;
    for round in 0..3 {
        quiet_until(&mut g, |g| g.turn().number >= t0 + 2 * round && g.turn().active == 0 && g.turn().step == Step::Main1 && at_priority(g));
        g.choose(Choice::Ability(rb, AbilitySlot::Activated(0)));
        g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
        pay(&mut g);
        resolve_with(&mut g, |_| None);
        let t = g.turn().number;
        quiet_until(&mut g, |g| g.turn().number > t);
    }
    assert_eq!(g.counters_on(rb, charge), 0);
    assert_eq!(g.count_in(0, "Pilot", Zone::Battlefield), 1);
    assert_eq!(g.count_in(0, "Treasure", Zone::Battlefield), 1);
}
