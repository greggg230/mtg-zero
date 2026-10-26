//! M8d (ENGINE.md 37): the official rulings of the cube's copy cards
//! (Phyrexian Metamorph, Superior Spider-Man, Saheeli, Sublime Artificer,
//! Shifting Woodland, Reflection of Kiki-Jiki) as engine tests: copiable
//! values and layer 1 (CR 707.2, 613.2), the copy exceptions (707.9b), X in
//! a copied cost (107.3m), "as enters" and "enters with" abilities of a copy
//! (614.1c, 707.5), no enter abilities on becoming a copy (707.4), effects
//! and counters that keep applying (613.1, 122), summoning sickness (302.6),
//! Equipment that stops or starts being one (704.5n, 704.5p). The comment
//! `ruling: <hash8>` names the ruling (docs/rulings.md).

mod common;
mod rulings_util;

use engine::db::{types, AbilitySlot, Counter, Kw};
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};
use rulings_util::*;

const METAMORPH: &str = "Phyrexian Metamorph";
const SSM: &str = "Superior Spider-Man";
const SAHEELI: &str = "Saheeli, Sublime Artificer";
const WOODLAND: &str = "Shifting Woodland";
const FABLE: &str = "Fable of the Mirror-Breaker // Reflection of Kiki-Jiki";

/// Cast the Metamorph from `p`'s hand, choosing `choice` to copy.
fn cast_metamorph(g: &mut Game, p: PlayerId, choice: Choice) -> CardIdx {
    let m = g.card(p, METAMORPH, Zone::Hand);
    g.choose(Choice::Cast(m));
    resolve_with(g, |g| (g.kind() == Some(DecisionKind::ChoosePermanent)).then_some(choice));
    m
}

/// Saheeli's -2: `artifact` becomes a copy of `other`.
fn saheeli_copy(g: &mut Game, artifact: CardIdx, other: CardIdx) {
    let sa = bf(g, 0, SAHEELI);
    g.choose(Choice::Ability(sa, AbilitySlot::Activated(0)));
    answer_targets(g, &[g.tgt(artifact), g.tgt(other)]);
}

/// Activate Shifting Woodland's delirium ability on `card` (paying with
/// the other lands).
fn woodland_copy(g: &mut Game, card: CardIdx) {
    let w = bf(g, 0, WOODLAND);
    let slot = (0..g.def_of(w).activated.len() as u8)
        .find(|&i| g.has_option(Choice::Ability(w, AbilitySlot::Activated(i))))
        .expect("delirium ability offered");
    g.choose(Choice::Ability(w, AbilitySlot::Activated(slot)));
    resolve_with(g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(g.tgt(card)),
        Some(DecisionKind::PayMana) => g
            .pending_decision()
            .unwrap()
            .options
            .iter()
            .find(|o| !matches!(o.choice, Choice::Ability(c, _) if c == w))
            .map(|o| o.choice),
        _ => None,
    });
}

const DELIRIUM: [&str; 4] = ["Lightning Bolt", "Mox Pearl", "Island", "Griselbrand"];

// ---- Phyrexian Metamorph -------------------------------------------------------------------

#[test]
fn metamorph_copying_walking_ballista_has_x_zero_and_its_enters_with_applies_cr_107_3m_614_1c() {
    // ruling: 168bc465 08e79b95
    // "If the chosen permanent has {X} in its mana cost, X is considered to
    // be zero"; its "enters with" ability works: Walking Ballista's "enters
    // with X +1/+1 counters" puts none on the copy, a 0/0 that dies.
    let mut g = scen()
        .battlefield(0, &["Island"; 4])
        .battlefield(1, &["Walking Ballista"])
        .counters(1, "Walking Ballista", Counter::P1P1, 3)
        .hand(0, &[METAMORPH])
        .start();
    let wb = bf(&g, 1, "Walking Ballista");
    let m = cast_metamorph(&mut g, 0, Choice::Card(wb));
    assert_eq!(g.obj(m).zone, Zone::Graveyard, "a 0/0 copy dies (CR 704.5f)\n{}", g.log_lines().join("\n"));
    assert_eq!(g.counters_on(wb, Counter::P1P1), 3);
}

