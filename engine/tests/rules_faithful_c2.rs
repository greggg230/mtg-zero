//! Workstream C2 of the faithfulness audit (ENGINE.md 100.9): permanents that
//! enter the battlefield at the same time. One event that puts several
//! permanents onto the battlefield is one entry: every permanent on the
//! battlefield, the newcomers included, is checked for enters triggers once
//! all of them are there and what applies as they enter is done (CR 603.6a,
//! 603.6d, 614.12). Each test names the CR rule it checks and failed before
//! the fix.

mod common;

use engine::db::{AbilitySlot, CardDb};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};
use serde_json::{json, Value};
use std::sync::OnceLock;

fn card(name: &str, printing: Value) -> Value {
    let mut p = printing;
    p["$t"] = json!("CardPrinting");
    p["name"] = json!(name);
    for (k, v) in [("subtypes", json!([])), ("supertypes", json!([])), ("type_line", json!(""))] {
        if p.get(k).is_none() {
            p[k] = v;
        }
    }
    json!({"name": name, "custom": true, "printing": p})
}

/// This suite's own test cards, written as card IR (the shapes real cards
/// have), lowered with the cube and the Edge of Eternities table.
fn c2_cards() -> Vec<card_ir::Card> {
    let v = vec![
        // "You may have this creature enter as a copy of a creature on the
        // battlefield." (Phyrexian Metamorph's shape at mana value 2.)
        card(
            "C2 Mimic",
            json!({"card_types": ["Creature"], "subtypes": ["Shapeshifter"], "power": 0, "toughness": 0,
                "mana_cost": {"$t": "ManaCost", "generic": 1, "colored": [["U", 1]]}, "mana_cost_str": "{1}{U}",
                "enters_as_copy_types": ["Creature"]}),
        ),
        // A land with "{T}: Add {W}" and "Whenever another land you control
        // enters, you gain 1 life."
        card(
            "C2 Cartographer",
            json!({"card_types": ["Land"], "mana_cost": {"$t": "ManaCost"}, "mana_cost_str": "", "produces": ["W"],
                "triggered_abilities": [{"$t": "TriggeredAbility", "event": "enters",
                    "predicate": {"$t": "And", "left": {"$t": "And", "left": {"$t": "Subject", "inner": {"$t": "IsLand"}, "slot": null},
                        "right": {"$t": "Controlled"}}, "right": {"$t": "Not", "inner": {"$t": "IsSelf"}}},
                    "effect": {"$t": "GainLife", "amount": 1}}]}),
        ),
        // A land creature, 1/1, "{T}: Add {G}" (Dryad Arbor's shape).
        card(
            "C2 Arbor",
            json!({"card_types": ["Land", "Creature"], "subtypes": ["Dryad"], "power": 1, "toughness": 1,
                "mana_cost": {"$t": "ManaCost"}, "mana_cost_str": "", "produces": ["G"]}),
        ),
        // A 0/0 with devour land 1 (CR 702.82a: "as this enters, you may
        // sacrifice any number of lands; it enters with a +1/+1 counter for
        // each").
        card(
            "C2 Devourer",
            json!({"card_types": ["Creature"], "subtypes": ["Beast"], "power": 0, "toughness": 0,
                "mana_cost": {"$t": "ManaCost", "generic": 1, "colored": [["G", 1]]}, "mana_cost_str": "{1}{G}",
                "devour_land_multiplier": 1}),
        ),
        // "Destroy two target creatures."
        card(
            "C2 Twin Doom",
            json!({"card_types": ["Sorcery"], "mana_cost": {"$t": "ManaCost", "colored": [["B", 1]]}, "mana_cost_str": "{B}",
                "spell_effect": {"$t": "Destroy", "targets": [{"$t": "TargetSpec", "filter": {"$t": "IsCreature"}, "count_min": 2, "count_max": 2}]}}),
        ),
        // A 1/1 with "Whenever another creature you control dies, you gain 1
        // life."
        card(
            "C2 Mourner",
            json!({"card_types": ["Creature"], "subtypes": ["Human"], "power": 1, "toughness": 1,
                "mana_cost": {"$t": "ManaCost", "colored": [["W", 1]]}, "mana_cost_str": "{W}",
                "triggered_abilities": [{"$t": "TriggeredAbility", "event": "dies",
                    "predicate": {"$t": "And", "left": {"$t": "And", "left": {"$t": "Subject", "inner": {"$t": "IsCreature"}, "slot": null},
                        "right": {"$t": "Controlled"}}, "right": {"$t": "Not", "inner": {"$t": "IsSelf"}}},
                    "effect": {"$t": "GainLife", "amount": 1}}]}),
        ),
    ];
    v.into_iter().map(|v| serde_json::from_value(v).expect("custom card IR")).collect()
}

