//! The test fixtures `engine/tests/common` shares (moved here with the fuzz loops, docs/ENGINE.md
//! 114.1; `tests/common` re-exports them): the cube plus custom test cards written as card IR, so
//! they go through the same lowering as real cards; the fuzz pool and random decks.

use crate::db::CardDb;
use serde_json::{json, Value};
use std::sync::OnceLock;

fn cost(s: &str) -> Value {
    let mut generic = 0;
    let mut colored: Vec<(String, i64)> = Vec::new();
    for sym in s.trim_start_matches('{').trim_end_matches('}').split("}{").filter(|x| !x.is_empty()) {
        if let Ok(n) = sym.parse::<i64>() {
            generic += n;
        } else if let Some(e) = colored.iter_mut().find(|e| e.0 == sym) {
            e.1 += 1;
        } else {
            colored.push((sym.to_string(), 1));
        }
    }
    let mut m = json!({"$t": "ManaCost"});
    if generic > 0 {
        m["generic"] = json!(generic);
    }
    if !colored.is_empty() {
        m["colored"] = json!(colored);
    }
    m
}

/// A custom creature card in IR form.
fn creature(name: &str, mana: &str, p: i64, t: i64, kws: &[&str], legendary: bool, extra: Value) -> Value {
    let mut pr = json!({
        "$t": "CardPrinting",
        "name": name,
        "mana_cost_str": mana,
        "mana_cost": cost(mana),
        "type_line": "Creature - Test",
        "supertypes": if legendary { json!(["Legendary"]) } else { json!([]) },
        "card_types": ["Creature"],
        "subtypes": ["Test"],
        "power": p,
        "toughness": t,
    });
    if !kws.is_empty() {
        pr["keywords"] = json!(kws.iter().map(|k| json!({"$t": "Keyword", "kind": k})).collect::<Vec<_>>());
    }
    if let Value::Object(m) = extra {
        for (k, v) in m {
            pr[k] = v;
        }
    }
    json!({"name": name, "custom": true, "printing": pr})
}

