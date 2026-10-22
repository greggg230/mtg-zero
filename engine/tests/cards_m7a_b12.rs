//! M7a: buyback (CR 702.27), overload (702.96), granted escape, token
//! copies of tokens (707.2), control of the source to an opponent (108.4),
//! a triggered ability an Equipment grants.

mod common;

use common::db;
use engine::db::{AbilitySlot, Kw};
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
fn cr_702_27_corpse_dance_bought_back_returns_to_hand() {
    let mut g = scen()
        .battlefield(0, &["Swamp"; 5])
        .graveyard(0, &["Test Bear"])
        .hand(0, &["Corpse Dance"])
        .at(Step::Main1, 0)
        .start();
    let cd = g.card(0, "Corpse Dance", Zone::Hand);
    g.choose(Choice::Cast(cd));
    g.drive_until(|g| !at_priority(g));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseCost));
    g.choose(Choice::Number(1)); // buyback
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(cd).zone, Zone::Hand);
    let bear = bf(&g, 0, "Test Bear");
    assert!(g.has_kw(bear, Kw::Haste));
}

#[test]
fn cr_702_96_damn_overloaded_destroys_each_creature() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Swamp", "Swamp", "Test Bear"])
        .battlefield(1, &["Test Giant", "Test Flyer"])
        .hand(0, &["Damn"])
        .at(Step::Main1, 0)
        .start();
    let damn = g.card(0, "Damn", Zone::Hand);
    g.choose(Choice::Cast(damn));
    g.drive_until(|g| !at_priority(g));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseAltCost));
    g.choose(Choice::Number(1)); // overload
    resolve_with(&mut g, |_| None);
    assert_eq!(g.creatures_of(0).count() + g.creatures_of(1).count(), 0);
    // Not overloaded: one target.
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp"])
        .battlefield(1, &["Test Giant", "Test Flyer"])
        .hand(0, &["Damn"])
        .at(Step::Main1, 0)
        .start();
    let giant = bf(&g, 1, "Test Giant");
    let damn = g.card(0, "Damn", Zone::Hand);
    g.choose(Choice::Cast(damn));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(g.tgt(giant)),
        _ => None,
    });
    assert_eq!(g.obj(giant).zone, Zone::Graveyard);
    assert_eq!(g.count_in(1, "Test Flyer", Zone::Battlefield), 1);
}

#[test]
fn underworld_breach_gives_graveyard_cards_escape_and_is_sacrificed() {
    let mut g = scen()
        .battlefield(0, &["Underworld Breach", "Mountain"])
        .graveyard(0, &["Lightning Bolt", "Island", "Forest", "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Graveyard);
    assert!(g.has_option(Choice::Cast(bolt)), "{{R}} and three other cards");
    let island = g.card(0, "Island", Zone::Graveyard);
    assert!(!g.has_option(Choice::Play(island)), "a land can't escape");
    g.choose(Choice::Cast(bolt));
    for _ in 0..20 {
        g.drive_until(|g| !at_priority(g) || g.stack().len() == 1);
        match g.kind() {
            Some(DecisionKind::Target) => g.choose(Choice::Target(engine::Target::Player(1))),
            Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseGraveyardCard) => {
                let c = g.pending_decision().unwrap().options[0].choice;
                g.choose(c)
            }
            _ => break,
        };
    }
    resolve_with(&mut g, |_| None);
    assert_eq!(g.life(1), 17);
    assert_eq!(g.obj(bolt).zone, Zone::Graveyard, "escaped instants go to the graveyard");
    assert_eq!(g.exile_zone().len(), 3);
    quiet_until(&mut g, |g| g.turn().step == Step::End && at_priority(g) && g.stack().is_empty());
    assert_eq!(g.count_in(0, "Underworld Breach", Zone::Graveyard), 1);
}

#[test]
fn wishclaw_talisman_tutors_then_the_opponent_controls_it() {
    let g = scen().battlefield(0, &["Wishclaw Talisman", "Swamp"]).at(Step::Main1, 0).start();
    let wt = bf(&g, 0, "Wishclaw Talisman");
    // Scenario permanents don't enter: no wish counter, can't activate.
    assert!(!g.has_option(Choice::Ability(wt, AbilitySlot::Activated(0))));
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp", "Swamp"])
        .hand(0, &["Wishclaw Talisman"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Wishclaw Talisman", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
    pay(&mut g);
    resolve_with(&mut g, |_| None);
    let wt = bf(&g, 0, "Wishclaw Talisman");
    g.choose(Choice::Ability(wt, AbilitySlot::Activated(0)));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
    pay(&mut g);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(wt).controller, 1);
    assert_eq!(g.player(0).hand.len(), 1, "a card found");
}

#[test]
fn esikas_chariot_copies_a_token_when_it_attacks() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest", "Forest", "Forest"])
        .hand(0, &["Esika's Chariot"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Esika's Chariot", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
    pay(&mut g);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.count_in(0, "Cat", Zone::Battlefield), 2);
    let chariot = bf(&g, 0, "Esika's Chariot");
    let t0 = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number > t0 + 1 && g.turn().step == Step::Main1 && at_priority(g));
    // Crew 4 with both Cats.
    g.choose(Choice::Ability(chariot, AbilitySlot::Activated(0)));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChoosePermanent) => {
            g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|c| *c != Choice::Pass).or(Some(Choice::Pass))
        }
        _ => None,
    });
    assert!(g.is_creature(chariot));
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(chariot));
    g.choose(Choice::Pass);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.count_in(0, "Cat", Zone::Battlefield), 3);
}

#[test]
fn kaldra_compleats_germ_exiles_a_creature_it_damages() {
    let mut g = scen()
        .battlefield(0, &["Sol Ring", "Sol Ring", "Mountain", "Mountain", "Mountain"])
        .hand(0, &["Kaldra Compleat"])
        .battlefield(1, &["Test Indestructible"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Kaldra Compleat", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
    pay(&mut g);
    resolve_with(&mut g, |_| None);
    let germ = bf(&g, 0, "Phyrexian Germ");
    assert_eq!(g.pt(germ), (5, 5));
    assert!(g.has_kw(germ, Kw::Haste) && g.has_kw(germ, Kw::FirstStrike));
    // An indestructible blocker survives the damage: then it's exiled.
    let giant = bf(&g, 1, "Test Indestructible");
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(germ));
    g.choose(Choice::Pass);
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareBlockers));
    g.choose(Choice::Card(germ));
    resolve_with(&mut g, |_| None);
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert!(matches!(g.obj(giant).zone, Zone::Exile | Zone::Graveyard), "{}", g.log_lines().join("\n"));
    assert_eq!(g.obj(giant).zone, Zone::Exile, "exiled by the granted trigger (or dead first)");
}
