//! M8d (ENGINE.md 37): the official rulings of the cube's cards with more
//! than one face (modal and transforming double-faced cards, Adventures,
//! daybound) and of the cards that remove spells without countering them,
//! as engine tests: CR 712 (double-faced cards), 715 (Adventures), 726 /
//! 702.145 (day and night), 701.5 vs. a spell leaving the stack (101.2),
//! copies made by Reflection of Kiki-Jiki (707.9). `ruling: <hash8>`
//! names the ruling.

mod common;
mod rulings_util;

use engine::db::{AbilitySlot, Counter, Kw};
use engine::{Choice, DecisionKind, Step, Zone};
use rulings_util::*;

const EMRAKUL: &str = "Emrakul, the Aeons Torn";
const FABLE: &str = "Fable of the Mirror-Breaker // Reflection of Kiki-Jiki";
const LIBERATOR: &str = "Outland Liberator // Frenzied Trapbreaker";
const SMASHING: &str = "Shatterskull Smashing // Shatterskull, the Hammer Pass";
const BORROWER: &str = "Brazen Borrower // Petty Theft";

/// P0 casts Emrakul (15 Islands); returns at P1's priority with the spell
/// and its cast trigger on the stack.
fn emrakul_on_the_stack(p1_lands: &[&str], p1_hand: &[&str]) -> (engine::Game, engine::CardIdx) {
    let mut g = scen().battlefield(0, &["Island"; 15]).battlefield(1, p1_lands).hand(0, &[EMRAKUL]).hand(1, p1_hand).start();
    let e = g.card(0, EMRAKUL, Zone::Hand);
    g.choose(Choice::Cast(e));
    g.drive_until(at_priority);
    g.pass();
    assert_eq!(g.pending_decision().unwrap().player, 1);
    (g, e)
}

#[test]
fn sink_into_stupor_returns_a_spell_that_cant_be_countered_cr_101_2() {
    // ruling: 7466b29b b4e16ada
    let (mut g, e) = emrakul_on_the_stack(&["Island", "Island", "Island"], &["Sink into Stupor // Soporific Springs"]);
    let s = g.card(1, "Sink into Stupor // Soporific Springs", Zone::Hand);
    g.choose(Choice::Cast(s));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|c| matches!(c, Choice::Target(engine::Target::Stack(_)))).or_else(|| {
            let i = g.options_desc().iter().position(|d| d.contains("Emrakul"))?;
            Some(g.pending_decision().unwrap().options[i].choice)
        }),
        _ => None,
    });
    assert_eq!(g.obj(e).zone, Zone::Hand, "returned, not countered\n{}", g.log_lines().join("\n"));
    // The cast trigger still resolved: an extra turn after this one.
    let t = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number == t + 1);
    assert_eq!(g.turn().active, 0, "P0 takes the extra turn");
}

#[test]
fn aang_airbends_a_spell_that_cant_be_countered_cr_101_2() {
    // ruling: f467488f
    let (mut g, e) = emrakul_on_the_stack(&["Island", "Island", "Plains"], &["Aang, Swift Savior // Aang and La, Ocean's Fury"]);
    let a = g.card(1, "Aang, Swift Savior // Aang and La, Ocean's Fury", Zone::Hand);
    g.choose(Choice::Cast(a));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => {
            let i = g.options_desc().iter().position(|d| d.contains("Emrakul"))?;
            Some(g.pending_decision().unwrap().options[i].choice)
        }
        _ => None,
    });
    assert_eq!(g.obj(e).zone, Zone::Exile, "exiled, not countered\n{}", g.log_lines().join("\n"));
}