/// The cube, the Edge of Eternities cards it doesn't have, the shared test
/// cards and this suite's, lowered once.
fn db() -> &'static CardDb {
    static DB: OnceLock<&'static CardDb> = OnceLock::new();
    DB.get_or_init(|| {
        let mut cards = engine::db::load_cube_document().cards;
        for c in engine::db::load_eoe_document().cards {
            if !cards.iter().any(|x| x.name == c.name) {
                cards.push(c);
            }
        }
        cards.extend(common::custom_cards());
        cards.extend(c2_cards());
        let db = CardDb::from_cards(&cards);
        for c in c2_cards() {
            db.expect_id(&c.name).unwrap_or_else(|e| panic!("{e}"));
        }
        db.leak()
    })
}

fn scen() -> Scenario {
    Scenario::new(db())
        .spec(|s| {
            s.auto_single = false;
            s.log = true;
        })
        .library(0, &["Island"; 12])
        .library(1, &["Island"; 12])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn gy(g: &Game, p: PlayerId, name: &str) -> Vec<CardIdx> {
    let mut v: Vec<CardIdx> = (0..g.objs().len() as u16).map(CardIdx).filter(|&c| g.obj(c).owner == p && g.obj(c).zone == Zone::Graveyard && g.name(c) == name).collect();
    v.sort();
    v
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

/// Pay with the first source offered until the payment is over.
fn pay(g: &mut Game) {
    g.drive_until(|g| at_priority(g) || g.kind() != Some(DecisionKind::PayMana));
    while matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana)) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
        g.drive_until(|g| at_priority(g) || !matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana)));
    }
}

/// Run the stack down; `answer` answers what is asked (None: first option).
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
    panic!("resolve_with: no progress\n{}", log(g));
}

/// Cast `name` from P0's hand choosing `targets` in order (then "no more"),
/// pay, and resolve, answering what is asked with `answer`.
fn cast_targets(g: &mut Game, name: &str, targets: &[CardIdx], answer: impl Fn(&Game) -> Option<Choice> + Copy) {
    let c = g.card(0, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    for &t in targets {
        g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
        let o = g.tgt(t);
        assert!(g.has_option(o), "{} a target: {:?}", g.name(t), g.options_desc());
        g.choose(o);
    }
    // ("Up to": no more targets.)
    if g.kind() == Some(DecisionKind::Target) && g.has_option(Choice::Pass) {
        g.choose(Choice::Pass);
    }
    pay(g);
    resolve_with(g, answer);
}

// ---- Returned together (CR 603.6a) ---------------------------------------------------------

/// Scout for Survivors returns two Guide of Souls ("Whenever another creature
/// you control enters, you gain 1 life and get {E}") together: each sees the
/// other enter (CR 603.6a: "all permanents on the battlefield (including the
/// newcomers) are checked"), so both trigger. One by one, only the second's
/// entry was seen.
#[test]
fn cr_603_6a_creatures_returned_together_see_each_other_enter() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains"])
        .graveyard(0, &["Guide of Souls", "Guide of Souls"])
        .hand(0, &["Scout for Survivors"])
        .at(Step::Main1, 0)
        .start();
    let guides = gy(&g, 0, "Guide of Souls");
    assert_eq!(guides.len(), 2);
    cast_targets(&mut g, "Scout for Survivors", &guides, |_| None);
    for &c in &guides {
        assert_eq!(g.obj(c).zone, Zone::Battlefield, "{}", log(&g));
    }
    assert_eq!(g.life(0), 22, "each Guide of Souls triggered for the other\n{}", log(&g));
}

