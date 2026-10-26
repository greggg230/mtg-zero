//! M8d part 2 (ENGINE.md 38): rulings about copies left after part 1 (37.7):
//! Reflection of Kiki-Jiki's token copies of a copy and of a token (707.2,
//! 707.3, 707.9b, 111.4), and the prepare cards (CR 722: Emeritus of
//! Abundance // Regrowth): prepared once, not copiable, kept through a loss
//! of abilities and a change of control, the land count checked twice
//! (603.4). `ruling: <hash8>` names the ruling.

mod common;
mod rulings_util;

use engine::db::{types, AbilitySlot, Counter, Kw};
use engine::{CardIdx, Choice, DecisionKind, Game, Step, Zone};
use rulings_util::*;

const FABLE: &str = "Fable of the Mirror-Breaker // Reflection of Kiki-Jiki";
const EMERITUS: &str = "Emeritus of Abundance // Regrowth";
const SAHEELI: &str = "Saheeli, Sublime Artificer";
const OKO: &str = "Oko, Thief of Crowns";

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

/// Reflection of Kiki-Jiki ({1}, {T}) copies `target`; returns the token.
fn reflect(g: &mut Game, target: CardIdx) -> CardIdx {
    let r = bf(g, 0, "Reflection of Kiki-Jiki");
    let before: Vec<CardIdx> = g.battlefield().to_vec();
    g.choose(Choice::Ability(r, AbilitySlot::Activated(0)));
    let t = g.tgt(target);
    answer_targets(g, &[t]);
    let made: Vec<CardIdx> = g.battlefield().iter().copied().filter(|c| !before.contains(c) && g.is_token(*c)).collect();
    assert_eq!(made.len(), 1, "{}", log(g));
    made[0]
}

#[test]
fn reflection_copying_a_metamorph_copying_a_bear_makes_that_bear_cr_707_9b() {
    // ruling: b30727cf
    // Phyrexian Metamorph entered as a copy of a Test Bear ("except it's an
    // artifact in addition"): the Reflection's token uses the Metamorph's
    // copiable values: a Test Bear that is an artifact, with haste.
    let mut g = scen()
        .battlefield(0, &[FABLE, "Mountain", "Test Bear"])
        .battlefield(0, &["Island"; 4])
        .transformed(0, FABLE)
        .hand(0, &["Phyrexian Metamorph"])
        .start();
    let bear = bf(&g, 0, "Test Bear");
    let m = g.card(0, "Phyrexian Metamorph", Zone::Hand);
    g.choose(Choice::Cast(m));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChoosePermanent) => Some(Choice::Card(bear)),
        Some(DecisionKind::PayPhyrexian) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| c == Choice::Number(0)),
        _ => None,
    });
    assert_eq!(g.name(m), "Test Bear");
    let tok = reflect(&mut g, m);
    assert_eq!(g.name(tok), "Test Bear");
    assert!(g.is_type(tok, types::ARTIFACT), "the Metamorph's exception is copiable");
    assert!(g.has_kw(tok, Kw::Haste));
    assert_eq!(g.pt(tok), (2, 2));
}

#[test]
fn reflection_copying_a_token_uses_what_made_it_cr_707_2_111_4() {
    // ruling: 3f6b90f2
    // Saheeli's Servo (a 1/1 colorless Servo artifact creature token).
    let mut g = scen()
        .battlefield(0, &[FABLE, "Mountain", SAHEELI, "Island"])
        .transformed(0, FABLE)
        .hand(0, &["Sol Ring"])
        .start();
    let ring = g.card(0, "Sol Ring", Zone::Hand);
    g.choose(Choice::Cast(ring));
    resolve(&mut g);
    let servo = tokens_named(&g, "Servo")[0];
    let tok = reflect(&mut g, servo);
    assert_eq!(g.name(tok), "Servo");
    assert_eq!(g.pt(tok), (1, 1));
    assert!(g.is_type(tok, types::ARTIFACT) && g.is_creature(tok));
    assert!(g.has_kw(tok, Kw::Haste));
}