#[test]
fn yawgmoths_will_lets_you_play_a_modal_double_faced_card_with_either_face_cr_712_12() {
    // ruling: e5e7f31f
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp", "Swamp", "Mountain", "Mountain"])
        .graveyard(0, &[SMASHING])
        .hand(0, &["Yawgmoth's Will"])
        .start();
    let w = g.card(0, "Yawgmoth's Will", Zone::Hand);
    g.choose(Choice::Cast(w));
    resolve(&mut g);
    let s = g.card(0, SMASHING, Zone::Graveyard);
    let land = g.has_option(Choice::PlayFace(s)) || g.pending_decision().unwrap().options.iter().any(|o| matches!(o.choice, Choice::Faces(c, _) if c == s));
    assert!(land, "the land face: {}", opts(&g));
    let cast = g.has_option(Choice::Cast(s)) || g.pending_decision().unwrap().options.iter().any(|o| matches!(o.choice, Choice::Faces(c, _) if c == s));
    assert!(cast, "the spell face: {}", opts(&g));
}

// ---- daybound (CR 702.145, 726) -----------------------------------------------------------

#[test]
fn a_daybound_card_cast_at_night_enters_with_its_nightbound_face_up_cr_702_145d() {
    // ruling: 483aa586
    let mut g = scen().battlefield(0, &["Forest", "Forest"]).hand(0, &[LIBERATOR]).day(Some(false)).start();
    let l = g.card(0, LIBERATOR, Zone::Hand);
    g.choose(Choice::Cast(l));
    resolve(&mut g);
    assert_eq!(g.obj(l).zone, Zone::Battlefield);
    assert_eq!(g.name(l), "Frenzied Trapbreaker");
    assert_eq!(g.pt(l), (3, 3));
}

#[test]
fn a_daybound_card_put_onto_the_battlefield_at_night_is_nightbound_cr_702_145e() {
    // ruling: 9d7e7537
    let mut g = scen().battlefield(0, &["Swamp"]).graveyard(0, &[LIBERATOR]).hand(0, &["Reanimate"]).day(Some(false)).start();
    let r = g.card(0, "Reanimate", Zone::Hand);
    let l = g.card(0, LIBERATOR, Zone::Graveyard);
    let t = g.tgt(l);
    cast_targets(&mut g, r, &[t]);
    assert_eq!(g.obj(l).zone, Zone::Battlefield);
    assert_eq!(g.name(l), "Frenzied Trapbreaker");
    assert_eq!(g.life(0), 18, "Reanimate: the card's mana value (front face, 2)");
}

#[test]
fn it_becomes_night_after_a_turn_without_spells_and_day_after_two_cr_726_3a() {
    // ruling: b5368c51 0a3a2fe8 03d27f15
    let mut g = scen().battlefield(0, &[LIBERATOR]).day(Some(true)).start();
    let l = bf(&g, 0, LIBERATOR);
    // P0 casts nothing in turn 3: at turn 4 it is night.
    quiet_until(&mut g, |g| g.turn().number == 4 && g.turn().step == Step::Main1);
    assert_eq!(g.day(), Some(false));
    assert_eq!(g.name(l), "Frenzied Trapbreaker");
    // P1 casts two spells in turn 4: day at turn 5.
    let mut g = scen()
        .battlefield(0, &[LIBERATOR])
        .transformed(0, LIBERATOR)
        .battlefield(1, &["Mountain", "Mountain"])
        .hand(1, &["Lightning Bolt", "Lightning Bolt"])
        .library(1, &["Mountain"; 10])
        .day(Some(false))
        .at(Step::Main1, 1)
        .start();
    let l = bf(&g, 0, "Frenzied Trapbreaker");
    assert_eq!(g.name(l), "Frenzied Trapbreaker");
    for _ in 0..2 {
        let b = g.card(1, "Lightning Bolt", Zone::Hand);
        cast_targets(&mut g, b, &[player(0)]);
    }
    let t = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number == t + 1 && g.turn().step == Step::Main1);
    assert_eq!(g.day(), Some(true));
    assert!(g.name(l).starts_with("Outland Liberator"), "the front face (named as the card)");
    assert_eq!(g.pt(l), (2, 2));
}

// ---- Fable of the Mirror-Breaker // Reflection of Kiki-Jiki ----------------------------------

