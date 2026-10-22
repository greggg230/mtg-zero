//! Deserialize every card of the checked-in PowerLSV cube IR into the
//! generated types and report how many objects of each category it holds.

use card_ir::{category_of, load_document, Document, LoadError, SCHEMA_SHA256};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn data_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/cards").join(name)
}

fn cube_json() -> String {
    std::fs::read_to_string(data_path("powerlsv.ir.json")).expect("data/cards/powerlsv.ir.json")
}

fn cube() -> Document {
    load_document(&cube_json()).unwrap_or_else(|e| panic!("{e}"))
}

/// Count `$t` objects per category in a JSON tree; every tag must be known.
fn count_categories(v: &serde_json::Value, out: &mut BTreeMap<&'static str, usize>) {
    match v {
        serde_json::Value::Object(m) => {
            if let Some(tag) = m.get("$t").and_then(|t| t.as_str()) {
                let cat = category_of(tag).unwrap_or_else(|| panic!("unknown tag {tag}"));
                *out.entry(cat).or_default() += 1;
            }
            m.values().for_each(|x| count_categories(x, out));
        }
        serde_json::Value::Array(a) => a.iter().for_each(|x| count_categories(x, out)),
        _ => {}
    }
}

#[test]
fn every_cube_card_deserializes() {
    let doc = cube();
    assert_eq!(doc.pool, "cube");
    assert_eq!(doc.schema_sha256, SCHEMA_SHA256);
    assert_eq!(doc.cards.len(), 546, "PowerLSV cube: 541 cards + 5 basics");
    let mut names: Vec<&str> = doc.cards.iter().map(|c| c.name.as_str()).collect();
    names.dedup();
    assert_eq!(names.len(), doc.cards.len(), "card names are unique and sorted");
    for c in &doc.cards {
        assert!(c.custom || c.oracle_id.as_deref().is_some_and(|s| s.len() == 36), "{}", c.name);
    }
}

#[test]
fn per_category_counts() {
    let raw: serde_json::Value = serde_json::from_str(&cube_json()).unwrap();
    let mut counts = BTreeMap::new();
    count_categories(&raw["cards"], &mut counts);
    let total: usize = counts.values().sum();
    println!("{:<28} {:>7}", "category", "objects");
    for (cat, n) in &counts {
        println!("{cat:<28} {n:>7}");
    }
    println!("{:<28} {:>7}", "total", total);
    assert!(counts["CardPrinting"] >= 546);
    for cat in ["Effect", "ObjectPredicate", "TriggerPredicate", "Condition", "Formula", "CostAtom"] {
        assert!(counts.get(cat).copied().unwrap_or(0) > 0, "no {cat} objects");
    }
}

fn first_card_with(field: &str) -> serde_json::Value {
    let raw: serde_json::Value = serde_json::from_str(&cube_json()).unwrap();
    raw["cards"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["printing"].get(field).is_some())
        .cloned()
        .unwrap_or_else(|| panic!("no card sets {field}"))
}

fn reject(card: serde_json::Value) -> String {
    let doc = serde_json::json!({
        "format": "mtg-bot-card-ir",
        "ir_version": card_ir::IR_VERSION,
        "pool": "test",
        "schema_sha256": SCHEMA_SHA256,
        "cards": [card],
    });
    match load_document(&doc.to_string()) {
        Err(LoadError::Card { error, .. }) => error.to_string(),
        other => panic!("expected a card error, got {other:?}"),
    }
}

#[test]
fn unknown_tags_fields_and_values_are_errors() {
    let mut c = first_card_with("triggered_abilities");
    c["printing"]["triggered_abilities"][0]["$t"] = serde_json::json!("NoSuchAbility");
    assert!(reject(c).contains("NoSuchAbility"));

    let mut c = first_card_with("power");
    c["printing"]["no_such_field"] = serde_json::json!(1);
    assert!(reject(c).contains("no_such_field"));

    let mut c = first_card_with("keywords");
    c["printing"]["keywords"][0]["kind"] = serde_json::json!("teleport");
    assert!(reject(c).contains("teleport"));
}

#[test]
fn header_mismatches_are_errors() {
    let text = cube_json().replacen(SCHEMA_SHA256, "0000", 1);
    assert!(matches!(load_document(&text), Err(LoadError::Schema { .. })));
    let text = cube_json().replacen("\"ir_version\":1", "\"ir_version\":99", 1);
    assert!(matches!(load_document(&text), Err(LoadError::Version(99))));
}

#[test]
fn generated_types_match_the_checked_in_schema() {
    let text = std::fs::read_to_string(data_path("powerlsv.schema.json")).unwrap();
    let schema: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(schema["format"], "mtg-bot-card-ir-schema");
    assert_eq!(schema["ir_version"], card_ir::IR_VERSION);
    let classes = schema["classes"].as_object().unwrap();
    assert_eq!(classes.len(), card_ir::TAG_CATEGORY.len(), "regenerate card_ir/src/generated.rs");
    for (tag, class) in classes {
        assert_eq!(
            category_of(tag),
            class["category"].as_str(),
            "{tag}: regenerate card_ir/src/generated.rs"
        );
    }
}
