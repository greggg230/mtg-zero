//! M8d part 2 (ENGINE.md 38): the riskiest untested rulings about triggers
//! and zones: sources gone before their abilities resolve (610.3c, 603.7,
//! 608.2b: Palace Jailer, Leyline Binding, Animate Dead, Flickerwisp,
//! Shallow Grave), the new object of a creature that left (400.7), a spell
//! cast by another player than its owner (Green Sun's Zenith stolen by
//! Invert Polarity, 108.3, 608.2), "exile until" one card at a time
//! (Laelia, 701.?), triggers on copies (magecraft, 707.10), once per target
//! (Leovold, 603.2), damage from several sources at once (Screaming
//! Nemesis, 510.2, 603.2c), attacking a planeswalker (Coveted Jewel,
//! 508.1b), playing a land versus putting one (City of Traitors, 305.1),
//! becoming a creature versus entering (Celestial Colonnade, 603.6a), an
//! escape permanent that wasn't escaped (Uro), flashback exile (702.34a),
//! Oust and Unexpectedly Absent's library positions (401.4). `ruling:
//! <hash8>` names the ruling.

mod common;
mod rulings_util;

use engine::db::{AbilitySlot, Cond, Counter};
use engine::hidden::{key, KEY_COIN};
use engine::{CardIdx, Choice, DecisionKind, Game, Step, Target, Zone};
use rulings_util::*;
use std::cell::Cell;

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

/// A scenario with empty libraries (`scen()` gives each player ten lands).
fn bare() -> engine::testkit::Scenario {
    engine::testkit::Scenario::new(common::db()).spec(|s| s.auto_single = false).library(0, &["Island"; 5])
}

fn monarch(g: &Game) -> Option<u8> {
    (0..2u8).find(|&p| g.holds(&Cond::IsMonarch, p))
}

fn act(g: &Game, c: CardIdx) -> Choice {
    (0..8u8)
        .map(|i| Choice::Ability(c, AbilitySlot::Activated(i)))
        .find(|&a| g.has_option(a))
        .unwrap_or_else(|| panic!("no ability of {} offered: {}", g.name(c), opts(g)))
}

/// Cast `name` from P`p`'s hand and resolve it, answering targets with
/// `targets` in order.
fn cast_resolve(g: &mut Game, p: u8, name: &str, targets: &[Choice]) -> CardIdx {
    let c = g.card(p, name, Zone::Hand);
    cast_targets(g, c, targets);
    c
}

/// P1, at its priority (the stack not empty), casts Force of Vigor for its
/// alternative cost (exiling the green Test Bear) at `target`.
fn p1_force_of_vigor(g: &mut Game, target: CardIdx) {
    g.drive_until(|g| at_priority(g) && g.turn().priority == 1);
    let fov = g.card(1, "Force of Vigor", Zone::Hand);
    g.choose(Choice::Cast(fov));
    let t = g.tgt(target);
    let bear = g.card(1, "Test Bear", Zone::Hand);
    let targeted = Cell::new(false);
    quiet_until_with(g, |g| at_priority(g) && targeted.get(), |g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(1)),
        Some(DecisionKind::ExileCost) => Some(Choice::Card(bear)),
        Some(DecisionKind::Target) if !targeted.get() => {
            targeted.set(true);
            Some(t)
        }
        Some(DecisionKind::Target) => Some(Choice::Pass),
        _ => None,
    });
}

// ---- sources gone, linked exile (610.3c, 603.7) ---------------------------------------------

