//! Random-play fuzzer (ENGINE.md 15): complete games between random decks
//! of supported cards, checking after every action that nothing panics, the
//! structural invariants hold (every card in exactly one zone), legal
//! actions are sorted, unique and non-empty, and the game terminates; that
//! the same seed and actions replay bit for bit; that a clone continues
//! identically; that `determinize` keeps the decider's view; and that every
//! option of a sampled decision can be completed, in the game and in the
//! worlds `determinize` samples.
//!
//! `FUZZ_GAMES=n` sets the number of games (default 300).
//!
//! Matches (M8b, ENGINE.md 36.8): `fuzz_random_matches` plays best-of-three
//! matches through `MtgAz` with random actions (sideboarding too), checking
//! the legal actions, the game invariants, clones, `determinize` (the
//! decider's actions, view and encoding), the decks of every later game
//! (legal, from the pool), the chooser of every later game (CR 103.1) and
//! the match's end; `FUZZ_MATCHES=n` (default `FUZZ_GAMES` / 20, at least
//! 20). M8b part b (ENGINE.md 36.11): `determinize` for both viewers, in
//! games and while sideboarding, samples the opponent's hidden pool and deck
//! consistently with what the viewer knows (`check_hidden_pool`), keeps the
//! viewer's encoding, and two sampled worlds stay indistinguishable to the
//! decider while zero-core would keep them in one node.
//!
//! `FUZZ_POOL=interactions` (M8d part 2, ENGINE.md 38.5): combo-dense decks.
//! Each player's 23 spells come from two or three of the interaction groups
//! below (`GROUPS`: cards that interact under the CR, one group per family
//! of cells of `docs/interactions.md`: copy effects with enter and leave
//! abilities, flicker and linked exile, reanimation with replacement
//! effects, face-down cards, double-faced cards, counterspells and copies of
//! spells, control changes, taxes and free casts, damage and protection,
//! planeswalkers, last known information and state-based actions, combat
//! keywords, day and night), one in eight from all the groups; 17 random
//! lands; a sideboard of seven from the same groups. Games and matches
//! (best-of-three on those lists, open decklists) both use it, with every
//! check of the default mode. The decision kinds asked are counted and
//! printed (interaction decisions per 1,000, against the default mode's).
//! `FUZZ_POOL=drafted` (M6 part 3, ENGINE.md 45): the matches are drafted
//! ones: a `team_draft` event (4v4 or 2v2; random picks and decks, or team A
//! drafting and building as `consensus`) and one of its matches as the `mtg`
//! spec a co-training run plays (`draft_bots::match_spec`: the drafted
//! decks, the rest of the pools as sideboards, the real first picks, what
//! each player knows is elsewhere, the match seed), one match in eight with
//! 12-turn games; every check of the matches' default mode (the games are
//! the default mode's). Every mode also checks that a sampled first pick is
//! a candidate of the viewer's prior (in a drafted match: a card its team
//! never saw).
//! An unknown `FUZZ_POOL` value fails. Unset: the default mode, unchanged.

mod common;

use engine::db::CardDb;
use engine::rng::Rng;
use engine::{Choice, DecisionKind, Game, GameSpec, Outcome, NUM_ACTIONS};
use std::sync::atomic::{AtomicU64, Ordering};

