//! EoE sealed (ENGINE.md 101): the collations' pools in engine terms, the randomised builder's
//! decks (always 40 cards, castable, the splash rule, the spread of colours / splashes / land
//! counts), and the `mtg` spec's `"sealed"` key (reproducible games, the evaluation pairs, the
//! errors, games played through `MtgAz`).
//!
//! `cargo test --release -p engine --test sealed -- --nocapture` prints the builder's spread over
//! 1,000 pools of each collation.

use engine::az::sealed::builder::{self, Build, Settings};
use engine::az::sealed::collation::{self, Collation, Options};
use engine::az::sealed::{self, SealedSpec};
use engine::az::{MtgAz, MtgSpec};
use engine::db::{types, CardDb};
use engine::rng::Rng;
use zero_search::game::SearchGame;

fn db() -> &'static CardDb {
    CardDb::eoe()
}

/// Pool `seed` of `c` in engine terms, or None when it holds a card the engine doesn't support
/// (on this base: The Dominion Bracelet, another branch's).
fn pool(c: Collation, seed: u64) -> Option<Vec<u16>> {
    let p = collation::generate(seed, c, Options::default()).unwrap();
    sealed::card_ids(db(), &p.cards()).ok()
}

/// The checks every build passes (module docs of `builder.rs`).
fn check_build(pool: &[u16], b: &Build, s: &Settings, what: &str) {
    let db = db();
    let basics = engine::az::pools::basic_ids(db);
    assert_eq!(b.deck.len(), 40, "{what}: {} cards", b.deck.len());
    // deck + sideboard = the pool + the deck's basics
    let mut all: Vec<u16> = b.deck.iter().copied().filter(|d| !basics.contains(d)).chain(b.sideboard.iter().copied()).collect();
    all.sort_unstable();
    let mut p = pool.to_vec();
    p.sort_unstable();
    assert_eq!(all, p, "{what}: deck + sideboard is not the pool");
    assert!(b.sideboard.iter().all(|d| !basics.contains(d)), "{what}: a basic in the sideboard");
    let cols = b.colors | b.splash;
    assert!(matches!(b.colors.count_ones(), 1 | 2), "{what}: colours {:05b}", b.colors);
    assert!(b.splash == 0 || (b.splash.count_ones() == 1 && b.colors.count_ones() == 2 && b.colors & b.splash == 0), "{what}: splash");
    let mut splash_cards = 0;
    for &id in &b.deck {
        let d = db.def(id);
        if d.is(types::LAND) {
            continue;
        }
        assert!(builder::playable(d, cols), "{what}: {} not castable in {:05b}", d.name, cols);
        if !builder::playable(d, b.colors) {
            splash_cards += 1;
            let xi = b.splash.trailing_zeros() as usize;
            assert_eq!(d.cost.pips[xi], 1, "{what}: splash card {} has {} pips of the splash", d.name, d.cost.pips[xi]);
        }
    }
    assert_eq!(splash_cards, b.splash_spells as usize, "{what}: splash cards");
    let lands = b.deck.iter().filter(|&&id| db.def(id).is(types::LAND)).count();
    assert_eq!(lands, b.lands as usize);
    assert!(lands >= s.lands.0 as usize, "{what}: {lands} lands");
    if b.splash != 0 {
        // The splash is castable: a fixer of the splash colour in the deck, and enough sources.
        let fixers = b.deck.iter().filter(|&&id| !basics.contains(&id) && builder::fixes(db, db.def(id)) & b.splash != 0).count();
        let xi = b.splash.trailing_zeros() as usize;
        let basics_x = b.deck.iter().filter(|&&id| id == basics[xi]).count();
        assert!(fixers >= 1, "{what}: a splash without a fixer");
        assert!(fixers + basics_x >= s.splash_sources as usize, "{what}: {fixers} fixers + {basics_x} basics");
    }
}

