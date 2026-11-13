//! `{C}` in a cost (CR 107.4c: "a cost that can be paid only with one
//! colorless mana"; 106.1b: colorless is a type of mana; 202.1a: paying a
//! mana cost "requires matching the type of any colored or colorless mana
//! symbols"), wherever the IR structures a mana cost (ENGINE.md 146.2).
//!
//! mtg-bot's `parse_mana_cost` counts `{C}` as generic mana unless a builder
//! opts in (docs/ir_defects.md 5, 67, 341): Warping Wail's `{1}{C}` was
//! lowered as `{2}` and cast with two Islands (the Fable review's probe
//! `zz_rules_c::c01`). The lowering now checks every structured cost against
//! its printed form (`*_str`, ENGINE.md 3 rule 4a, `lower_printed_cost`) and
//! takes the `{C}` pips from it; any other difference is a blocker. The
//! custom cards below are lowered one by one (`engine::lower::lower_card`), each
//! in the lossy form (`{C}` as generic) and the exact one (`colored: [[C, n]]`).

use engine::db::{CardDb, CardDef, WardCost};
use engine::mana::{ManaCost, C};
use engine::testkit::Scenario;
use engine::{Choice, DecisionKind, Step, Zone};
use serde_json::{json, Value};

fn mana(s: &str) -> ManaCost {
    ManaCost::parse(s).unwrap()
}

/// A custom card from its printing's fields (`name`, `type_line` and the
/// type fields filled in when missing).
fn card(name: &str, types: &[&str], fields: Value) -> card_ir::Card {
    let mut p = json!({
        "$t": "CardPrinting", "name": name, "mana_cost": {"$t": "ManaCost"}, "mana_cost_str": "",
        "type_line": types.join(" "), "supertypes": [], "card_types": types, "subtypes": [],
    });
    if let Value::Object(m) = fields {
        for (k, v) in m {
            p[k] = v;
        }
    }
    serde_json::from_value(json!({"name": name, "custom": true, "printing": p})).expect("custom card IR")
}

fn lower(c: &card_ir::Card) -> Result<CardDef, Vec<String>> {
    engine::lower::lower_card(c, &mut Vec::new(), &mut Vec::new()).map(|(d, _)| d)
}

fn lowered(c: &card_ir::Card) -> CardDef {
    lower(c).unwrap_or_else(|e| panic!("{}: {e:?}", c.name))
}

/// `{C}` as mtg-bot's lossy default writes it (`n` generic) and as the
/// exact form (`colored: [[C, 1]]`), for the printed cost "{g}{C}".
fn both(g: i64) -> [Value; 2] {
    [json!({"$t": "ManaCost", "generic": g + 1}), json!({"$t": "ManaCost", "generic": g, "colored": [["C", 1]]})]
}

const DRAW: &str = r#"{"$t": "DrawCards", "amount": 1}"#;

fn draw() -> Value {
    serde_json::from_str(DRAW).unwrap()
}

// ---- Warping Wail ---------------------------------------------------------------------

/// Warping Wail ({1}{C}): its cost has one generic and one colorless pip;
/// mana value 2 (CR 202.3); a colorless card (202.2: no coloured symbol).
#[test]
fn cr_107_4c_warping_wail_costs_one_generic_and_one_colorless() {
    let db = CardDb::eoe();
    let d = db.def(db.id("Warping Wail").unwrap());
    assert_eq!(d.cost, mana("{1}{C}"));
    assert_eq!((d.cost.generic, d.cost.pips[C]), (1, 1));
    assert_eq!(d.cost.mana_value(), 2);
    assert_eq!(d.colors, 0);
}

fn wail(lands: &[&str]) -> engine::Game {
    Scenario::new(CardDb::eoe())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 12])
        .library(1, &["Island"; 12])
        .battlefield(0, lands)
        .hand(0, &["Warping Wail"])
        .at(Step::Main1, 0)
        .start()
}

