//! The support report over the whole PowerLSV cube (ENGINE.md 3): which
//! cards the engine plays, and what blocks the others.
//!
//! `cargo test -p engine --test support -- --nocapture` prints the report.

mod common;

use engine::db::CardDb;
use engine::lower::support::{threat_kinds, SupportReport, Threat};

/// The supported set (M4b-2). Changing it is deliberate: update this list.
const SUPPORTED: &[&str] = &[
    "Abandoned Air Temple",
    "Abrade",
    "Abrupt Decay",
    "Aether Spellbomb",
    "Ancestral Recall",
    "Ancient Tomb",
    "Arid Mesa",
    "Avacyn's Pilgrim",
    "Badlands",
    "Baleful Strix",
    "Baloth Prime",
    "Basalt Monolith",
    "Bayou",
    "Birds of Paradise",
    "Bitter Triumph",
    "Black Lotus",
    "Blackcleave Cliffs",
    "Blazemire Verge",
    "Bleachbone Verge",
    "Blood Crypt",
    "Bloodstained Mire",
    "Blooming Marsh",
    "Bone Shards",
    "Botanical Sanctum",
    "Brainstorm",
    "Breeding Pool",
    "Cabal Ritual",
    "Cathar Commando",
    "Cogwork Librarian",
    "Commercial District",
    "Concealed Courtyard",
    "Consider",
    "Copperline Gorge",
    "Counterspell",
    "Crop Rotation",
    "Cut Down",
    "Dark Confidant",
    "Dark Ritual",
    "Darkslick Shores",
    "Demonic Tutor",
    "Dismember",
    "Duress",
    "Eagles of the North",
    "Elegant Parlor",
    "Elvish Mystic",
    "Elvish Spirit Guide",
    "Enlightened Tutor",
    "Entomb",
    "Erode",
    "Exorcise",
    "Expedition Map",
    "Fatal Push",
    "Fiery Confluence",
    "Fiery Islet",
    "Figure of Destiny",
    "Figure of Fable",
    "Flame Slash",
    "Flametongue Kavu",
    "Flooded Strand",
    "Forest",
    "Gaea's Cradle",
    "Generous Ent",
    "Gitaxian Probe",
    "Giver of Runes",
    "Gleemox",
    "Goblin Bombardment",
    "Godless Shrine",
    "Grim Monolith",
    "Griselbrand",
    "Guerrilla Gorilla",
    "Hallowed Fountain",
    "Harvester of Misery",
    "Haywire Mite",
    "Hedge Maze",
    "Horizon Canopy",
    "Hymn to Tourach",
    "Ignoble Hierarch",
    "Imperial Seal",
    "Indatha Triome",
    "Inquisition of Kozilek",
    "Inspiring Vantage",
    "Iron-Shield Elf",
    "Island",
    "Jetmir's Garden",
    "Karakas",
    "Ketria Triome",
    "Knight of Autumn",
    "Legion Extruder",
    "Library of Alexandria",
    "Lightning Bolt",
    "Lightning Greaves",
    "Lion's Eye Diamond",
    "Llanowar Elves",
    "Loran of the Third Path",
    "Lotus Cobra",
    "Lotus Petal",
    "Luminarch Aspirant",
    "Lush Portico",
    "Lórien Revealed",
    "Magda, Brazen Outlaw",
    "Malevolent Rumble",
    "Mana Confluence",
    "Mana Leak",
    "Mana Tithe",
    "Manamorphose",
    "Manifold Key",
    "Marsh Flats",
    "Memory Lapse",
    "Meticulous Archive",
    "Mind Stone",
    "Misty Rainforest",
    "Mother of Runes",
    "Mountain",
    "Mox Amber",
    "Mox Emerald",
    "Mox Jet",
    "Mox Opal",
    "Mox Pearl",
    "Mox Ruby",
    "Mox Sapphire",
    "Mystic Confluence",
    "Mystical Tutor",
    "Natural Order",
    "No More Lies",
    "Noble Hierarch",
    "Nurturing Peatland",
    "Oliphaunt",
    "Oust",
    "Overgrown Tomb",
    "Path to Exile",
    "Plains",
    "Plateau",
    "Polluted Delta",
    "Ponder",
    "Preordain",
    "Prismatic Vista",
    "Pyrite Spellbomb",
    "Pyroblast",
    "Raffine's Tower",
    "Raucous Theater",
    "Raugrin Triome",
    "Ravenous Robots",
    "Razorverge Thicket",
    "Relic of Sauron",
    "Remand",
    "Reprieve",
    "Restless Vinestalk",
    "Retrofitter Foundry",
    "Sacred Foundry",
    "Sanguine Evangelist",
    "Savai Triome",
    "Savannah",
    "Scalding Tarn",
    "Scrubland",
    "Seachrome Coast",
    "Sear",
    "Sensei's Divining Top",
    "Shadowy Backstreet",
    "Sheoldred's Edict",
    "Sheoldred, the Apocalypse",
    "Simian Spirit Guide",
    "Skateboard",
    "Sol Ring",
    "Spara's Headquarters",
    "Spectacular Spider-Man",
    "Spell Pierce",
    "Spellseeker",
    "Spirebluff Canal",
    "Starting Town",
    "Steam Vents",
    "Stern Scolding",
    "Stock Up",
    "Stomping Ground",
    "Stoneforge Mystic",
    "Sunbaked Canyon",
    "Super Shredder",
    "Swamp",
    "Swords to Plowshares",
    "Sylvan Caryatid",
    "Sylvan Safekeeper",
    "Taiga",
    "Talisman of Conviction",
    "Talisman of Creativity",
    "Talisman of Curiosity",
    "Talisman of Dominance",
    "Talisman of Indulgence",
    "Talisman of Progress",
    "Temple Garden",
    "Thopter Foundry",
    "Thornspire Verge",
    "Thoughtseize",
    "Thraben Inspector",
    "Thundering Falls",
    "Time Walk",
    "Time Warp",
    "Timetwister",
    "Tinker",
    "Tolarian Academy",
    "Trinket Mage",
    "Troll of Khazad-dûm",
    "Tropical Island",
    "Tundra",
    "Undercity Sewers",
    "Underground Mortuary",
    "Underground Sea",
    "Unholy Heat",
    "Vampiric Tutor",
    "Verdant Catacombs",
    "Vindicate",
    "Volcanic Island",
    "Voltaic Key",
    "Wasteland",
    "Waterlogged Grove",
    "Watery Grave",
    "Wheel of Fortune",
    "Windswept Heath",
    "Wooded Foothills",
    "Worldly Tutor",
    "Wrath of God",
    "Xander's Lounge",
    "Zagoth Triome",
    "Ziatora's Proving Ground",
    "Zuran Orb",
];

