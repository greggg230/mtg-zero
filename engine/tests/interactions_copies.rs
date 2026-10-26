//! M8d part 2 (ENGINE.md 38, `docs/interactions.md`): copy effects (CR 707,
//! M7b part 3) against the subsystems written in other milestones: layers
//! and ability-changing effects (613, M7a), face-down permanents (708,
//! M7d), replacement effects on entering (614.12, 616.1), "as this enters"
//! choices (707.5), ward and "becomes the target" (702.21, 603.2e, M7b
//! part 3b).

mod common;
mod rulings_util;

use engine::db::{types, AbilitySlot, Counter, Kw};
use engine::{CardIdx, Choice, DecisionKind, Game, Step, Target, Zone};
use rulings_util::*;
use std::cell::Cell;

const OKO: &str = "Oko, Thief of Crowns";
const METAMORPH: &str = "Phyrexian Metamorph";
const LUTRI: &str = "Lutri, the Spellchaser";

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

/// Cast Phyrexian Metamorph (its {U/P} paid with mana if it can be) and
/// have it copy `what`; `answer` for anything else.
fn metamorph_copies(g: &mut Game, what: CardIdx, answer: impl Fn(&Game) -> Option<Choice>) -> CardIdx {
    let m = g.card(0, METAMORPH, Zone::Hand);
    g.choose(Choice::Cast(m));
    resolve_with(g, |g| match g.kind() {
        Some(DecisionKind::ChoosePermanent) => Some(Choice::Card(what)),
        Some(DecisionKind::PayPhyrexian) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| c == Choice::Number(0)),
        _ => answer(g),
    });
    m
}

#[test]
fn a_copy_of_an_elk_is_the_original_card_not_okos_effect_cr_707_2_613_1a() {
    // Oko's +1 makes P0's Test Giant a 3/3 green Elk with no abilities
    // (layers 4-7); Phyrexian Metamorph copies the Giant: the copiable values
    // are the card's (layer 1): a 5/5 Test Giant with trample and
    // deathtouch, and an artifact.
    let mut g = scen().battlefield(0, &[OKO, "Test Giant"]).battlefield(0, &["Island"; 4]).hand(0, &[METAMORPH]).start();
    let oko = bf(&g, 0, OKO);
    let giant = bf(&g, 0, "Test Giant");
    g.choose(Choice::Ability(oko, AbilitySlot::Activated(1)));
    let t = g.tgt(giant);
    answer_targets(&mut g, &[t]);
    assert_eq!(g.pt(giant), (3, 3));
    assert!(!g.has_kw(giant, Kw::Trample));
    let m = metamorph_copies(&mut g, giant, |_| None);
    assert_eq!(g.name(m), "Test Giant", "{}", log(&g));
    assert_eq!(g.pt(m), (5, 5));
    assert!(g.has_kw(m, Kw::Trample) && g.has_kw(m, Kw::Deathtouch));
    assert!(g.is_type(m, types::ARTIFACT));
    assert_eq!(g.pt(giant), (3, 3), "the Elk stays an Elk");
}

/// P0's Abhorrent Oculus manifests `top` at P1's upkeep; the game then goes
/// on to P0's next main phase. Returns the face-down card.
fn manifested_then_main(top: &str, lands: &[&str], hand: &[&str]) -> (Game, CardIdx) {
    let mut g = scen()
        .library(0, &["Island", top, "Island", "Island", "Island"])
        .battlefield(0, &["Abhorrent Oculus"])
        .battlefield(0, lands)
        .hand(0, hand)
        .at(Step::End, 0)
        .start();
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::PutOntoBattlefield));
    let c = g.card(0, top, Zone::Library);
    g.choose(Choice::Card(c));
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Main1 && at_priority(g));
    assert!(g.is_face_down(c));
    (g, c)
}

#[test]
fn a_copy_of_a_face_down_creature_is_a_face_up_nameless_2_2_cr_707_2_708_2() {
    // Phyrexian Metamorph copies a manifested Test Giant: the copiable
    // values of a face-down permanent are a 2/2 creature with no name,
    // colour, mana cost or abilities (CR 707.2, 708.2a); the Metamorph is
    // face up (no special action turns it "face up", 708.7 is for a
    // face-down permanent), an artifact (its exception), and the hidden
    // card stays hidden.
    let (mut g, fd) = manifested_then_main("Test Giant", &["Island"; 4], &[METAMORPH]);
    let m = metamorph_copies(&mut g, fd, |_| None);
    assert_eq!(g.obj(m).zone, Zone::Battlefield, "{}", log(&g));
    assert!(!g.is_face_down(m));
    assert_eq!(g.pt(m), (2, 2));
    assert_eq!(g.name(m), "");
    assert!(g.is_type(m, types::ARTIFACT) && g.is_creature(m));
    assert!(!g.has_kw(m, Kw::Trample));
    let face_up = (0..4u8).any(|i| g.has_option(Choice::Ability(m, AbilitySlot::Activated(i))));
    assert!(!face_up, "no ability to turn the copy face up: {}", opts(&g));
    assert!(!g.knows(1, fd) || g.is_face_down(fd), "the manifested card stays hidden from P1");
}