#[test]
fn reflection_copies_neither_counters_nor_attachments_nor_tapped_state_cr_707_2() {
    // ruling: 40a8e390
    // A Test Bear with two +1/+1 counters, equipped with Skullclamp: the
    // token is an untapped, unequipped 2/2 without counters.
    let mut g = scen()
        .battlefield(0, &[FABLE, "Mountain", "Mountain", "Test Bear", "Skullclamp"])
        .counters(0, "Test Bear", Counter::P1P1, 2)
        .transformed(0, FABLE)
        .start();
    let bear = bf(&g, 0, "Test Bear");
    let clamp = bf(&g, 0, "Skullclamp");
    g.choose(Choice::Ability(clamp, AbilitySlot::Activated(0)));
    let t = g.tgt(bear);
    answer_targets(&mut g, &[t]);
    assert_eq!(g.obj(clamp).attached.map(|h| h.idx), Some(bear));
    assert_eq!(g.pt(bear), (5, 3));
    let tok = reflect(&mut g, bear);
    assert_eq!(g.pt(tok), (2, 2), "{}", log(&g));
    assert!(g.counters_of(tok).is_empty());
    assert!(!g.obj(tok).tapped);
    assert_eq!(g.obj(clamp).attached.map(|h| h.idx), Some(bear));
}

#[test]
fn reflections_token_has_the_copied_creatures_enter_ability_cr_707_4_603_6a() {
    // ruling: 7409fd1a
    // Test May Draw ("When this enters, you may draw a card"): the token's
    // own enter trigger.
    let mut g = scen()
        .battlefield(0, &[FABLE, "Mountain", "Test May Draw"])
        .transformed(0, FABLE)
        .start();
    let md = bf(&g, 0, "Test May Draw");
    let hand = g.player(0).hand.len();
    let r = bf(&g, 0, "Reflection of Kiki-Jiki");
    g.choose(Choice::Ability(r, AbilitySlot::Activated(0)));
    let t = g.tgt(md);
    let i = std::cell::Cell::new(false);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if !i.get() => {
            i.set(true);
            Some(t)
        }
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        _ => None,
    });
    assert_eq!(tokens_named(&g, "Test May Draw").len(), 1);
    assert_eq!(g.player(0).hand.len(), hand + 1, "{}", log(&g));
}

// ---- Emeritus of Abundance // Regrowth (prepare, CR 722) --------------------------------------

/// The Regrowth copies in exile.
fn regrowths(g: &Game) -> Vec<CardIdx> {
    g.exile_zone().iter().copied().filter(|&c| g.is_copy_obj(c) && g.name(c) == "Regrowth").collect()
}

/// P0 casts Emeritus (it enters prepared); returns it.
fn cast_emeritus(g: &mut Game) -> CardIdx {
    let e = g.card(0, EMERITUS, Zone::Hand);
    g.choose(Choice::Cast(e));
    resolve(g);
    assert!(g.is_prepared(e));
    e
}

fn attack_with(g: &mut Game, c: CardIdx) {
    quiet_until(g, |g| g.kind() == Some(DecisionKind::DeclareAttackers) && g.turn().active == 0);
    g.choose(Choice::Card(c));
    g.choose(Choice::Pass);
}

#[test]
fn emeritus_already_prepared_isnt_prepared_again_cr_722_3a() {
    // ruling: 912d01f2
    // Prepared as it entered; next turn it attacks with eight lands: its
    // trigger makes no second Regrowth.
    let mut g = scen().battlefield(0, &["Forest"; 8]).hand(0, &[EMERITUS]).start();
    let e = cast_emeritus(&mut g);
    assert_eq!(regrowths(&g).len(), 1);
    let t = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number == t + 2);
    attack_with(&mut g, e);
    resolve(&mut g);
    assert!(g.is_prepared(e));
    assert_eq!(regrowths(&g).len(), 1, "{}", log(&g));
}

#[test]
fn being_prepared_isnt_copiable_cr_722_2b_707_2() {
    // ruling: 30549a2b
    // Saheeli's -2 makes P0's Sol Ring a copy of the prepared Emeritus: the
    // Sol Ring doesn't enter, so it isn't prepared (and no second Regrowth).
    let mut g = scen().battlefield(0, &["Forest"; 3]).battlefield(0, &[SAHEELI, "Sol Ring"]).hand(0, &[EMERITUS]).start();
    let e = cast_emeritus(&mut g);
    let sa = bf(&g, 0, SAHEELI);
    let ring = bf(&g, 0, "Sol Ring");
    g.choose(Choice::Ability(sa, AbilitySlot::Activated(0)));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(g.tgt(ring)) => Some(g.tgt(ring)),
        Some(DecisionKind::Target) => Some(g.tgt(e)),
        _ => None,
    });
    assert_eq!(g.name(ring), g.name(e), "{}", log(&g));
    assert!(!g.is_prepared(ring));
    assert!(g.is_prepared(e));
    assert_eq!(regrowths(&g).len(), 1);
}

