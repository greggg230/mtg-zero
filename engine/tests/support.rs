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
    "Adeline, Resplendent Cathar",
    "Aether Spellbomb",
    "Agent Bishop, Man in Black",
    "Alpha Deathclaw",
    "Ancestral Recall",
    "Ancient Tomb",
    "Aragorn, King of Gondor",
    "Archon of Cruelty",
    "Archon of Emeria",
    "Arid Mesa",
    "Arwen, Mortal Queen",
    "Atraxa, Grand Unifier",
    "Auriok Salvagers",
    "Avacyn's Pilgrim",
    "Badlands",
    "Baleful Mastery",
    "Baleful Strix",
    "Baloth Prime",
    "Barrowgoyf",
    "Basalt Monolith",
    "Bayou",
    "Bejeweled Warg",
    "Birds of Paradise",
    "Bitter Triumph",
    "Black Lotus",
    "Blackcleave Cliffs",
    "Blazemire Verge",
    "Bleachbone Verge",
    "Blood Crypt",
    "Bloodstained Mire",
    "Bloodtithe Harvester",
    "Blooming Marsh",
    "Bone Shards",
    "Boomerang Basics",
    "Botanical Sanctum",
    "Brainstorm",
    "Breeding Pool",
    "Bristly Bill, Spine Sower",
    "Burst Lightning",
    "Cabal Ritual",
    "Carnage Interpreter",
    "Cathar Commando",
    "Chandra, Torch of Defiance",
    "Chaos Defiler",
    "Clarion Conqueror",
    "Cogwork Librarian",
    "Collective Brutality",
    "Comet, Stellar Pup",
    "Commercial District",
    "Concealed Courtyard",
    "Consider",
    "Consult the Star Charts",
    "Containment Priest",
    "Copperline Gorge",
    "Cori-Steel Cutter",
    "Corpse Dance",
    "Council's Judgment",
    "Counterspell",
    "Crop Rotation",
    "Crucible of Worlds",
    "Cryogen Relic",
    "Cut Down",
    "Dack Fayden",
    "Damn",
    "Dark Confidant",
    "Dark Ritual",
    "Darkslick Shores",
    "Daze",
    "Death-Greeter's Champion",
    "Deep-Cavern Bat",
    "Demonic Tutor",
    "Dig Through Time",
    "Dismember",
    "Displacer Kitten",
    "Dragon's Rage Channeler",
    "Duress",
    "Eagles of the North",
    "Echo of Eons",
    "Elegant Parlor",
    "Elspeth, Storm Slayer",
    "Elvish Mystic",
    "Elvish Spirit Guide",
    "Emry, Lurker of the Loch",
    "Endurance",
    "Enlightened Tutor",
    "Entomb",
    "Erode",
    "Esika's Chariot",
    "Exhume",
    "Exorcise",
    "Expedition Map",
    "Expressive Iteration",
    "Faerie Mastermind",
    "Faithless Looting",
    "Fatal Push",
    "Fiery Confluence",
    "Fiery Islet",
    "Figure of Destiny",
    "Figure of Fable",
    "Flame Slash",
    "Flametongue Kavu",
    "Flash",
    "Flashback",
    "Flickerwisp",
    "Flooded Strand",
    "Force of Negation",
    "Force of Vigor",
    "Force of Will",
    "Forest",
    "Formidable Speaker",
    "Frantic Search",
    "Fury",
    "Gaea's Cradle",
    "Galvanic Discharge",
    "Generous Ent",
    "Get Lost",
    "Ghost Vacuum",
    "Gitaxian Probe",
    "Giver of Runes",
    "Gleemox",
    "Glimmer Lens",
    "Goblin Bombardment",
    "Goblin Engineer",
    "Godless Shrine",
    "Golos, Tireless Pilgrim",
    "Grief",
    "Grim Monolith",
    "Grindstone",
    "Griselbrand",
    "Guerrilla Gorilla",
    "Guide of Souls",
    "Gut, True Soul Zealot",
    "Hallowed Fountain",
    "Harvester of Misery",
    "Haywire Mite",
    "Hedge Maze",
    "Hexdrinker",
    "Horizon Canopy",
    "Hymn to Tourach",
    "Icetill Explorer",
    "Ignoble Hierarch",
    "Imperial Seal",
    "Indatha Triome",
    "Inquisition of Kozilek",
    "Inspiring Vantage",
    "Iron-Shield Elf",
    "Island",
    "Ivora, Insatiable Heir",
    "Jace, Wielder of Mysteries",
    "Jace, the Mind Sculptor",
    "Jetmir's Garden",
    "Kaito, Bane of Nightmares",
    "Kaldra Compleat",
    "Karakas",
    "Karn, Scion of Urza",
    "Ketria Triome",
    "Knight of Autumn",
    "Kolaghan's Command",
    "Leatherhead, Swamp Stalker",
    "Legion Extruder",
    "Leyline Binding",
    "Library of Alexandria",
    "Lightning Bolt",
    "Lightning Greaves",
    "Lingering Souls",
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
    "Mana Crypt",
    "Mana Drain",
    "Mana Leak",
    "Mana Tithe",
    "Manamorphose",
    "Manifold Key",
    "Marsh Flats",
    "Memory Lapse",
    "Meticulous Archive",
    "Mind Stone",
    "Mind Twist",
    "Mine Collapse",
    "Mishra's Bauble",
    "Misty Rainforest",
    "Mole Man, Moloid Master",
    "Mother of Runes",
    "Mountain",
    "Mox Amber",
    "Mox Diamond",
    "Mox Emerald",
    "Mox Jet",
    "Mox Opal",
    "Mox Pearl",
    "Mox Ruby",
    "Mox Sapphire",
    "Mystic Confluence",
    "Mystical Tutor",
    "Narset, Parter of Veils",
    "Natural Order",
    "Nettlecyst",
    "Nissa, Who Shakes the World",
    "No More Lies",
    "Noble Hierarch",
    "Nurturing Peatland",
    "Oath of Druids",
    "Oko, Thief of Crowns",
    "Okoye, Mighty and Adored",
    "Oliphaunt",
    "Once Upon a Time",
    "Orcish Bowmasters",
    "Ouroboroid",
    "Oust",
    "Overgrown Tomb",
    "Palace Jailer",
    "Path to Exile",
    "Phelia, Exuberant Shepherd",
    "Phlage, Titan of Fire's Fury",
    "Plains",
    "Plateau",
    "Polluted Delta",
    "Ponder",
    "Portable Hole",
    "Preordain",
    "Prismatic Vista",
    "Psychic Frog",
    "Pyrite Spellbomb",
    "Pyroblast",
    "Pyrogoyf",
    "Pyrokinesis",
    "Questing Beast",
    "Raffine's Tower",
    "Ragavan, Nimble Pilferer",
    "Raph & Mikey, Troublemakers",
    "Raucous Theater",
    "Raugrin Triome",
    "Ravenous Robots",
    "Razorverge Thicket",
    "Reanimate",
    "Reckoner Bankbuster",
    "Relic of Sauron",
    "Remand",
    "Reprieve",
    "Restless Vinestalk",
    "Retrofitter Foundry",
    "Robber of the Rich",
    "Sacred Foundry",
    "Sanguine Evangelist",
    "Savai Triome",
    "Savannah",
    "Scalding Tarn",
    "Screaming Nemesis",
    "Scrubland",
    "Scythecat Cub",
    "Seachrome Coast",
    "Sear",
    "Sensei's Divining Top",
    "Sentinel of the Nameless City",
    "Sewer-veillance Cam",
    "Shadowy Backstreet",
    "Shallow Grave",
    "Sheoldred's Edict",
    "Sheoldred, the Apocalypse",
    "Simian Spirit Guide",
    "Skateboard",
    "Skullclamp",
    "Skyclave Apparition",
    "Smuggler's Copter",
    "Snapcaster Mage",
    "Sneak Attack",
    "Snuff Out",
    "Sol Ring",
    "Solitude",
    "Soul-Guide Lantern",
    "Spara's Headquarters",
    "Spectacular Spider-Man",
    "Spell Pierce",
    "Spellseeker",
    "Spider-Woman, Stunning Savior",
    "Spirebluff Canal",
    "Staff of the Storyteller",
    "Starting Town",
    "Steam Vents",
    "Stern Scolding",
    "Stock Up",
    "Stomping Ground",
    "Stoneforge Mystic",
    "Subtlety",
    "Sunbaked Canyon",
    "Super Shredder",
    "Suplex",
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
    "Tamiyo, Collector of Tales",
    "Tear Asunder",
    "Teferi, Hero of Dominaria",
    "Teferi, Time Raveler",
    "Temple Garden",
    "Territorial Kavu",
    "Tezzeret, Cruel Captain",
    "Thalia, Guardian of Thraben",
    "Thassa's Oracle",
    "The Fantasticar",
    "The Wandering Emperor",
    "Thopter Foundry",
    "Thornspire Verge",
    "Thoughtseize",
    "Thraben Inspector",
    "Thundering Falls",
    "Tidehollow Sculler",
    "Time Spiral",
    "Time Walk",
    "Time Warp",
    "Timetwister",
    "Tinker",
    "Titania, Protector of Argoth",
    "Tolarian Academy",
    "Torsten, Founder of Benalia",
    "Touch the Spirit Realm",
    "Traumatic Critique",
    "Treasure Cruise",
    "Trinket Mage",
    "Troll of Khazad-dûm",
    "Tropical Island",
    "True-Name Nemesis",
    "Tundra",
    "Ugin, Eye of the Storms",
    "Undercity Sewers",
    "Underground Mortuary",
    "Underground Sea",
    "Underworld Breach",
    "Unearth",
    "Unexpectedly Absent",
    "Unholy Heat",
    "Upheaval",
    "Uro, Titan of Nature's Wrath",
    "Urza's Bauble",
    "Vampiric Tutor",
    "Vaultborn Tyrant",
    "Verdant Catacombs",
    "Vindicate",
    "Volcanic Island",
    "Voltaic Key",
    "Walking Ballista",
    "Wasteland",
    "Waterlogged Grove",
    "Watery Grave",
    "Wheel of Fortune",
    "Windswept Heath",
    "Wishclaw Talisman",
    "Witherbloom Apprentice",
    "Wolverine, Fierce Fighter",
    "Wooded Foothills",
    "Woodfall Primus",
    "Worldly Tutor",
    "Wrath of God",
    "Wrenn and Six",
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
    let err = db.expect_id("Balance").unwrap_err();
    assert!(err.contains("Effect:BalanceEffect"), "{err}");
    let err = db.expect_id("Saheeli, Sublime Artificer").unwrap_err();
    assert!(err.contains("Effect:TargetBecomesCopyOfAnotherTargetUEOT"), "{err}");
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