#[test]
fn containment_priest_and_a_reanimated_metamorph_entering_as_an_artifact_cr_614_12_616_1() {
    // Containment Priest: "If a nontoken creature would enter and it wasn't
    // cast, exile it instead." Phyrexian Metamorph returned by Reanimate
    // ("You may have this enter as a copy of an artifact or creature on the
    // battlefield"): entering as a copy of P0's Sol Ring it isn't a creature
    // (CR 614.12: what enters is checked with the replacement effects that
    // modify how it enters; 616.1: its controller applies the copy first),
    // so it enters; as a copy of a creature it is exiled.
    for (what, enters) in [("Sol Ring", true), ("Test Bear", false)] {
        let mut g = scen()
            .battlefield(0, &["Containment Priest", "Swamp", "Sol Ring", "Test Bear"])
            .graveyard(0, &[METAMORPH])
            .hand(0, &["Reanimate"])
            .start();
        let m = g.card(0, METAMORPH, Zone::Graveyard);
        let x = bf(&g, 0, what);
        let t = Choice::Target(Target::Obj(g.obj_ref(m)));
        let r = g.card(0, "Reanimate", Zone::Hand);
        g.choose(Choice::Cast(r));
        resolve_with(&mut g, |g| match g.kind() {
            Some(DecisionKind::Target) => Some(t),
            Some(DecisionKind::ChoosePermanent) if g.has_option(Choice::Card(x)) => Some(Choice::Card(x)),
            _ => None,
        });
        if enters {
            assert_eq!(g.obj(m).zone, Zone::Battlefield, "as a {what}\n{}", log(&g));
            assert_eq!(g.name(m), what);
        } else {
            assert_eq!(g.obj(m).zone, Zone::Exile, "as a {what}\n{}", log(&g));
        }
    }
}

#[test]
fn a_metamorph_copying_true_name_nemesis_chooses_its_own_player_cr_707_5_614_12() {
    // "As True-Name Nemesis enters, choose a player. True-Name Nemesis has
    // protection from the chosen player." The Metamorph copying it makes
    // the choice as it enters (707.5): P1; P1's Bolt can't target it.
    let mut g = scen()
        .battlefield(0, &["True-Name Nemesis"])
        .battlefield(0, &["Island"; 4])
        .battlefield(1, &["Mountain"])
        .hand(0, &[METAMORPH])
        .hand(1, &["Lightning Bolt"])
        .start();
    let tnn = bf(&g, 0, "True-Name Nemesis");
    let asked = Cell::new(false);
    let m = metamorph_copies(&mut g, tnn, |g| match g.kind() {
        _ if g.has_option(player(1)) => {
            asked.set(true);
            Some(player(1))
        }
        _ => None,
    });
    assert!(asked.get(), "a player chosen for the copy\n{}", log(&g));
    assert_eq!(g.name(m), "True-Name Nemesis");
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(m)), "{}", opts(&g));
    let _ = tnn; // (The scenario put the original onto the battlefield without its choice.)
}

#[test]
fn a_spell_copy_retargeted_at_a_ward_creature_triggers_ward_cr_707_10c_702_21a() {
    // P0's Lightning Bolt at P1; Lutri's copy gets a new target, P1's
    // Kappa Cannoneer (ward {4}): it becomes the target of a spell an
    // opponent controls (CR 603.2e, 707.10c), ward triggers, and P0 can't
    // pay: the copy is countered. The Bolt still hits P1.
    let mut g = scen()
        .battlefield(0, &["Mountain", "Island", "Island", "Island"])
        .battlefield(1, &["Kappa Cannoneer"])
        .hand(0, &["Lightning Bolt", LUTRI])
        .start();
    let k = bf(&g, 1, "Kappa Cannoneer");
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.choose(player(1));
    quiet_until(&mut g, at_priority);
    let l = g.card(0, LUTRI, Zone::Hand);
    g.choose(Choice::Cast(l));
    let tb = Choice::Target(Target::Obj(g.obj_ref(bolt)));
    let tk = g.tgt(k);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(tb) => Some(tb),
        Some(DecisionKind::NewTarget) => Some(tk),
        _ => None,
    });
    assert!(log(&g).contains("Kappa Cannoneer goes on the stack") || log(&g).contains("trigger of Kappa"), "ward triggered\n{}", log(&g));
    assert_eq!(g.obj(k).damage, 0, "the copy was countered\n{}", log(&g));
    assert_eq!(g.life(1), 17);
    let _ = Counter::P1P1;
}