#[test]
fn emeritus_losing_all_abilities_stays_prepared_cr_722_3a_613_1f() {
    // ruling: f028f400 b775049c
    // Oko's +1: "Target artifact or creature loses all abilities and
    // becomes a green Elk creature with base power and toughness 3/3." The
    // Elk is still prepared and its Regrowth still castable.
    let mut g = scen()
        .battlefield(0, &["Forest"; 5])
        .battlefield(0, &[OKO])
        .graveyard(0, &["Lightning Bolt"])
        .hand(0, &[EMERITUS])
        .start();
    let e = cast_emeritus(&mut g);
    let oko = bf(&g, 0, OKO);
    g.choose(Choice::Ability(oko, AbilitySlot::Activated(1)));
    let t = g.tgt(e);
    answer_targets(&mut g, &[t]);
    assert_eq!(g.pt(e), (3, 3), "an Elk\n{}", log(&g));
    assert!(!g.has_kw(e, Kw::Vigilance), "lost its abilities");
    assert!(g.is_prepared(e));
    let copy = regrowths(&g)[0];
    assert!(g.has_option(Choice::Cast(copy)), "{}", opts(&g));
}

#[test]
fn emeritus_the_land_count_is_checked_again_on_resolution_cr_603_4() {
    // ruling: 41772f5d
    // Eight lands as it attacks (one a Strip Mine); with the trigger on the
    // stack P0 strips its own Forest: six lands, the trigger does nothing.
    let mut g = scen().battlefield(0, &[EMERITUS]).battlefield(0, &["Forest"; 7]).battlefield(0, &["Strip Mine"]).start();
    let e = bf(&g, 0, EMERITUS);
    attack_with(&mut g, e);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 0);
    assert_eq!(g.stack().len(), 1, "it triggered with eight lands\n{}", log(&g));
    let sm = bf(&g, 0, "Strip Mine");
    let f = bf(&g, 0, "Forest");
    // (Its mana ability isn't offered at priority, CR 605.3a.)
    let a = (0..4u8).map(|i| Choice::Ability(sm, AbilitySlot::Activated(i))).find(|&a| g.has_option(a));
    g.choose(a.unwrap_or_else(|| panic!("{}", opts(&g))));
    let t = g.tgt(f);
    answer_targets(&mut g, &[t]);
    assert_eq!(g.lands_of(0).count(), 6);
    assert!(!g.is_prepared(e), "{}", log(&g));
    assert!(regrowths(&g).is_empty());
}

#[test]
fn only_the_prepared_creatures_controller_may_cast_its_copy_cr_722_3c() {
    // ruling: 341b5cd9
    // P1's Oko exchanges its Test Bear for P0's prepared Emeritus (power 3):
    // P1, its controller now, may cast the Regrowth copy.
    let mut g = scen()
        .battlefield(0, &["Forest"; 3])
        .battlefield(1, &[OKO, "Test Bear", "Forest", "Forest"])
        .counters(1, OKO, Counter::Loyalty, 6)
        .graveyard(1, &["Lightning Bolt"])
        .hand(0, &[EMERITUS])
        .start();
    let e = cast_emeritus(&mut g);
    let copy = regrowths(&g)[0];
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
    let oko = bf(&g, 1, OKO);
    let bear = bf(&g, 1, "Test Bear");
    assert!(g.has_option(Choice::Ability(oko, AbilitySlot::Activated(2))), "power {} ctrl {} loyalty {}: {}", g.power(e), g.obj(e).controller, g.counters_on(oko, Counter::Loyalty), opts(&g));
    g.choose(Choice::Ability(oko, AbilitySlot::Activated(2)));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(g.tgt(bear)) => Some(g.tgt(bear)),
        Some(DecisionKind::Target) => Some(g.tgt(e)),
        _ => None,
    });
    assert_eq!(g.obj(e).controller, 1, "{}", log(&g));
    assert!(g.is_prepared(e));
    assert!(g.has_option(Choice::Cast(copy)), "{}", opts(&g));
    let bolt = g.card(1, "Lightning Bolt", Zone::Graveyard);
    g.choose(Choice::Cast(copy));
    let t = g.tgt(bolt);
    answer_targets(&mut g, &[t]);
    assert_eq!(g.obj(bolt).zone, Zone::Hand);
    assert!(!g.is_prepared(e));
}