#[test]
fn palace_jailer_gone_the_creature_returns_when_an_opponent_becomes_the_monarch_cr_610_3c() {
    // ruling: e5e352da bd46894c
    // "When this creature enters, you become the monarch. When this
    // creature enters, exile up to one target creature an opponent controls
    // until an opponent becomes the monarch." The Jailer is Bolted: the
    // Bear stays exiled; P1's Test Hasty deals combat damage to P0 (the
    // monarch), P1 becomes the monarch, the Bear returns.
    let mut g = scen()
        .battlefield(0, &["Plains"; 4])
        .battlefield(0, &["Mountain"])
        .battlefield(1, &["Test Bear", "Test Hasty"])
        .hand(0, &["Palace Jailer", "Lightning Bolt"])
        .start();
    let bear = bf(&g, 1, "Test Bear");
    let jailer = g.card(0, "Palace Jailer", Zone::Hand);
    let t = g.tgt(bear);
    cast_targets(&mut g, jailer, &[t]);
    assert_eq!(g.obj(bear).zone, Zone::Exile, "{}", log(&g));
    assert_eq!(monarch(&g), Some(0));
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    let tj = g.tgt(jailer);
    cast_targets(&mut g, bolt, &[tj]);
    assert_eq!(g.obj(jailer).zone, Zone::Graveyard);
    assert_eq!(g.obj(bear).zone, Zone::Exile, "the Jailer leaving doesn't return it");
    let hasty = bf(&g, 1, "Test Hasty");
    quiet_until(&mut g, |g| g.turn().active == 1 && g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(hasty));
    g.choose(Choice::Pass);
    quiet_until(&mut g, |g| monarch(g) == Some(1) && g.stack().is_empty() && at_priority(g));
    assert_eq!(g.count_in(1, "Test Bear", Zone::Battlefield), 1, "{}", log(&g));
}

#[test]
fn palace_jailer_exiling_a_token_it_ceases_to_exist_cr_111_7() {
    // ruling: 078cc0d2 ac3bb678
    // P1's Spirit token (Lingering Souls) exiled by the Jailer is gone for
    // good: it isn't in exile, and P1 becoming the monarch returns nothing.
    let mut g = scen()
        .battlefield(0, &["Plains"; 4])
        .battlefield(1, &["Plains", "Plains", "Plains"])
        .hand(1, &["Lingering Souls"])
        .hand(0, &["Palace Jailer"])
        .at(Step::Main1, 1)
        .start();
    cast_resolve(&mut g, 1, "Lingering Souls", &[]);
    let spirits = tokens_named(&g, "Spirit");
    assert_eq!(spirits.len(), 2);
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Main1 && at_priority(g));
    let jailer = g.card(0, "Palace Jailer", Zone::Hand);
    let t = g.tgt(spirits[0]);
    cast_targets(&mut g, jailer, &[t]);
    assert!(!g.battlefield().contains(&spirits[0]));
    assert!(!g.exile_zone().contains(&spirits[0]), "a token ceases to exist (CR 111.7)");
    assert_eq!(tokens_named(&g, "Spirit").len(), 1);
}

#[test]
fn leyline_binding_gone_before_its_enter_ability_resolves_exiles_nothing_cr_610_3c() {
    // ruling: 0f86b94f
    // Domain 5: Leyline Binding costs {W}. Its trigger targets P1's Giant;
    // P1 destroys the Binding with Force of Vigor in response: the Giant
    // stays.
    let mut g = scen()
        .battlefield(0, &["Plains", "Island", "Swamp", "Mountain", "Forest"])
        .battlefield(1, &["Test Giant"])
        .hand(0, &["Leyline Binding"])
        .hand(1, &["Force of Vigor", "Test Bear"])
        .start();
    let giant = bf(&g, 1, "Test Giant");
    let lb = g.card(0, "Leyline Binding", Zone::Hand);
    g.choose(Choice::Cast(lb));
    let t = g.tgt(giant);
    quiet_until_with(&mut g, |g| g.stack().len() == 1 && g.obj(lb).zone == Zone::Battlefield && at_priority(g), |g| {
        (g.kind() == Some(DecisionKind::Target)).then_some(t)
    });
    p1_force_of_vigor(&mut g, lb);
    resolve(&mut g);
    assert_eq!(g.obj(lb).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.obj(giant).zone, Zone::Battlefield, "never exiled\n{}", log(&g));
}

