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

/// Seat `p`'s hand then library (definitions) after the setup of the game with this spec and seed:
/// the chooser takes the first turn, then the setup shuffle (CR 103.3) and the opening hands
/// (103.5) up to the first mulligan decision (ENGINE.md 104).
fn after_setup(sp: &MtgSpec, seed: u64, p: u8) -> (Vec<u16>, u8) {
    let mut g = MtgAz::new(sp, seed).unwrap();
    let (mut legal, mut keys) = (Vec::new(), Vec::new());
    // The chooser's play-or-draw, then (with hidden decklists, which "sealed" turns on, ENGINE.md
    // 102) each player's companion reveal, a single option here; then the hands are dealt.
    for _ in 0..4 {
        if g.g.player(p).hand.len() == 7 {
            break;
        }
        legal.clear();
        g.legal_actions(&mut legal);
        g.step(legal[0], &mut keys).unwrap();
    }
    assert_eq!(g.g.player(p).hand.len(), 7, "seed {seed}: the opening hands are drawn");
    let pl = g.g.player(p);
    (pl.hand.iter().chain(pl.library.iter()).map(|&c| g.g.obj(c).def).collect(), g.g.turn().starting)
}

fn names_of(ids: &[u16]) -> Vec<String> {
    ids.iter().map(|&id| db().def(id).name.clone()).collect()
}

/// A seat's given deck in a given order (ENGINE.md 104): `"order": "given"` makes the list's order
/// the library's before the setup shuffle; `"sorted"` (the default) sorts it by definition.
fn seat_json(deck: &[u16], order: Option<&str>) -> serde_json::Value {
    let mut v = serde_json::json!({"deck": names_of(deck)});
    if let Some(o) = order {
        v["order"] = serde_json::json!(o);
    }
    v
}

#[test]
fn a_given_order_is_the_library_before_the_setup_shuffle() {
    let db = db();
    let a = builder::build(db, &pool(Collation::Arena, 21).unwrap(), &Settings::default(), &mut Rng::new(1));
    let opp = builder::build(db, &pool(Collation::Arena, 22).unwrap(), &Settings::default(), &mut Rng::new(2));
    let mut rev = a.deck.clone();
    rev.reverse();
    let spec = |hero: serde_json::Value| {
        let s = serde_json::json!({"sealed": {"set": "eoe", "p0": hero, "p1": {"deck": names_of(&opp.deck)}}});
        MtgSpec::parse(&s.to_string()).unwrap()
    };
    let plain = spec(seat_json(&a.deck, None));
    // the default sorts: any given order is the sorted deck's game, and "sorted" says so explicitly
    assert_eq!(spec(seat_json(&rev, None)), plain);
    assert_eq!(spec(seat_json(&rev, Some("sorted"))), plain);
    // the sorted order given is the default's game; the reversed order given is another game of the
    // same cards (the shuffle maps slots, not cards), the other seat's untouched
    let sorted_given = spec(seat_json(&a.deck, Some("given")));
    let rev_given = spec(seat_json(&rev, Some("given")));
    let mut differ = 0;
    for seed in 0..16u64 {
        let base = after_setup(&plain, seed, 0);
        assert_eq!(after_setup(&sorted_given, seed, 0), base, "seed {seed}");
        let r = after_setup(&rev_given, seed, 0);
        let (mut x, mut y) = (r.0.clone(), base.0.clone());
        x.sort_unstable();
        y.sort_unstable();
        assert_eq!(x, y, "seed {seed}: the same cards");
        differ += (r.0 != base.0) as u32;
        assert_eq!(after_setup(&rev_given, seed, 1), after_setup(&plain, seed, 1), "seed {seed}: p1's setup");
    }
    assert!(differ >= 15, "{differ} of 16 setups differ");
    let s = rev_given.sealed.as_deref().unwrap();
    assert_eq!(&SealedSpec::parse(&s.to_json()).unwrap(), s, "the order reads back from the JSON");
}