#[test]
fn metamorph_doesnt_copy_counters_or_noncopy_effects_cr_707_2() {
    // ruling: 08bf5ec5
    // A Test Bear with two +1/+1 counters and protection from green (Mother
    // of Runes): the copy is a plain 2/2 with no counters and no protection.
    let mut g = scen()
        .battlefield(0, &["Island"; 4])
        .battlefield(0, &["Mother of Runes", "Test Bear"])
        .counters(0, "Test Bear", Counter::P1P1, 2)
        .hand(0, &[METAMORPH])
        .start();
    let mom = bf(&g, 0, "Mother of Runes");
    let bear = bf(&g, 0, "Test Bear");
    g.choose(Choice::Ability(mom, AbilitySlot::Activated(0)));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(g.tgt(bear)),
        Some(DecisionKind::ChooseColor) => Some(Choice::Number(4)),
        _ => None,
    });
    assert_ne!(g.protection_of(bear).colors, 0);
    assert!(g.obj(mom).tapped);
    let m = cast_metamorph(&mut g, 0, Choice::Card(bear));
    assert_eq!(g.name(m), "Test Bear");
    assert_eq!(g.pt(m), (2, 2));
    assert!(g.counters_of(m).is_empty());
    assert_eq!(g.protection_of(m).colors, 0);
    // Copying Mother of Runes, tapped, gives an untapped Mother.
    let mut g = scen().battlefield(0, &["Island"; 4]).battlefield(0, &["Mother of Runes", "Test Bear"]).hand(0, &[METAMORPH]).start();
    let mom = bf(&g, 0, "Mother of Runes");
    let bear = bf(&g, 0, "Test Bear");
    g.choose(Choice::Ability(mom, AbilitySlot::Activated(0)));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(g.tgt(bear)),
        Some(DecisionKind::ChooseColor) => Some(Choice::Number(4)),
        _ => None,
    });
    let m = cast_metamorph(&mut g, 0, Choice::Card(mom));
    assert_eq!(g.name(m), "Mother of Runes");
    assert!(g.obj(mom).tapped && !g.obj(m).tapped);
}

#[test]
fn metamorph_copying_a_noncreature_artifact_isnt_a_creature_cr_707_2() {
    // ruling: 7fcaae91
    let mut g = scen().battlefield(0, &["Island"; 4]).battlefield(1, &["Sol Ring"]).hand(0, &[METAMORPH]).start();
    let ring = bf(&g, 1, "Sol Ring");
    let m = cast_metamorph(&mut g, 0, Choice::Card(ring));
    assert_eq!(g.obj(m).zone, Zone::Battlefield);
    assert_eq!(g.name(m), "Sol Ring");
    assert!(!g.is_creature(m) && g.is_type(m, types::ARTIFACT));
    assert_eq!(g.obj(m).controller, 0);
}

#[test]
fn metamorph_copying_a_token_copies_the_token_and_isnt_a_token_cr_707_2_111_10() {
    // ruling: 2b2b774b
    // A Clue token (from Thraben Inspector): the Metamorph is a Clue, with
    // "{2}, Sacrifice this artifact: Draw a card.", and not a token.
    let mut g = scen()
        .battlefield(0, &["Plains"])
        .battlefield(0, &["Island"; 4])
        .hand(0, &["Thraben Inspector", METAMORPH])
        .start();
    let insp = g.card(0, "Thraben Inspector", Zone::Hand);
    g.choose(Choice::Cast(insp));
    resolve(&mut g);
    let clue = tokens_named(&g, "Clue")[0];
    let m = cast_metamorph(&mut g, 0, Choice::Card(clue));
    assert_eq!(g.name(m), "Clue");
    assert!(!g.is_token(m), "the Metamorph is not a token");
    assert!(!g.is_creature(m) && g.is_type(m, types::ARTIFACT));
    assert_eq!(g.def_of(m).activated.len(), g.def_of(clue).activated.len());
}

// ---- Superior Spider-Man ----------------------------------------------------------------------

