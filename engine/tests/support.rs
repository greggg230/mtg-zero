//! The support report over the whole PowerLSV cube (ENGINE.md 3): which
//! cards the engine plays, and what blocks the others.
//!
//! `cargo test -p engine --test support -- --nocapture` prints the report.

mod common;

use engine::db::CardDb;
use engine::lower::support::{threat_kinds, SupportReport, Threat};

/// The supported set (M4b-2). Changing it is deliberate: update this list.
const SUPPORTED: &[&str] = &[
    "\"Name Sticker\" Goblin",
    "Aang, Swift Savior // Aang and La, Ocean's Fury",
    "Abandoned Air Temple",
    "Abhorrent Oculus",
    "Abrade",
    "Abrupt Decay",
    "Adeline, Resplendent Cathar",
    "Aether Spellbomb",
    "Agent Bishop, Man in Black",
    "Ajani, Nacatl Pariah // Ajani, Nacatl Avenger",
    "Alpha Deathclaw",
    "Ancestral Recall",
    "Ancient Tomb",
    "Animate Dead",
    "Anointed Peacekeeper",
    "Aragorn, King of Gondor",
    "Archon of Cruelty",
    "Archon of Emeria",
    "Arena of Glory",
    "Arid Mesa",
    "Arwen, Mortal Queen",
    "Atraxa, Grand Unifier",
    "Auriok Salvagers",
    "Avacyn's Pilgrim",
    "Badgermole Cub",
    "Badlands",
    "Balance",
    "Baleful Mastery",
    "Baleful Strix",
    "Baloth Prime",
    "Barrowgoyf",
    "Basalt Monolith",
    "Batroc the Leaper",
    "Bayou",
    "Bejeweled Warg",
    "Bilbo, Thief in the Night",
    "Birds of Paradise",
    "Bitter Triumph",
    "Black Lotus",
    "Black Widow, Super Spy",
    "Blackcleave Cliffs",
    "Blazemire Verge",
    "Bleachbone Verge",
    "Blightsteel Colossus",
    "Blood Crypt",
    "Bloodbraid Challenger",
    "Bloodstained Mire",
    "Bloodtithe Harvester",
    "Blooming Marsh",
    "Bolas's Citadel",
    "Bone Shards",
    "Boomerang Basics",
    "Boseiju, Who Endures",
    "Botanical Sanctum",
    "Brain Freeze",
    "Brainstorm",
    "Brazen Borrower // Petty Theft",
    "Breeding Pool",
    "Bristly Bill, Spine Sower",
    "Broadside Bombardiers",
    "Burst Lightning",
    "Cabal Ritual",
    "Candelabra of Tawnos",
    "Carnage Interpreter",
    "Cathar Commando",
    "Caustic Bronco",
    "Caves of Chaos Adventurer",
    "Cecil, Dark Knight // Cecil, Redeemed Paladin",
    "Celestial Colonnade",
    "Chain Lightning",
    "Chain of Smog",
    "Chandra, Torch of Defiance",
    "Chaos Defiler",
    "Chromatic Star",
    "Chrome Mox",
    "City of Traitors",
    "Clarion Conqueror",
    "Cloak and Dagger, Entwined",
    "Coalition Relic",
    "Cogwork Librarian",
    "Collective Brutality",
    "Comet, Stellar Pup",
    "Commercial District",
    "Concealed Courtyard",
    "Concealing Curtains // Revealing Eye",
    "Consider",
    "Consult the Star Charts",
    "Containment Priest",
    "Copperline Gorge",
    "Cori-Steel Cutter",
    "Corpse Dance",
    "Council's Judgment",
    "Counterspell",
    "Coveted Jewel",
    "Crabomination",
    "Creeping Tar Pit",
    "Crop Rotation",
    "Crucible of Worlds",
    "Cryogen Relic",
    "Currency Converter",
    "Cut Down",
    "Dack Fayden",
    "Damn",
    "Dark Confidant",
    "Dark Ritual",
    "Darkslick Shores",
    "Dauthi Voidwalker",
    "Daze",
    "Death-Greeter's Champion",
    "Deep-Cavern Bat",
    "Delighted Halfling",
    "Demonic Consultation",
    "Demonic Tutor",
    "Dig Through Time",
    "Dismember",
    "Displacer Kitten",
    "Does Machines",
    "Dragon's Rage Channeler",
    "Duress",
    "Eagles of the North",
    "Echo of Eons",
    "Elegant Parlor",
    "Elite Spellbinder",
    "Elspeth, Storm Slayer",
    "Elvish Mystic",
    "Elvish Spirit Guide",
    "Embereth Shieldbreaker // Battle Display",
    "Emeritus of Abundance // Regrowth",
    "Emperor of Bones",
    "Emrakul, the Aeons Torn",
    "Emry, Lurker of the Loch",
    "Endurance",
    "Enduring Innocence",
    "Enlightened Tutor",
    "Entomb",
    "Erode",
    "Ertai Resurrected",
    "Esika's Chariot",
    "Etali, Primal Conqueror // Etali, Primal Sickness",
    "Exhume",
    "Exorcise",
    "Expedition Map",
    "Expressive Iteration",
    "Fable of the Mirror-Breaker // Reflection of Kiki-Jiki",
    "Faerie Mastermind",
    "Faithless Looting",
    "Fallen Shinobi",
    "Fanatic of Rhonas",
    "Fastbond",
    "Fatal Push",
    "Fear of Missing Out",
    "Feywild Caretaker",
    "Fiery Confluence",
    "Fiery Islet",
    "Figure of Destiny",
    "Figure of Fable",
    "Fire Covenant",
    "Flame Slash",
    "Flametongue Kavu",
    "Flash",
    "Flashback",
    "Flickerwisp",
    "Flooded Strand",
    "Force of Negation",
    "Force of Vigor",
    "Force of Will",
    "Forensic Gadgeteer",
    "Forest",
    "Formidable Speaker",
    "Forth Eorlingas!",
    "Fractured Identity",
    "Frantic Search",
    "From the Catacombs",
    "Fury",
    "Gaea's Cradle",
    "Galvanic Discharge",
    "Gemstone Caverns",
    "Gemstone Mine",
    "Generous Ent",
    "Generous Plunderer",
    "Get Lost",
    "Ghost Vacuum",
    "Gitaxian Probe",
    "Giver of Runes",
    "Gleemox",
    "Glimmer Lens",
    "Goblin Bombardment",
    "Goblin Engineer",
    "Goblin Welder",
    "Godless Shrine",
    "Gollum, Riddle Master",
    "Golos, Tireless Pilgrim",
    "Goryo's Vengeance",
    "Green Sun's Zenith",
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
    "Headliner Scarlett",
    "Hedge Maze",
    "Hexdrinker",
    "Hexing Squelcher",
    "Horizon Canopy",
    "Hullbreacher",
    "Hymn to Tourach",
    "Icetill Explorer",
    "Ignoble Hierarch",
    "Imperial Seal",
    "Indatha Triome",
    "Inquisition of Kozilek",
    "Inspiring Vantage",
    "Inti, Seneschal of the Sun",
    "Invert Polarity",
    "Iron-Shield Elf",
    "Island",
    "Ivora, Insatiable Heir",
    "Jace, Vryn's Prodigy // Jace, Telepath Unbound",
    "Jace, Wielder of Mysteries",
    "Jace, the Mind Sculptor",
    "Jacked Rabbit",
    "Jennifer Walters // The Sensational She-Hulk",
    "Jetmir's Garden",
    "Kaito, Bane of Nightmares",
    "Kaldra Compleat",
    "Kappa Cannoneer",
    "Karakas",
    "Karn, Scion of Urza",
    "Keen-Eyed Curator",
    "Ketria Triome",
    "Knight of Autumn",
    "Kolaghan's Command",
    "Laelia, the Blade Reforged",
    "Leatherhead, Swamp Stalker",
    "Legion Extruder",
    "Leovold, Emissary of Trest",
    "Leyline Binding",
    "Library of Alexandria",
    "Life // Death",
    "Lightning Bolt",
    "Lightning Greaves",
    "Lim-Dûl's Vault",
    "Lingering Souls",
    "Lion Sash",
    "Lion's Eye Diamond",
    "Llanowar Elves",
    "Loot, the Pathfinder",
    "Loran of the Third Path",
    "Lose Focus",
    "Lotus Cobra",
    "Lotus Petal",
    "Luminarch Aspirant",
    "Lurrus of the Dream-Den",
    "Lush Portico",
    "Lutri, the Spellchaser",
    "Lórien Revealed",
    "Magda, Brazen Outlaw",
    "Malcolm, Alluring Scoundrel",
    "Malevolent Rumble",
    "Mana Confluence",
    "Mana Crypt",
    "Mana Drain",
    "Mana Leak",
    "Mana Tithe",
    "Mana Vault",
    "Manamorphose",
    "Manifold Key",
    "Marsh Flats",
    "Mawloc",
    "Memory Jar",
    "Memory Lapse",
    "Metamorphosis Fanatic",
    "Meticulous Archive",
    "Mind Stone",
    "Mind Twist",
    "Mine Collapse",
    "Minsc & Boo, Timeless Heroes",
    "Mishra's Bauble",
    "Mishra's Workshop",
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
    "Multiversal Passage",
    "Mystic Confluence",
    "Mystical Tutor",
    "Nadu, Winged Wisdom",
    "Narset, Parter of Veils",
    "Natural Order",
    "Necromancy",
    "Nettlecyst",
    "Nissa, Who Shakes the World",
    "No More Lies",
    "Noble Hierarch",
    "Norman Osborn // Green Goblin",
    "Nurturing Peatland",
    "Oath of Druids",
    "Occult Epiphany",
    "Ocelot Pride",
    "Oko, Thief of Crowns",
    "Okoye, Mighty and Adored",
    "Oliphaunt",
    "Once Upon a Time",
    "Orcish Bowmasters",
    "Orcish Lumberjack",
    "Otawara, Soaring City",
    "Otharri, Suns' Glory",
    "Ouroboroid",
    "Oust",
    "Outland Liberator // Frenzied Trapbreaker",
    "Overgrown Tomb",
    "Painter's Servant",
    "Palace Jailer",
    "Palantír of Orthanc",
    "Parallax Wave",
    "Passageway Seer",
    "Path to Exile",
    "Pentad Prism",
    "Pest Infestation",
    "Phelia, Exuberant Shepherd",
    "Phlage, Titan of Fire's Fury",
    "Phyrexian Metamorph",
    "Pillage the Bog",
    "Pinnacle Emissary",
    "Plains",
    "Plateau",
    "Polluted Delta",
    "Ponder",
    "Portable Hole",
    "Portal to Phyrexia",
    "Preordain",
    "Prismatic Ending",
    "Prismatic Vista",
    "Psychic Frog",
    "Pyrite Spellbomb",
    "Pyroblast",
    "Pyrogoyf",
    "Pyrokinesis",
    "Quantum Riddler",
    "Questing Beast",
    "Raffine's Tower",
    "Raffine, Scheming Seer",
    "Ragavan, Nimble Pilferer",
    "Rain of Filth",
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
    "Sage of the Skies",
    "Saheeli, Sublime Artificer",
    "Sanguine Evangelist",
    "Savai Triome",
    "Savannah",
    "Scalding Tarn",
    "Screaming Nemesis",
    "Scrubland",
    "Scythecat Cub",
    "Seachrome Coast",
    "Sear",
    "Seasoned Dungeoneer",
    "Seasoned Pyromancer",
    "Sedgemoor Witch",
    "Sensei's Divining Top",
    "Sentinel of the Nameless City",
    "Serra Paragon",
    "Sewer-veillance Cam",
    "Shadowy Backstreet",
    "Shallow Grave",
    "Shatterskull Smashing // Shatterskull, the Hammer Pass",
    "Shelldock Isle",
    "Sheoldred's Edict",
    "Sheoldred, the Apocalypse",
    "Shifting Woodland",
    "Sicarian Infiltrator",
    "Simian Spirit Guide",
    "Sink into Stupor // Soporific Springs",
    "Six",
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
    "Sowing Mycospawn",
    "Spara's Headquarters",
    "Spectacular Spider-Man",
    "Spell Pierce",
    "Spellseeker",
    "Spider-Woman, Stunning Savior",
    "Spirebluff Canal",
    "Springheart Nantuko",
    "Squandered Resources",
    "Staff of the Storyteller",
    "Starting Town",
    "Steam Vents",
    "Stern Scolding",
    "Stock Up",
    "Stomping Ground",
    "Stoneforge Mystic",
    "Strip Mine",
    "Subtlety",
    "Sunbaked Canyon",
    "Sunfall",
    "Sunken Ruins",
    "Sunset Saboteur",
    "Super Shredder",
    "Superior Spider-Man",
    "Suplex",
    "Surrak, Elusive Hunter",
    "Swamp",
    "Sword of the Meek",
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
    "Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar",
    "Tear Asunder",
    "Teferi, Hero of Dominaria",
    "Teferi, Time Raveler",
    "Temple Garden",
    "Territorial Kavu",
    "Tersa Lightshatter",
    "Tezzeret, Cruel Captain",
    "Thalia, Guardian of Thraben",
    "Thassa's Oracle",
    "The Fantasticar",
    "The Mind Stone",
    "The One Ring",
    "The Wandering Emperor",
    "The Wondrous Wasp",
    "Thieving Skydiver",
    "Thopter Foundry",
    "Thor, God of Thunder",
    "Thornspire Verge",
    "Thoughtseize",
    "Thraben Inspector",
    "Three Steps Ahead",
    "Through the Breach",
    "Thundering Falls",
    "Thundertrap Trainer",
    "Tidehollow Sculler",
    "Time Spiral",
    "Time Vault",
    "Time Walk",
    "Time Warp",
    "Timetwister",
    "Tinker",
    "Tishana's Tidebinder",
    "Titania, Protector of Argoth",
    "Tolarian Academy",
    "Torsten, Founder of Benalia",
    "Touch the Spirit Realm",
    "Toxic Deluge",
    "Transmute Artifact",
    "Traumatic Critique",
    "Treasure Cruise",
    "Trinket Mage",
    "Troll of Khazad-dûm",
    "Tropical Island",
    "True-Name Nemesis",
    "Tundra",
    "Ugin, Eye of the Storms",
    "Ulvenwald Oddity // Ulvenwald Behemoth",
    "Undercity Sewers",
    "Underground Mortuary",
    "Underground Sea",
    "Undermountain Adventurer",
    "Underworld Breach",
    "Unearth",
    "Unexpectedly Absent",
    "Unholy Heat",
    "Upheaval",
    "Uro, Titan of Nature's Wrath",
    "Ursine Monstrosity",
    "Urza's Bauble",
    "Urza's Saga",
    "Urza, Lord High Artificer",
    "Vampiric Tutor",
    "Vaultborn Tyrant",
    "Verdant Catacombs",
    "Vicious Rivalry",
    "Vindicate",
    "Voice of Victory",
    "Volcanic Island",
    "Voltaic Key",
    "Walking Ballista",
    "Wan Shi Tong, All-Knowing",
    "Wasteland",
    "Waterlogged Grove",
    "Watery Grave",
    "We Say Thee Nay!",
    "Wheel of Fortune",
    "White Plume Adventurer",
    "Winds of Abandon",
    "Windswept Heath",
    "Wishclaw Talisman",
    "Witch Enchanter // Witch-Blessed Meadow",
    "Witherbloom Apprentice",
    "Wizard's Rockets",
    "Wolverine, Fierce Fighter",
    "Wooded Foothills",
    "Woodfall Primus",
    "Worldly Tutor",
    "Worldspine Wurm",
    "Wrath of God",
    "Wrenn and Six",
    "Xander's Lounge",
    "Yawgmoth's Will",
    "Zagoth Triome",
    "Ziatora's Proving Ground",
    "Zirda, the Dawnwaker",
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
    if let Ok(path) = std::env::var("SUPPORT_DEFS") {
        // Every definition (cards, tokens, faces), lowered, to diff the
        // lowering of two IRs: the interned subtypes (`@subtype id name`),
        // then `#id kind name` and its `CardDef` (Debug). Token, face and
        // subtype ids differ when another card's support changes;
        // `tools/diff_defs.py` compares them by name.
        let mut s = String::new();
        for (k, name) in db.subtype_names().iter().enumerate() {
            s.push_str(&format!("@subtype {k} {name}\n"));
        }
        for i in 0..db.len() {
            let d = db.def(i as engine::ids::DefId);
            let kind = if i < db.num_cards() { "card" } else if db.is_face(i as engine::ids::DefId) { "face" } else { "token" };
            s.push_str(&format!("#{i} {kind} {}\n{d:#?}\n", d.name));
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

/// The Edge of Eternities sealed card table's supported set (ENGINE.md 99),
/// one list per workstream so parallel branches never edit the same lines:
/// `SUPPORTED_EOE_BASE` = the cards supported when the set was imported
/// (lane B audits them against the Oracle text and may remove some), then
/// one list per lane of the port (ENGINE.md 99.2-99.5), each sorted. The
/// test compares their sorted union with the report. Changing them is
/// deliberate (`SUPPORT_LIST=path` with `eoe_support_report` writes the
/// whole supported set).
const SUPPORTED_EOE_BASE: &[&str] = &[
    "Ancient Tomb",
    "Annul",
    "Auxiliary Boosters",
    "Banishing Light",
    "Blooming Stinger",
    "Bombard",
    "Breeding Pool",
    "Burgeoning",
    "Bygone Colossus",
    "Celestial Colonnade",
    "Cloudsculpt Technician",
    "Consult the Star Charts",
    "Creeping Tar Pit",
    "Cryogen Relic",
    "Dual-Sun Adepts",
    "Dubious Delicacy",
    "Dust Bowl",
    "Eumidian Terrabotanist",
    "Eusocial Engineering",
    "Exalted Sunborn",
    "Forest",
    "Gemstone Caverns",
    "Germinating Wurm",
    "Godless Shrine",
    "Gravblade Heavy",
    "Gravpack Monoist",
    "Green Sun's Zenith",
    "Hissing Quagmire",
    "Honor",
    "Hullcarver",
    "Icecave Crasher",
    "Icetill Explorer",
    "Illvoi Galeblade",
    "Illvoi Operative",
    "Intrepid Tenderfoot",
    "Island",
    "Kavaron Turbodrone",
    "Lightless Evangel",
    "Lumbering Falls",
    "Mana Confluence",
    "Mechan Assembler",
    "Mechan Navigator",
    "Melded Moxite",
    "Mm'menon, Uthros Exile",
    "Molecular Modifier",
    "Monoist Sentry",
    "Mountain",
    "Nanoform Sentinel",
    "Nebula Dragon",
    "Needle Spires",
    "Nutrient Block",
    "Oreplate Pangolin",
    "Ouroboroid",
    "Pinnacle Emissary",
    "Plains",
    "Quantum Riddler",
    "Red Tiger Mechan",
    "Remnant Elemental",
    "Rig for War",
    "Sacred Foundry",
    "Seam Rip",
    "Shambling Vent",
    "Sinister Cryologist",
    "Slagdrill Scrapper",
    "Starbreach Whale",
    "Starfighter Pilot",
    "Station Monitor",
    "Stirring Wildwood",
    "Stomping Ground",
    "Strip Mine",
    "Sunset Saboteur",
    "Sunstar Lightsmith",
    "Susurian Voidborn",
    "Swamp",
    "Syr Vondam, the Lucent",
    "Tezzeret, Cruel Captain",
    "Thawbringer",
    "Thrumming Hivepool",
    "Umbral Collar Zealot",
    "Watery Grave",
    "Weapons Manufacturing",
    "Weftblade Enhancer",
    "Weftstalker Ardent",
    "Zookeeper Mechan",
];
/// Lane A: Station, spacecraft, counters, artifacts, Equipment and Auras.
const SUPPORTED_EOE_A: &[&str] = &[
    "Atmospheric Greenhouse",
    "Beyond the Quiet",
    "Cerebral Download",
    "Cryoshatter",
    "Debris Field Crusher",
    "Drill Too Deep",
    "Drix Fatemaker",
    "Dual-Sun Technique",
    "Emissary Escort",
    "Extinguisher Battleship",
    "Galvanizing Sawship",
    "Illvoi Light Jammer",
    "Invasive Maneuvers",
    "Lost in Space",
    "Lumen-Class Frigate",
    "Meltstrider Eulogist",
    "Meltstrider's Gear",
    "Pulsar Squadron Ace",
    "Radiant Strike",
    "Rescue Skiff",
    "Ruinous Rampage",
    "Scour for Scrap",
    "Selfcraft Mechan",
    "Shattered Wings",
    "Squire's Lightblade",
    "Susurian Dirgecraft",
    "Synthesizer Labship",
    "Uthros Scanship",
    "Virulent Silencer",
    "Warmaker Gunship",
    "Wedgelight Rammer",
    "Wurmwall Sweeper",
];
/// Lane B: lands, mana abilities, Lander tokens (and the base list's audit).
const SUPPORTED_EOE_B: &[&str] = &[
    "Emergency Eject",
    "Fungal Colossus",
    "Grove of the Burnwillows",
    "Harmonious Grovestrider",
    "Kav Landseeker",
    "Lavaclaw Reaches",
    "Lotus Field",
    "Meteor Crater",
    "Orbital Plunge",
    "Petrified Field",
    "Reflecting Pool",
    "Tannuk, Memorial Ensign",
    "Territorial Bruntar",
    "Wandering Fumarole",
];
/// Lane C: void, warp, spells cast this turn, cost reductions, graveyard,
/// exile, library and hand effects.
const SUPPORTED_EOE_C: &[&str] = &[
    "All-Fates Scroll",
    "Brightspear Zealot",
    "Chrome Companion",
    "Codecracker Hound",
    "Cosmogoyf",
    "Dark Endurance",
    "Deafening Silence",
    "Decode Transmissions",
    "Gigastorm Titan",
    "Hymn of the Faller",
    "Illvoi Infiltrator",
    "Insatiable Skittermaw",
    "Interceptor Mechan",
    "Kavaron Skywarden",
    "Lashwhip Predator",
    "Lightstall Inquisitor",
    "Memorial Vault",
    "Plasma Bolt",
    "Sami, Wildcat Captain",
    "Scavenger Grounds",
    "Seedship Broodtender",
    "Space-Time Anomaly",
    "Starwinder",
    "Temporal Intervention",
    "Timeline Culler",
    "Tragic Trajectory",
    "Uthros Psionicist",
    "Virus Beetle",
    "Voidforged Titan",
    "Warping Wail",
    "Weftwalking",
];
/// Lane D: creatures, combat, removal, damage, life, tokens and pumps.
const SUPPORTED_EOE_D: &[&str] = &[
    "Anticausal Vestige",
    "Biosynthic Burst",
    "Comet Crawler",
    "Command Bridge",
    "Cut Propulsion",
    "Darkness",
    "Dawnstrike Vanguard",
    "Depressurize",
    "Devastating Onslaught",
    "Diplomatic Relations",
    "Exosuit Savior",
    "Faller's Faithful",
    "Flight-Deck Coordinator",
    "Focus Fire",
    "Frontline War-Rager",
    "Genemorph Imago",
    "Haliya, Guided by Light",
    "Hemosymbic Mite",
    "Kavaron Harrier",
    "Mechanozoa",
    "Memorial Team Leader",
    "Mightform Harmonizer",
    "Monoist Circuit-Feeder",
    "Mouth of the Storm",
    "Mutinous Massacre",
    "Nova Hellkite",
    "Perigee Beckoner",
    "Sami, Ship's Engineer",
    "Singularity Rupture",
    "Skystinger",
    "Swarm Culler",
    "Systems Override",
    "The Endstone",
    "Vaultguard Trooper",
    "Zealous Display",
    "Zero Point Ballad",
];

/// Cards that need features of two or more lanes, finished after the
/// lanes merged (ENGINE.md 99.6, 99.8; tests in `tests/cards_eoe_cross*.rs`).
const SUPPORTED_EOE_CROSS: &[&str] = &[
    "Alpharael, Dreaming Acolyte",
    "Atomic Microsizer",
    "Broodguard Elite",
    "Embrace Oblivion",
    "Full Bore",
    "Hardlight Containment",
    "Hylderblade",
    "Infinite Guideline Station",
    "Larval Scoutlander",
    "Mental Modulation",
    "Pinnacle Kill-Ship",
    "Reroute Systems",
    "Roving Actuator",
    "Seedship Impact",
    "Sledge-Class Seedship",
    "Sliver Overlord",
    "Specimen Freighter",
    "Starport Security",
    "Survey Mechan",
    "Tapestry Warden",
    "Terrapact Intimidator",
    "Thaumaton Torpedo",
    "The Seriema",
    "Vote Out",
];

/// Cards the mtg-bot IR-fix batch unblocks (ENGINE.md 99.9; their card
/// tests in `tests/cards_eoe_irfix*.rs`).
const SUPPORTED_EOE_IRFIX: &[&str] = &[
    "All-Fates Stalker",
    "Archenemy's Charm",
    "Beamsaw Prospector",
    "Biomechan Engineer",
    "Biotech Specialist",
    "Blinkmoth Nexus",
    "Bonders' Enclave",
    "Cathedral of War",
    "Cosmogrand Zenith",
    "Dauntless Scrapbot",
    "Dawnsire, Sunstar Dreadnought",
    "Desculpting Blast",
    "Deserted Temple",
    "Divert Disaster",
    "Dockworker Drone",
    "Edge Rover",
    "Elegy Acolyte",
    "Endless Sands",
    "Entropic Battlecruiser",
    "Evendo, Waking Haven",
    "Fell Gravship",
    "Galactic Wayfarer",
    "Glacier Godmaw",
    "Gravkill",
    "Haliya, Ascendant Cadet",
    "High Market",
    "Honored Knight-Captain",
    "Inkmoth Nexus",
    "Inventors' Fair",
    "Kavaron, Memorial World",
    "Knight Luminary",
    "Lithobraking",
    "Luxknight Breacher",
    "Mechan Shieldmate",
    "Meltstrider's Resolve",
    "Mirrorpool",
    "Mutavault",
    "Mystifying Maze",
    "Nexus of Fate",
    "Pinnacle Starcage",
    "Pull Through the Weft",
    "Rayblade Trooper",
    "Sami's Curiosity",
    "Scrounge for Eternity",
    "Seedship Agrarian",
    "Sothera, the Supervoid",
    "Starfield Shepherd",
    "Sunstar Expansionist",
    "Syr Vondam, Sunstar Exemplar",
    "Terrain Generator",
    "Uthros, Titanic Godcore",
    "Xu-Ifit, Osteoharmonist",
];

/// The last four workstreams (ENGINE.md 99.11-99.14): W1 adds to
/// `SUPPORTED_EOE_IRFIX`; W2, W3, W4 to their own lists.
const SUPPORTED_EOE_MANA: &[&str] = &[
    "Bioengineered Future",
    "Blast Zone",
    "Cascading Cataracts",
    "Crystal Quarry",
    "Echoing Deeps",
    "Eldrazi Temple",
    "Famished Worldsire",
    "Gene Pollinator",
    "Magus of the Moon",
    "Steelswarm Operator",
    "The Eternity Elevator",
];
const SUPPORTED_EOE_COMBAT: &[&str] = &[
    "Astelli Reclaimer",
    "Dyadrine, Synthesis Amalgam",
    "Frenzied Baloth",
    "Pain for All",
    "Possibility Technician",
    "Raging Ravine",
    "Ragost, Deft Gastronaut",
    "Requiem Monolith",
    "Robe of Stars",
    "Rust Harvester",
    "Secluded Starforge",
    "Sunstar Chaplain",
    "Swarmyard",
    "Terminal Velocity",
];
const SUPPORTED_EOE_EXILE: &[&str] = &[
    "Adagia, Windswept Bastion",
    "Alpharael, Stonechosen",
    "Blade of the Swarm",
    "Chorale of the Void",
    "Close Encounter",
    "Scout for Survivors",
    "Susur Secundi, Void Altar",
    // (ENGINE.md 100.5: a controlled turn's decisions encoded in the
    // controlled seat; blocked on that since ENGINE.md 99.14.)
    "The Dominion Bracelet",
];
/// W5 (ENGINE.md 99.15): alternative costs other permanents grant.
const SUPPORTED_EOE_W5: &[&str] = &[
    "Tannuk, Steadfast Second",
];

/// EoE W6, the remaining cards (ENGINE.md 99.16).
const SUPPORTED_EOE_W6: &[&str] = &[
    "Contested War Zone",
    "Loading Zone",
    "Mm'menon, the Right Hand",
    "Moonlit Meditation",
    "Nesting Grounds",
    "Plaza of Heroes",
    "Power Depot",
    "Starfield Vocalist",
    "Sunken Citadel",
    "Terrasymbiosis",
    "Thespian's Stage",
    "Tractor Beam",
    "Unravel",
];

fn supported_eoe() -> Vec<String> {
    let lists = [SUPPORTED_EOE_BASE, SUPPORTED_EOE_A, SUPPORTED_EOE_B, SUPPORTED_EOE_C, SUPPORTED_EOE_D, SUPPORTED_EOE_CROSS, SUPPORTED_EOE_IRFIX, SUPPORTED_EOE_MANA, SUPPORTED_EOE_COMBAT, SUPPORTED_EOE_EXILE, SUPPORTED_EOE_W5, SUPPORTED_EOE_W6];
    let mut v: Vec<String> = lists.concat().into_iter().map(String::from).collect();
    let n = v.len();
    v.sort();
    v.dedup();
    assert_eq!(v.len(), n, "a card is in two SUPPORTED_EOE lists");
    v
}

/// The support report over the Edge of Eternities sealed card table
/// (ENGINE.md 99): the set's 315 cards with IR (all but Paradox Haze, which
/// mtg-bot's parser leaves out) and the five basic lands.
/// `cargo test -p engine --test support eoe -- --nocapture` prints it;
/// `SUPPORT_DUMP` and `SUPPORT_LIST` as for the cube.
#[test]
fn eoe_support_report() {
    let db = CardDb::eoe();
    let r = SupportReport::from_db(db);
    if let Ok(path) = std::env::var("SUPPORT_LIST") {
        std::fs::write(&path, r.supported.join("
")).unwrap();
    }
    if let Ok(path) = std::env::var("SUPPORT_DUMP") {
        let mut s = String::new();
        for (name, b) in &r.blocked {
            s.push_str(&format!("{name}	{}
", b.join(" | ")));
        }
        std::fs::write(&path, s).unwrap();
    }
    println!("{}", r.render(60));
    println!("greedy staging (feature, cards it completes, cumulative):");
    for (f, n, total) in r.greedy(60) {
        println!("  {f:<60} {n:>4} {total:>5}");
    }
    assert_eq!(r.total(), 320);
    assert_eq!(r.supported, supported_eoe());
    assert!(r.blocked.iter().all(|(_, b)| !b.is_empty()));
}

/// The legacy cards of constructed play (`data/cards/legacy.ir.json`: the
/// Delver mirror's two, ENGINE.md 54; the Legacy Doomsday deck's ten, 124;
/// the Legacy Sneak and Show deck's six, 123; the Delver 75's five sideboard
/// cards, 122), sorted. Every one is supported (`CardDb::constructed` fails
/// loudly otherwise); changing the list is deliberate.
const SUPPORTED_LEGACY: &[&str] = &[
    "Brotherhood's End",
    "Cavern of Souls",
    "Consign to Memory",
    "Disruptor Flute",
    "Doomsday",
    "Edge of Autumn",
    "Flow State",
    "Flusterstorm",
    "Hydroblast",
    "Leyline of the Void",
    "Magus of the Moon",
    "Meltdown",
    "Misdirection",
    "Mistrise Village",
    "Murktide Regent",
    "Omniscience",
    "Pact of Negation",
    "Personal Tutor",
    "Price of Progress",
    "Prismari Charm",
    "Rough // Tumble",
    "Show and Tell",
    "Street Wraith",
];

/// The support report over the legacy cards alone (their own table): every
/// card of the legacy IR supported, none blocked.
/// `cargo test -p engine --test support legacy -- --nocapture` prints it.
#[test]
fn legacy_support_report() {
    let cards = engine::db::load_legacy_document().cards;
    let db = CardDb::from_cards(&cards);
    let r = SupportReport::from_db(&db);
    println!("{}", r.render(40));
    assert_eq!(r.total(), SUPPORTED_LEGACY.len());
    assert_eq!(r.supported, SUPPORTED_LEGACY);
    assert!(r.blocked.is_empty(), "{:?}", r.blocked);
    // And in constructed play's table, after the cube's 546.
    for n in SUPPORTED_LEGACY {
        assert!(CardDb::constructed().expect_id(n).is_ok(), "{n}");
    }
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
    // (Every card of the cube is supported since M7d's IR fixes.)
    assert!(sup_creatures > 0 && sup_threats > 0 && blk_threats == 0);
}

#[test]
fn unsupported_cards_fail_loudly() {
    // A land whose IR relies on the Python engine's "any land pays one
    // generic" fallback (no mana ability, no `mana_only_from_produces`: Strip
    // Mine's IR before mtg-bot 8ea9fcca) is not supported, and says why.
    let mut cards = engine::db::load_cube_document().cards;
    let mine = cards.iter_mut().find(|c| c.name == "Strip Mine").unwrap();
    let card_ir::cats::CardPrinting::CardPrinting(p) = &mut mine.printing;
    p.produces.clear();
    p.mana_only_from_produces = false;
    let db = CardDb::from_cards(&cards);
    let err = db.expect_id("Strip Mine").unwrap_err();
    assert!(err.contains("CardPrinting.mana_only_from_produces"), "{err}");
    assert!(CardDb::cube().expect_id("No Such Card").is_err());
}

/// The cube document with `edit` applied to card `name`'s JSON, lowered.
fn db_with_edit(name: &str, edit: impl Fn(&mut serde_json::Value)) -> CardDb {
    let mut doc: serde_json::Value = serde_json::from_str(engine::db::CUBE_IR).unwrap();
    let card = doc["cards"].as_array_mut().unwrap().iter_mut().find(|c| c["name"] == name).unwrap();
    edit(&mut card["printing"]);
    let cards: Vec<card_ir::Card> = serde_json::from_value(doc["cards"].clone()).unwrap();
    CardDb::from_cards(&cards)
}

#[test]
fn ir_values_that_disagree_with_the_oracle_shape_are_blockers() {
    // Robber of the Rich: "during any turn you attacked with a Rogue" with a
    // this-turn duration (the IR before mtg-bot 8ea9fcca said so; the
    // permission lasts while the card is exiled).
    let db = db_with_edit("Robber of the Rich", |p| {
        p["triggered_abilities"][0]["effect"]["duration"] = serde_json::json!("this_turn");
    });
    let err = db.expect_id("Robber of the Rich").unwrap_err();
    assert!(err.contains("ExileTopCardsWithPlayPermission.duration(this_turn with requires_attacked_with_subtype)"), "{err}");
    // Green Sun's Zenith: shuffling itself into its library is the last
    // instruction of a spell, nowhere else.
    let db = db_with_edit("Green Sun's Zenith", |p| {
        let e = p["spell_effect"]["effects"].as_array_mut().unwrap();
        e.reverse();
    });
    let err = db.expect_id("Green Sun's Zenith").unwrap_err();
    assert!(err.contains("ShuffleThisSpellIntoOwnersLibrary(not the last instruction"), "{err}");
    // Wizard's Rockets: "add X mana" without an {X} in the cost.
    let db = db_with_edit("Wizard's Rockets", |p| {
        p["activated_abilities"][0]["cost"]["mana"] = serde_json::json!({"$t": "ManaCost", "generic": 1});
        p["activated_abilities"][0]["cost"]["mana_str"] = serde_json::json!("{1}");
    });
    let err = db.expect_id("Wizard's Rockets").unwrap_err();
    assert!(err.contains("AddManaInAnyCombination.amount_is_x(no {X} in the cost)"), "{err}");
}

#[test]
fn costs_the_cast_cant_pay_together_are_blockers_cr_601_2b_601_2f() {
    // The total cost is the mana cost or an alternative cost plus every
    // additional cost (CR 601.2b, 601.2f). The engine pays an alternative
    // cost's rest alone and a printed escape's exile alone (ENGINE.md
    // 40.9): a card with such costs together is not supported, and says
    // why. Snuff Out with "as an additional cost, discard a card"; Three
    // Steps Ahead with an alternative cost of {1} (spree's mode costs are
    // additional costs, 702.172a); From the Catacombs (escape) with the
    // discard.
    let discard = serde_json::json!([{"$t": "AdditionalCostOption", "kind": "discard", "amount": 1}]);
    for (name, field, value, blocker) in [
        ("Snuff Out", "additional_cost_options", discard.clone(), "CardPrinting.alternative_costs(with additional costs)"),
        (
            "Three Steps Ahead",
            "alternative_costs",
            serde_json::json!([{"$t": "AlternativeCost", "mana": "{1}"}]),
            "CardPrinting.alternative_costs(with additional costs)",
        ),
        ("From the Catacombs", "additional_cost_options", discard.clone(), "CardPrinting.escape_cost(with additional costs)"),
    ] {
        assert!(CardDb::cube().expect_id(name).is_ok(), "{name} is supported as printed");
        let db = db_with_edit(name, |p| p[field] = value.clone());
        let err = db.expect_id(name).unwrap_err();
        assert!(err.contains(blocker), "{name}: {err}");
    }
}

#[test]
fn a_new_ir_field_value_is_a_blocker() {
    // A supported card with one unsupported flag set is no longer supported.
    let mut cards = engine::db::load_cube_document().cards;
    let bolt = cards.iter_mut().find(|c| c.name == "Lightning Bolt").unwrap();
    let card_ir::cats::CardPrinting::CardPrinting(p) = &mut bolt.printing;
    // (Convoke was this test's flag until the EoE cross-lane pass built it,
    // ENGINE.md 99.8.)
    p.nonbasic_lands_are_mountains = true;
    let db = CardDb::from_cards(&cards);
    let err = db.expect_id("Lightning Bolt").unwrap_err();
    assert!(err.contains("CardPrinting.nonbasic_lands_are_mountains"), "{err}");
}

#[test]
fn custom_test_cards_lower() {
    let db = common::db();
    for c in common::custom_cards() {
        db.expect_id(&c.name).unwrap();
    }
}
