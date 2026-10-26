//! M7a: casting from other zones (CR 601.2a): flashback (702.34), escape
//! (702.138), permissions to play exiled cards, lands from the graveyard,
//! dash (702.109), backup (702.165).

mod common;

use common::db;
use engine::db::{AbilitySlot, Counter, Kw};
use engine::testkit::Scenario;
use engine::{act, CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

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
        g.drive_until(at_priority);
    }
}

/// Cast `c` (from wherever it is), choosing `alt` if asked (0 = the mana
/// cost), then pay with the first sources.
fn cast_card(g: &mut Game, c: CardIdx, alt: u8) {
    g.choose(Choice::Cast(c));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::ChooseAltCost) || g.kind() == Some(DecisionKind::PayMana));
    if g.kind() == Some(DecisionKind::ChooseAltCost) {
        g.choose(Choice::Number(alt));
    }
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
    pay(g);
}

/// Run the stack down; `answer` answers what is asked (None: the first
/// option).
fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..300 {
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

/// Pass priority, decline attacks and blocks, answer the rest with the
/// first option until `stop`.
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

fn action_of(g: &Game, c: Choice) -> u16 {
    g.pending_decision().unwrap().options.iter().find(|o| o.choice == c).unwrap().action
}

#[test]
fn cr_702_34_lingering_souls_flashback_from_the_graveyard_then_exiled() {
    // ruling: 12673ccc 15896ad1
    let mut g = scen().battlefield(0, &["Swamp", "Plains"]).graveyard(0, &["Lingering Souls"]).at(Step::Main1, 0).start();
    let ls = g.card(0, "Lingering Souls", Zone::Graveyard);
    assert!(g.has_option(Choice::Cast(ls)));
    assert_eq!(action_of(&g, Choice::Cast(ls)), act::GRAVEYARD, "its graveyard slot");
    cast_card(&mut g, ls, 0);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.count_in(0, "Spirit", Zone::Battlefield), 2);
    assert_eq!(g.obj(ls).zone, Zone::Exile, "exiled instead of going to the graveyard");
}

#[test]
fn cr_702_34a_a_countered_flashback_spell_is_exiled() {
    // ruling: 15896ad1
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Mountain"])
        .graveyard(0, &["Faithless Looting"])
        .battlefield(1, &["Island", "Island"])
        .hand(1, &["Counterspell"])
        .at(Step::Main1, 0)
        .start();
    let fl = g.card(0, "Faithless Looting", Zone::Graveyard);
    cast_card(&mut g, fl, 0);
    g.pass();
    let cs = g.card(1, "Counterspell", Zone::Hand);
    cast_card(&mut g, cs, 0);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(fl).zone, Zone::Exile);
}

#[test]
fn snapcaster_grants_flashback_for_the_mana_cost_until_end_of_turn() {
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Mountain", "Mountain"])
        .hand(0, &["Snapcaster Mage"])
        .graveyard(0, &["Lightning Bolt", "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Graveyard);
    assert!(!g.has_option(Choice::Cast(bolt)));
    let snap = g.card(0, "Snapcaster Mage", Zone::Hand);
    cast_card(&mut g, snap, 0);
    resolve_with(&mut g, |_| None);
    assert!(g.has_option(Choice::Cast(bolt)), "{:?}
{}", g.options_desc(), g.log_lines().join("
"));
    cast_card(&mut g, bolt, 0);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(player(1))).then(|| player(1)));
    assert_eq!(g.life(1), 17);
    assert_eq!(g.obj(bolt).zone, Zone::Exile);
}

#[test]
fn granted_flashback_ends_with_the_turn() {
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Mountain"])
        .hand(0, &["Snapcaster Mage"])
        .graveyard(0, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Graveyard);
    let snap = g.card(0, "Snapcaster Mage", Zone::Hand);
    cast_card(&mut g, snap, 0);
    resolve_with(&mut g, |_| None);
    let t0 = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number > t0 && at_priority(g));
    assert!(!g.has_option(Choice::Cast(bolt)));
}