/// The interaction groups of `FUZZ_POOL=interactions` (ENGINE.md 38.5; cube
/// cards only, so matches can use them; a misspelt name fails loudly).
const GROUPS: &[(&str, &[&str])] = &[
    ("copies x enter and leave abilities", &[
        "Phyrexian Metamorph", "Fable of the Mirror-Breaker // Reflection of Kiki-Jiki", "Saheeli, Sublime Artificer",
        "Superior Spider-Man", "Fractured Identity", "Springheart Nantuko", "Esika's Chariot", "Vaultborn Tyrant",
        "Thundertrap Trainer", "Sicarian Infiltrator", "Archon of Cruelty", "Grief", "Solitude", "Fury", "Endurance",
        "Subtlety", "Orcish Bowmasters", "Haywire Mite", "Skyclave Apparition", "Flametongue Kavu", "Baleful Strix",
        "Thraben Inspector", "Shifting Woodland", "Seasoned Pyromancer", "Enduring Innocence", "Woodfall Primus",
        "Three Steps Ahead", "Emeritus of Abundance // Regrowth", "Sage of the Skies",
    ]),
    ("flicker x linked exile, the new object", &[
        "Flickerwisp", "Phelia, Exuberant Shepherd", "Palace Jailer", "Leyline Binding", "Portable Hole",
        "Touch the Spirit Realm", "Displacer Kitten", "Skyclave Apparition", "Oust", "Elite Spellbinder",
        "Tidehollow Sculler", "Parallax Wave", "Containment Priest", "Unexpectedly Absent", "Swords to Plowshares",
        "Path to Exile", "Get Lost", "Council's Judgment", "Brazen Borrower // Petty Theft", "Memory Jar",
        "Phyrexian Metamorph", "Fable of the Mirror-Breaker // Reflection of Kiki-Jiki", "Solitude", "Grief",
    ]),
    ("reanimation x replacement effects", &[
        "Reanimate", "Animate Dead", "Exhume", "Necromancy", "Shallow Grave", "Corpse Dance", "Goryo's Vengeance",
        "Unearth", "Entomb", "Faithless Looting", "Frantic Search", "Containment Priest", "Dauthi Voidwalker",
        "Archon of Cruelty", "Griselbrand", "Atraxa, Grand Unifier", "Uro, Titan of Nature's Wrath",
        "Phlage, Titan of Fire's Fury", "Woodfall Primus", "Emrakul, the Aeons Torn", "Sneak Attack",
        "Through the Breach", "Blightsteel Colossus", "Worldspine Wurm", "Yawgmoth's Will", "Underworld Breach",
        "Hullbreacher", "Quantum Riddler", "Sword of the Meek", "Thopter Foundry",
    ]),
    ("face-down x copies, flicker, zones", &[
        "Abhorrent Oculus", "Cloak and Dagger, Entwined", "Headliner Scarlett", "Shelldock Isle", "Memory Jar",
        "Phyrexian Metamorph", "Containment Priest", "Flickerwisp", "Oust", "Swords to Plowshares",
        "Fable of the Mirror-Breaker // Reflection of Kiki-Jiki", "Saheeli, Sublime Artificer", "Oko, Thief of Crowns",
        "Tishana's Tidebinder", "Displacer Kitten", "Fractured Identity", "Dauthi Voidwalker", "Skyclave Apparition",
    ]),
    ("double-faced cards, Adventures x copies, zones, mana value", &[
        "Fable of the Mirror-Breaker // Reflection of Kiki-Jiki", "Concealing Curtains // Revealing Eye",
        "Cecil, Dark Knight // Cecil, Redeemed Paladin", "Jace, Vryn's Prodigy // Jace, Telepath Unbound",
        "Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar", "Etali, Primal Conqueror // Etali, Primal Sickness",
        "Outland Liberator // Frenzied Trapbreaker", "Ulvenwald Oddity // Ulvenwald Behemoth",
        "Jennifer Walters // The Sensational She-Hulk", "Shatterskull Smashing // Shatterskull, the Hammer Pass",
        "Sink into Stupor // Soporific Springs", "Witch Enchanter // Witch-Blessed Meadow", "Brazen Borrower // Petty Theft",
        "Embereth Shieldbreaker // Battle Display", "Emeritus of Abundance // Regrowth", "Norman Osborn // Green Goblin",
        "Life // Death", "Ajani, Nacatl Pariah // Ajani, Nacatl Avenger", "Aang, Swift Savior // Aang and La, Ocean's Fury",
        "Phyrexian Metamorph", "Flickerwisp", "Oust", "Displacer Kitten", "Phelia, Exuberant Shepherd",
        "Saheeli, Sublime Artificer", "Cut Down", "Abrupt Decay", "Fatal Push",
    ]),
    ("counterspells x copies of spells, split, modal, X", &[
        "Counterspell", "Mana Leak", "Memory Lapse", "Remand", "Force of Will", "Force of Negation", "Daze",
        "Mana Drain", "Spell Pierce", "No More Lies", "Stern Scolding", "We Say Thee Nay!", "Lutri, the Spellchaser",
        "Chain Lightning", "Three Steps Ahead", "Mystic Confluence", "Fiery Confluence", "Invert Polarity",
        "Tishana's Tidebinder", "Ertai Resurrected", "Subtlety", "Witherbloom Apprentice", "Sedgemoor Witch",
        "Lose Focus", "Brain Freeze", "Chain of Smog", "Pyroblast", "Kolaghan's Command", "Collective Brutality",
        "Snapcaster Mage", "Flashback", "Fire Covenant", "Pyrokinesis", "Life // Death", "Prismatic Ending",
        "Green Sun's Zenith", "Walking Ballista",
    ]),
    ("control changes x you and its controller", &[
        "Fractured Identity", "Oko, Thief of Crowns", "Dack Fayden", "Thieving Skydiver", "Invert Polarity",
        "Coveted Jewel", "Palace Jailer", "Forth Eorlingas!", "Aragorn, King of Gondor", "Emeritus of Abundance // Regrowth",
        "Hullbreacher", "Ragavan, Nimble Pilferer", "Sol Ring", "Mana Vault", "Lightning Greaves", "Skullclamp",
        "Smuggler's Copter", "Esika's Chariot", "Bolas's Citadel", "Phyrexian Metamorph",
    ]),
    ("taxes, alternative and additional costs x free casts", &[
        "Thalia, Guardian of Thraben", "Etali, Primal Conqueror // Etali, Primal Sickness", "Bolas's Citadel",
        "Dauthi Voidwalker", "Flash", "Force of Will", "Daze", "Solitude", "Fury", "Subtlety", "Grief", "Snapcaster Mage",
        "Yawgmoth's Will", "Underworld Breach", "Lurrus of the Dream-Den", "Zirda, the Dawnwaker", "Mishra's Workshop",
        "Arena of Glory", "Bloodbraid Challenger", "Prismatic Ending", "Walking Ballista", "Malcolm, Alluring Scoundrel",
        "Through the Breach", "Sneak Attack", "Gitaxian Probe", "Dismember", "Mana Tithe", "Spell Pierce",
        "Delighted Halfling", "Lotus Petal", "Black Lotus", "Chrome Mox", "Mox Diamond", "Pillage the Bog",
        "Fallen Shinobi", "Robber of the Rich", "Tersa Lightshatter", "Laelia, the Blade Reforged", "Golos, Tireless Pilgrim",
        "Abhorrent Oculus", "Fire Covenant", "Toxic Deluge",
    ]),
    ("damage x protection, deathtouch, infect, divided damage", &[
        "Giver of Runes", "Mother of Runes", "True-Name Nemesis", "Sylvan Safekeeper", "Lightning Greaves",
        "Emrakul, the Aeons Torn", "Fury", "Pyrokinesis", "Fire Covenant", "Walking Ballista", "Lightning Bolt",
        "Chain Lightning", "Burst Lightning", "Galvanic Discharge", "Sheoldred, the Apocalypse", "Screaming Nemesis",
        "Blightsteel Colossus", "Goblin Bombardment", "Kappa Cannoneer", "Hexdrinker", "Pyrogoyf", "Fiery Confluence",
        "Wizard's Rockets", "Kolaghan's Command", "Suplex", "Skullclamp", "Bone Shards",
        "Shatterskull Smashing // Shatterskull, the Hammer Pass", "Questing Beast", "Kaldra Compleat",
    ]),
    ("planeswalkers x damage, copies, control", &[
        "Oko, Thief of Crowns", "Dack Fayden", "Jace, the Mind Sculptor", "Teferi, Hero of Dominaria", "Teferi, Time Raveler",
        "Chandra, Torch of Defiance", "Karn, Scion of Urza", "Narset, Parter of Veils", "Nissa, Who Shakes the World",
        "Wrenn and Six", "Minsc & Boo, Timeless Heroes", "Tamiyo, Collector of Tales", "The Wandering Emperor",
        "Elspeth, Storm Slayer", "Ugin, Eye of the Storms", "Tezzeret, Cruel Captain", "Kaito, Bane of Nightmares",
        "Saheeli, Sublime Artificer", "Fury", "Lightning Bolt", "Questing Beast", "Phyrexian Metamorph",
        "Fractured Identity", "Clarion Conqueror", "Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar",
        "Jace, Vryn's Prodigy // Jace, Telepath Unbound",
    ]),
    ("last known information, triggers and state-based actions", &[
        "Pyrogoyf", "Jacked Rabbit", "Ouroboroid", "Orcish Bowmasters", "Woodfall Primus", "Sword of the Meek",
        "Thopter Foundry", "Skullclamp", "Laelia, the Blade Reforged", "Nadu, Winged Wisdom", "Hullbreacher",
        "Quantum Riddler", "Sheoldred, the Apocalypse", "Leovold, Emissary of Trest", "Wan Shi Tong, All-Knowing",
        "Inti, Seneschal of the Sun", "Currency Converter", "Ivora, Insatiable Heir", "Bloodtithe Harvester",
        "Carnage Interpreter", "Forensic Gadgeteer", "Generous Plunderer", "Screaming Nemesis", "Walking Ballista",
        "Toxic Deluge", "Balance", "Wrath of God", "Damn", "Sunfall", "Goblin Bombardment", "Swords to Plowshares",
        "Fatal Push", "Tishana's Tidebinder",
    ]),
    ("combat: first and double strike, trample, deathtouch, protection", &[
        "Questing Beast", "True-Name Nemesis", "Ragavan, Nimble Pilferer", "Adeline, Resplendent Cathar",
        "Mother of Runes", "Giver of Runes", "Kaldra Compleat", "Blightsteel Colossus", "Swords to Plowshares",
        "Lightning Greaves", "Skullclamp", "Nettlecyst", "Kappa Cannoneer", "Hexdrinker", "Figure of Destiny",
        "Smuggler's Copter", "Esika's Chariot", "Dauthi Voidwalker", "Dragon's Rage Channeler", "Gut, True Soul Zealot",
        "Okoye, Mighty and Adored", "Surrak, Elusive Hunter", "Coveted Jewel", "Thalia, Guardian of Thraben",
        "Archon of Emeria", "Ocelot Pride", "Guide of Souls", "Luminarch Aspirant",
    ]),
    ("day and night, transform x copies", &[
        "Outland Liberator // Frenzied Trapbreaker", "Ulvenwald Oddity // Ulvenwald Behemoth",
        "Concealing Curtains // Revealing Eye", "Cecil, Dark Knight // Cecil, Redeemed Paladin",
        "Jace, Vryn's Prodigy // Jace, Telepath Unbound", "Fable of the Mirror-Breaker // Reflection of Kiki-Jiki",
        "Phyrexian Metamorph", "Saheeli, Sublime Artificer", "Flickerwisp", "Oust", "Swords to Plowshares",
        "Etali, Primal Conqueror // Etali, Primal Sickness", "Brainstorm", "Ponder", "Lightning Bolt",
    ]),
];

