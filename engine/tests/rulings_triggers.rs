//! M8d (ENGINE.md 37): the official rulings about triggered abilities and
//! their sources as engine tests: a source gone before its "enters"
//! ability resolves (Skyclave Apparition, Portable Hole, Tidehollow
//! Sculler, Deep-Cavern Bat: CR 610.3c, 603.6), an ability whose source
//! moved (Sensei's Divining Top, 113.7a), a delayed trigger about an object
//! that left (Goryo's Vengeance, 603.7c, 400.7), a cast trigger resolving
//! first (Chandra's emblem, 603.3), Mana Drain's "next main phase" (603.7),
//! control that outlasts its source (Dack Fayden, 611.2a). `ruling: <hash8>`
//! names the ruling.

mod common;
mod rulings_util;

use engine::db::AbilitySlot;
use engine::{CardIdx, Choice, DecisionKind, Game, Step, Zone};
use rulings_util::*;

fn act(g: &Game, c: CardIdx) -> Choice {
    (0..8u8)
        .map(|i| Choice::Ability(c, AbilitySlot::Activated(i)))
        .find(|&a| g.has_option(a))
        .unwrap_or_else(|| panic!("no ability of {} offered: {}", g.name(c), opts(g)))
}

/// Cast `card` (P0) and stop when its "enters" trigger is on the stack and
/// P1 has priority, answering targets with `target`.
fn enters_trigger_waiting(g: &mut Game, card: CardIdx, target: impl Fn(&Game) -> Option<Choice>) {
    g.choose(Choice::Cast(card));
    for _ in 0..100 {
        if g.obj(card).zone == Zone::Battlefield && !g.stack().is_empty() && at_priority(g) && g.pending_decision().unwrap().player == 1 {
            return;
        }
        let d = g.pending_decision().unwrap();
        let c = match d.kind {
            DecisionKind::Target => target(g).unwrap_or(d.options[0].choice),
            DecisionKind::Priority => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    panic!("no trigger\n{}", g.log_lines().join("\n"));
}

/// P1 Bolts `c` (P1 has priority).
fn p1_bolts(g: &mut Game, c: CardIdx) {
    let b = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(c));
}

#[test]
fn skyclave_apparition_gone_before_its_enter_ability_resolves_still_exiles_and_no_token_cr_603_6() {
    // ruling: 5548984d ecf56d5c
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains"])
        .battlefield(1, &["Sol Ring", "Mountain"])
        .hand(0, &["Skyclave Apparition"])
        .hand(1, &["Lightning Bolt"])
        .start();
    let sk = g.card(0, "Skyclave Apparition", Zone::Hand);
    let ring = bf(&g, 1, "Sol Ring");
    enters_trigger_waiting(&mut g, sk, |g| g.has_option(g.tgt(ring)).then(|| g.tgt(ring)));
    p1_bolts(&mut g, sk);
    resolve(&mut g);
    assert_eq!(g.obj(sk).zone, Zone::Graveyard);
    assert_eq!(g.obj(ring).zone, Zone::Exile, "still exiled\n{}", g.log_lines().join("\n"));
    assert!(tokens_named(&g, "Illusion").is_empty(), "no exiled card as it left: no token");
}

#[test]
fn portable_hole_gone_before_its_ability_resolves_exiles_nothing_cr_610_3c() {
    // ruling: f8e77455
    let mut g = scen()
        .battlefield(0, &["Plains"])
        .battlefield(1, &["Sol Ring", "Haywire Mite", "Forest"])
        .hand(0, &["Portable Hole"])
        .start();
    let ph = g.card(0, "Portable Hole", Zone::Hand);
    let ring = bf(&g, 1, "Sol Ring");
    let mite = bf(&g, 1, "Haywire Mite");
    enters_trigger_waiting(&mut g, ph, |g| g.has_option(g.tgt(ring)).then(|| g.tgt(ring)));
    // P1: Haywire Mite exiles the Portable Hole in response.
    g.choose(act(&g, mite));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(g.tgt(ph)) => Some(g.tgt(ph)),
        _ => None,
    });
    assert_eq!(g.obj(ph).zone, Zone::Exile);
    assert_eq!(g.obj(ring).zone, Zone::Battlefield, "not exiled\n{}", g.log_lines().join("\n"));
}