#[test]
fn animate_dead_gone_before_its_trigger_resolves_returns_nothing_cr_608_2b() {
    // ruling: 2a2eab5b
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp"])
        .graveyard(0, &["Test Giant"])
        .hand(0, &["Animate Dead"])
        .hand(1, &["Force of Vigor", "Test Bear"])
        .start();
    let giant = g.card(0, "Test Giant", Zone::Graveyard);
    let ad = g.card(0, "Animate Dead", Zone::Hand);
    g.choose(Choice::Cast(ad));
    let t = Choice::Target(Target::Obj(g.obj_ref(giant)));
    quiet_until_with(&mut g, |g| g.stack().len() == 1 && g.obj(ad).zone == Zone::Battlefield && at_priority(g), |g| {
        (g.kind() == Some(DecisionKind::Target)).then_some(t)
    });
    p1_force_of_vigor(&mut g, ad);
    resolve(&mut g);
    assert_eq!(g.obj(ad).zone, Zone::Graveyard);
    assert_eq!(g.obj(giant).zone, Zone::Graveyard, "{}", log(&g));
}

#[test]
fn flickerwisp_gone_its_exiled_permanent_still_returns_at_the_end_step_cr_603_7() {
    // ruling: 2cf497b2
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains", "Mountain"])
        .battlefield(1, &["Test Bear"])
        .hand(0, &["Flickerwisp", "Lightning Bolt"])
        .start();
    let bear = bf(&g, 1, "Test Bear");
    let fw = g.card(0, "Flickerwisp", Zone::Hand);
    let t = g.tgt(bear);
    cast_targets(&mut g, fw, &[t]);
    assert_eq!(g.obj(bear).zone, Zone::Exile);
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    let tf = g.tgt(fw);
    cast_targets(&mut g, bolt, &[tf]);
    assert_eq!(g.obj(fw).zone, Zone::Graveyard);
    quiet_until(&mut g, |g| g.turn().step == Step::End && g.stack().is_empty() && at_priority(g));
    assert_eq!(g.count_in(1, "Test Bear", Zone::Battlefield), 1, "{}", log(&g));
}

#[test]
fn shallow_grave_exiles_only_the_creature_still_on_the_battlefield_cr_603_7_400_7() {
    // ruling: 58787dbc 280ed0d1
    // "Return the top creature card of your graveyard to the battlefield.
    // That creature gains haste until end of turn. Exile it at the beginning
    // of the next end step." The Giant returned, then Bolted twice... killed
    // by P0's own Fire Covenant: in the graveyard at the end step, it stays
    // there.
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp", "Swamp", "Mountain", "Mountain"])
        .graveyard(0, &["Test Bear"])
        .hand(0, &["Shallow Grave", "Fire Covenant"])
        .start();
    let bear = g.card(0, "Test Bear", Zone::Graveyard);
    cast_resolve(&mut g, 0, "Shallow Grave", &[]);
    assert_eq!(g.obj(bear).zone, Zone::Battlefield, "{}", log(&g));
    let fc = g.card(0, "Fire Covenant", Zone::Hand);
    g.choose(Choice::Cast(fc));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(2));
    g.choose(g.tgt(bear));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Pass));
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
    quiet_until(&mut g, |g| g.turn().active == 1);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard, "not exiled from the graveyard\n{}", log(&g));
}

#[test]
fn uro_put_onto_the_battlefield_is_sacrificed_and_its_enter_ability_triggers_cr_603_6a() {
    // ruling: 655d544a 1b138927
    // Reanimated (not cast, not escaped): "When Uro enters, sacrifice it
    // unless it escaped" and "Whenever Uro enters or attacks, you gain 3
    // life and draw a card, then you may put a land card from your hand
    // onto the battlefield" both trigger.
    let mut g = scen()
        .battlefield(0, &["Swamp"])
        .graveyard(0, &["Uro, Titan of Nature's Wrath"])
        .hand(0, &["Reanimate"])
        .start();
    let uro = g.card(0, "Uro, Titan of Nature's Wrath", Zone::Graveyard);
    let hand = g.player(0).hand.len();
    let t = Choice::Target(Target::Obj(g.obj_ref(uro)));
    cast_resolve(&mut g, 0, "Reanimate", &[t]);
    assert_eq!(g.obj(uro).zone, Zone::Graveyard, "sacrificed\n{}", log(&g));
    assert_eq!(g.life(0), 20 - 3 + 3, "lost 3 for Reanimate, gained 3");
    assert_eq!(g.player(0).hand.len(), hand - 1 + 1, "drew a card");
}