pub fn custom_cards() -> Vec<card_ir::Card> {
    let none = json!({});
    let dies_draw = json!({"triggered_abilities": [{"$t": "TriggeredAbility", "event": "dies",
        "predicate": {"$t": "IsSelf"}, "effect": {"$t": "DrawCards", "amount": 1}}]});
    let dies_ping = json!({"triggered_abilities": [{"$t": "TriggeredAbility", "event": "dies",
        "predicate": {"$t": "IsSelf"}, "effect": {"$t": "DealDamageAnyTarget", "amount": 1,
        "targets": [{"$t": "TargetSpec", "filter": {"$t": "OneOf", "filters": [
            {"$t": "IsCreature"}, {"$t": "IsPlayer"}]}}]}}]});
    let dies_shock = json!({"triggered_abilities": [{"$t": "TriggeredAbility", "event": "dies",
        "predicate": {"$t": "IsSelf"}, "effect": {"$t": "DealDamageToTargetCreature", "amount": 1,
        "targets": [{"$t": "TargetSpec", "filter": {"$t": "IsCreature"}}]}}]});
    // When this enters, manifest dread (M7d: face-down permanents, fuzzed
    // often).
    let manifest = json!({"triggered_abilities": [{"$t": "TriggeredAbility", "event": "enters",
        "predicate": {"$t": "IsSelf"}, "effect": {"$t": "ManifestDread"}}]});
    let cards = vec![
        creature("Test Manifester", "{1}{U}", 1, 1, &[], false, manifest),
        creature("Test Bear", "{1}{G}", 2, 2, &[], false, none.clone()),
        creature("Test Flyer", "{1}{U}", 2, 2, &["flying"], false, none.clone()),
        creature("Test Reacher", "{1}{G}", 1, 3, &["reach"], false, none.clone()),
        creature("Test Striker", "{1}{W}", 2, 1, &["first strike"], false, none.clone()),
        creature("Test Double", "{2}{R}", 2, 2, &["double strike"], false, none.clone()),
        creature("Test Deathtouch", "{B}", 1, 1, &["deathtouch"], false, none.clone()),
        creature("Test Trampler", "{2}{G}{G}", 4, 4, &["trample"], false, none.clone()),
        creature("Test Lifelinker", "{1}{W}", 2, 2, &["lifelink"], false, none.clone()),
        creature("Test Vigilant", "{1}{W}", 2, 3, &["vigilance"], false, none.clone()),
        creature("Test Hasty", "{R}", 2, 1, &["haste"], false, none.clone()),
        creature("Test Wall", "{1}{W}", 0, 4, &["defender"], false, none.clone()),
        creature("Test Menace", "{2}{B}", 3, 2, &["menace"], false, none.clone()),
        creature("Test Flash", "{1}{U}", 2, 2, &["flash"], false, none.clone()),
        creature("Test Legend", "{1}{W}", 2, 2, &[], true, none.clone()),
        creature("Test Zero", "{1}", 0, 0, &[], false, none.clone()),
        creature("Test Dies Draw", "{1}{B}", 2, 2, &[], false, dies_draw),
        creature("Test Dies Ping", "{1}{R}", 2, 2, &[], false, dies_ping),
        creature("Test Giant", "{4}{G}", 5, 5, &["trample", "deathtouch"], false, none.clone()),
        creature("Test Dies Shock", "{1}{R}", 1, 1, &[], false, dies_shock),
        creature("Test Dwarf", "{1}{R}", 2, 2, &[], false, json!({"subtypes": ["Dwarf"]})),
        // {T}: 1 damage to any target (summoning sickness, CR 302.6).
        creature("Test Tapper", "{1}{R}", 1, 1, &[], false, tap_ping()),
        creature("Test Hasty Tapper", "{1}{R}", 1, 1, &["haste"], false, tap_ping()),
        creature("Test Indestructible", "{2}{W}", 2, 2, &["indestructible"], false, none.clone()),
        // "This creature crews Vehicles as though its power were 3 greater."
        creature("Test Pilot", "{1}", 0, 1, &[], false, json!({"crew_power_bonus": 3})),
        // When this enters, you may draw a card.
        creature(
            "Test May Draw",
            "{1}{U}",
            1,
            1,
            &[],
            false,
            json!({"triggered_abilities": [{"$t": "TriggeredAbility", "event": "enters",
                "predicate": {"$t": "IsSelf"},
                "effect": {"$t": "MayDo", "inner": {"$t": "DrawCards", "amount": 1}}}]}),
        ),
        // When this enters, choose one: you gain 3 life; or 1 damage to any
        // target (a modal trigger, CR 603.3c).
        creature(
            "Test Modal Entry",
            "{1}{W}",
            1,
            1,
            &[],
            false,
            json!({"triggered_abilities": [{"$t": "TriggeredAbility", "event": "enters",
                "predicate": {"$t": "IsSelf"},
                "effect": {"$t": "ModalChooseOne", "modes": [
                    {"$t": "GainLife", "amount": 3},
                    {"$t": "DealDamageAnyTarget", "amount": 1, "targets": [{"$t": "TargetSpec",
                        "filter": {"$t": "OneOf", "filters": [{"$t": "IsCreature"}, {"$t": "IsPlayer"}]}}]}]}}]}),
        ),
        // When this enters, choose up to one: you gain 3 life.
        creature(
            "Test Up To One",
            "{1}{W}",
            1,
            1,
            &[],
            false,
            json!({"triggered_abilities": [{"$t": "TriggeredAbility", "event": "enters",
                "predicate": {"$t": "IsSelf"},
                "effect": {"$t": "ModalChooseOne", "modes": [
                    {"$t": "GainLife", "amount": 3}, {"$t": "ChooseNoMode"}]}}]}),
        ),
        // Double-faced test cards (CR 712): "{1}: Transform this creature."
        // on both faces; a back face that is a planeswalker; and a
        // single-faced card with the same ability (CR 701.27c).
        creature(
            "Test Flipper",
            "{1}{G}",
            2,
            2,
            &[],
            false,
            json!({"activated_abilities": [transform_ability()],
                "transform_back_face": back_face("Test Flipped", json!({"card_types": ["Creature"], "subtypes": ["Test"],
                    "power": 3, "toughness": 3, "face_colors": ["G"], "activated_abilities": [transform_ability()]}))}),
        ),
        creature(
            "Test Walker Flip",
            "{1}{U}",
            1,
            1,
            &[],
            false,
            json!({"activated_abilities": [transform_ability()],
                "transform_back_face": back_face("Test Flipped Walker", json!({"card_types": ["Planeswalker"],
                    "subtypes": ["Test"], "loyalty": 3, "face_colors": ["U"],
                    "activated_abilities": [{"$t": "ActivatedAbility", "sorcery_speed": true,
                        "cost": {"$t": "ActivationCost", "loyalty": -1},
                        "effect": {"$t": "DrawCards", "amount": 1}}]}))}),
        ),
        creature("Test Not Flipper", "{1}", 1, 1, &[], false, json!({"activated_abilities": [transform_ability()]})),
        creature("Test Cat", "{1}{W}", 2, 1, &[], false, json!({"subtypes": ["Cat"]})),
        // Daybound with a transform ability (CR 702.145b: it can't
        // transform except through day and night).
        creature(
            "Test Werewolf",
            "{1}{G}",
            2,
            2,
            &[],
            false,
            json!({"daybound": true, "activated_abilities": [transform_ability()],
                "transform_back_face": back_face("Test Nightwolf", json!({"card_types": ["Creature"], "subtypes": ["Test"],
                    "power": 3, "toughness": 3, "face_colors": ["G"], "nightbound": true}))}),
        ),
        spell(
            "Test Phyrexian Shock",
            "Instant",
            json!({"$t": "ManaCost", "phyrexian": ["R"]}),
            json!({"$t": "DealDamageAnyTarget", "amount": 2, "targets": [{"$t": "TargetSpec",
                "filter": {"$t": "OneOf", "filters": [{"$t": "IsCreature"}, {"$t": "IsPlayer"}]}}]}),
            none.clone(),
        ),
        // As an additional cost, discard a card. Draw a card.
        spell(
            "Test Rummage",
            "Instant",
            cost("{R}"),
            json!({"$t": "DrawCards", "amount": 1}),
            json!({"additional_cost_options": [{"$t": "AdditionalCostOption", "kind": "discard", "amount": 1}]}),
        ),
        // As an additional cost, sacrifice an artifact (Tinker's cost).
        spell(
            "Test Artifact Offering",
            "Sorcery",
            cost("{1}"),
            json!({"$t": "DrawCards", "amount": 1}),
            json!({"additional_cost_sacrifice_filter": {"$t": "CardFilter", "any_types": ["Artifact"]}}),
        ),
        // "You take the initiative." (An instant: the initiative at instant
        // speed, CR 701.49c.)
        spell("Test Initiative", "Instant", cost("{U}"), json!({"$t": "TakeInitiative"}), none.clone()),
        // "This spell costs {1} less to cast for each artifact you control.
        // You may pay {2}{U} rather than pay this spell's mana cost. Draw a
        // card." (A reduction applies to an alternative cost, CR 601.2f.)
        spell(
            "Test Discount Alt",
            "Instant",
            cost("{3}{U}"),
            json!({"$t": "DrawCards", "amount": 1}),
            json!({"cost_reductions": [{"$t": "CostReduction", "amount": 1,
                    "condition": {"$t": "PerControlled", "filter": {"$t": "IsArtifact"}}}],
                "alternative_costs": [{"$t": "AlternativeCost", "mana": "{2}{U}"}]}),
        ),
        // "Exile target card from a graveyard. Exile target card from a
        // graveyard." Two instructions: two events (CR 603.2c; "one or
        // more" triggers, ENGINE.md shortcuts).
        spell(
            "Test Twice Exile",
            "Sorcery",
            cost("{B}"),
            json!({"$t": "Sequence", "effects": [{"$t": "ExileTargetCardFromGraveyard"}, {"$t": "ExileTargetCardFromGraveyard"}]}),
            none.clone(),
        ),
        // "Put target permanent on top of its owner's library. Put target
        // permanent on top of its owner's library." (Two events.)
        spell(
            "Test Twice Oust",
            "Sorcery",
            cost("{W}"),
            json!({"$t": "Sequence", "effects": [{"$t": "PutTargetPermanentIntoOwnersLibrary"}, {"$t": "PutTargetPermanentIntoOwnersLibrary"}]}),
            none.clone(),
        ),
    ];
    cards.into_iter().map(|v| serde_json::from_value(v).expect("custom card IR")).collect()
}

