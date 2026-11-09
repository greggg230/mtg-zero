//! M8d (ENGINE.md 37): the official rulings of the cube's face-down cards
//! (Abhorrent Oculus's manifest dread, Headliner Scarlett) as engine tests:
//! manifesting (701.40, 701.62), turning face up (708.8, 702.37e-like
//! special action 116.2b), the same permanent before and after (708.8),
//! a card that isn't a creature card (701.40c), playing a card exiled with
//! permission (601.2, 305.2). `ruling: <hash8>` names the ruling.

mod common;
mod rulings_util;

use engine::db::AbilitySlot;
use engine::{CardIdx, Choice, DecisionKind, Game, Step, Zone};
use rulings_util::*;

const OCULUS: &str = "Abhorrent Oculus";
const JACE: &str = "Jace, Vryn's Prodigy // Jace, Telepath Unbound";

/// P0's Oculus manifests dread at P1's upkeep from a library whose top two
/// cards are `top` (manifested) and an Island; `lands` on P0's side. Returns
/// the game at the first priority P0 gets afterwards, and the card.
fn manifested(top: &str, lands: &[&str]) -> (Game, CardIdx) {
    let mut g = scen().library(0, &["Island", top]).battlefield(0, &[OCULUS]).battlefield(0, lands).at(Step::End, 0).start();
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::PutOntoBattlefield));
    let c = g.card(0, top, Zone::Library);
    g.choose(Choice::Card(c));
    quiet_until(&mut g, |g| at_priority(g) && g.pending_decision().unwrap().player == 0);
    assert!(g.is_face_down(c));
    (g, c)
}

fn up(c: CardIdx) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(0))
}

/// Turn `c` face up, paying with the first sources.
fn turn_up(g: &mut Game, c: CardIdx) {
    assert!(g.has_option(up(c)), "{}", opts(g));
    g.choose(up(c));
    g.drive_until(|g| at_priority(g) && g.pending_decision().unwrap().player == 0 || !matches!(g.kind(), Some(DecisionKind::PayMana)));
    while g.kind() == Some(DecisionKind::PayMana) {
        let o = g.pending_decision().unwrap().options[0].choice;
        g.choose(o);
    }
    assert!(!g.is_face_down(c), "{}", g.log_lines().join("\n"));
}

#[test]
fn turning_a_manifested_creature_face_up_doesnt_trigger_its_enter_ability_cr_708_8() {
    // ruling: 57af8087
    let (mut g, c) = manifested("Thraben Inspector", &["Plains"]);
    turn_up(&mut g, c);
    assert_eq!(g.name(c), "Thraben Inspector");
    g.drive_until(at_priority);
    assert_eq!(g.count_in(0, "Clue", Zone::Battlefield), 0, "no investigate");
}

#[test]
fn a_manifested_instant_cant_be_turned_face_up_cr_701_40c() {
    // ruling: 522ee812
    let (g, c) = manifested("Lightning Bolt", &["Mountain", "Mountain"]);
    assert!(!g.has_option(up(c)), "{}", opts(&g));
    assert_eq!(g.pt(c), (2, 2));
}

#[test]
fn turning_face_up_keeps_it_tapped_and_attacking_the_same_permanent_cr_708_8() {
    // ruling: a4c44a79 6bdf2a83
    // The manifested Test Giant attacks on P0's turn as a 2/2, then is
    // turned face up: still tapped and attacking, it deals 5.
    let (mut g, c) = manifested("Test Giant", &["Forest"; 5]);
    quiet_until(&mut g, |g| g.turn().active == 0 && g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(c));
    g.choose(Choice::Pass);
    g.drive_until(at_priority);
    assert!(g.obj(c).tapped && g.obj(c).attacking);
    turn_up(&mut g, c);
    assert_eq!(g.name(c), "Test Giant");
    assert!(g.obj(c).tapped && g.obj(c).attacking, "the same permanent, still tapped and attacking");
    quiet_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(g.life(1), 15);
}

#[test]
fn an_equipment_stays_attached_when_the_permanent_turns_face_up_cr_708_8() {
    // ruling: 6bdf2a83
    // (Lightning Greaves would give shroud, Skullclamp is used below.)
    let mut g = scen()
        .library(0, &["Island", "Test Giant"])
        .battlefield(0, &[OCULUS, "Skullclamp"])
        .battlefield(0, &["Forest"; 6])
        .at(Step::End, 0)
        .start();
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::PutOntoBattlefield));
    let c2 = g.card(0, "Test Giant", Zone::Library);
    g.choose(Choice::Card(c2));
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Main1 && at_priority(g));
    let clamp = bf(&g, 0, "Skullclamp");
    let eq = (0..4u8).map(|i| Choice::Ability(clamp, AbilitySlot::Activated(i))).find(|&a| g.has_option(a)).unwrap();
    g.choose(eq);
    let t = g.tgt(c2);
    answer_targets(&mut g, &[t]);
    assert_eq!(g.pt(c2), (3, 1));
    turn_up(&mut g, c2);
    assert_eq!(g.obj(clamp).attached.map(|h| h.idx), Some(c2));
    assert_eq!(g.pt(c2), (6, 4));
}