// ---- a spell cast by another player than its owner ------------------------------------------

#[test]
fn green_suns_zenith_stolen_its_new_controller_searches_its_owners_library_gets_it_cr_608_2c() {
    // ruling: f8e4188f
    // P0's Green Sun's Zenith (X = 2) stolen by P1's Invert Polarity: P1
    // searches P1's library for a green creature with mana value 2 or less
    // (a Test Bear, onto the battlefield under P1's control), and the
    // Zenith is shuffled into P0's library, its owner's.
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest", "Forest"])
        .battlefield(1, &["Island", "Island", "Mountain"])
        .hand(0, &["Green Sun's Zenith"])
        .hand(1, &["Invert Polarity"])
        .library(0, &["Forest"; 5])
        .library(1, &["Island", "Test Bear", "Island"])
        .start();
    let gsz = g.card(0, "Green Sun's Zenith", Zone::Hand);
    g.choose(Choice::Cast(gsz));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(2));
    quiet_until(&mut g, at_priority);
    g.pass();
    let ip = g.card(1, "Invert Polarity", Zone::Hand);
    g.choose(Choice::Cast(ip));
    let t = Choice::Target(Target::Obj(g.obj_ref(gsz)));
    quiet_until_with(&mut g, at_priority, |g| (g.kind() == Some(DecisionKind::Target)).then_some(t));
    g.pass();
    g.force_chance(&[key(KEY_COIN, 1, 1)]);
    g.pass();
    let p1_lib = g.player(1).library.len();
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::SearchLibrary) => {
            let d = g.pending_decision().unwrap();
            assert_eq!(d.player, 1, "the spell's controller searches");
            Some(Choice::Def(id("Test Bear")))
        }
        _ => None,
    });
    let bear = g.card(1, "Test Bear", Zone::Battlefield);
    assert_eq!(g.obj(bear).controller, 1, "{}", log(&g));
    assert_eq!(g.player(1).library.len(), p1_lib - 1, "P1's library: the Bear found, nothing shuffled in");
    assert_eq!(g.obj(gsz).zone, Zone::Library, "{}", log(&g));
    assert!(g.player(0).library.contains(&gsz), "shuffled into its owner's library");
}

#[test]
fn green_suns_zenith_countered_goes_to_the_graveyard_cr_701_6a() {
    // ruling: 673739a0
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest"])
        .battlefield(1, &["Island", "Island"])
        .hand(0, &["Green Sun's Zenith"])
        .hand(1, &["Counterspell"])
        .library(0, &["Test Bear", "Forest"])
        .start();
    let gsz = g.card(0, "Green Sun's Zenith", Zone::Hand);
    g.choose(Choice::Cast(gsz));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(1));
    quiet_until(&mut g, at_priority);
    g.pass();
    let cs = g.card(1, "Counterspell", Zone::Hand);
    let t = Choice::Target(Target::Obj(g.obj_ref(gsz)));
    cast_targets(&mut g, cs, &[t]);
    assert_eq!(g.obj(gsz).zone, Zone::Graveyard, "{}", log(&g));
}

// ---- triggers counted per card, per copy, per target, per event -------------------------------


// ---- triggers counted per card, per copy, per target, per event -------------------------------