#[test]
fn the_builder_builds_40_cards_from_the_pool_deterministically() {
    let s = Settings::default();
    let mut unsupported = 0;
    for c in [Collation::Arena, Collation::Paper] {
        for seed in 0..300u64 {
            let Some(p) = pool(c, seed) else {
                unsupported += 1;
                continue;
            };
            let b = builder::build(db(), &p, &s, &mut Rng::new(seed));
            check_build(&p, &b, &s, &format!("{c:?} pool {seed}"));
            assert!(b.lands <= s.lands.1, "{c:?} pool {seed}: {} lands", b.lands);
            // Seeded: the same build again, whatever the pool's order.
            let mut rev = p.clone();
            rev.reverse();
            assert_eq!(builder::build(db(), &rev, &s, &mut Rng::new(seed)), b);
        }
    }
    assert!(unsupported < 60, "{unsupported} of 600 pools hold an unsupported card");
}

#[test]
fn the_builder_spread_over_1000_pools() {
    let s = Settings::default();
    for c in [Collation::Arena, Collation::Paper] {
        let (mut mono, mut pair, mut splash, mut skipped) = (0, 0, 0, 0);
        let mut lands = [0u32; 25];
        let mut pairs = std::collections::BTreeMap::new();
        let mut splash_spells = [0u32; 8];
        let mut mv = [0u32; 8];
        let mut creatures = 0u32;
        let mut nonbasic = 0u32;
        let mut tilt_hi = 0;
        let n = 1000u64;
        for seed in 0..n {
            let Some(p) = pool(c, 10_000 + seed) else {
                skipped += 1;
                continue;
            };
            let b = builder::build(db(), &p, &s, &mut Rng::new(seed ^ 0xABCD));
            check_build(&p, &b, &s, &format!("{c:?} pool {}", 10_000 + seed));
            if b.colors.count_ones() == 1 {
                mono += 1;
            } else {
                pair += 1;
            }
            if b.splash != 0 {
                splash += 1;
                splash_spells[b.splash_spells as usize] += 1;
            }
            *pairs.entry(builder::letters(b.colors)).or_insert(0u32) += 1;
            lands[b.lands as usize] += 1;
            for &id in &b.deck {
                let d = db().def(id);
                if d.is(types::LAND) {
                    if d.supertypes & engine::db::supertypes::BASIC == 0 {
                        nonbasic += 1;
                    }
                    continue;
                }
                mv[(d.mana_value() as usize).min(7)] += 1;
                creatures += d.is(types::CREATURE) as u32;
            }
            tilt_hi += (b.tilt_milli > 500) as u32;
        }
        let built = (n - skipped) as f64;
        println!(
            "{c:?}: {built} pools built ({skipped} hold an unsupported card): mono {mono}, two colours {pair}, splash {splash} (spells {:?}); lands {:?} (16..=18 = {:?}); \
             colours {pairs:?}; per deck: creatures {:.1}, nonbasic lands {:.2}, mana values 0..7+ {:?}; tilt > 0.5 in {tilt_hi}",
            &splash_spells[1..4],
            lands.iter().enumerate().filter(|x| *x.1 > 0).collect::<Vec<_>>(),
            &lands[16..=18],
            creatures as f64 / built,
            nonbasic as f64 / built,
            mv.iter().map(|&x| format!("{:.1}", x as f64 / built)).collect::<Vec<_>>()
        );
        assert!(mono > 0 && splash > 100 && pair > 600, "{c:?}: mono {mono}, splash {splash}, pairs {pair}");
        assert!(lands[16] > 200 && lands[17] > 200 && lands[18] > 200, "{c:?}: lands {lands:?}");
        assert!(pairs.len() >= 10, "{c:?}: colour pairs {pairs:?}");
    }
}