/// The same with the other order of the targets: the result doesn't depend
/// on which permanent the engine moves first.
#[test]
fn cr_603_6a_creatures_returned_together_in_either_order() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains"])
        .graveyard(0, &["Guide of Souls", "Guide of Souls"])
        .hand(0, &["Scout for Survivors"])
        .at(Step::Main1, 0)
        .start();
    let mut guides = gy(&g, 0, "Guide of Souls");
    guides.reverse();
    cast_targets(&mut g, "Scout for Survivors", &guides, |_| None);
    assert_eq!(g.life(0), 22, "each Guide of Souls triggered for the other
{}", log(&g));
}

// ---- Created together (CR 603.6a, 111.2) ---------------------------------------------------

/// Devastating Onslaught with X = 2 creates two token copies of Weftstalker
/// Ardent ("Whenever another creature or artifact you control enters, this
/// creature deals 1 damage to each opponent") at once: the original triggers
/// for each token, and each token for the other (CR 603.6a): 4 damage, not 3.
#[test]
fn cr_603_6a_tokens_created_together_see_each_other_enter() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Mountain", "Mountain", "Mountain", "Weftstalker Ardent"])
        .hand(0, &["Devastating Onslaught"])
        .at(Step::Main1, 0)
        .start();
    let ardent = bf(&g, 0, "Weftstalker Ardent");
    let c = g.card(0, "Devastating Onslaught", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(2));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(ardent));
    pay(&mut g);
    resolve_with(&mut g, |_| None);
    let n = g.permanents_of(0).filter(|&x| g.name(x) == "Weftstalker Ardent").count();
    assert_eq!(n, 3, "{}", log(&g));
    assert_eq!(g.life(1), 16, "2 from the original, 1 from each token for the other\n{}", log(&g));
}

// ---- "Enter as a copy" with others (CR 603.6a, 614.12, 707.5) ------------------------------

/// Scout for Survivors returns Guide of Souls and C2 Mimic together; Mimic
/// enters as a copy of Weftstalker Ardent (on the battlefield before the
/// entry: Guide of Souls, entering at the same time, isn't offered, CR
/// 614.12a). The copy choice is part of the entry (CR 614.12, 603.6d), and
/// the entry's triggers are checked after it: the Mimic, an Ardent, sees
/// Guide of Souls enter. P1 takes 3 (the Ardent for both, the Mimic for
/// Guide), P0 gains 1 (Guide for the Mimic).
#[test]
fn cr_603_6a_a_copy_entering_with_others_sees_them_as_what_it_copies() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains", "Weftstalker Ardent"])
        .graveyard(0, &["Guide of Souls", "C2 Mimic"])
        .hand(0, &["Scout for Survivors"])
        .at(Step::Main1, 0)
        .start();
    let ardent = bf(&g, 0, "Weftstalker Ardent");
    let guide = gy(&g, 0, "Guide of Souls")[0];
    let mimic = gy(&g, 0, "C2 Mimic")[0];
    let copy = move |g: &Game| {
        (g.kind() == Some(DecisionKind::ChoosePermanent)).then(|| {
            assert!(!g.has_option(Choice::Card(guide)), "entering at the same time: not a choice (CR 614.12a)");
            Choice::Card(ardent)
        })
    };
    cast_targets(&mut g, "Scout for Survivors", &[guide, mimic], copy);
    assert_eq!(g.name(mimic), "Weftstalker Ardent", "{}", log(&g));
    assert_eq!(g.life(1), 17, "the Ardent twice, the Mimic-Ardent for Guide of Souls\n{}", log(&g));
    assert_eq!(g.life(0), 21, "Guide of Souls for the Mimic\n{}", log(&g));
}