#[test]
fn laelia_cascade_exiles_one_card_at_a_time_one_trigger_each_cr_702_85a_603_2c() {
    // ruling: 1178d637
    // Bloodbraid Challenger's cascade exiles the Forest, the Test Giant and
    // the Bolt from P0's library one at a time: Laelia ("Whenever one or
    // more cards are put into exile from your library and/or your
    // graveyard, put a +1/+1 counter on Laelia") triggers three times.
    let mut g = scen()
        .battlefield(0, &["Laelia, the Blade Reforged"])
        .battlefield(0, &["Mountain", "Mountain", "Mountain", "Forest", "Forest"])
        .hand(0, &["Bloodbraid Challenger"])
        .library(0, &["Forest", "Test Giant", "Lightning Bolt", "Plains"])
        .start();
    let l = bf(&g, 0, "Laelia, the Blade Reforged");
    let bb = g.card(0, "Bloodbraid Challenger", Zone::Hand);
    g.choose(Choice::Cast(bb));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::YesNo) => Some(Choice::Pass),
        _ => None,
    });
    assert_eq!(g.counters_on(l, Counter::P1P1), 3, "{}", log(&g));
}

#[test]
fn magecraft_triggers_for_a_copy_and_once_per_copy_cr_707_10_702_40a() {
    // ruling: ee14929e be6a31bb
    // Witherbloom Apprentice ("Whenever you cast or copy an instant or
    // sorcery spell, each opponent loses 1 life and you gain 1 life"): two
    // Bolts, then Brain Freeze with storm 2: three casts and two copies.
    let mut g = scen()
        .battlefield(0, &["Witherbloom Apprentice", "Mountain", "Mountain", "Island", "Island"])
        .hand(0, &["Lightning Bolt", "Lightning Bolt", "Brain Freeze"])
        .library(1, &["Forest"; 12])
        .start();
    for _ in 0..2 {
        cast_resolve(&mut g, 0, "Lightning Bolt", &[player(1)]);
    }
    assert_eq!((g.life(0), g.life(1)), (22, 12));
    let bfz = g.card(0, "Brain Freeze", Zone::Hand);
    g.choose(Choice::Cast(bfz));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(player(1)) => Some(player(1)),
        Some(DecisionKind::NewTarget) => Some(Choice::Pass),
        _ => None,
    });
    assert_eq!((g.life(0), g.life(1)), (25, 9), "{}", log(&g));
    // Lutri's copy of a Bolt: one more.
    let mut g = scen()
        .battlefield(0, &["Witherbloom Apprentice", "Mountain", "Mountain", "Island", "Island"])
        .hand(0, &["Lightning Bolt", "Lutri, the Spellchaser"])
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.choose(player(1));
    quiet_until(&mut g, at_priority);
    let l = g.card(0, "Lutri, the Spellchaser", Zone::Hand);
    g.choose(Choice::Cast(l));
    let t = Choice::Target(Target::Obj(g.obj_ref(bolt)));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(t) => Some(t),
        Some(DecisionKind::NewTarget) => Some(Choice::Pass),
        _ => None,
    });
    assert_eq!((g.life(0), g.life(1)), (22, 12), "the cast and the copy\n{}", log(&g));
}

#[test]
fn leovold_triggers_once_per_target_of_one_spell_cr_603_2e() {
    // ruling: f0765796
    // Kolaghan's Command: "Target player discards a card" at P1 and "2
    // damage to any target" at P1's Leovold: two triggers.
    let mut g = scen()
        .battlefield(0, &["Mountain", "Swamp", "Mountain"])
        .battlefield(1, &["Leovold, Emissary of Trest"])
        .hand(0, &["Kolaghan's Command"])
        .hand(1, &["Island"])
        .start();
    let leo = bf(&g, 1, "Leovold, Emissary of Trest");
    let kc = g.card(0, "Kolaghan's Command", Zone::Hand);
    g.choose(Choice::Cast(kc));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(1));
    g.choose(Choice::Number(3));
    let tl = g.tgt(leo);
    quiet_until_with(&mut g, at_priority, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(tl) => Some(tl),
        Some(DecisionKind::Target) => Some(player(1)),
        _ => None,
    });
    let draws = Cell::new(0);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::YesNo) => {
            draws.set(draws.get() + 1);
            Some(Choice::Yes)
        }
        _ => None,
    });
    assert_eq!(draws.get(), 2, "{}", log(&g));
}