#[test]
fn the_builder_settings_steer_it() {
    let db = db();
    let p = pool(Collation::Arena, 7).expect("pool 7 is supported");
    // Temperature 0, no mono, no splash, no curve tilt: the best pair, deterministic in the pool.
    let greedy = Settings { temperature: 0.0, mono: 0.0, splash: 0.0, curve: 0.0, lands: (17, 17), ..Settings::default() };
    let cols: Vec<u8> = (0..20).map(|s| builder::build(db, &p, &greedy, &mut Rng::new(s)).colors).collect();
    assert!(cols.iter().all(|&c| c == cols[0]), "{cols:?}");
    assert!((0..20).all(|s| builder::build(db, &p, &greedy, &mut Rng::new(s)).lands == 17));
    // Splash 1: a splash whenever one is castable; the default sometimes.
    let always = Settings { splash: 1.0, mono: 0.0, ..Settings::default() };
    let splashed = (0..200).filter(|&s| builder::build(db, &p, &always, &mut Rng::new(s)).splash != 0).count();
    let sometimes = (0..200).filter(|&s| builder::build(db, &p, &Settings::default(), &mut Rng::new(s)).splash != 0).count();
    assert!(splashed > sometimes, "{splashed} vs {sometimes}");
    // The settings parse back from their JSON, and unknown keys fail.
    let s = Settings::parse(&serde_json::json!({"lands": [15, 19], "splash": 0.5, "temperature": 2})).unwrap();
    assert_eq!(Settings::parse(&s.to_json()).unwrap(), s);
    assert!(Settings::parse(&serde_json::json!({"land": [16, 18]})).is_err());
    assert!(Settings::parse(&serde_json::json!({"splash": 1.5})).is_err());
    assert!(Settings::parse(&serde_json::json!({"lands": [8, 18]})).is_err());
}

#[test]
fn fixers_are_read_from_what_the_card_does() {
    // (The builder's splash rule: mana abilities, library searches for lands, tokens that do
    // either. Examples only: the rule reads the IR, never a name.)
    let db = db();
    let f = |n: &str| builder::letters(builder::fixes(db, db.def(db.id(n).unwrap())));
    assert_eq!(f("Plains"), "W");
    assert_eq!(f("Breeding Pool"), "UG"); // a dual land's mana ability
    assert_eq!(f("Command Bridge"), "WUBRG"); // any colour
    assert_eq!(f("Starfield Shepherd"), "W"); // "a basic Plains card or ...": "and" narrows
    assert_eq!(f("Gene Pollinator"), "WUBRG"); // a mana ability of any colour
    assert_eq!(f("Nebula Dragon"), ""); // no mana, no search, no such token
}

#[test]
fn a_thin_pool_still_makes_40_cards() {
    // A pool of only a few spells: the best pair, every playable spell, basics make up the 40.
    let db = db();
    let p = pool(Collation::Arena, 3).expect("supported");
    let few: Vec<u16> = p.iter().copied().filter(|&id| !db.def(id).is(types::LAND)).take(12).collect();
    let b = builder::build(db, &few, &Settings::default(), &mut Rng::new(1));
    assert_eq!(b.deck.len(), 40);
    assert!(b.lands > 18);
}

const SEALED: &str = r#"{"sealed": {"set": "eoe", "collation": "arena"}}"#;

#[test]
fn a_sealed_spec_plays_the_builders_decks_reproducibly() {
    let sp = MtgSpec::parse(SEALED).unwrap();
    assert!(std::ptr::eq(sp.db(), CardDb::eoe()));
    let (mut games, mut failed) = (0, 0);
    for seed in 0..40u64 {
        match sp.try_seats(seed) {
            Ok(seats) => {
                games += 1;
                assert_eq!(sp.try_seats(seed).unwrap(), seats, "seed {seed}: not reproducible");
                for (p, s) in seats.iter().enumerate() {
                    assert_eq!(s.deck.len(), 40, "seed {seed} p{p}");
                    assert!(s.sideboard.len() >= 38, "seed {seed} p{p}: sideboard {}", s.sideboard.len());
                }
                assert_ne!(seats[0].deck, seats[1].deck);
                let g = MtgAz::new(&sp, seed).unwrap();
                assert_eq!(g.g.player(0).library.len() + g.g.player(0).hand.len(), 40);
            }
            Err(e) => {
                // (A loud failure, never a dropped card: on this base only The Dominion Bracelet.)
                assert!(e.contains("The Dominion Bracelet"), "seed {seed}: {e}");
                assert!(MtgAz::new_game(&sp, 2, seed).is_err());
                failed += 1;
            }
        }
    }
    assert!(games > 30, "{games} games, {failed} failed");
    // The seats are the documented ones: pool seed and builder generator per seat.
    let s = sp.sealed.as_deref().unwrap();
    let seed = (0..40).find(|&x| sp.try_seats(x).is_ok()).unwrap();
    let (seat, info) = s.seat(seed, 1).unwrap();
    assert_eq!(info.pool_seed, Some(sealed::pool_seed(seed, 1)));
    let want = sealed::generated_pool(db(), Collation::Arena, 6, sealed::pool_seed(seed, 1)).unwrap();
    assert_eq!(info.pool, want);
    let b = builder::build(db(), &want, &Settings::default(), &mut sealed::build_rng(seed, 1));
    assert_eq!((seat.deck, seat.sideboard), (b.deck, b.sideboard));
}