/// "{1}: Transform this creature."
fn transform_ability() -> Value {
    json!({"$t": "ActivatedAbility", "cost": {"$t": "ActivationCost", "mana": cost("{1}"), "mana_str": "{1}"},
        "effect": {"$t": "TransformSource"}})
}

/// A back face printing (no mana cost of its own, CR 712.8e).
fn back_face(name: &str, extra: Value) -> Value {
    let mut pr = json!({
        "$t": "CardPrinting",
        "name": name,
        "mana_cost_str": "",
        "mana_cost": {"$t": "ManaCost"},
        "type_line": "",
        "supertypes": [],
    });
    if let Value::Object(m) = extra {
        for (k, v) in m {
            pr[k] = v;
        }
    }
    pr
}

fn tap_ping() -> Value {
    json!({"activated_abilities": [{"$t": "ActivatedAbility",
        "cost": {"$t": "ActivationCost", "tap_self": true},
        "effect": {"$t": "DealDamageAnyTarget", "amount": 1, "targets": [{"$t": "TargetSpec",
            "filter": {"$t": "OneOf", "filters": [{"$t": "IsCreature"}, {"$t": "IsPlayer"}]}}]}}]})
}

/// A custom instant or sorcery card in IR form.
fn spell(name: &str, kind: &str, mana_cost: Value, effect: Value, extra: Value) -> Value {
    let mut pr = json!({
        "$t": "CardPrinting",
        "name": name,
        "mana_cost_str": "",
        "mana_cost": mana_cost,
        "type_line": kind,
        "supertypes": [],
        "card_types": [kind],
        "subtypes": [],
        "spell_effect": effect,
    });
    if let Value::Object(m) = extra {
        for (k, v) in m {
            pr[k] = v;
        }
    }
    json!({"name": name, "custom": true, "printing": pr})
}