/// The paired deck comparison's common random numbers (ENGINE.md 104): two 40-card decks laid out
/// slot by slot, differing in one slot, get the same permutation of their slots from one seed: the
/// shuffled libraries and opening hands agree everywhere but where that slot went, the other
/// player's deck is dealt the same and the same player takes the first turn. Sorted by definition
/// (the default), the cards between the two swapped definitions shift a slot and land elsewhere.
#[test]
fn decks_laid_out_slot_by_slot_share_the_setup_shuffle() {
    let db = db();
    let a = builder::build(db, &pool(Collation::Arena, 21).unwrap(), &Settings::default(), &mut Rng::new(1));
    let opp = builder::build(db, &pool(Collation::Arena, 22).unwrap(), &Settings::default(), &mut Rng::new(2));
    // swap the deck's lowest definition for the sideboard card with the most deck cards below it
    let x = a.deck[0];
    let y = *a.sideboard.iter().max_by_key(|&&d| (a.deck.iter().filter(|&&e| e < d).count(), d)).unwrap();
    assert!(a.deck.iter().filter(|&&e| e > x && e < y).count() >= 5, "a swap across the deck");
    let mut b = a.deck.clone();
    b[0] = y;
    for hero in 0..2u8 {
        let spec = |deck: &[u16], order: Option<&str>| {
            let mut sealed = serde_json::json!({"set": "eoe"});
            sealed[format!("p{hero}")] = seat_json(deck, order);
            sealed[format!("p{}", 1 - hero)] = serde_json::json!({"deck": names_of(&opp.deck)});
            MtgSpec::parse(&serde_json::json!({"sealed": sealed}).to_string()).unwrap()
        };
        let (sa, sb) = (spec(&a.deck, Some("given")), spec(&b, Some("given")));
        let (ua, ub) = (spec(&a.deck, None), spec(&b, None));
        let mut sorted_moved = 0;
        for seed in 0..24u64 {
            let (la, start_a) = after_setup(&sa, seed, hero);
            let (lb, start_b) = after_setup(&sb, seed, hero);
            assert_eq!(start_a, start_b, "seed {seed}: the same player takes the first turn");
            let diff: Vec<usize> = (0..40).filter(|&i| la[i] != lb[i]).collect();
            assert_eq!(diff.len(), 1, "seed {seed} hero p{hero}: one position differs");
            assert_eq!((la[diff[0]], lb[diff[0]]), (x, y), "seed {seed}: the swapped slot");
            assert_eq!(after_setup(&sa, seed, 1 - hero), after_setup(&sb, seed, 1 - hero), "seed {seed}: the opponent's setup");
            let (ma, _) = after_setup(&ua, seed, hero);
            let (mb, _) = after_setup(&ub, seed, hero);
            sorted_moved += (0..40).filter(|&i| ma[i] != mb[i]).count();
        }
        assert!(sorted_moved > 3 * 24, "sorted decks: {sorted_moved} positions differ over 24 setups");
    }
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
        // (ENGINE.md 104: a given deck's order, "given" or "sorted"; a pool has none)
        r#"{"sealed": {"set": "eoe", "collation": "arena", "p0": {"pool": ["Mechan Shieldmate"], "order": "given"}}}"#,
        r#"{"sealed": {"set": "eoe", "collation": "arena", "p0": {"deck": ["Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains"], "order": "random"}}}"#,
    ];
    for b in bad {
        assert!(MtgSpec::parse(b).is_err(), "{b} parsed");
    }
    // A card the table lacks says which (since the merge into eoe-hidden-decks The Dominion
    // Bracelet is supported, D2's ENGINE.md 100.5; Paradox Haze has no IR, excluded by the user).
    let e = MtgSpec::parse(bad[12]).unwrap_err();
    assert!(e.contains("Paradox Haze"), "{e}");
    for b in &bad[15..] {
        let e = MtgSpec::parse(b).unwrap_err();
        assert!(e.contains("order"), "{e}");
    }
    // the same 40 cards with "order": "sorted" parse (the default, explicitly)
    assert!(MtgSpec::parse(&bad[16].replace("\"random\"", "\"sorted\"")).is_ok());
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
                    // (layout v7: the whole row, ENGINE.md 125)
                    let row = engine::az::tail_encode::Row::of_az(&g, v);
                    assert!(row.head.iter().chain(row.tail.iter().flat_map(|t| t.2.iter())).all(|x| x.is_finite()));
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