#[test]
fn cr_702_138_uro_cast_from_hand_is_sacrificed_escaped_it_stays() {
    let mut g = base()
        .library(0, &["Island"; 10])
        .library(1, &["Forest"; 10])
        .battlefield(0, &["Forest", "Forest", "Island", "Island"])
        .hand(0, &["Uro, Titan of Nature's Wrath"])
        .graveyard(0, &["Lightning Bolt", "Counterspell", "Test Bear", "Sol Ring", "Island"])
        .at(Step::Main1, 0)
        .start();
    let uro = g.card(0, "Uro, Titan of Nature's Wrath", Zone::Hand);
    cast_card(&mut g, uro, 0);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(uro).zone, Zone::Graveyard, "not escaped: sacrificed");
    assert_eq!(g.life(0), 23, "its enters trigger still resolves");
    // Escape it next turn: {G}{G}{U}{U} and five other cards exiled.
    let t0 = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number > t0 + 1 && g.turn().step == Step::Main1 && at_priority(g));
    let uro_gy = g.card(0, "Uro, Titan of Nature's Wrath", Zone::Graveyard);
    assert!(g.has_option(Choice::Cast(uro_gy)), "{:?}", g.options_desc());
    g.choose(Choice::Cast(uro_gy));
    let mut exiled = 0;
    for _ in 0..40 {
        g.drive_until(|g| !at_priority(g) || g.stack().len() == 1);
        match g.kind() {
            Some(DecisionKind::PayMana) => {
                let c = g.pending_decision().unwrap().options[0].choice;
                g.choose(c);
            }
            Some(DecisionKind::ChooseGraveyardCard) => {
                let c = g.pending_decision().unwrap().options[0].choice;
                assert_ne!(c, Choice::Card(uro_gy), "other cards");
                g.choose(c);
                exiled += 1;
            }
            _ => break,
        }
    }
    assert_eq!(exiled, 5);
    resolve_with(&mut g, |_| None);
    let uro = bf(&g, 0, "Uro, Titan of Nature's Wrath");
    assert_eq!(g.pt(uro), (6, 6));
    assert_eq!(g.life(0), 26);
}

#[test]
fn cr_702_109_ragavan_dashed_has_haste_steals_a_card_and_returns_to_hand() {
    let mut g = base()
        .library(0, &["Island"; 10])
        .library(1, &["Lightning Bolt", "Forest", "Forest"])
        .battlefield(0, &["Mountain", "Mountain", "Mountain"])
        .hand(0, &["Ragavan, Nimble Pilferer"])
        .at(Step::Main1, 0)
        .start();
    let rag = g.card(0, "Ragavan, Nimble Pilferer", Zone::Hand);
    cast_card(&mut g, rag, 1);
    resolve_with(&mut g, |_| None);
    assert!(g.has_kw(rag, Kw::Haste), "dashed");
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(rag));
    g.choose(Choice::Pass);
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert_eq!(g.life(1), 18);
    assert_eq!(g.count_in(0, "Treasure", Zone::Battlefield), 1);
    let bolt = g.card(1, "Lightning Bolt", Zone::Exile);
    assert!(g.has_option(Choice::Cast(bolt)), "{:?}", g.options_desc());
    assert_eq!(action_of(&g, Choice::Cast(bolt)), act::EXILE);
    cast_card(&mut g, bolt, 0);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(player(1))).then(|| player(1)));
    assert_eq!(g.life(1), 15);
    assert_eq!(g.obj(bolt).zone, Zone::Graveyard);
    assert_eq!(g.player(1).graveyard.last(), Some(&bolt), "its owner's graveyard");
    quiet_until(&mut g, |g| g.turn().step == Step::End && at_priority(g) && g.stack().is_empty());
    assert_eq!(g.obj(rag).zone, Zone::Hand, "returned by dash");
}

#[test]
fn ragavan_cast_normally_has_no_haste() {
    let mut g = scen().battlefield(0, &["Mountain", "Mountain"]).hand(0, &["Ragavan, Nimble Pilferer"]).at(Step::Main1, 0).start();
    let rag = g.card(0, "Ragavan, Nimble Pilferer", Zone::Hand);
    cast_card(&mut g, rag, 0);
    resolve_with(&mut g, |_| None);
    assert!(!g.has_kw(rag, Kw::Haste));
    quiet_until(&mut g, |g| g.turn().step == Step::Cleanup || g.turn().active == 1);
    assert_eq!(g.obj(rag).zone, Zone::Battlefield);
}