// ---- Returned together when a permanent leaves (CR 603.6a, 607.2a) --------------------------

/// Parallax Wave exiled two Guide of Souls; when it leaves, "each player
/// returns to the battlefield all cards they own exiled with it": both return
/// at once and each sees the other enter (CR 603.6a).
#[test]
fn cr_603_6a_cards_returned_together_when_their_exiler_leaves() {
    let fade = engine::db::Counter::Named(db().subtype_id("counter:fade").unwrap());
    let mut g = scen()
        .battlefield(0, &["Parallax Wave", "Scrubland", "Scrubland", "Scrubland", "Guide of Souls", "Guide of Souls"])
        .counters(0, "Parallax Wave", fade, 5)
        .hand(0, &["Vindicate"])
        .at(Step::Main1, 0)
        .start();
    let wave = bf(&g, 0, "Parallax Wave");
    let guides: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.name(c) == "Guide of Souls").collect();
    for &t in &guides {
        g.choose(Choice::Ability(wave, AbilitySlot::Activated(0)));
        g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
        g.choose(g.tgt(t));
        resolve_with(&mut g, |_| None);
    }
    assert_eq!(g.permanents_of(0).filter(|&c| g.name(c) == "Guide of Souls").count(), 0, "{}", log(&g));
    let life = g.life(0);
    let c = g.card(0, "Vindicate", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(wave));
    pay(&mut g);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.permanents_of(0).filter(|&c| g.name(c) == "Guide of Souls").count(), 2, "{}", log(&g));
    assert_eq!(g.life(0), life + 2, "each Guide of Souls triggered for the other\n{}", log(&g));
}

// ---- What applies as they enter (CR 614.12a) -----------------------------------------------

/// Exhume: P0 returns Spider-Woman, Stunning Savior ("Artifacts and creatures
/// your opponents control enter tapped"), P1 returns a creature, at the same
/// time. Spider-Woman's effect doesn't exist yet as P1's creature enters (CR
/// 614.12a: replacement effects from permanents already on the battlefield):
/// it enters untapped, though the engine puts P0's card onto the battlefield
/// first.
#[test]
fn cr_614_12a_a_permanent_entering_at_the_same_time_applies_nothing_to_the_others() {
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp"])
        .graveyard(0, &["Spider-Woman, Stunning Savior"])
        .graveyard(1, &["Test Bear"])
        .hand(0, &["Exhume"])
        .at(Step::Main1, 0)
        .start();
    let spider = gy(&g, 0, "Spider-Woman, Stunning Savior")[0];
    let bear = gy(&g, 1, "Test Bear")[0];
    let c = g.card(0, "Exhume", Zone::Hand);
    g.choose(Choice::Cast(c));
    pay(&mut g);
    resolve_with(&mut g, |g| {
        (g.kind() == Some(DecisionKind::ChooseGraveyardCard)).then(|| {
            if g.pending_decision().unwrap().player == 0 {
                Choice::Card(spider)
            } else {
                Choice::Card(bear)
            }
        })
    });
    assert_eq!(g.obj(spider).zone, Zone::Battlefield, "{}", log(&g));
    assert_eq!(g.obj(bear).zone, Zone::Battlefield, "{}", log(&g));
    assert!(!g.obj(bear).tapped, "entered at the same time as Spider-Woman: untapped\n{}", log(&g));
}

// ---- Chosen from a pile, put onto the battlefield together (CR 603.6a) -----------------------