// ---- The mixed field (ENGINE.md 107.1) ----

/// Without `best_share` the builder, the spec's seats and the sealed prior draw exactly as before the
/// mixed field: a hash of 400 builds (two settings), 200 spec seats (self-play and evaluation) and 150
/// prior samples (fresh pools and a bank's), recorded on 72a44fb, before the change.
#[test]
fn without_best_share_the_builder_seats_and_prior_draw_as_before() {
    use engine::az::hidden_decks::{DeckPrior, DeckWant, SealedPrior};
    let db = db();
    let mut h = 0u64;
    let mut mix = |x: u64| h = engine::rng::split64(h ^ x);
    let other = Settings::parse(&serde_json::json!({"splash": 1.0, "mono": 0.5, "temperature": 1.0, "lands": [15, 19]})).unwrap();
    for c in [Collation::Arena, Collation::Paper] {
        for seed in 0..100u64 {
            let p = pool(c, seed).unwrap();
            for s in [Settings::default(), other] {
                let b = builder::build(db, &p, &s, &mut Rng::new(seed));
                for &d in b.deck.iter().chain(&b.sideboard) {
                    mix(d as u64);
                }
                mix(b.colors as u64 | (b.splash as u64) << 8 | (b.lands as u64) << 16 | (b.splash_spells as u64) << 24 | (b.tilt_milli as u32 as u64) << 32);
            }
        }
    }
    for spec in [r#"{"sealed": {"set": "eoe", "collation": "arena"}}"#, r#"{"sealed": {"set": "eoe", "collation": "paper"}, "decks": "eval", "eval_decks": 8}"#] {
        let sp = MtgSpec::parse(spec).unwrap();
        for seed in 0..50u64 {
            for s in sp.try_seats(seed).unwrap() {
                for &d in s.deck.iter().chain(&s.sideboard) {
                    mix(d as u64);
                }
                mix(0xFFFF);
            }
        }
    }
    let spec = SealedSpec::parse(&serde_json::json!({"set": "eoe", "collation": "arena"})).unwrap();
    let prior = SealedPrior::of_spec(&spec);
    let bank = prior.bank(4, &mut Rng::new(7));
    for seed in 0..75u64 {
        let real = spec.seat(seed, 1).unwrap().0.deck;
        let k = (seed % 13) as usize;
        let mut shown: Vec<u16> = real.iter().copied().step_by(3).take(k).collect();
        shown.sort_unstable();
        let want = DeckWant { db, n: 40, shown: &shown, companion: None, tilt: None, pick: None };
        let a = prior.sample(&want, &mut Rng::new(seed));
        let b = prior.sample_from(&want, Some(&bank[(seed % 4) as usize]), &mut Rng::new(seed));
        for s in [a, b] {
            for &d in s.deck.iter().chain(&s.sideboard) {
                mix(d as u64);
            }
            mix(0xFFFE);
        }
    }
    println!("fingerprint {h:016x}");
    assert_eq!(format!("{h:016x}"), "5e986ac22869924f", "the draws without best_share changed");
}