#[test]
fn reflection_of_kiki_jiki_copying_walking_ballista_has_x_zero_cr_107_3m() {
    // ruling: 602bf769
    let mut g = scen()
        .battlefield(0, &[FABLE, "Mountain", "Walking Ballista"])
        .counters(0, "Walking Ballista", Counter::P1P1, 2)
        .transformed(0, FABLE)
        .start();
    let r = bf(&g, 0, "Reflection of Kiki-Jiki");
    let wb = bf(&g, 0, "Walking Ballista");
    g.choose(Choice::Ability(r, AbilitySlot::Activated(0)));
    let t = g.tgt(wb);
    answer_targets(&mut g, &[t]);
    assert!(tokens_named(&g, "Walking Ballista").is_empty(), "a 0/0 token: it died\n{}", g.log_lines().join("\n"));
}

#[test]
fn a_copy_of_the_reflection_token_has_haste_and_isnt_sacrificed_cr_707_9b() {
    // ruling: 6c4f38f7
    // Phyrexian Metamorph copies the hasty token copy of a Test Bear: it has
    // haste (the exception is copiable) and stays at the end step.
    let mut g = scen()
        .battlefield(0, &[FABLE, "Mountain", "Test Bear"])
        .battlefield(0, &["Island"; 4])
        .transformed(0, FABLE)
        .hand(0, &["Phyrexian Metamorph"])
        .start();
    let r = bf(&g, 0, "Reflection of Kiki-Jiki");
    let bear = bf(&g, 0, "Test Bear");
    g.choose(Choice::Ability(r, AbilitySlot::Activated(0)));
    let t = g.tgt(bear);
    answer_targets(&mut g, &[t]);
    let tok = tokens_named(&g, "Test Bear")[0];
    let m = g.card(0, "Phyrexian Metamorph", Zone::Hand);
    g.choose(Choice::Cast(m));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::ChoosePermanent)).then_some(Choice::Card(tok)));
    assert_eq!(g.name(m), "Test Bear");
    assert!(g.has_kw(m, Kw::Haste));
    quiet_until(&mut g, |g| g.turn().number == 4);
    assert!(!g.battlefield().contains(&tok), "the token was sacrificed");
    assert_eq!(g.obj(m).zone, Zone::Battlefield, "the copy wasn't");
}

// ---- Adventures (CR 715) -------------------------------------------------------------------------

#[test]
fn a_countered_adventure_goes_to_the_graveyard_without_the_permission_cr_715_4() {
    // ruling: c5e2c49b 13492141
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Island", "Island"])
        .battlefield(1, &["Sol Ring", "Island", "Island"])
        .hand(0, &[BORROWER])
        .hand(1, &["Counterspell"])
        .start();
    let b = g.card(0, BORROWER, Zone::Hand);
    let ring = bf(&g, 1, "Sol Ring");
    let f = g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|c| matches!(c, Choice::Faces(x, _) if *x == b)).unwrap();
    g.choose(f);
    if g.kind() == Some(DecisionKind::ChooseFace) {
        let i = g.options_desc().iter().position(|d| d.contains("Petty") || d.contains("other")).unwrap_or(1);
        let c = g.pending_decision().unwrap().options[i].choice;
        g.choose(c);
    }
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(ring));
    g.drive_until(at_priority);
    g.pass();
    let cs = g.card(1, "Counterspell", Zone::Hand);
    g.choose(Choice::Cast(cs));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => {
            let i = g.options_desc().iter().position(|d| d.contains("Petty Theft") || d.contains("Brazen"))?;
            Some(g.pending_decision().unwrap().options[i].choice)
        }
        _ => None,
    });
    assert_eq!(g.obj(b).zone, Zone::Graveyard, "countered: its owner's graveyard");
    assert!(!g.has_option(Choice::Cast(b)));
    assert_eq!(g.obj(ring).zone, Zone::Battlefield);
}