#[test]
fn cube_support_report() {
    let db = CardDb::cube();
    let r = SupportReport::from_db(db);
    if let Ok(path) = std::env::var("SUPPORT_LIST") {
        // The supported names, one per line (to update `SUPPORTED`).
        std::fs::write(&path, r.supported.join("
")).unwrap();
    }
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

/// The threat view (ENGINE.md 16): which blocking features stand between
/// the engine and the cube's creatures and other ways to win (planeswalkers
/// not counted), and a greedy staging order over threats only.
/// `cargo test -p engine --test support threat -- --nocapture`.
#[test]
fn threat_report() {
    let db = CardDb::cube();
    let r = SupportReport::from_db(db);
    let kinds = threat_kinds(engine::db::CUBE_IR);
    let count = |names: &mut dyn Iterator<Item = &str>, k: Option<Threat>| {
        names.filter(|n| kinds.get(*n).is_some_and(|t| k.is_none_or(|k| *t == k))).count()
    };
    let sup_creatures = count(&mut r.supported.iter().map(|s| s.as_str()), Some(Threat::Creature));
    let sup_threats = count(&mut r.supported.iter().map(|s| s.as_str()), None);
    let blk_creatures = count(&mut r.blocked.iter().map(|(n, _)| n.as_str()), Some(Threat::Creature));
    let blk_threats = count(&mut r.blocked.iter().map(|(n, _)| n.as_str()), None);
    println!("threats: supported {sup_threats} ({sup_creatures} creatures), blocked {blk_threats} ({blk_creatures} creatures)");
    for t in [Threat::Creature, Threat::Vehicle, Threat::Equipment, Threat::Manland, Threat::Tokens, Threat::Burn] {
        let s = count(&mut r.supported.iter().map(|s| s.as_str()), Some(t));
        let b = count(&mut r.blocked.iter().map(|(n, _)| n.as_str()), Some(t));
        println!("  {t:?}: {s} supported, {b} blocked");
    }
    println!("{:<64} {:>5} {:>7}", "feature (threats it blocks)", "alone", "threats");
    for f in r.threat_features(&kinds).into_iter().take(40) {
        println!("{:<64} {:>5} {:>7}  {}", f.feature, f.alone, f.threats, f.cards.join(", "));
    }
    println!("greedy staging over threats (feature, threats completed):");
    for (f, n, cards) in r.threat_greedy(&kinds, 40) {
        println!("  {f:<62} {n:>3}  {}", cards.join(", "));
    }
    assert!(sup_creatures > 0 && blk_threats > 0);
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