/// The best guess (ENGINE.md 107.1) is the builder with every random choice at its most likely value:
/// 40 cards from the pool, 17 lands, the pair the builder takes at temperature 0 (no mono, no splash, no
/// tilt), the spells greedy by weight (no playable spell left out is heavier than one played, but for the
/// mana-value caps; on a tie the mana value nearest 3), no colourless-only land, and no randomness: the
/// same deck for any order of the pool.
#[test]
fn the_best_guess_is_the_builders_most_likely_build() {
    let db = db();
    let s = Settings::default();
    let modal = Settings::parse(&serde_json::json!({"temperature": 0.0, "mono": 0.0, "splash": 0.0, "curve": 0.0, "lands": [17, 17]})).unwrap();
    let mut pairs = std::collections::BTreeMap::new();
    let (mut creatures, mut decks, mut capped) = (0usize, 0usize, 0usize);
    for c in [Collation::Arena, Collation::Paper] {
        for seed in 0..200u64 {
            let p = pool(c, 20_000 + seed).unwrap();
            let what = format!("{c:?} pool {}", 20_000 + seed);
            let b = builder::best_guess(db, &p, &s);
            check_build(&p, &b, &s, &what);
            assert_eq!((b.lands, b.splash, b.splash_spells, b.tilt_milli), (17, 0, 0, 0), "{what}");
            assert_eq!(b.colors.count_ones(), 2, "{what}: a pair");
            let mut rev = p.clone();
            rev.reverse();
            assert_eq!(builder::best_guess(db, &rev, &s), b, "{what}: the pool's order");
            assert_eq!(builder::best_guess_covering(db, &p, &s, None, 0), Some(b.clone()), "{what}");
            // the pair: the builder's at temperature 0 (its spells are drawn, the pair is not)
            for r in 0..3 {
                assert_eq!(builder::build(db, &p, &modal, &mut Rng::new(r)).colors, b.colors, "{what}: the temperature-0 pair");
            }
            // greedy: a playable spell left out is no heavier than every spell played (the caps aside)
            let spells: Vec<u16> = b.deck.iter().copied().filter(|&d| !db.def(d).is(types::LAND)).collect();
            let lightest = spells.iter().map(|&d| builder::spell_weight(db.def(d))).fold(f64::MAX, f64::min);
            let mv = |d: u16| db.def(d).mana_value() as i64;
            let at_cap = spells.iter().filter(|&&d| mv(d) >= 5).count() >= s.max_mv5 as usize
                || spells.iter().filter(|&&d| mv(d) == 4).count() >= s.max_mv4 as usize;
            for &d in b.sideboard.iter().filter(|&&d| builder::playable(db.def(d), b.colors)) {
                let w = builder::spell_weight(db.def(d));
                if w > lightest {
                    assert!(at_cap && mv(d) >= 4, "{what}: {} (weight {w}) left out, a spell of weight {lightest} played", db.def(d).name);
                    capped += 1;
                }
                // on a tie of weight, the mana value nearest 3 first
                for &x in spells.iter().filter(|&&x| builder::spell_weight(db.def(x)) == w) {
                    assert!(at_cap || (mv(x) - 3).abs() <= (mv(d) - 3).abs(), "{what}: {} played before {}", db.def(x).name, db.def(d).name);
                }
            }
            // no colourless-only land: every nonbasic land makes or fetches a colour of the pair
            for &d in b.deck.iter().filter(|&&d| db.def(d).is(types::LAND) && db.def(d).supertypes & engine::db::supertypes::BASIC == 0) {
                assert!(builder::fixes(db, db.def(d)) & b.colors != 0, "{what}: {} makes none of the pair's colours", db.def(d).name);
            }
            *pairs.entry(builder::letters(b.colors)).or_insert(0u32) += 1;
            creatures += spells.iter().filter(|&&d| db.def(d).is(types::CREATURE)).count();
            decks += 1;
        }
    }
    println!(
        "400 best guesses: pairs {pairs:?}; {:.1} creatures a deck; {capped} heavier spells left out by the caps",
        creatures as f64 / decks as f64
    );
    assert!(pairs.len() >= 8, "the best guesses' pairs {pairs:?}");
    // a pair holding a colour: the strongest such pair; three colours: none (no splash)
    let p = pool(Collation::Arena, 20_000).unwrap();
    let b = builder::best_guess(db, &p, &s);
    let other = (0..5).map(|i| 1u8 << i).find(|c| b.colors & c == 0).unwrap();
    let held = builder::best_guess_covering(db, &p, &s, None, other).unwrap();
    assert!(held.colors & other != 0 && held.colors.count_ones() == 2);
    assert_eq!(builder::best_guess_covering(db, &p, &s, None, b.colors | other), None);
}