/// `FUZZ_POOL`: unset (the default decks), `interactions` or `drafted`.
fn pool_mode() -> &'static str {
    match std::env::var("FUZZ_POOL").as_deref() {
        Err(_) | Ok("") => "",
        Ok("interactions") => "interactions",
        Ok("drafted") => "drafted",
        Ok(other) => panic!("FUZZ_POOL={other}: unknown (unset, \"interactions\" or \"drafted\")"),
    }
}

fn interactions_mode() -> bool {
    pool_mode() == "interactions"
}

/// The spec of drafted match `seed` (`FUZZ_POOL=drafted`): a 4v4 or 2v2 event
/// (random, or team A as `consensus`) and one of its matches.
fn drafted_spec(seed: u64) -> engine::az::MtgSpec {
    use engine::az::draft::{DraftSpec, TeamDraftAz};
    use engine::az::draft_bots::{match_spec, Consensus};
    use zero_search::game::{Bot, SearchGame};
    let players = if seed % 2 == 0 { 8 } else { 4 };
    let consensus = seed % 4 >= 2;
    let mut ev = TeamDraftAz::new(&DraftSpec::default(), players, seed ^ 0xD2AF_7ED0).unwrap();
    let mut rng = Rng::new(seed ^ 0xD2AF_7ED1);
    let mut bot = Consensus::default();
    let (mut legal, mut keys) = (Vec::new(), Vec::new());
    while let Some(p) = ev.to_move() {
        ev.legal_actions(&mut legal);
        let a = if consensus && p % 2 == 0 { bot.choose(&ev, &legal) } else { legal[rng.below(legal.len() as u32) as usize] };
        ev.step(a, &mut keys).unwrap();
    }
    let ms = ev.matches().unwrap();
    let setup = &ms[(seed / 4) as usize % ms.len()];
    let turns = if seed % 8 == 7 { 12 } else { 60 };
    engine::az::MtgSpec::parse(&match_spec(setup, turns, engine::az::Format::Bo3)).unwrap()
}

/// The groups' cards as ids of `db`, and `db`'s supported lands.
fn interaction_pool(db: &CardDb) -> (Vec<Vec<u16>>, Vec<u16>) {
    let groups = GROUPS
        .iter()
        .map(|(_, names)| names.iter().map(|n| db.expect_id(n).unwrap_or_else(|e| panic!("FUZZ_POOL group card: {e}"))).collect())
        .collect();
    let lands = db.supported().map(|n| db.id(n).unwrap()).filter(|&id| db.def(id).is(engine::db::types::LAND)).collect();
    (groups, lands)
}

/// A combo-dense deck (40 cards: 17 random lands; 23 spells, each from two
/// or three interaction groups, or one time in eight from all of them) and
/// a sideboard of seven from the same groups.
fn interaction_deck(db: &CardDb, rng: &mut Rng) -> (Vec<u16>, Vec<u16>) {
    let (groups, lands) = interaction_pool(db);
    let k = 2 + rng.below(2) as usize;
    let mut chosen: Vec<usize> = Vec::new();
    while chosen.len() < k {
        let i = rng.below(groups.len() as u32) as usize;
        if !chosen.contains(&i) {
            chosen.push(i);
        }
    }
    let cards: Vec<u16> = chosen.iter().flat_map(|&i| groups[i].iter().copied()).collect();
    let all: Vec<u16> = groups.iter().flatten().copied().collect();
    let mut deck: Vec<u16> = (0..17).map(|_| lands[rng.below(lands.len() as u32) as usize]).collect();
    for _ in 0..23 {
        let from = if rng.below(8) == 0 { &all } else { &cards };
        deck.push(from[rng.below(from.len() as u32) as usize]);
    }
    let side = (0..7).map(|_| cards[rng.below(cards.len() as u32) as usize]).collect();
    (deck, side)
}

/// Decisions asked, by kind (printed: what the interaction mode changes).
static KINDS: [AtomicU64; 64] = [const { AtomicU64::new(0) }; 64];

fn count_kind(k: Option<DecisionKind>) {
    if let Some(k) = k {
        KINDS[(k as usize).min(63)].fetch_add(1, Ordering::Relaxed);
    }
}

fn kinds_report() -> String {
    let total: u64 = KINDS.iter().map(|k| k.load(Ordering::Relaxed)).sum();
    let names = [
        DecisionKind::NewTarget,
        DecisionKind::DivideDamage,
        DecisionKind::ChooseExiled,
        DecisionKind::ChoosePermanent,
        DecisionKind::ChooseGraveyardCard,
        DecisionKind::ChooseAltCost,
        DecisionKind::ChooseMode,
        DecisionKind::OrderTriggers,
        DecisionKind::YesNo,
        DecisionKind::Sacrifice,
        DecisionKind::Target,
    ];
    let per = |k: DecisionKind| KINDS[(k as usize).min(63)].load(Ordering::Relaxed) as f64 * 1000.0 / total.max(1) as f64;
    names.iter().map(|&k| format!("{k:?} {:.2}", per(k))).collect::<Vec<_>>().join(", ") + " (per 1,000 decisions)"
}

