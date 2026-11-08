//! Edge of Eternities W2 (ENGINE.md 99.12): mana and lands. Spending
//! restrictions (CR 106.6) here; the lands and the other cards in
//! `cards_eoe_w2_lands.rs`.

mod eoe_w2;

use eoe_w2::*;
use engine::db::{SPEND_ARTIFACT, SPEND_ARTIFACT_ABILITY, SPEND_ELDRAZI_ABILITY, SPEND_ELDRAZI_SPELL, SPEND_LAND_ABILITY};
use engine::{Choice, DecisionKind, Step, Zone};

const C: u8 = 5;
const U: u8 = 1;
const ELDRAZI: u8 = SPEND_ELDRAZI_SPELL | SPEND_ELDRAZI_ABILITY;

/// Eldrazi Temple: "{T}: Add {C}{C}. Spend this mana only to cast colorless
/// Eldrazi spells or activate abilities of colorless Eldrazi" (CR 106.6,
/// 105.2c). Castability counts the two {C} for a colorless Eldrazi spell
/// only.
#[test]
fn cr_106_6_eldrazi_temple_pays_for_colorless_eldrazi_spells() {
    let mut g = scen()
        .battlefield(0, &["Eldrazi Temple", "Island", "Island", "Island", "Island"])
        .hand(0, &["Anticausal Vestige", "Test Blue Eldrazi"])
        .at(Step::Main1, 0)
        .start();
    let temple = bf(&g, 0, "Eldrazi Temple");
    let vestige = g.card(0, "Anticausal Vestige", Zone::Hand);
    assert!(g.has_option(Choice::Cast(vestige)), "{{6}}: four Islands and the Temple's {{C}}{{C}}");
    assert!(!g.can_pay(0, &cost("{6}")), "not for a cost of anything else");
    assert!(g.can_pay(0, &cost("{5}")));
    g.choose(Choice::Cast(vestige));
    // (Warp or the mana cost: the mana cost.)
    while g.kind() != Some(DecisionKind::PayMana) {
        let c = opts(&g)[0];
        g.choose(c);
    }
    pay_with(&mut g, &[act(temple, 1)]);
    settle(&mut g);
    assert_eq!(g.obj(vestige).zone, Zone::Battlefield);
    assert!(g.restricted_mana(0).is_empty(), "both {{C}} spent");
}

/// What the Temple's mana leaves stays in the pool with its restriction (CR
/// 106.6): it pays a colorless Eldrazi's activated ability, never a spell
/// that isn't a colorless Eldrazi (a blue Eldrazi is not colorless).
#[test]
fn cr_106_6_eldrazi_temple_mana_left_in_the_pool_keeps_its_restriction() {
    let mut g = scen()
        .battlefield(0, &["Eldrazi Temple", "Test Eldrazi Seer", "Island"])
        .hand(0, &["Test Eldrazi Relic", "Test Bear", "Test Blue Eldrazi"])
        .at(Step::Main1, 0)
        .start();
    let temple = bf(&g, 0, "Eldrazi Temple");
    let seer = bf(&g, 0, "Test Eldrazi Seer");
    let relic = g.card(0, "Test Eldrazi Relic", Zone::Hand);
    g.choose(Choice::Cast(relic));
    pay_with(&mut g, &[act(temple, 1)]);
    settle(&mut g);
    assert_eq!(g.obj(relic).zone, Zone::Battlefield);
    assert_eq!(g.player(0).pool.amt[5], 1);
    assert_eq!(g.restricted_mana(0), vec![(C, ELDRAZI, 1)]);
    let blue = g.card(0, "Test Blue Eldrazi", Zone::Hand);
    assert!(g.has_option(Choice::Cast(blue)), "{{U}} from the Island");
    assert!(!g.can_pay(0, &cost("{1}{U}")), "the pool's {{C}} can't pay for anything else");
    // The Seer's "{1}: Draw a card" (a colorless Eldrazi's ability).
    assert!(g.has_option(act(seer, 0)));
    let hand = g.player(0).hand.len();
    start_with(&mut g, act(seer, 0), |_| None);
    settle(&mut g);
    assert_eq!(g.player(0).hand.len(), hand + 1);
    assert_eq!(g.player(0).pool.amt, [0; 6]);
    assert!(g.restricted_mana(0).is_empty());
    assert!(!g.obj(bf(&g, 0, "Island")).tapped, "the Island stayed untapped");
}