/// The mixed field (ENGINE.md 107.1): `best_share` p makes each built seat's deck the best guess with
/// probability p, independently per seat, from the game's seed; the randomised builds and the pools are
/// the draws without the key; 0 is no key; evaluation pairs keep their modes; given builds are `given`.
#[test]
fn best_share_mixes_the_field() {
    let db = db();
    let parse = |share: &str| MtgSpec::parse(&format!(r#"{{"sealed": {{"set": "eoe", "collation": "arena"{share}}}}}"#)).unwrap();
    let plain = parse("");
    let half = parse(r#", "best_share": 0.5"#);
    let zero = parse(r#", "best_share": 0"#);
    let all = parse(r#", "best_share": 1"#);
    assert_eq!(zero, plain, "0 is no key");
    let sl = half.sealed.as_deref().unwrap();
    assert_eq!(SealedSpec::parse(&sl.to_json()).unwrap(), *sl, "the JSON round trip");
    assert!(plain.sealed.as_deref().unwrap().to_json().get("best_share").is_none());
    for bad in ["-0.1", "1.5", r#""half""#] {
        let e = MtgSpec::parse(&format!(r#"{{"sealed": {{"set": "eoe", "collation": "arena", "best_share": {bad}}}}}"#)).unwrap_err();
        assert!(e.contains("best_share"), "{e}");
    }
    let (mut best, mut both) = (0u32, 0u32);
    let n = 1000u64;
    for seed in 0..n {
        let mut modes = [sealed::Mode::Given; 2];
        for p in 0..2u8 {
            let (s0, i0) = plain.sealed.as_deref().unwrap().seat(seed, p).unwrap();
            let (s1, i1) = sl.seat(seed, p).unwrap();
            assert_eq!(i0.mode, sealed::Mode::Explore);
            assert_eq!((i1.pool_seed, &i1.pool), (i0.pool_seed, &i0.pool), "seed {seed}: the same pool");
            assert_eq!(i1.mode == sealed::Mode::Best, sealed::best_seat(seed, p, 0.5));
            match i1.mode {
                sealed::Mode::Explore => assert_eq!(s1, s0, "seed {seed}: the randomised build unchanged"),
                sealed::Mode::Best => {
                    assert_eq!(i1.build.as_ref(), Some(&builder::best_guess(db, &i1.pool, &sl.builder)));
                    assert_eq!(s1.deck, i1.build.as_ref().unwrap().deck);
                    best += 1;
                }
                sealed::Mode::Given | sealed::Mode::Chosen => unreachable!(),
            }
            modes[p as usize] = i1.mode;
            if seed < 50 {
                assert_eq!(all.sealed.as_deref().unwrap().seat(seed, p).unwrap().1.mode, sealed::Mode::Best);
            }
        }
        both += (modes == [sealed::Mode::Best; 2]) as u32;
        // the game's decks are the seats'
        if seed < 20 {
            let seats = half.try_seats(seed).unwrap();
            assert_eq!(seats[0].deck, sl.seat(seed, 0).unwrap().0.deck);
        }
    }
    println!("best_share 0.5: {best} best guesses of {} seats, both seats in {both} of {n} games", 2 * n);
    assert!((900..=1100).contains(&best) && (180..=320).contains(&both), "{best} {both}");
    // evaluation pairs: a pair's seed, so its modes and decks are fixed
    let ev = MtgSpec::parse(r#"{"sealed": {"set": "eoe", "collation": "arena", "best_share": 0.5}, "decks": "eval", "eval_decks": 4}"#).unwrap();
    for seed in 0..40u64 {
        let i = (zero_search::rng::split64(seed) % 4) as u32;
        assert_eq!(ev.try_seats(seed).unwrap(), half.try_seats(sealed::eval_game_seed(1, i)).unwrap());
    }
    // a given build is played as given
    let given = sl.seat(3, 0).unwrap().0.deck;
    let names: Vec<String> = given.iter().map(|&d| db.def(d).name.clone()).collect();
    let gs = SealedSpec::parse(&serde_json::json!({"set": "eoe", "collation": "arena", "best_share": 1.0, "p0": {"deck": names}})).unwrap();
    let (s, i) = gs.seat(0, 0).unwrap();
    assert_eq!((s.deck, i.mode), (given, sealed::Mode::Given));
    assert_eq!(gs.seat(0, 1).unwrap().1.mode, sealed::Mode::Best);
    // the seats' JSON names the mode
    let j: serde_json::Value = serde_json::from_str(&sealed::seats_json(r#"{"sealed": {"set": "eoe", "collation": "arena", "best_share": 1}}"#, 5).unwrap()).unwrap();
    assert_eq!(j[0]["mode"], "best");
}
