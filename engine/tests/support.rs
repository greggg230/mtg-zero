//! The support report over the whole PowerLSV cube (ENGINE.md 3): which
//! cards the engine plays, and what blocks the others.
//!
//! `cargo test -p engine --test support -- --nocapture` prints the report.

mod common;

use engine::db::CardDb;
use engine::lower::support::SupportReport;

/// The supported set (M4b-1). Changing it is deliberate: update this list.
const SUPPORTED: &[&str] = &[
    "Ancestral Recall",
    "Ancient Tomb",
    "Avacyn's Pilgrim",
    "Badlands",
    "Baleful Strix",
    "Bayou",
    "Birds of Paradise",
    "Black Lotus",
    "Blackcleave Cliffs",
    "Blazemire Verge",
    "Bleachbone Verge",
    "Blood Crypt",
    "Blooming Marsh",
    "Botanical Sanctum",
    "Brainstorm",
    "Breeding Pool",
    "Cabal Ritual",
    "Cogwork Librarian",
    "Concealed Courtyard",
    "Copperline Gorge",
    "Counterspell",
    "Cut Down",
    "Dark Ritual",
    "Darkslick Shores",
    "Elvish Mystic",
    "Elvish Spirit Guide",
    "Exorcise",
    "Fatal Push",
    "Fiery Islet",
    "Flame Slash",
    "Flametongue Kavu",
    "Forest",
    "Gaea's Cradle",
    "Gleemox",
    "Goblin Bombardment",
    "Godless Shrine",
    "Griselbrand",
    "Guerrilla Gorilla",
    "Hallowed Fountain",
    "Horizon Canopy",
    "Hymn to Tourach",
    "Indatha Triome",
    "Inspiring Vantage",
    "Island",
    "Jetmir's Garden",
    "Ketria Triome",
    "Library of Alexandria",
    "Lightning Bolt",
    "Lion's Eye Diamond",
    "Llanowar Elves",
    "Lotus Petal",
    "Mana Confluence",
    "Mana Leak",
    "Mana Tithe",
    "Memory Lapse",
    "Mind Stone",
    "Mountain",
    "Mox Amber",
    "Mox Emerald",
    "Mox Jet",
    "Mox Opal",
    "Mox Pearl",
    "Mox Ruby",
    "Mox Sapphire",
    "Nurturing Peatland",
    "Overgrown Tomb",
    "Plains",
    "Plateau",
    "Pyrite Spellbomb",
    "Raffine's Tower",
    "Raugrin Triome",
    "Razorverge Thicket",
    "Sacred Foundry",
    "Savai Triome",
    "Savannah",
    "Scrubland",
    "Seachrome Coast",
    "Simian Spirit Guide",
    "Sol Ring",
    "Spara's Headquarters",
    "Spell Pierce",
    "Spirebluff Canal",
    "Starting Town",
    "Steam Vents",
    "Stern Scolding",
    "Stomping Ground",
    "Sunbaked Canyon",
    "Swamp",
    "Sylvan Caryatid",
    "Taiga",
    "Talisman of Conviction",
    "Talisman of Creativity",
    "Talisman of Curiosity",
    "Talisman of Dominance",
    "Talisman of Indulgence",
    "Talisman of Progress",
    "Temple Garden",
    "Thornspire Verge",
    "Time Walk",
    "Tolarian Academy",
    "Tropical Island",
    "Tundra",
    "Underground Sea",
    "Volcanic Island",
    "Waterlogged Grove",
    "Watery Grave",
    "Xander's Lounge",
    "Zagoth Triome",
    "Ziatora's Proving Ground",
];

#[test]
fn cube_support_report() {
    let db = CardDb::cube();
    let r = SupportReport::from_db(db);
    if let Ok(path) = std::env::var("SUPPORT_DUMP") {
        // Every blocked card with all its blockers, one per line.
        let mut s = String::new();
        for (name, b) in &r.blocked {
            s.push_str(&format!("{name}\t{}\n", b.join(" | ")));
        }
        std::fs::write(&path, s).unwrap();
    }
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
    p.has_storm = true;
    let db = CardDb::from_cards(&cards);
    let err = db.expect_id("Lightning Bolt").unwrap_err();
    assert!(err.contains("CardPrinting.has_storm"), "{err}");
}

#[test]
fn custom_test_cards_lower() {
    let db = common::db();
    for c in common::custom_cards() {
        db.expect_id(&c.name).unwrap();
    }
}