/// Steelswarm Operator: "{T}: Add {U}. Spend this mana only to cast an
/// artifact spell." and "{T}: Add {U}{U}. Spend this mana only to activate
/// abilities of artifact sources" (CR 106.6, 602.2).
#[test]
fn cr_106_6_steelswarm_operator_mana_for_artifact_spells_and_artifact_abilities() {
    let mut g = scen()
        .battlefield(0, &["Steelswarm Operator", "Atomic Microsizer", "Test Bear"])
        .hand(0, &["Test Flyer", "Test Eldrazi Relic"])
        .at(Step::Main1, 0)
        .start();
    let op = bf(&g, 0, "Steelswarm Operator");
    let gear = bf(&g, 0, "Atomic Microsizer");
    let flyer = g.card(0, "Test Flyer", Zone::Hand);
    let relic = g.card(0, "Test Eldrazi Relic", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(flyer)), "not a spell its mana may pay");
    assert!(g.has_option(Choice::Cast(relic)), "an artifact spell: the first ability's {{U}}");
    assert!(g.has_option(act(gear, 0)), "equip {{2}}: an artifact's ability, the second ability's {{U}}{{U}}");
    let bear = bf(&g, 0, "Test Bear");
    start_with(&mut g, act(gear, 0), |g| pick(g, &[act(op, 1), g.tgt(bear)]));
    settle(&mut g);
    assert_eq!(g.obj(gear).attached.map(|r| r.idx), Some(bear), "equipped");
    assert!(g.obj(op).tapped);
    assert!(!g.has_option(Choice::Cast(relic)), "the Operator is tapped");
}

/// What the Operator's {U}{U} leaves after a {1} artifact ability stays for
/// abilities of artifact sources only: not for an artifact spell (CR 106.6).
#[test]
fn cr_106_6_artifact_ability_mana_left_in_the_pool_pays_only_artifact_abilities() {
    let mut g = scen()
        .battlefield(0, &["Steelswarm Operator", "Test Artifact Engine", "Test Land Spark"])
        .hand(0, &["Test Eldrazi Relic"])
        .at(Step::Main1, 0)
        .start();
    let op = bf(&g, 0, "Steelswarm Operator");
    let engine_ = bf(&g, 0, "Test Artifact Engine");
    let spark = bf(&g, 0, "Test Land Spark");
    assert!(!g.has_option(act(spark, 1)), "a land's ability: not with the Operator's mana");
    start_with(&mut g, act(engine_, 0), |g| pick(g, &[act(op, 1)]));
    settle(&mut g);
    assert_eq!(g.restricted_mana(0), vec![(U, SPEND_ARTIFACT_ABILITY, 1)]);
    let relic = g.card(0, "Test Eldrazi Relic", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(relic)), "an artifact spell is not an artifact's ability");
    assert!(!g.has_option(act(spark, 1)));
    assert!(g.has_option(act(engine_, 0)), "again, from the pool");
    start_with(&mut g, act(engine_, 0), |_| None);
    settle(&mut g);
    assert_eq!(g.player(0).pool.amt[1], 0);
    assert!(g.restricted_mana(0).is_empty());
}