/// The cube plus the custom cards, lowered once per test binary.
pub fn db() -> &'static CardDb {
    static DB: OnceLock<&'static CardDb> = OnceLock::new();
    DB.get_or_init(|| {
        let mut cards = crate::db::load_cube_document().cards;
        cards.extend(custom_cards());
        let db = CardDb::from_cards(&cards);
        for c in custom_cards() {
            db.expect_id(&c.name).unwrap_or_else(|e| panic!("{e}"));
        }
        db.leak()
    })
}

/// The fuzz pool: every supported card of the cube plus the custom test
/// cards, split into lands and the rest (so every newly supported card is
/// fuzzed without editing a list).
pub fn fuzz_pool() -> &'static (Vec<u16>, Vec<u16>) {
    static POOL: OnceLock<(Vec<u16>, Vec<u16>)> = OnceLock::new();
    POOL.get_or_init(|| {
        let db = db();
        let (mut lands, mut spells) = (Vec::new(), Vec::new());
        // `POOL_EXCLUDE=<file>`: names (one per line) left out of the pool,
        // to play an earlier milestone's games (ENGINE.md 17).
        let exclude: Vec<String> = super::env::var("POOL_EXCLUDE")
            .ok()
            .map(|p| std::fs::read_to_string(p).unwrap().lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
            .unwrap_or_default();
        for name in db.supported() {
            if exclude.iter().any(|x| x == name) {
                continue;
            }
            let id = db.id(name).unwrap();
            if db.def(id).is(crate::db::types::LAND) {
                lands.push(id);
            } else {
                spells.push(id);
            }
        }
        (lands, spells)
    })
}

/// A random 40-card deck: 17 lands, 23 spells (duplicates allowed).
pub fn random_deck(_db: &CardDb, rng: &mut crate::rng::Rng) -> Vec<u16> {
    let (lands, spells) = fuzz_pool();
    let mut d = Vec::with_capacity(40);
    for _ in 0..17 {
        d.push(lands[rng.below(lands.len() as u32) as usize]);
    }
    for _ in 0..23 {
        d.push(spells[rng.below(spells.len() as u32) as usize]);
    }
    d
}
