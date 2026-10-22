//! The support report over the whole PowerLSV cube (ENGINE.md 3): which
//! cards the engine plays, and what blocks the others.
//!
//! `cargo test -p engine --test support -- --nocapture` prints the report.

mod common;

use engine::db::CardDb;
use engine::lower::support::SupportReport;

/// The M4a supported set. Changing it is deliberate: update this list.
const SUPPORTED: &[&str] = &[
    "Ancestral Recall",
    "Badlands",
    "Baleful Strix",
    "Bayou",
    "Brainstorm",
    "Cogwork Librarian",
    "Counterspell",
    "Cut Down",
    "Dark Ritual",
    "Flame Slash",
    "Flametongue Kavu",
    "Forest",
    "Hymn to Tourach",
    "Island",
    "Lightning Bolt",
    "Mana Leak",
    "Mana Tithe",
    "Memory Lapse",
    "Mountain",
    "Plains",
    "Plateau",
    "Savannah",
    "Scrubland",
    "Spell Pierce",
    "Swamp",
    "Taiga",
    "Time Walk",
    "Tropical Island",
    "Tundra",
    "Underground Sea",
    "Volcanic Island",
];

#[test]
fn cube_support_report() {
    let db = CardDb::cube();
    let r = SupportReport::from_db(db);
    println!("{}", r.render(40));
    println!("greedy staging (feature, cards it completes, cumulative):");
    for (f, n, total) in r.greedy(30) {
        println!("  {f:<60} {n:>4} {total:>5}");
    }
    assert_eq!(r.total(), 546);
    assert_eq!(r.supported, SUPPORTED);
    // Every unsupported card names at least one blocker.
    assert!(r.blocked.iter().all(|(_, b)| !b.is_empty()));
}

#[test]
fn unsupported_cards_fail_loudly() {
    let db = CardDb::cube();
    let err = db.expect_id("Mana Drain").unwrap_err();
    assert!(err.contains("Effect:CounterThenDelayedManaEqualToMV"), "{err}");
    let err = db.expect_id("Force of Will").unwrap_err();
    assert!(err.contains("CardPrinting.alternative_costs"), "{err}");
    assert!(db.expect_id("No Such Card").is_err());
}

#[test]
fn a_new_ir_field_value_is_a_blocker() {
    // A supported card with one unsupported flag set is no longer supported.
    let mut cards = engine::db::load_cube_document().cards;
    let bolt = cards.iter_mut().find(|c| c.name == "Lightning Bolt").unwrap();
    let card_ir::cats::CardPrinting::CardPrinting(p) = &mut bolt.printing;
    p.enters_tapped = true;
    let db = CardDb::from_cards(&cards);
    let err = db.expect_id("Lightning Bolt").unwrap_err();
    assert!(err.contains("CardPrinting.enters_tapped"), "{err}");
}

#[test]
fn custom_test_cards_lower() {
    let db = common::db();
    for c in common::custom_cards() {
        db.expect_id(&c.name).unwrap();
    }
}