#[test]
fn sealed_evaluation_pairs_are_fixed() {
    let sp = MtgSpec::parse(&MtgSpec::eval_json(SEALED).unwrap()).unwrap();
    let sp2 = MtgSpec::parse(r#"{"sealed": {"set": "eoe", "collation": "arena"}, "decks": "eval", "eval_decks": 4, "eval_seed": 9}"#).unwrap();
    let mut distinct = std::collections::BTreeSet::new();
    for seed in 0..64u64 {
        let i = (zero_search::rng::split64(seed) % 4) as u32;
        let a = sp2.try_seats(seed);
        let b = sp2.try_seats(seed ^ 0x55);
        if (zero_search::rng::split64(seed ^ 0x55) % 4) as u32 == i {
            assert_eq!(a, b);
        }
        if let Ok(x) = a {
            distinct.insert(x[0].deck.clone());
        }
    }
    assert!(distinct.len() <= 4);
    assert!(sp.try_seats(1).is_ok() || sp.try_seats(2).is_ok());
}

#[test]
fn given_pools_and_builds_play_as_given() {
    let db = db();
    let p = pool(Collation::Paper, 11).unwrap();
    let names: Vec<String> = p.iter().map(|&id| db.def(id).name.clone()).collect();
    let b = builder::build(db, &p, &Settings::default(), &mut Rng::new(5));
    let deck: Vec<String> = b.deck.iter().map(|&id| db.def(id).name.clone()).collect();
    let side: Vec<String> = b.sideboard.iter().map(|&id| db.def(id).name.clone()).collect();
    let spec = serde_json::json!({"sealed": {"set": "eoe", "collation": "paper",
        "p0": {"deck": deck, "sideboard": side}, "p1": {"pool": names}}});
    let sp = MtgSpec::parse(&spec.to_string()).unwrap();
    for seed in 0..10u64 {
        let Ok(seats) = sp.try_seats(seed) else { continue };
        assert_eq!((&seats[0].deck, &seats[0].sideboard), (&b.deck, &b.sideboard), "p0 plays the given build");
        let mut pool1: Vec<u16> = seats[1].deck.iter().copied().filter(|d| !engine::az::pools::basic_ids(db).contains(d)).chain(seats[1].sideboard.iter().copied()).collect();
        pool1.sort_unstable();
        assert_eq!(pool1, p, "p1 is built from the given pool");
    }
    // Without a generated seat no collation is needed; the spec reads back from its JSON.
    let both = serde_json::json!({"sealed": {"set": "eoe", "p0": {"pool": names.clone()}, "p1": {"pool": names}}});
    let sp = MtgSpec::parse(&both.to_string()).unwrap();
    let s = sp.sealed.as_deref().unwrap();
    assert_eq!(&SealedSpec::parse(&s.to_json()).unwrap(), s);
    assert!(MtgAz::new(&sp, 3).is_ok());
}

#[test]
fn sealed_spec_errors_are_loud() {
    let bad = [
        r#"{"sealed": {"collation": "arena"}}"#,
        r#"{"sealed": {"set": "dsk", "collation": "arena"}}"#,
        r#"{"sealed": {"set": "eoe"}}"#,
        r#"{"sealed": {"set": "eoe", "collation": "mtgo"}}"#,
        r#"{"sealed": {"set": "eoe", "collation": "arena", "flavour": 1}}"#,
        r#"{"sealed": {"set": "eoe", "collation": "arena"}, "format": "bo3"}"#,
        r#"{"sealed": {"set": "eoe", "collation": "arena"}, "constructed": true}"#,
        r#"{"sealed": {"set": "eoe", "collation": "arena"}, "pools": true}"#,
        r#"{"sealed": {"set": "eoe", "collation": "arena"}, "lands": [16, 17]}"#,
        r#"{"sealed": {"set": "eoe", "collation": "arena"}, "decks": {"p0": ["Plains"], "p1": ["Plains"]}}"#,
        r#"{"sealed": {"set": "eoe", "collation": "arena", "p0": {"pool": ["Lightning Bolt"]}}}"#,
        r#"{"sealed": {"set": "eoe", "collation": "arena", "p0": {"pool": ["Plains"]}}}"#,
        r#"{"sealed": {"set": "eoe", "collation": "arena", "p0": {"pool": ["Paradox Haze"]}}}"#,
        r#"{"sealed": {"set": "eoe", "collation": "arena", "p0": {"deck": ["Plains"]}}}"#,
        r#"{"sealed": {"set": "eoe", "collation": "arena", "builder": {"lands": [3, 4]}}}"#,
    ];
    for b in bad {
        assert!(MtgSpec::parse(b).is_err(), "{b} parsed");
    }
    // A card the table lacks says which (since the merge into eoe-hidden-decks The Dominion
    // Bracelet is supported, D2's ENGINE.md 100.5; Paradox Haze has no IR, excluded by the user).
    let e = MtgSpec::parse(bad[12]).unwrap_err();
    assert!(e.contains("Paradox Haze"), "{e}");
}

#[test]
fn specs_without_the_key_are_unchanged() {
    for spec in ["{}", r#"{"format": "bo3"}"#, r#"{"decks": "eval"}"#] {
        let sp = MtgSpec::parse(spec).unwrap();
        assert!(sp.sealed.is_none());
        assert!(std::ptr::eq(sp.db(), CardDb::cube()));
        assert_eq!(sp.try_seats(5).unwrap(), sp.seats(5));
    }
}

/// Sealed games through `MtgAz` (the `SearchGame` zero-core plays): random actions, a
/// `determinize` and both encodings at times, to the end.
#[test]
fn sealed_games_play_to_the_end_through_mtgaz() {
    let sp = MtgSpec::parse(r#"{"sealed": {"set": "eoe", "collation": "paper"}, "max_turns": 30}"#).unwrap();
    let mut rng = Rng::new(3);
    let mut played = 0;
    let mut buf = vec![0f32; MtgAz::NUM_FEATURES];
    for seed in 0..12u64 {
        let Ok(mut g) = MtgAz::new_game(&sp, 2, seed) else { continue };
        let mut legal = Vec::new();
        let mut keys = Vec::new();
        let mut steps = 0;
        while g.to_move().is_some() {
            g.legal_actions(&mut legal);
            assert!(!legal.is_empty());
            if steps % 50 == 0 {
                let p = g.to_move().unwrap();
                let mut d = g.clone();
                let mut zr = zero_search::rng::Rng::new(seed ^ steps);
                d.determinize(p, &mut zr);
                let mut l2 = Vec::new();
                d.legal_actions(&mut l2);
                assert_eq!(legal, l2, "seed {seed}: determinize changed the decider's actions");
                for v in 0..2 {
                    buf.iter_mut().for_each(|x| *x = 0.0);
                    g.encode(v, &mut buf);
                    assert!(buf.iter().all(|x| x.is_finite()));
                }
            }
            let a = legal[rng.below(legal.len() as u32) as usize];
            g.step(a, &mut keys).unwrap();
            steps += 1;
            assert!(steps < 100_000);
        }
        let r = g.result();
        assert!((r[0] + r[1] - 1.0).abs() < 1e-6 || g.aborted());
        played += 1;
    }
    assert!(played >= 8, "{played} games");
}