#[test]
fn screaming_nemesis_dealt_damage_by_two_blockers_triggers_once_cr_603_2c_510_2() {
    // ruling: 66fc62ef ea71103f
    // "Whenever this creature is dealt damage, it deals that much damage to
    // any other target." Two Bears block it: one trigger, 4 damage (to P1),
    // although the 3/3 dies.
    let mut g = scen().battlefield(0, &["Screaming Nemesis"]).battlefield(1, &["Test Bear", "Test Bear"]).start();
    let n = bf(&g, 0, "Screaming Nemesis");
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(n));
    g.choose(Choice::Pass);
    let targets = Cell::new(0);
    quiet_until_with(&mut g, |g| g.turn().step == Step::End || g.turn().active == 1, |g| match g.kind() {
        Some(DecisionKind::DeclareBlockers) => Some(Choice::Card(n)),
        Some(DecisionKind::Target) => {
            targets.set(targets.get() + 1);
            Some(player(1))
        }
        _ => None,
    });
    assert_eq!(targets.get(), 1, "one trigger\n{}", log(&g));
    assert_eq!(g.life(1), 16, "{}", log(&g));
    assert_eq!(g.obj(n).zone, Zone::Graveyard);
}

#[test]
fn coveted_jewel_doesnt_trigger_on_an_attack_at_a_planeswalker_cr_508_1b() {
    // ruling: 8279c4b9
    let mut g = scen()
        .battlefield(0, &["Coveted Jewel", "Teferi, Time Raveler"])
        .battlefield(1, &["Test Hasty"])
        .at(Step::Main1, 1)
        .start();
    let jewel = bf(&g, 0, "Coveted Jewel");
    let tef = bf(&g, 0, "Teferi, Time Raveler");
    let h = bf(&g, 1, "Test Hasty");
    let hand = g.player(1).hand.len();
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(h));
    if g.kind() != Some(DecisionKind::AttackTarget) {
        g.choose(Choice::Pass);
    }
    assert_eq!(g.kind(), Some(DecisionKind::AttackTarget), "{}", opts(&g));
    g.choose(g.tgt(tef));
    quiet_until(&mut g, |g| g.turn().step == Step::End);
    assert_eq!(g.counters_on(tef, Counter::Loyalty), 2, "Teferi was attacked and dealt 2
{}", log(&g));
    assert_eq!(g.obj(jewel).controller, 0, "{}", log(&g));
    assert_eq!(g.player(1).hand.len(), hand);
}

#[test]
fn city_of_traitors_ignores_itself_and_a_land_put_onto_the_battlefield_cr_305_1() {
    // ruling: 119d09c6 123354f4
    // "When you play another land, sacrifice City of Traitors." Played: no
    // trigger; a fetch land's Swamp put onto the battlefield: none.
    let mut g = scen().hand(0, &["City of Traitors"]).battlefield(0, &["Polluted Delta"]).library(0, &["Swamp", "Island", "Island"]).start();
    let c = g.card(0, "City of Traitors", Zone::Hand);
    g.choose(Choice::Play(c));
    g.drive_until(at_priority);
    assert!(g.stack().is_empty(), "no trigger on itself");
    let pd = bf(&g, 0, "Polluted Delta");
    g.choose(act(&g, pd));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::SearchLibrary) => {
            Some(Choice::Def(id("Swamp")))
        }
        _ => None,
    });
    assert_eq!(g.count_in(0, "Swamp", Zone::Battlefield), 1, "{}", log(&g));
    assert_eq!(g.obj(c).zone, Zone::Battlefield, "not sacrificed\n{}", log(&g));
}