#[test]
fn superior_spider_man_copying_walking_ballista_has_x_zero_cr_107_3m() {
    // ruling: ce685c04 1a9b0f9a
    // "If the copied card has {X} in its mana cost, X is 0": Walking
    // Ballista's "enters with X +1/+1 counters" works and puts none; he is a
    // 4/4 by the exception.
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Swamp", "Swamp"])
        .graveyard(1, &["Walking Ballista"])
        .hand(0, &[SSM])
        .start();
    let s = g.card(0, SSM, Zone::Hand);
    let wb = g.card(1, "Walking Ballista", Zone::Graveyard);
    g.choose(Choice::Cast(s));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::ChooseGraveyardCard)).then_some(Choice::Card(wb)));
    assert_eq!(g.obj(s).zone, Zone::Battlefield);
    assert_eq!(g.counters_on(s, Counter::P1P1), 0);
    assert_eq!(g.pt(s), (4, 4));
    assert!(g.is_type(s, types::ARTIFACT));
    assert_eq!(g.obj(wb).zone, Zone::Exile);
}

#[test]
fn superior_spider_man_copys_when_enters_ability_triggers_cr_707_5() {
    // ruling: 1a9b0f9a
    // A copy of Thraben Inspector: its "When this creature enters,
    // investigate" triggers.
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Swamp", "Swamp"])
        .graveyard(0, &["Thraben Inspector"])
        .hand(0, &[SSM])
        .start();
    let s = g.card(0, SSM, Zone::Hand);
    let insp = g.card(0, "Thraben Inspector", Zone::Graveyard);
    g.choose(Choice::Cast(s));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::ChooseGraveyardCard)).then_some(Choice::Card(insp)));
    assert_eq!(g.name(s), SSM);
    assert_eq!(g.count_in(0, "Clue", Zone::Battlefield), 1);
}

// ---- Saheeli, Sublime Artificer -----------------------------------------------------------

#[test]
fn saheeli_an_effect_applied_before_the_copy_keeps_applying_cr_613_1() {
    // ruling: 4b54b70b
    // Haywire Mite given protection from green by Mother of Runes, then made
    // a copy of a Test Bear: still protection from green (layer 1 doesn't
    // remove a layer-6 effect).
    let mut g = scen().battlefield(0, &[SAHEELI, "Mother of Runes", "Haywire Mite", "Test Bear"]).start();
    let mom = bf(&g, 0, "Mother of Runes");
    let mite = bf(&g, 0, "Haywire Mite");
    let bear = bf(&g, 0, "Test Bear");
    g.choose(Choice::Ability(mom, AbilitySlot::Activated(0)));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(g.tgt(mite)),
        Some(DecisionKind::ChooseColor) => Some(Choice::Number(4)),
        _ => None,
    });
    let before = g.protection_of(mite).colors;
    assert_ne!(before, 0);
    saheeli_copy(&mut g, mite, bear);
    assert_eq!(g.name(mite), "Test Bear");
    assert_eq!(g.protection_of(mite).colors, before);
}

#[test]
fn saheeli_an_artifact_that_entered_this_turn_and_becomes_a_creature_cant_attack_cr_302_6() {
    // ruling: 0fa188fc
    let mut g = scen()
        .battlefield(0, &[SAHEELI, "Island", "Test Bear"])
        .hand(0, &["Sol Ring"])
        .at(Step::Main1, 0)
        .start();
    let ring = g.card(0, "Sol Ring", Zone::Hand);
    g.choose(Choice::Cast(ring));
    resolve(&mut g);
    let bear = bf(&g, 0, "Test Bear");
    // (The Servo from casting Sol Ring is there too.)
    saheeli_copy(&mut g, ring, bear);
    assert!(g.is_creature(ring));
    assert!(g.is_sick(ring));
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    assert!(!g.has_option(Choice::Card(ring)), "summoning sick: {}", opts(&g));
    assert!(g.has_option(Choice::Card(bear)));
}