#[test]
fn tidehollow_sculler_gone_first_exiles_the_card_for_good_cr_610_3c() {
    // ruling: b2f7ac62
    let mut g = scen()
        .battlefield(0, &["Plains", "Swamp"])
        .battlefield(1, &["Mountain"])
        .hand(0, &["Tidehollow Sculler"])
        .hand(1, &["Lightning Bolt", "Test Giant"])
        .start();
    let sc = g.card(0, "Tidehollow Sculler", Zone::Hand);
    let giant = g.card(1, "Test Giant", Zone::Hand);
    enters_trigger_waiting(&mut g, sc, |g| g.has_option(player(1)).then(|| player(1)));
    p1_bolts(&mut g, sc);
    resolve_with(&mut g, |g| g.has_option(Choice::Card(giant)).then_some(Choice::Card(giant)));
    assert_eq!(g.obj(sc).zone, Zone::Graveyard);
    assert_eq!(g.obj(giant).zone, Zone::Exile, "exiled\n{}", g.log_lines().join("\n"));
    quiet_until(&mut g, |g| g.turn().number == 5);
    assert_eq!(g.obj(giant).zone, Zone::Exile, "for good");
}

#[test]
fn deep_cavern_bat_gone_before_its_ability_resolves_exiles_nothing_cr_610_3c() {
    // ruling: 3eb469b4
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp"])
        .battlefield(1, &["Mountain"])
        .hand(0, &["Deep-Cavern Bat"])
        .hand(1, &["Lightning Bolt", "Test Giant"])
        .start();
    let bat = g.card(0, "Deep-Cavern Bat", Zone::Hand);
    let giant = g.card(1, "Test Giant", Zone::Hand);
    enters_trigger_waiting(&mut g, bat, |g| g.has_option(player(1)).then(|| player(1)));
    p1_bolts(&mut g, bat);
    resolve_with(&mut g, |g| g.has_option(Choice::Card(giant)).then_some(Choice::Card(giant)));
    assert_eq!(g.obj(bat).zone, Zone::Graveyard);
    assert_eq!(g.obj(giant).zone, Zone::Hand, "not exiled\n{}", g.log_lines().join("\n"));
}

#[test]
fn senseis_top_gone_while_its_draw_ability_waits_you_still_draw_and_it_stays_cr_113_7a() {
    // ruling: b5ec1048
    let mut g = scen().battlefield(0, &["Sensei's Divining Top"]).battlefield(1, &["Haywire Mite", "Forest"]).start();
    let top = bf(&g, 0, "Sensei's Divining Top");
    let mite = bf(&g, 1, "Haywire Mite");
    let hand = g.player(0).hand.len();
    // "{T}: Draw a card, then put this artifact on top of its owner's library."
    let draw = (0..2u8).map(|i| Choice::Ability(top, AbilitySlot::Activated(i))).find(|&a| g.has_option(a) && i_is_tap(&g, top, a)).unwrap();
    g.choose(draw);
    g.drive_until(at_priority);
    g.pass();
    g.choose(act(&g, mite));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(g.tgt(top)) => Some(g.tgt(top)),
        _ => None,
    });
    assert_eq!(g.obj(top).zone, Zone::Exile, "it stays where it went\n{}", g.log_lines().join("\n"));
    assert_eq!(g.player(0).hand.len(), hand + 1, "drew");
}

/// The Top's tap ability: the one whose cost taps it.
fn i_is_tap(g: &Game, top: CardIdx, a: Choice) -> bool {
    let Choice::Ability(_, AbilitySlot::Activated(i)) = a else { return false };
    g.def_of(top).activated[i as usize].cost.tap
}

#[test]
fn goryos_vengeance_a_returned_creature_that_left_stays_where_it_is_cr_603_7c() {
    // ruling: 0e4f61cd
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp", "Goblin Bombardment"])
        .graveyard(0, &["Griselbrand"])
        .hand(0, &["Goryo's Vengeance"])
        .start();
    let gv = g.card(0, "Goryo's Vengeance", Zone::Hand);
    let gr = g.card(0, "Griselbrand", Zone::Graveyard);
    let t = g.tgt(gr);
    cast_targets(&mut g, gv, &[t]);
    assert_eq!(g.obj(gr).zone, Zone::Battlefield);
    let gb = bf(&g, 0, "Goblin Bombardment");
    g.choose(act(&g, gb));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Sacrifice) => Some(Choice::Card(gr)),
        Some(DecisionKind::Target) => Some(player(1)),
        _ => None,
    });
    assert_eq!(g.obj(gr).zone, Zone::Graveyard);
    quiet_until(&mut g, |g| g.turn().number == 4);
    assert_eq!(g.obj(gr).zone, Zone::Graveyard, "not exiled at the end step");
}