/// Sunken Citadel's restriction ("Spend this mana only to activate abilities
/// of land sources", CR 106.6) on a land of a fixed colour: a land's
/// abilities only, the {X} of one included; what is left stays.
#[test]
fn cr_106_6_land_ability_mana_pays_only_abilities_of_lands() {
    let mut g = scen()
        .battlefield(0, &["Test Citadel", "Test Land Engine", "Test Land Spark", "Test Artifact Engine"])
        .hand(0, &["Test Eldrazi Relic"])
        .at(Step::Main1, 0)
        .start();
    let citadel = bf(&g, 0, "Test Citadel");
    let land_engine = bf(&g, 0, "Test Land Engine");
    let spark = bf(&g, 0, "Test Land Spark");
    let art = bf(&g, 0, "Test Artifact Engine");
    let relic = g.card(0, "Test Eldrazi Relic", Zone::Hand);
    assert!(g.has_option(act(land_engine, 1)), "{{2}}: a land's ability");
    assert!(!g.has_option(act(art, 0)), "an artifact's: no");
    assert!(!g.has_option(Choice::Cast(relic)), "a spell: no");
    start_with(&mut g, act(spark, 1), |g| pick(g, &[act(citadel, 0)]));
    settle(&mut g);
    assert_eq!(g.restricted_mana(0), vec![(U, SPEND_LAND_ABILITY, 1)]);
    assert!(!g.has_option(act(land_engine, 1)), "one {{U}} left: not {{2}}");
    assert!(g.has_option(act(spark, 1)) && !g.has_option(act(art, 0)));
}

/// Restricted mana of two kinds a payment may both spend (CR 106.4, 601.2h:
/// the player chooses which mana pays): a colorless Eldrazi artifact spell
/// can take the artifact-only {C} or the Eldrazi {C}; the pool left is the
/// player's choice (`PayPool`).
#[test]
fn cr_601_2h_which_restricted_mana_pays_is_the_players_choice() {
    let mut g = scen()
        .battlefield(0, &["Test Workshop", "Eldrazi Temple", "Test Eldrazi Seer"])
        .hand(0, &["Test Eldrazi Relic", "Test Eldrazi Relic"])
        .at(Step::Main1, 0)
        .start();
    let ws = bf(&g, 0, "Test Workshop");
    let temple = bf(&g, 0, "Eldrazi Temple");
    let seer = bf(&g, 0, "Test Eldrazi Seer");
    let relics: Vec<_> = g.player(0).hand.clone();
    g.choose(Choice::Cast(relics[0]));
    pay_with(&mut g, &[act(ws, 0)]);
    settle(&mut g);
    assert_eq!(g.restricted_mana(0), vec![(C, SPEND_ARTIFACT, 1)]);
    // The Seer's ability: the artifact-only {C} can't pay it; the Temple's.
    start_with(&mut g, act(seer, 0), |g| pick(g, &[act(temple, 1)]));
    settle(&mut g);
    let mut kinds = g.restricted_mana(0);
    kinds.sort();
    assert_eq!(kinds, vec![(C, SPEND_ARTIFACT, 1), (C, ELDRAZI, 1)]);
    g.choose(Choice::Cast(relics[1]));
    assert_eq!(g.kind(), Some(DecisionKind::PayPool), "{:?}", g.options_desc());
    assert_eq!(opts(&g).len(), 2, "keep the artifact-only {{C}} or the Eldrazi {{C}}");
    let second = opts(&g)[1];
    g.choose(second);
    settle(&mut g);
    assert_eq!(g.restricted_mana(0).len(), 1);
    assert_eq!(g.player(0).pool.amt[5], 1);
}

/// Mana a mana ability makes with priority (floated in response, its source
/// targeted: ENGINE.md 86.1) keeps its restriction in the pool (CR 106.6).
#[test]
fn cr_106_6_restricted_mana_floated_in_response_keeps_its_restriction() {
    let mut g = scen()
        .battlefield(0, &["Steelswarm Operator"])
        .battlefield(1, &["Mountain", "Mountain", "Mountain"])
        .hand(1, &["Bombard"])
        .at(Step::Main1, 1)
        .start();
    let op = bf(&g, 0, "Steelswarm Operator");
    let bombard = g.card(1, "Bombard", Zone::Hand);
    start_with(&mut g, Choice::Cast(bombard), |g| pick(g, &[g.tgt(op)]));
    g.pass();
    assert_eq!(g.pending_decision().unwrap().player, 0);
    assert!(g.has_option(act(op, 1)), "its mana before it dies");
    g.choose(act(op, 1));
    g.drive_until(|g| at_priority(g));
    assert_eq!(g.player(0).pool.amt[1], 2);
    assert_eq!(g.restricted_mana(0), vec![(U, SPEND_ARTIFACT_ABILITY, 2)]);
}