#[test]
fn golos_exiles_three_to_play_free_this_turn_a_land_uses_the_land_drop() {
    let mut g = base()
        .library(0, &["Test Giant", "Forest", "Lightning Bolt", "Island", "Island"])
        .library(1, &["Forest"; 10])
        .battlefield(0, &["Golos, Tireless Pilgrim", "Plains", "Island", "Swamp", "Mountain", "Forest", "Forest", "Forest"])
        .at(Step::Main1, 0)
        .start();
    let golos = bf(&g, 0, "Golos, Tireless Pilgrim");
    g.choose(Choice::Ability(golos, AbilitySlot::Activated(0)));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
    pay(&mut g);
    resolve_with(&mut g, |_| None);
    let giant = g.card(0, "Test Giant", Zone::Exile);
    let bolt = g.card(0, "Lightning Bolt", Zone::Exile);
    let forest = g.card(0, "Forest", Zone::Exile);
    assert!(g.has_option(Choice::Cast(giant)), "free: no mana left: {:?}", g.options_desc());
    assert!(g.has_option(Choice::Play(forest)));
    g.choose(Choice::Play(forest));
    g.drive_until(at_priority);
    assert_eq!(g.obj(forest).zone, Zone::Battlefield);
    cast_card(&mut g, giant, 0);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(giant).zone, Zone::Battlefield);
    assert!(g.has_option(Choice::Cast(bolt)));
    let t0 = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number > t0 && at_priority(g));
    assert!(!g.has_option(Choice::Cast(bolt)), "this turn only");
}

#[test]
fn robber_of_the_rich_cast_with_any_mana_during_a_turn_you_attacked_with_a_rogue() {
    let mut g = base()
        .library(0, &["Island"; 10])
        .library(1, &["Test Bear", "Forest", "Forest", "Forest"])
        .battlefield(0, &["Robber of the Rich", "Mountain", "Mountain"])
        .hand(1, &["Island", "Island"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let robber = bf(&g, 0, "Robber of the Rich");
    g.choose(Choice::Card(robber));
    g.choose(Choice::Pass);
    resolve_with(&mut g, |_| None);
    let bear = g.card(1, "Test Bear", Zone::Exile);
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    // {1}{G} with two Mountains: mana as though of any colour.
    assert!(g.has_option(Choice::Cast(bear)), "{:?}", g.options_desc());
    let t0 = g.turn().number;
    // Next turn, before attacking: not yet.
    quiet_until(&mut g, |g| g.turn().number == t0 + 2 && g.turn().step == Step::Main1 && at_priority(g));
    assert!(!g.has_option(Choice::Cast(bear)), "no Rogue attacked this turn");
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(robber));
    g.choose(Choice::Pass);
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert!(g.has_option(Choice::Cast(bear)), "attacked with a Rogue: while it stays exiled");
    cast_card(&mut g, bear, 0);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(bear).zone, Zone::Battlefield);
    assert_eq!(g.obj(bear).controller, 0);
}

#[test]
fn crucible_of_worlds_plays_a_land_from_the_graveyard() {
    let g = scen().battlefield(0, &["Crucible of Worlds"]).graveyard(0, &["Forest"]).at(Step::Main1, 0).start();
    let forest = g.card(0, "Forest", Zone::Graveyard);
    assert!(g.has_option(Choice::Play(forest)));
    let g = scen().graveyard(0, &["Forest"]).at(Step::Main1, 0).start();
    let forest = g.card(0, "Forest", Zone::Graveyard);
    assert!(!g.has_option(Choice::Play(forest)));
}

#[test]
fn cr_702_165_backup_on_another_creature_gives_it_double_strike() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Mountain", "Test Bear"])
        .hand(0, &["Death-Greeter's Champion"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 0, "Test Bear");
    let dgc = g.card(0, "Death-Greeter's Champion", Zone::Hand);
    cast_card(&mut g, dgc, 0);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(g.tgt(bear))).then(|| g.tgt(bear)));
    assert_eq!(g.counters_on(bear, Counter::P1P1), 1);
    assert!(g.has_kw(bear, Kw::DoubleStrike));
    assert_eq!(g.pt(bear), (3, 3));
}