#[test]
fn manifest_dread_with_one_card_manifests_it_and_with_none_does_nothing_cr_701_62() {
    // ruling: a3f0fe48 aa366faf
    let bare = || engine::testkit::Scenario::new(common::db()).spec(|s| s.auto_single = false).library(1, &["Forest"; 10]);
    let mut g = bare().library(0, &["Test Bear"]).battlefield(0, &[OCULUS]).at(Step::End, 0).start();
    let bear = g.card(0, "Test Bear", Zone::Library);
    let asked = std::cell::Cell::new(false);
    quiet_until_with(
        &mut g,
        |g| g.turn().active == 1 && g.turn().step == Step::Draw,
        |g| {
            if g.kind() == Some(DecisionKind::PutOntoBattlefield) {
                assert_eq!(g.pending_decision().unwrap().options.len(), 1);
                asked.set(true);
            }
            None
        },
    );
    assert_eq!(g.obj(bear).zone, Zone::Battlefield);
    assert!(g.is_face_down(bear));
    assert_eq!(g.player(0).graveyard.len(), 0);
    // (One option, the Bear: no graveyard choice.)
    assert!(asked.get());
    // No card: nothing happens (and nobody loses for it).
    let mut g = bare().battlefield(0, &[OCULUS]).at(Step::End, 0).start();
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Draw);
    assert!(!g.is_over());
    assert_eq!(g.battlefield().len(), 1);
}

#[test]
fn a_manifested_double_faced_card_turns_face_up_front_face_up_cr_701_40e() {
    // ruling: 7f95b9dc 5653e6a4
    let (mut g, c) = manifested(JACE, &["Island", "Island"]);
    turn_up(&mut g, c);
    // Its front face: a 0/2 creature (the card's name is both faces').
    assert!(g.name(c).starts_with("Jace, Vryn's Prodigy"));
    assert!(g.is_creature(c));
    assert_eq!(g.pt(c), (0, 2));
}

// ---- Headliner Scarlett --------------------------------------------------------------------------

const SCARLETT: &str = "Headliner Scarlett";

/// P0's upkeep with Scarlett: the top card of a library of `card`s exiled
/// face down; returns the game at P0's first main-phase priority and the
/// card.
fn scarlett_exiles(card: &str, lands: &[&str]) -> (Game, CardIdx) {
    let mut g = scen().battlefield(0, &[SCARLETT]).battlefield(0, lands).library(0, &[card; 10]).at(Step::Main2, 1).start();
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Main1 && at_priority(g));
    let fd: Vec<CardIdx> = (0..g.objs().len()).map(|i| CardIdx(i as u32)).filter(|&c| g.obj(c).zone == Zone::FaceDown).collect();
    assert_eq!(fd.len(), 1);
    (g, fd[0])
}

#[test]
fn headliner_scarlett_gone_the_card_stays_playable_this_turn_cr_611_2a() {
    // ruling: 5b673494
    // P0 exiles its own Scarlett with Swords to Plowshares.
    let mut g2 = scen()
        .battlefield(0, &[SCARLETT, "Forest", "Forest", "Plains"])
        .hand(0, &["Swords to Plowshares"])
        .library(0, &["Test Bear"; 10])
        .at(Step::Main2, 1)
        .start();
    quiet_until(&mut g2, |g| g.turn().active == 0 && g.turn().step == Step::Main1 && at_priority(g));
    let fd = (0..g2.objs().len()).map(|i| CardIdx(i as u32)).find(|&c| g2.obj(c).zone == Zone::FaceDown).unwrap();
    let stp = g2.card(0, "Swords to Plowshares", Zone::Hand);
    let s = bf(&g2, 0, SCARLETT);
    let t = g2.tgt(s);
    cast_targets(&mut g2, stp, &[t]);
    assert_eq!(g2.obj(s).zone, Zone::Exile);
    assert!(g2.has_option(Choice::Cast(fd)), "{}", opts(&g2));
    g2.choose(Choice::Cast(fd));
    resolve(&mut g2);
    assert_eq!(g2.obj(fd).zone, Zone::Battlefield);
}

#[test]
fn headliner_scarlett_a_land_is_played_as_the_land_drop_in_a_main_phase_cr_305_2() {
    // ruling: 36419b73
    let (mut g, c) = scarlett_exiles("Island", &["Mountain"]);
    assert!(g.has_option(Choice::Play(c)), "{}", opts(&g));
    g.choose(Choice::Play(c));
    g.drive_until(at_priority);
    assert_eq!(g.obj(c).zone, Zone::Battlefield);
    assert_eq!(g.player(0).lands_played, 1);
    // With the land drop used, no: a second Scarlett card, next turn, after
    // playing a land from hand.
    let mut g = scen()
        .battlefield(0, &[SCARLETT])
        .hand(0, &["Mountain"])
        .library(0, &["Island"; 10])
        .at(Step::Main2, 1)
        .start();
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Main1 && at_priority(g));
    let fd = (0..g.objs().len()).map(|i| CardIdx(i as u32)).find(|&c| g.obj(c).zone == Zone::FaceDown).unwrap();
    let m = g.card(0, "Mountain", Zone::Hand);
    g.choose(Choice::Play(m));
    g.drive_until(at_priority);
    assert!(!g.has_option(Choice::Play(fd)), "one land per turn");
}