#[test]
fn saheelis_servo_trigger_resolves_even_if_the_spell_is_countered_cr_603_2() {
    // ruling: fac8c461
    let mut g = scen()
        .battlefield(0, &[SAHEELI, "Island"])
        .battlefield(1, &["Island", "Island"])
        .hand(0, &["Sol Ring"])
        .hand(1, &["Counterspell"])
        .at(Step::Main1, 0)
        .start();
    let ring = g.card(0, "Sol Ring", Zone::Hand);
    let cs = g.card(1, "Counterspell", Zone::Hand);
    g.choose(Choice::Cast(ring));
    g.drive_until(at_priority);
    assert_eq!(g.stack().len(), 2, "the trigger above the spell");
    g.pass();
    // P1 counters Sol Ring with the trigger still on the stack.
    let sr = g.stack()[0].id;
    g.choose(Choice::Cast(cs));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|c| matches!(c, Choice::Target(engine::Target::Stack(id)) if *id == sr)),
        _ => None,
    });
    assert_eq!(g.obj(ring).zone, Zone::Graveyard);
    assert_eq!(tokens_named(&g, "Servo").len(), 1);
}

#[test]
fn saheeli_copy_of_an_animated_manland_is_a_land_not_a_creature_cr_707_2() {
    // ruling: d5edcac7
    // "it won't copy effects that made the target permanent become a
    // creature": a copy of Celestial Colonnade animated by its own ability
    // is a land (and an artifact), not a creature.
    let mut g = scen()
        .battlefield(0, &[SAHEELI, "Sol Ring", "Celestial Colonnade", "Plains", "Plains", "Plains", "Island", "Island"])
        .start();
    let ring = bf(&g, 0, "Sol Ring");
    let col = bf(&g, 0, "Celestial Colonnade");
    let slot = (0..g.def_of(col).activated.len() as u8)
        .find(|&i| g.has_option(Choice::Ability(col, AbilitySlot::Activated(i))))
        .expect("animate offered");
    g.choose(Choice::Ability(col, AbilitySlot::Activated(slot)));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::PayMana) => g
            .pending_decision()
            .unwrap()
            .options
            .iter()
            .find(|o| !matches!(o.choice, Choice::Ability(c, _) if c == col || c == ring))
            .map(|o| o.choice),
        _ => None,
    });
    assert!(g.is_creature(col), "animated");
    saheeli_copy(&mut g, ring, col);
    assert_eq!(g.name(ring), "Celestial Colonnade");
    assert!(g.is_type(ring, types::LAND) && g.is_type(ring, types::ARTIFACT));
    assert!(!g.is_creature(ring), "the animation isn't copied");
}

#[test]
fn saheelis_artifact_exception_is_copiable_cr_707_9b() {
    // ruling: 53d7ebba
    // Sol Ring made a copy of Thraben Inspector (an artifact by the
    // exception); Reflection of Kiki-Jiki's token copy of it is an artifact.
    let mut g = scen()
        .battlefield(0, &[SAHEELI, "Sol Ring", "Test Bear", "Mountain", FABLE])
        .transformed(0, FABLE)
        .start();
    let ring = bf(&g, 0, "Sol Ring");
    let bear = bf(&g, 0, "Test Bear");
    saheeli_copy(&mut g, ring, bear);
    assert!(g.is_type(ring, types::ARTIFACT) && g.is_creature(ring));
    let r = bf(&g, 0, "Reflection of Kiki-Jiki");
    g.choose(Choice::Ability(r, AbilitySlot::Activated(0)));
    let t = g.tgt(ring);
    answer_targets(&mut g, &[t]);
    let t = tokens_named(&g, "Test Bear");
    assert_eq!(t.len(), 1);
    assert!(g.is_type(t[0], types::ARTIFACT), "the exception is part of the copiable values");
    assert!(g.has_kw(t[0], Kw::Haste));
}