/// Famished Worldsire's "look at the top X cards ... Put any number of land
/// cards from among them onto the battlefield tapped": the lands are chosen,
/// then put onto the battlefield at once. Two C2 Cartographers ("Whenever
/// another land you control enters, you gain 1 life") and an Island: each
/// Cartographer sees the two other lands enter (CR 603.6a): 4 life, not 3 as
/// when each land entered as it was chosen.
#[test]
fn cr_603_6a_lands_chosen_from_a_pile_enter_together() {
    let mut g = scen()
        .battlefield(0, &["Forest"; 9])
        .hand(0, &["Famished Worldsire"])
        .library(0, &["C2 Cartographer", "C2 Cartographer", "Island"])
        .at(Step::Main1, 0)
        .start();
    let ws = g.card(0, "Famished Worldsire", Zone::Hand);
    g.choose(Choice::Cast(ws));
    pay(&mut g);
    let devoured = std::cell::Cell::new(false);
    let first_card = |g: &Game| g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|c| matches!(c, Choice::Card(_)));
    resolve_with(&mut g, |g| match g.kind() {
        // Devour land 3: one land (power 3: the top three cards).
        Some(DecisionKind::Sacrifice) => Some(if devoured.replace(true) { Choice::Pass } else { first_card(g).unwrap() }),
        // Every land of the pile.
        Some(DecisionKind::PutInHand) => Some(first_card(g).unwrap_or(Choice::Pass)),
        _ => None,
    });
    assert_eq!(g.power(ws), 3, "{}", log(&g));
    assert_eq!(g.permanents_of(0).filter(|&c| g.name(c) == "C2 Cartographer").count(), 2, "{}", log(&g));
    assert_eq!(g.life(0), 24, "each Cartographer saw the two other lands enter
{}", log(&g));
}

// ---- Destroyed together (CR 603.10a) -------------------------------------------------------

/// "Destroy two target creatures" destroys them at once: each C2 Mourner
/// ("Whenever another creature you control dies, you gain 1 life") looks back
/// and sees the other die (CR 603.10a): 2 life, not 1 as when they were
/// destroyed one after the other.
#[test]
fn cr_603_10a_creatures_destroyed_together_see_each_other_die() {
    let mut g = scen().battlefield(0, &["Swamp", "C2 Mourner", "C2 Mourner"]).hand(0, &["C2 Twin Doom"]).at(Step::Main1, 0).start();
    let mourners: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.name(c) == "C2 Mourner").collect();
    cast_targets(&mut g, "C2 Twin Doom", &mourners, |_| None);
    assert_eq!(g.permanents_of(0).filter(|&c| g.name(c) == "C2 Mourner").count(), 0, "{}", log(&g));
    assert_eq!(g.life(0), 22, "each Mourner saw the other die
{}", log(&g));
}

/// Scout for Survivors returns C2 Arbor (a land creature) and C2 Devourer
/// (devour land 1) together: the Devourer may sacrifice the lands already on
/// the battlefield as it enters, not the Arbor, which enters at the same time
/// (CR 702.82a, 614.12a), though the engine moves the Arbor first.
#[test]
fn cr_614_12a_devour_offers_only_lands_already_on_the_battlefield() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains"])
        .graveyard(0, &["C2 Arbor", "C2 Devourer"])
        .hand(0, &["Scout for Survivors"])
        .at(Step::Main1, 0)
        .start();
    let arbor = gy(&g, 0, "C2 Arbor")[0];
    let devourer = gy(&g, 0, "C2 Devourer")[0];
    let asked = std::cell::Cell::new(false);
    cast_targets(&mut g, "Scout for Survivors", &[arbor, devourer], |g| {
        (g.kind() == Some(DecisionKind::Sacrifice)).then(|| {
            assert!(!g.has_option(Choice::Card(arbor)), "entering at the same time: not a land to devour
{}", log(g));
            assert_eq!(g.options_desc().len(), 4, "the three Plains and no more: {:?}", g.options_desc());
            asked.set(true);
            Choice::Pass
        })
    });
    assert!(asked.get(), "devour asked
{}", log(&g));
    assert_eq!(g.obj(arbor).zone, Zone::Battlefield, "{}", log(&g));
}