struct Played {
    actions: Vec<u16>,
    keys: Vec<u64>,
    fingerprint: u64,
    outcome: Outcome,
    turns: u32,
    /// Companions revealed, and put into a hand (ENGINE.md 35).
    companions: u32,
    companions_used: u32,
}

/// The decks and sideboards of game `seed` (ENGINE.md 35): one game in four
/// has the self-play generator's decks (two colours, singleton, a sideboard,
/// often a companion, a deck built for it); the others random decks with a
/// sideboard of seven random cards.
fn decks(seed: u64) -> [(Vec<u16>, Vec<u16>); 2] {
    let db = common::db();
    let mut rng = Rng::new(seed ^ 0xDEC5);
    let mut side = Rng::new(seed ^ 0x51DE);
    if interactions_mode() {
        return [interaction_deck(db, &mut rng), interaction_deck(db, &mut rng)];
    }
    if seed % 4 == 3 {
        let gen = engine::az::decks::DeckGen::default();
        return [
            engine::az::decks::generate_pool(db, &mut rng, &mut side, &gen),
            engine::az::decks::generate_pool(db, &mut rng, &mut side, &gen),
        ];
    }
    let (_, spells) = common::fuzz_pool();
    let sideboard = |side: &mut Rng| -> Vec<u16> { (0..7).map(|_| spells[side.below(spells.len() as u32) as usize]).collect() };
    let d0 = common::random_deck(db, &mut rng);
    let d1 = common::random_deck(db, &mut rng);
    [(d0, sideboard(&mut side)), (d1, sideboard(&mut side))]
}

fn new_game(seed: u64) -> Game {
    let [(d0, s0), (d1, s1)] = decks(seed);
    // `FUZZ_LOG=1` keeps the game log, so a panic message shows the game.
    let log = std::env::var("FUZZ_LOG").is_ok();
    Game::with_sideboards(common::db(), GameSpec { log, ..GameSpec::default() }, [&d0, &d1], [&s0, &s1], seed).unwrap()
}

/// Play one game with random actions; run the extra checks along the way.
fn play(seed: u64, checks: bool) -> Played {
    let mut g = new_game(seed);
    let mut rng = Rng::new(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 1);
    // (The completion check's sampling: its own generator, so the game's
    // actions are the same with or without it.)
    let mut crng = Rng::new(seed ^ 0xC0_4D_1E7E);
    let mut legal = Vec::new();
    let mut actions = Vec::new();
    let mut keys = Vec::new();
    let mut steps = 0;
    while !g.is_over() {
        g.legal_actions(&mut legal);
        assert!(!legal.is_empty(), "seed {seed}: no legal action in a running game");
        assert!(legal.windows(2).all(|w| w[0] < w[1]), "seed {seed}: actions not sorted/unique");
        assert!(legal.iter().all(|&a| (a as usize) < NUM_ACTIONS));
        let a = legal[rng.below(legal.len() as u32) as usize];
        if checks {
            count_kind(g.kind());
        }
        if checks && rng.below(40) == 0 {
            check_clone(&g, a, seed);
            check_determinize(&g, seed, &mut rng);
        }
        if checks && complete_now(&g, &mut crng) {
            check_options_complete(&g, seed, "the game");
        }
        g.apply(a, &mut keys).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        g.check_invariants().unwrap_or_else(|e| panic!("seed {seed} step {steps}: {e}
{}", g.log_lines().iter().rev().take(60).rev().cloned().collect::<Vec<_>>().join("
")));
        actions.push(a);
        steps += 1;
        assert!(steps < 100_000, "seed {seed}: game does not end
{}", g.log_lines().iter().rev().take(40).rev().cloned().collect::<Vec<_>>().join("
"));
    }
    let companions = (0..2).filter(|&p| g.player(p).companion.is_some()).count() as u32;
    let companions_used = (0..2).filter(|&p| g.player(p).companion_used).count() as u32;
    Played { actions, keys, fingerprint: g.fingerprint(), outcome: g.outcome().unwrap(), turns: g.turn().number, companions, companions_used }
}

/// A clone continues exactly like the original.
fn check_clone(g: &Game, a: u16, seed: u64) {
    let mut x = g.clone();
    let mut y = g.clone();
    let (mut kx, mut ky) = (Vec::new(), Vec::new());
    x.apply(a, &mut kx).unwrap();
    y.apply(a, &mut ky).unwrap();
    assert_eq!(x.fingerprint(), y.fingerprint(), "seed {seed}: clones diverged");
    assert_eq!(kx, ky);
}

/// `determinize(to_move)` keeps the decider's legal actions and the public
/// state; the resampled world keeps playing without breaking invariants.
fn check_determinize(g: &Game, seed: u64, rng: &mut Rng) {
    let mover = g.to_move().unwrap();
    // The decider (what the search does) and the other player (tests and
    // analysis: its determinize refreshes the decider's decision).
    for viewer in [mover, 1 - mover] {
        let mut d = g.clone();
        d.determinize(viewer, rng);
        if viewer == mover {
            let (mut l1, mut l2) = (Vec::new(), Vec::new());
            g.legal_actions(&mut l1);
            d.legal_actions(&mut l2);
            assert_eq!(l1, l2, "seed {seed}: determinize changed the decider's actions");
        } else {
            assert_eq!(d.to_move(), Some(mover), "seed {seed}: a refresh ran past the decision");
        }
        // Nothing the viewer can observe changes; no card it knows exactly
        // is resampled.
        assert_eq!(
            g.view_of(viewer),
            d.view_of(viewer),
            "seed {seed}: determinize changed the view of P{viewer} (mover P{mover})
{}",
            g.log_lines().iter().rev().take(20).cloned().collect::<Vec<_>>().join("
")
        );
        for (i, o) in g.objs().iter().enumerate() {
            let c = engine::CardIdx(i as u16);
            if g.knows(viewer, c) {
                assert_eq!(o.def, d.obj(c).def, "seed {seed}: a card P{viewer} knows was resampled");
                assert_eq!(g.ident(c), d.ident(c), "seed {seed}: a face-down card P{viewer} knows was resampled");
            }
        }
        d.check_invariants().unwrap();
        // What the search does in that world: try every option.
        check_options_complete(&d, seed, &format!("a world determinized for P{viewer} (mover P{mover})"));
        if viewer == mover {
            check_information_set(g, viewer, seed, rng);
        }
        let mut legal = Vec::new();
        let mut keys = Vec::new();
        for _ in 0..30 {
            if d.is_over() {
                break;
            }
            d.legal_actions(&mut legal);
            d.apply(legal[rng.below(legal.len() as u32) as usize], &mut keys).unwrap();
            d.check_invariants().unwrap_or_else(|e| {
                panic!(
                    "seed {seed} (determinized for P{viewer}): {e}\n{}",
                    d.log_lines().iter().rev().take(40).rev().cloned().collect::<Vec<_>>().join("\n")
                )
            });
        }
    }
}

/// Decisions followed after an option before giving up on reaching priority.
const COMPLETE_STEPS: usize = 64;

/// Options the completion check tried, and the decisions it answered after
/// them (reported by `fuzz_random_games`).
static OPTIONS_TRIED: AtomicU64 = AtomicU64::new(0);
static OPTION_STEPS: AtomicU64 = AtomicU64::new(0);

/// Whether the completion check runs at this decision of the game: not at
/// mulligans and combat declarations (no cast or activation starts there), at
/// half of the frequent priority, payment and target decisions (a castable
/// card stays castable over many of them), at every other one (the rare
/// ones: an effect's "cast it?", a cost option, an X, a sacrifice, ...).
fn complete_now(g: &Game, rng: &mut Rng) -> bool {
    use DecisionKind as K;
    match g.kind() {
        None | Some(K::Mulligan | K::MulliganBottom | K::DeclareAttackers | K::DeclareBlockers | K::AssignDamage | K::AttackTarget) => {
            false
        }
        Some(K::Priority | K::PayMana | K::Target) => rng.below(2) == 0,
        Some(_) => true,
    }
}

/// Every option of the pending decision can be completed (ENGINE.md 6, 18: a
/// cast, an activation or an alternative cost is offered only if it can be
/// completed, so nothing is ever rewound): each legal action, applied to a
/// clone, then the decisions after it answered along one legal path (random,
/// from a generator of its own: the game's sequence is unchanged) until the
/// next priority decision, never panics or fails. The payment's own
/// decisions (targets, X, the cost option, mana sources, sacrifices,
/// discards) all come before the caster's next priority (CR 601.2i, 117.3c).
/// At priority, passing and playing a land are left out (no cost to
/// complete).
fn check_options_complete(g: &Game, seed: u64, world: &str) {
    let Some(d) = g.pending_decision() else { return };
    if d.options.len() < 2 {
        return; // (the game itself takes the one option)
    }
    let priority = d.kind == DecisionKind::Priority;
    for o in &d.options {
        if priority && matches!(o.choice, Choice::Pass | Choice::Play(_) | Choice::PlayFace(_)) {
            continue;
        }
        let a = o.action;
        let path_seed = seed ^ (g.decisions() as u64).wrapping_mul(0x9E37_79B9) ^ ((a as u64) << 40);
        // The option, then one random legal path to the next priority;
        // `describe` keeps the path's descriptions (for a failure).
        let follow = |describe: &mut Option<Vec<String>>| -> Result<(), String> {
            let mut prng = Rng::new(path_seed);
            let mut x = g.clone();
            let (mut keys, mut l) = (Vec::new(), Vec::new());
            x.apply(a, &mut keys)?;
            for _ in 0..COMPLETE_STEPS {
                if x.is_over() || x.kind() == Some(DecisionKind::Priority) {
                    break;
                }
                x.legal_actions(&mut l);
                let b = l[prng.below(l.len() as u32) as usize];
                if let Some(p) = describe {
                    p.push(format!("{b} = {}", x.describe_action(b).unwrap_or_default()));
                }
                OPTION_STEPS.fetch_add(1, Ordering::Relaxed);
                x.apply(b, &mut keys)?;
            }
            Ok(())
        };
        OPTIONS_TRIED.fetch_add(1, Ordering::Relaxed);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| follow(&mut None)));
        let err = match r {
            Ok(Ok(())) => continue,
            Ok(Err(e)) => e,
            Err(p) => p
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default(),
        };
        // The same path again, described.
        let mut path = Some(Vec::new());
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| follow(&mut path)));
        panic!(
            "seed {seed}: in {world}, option {a} ({}) of {:?} for P{} can't be completed: {err}\npath after it: {:?}\noptions: {:?}\n{}",
            g.describe_action(a).unwrap_or_default(),
            d.kind,
            d.player,
            path.unwrap_or_default(),
            g.options_desc(),
            g.log_lines().iter().rev().take(40).rev().cloned().collect::<Vec<_>>().join("\n")
        );
    }
}