#[test]
fn celestial_colonnade_becoming_a_creature_doesnt_enter_cr_603_6a() {
    // ruling: e43b1205
    // Vaultborn Tyrant: "Whenever this creature or another creature you
    // control with power 4 or greater enters, you gain 3 life and draw a
    // card." The Colonnade becomes a 4/4: no trigger.
    let mut g = scen()
        .battlefield(0, &["Vaultborn Tyrant", "Celestial Colonnade", "Plains", "Plains", "Island", "Island", "Island"])
        .start();
    let c = bf(&g, 0, "Celestial Colonnade");
    let a = (0..4u8).map(|i| Choice::Ability(c, AbilitySlot::Activated(i))).filter(|&a| g.has_option(a)).last().unwrap();
    g.choose(a);
    resolve(&mut g);
    assert!(g.is_creature(c) && g.pt(c) == (4, 4), "{}", log(&g));
    assert_eq!(g.life(0), 20, "no enter trigger\n{}", log(&g));
}

// ---- flashback (702.34a) ----------------------------------------------------------------------

#[test]
fn a_flashback_spell_countered_or_remanded_is_exiled_cr_702_34a() {
    // ruling: 0ede9b79 e487566e
    for counter in ["Counterspell", "Remand"] {
        let mut g = scen()
            .battlefield(0, &["Mountain", "Mountain", "Mountain"])
            .battlefield(1, &["Island", "Island"])
            .graveyard(0, &["Faithless Looting"])
            .hand(0, &["Island"])
            .hand(1, &[counter])
            .start();
        let fl = g.card(0, "Faithless Looting", Zone::Graveyard);
        assert!(g.has_option(Choice::Cast(fl)), "{}", opts(&g));
        g.choose(Choice::Cast(fl));
        quiet_until(&mut g, at_priority);
        g.pass();
        let cs = g.card(1, counter, Zone::Hand);
        let t = Choice::Target(Target::Obj(g.obj_ref(fl)));
        cast_targets(&mut g, cs, &[t]);
        assert_eq!(g.obj(fl).zone, Zone::Exile, "{counter}\n{}", log(&g));
    }
}

// ---- library positions (401.4) ----------------------------------------------------------------

#[test]
fn oust_into_an_empty_library_and_second_from_the_top_cr_401_4() {
    // ruling: 1f35b259 cd7798a0
    for (lib, pos) in [(0usize, 0usize), (3, 1)] {
        let mut g = bare().battlefield(0, &["Plains"]).battlefield(1, &["Test Bear"]).library(1, &vec!["Forest"; lib]).hand(0, &["Oust"]).start();
        let bear = bf(&g, 1, "Test Bear");
        let t = g.tgt(bear);
        cast_resolve(&mut g, 0, "Oust", &[t]);
        let l = &g.player(1).library;
        assert_eq!(l.len(), lib + 1);
        assert_eq!(l[l.len() - 1 - pos], bear, "{lib} cards: position {pos} from the top");
        assert_eq!(g.life(1), 23);
    }
}

#[test]
fn unexpectedly_absent_x_zero_puts_it_on_top_and_x_past_the_bottom_on_the_bottom_cr_401_4() {
    // ruling: 68100e83 f13eb15b
    // "Put target nonland permanent into its owner's library just beneath
    // the top X cards of that library." ({X}{W}{W}, instant.)
    for (x, top) in [(0u8, true), (5, false)] {
        let mut g = bare()
            .battlefield(0, &["Plains"; 7])
            .battlefield(1, &["Test Bear"])
            .library(1, &["Forest", "Forest"])
            .hand(0, &["Unexpectedly Absent"])
            .start();
        let bear = bf(&g, 1, "Test Bear");
        let ua = g.card(0, "Unexpectedly Absent", Zone::Hand);
        g.choose(Choice::Cast(ua));
        let t = g.tgt(bear);
        resolve_with(&mut g, |g| match g.kind() {
            Some(DecisionKind::ChooseMode) => Some(Choice::Number(x)),
            Some(DecisionKind::Target) => Some(t),
            _ => None,
        });
        let l = &g.player(1).library;
        assert_eq!(l.len(), 3, "{}", log(&g));
        if top {
            assert_eq!(*l.last().unwrap(), bear, "X = 0: on top");
        } else {
            assert_eq!(l[0], bear, "X = 5 > 2 cards: on the bottom");
        }
    }
}