/// The probe `zz_rules_c::c01`: two Islands make no colorless mana, so
/// Warping Wail ({1}{C}) can't be cast (CR 107.4c, 601.2f-h, 118.3); an
/// Island and Strip Mine ({T}: Add {C}) cast it, the {C} paid by Strip Mine
/// and the {1} by the Island; two Strip Mines too (generic mana is paid with
/// any type, 107.4b).
#[test]
fn cr_107_4c_warping_wail_needs_colorless_mana() {
    let g = wail(&["Island", "Island"]);
    let w = g.card(0, "Warping Wail", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(w)), "two Islands: {:?}", g.options_desc());
    let g = wail(&["Strip Mine", "Strip Mine"]);
    assert!(g.has_option(Choice::Cast(w)), "two Strip Mines: {:?}", g.options_desc());
    let mut g = wail(&["Island", "Strip Mine"]);
    assert!(g.has_option(Choice::Cast(w)), "Island and Strip Mine: {:?}", g.options_desc());
    // Cast it for its third mode ("Create a 1/1 colorless Eldrazi Scion
    // creature token ..."), paying with both lands.
    g.choose(Choice::Cast(w));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(2));
    let paying = |g: &engine::Game| matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Priority) || !paying(g));
    while paying(&g) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
        g.drive_until(|g| g.kind() == Some(DecisionKind::Priority) || !paying(g));
    }
    let lands: Vec<_> = g.lands_of(0).collect();
    assert!(lands.iter().all(|&l| g.obj(l).tapped), "both lands paid: {}", g.log_lines().join("\n"));
    assert_eq!(g.stack().len(), 1, "{}", g.log_lines().join("\n"));
}

// ---- Every structured cost: the lossy {C} and the exact one -------------------------

/// A spell's mana cost (CR 202.1a).
#[test]
fn cr_202_1a_a_spells_c_pip_is_colorless_in_either_ir_form() {
    for m in both(1) {
        let d = lowered(&card("Test Void Spell", &["Instant"], json!({"mana_cost": m, "mana_cost_str": "{1}{C}", "spell_effect": draw()})));
        assert_eq!(d.cost, mana("{1}{C}"), "{m}");
    }
}

/// An activated ability's cost (CR 602.2b, 601.2f-h): Mirrorpool's "{2}{C},
/// {T}, Sacrifice this land" is the real card in the exact form.
#[test]
fn cr_602_2b_an_activation_costs_c_pip_is_colorless_in_either_ir_form() {
    for m in both(2) {
        let ab = json!([{"$t": "ActivatedAbility", "cost": {"$t": "ActivationCost", "mana": m, "mana_str": "{2}{C}"}, "effect": draw()}]);
        let d = lowered(&card("Test Void Relic", &["Artifact"], json!({"mana_cost": {"$t": "ManaCost", "generic": 1}, "mana_cost_str": "{1}", "activated_abilities": ab})));
        assert_eq!(d.activated[0].cost.mana, mana("{2}{C}"), "{m}");
    }
    let db = CardDb::eoe();
    let d = db.def(db.id("Mirrorpool").unwrap());
    assert_eq!(d.activated.iter().map(|a| a.cost.mana).filter(|c| c.pips[C] == 1).count(), 2, "Mirrorpool's {{2}}{{C}} and {{4}}{{C}}");
}

/// Flashback (CR 702.34a: an alternative cost from the graveyard) and buyback
/// (702.27a: an additional cost).
#[test]
fn cr_702_34a_flashback_and_buyback_c_pips_are_colorless_in_either_ir_form() {
    for m in both(1) {
        let d = lowered(&card("Test Void Echo", &["Sorcery"], json!({"mana_cost": {"$t": "ManaCost", "generic": 1}, "mana_cost_str": "{1}",
            "spell_effect": draw(), "flashback_cost": m, "flashback_cost_str": "{1}{C}"})));
        assert_eq!(d.flashback, Some(mana("{1}{C}")), "{m}");
        let d = lowered(&card("Test Void Return", &["Instant"], json!({"mana_cost": {"$t": "ManaCost", "generic": 1}, "mana_cost_str": "{1}",
            "spell_effect": draw(), "buyback_cost": m, "buyback_cost_str": "{1}{C}"})));
        assert_eq!(d.additional.get(1).map(|c| c.mana), Some(mana("{1}{C}")), "{m}");
    }
}

/// Warp (CR 702.185a) and dash (702.109a): alternative costs of a permanent
/// card.
#[test]
fn cr_702_185a_warp_and_dash_c_pips_are_colorless_in_either_ir_form() {
    for m in both(1) {
        let fields = |k: &str| {
            let mut v = json!({"mana_cost": {"$t": "ManaCost", "generic": 3}, "mana_cost_str": "{3}", "power": 2, "toughness": 2});
            v[format!("{k}_cost")] = m.clone();
            v[format!("{k}_cost_str")] = json!("{1}{C}");
            v
        };
        let d = lowered(&card("Test Void Drifter", &["Creature"], fields("warp")));
        assert!(d.alt_costs.iter().any(|a| a.warp && a.mana == mana("{1}{C}")), "warp {m}: {:?}", d.alt_costs.iter().map(|a| a.mana).collect::<Vec<_>>());
        let d = lowered(&card("Test Void Raider", &["Creature"], fields("dash")));
        assert!(d.alt_costs.iter().any(|a| a.mana == mana("{1}{C}")), "dash {m}");
    }
}