/// Every revealed fact is keyed: two worlds `determinize(viewer)` samples,
/// played with the same actions, stay indistinguishable to the viewer as
/// long as the search would keep them in one node: zero-core keys children
/// by (chance keys, next decider, next legal actions), so equal keys, an
/// equal next decider and equal legal actions must mean an equal view.
fn check_information_set(g: &Game, viewer: u8, seed: u64, rng: &mut Rng) {
    let s = rng.next_u64();
    let mut a = g.clone();
    a.determinize(viewer, &mut Rng::new(s));
    let mut b = g.clone();
    b.determinize(viewer, &mut Rng::new(s ^ 0x5EED));
    let (mut la, mut lb) = (Vec::new(), Vec::new());
    for step in 0..25 {
        a.legal_actions(&mut la);
        b.legal_actions(&mut lb);
        if a.to_move() != b.to_move() || la != lb {
            // Another player's options may depend on its hidden cards (and a
            // decision with one option is answered internally, so who
            // decides next may differ too): the search keeps such worlds in
            // different nodes. The viewer's own options never differ.
            assert!(
                !(a.to_move() == Some(viewer) && b.to_move() == Some(viewer)),
                "seed {seed} step {step}: the viewer's options differ: {:?} / {:?}",
                a.options_desc(),
                b.options_desc()
            );
            return;
        }
        assert_eq!(a.view_of(viewer), b.view_of(viewer), "seed {seed} step {step}: same keys, different views
{}
----
{}", a.log_lines().iter().rev().take(25).rev().cloned().collect::<Vec<_>>().join("
"), b.log_lines().iter().rev().take(25).rev().cloned().collect::<Vec<_>>().join("
"));
        if a.is_over() {
            return;
        }
        let act = la[rng.below(la.len() as u32) as usize];
        let (mut ka, mut kb) = (Vec::new(), Vec::new());
        a.apply(act, &mut ka).unwrap();
        b.apply(act, &mut kb).unwrap();
        if ka != kb {
            return; // different outcomes, told apart by the keys
        }
    }
}

#[test]
fn fuzz_random_games() {
    let n: u64 = std::env::var("FUZZ_GAMES").ok().and_then(|s| s.parse().ok()).unwrap_or(300);
    // `FUZZ_FIRST=s` starts at seed s (to rerun a failing seed alone).
    let first: u64 = std::env::var("FUZZ_FIRST").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    let (mut wins, mut draws, mut turns, mut decisions) = ([0u32; 2], 0u32, 0u64, 0u64);
    let (mut companions, mut companions_used) = (0u32, 0u32);
    for seed in first..first + n {
        let p = std::panic::catch_unwind(|| play(seed, true)).unwrap_or_else(|e| {
            panic!("seed {seed} failed (rerun: FUZZ_FIRST={seed} FUZZ_GAMES=1 FUZZ_LOG=1): {e:?}")
        });
        match p.outcome.winner {
            Some(w) => wins[w as usize] += 1,
            None => draws += 1,
        }
        turns += p.turns as u64;
        decisions += p.actions.len() as u64;
        companions += p.companions;
        companions_used += p.companions_used;
    }
    println!(
        "{n} games: wins {wins:?}, draws {draws}, mean turns {:.1}, mean decisions {:.1}; {} options completed ({} decisions after them)",
        turns as f64 / n as f64,
        decisions as f64 / n as f64,
        OPTIONS_TRIED.load(Ordering::Relaxed),
        OPTION_STEPS.load(Ordering::Relaxed)
    );
    println!("companions revealed {companions}, put into a hand {companions_used}");
    println!("decisions{}: {}", if interactions_mode() { " (FUZZ_POOL=interactions)" } else { "" }, kinds_report());
    if n >= 200 && !interactions_mode() {
        assert!(companions_used > 0, "no companion special action in {n} games");
    }
}

#[test]
fn same_seed_and_actions_replay_identically() {
    for seed in 1000..1030 {
        let a = play(seed, false);
        // Replay the recorded actions from the same seed.
        let mut g = new_game(seed);
        let mut keys = Vec::new();
        for &x in &a.actions {
            g.apply(x, &mut keys).unwrap();
        }
        assert_eq!(g.fingerprint(), a.fingerprint, "seed {seed}");
        assert_eq!(keys, a.keys, "seed {seed}: chance keys differ");
        assert_eq!(g.outcome(), Some(a.outcome));
        // And the whole random game is reproducible.
        let b = play(seed, false);
        assert_eq!(b.fingerprint, a.fingerprint);
    }
}

/// The spec of fuzzed match `seed`: random pools (bo3's default), the
/// evaluation pools, M8a's generated decks with 7-card sideboards, or short
/// games (drawn games: the turn cap).
fn match_spec(seed: u64) -> engine::az::MtgSpec {
    if pool_mode() == "drafted" {
        return drafted_spec(seed);
    }
    if interactions_mode() {
        // Combo-dense lists (cube cards), best-of-three, open decklists;
        // one match in four short.
        let db = CardDb::cube();
        let mut rng = Rng::new(seed ^ 0x1A7E_DEC5);
        let p = [interaction_deck(db, &mut rng), interaction_deck(db, &mut rng)];
        let mut s = engine::az::MtgSpec::parse(r#"{"format": "bo3", "pools": false}"#).unwrap();
        s.decks = engine::az::DeckMode::ListsWithSideboards(p);
        if seed % 4 == 3 {
            s.max_turns = 12;
        }
        return s;
    }
    let json = match seed % 4 {
        0 => r#"{"format": "bo3"}"#,
        1 => r#"{"format": "bo3", "decks": "eval"}"#,
        2 => r#"{"format": "bo3", "pools": false}"#,
        _ => r#"{"format": "bo3", "max_turns": 12}"#,
    };
    engine::az::MtgSpec::parse(json).unwrap()
}

struct PlayedMatch {
    actions: Vec<u16>,
    keys: Vec<u64>,
    fingerprint: u64,
    results: [u8; 3],
    games: u8,
    sideboard_decisions: u32,
    changed_decks: u32,
}

/// Counts of the hidden-pool checks (ENGINE.md 36.11).
#[derive(Default)]
struct HiddenStats {
    worlds: u64,
    first_picks_changed: u64,
    decks_changed: u64,
}

/// A world `determinize(viewer)` sampled (`d`) from match `g`: the viewer's
/// view and own cards unchanged, no card it knows resampled; the opponent's
/// pool its known part plus as many first picks; their deck a legal deck of
/// that pool with the public sizes, holding every card of theirs the viewer
/// knows, fulfilling a revealed companion, and in the game the world's
/// cards; the clouds hold.
fn check_hidden_pool(g: &engine::az::MtgAz, d: &engine::az::MtgAz, viewer: u8, seed: u64, stats: &mut HiddenStats) {
    use engine::az::sample;
    let o = 1 - viewer;
    let (v, ou) = (viewer as usize, o as usize);
    assert_eq!(g.m.pools[v], d.m.pools[v], "match {seed}: the viewer's pool was resampled");
    assert_eq!(g.m.decks[v], d.m.decks[v], "match {seed}: the viewer's deck was resampled");
    assert_eq!(g.g.player(viewer).sideboard, d.g.player(viewer).sideboard, "match {seed}: the viewer's sideboard was resampled");
    let ids = engine::az::identity::identities(g.g.db());
    let (rp, sp) = (&g.m.pools[ou], &d.m.pools[ou]);
    let known = |m: &engine::az::MatchState, pool: &engine::az::matches::MatchPool| -> Vec<(u16, u8)> {
        pool.list[..pool.n_cards].iter().copied().filter(|e| sample::known_before(m, o, ids, e.0)).collect()
    };
    assert_eq!(known(&g.m, rp), known(&d.m, sp), "match {seed}: the known part of P{o}'s pool changed");
    assert_eq!(rp.first_picks.len(), sp.first_picks.len());
    assert_eq!(rp.list.len(), sp.list.len());
    assert!(sp.list[..sp.n_cards].windows(2).all(|w| w[0].0 < w[1].0), "match {seed}: sampled pool not sorted");
    assert!(sp.first_picks.iter().all(|f| sp.list.iter().any(|e| e.0 == *f)));
    assert!(sp.first_picks.iter().all(|f| !g.m.pools[v].list.iter().any(|e| e.0 == *f)), "match {seed}: a first pick in the viewer's pool");
    if let Some(pri) = &g.m.priors {
        // a sampled first pick is a candidate of the prior (a real one kept: seen)
        assert!(
            sp.first_picks.iter().all(|f| pri[ou].cands.contains(f) || rp.first_picks.contains(f)),
            "match {seed}: a sampled first pick outside the prior"
        );
    }
    let size = |c: &[u8]| c.iter().map(|&n| n as usize).sum::<usize>();
    assert_eq!(size(&g.m.decks[ou]), size(&d.m.decks[ou]), "match {seed}: deck size");
    for (k, &(_, n)) in sp.list.iter().enumerate() {
        assert!(d.m.decks[ou][k] <= n, "match {seed}: more copies than the pool has");
    }
    let deck = sp.deck(&d.m.decks[ou]);
    if let Some(c) = d.g.player(o).companion_def {
        if let Some(cond) = &g.g.db().def(c).companion {
            assert!(g.g.db().deck_satisfies(cond, &deck), "match {seed}: the sampled deck breaks the revealed companion");
        }
    }
    // What they revealed in this game (between games: the last) is in it.
    let comp = g.g.player(o).companion_def;
    for &(dd, _) in &sp.list {
        let i = ids[dd as usize] as usize;
        if g.m.seen[ou][i >> 6] & (1 << (i & 63)) != 0 && Some(dd) != comp {
            assert!(deck.contains(&dd), "match {seed}: a card P{o} revealed this game is not in the sampled deck");
        }
    }
    stats.worlds += 1;
    stats.first_picks_changed += (rp.first_picks != sp.first_picks) as u64;
    stats.decks_changed += (rp.deck(&g.m.decks[ou]) != deck) as u64;
    if d.sideboarding().is_some() {
        return;
    }
    let mut want = deck.clone();
    if let Some(c) = d.g.player(o).companion_def {
        want.push(c);
        want.sort_unstable();
    }
    assert_eq!(sample::game_cards(d, o), want, "match {seed}: the world's cards of P{o} are not the sampled deck (and companion)");
    assert_eq!(d.g.player(o).sideboard.as_ref(), sp.sideboard(&d.m.decks[ou]).as_slice(), "match {seed}: the sampled sideboard");
    assert_eq!(g.g.sideboard_size(o), d.g.sideboard_size(o));
    assert_eq!(g.g.view_of(viewer), d.g.view_of(viewer), "match {seed}: determinize changed the view of P{viewer}");
    for (i, x) in g.g.objs().iter().enumerate() {
        let c = engine::CardIdx(i as u16);
        if g.g.knows(viewer, c) {
            assert_eq!((x.def, g.g.ident(c)), (d.g.obj(c).def, d.g.ident(c)), "match {seed}: a card P{viewer} knows was resampled");
        }
    }
    d.g.check_invariants().unwrap_or_else(|e| panic!("match {seed} (a world of P{viewer}): {e}"));
    d.g.check_knowledge().unwrap_or_else(|e| panic!("match {seed} (a world of P{viewer}): {e}"));
}

/// Two worlds sampled for the decider, played with the same actions, give it
/// the same encoding while their keys, next decider and legal actions agree
/// (zero-core's child signature), across games and sideboarding.
fn check_match_information_set(g: &engine::az::MtgAz, viewer: usize, seed: u64, rng: &mut Rng) {
    use zero_search::game::SearchGame;
    let s = rng.next_u64();
    let mut a = g.clone();
    a.determinize(viewer, &mut zero_search::rng::Rng::new(s));
    let mut b = g.clone();
    b.determinize(viewer, &mut zero_search::rng::Rng::new(s ^ 0x5EED));
    let (mut la, mut lb) = (Vec::new(), Vec::new());
    let (mut xa, mut xb) = (vec![0.0f32; engine::az::NUM_FEATURES], vec![0.0f32; engine::az::NUM_FEATURES]);
    for step in 0..25 {
        if a.match_over() || b.match_over() {
            return;
        }
        a.legal_actions(&mut la);
        b.legal_actions(&mut lb);
        if a.to_move() != b.to_move() || la != lb {
            assert!(!(a.to_move() == Some(viewer) && b.to_move() == Some(viewer)), "match {seed} step {step}: the viewer's options differ");
            return;
        }
        xa.fill(0.0);
        xb.fill(0.0);
        a.encode(viewer, &mut xa);
        b.encode(viewer, &mut xb);
        if let Some(i) = (0..xa.len()).find(|&i| xa[i] != xb[i]) {
            panic!(
                "match {seed} step {step}: same keys, different encodings of P{viewer} at {i} ({} / {}); decisions {:?} / {:?}; game {} sb {}
{}
----
{}",
                xa[i],
                xb[i],
                a.pending_decision(),
                b.pending_decision(),
                a.m.game_no,
                a.sideboarding().is_some(),
                stack_names(&a.g),
                stack_names(&b.g)
            );
        }
        let act = la[rng.below(la.len() as u32) as usize];
        let (mut ka, mut kb) = (Vec::new(), Vec::new());
        a.step(act, &mut ka).unwrap();
        b.step(act, &mut kb).unwrap();
        if ka != kb {
            return;
        }
    }
}

fn stack_names(g: &Game) -> String {
    let db = g.db();
    let mut v: Vec<String> = g
        .stack()
        .iter()
        .map(|it| match it.kind {
            engine::game::StackKind::Spell { card } => format!("spell {} (own {})", db.def(g.obj(card).def).name, db.def(g.own_def(card)).name),
            engine::game::StackKind::Ability { source, .. } => format!("ability of {}", db.def(g.obj(source.idx).def).name),
        })
        .collect();
    for p in 0..2u8 {
        v.push(format!("P{p} battlefield: {:?}", g.objs().iter().filter(|o| o.zone == engine::Zone::Battlefield && o.controller == p).map(|o| db.def(o.def).name.clone()).collect::<Vec<_>>()));
    }
    v.join("
")
}

fn play_match(seed: u64, checks: bool) -> PlayedMatch {
    play_match_stats(seed, checks, &mut HiddenStats::default())
}

fn play_match_stats(seed: u64, checks: bool, stats: &mut HiddenStats) -> PlayedMatch {
    use engine::az::MtgAz;
    use zero_search::game::SearchGame;
    let sp = match_spec(seed);
    let mut g = MtgAz::new_game(&sp, 2, seed).unwrap_or_else(|e| panic!("match {seed}: {e}"));
    let mut rng = Rng::new(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 5);
    let (mut legal, mut legal2, mut keys, mut all_keys, mut actions) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut x1 = vec![0.0f32; engine::az::NUM_FEATURES];
    let mut x2 = vec![0.0f32; engine::az::NUM_FEATURES];
    let (mut sideboard_decisions, mut changed_decks) = (0u32, 0u32);
    let mut decks = [g.m.decks[0].clone(), g.m.decks[1].clone()];
    let mut game_no = 1;
    while let Some(p) = g.to_move() {
        g.legal_actions(&mut legal);
        assert!(!legal.is_empty(), "match {seed}: no legal action");
        assert!(legal.windows(2).all(|w| w[0] < w[1]), "match {seed}: actions not sorted/unique");
        assert!(legal.iter().all(|&a| (a as usize) < NUM_ACTIONS));
        let sb = g.sideboarding().is_some();
        sideboard_decisions += sb as u32;
        let a = legal[rng.below(legal.len() as u32) as usize];
        if checks && rng.below(40) == 0 {
            // A clone continues identically.
            let (mut c1, mut c2) = (g.clone(), g.clone());
            let (mut k1, mut k2) = (Vec::new(), Vec::new());
            c1.step(a, &mut k1).unwrap();
            c2.step(a, &mut k2).unwrap();
            assert_eq!((c1.g.fingerprint(), &k1, c1.m.game_no), (c2.g.fingerprint(), &k2, c2.m.game_no), "match {seed}: clones diverged");
            // determinize keeps the decider's actions, both viewers' view
            // and encoding, and samples the opponent's hidden pool and deck
            // consistently (ENGINE.md 36.11).
            for viewer in [p, 1 - p] {
                let mut d = g.clone();
                d.determinize(viewer, &mut zero_search::rng::Rng::new(rng.next_u64()));
                d.legal_actions(&mut legal2);
                if viewer == p {
                    assert_eq!(legal, legal2, "match {seed}: determinize changed the decider's actions");
                } else {
                    assert_eq!(d.to_move(), Some(p), "match {seed}: determinize changed the decider: {:?} -> {:?}", g.pending_decision(), d.pending_decision());
                }
                x1.fill(0.0);
                x2.fill(0.0);
                g.encode(viewer, &mut x1);
                d.encode(viewer, &mut x2);
                if let Some(i) = (0..x1.len()).find(|&i| x1[i] != x2[i]) {
                    panic!("match {seed}: determinize changed the encoding of P{viewer} (mover P{p}) at {i}: {} -> {}", x1[i], x2[i]);
                }
                check_hidden_pool(&g, &d, viewer as u8, seed, stats);
                if viewer == p && rng.below(4) == 0 {
                    check_match_information_set(&g, viewer, seed, &mut rng);
                }
                // The world plays on.
                let mut kd = Vec::new();
                for _ in 0..30 {
                    if d.to_move().is_none() {
                        break;
                    }
                    d.legal_actions(&mut legal2);
                    let a2 = legal2[rng.below(legal2.len() as u32) as usize];
                    d.step(a2, &mut kd).unwrap_or_else(|e| panic!("match {seed} (determinized for P{viewer}): {e}"));
                    if d.sideboarding().is_none() && !d.match_over() {
                        d.g.check_invariants().unwrap_or_else(|e| panic!("match {seed} (determinized for P{viewer}): {e}"));
                    }
                }
            }
        }
        g.step(a, &mut keys).unwrap_or_else(|e| panic!("match {seed}: {e}"));
        all_keys.append(&mut keys);
        actions.push(a);
        if g.sideboarding().is_none() && !g.match_over() {
            g.g.check_invariants().unwrap_or_else(|e| panic!("match {seed} step {}: {e}", actions.len()));
        }
        if g.m.game_no != game_no && g.sideboarding().is_none() {
            // A later game: its decks are legal and from the pools; its
            // chooser is the previous game's loser, or after a draw its
            // chooser (CR 103.1).
            game_no = g.m.game_no;
            let i = (game_no - 2) as usize;
            let want = match g.m.results[i] {
                3 => g.m.choosers[i],
                w => 1 - (w - 1),
            };
            assert_eq!(g.g.turn().chooser, want, "match {seed}: the chooser of game {game_no}");
            for q in 0..2u8 {
                let pool = &g.m.pools[q as usize];
                let deck = g.m.deck(q);
                // (Every fuzzed deck has 40 cards, and a pool's deck keeps
                // them: ENGINE.md 42.)
                assert_eq!((pool.min_deck, pool.max_deck, deck.len()), (40, 40, 40), "match {seed}: {} cards", deck.len());
                for (k, &(_, n)) in pool.list.iter().enumerate() {
                    assert!(g.m.decks[q as usize][k] <= n.max(decks[q as usize][k]));
                }
                changed_decks += (g.m.decks[q as usize] != decks[q as usize]) as u32;
            }
            decks = [g.m.decks[0].clone(), g.m.decks[1].clone()];
        }
        assert!(actions.len() < 300_000, "match {seed}: does not end");
    }
    let m = &g.m;
    assert!(g.match_over());
    assert!(m.wins.contains(&2) || m.game_no == 3, "match {seed}: ended early: {:?}", m.results);
    assert_eq!(m.wins[0] + m.wins[1] + m.draws, m.game_no);
    let r = g.result();
    assert!((r[0] + r[1] - 1.0).abs() < 1e-6);
    PlayedMatch {
        actions,
        keys: all_keys,
        fingerprint: g.g.fingerprint(),
        results: m.results,
        games: m.game_no,
        sideboard_decisions,
        changed_decks,
    }
}

#[test]
fn fuzz_random_matches() {
    let games: u64 = std::env::var("FUZZ_GAMES").ok().and_then(|s| s.parse().ok()).unwrap_or(300);
    let n: u64 = std::env::var("FUZZ_MATCHES").ok().and_then(|s| s.parse().ok()).unwrap_or((games / 20).max(20));
    let first: u64 = std::env::var("FUZZ_FIRST").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    let (mut games_played, mut draws, mut decisions, mut sb, mut changed, mut three) = (0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
    let mut stats = HiddenStats::default();
    for seed in first..first + n {
        let p = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| play_match_stats(seed, true, &mut stats)))
            .unwrap_or_else(|e| panic!("match {seed} failed (rerun: FUZZ_FIRST={seed} FUZZ_MATCHES=1): {e:?}"));
        games_played += p.games as u64;
        draws += p.results.iter().filter(|&&r| r == 3).count() as u64;
        decisions += p.actions.len() as u64;
        sb += p.sideboard_decisions as u64;
        changed += p.changed_decks as u64;
        three += (p.games == 3) as u64;
    }
    println!(
        "{n} matches: {games_played} games ({three} matches of three, {draws} drawn games), mean decisions {:.1}, \
         sideboarding decisions {sb}, decks changed {changed}; hidden pools: {} worlds, first picks resampled in {}, \
         decks in {}",
        decisions as f64 / n as f64,
        stats.worlds,
        stats.first_picks_changed,
        stats.decks_changed
    );
    assert!(sb > 0 && changed > 0 && three > 0);
    // (Interaction lists have no first picks: open decklists.)
    assert!(
        n < 20 || interactions_mode() || stats.first_picks_changed > stats.worlds / 4 && stats.decks_changed > stats.worlds / 4,
        "the hidden pool is rarely resampled"
    );
}

#[test]
fn same_seed_and_actions_replay_matches_identically() {
    use engine::az::MtgAz;
    use zero_search::game::SearchGame;
    for seed in 2000..2012 {
        let a = play_match(seed, false);
        let mut g = MtgAz::new_game(&match_spec(seed), 2, seed).unwrap();
        let mut keys = Vec::new();
        for &x in &a.actions {
            g.step(x, &mut keys).unwrap();
        }
        assert_eq!((g.g.fingerprint(), g.m.results), (a.fingerprint, a.results), "match {seed}");
        assert_eq!(keys, a.keys, "match {seed}: chance keys differ");
        assert_eq!(play_match(seed, false).fingerprint, a.fingerprint);
    }
}