#[test]
fn chandras_emblem_trigger_resolves_even_if_the_spell_is_countered_cr_603_3() {
    // ruling: 3c21533f
    let mut g = scen()
        .battlefield(0, &["Mountain"])
        .battlefield(1, &["Island", "Island"])
        .emblem(0, "Chandra, Torch of Defiance")
        .hand(0, &["Lightning Bolt"])
        .hand(1, &["Counterspell"])
        .start();
    let b = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b));
    // Bolt at P1; the emblem's 5 at P1 too.
    for _ in 0..20 {
        if at_priority(&g) {
            break;
        }
        let d = g.pending_decision().unwrap();
        let c = if d.kind == DecisionKind::Target && g.has_option(player(1)) { player(1) } else { d.options[0].choice };
        g.choose(c);
    }
    for _ in 0..20 {
        if at_priority(&g) && g.stack().len() == 2 {
            break;
        }
        let d = g.pending_decision().unwrap();
        let c = if d.kind == DecisionKind::Target && g.has_option(player(1)) { player(1) } else { d.options[0].choice };
        g.choose(c);
    }
    assert_eq!(g.stack().len(), 2, "the Bolt and the emblem's trigger above it");
    g.pass();
    let cs = g.card(1, "Counterspell", Zone::Hand);
    g.choose(Choice::Cast(cs));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => {
            let i = g.options_desc().iter().position(|d| d.contains("Lightning Bolt"))?;
            Some(g.pending_decision().unwrap().options[i].choice)
        }
        _ => None,
    });
    assert_eq!(g.obj(b).zone, Zone::Graveyard);
    assert_eq!(g.life(1), 15, "the emblem's 5 only\n{}", g.log_lines().join("\n"));
}

#[test]
fn mana_drain_cast_in_your_precombat_main_phase_gives_the_mana_in_the_postcombat_one_cr_505_1() {
    // ruling: a957cfa5
    let mut g = scen()
        .battlefield(0, &["Island", "Island"])
        .battlefield(1, &["Mountain"])
        .hand(0, &["Mana Drain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    g.pass();
    let b = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(player(0));
    g.drive_until(at_priority);
    g.pass(); // P1 passes; P0 has priority
    let md = g.card(0, "Mana Drain", Zone::Hand);
    g.choose(Choice::Cast(md));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => {
            let i = g.options_desc().iter().position(|d| d.contains("Lightning Bolt"))?;
            Some(g.pending_decision().unwrap().options[i].choice)
        }
        _ => None,
    });
    assert_eq!(g.life(0), 20);
    assert_eq!(g.turn().step, Step::Main1);
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert_eq!(g.stack().len(), 1, "the delayed trigger, at the postcombat main phase");
    resolve(&mut g);
    assert_eq!(g.turn().step, Step::Main2);
    assert_eq!(g.player(0).pool.amt[5], 1, "{{C}} for the Bolt's mana value, this turn\n{}", g.log_lines().join("\n"));
}

#[test]
fn dacks_control_change_outlasts_dack_cr_611_2a() {
    // ruling: 24e0da47
    let mut g = scen()
        .battlefield(0, &["Dack Fayden"])
        .battlefield(1, &["Sol Ring", "Mountain"])
        .hand(1, &["Lightning Bolt"])
        .start();
    let dack = bf(&g, 0, "Dack Fayden");
    let ring = bf(&g, 1, "Sol Ring");
    g.choose(Choice::Ability(dack, AbilitySlot::Activated(1)));
    let t = g.tgt(ring);
    answer_targets(&mut g, &[t]);
    assert_eq!(g.obj(ring).controller, 0);
    // P1 Bolts Dack (one loyalty left) on its turn.
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
    let b = g.card(1, "Lightning Bolt", Zone::Hand);
    let t = g.tgt(dack);
    cast_targets(&mut g, b, &[t]);
    assert_eq!(g.obj(dack).zone, Zone::Graveyard);
    assert_eq!(g.obj(ring).controller, 0, "indefinitely");
}
