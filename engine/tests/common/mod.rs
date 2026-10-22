//! Shared fixtures: the cube plus custom test cards written as card IR, so
//! they go through the same lowering as real cards.

#![allow(dead_code)]

use engine::db::CardDb;
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
    let cards = vec![
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
        creature("Test Giant", "{4}{G}", 5, 5, &["trample", "deathtouch"], false, none),
        creature("Test Dies Shock", "{1}{R}", 1, 1, &[], false, dies_shock),
    ];
    cards.into_iter().map(|v| serde_json::from_value(v).expect("custom card IR")).collect()
}

/// The cube plus the custom cards, lowered once per test binary.
pub fn db() -> &'static CardDb {
    static DB: OnceLock<&'static CardDb> = OnceLock::new();
    DB.get_or_init(|| {
        let mut cards = engine::db::load_cube_document().cards;
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
        for name in db.supported() {
            let id = db.id(name).unwrap();
            if db.def(id).is(engine::db::types::LAND) {
                lands.push(id);
            } else {
                spells.push(id);
            }
        }
        (lands, spells)
    })
}

/// A random 40-card deck: 17 lands, 23 spells (duplicates allowed).
pub fn random_deck(_db: &CardDb, rng: &mut engine::rng::Rng) -> Vec<u16> {
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