#[test]
fn saheeli_an_attached_equipment_made_a_copy_of_an_equipment_stays_attached_cr_704_5n() {
    // ruling: eec803f5
    // Skullclamp attached to a Test Bear becomes a copy of Lightning Greaves:
    // still an Equipment, still attached, the Bear has haste and shroud;
    // made a copy of Sol Ring instead, it becomes unattached (CR 704.5p).
    for (other, attached) in [("Lightning Greaves", true), ("Sol Ring", false)] {
        let mut g = scen().battlefield(0, &[SAHEELI, "Skullclamp", other, "Test Bear", "Island"]).start();
        let clamp = bf(&g, 0, "Skullclamp");
        let bear = bf(&g, 0, "Test Bear");
        let o = bf(&g, 0, other);
        let slot = (0..g.def_of(clamp).activated.len() as u8)
            .find(|&i| g.has_option(Choice::Ability(clamp, AbilitySlot::Activated(i))))
            .expect("equip");
        g.choose(Choice::Ability(clamp, AbilitySlot::Activated(slot)));
        let t = g.tgt(bear);
        answer_targets(&mut g, &[t]);
        assert_eq!(g.obj(clamp).attached.map(|h| h.idx), Some(bear));
        saheeli_copy(&mut g, clamp, o);
        assert_eq!(g.name(clamp), other);
        assert_eq!(g.obj(clamp).attached.is_some(), attached, "{other}");
        assert_eq!(g.has_kw(bear, Kw::Haste), attached, "{other}");
        assert_eq!(g.obj(clamp).zone, Zone::Battlefield);
    }
}

// ---- Shifting Woodland -------------------------------------------------------------------------

#[test]
fn shifting_woodland_keeps_its_counters_after_becoming_a_copy_cr_122_1() {
    // ruling: 0b4e869e
    let mut g = scen()
        .battlefield(0, &[WOODLAND, "Forest", "Forest", "Forest", "Forest"])
        .counters(0, WOODLAND, Counter::P1P1, 1)
        .graveyard(0, &DELIRIUM)
        .start();
    let w = bf(&g, 0, WOODLAND);
    let gris = g.card(0, "Griselbrand", Zone::Graveyard);
    woodland_copy(&mut g, gris);
    assert_eq!(g.name(w), "Griselbrand");
    assert_eq!(g.pt(w), (8, 8), "7/7 plus its +1/+1 counter");
}

#[test]
fn shifting_woodland_becoming_a_copy_gets_no_enter_abilities_cr_707_4() {
    // ruling: 310c47f1 cdeb5763
    // A copy of Thraben Inspector: no Clue. A copy of Walking Ballista: its
    // "enters with X counters" doesn't apply, a 0/0 that dies.
    let mut g = scen()
        .battlefield(0, &[WOODLAND, "Forest", "Forest", "Forest", "Forest"])
        .graveyard(0, &["Lightning Bolt", "Mox Pearl", "Island", "Thraben Inspector"])
        .start();
    let w = bf(&g, 0, WOODLAND);
    let insp = g.card(0, "Thraben Inspector", Zone::Graveyard);
    woodland_copy(&mut g, insp);
    assert_eq!(g.name(w), "Thraben Inspector");
    assert_eq!(g.count_in(0, "Clue", Zone::Battlefield), 0);
    let mut g = scen()
        .battlefield(0, &[WOODLAND, "Forest", "Forest", "Forest", "Forest"])
        .graveyard(0, &["Lightning Bolt", "Mox Pearl", "Island", "Walking Ballista"])
        .start();
    let w = bf(&g, 0, WOODLAND);
    let wb = g.card(0, "Walking Ballista", Zone::Graveyard);
    woodland_copy(&mut g, wb);
    assert_eq!(g.obj(w).zone, Zone::Graveyard, "a 0/0 Walking Ballista dies");
}

#[test]
fn shifting_woodland_enters_tapped_unless_you_control_a_forest_cr_614_1c() {
    // ruling: 9646e4e7
    for (others, tapped) in [(&["Island"][..], true), (&["Forest"][..], false)] {
        let mut g = scen().battlefield(0, others).hand(0, &[WOODLAND]).at(Step::Main1, 0).start();
        let w = g.card(0, WOODLAND, Zone::Hand);
        g.choose(Choice::Play(w));
        resolve(&mut g);
        assert_eq!(g.obj(w).zone, Zone::Battlefield);
        assert_eq!(g.obj(w).tapped, tapped, "{others:?}");
    }
}