/// Ward (CR 702.21a: "counter it unless that player pays <cost>") and a
/// permanent's kicker (702.33a; Sowing Mycospawn's "Kicker {1}{C}" is the real
/// card, in the lossy form).
#[test]
fn cr_702_21a_ward_and_kicker_c_pips_are_colorless_in_either_ir_form() {
    for m in both(0) {
        let d = lowered(&card("Test Void Warden", &["Creature"], json!({"mana_cost": {"$t": "ManaCost", "generic": 2}, "mana_cost_str": "{2}",
            "power": 2, "toughness": 2, "ward_cost": m, "ward_cost_str": "{C}"})));
        assert!(matches!(d.ward, Some(WardCost::Mana(c)) if c == mana("{C}")), "ward {m}");
    }
    for m in both(1) {
        let d = lowered(&card("Test Void Hulk", &["Creature"], json!({"mana_cost": {"$t": "ManaCost", "generic": 2}, "mana_cost_str": "{2}",
            "power": 2, "toughness": 2, "kicker_cost": m, "kicker_cost_str": "{1}{C}"})));
        assert_eq!(d.times_cost.as_ref().map(|t| t.mana), Some(mana("{1}{C}")), "kicker {m}");
    }
    let db = CardDb::cube();
    let d = db.def(db.id("Sowing Mycospawn").unwrap());
    assert_eq!(d.times_cost.as_ref().map(|t| t.mana), Some(mana("{1}{C}")));
}

/// Rule 4a (ENGINE.md 3): a structured cost that differs from its printed form
/// otherwise than by `{C}` counted as generic is a blocker, never a guess; so
/// is a printed form with no structured cost, and one that can't be read.
#[test]
fn a_structured_cost_that_differs_from_its_printed_form_is_a_blocker() {
    let bad = [
        ("Test Bad Spell", json!({"mana_cost": {"$t": "ManaCost", "generic": 3}, "mana_cost_str": "{1}{C}", "spell_effect": draw()}), "CardPrinting.mana_cost_str(differs from CardPrinting.mana_cost)"),
        ("Test Bad Colour", json!({"mana_cost": {"$t": "ManaCost", "generic": 1, "colored": [["U", 1]]}, "mana_cost_str": "{1}{C}", "spell_effect": draw()}), "CardPrinting.mana_cost_str(differs from CardPrinting.mana_cost)"),
        ("Test Bad Echo", json!({"mana_cost": {"$t": "ManaCost", "generic": 1}, "mana_cost_str": "{1}", "spell_effect": draw(),
            "flashback_cost_str": "{1}{C}"}), "CardPrinting.flashback_cost_str(without flashback_cost)"),
        ("Test Bad Text", json!({"mana_cost": {"$t": "ManaCost", "generic": 1}, "mana_cost_str": "{1}{Q}", "spell_effect": draw()}), "CardPrinting.mana_cost_str({1}{Q})"),
    ];
    for (name, fields, blocker) in bad {
        let e = lower(&card(name, &["Instant"], fields)).err().unwrap_or_else(|| panic!("{name}: lowered"));
        assert!(e.iter().any(|b| b == blocker), "{name}: {e:?}");
    }
    let ab = json!([{"$t": "ActivatedAbility", "cost": {"$t": "ActivationCost", "mana": {"$t": "ManaCost", "generic": 2}, "mana_str": "{2}{C}"}, "effect": draw()}]);
    let e = lower(&card("Test Bad Relic", &["Artifact"], json!({"mana_cost": {"$t": "ManaCost", "generic": 1}, "mana_cost_str": "{1}", "activated_abilities": ab}))).unwrap_err();
    assert!(e.iter().any(|b| b == "ActivationCost.mana_str(differs from ActivationCost.mana)"), "{e:?}");
}

/// Every card of the three tables whose printed mana cost has `{C}`: its
/// definition's cost has as many colorless pips (the IR today: Warping Wail
/// alone, lossy).
#[test]
fn cr_107_4c_every_printed_c_pip_in_a_mana_cost_is_colorless() {
    let mut found = Vec::new();
    for (db, doc) in [
        (CardDb::cube(), engine::db::load_cube_document()),
        (CardDb::constructed(), engine::db::load_legacy_document()),
        (CardDb::eoe(), engine::db::load_eoe_document()),
    ] {
        for c in &doc.cards {
            let card_ir::cats::CardPrinting::CardPrinting(p) = &c.printing;
            let n = p.mana_cost_str.matches("{C}").count() as u64;
            if n == 0 {
                continue;
            }
            let Some(id) = db.id(&c.name) else { continue };
            assert_eq!(db.def(id).cost.pips[C], n, "{}: {}", c.name, p.mana_cost_str);
            found.push(c.name.clone());
        }
    }
    assert_eq!(found, ["Warping Wail"]);
}
