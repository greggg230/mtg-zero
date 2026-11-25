//! Constructed best-of-three with a registered main of at least 60 (ENGINE.md 272; the user's decision of 2026-11-24):
//! CR 100.2a's minimum, no maximum but the engine's 64 (`constructed::MAX_DECK`), a sideboard of at most 15 (100.4a).
//! Sideboarding stays one-for-one swaps, so a seat's deck is its registered main's size N in every game of the match
//! (the format restriction of 2026-11-11, "exactly 60", became "the registered size"). Checked on the Legacy Lands 76
//! (61 main, 15 sideboard: `Decklist::legacy_lands`, ENGINE.md 270) against the Delver 75: the spec (a 59 and a
//! 16-card sideboard still refused), sideboarding (every legal 61 reachable by its canonical path, a 60 or 62 by none,
//! `PASS` only at 61; every legal deck of a small pool with a main above the minimum reached exactly once), the
//! constructed prior of the hidden 61 (its contract at n = 61), whole matches (each game's decks, sideboards and
//! `KEY_GAME`, the later games' worlds dealing 61s of the public 76 with the viewer's encoding kept), choice tables of
//! 61s (followed; a table of another size refused), dealt decks of 61 (`dealt_decks`, `"around": "table"`), and a
//! best-of-three metagame with the Lands deck.

use std::collections::BTreeSet;

use engine::az::bots::Heuristic;
use engine::az::constructed::{self, Decklist};
use engine::az::hidden_decks::{check_sample, ConstructedPrior, DeckPrior, DeckWant};
use engine::az::matches::{MatchPool, Sideboarding};
use engine::az::sbtable::context_index;
use engine::az::{DeckMode, MtgAz, MtgSpec};
use engine::db::CardDb;
use engine::decision::act;
use engine::hidden::{key_kind, key_payload, KEY_GAME};
use engine::rng::Rng;
use engine::DecisionKind;
use serde_json::{json, Value};
use zero_search::game::{Next, SearchGame};

fn db() -> &'static CardDb {
    CardDb::constructed()
}

fn lands() -> Decklist {
    Decklist::legacy_lands()
}

fn delver() -> Decklist {
    Decklist::izzet_delver()
}

fn side_names(d: &Decklist) -> Vec<String> {
    d.sideboard.iter().flat_map(|(k, n)| std::iter::repeat_n(n.clone(), *k as usize)).collect()
}

fn ids(d: &Decklist) -> (Vec<u16>, Vec<u16>) {
    (Decklist::ids(db(), &d.main).unwrap(), Decklist::ids(db(), &d.sideboard).unwrap())
}

/// The fixed two-75 constructed best-of-three of `a` (seat 0) and `b` (seat 1), the keys of `extra` merged in
/// (`decks.*` keys under `decks`).
fn fixed(a: &Decklist, b: &Decklist, extra: Value) -> Value {
    let mut v = json!({"max_turns": 60, "constructed": true, "format": "bo3",
        "decks": {"p0": a.main_names(), "p1": b.main_names(), "p0_sideboard": side_names(a), "p1_sideboard": side_names(b)}});
    for (k, x) in extra.as_object().unwrap() {
        if let Some(dk) = k.strip_prefix("decks.") {
            v["decks"][dk] = x.clone();
        } else {
            v[k] = x.clone();
        }
    }
    v
}

fn parse(v: &Value) -> Result<MtgSpec, String> {
    MtgSpec::parse(&v.to_string())
}

fn refused(v: &Value, want: &str) {
    match parse(v) {
        Ok(_) => panic!("accepted: {v}"),
        Err(e) => assert!(e.contains(want), "refusal {e:?} lacks {want:?}"),
    }
}

fn legal(g: &MtgAz) -> Vec<u16> {
    let mut v = Vec::new();
    g.legal_actions(&mut v);
    v
}

fn enc(g: &MtgAz, viewer: usize) -> Vec<f32> {
    engine::az::tail_encode::Row::of_az(g, viewer).flat()
}

fn size(counts: &[u8]) -> usize {
    counts.iter().map(|&c| c as usize).sum()
}

/// Does sorted multiset `a` hold sorted multiset `b`?
fn holds(a: &[u16], b: &[u16]) -> bool {
    let mut left = a.to_vec();
    b.iter().all(|x| match left.binary_search(x) {
        Ok(i) => {
            left.remove(i);
            true
        }
        Err(_) => false,
    })
}

/// Player `p`'s cards of the current game's starting deck (sorted definitions).
fn game_deck(a: &MtgAz, p: u8) -> Vec<u16> {
    let n0 = size(&a.m.decks[0]);
    let n1 = size(&a.m.decks[1]);
    let r = if p == 0 { 0..n0 } else { n0..n0 + n1 };
    let mut v: Vec<u16> = r.map(|i| engine::az::sample::card_of(&a.g, engine::CardIdx::new(i as u32))).collect();
    v.sort_unstable();
    v
}

/// The cards `viewer` has seen of the opponent's current deck (cards it knows exactly, its clouds' identities).
fn shown(a: &MtgAz, viewer: u8) -> Vec<u16> {
    let o = 1 - viewer;
    let g = &a.g;
    let n0 = size(&a.m.decks[0]);
    let n1 = size(&a.m.decks[1]);
    let r = if o == 0 { 0..n0 } else { n0..n0 + n1 };
    let mut v: Vec<u16> =
        r.map(|i| engine::CardIdx::new(i as u32)).filter(|&c| g.knows(viewer, c)).map(|c| engine::az::sample::card_of(g, c)).collect();
    for k in g.clouds().iter().filter(|k| k.viewer == viewer && k.owner == o) {
        v.extend(k.defs.iter().map(|&d| g.db().def(d).card_def(d)));
    }
    v.sort_unstable();
    v
}

#[test]
fn cr_100_2a_a_best_of_three_main_of_at_least_60_is_played_and_under_60_or_a_sideboard_over_15_refused() {
    let (l, d) = (lands(), delver());
    assert_eq!((l.main_names().len(), side_names(&l).len()), (61, 15));
    // The Lands 76 in either seat, and its mirror: parsed, its pool's decks 61.
    for (a, b) in [(&l, &d), (&d, &l), (&l, &l)] {
        let sp = parse(&fixed(a, b, json!({}))).unwrap();
        match &sp.decks {
            DeckMode::ListsWithSideboards(p) => {
                assert_eq!((p[0].0.len(), p[1].0.len()), (a.main_names().len(), b.main_names().len()));
                assert_eq!((p[0].1.len(), p[1].1.len()), (15, 15));
            }
            other => panic!("{other:?}"),
        }
        let g = MtgAz::new_game(&sp, 2, 5).unwrap();
        for (p, x) in [a, b].into_iter().enumerate() {
            let n = x.main_names().len();
            let pool = &g.m.pools[p];
            assert_eq!((pool.min_deck, pool.max_deck, g.m.deck(p as u8).len(), g.g.sideboard_size(p as u8)), (n, n, n, 15));
            assert_eq!(pool.list.iter().map(|e| e.1 as usize).sum::<usize>(), n + 15, "the pool is the whole registered list");
        }
    }
    // The evaluation spec of a decklist spec is the spec.
    let v = fixed(&l, &d, json!({}));
    assert_eq!(MtgSpec::parse(&MtgSpec::eval_json(&v.to_string()).unwrap()).unwrap(), parse(&v).unwrap());
    // Without a sideboard (CR 100.4a: up to 15): the 61 can only be kept.
    parse(&json!({"constructed": true, "format": "bo3", "decks": {"p0": l.main_names(), "p1": d.main_names()}})).unwrap();
    // Up to the engine's 64 (a basic land more each time); 65 refused (the action slots, ENGINE.md 54.4).
    let mut big = d.main_names();
    while big.len() < constructed::MAX_DECK {
        big.push("Island".to_string());
    }
    parse(&json!({"constructed": true, "format": "bo3", "decks": {"p0": big, "p1": d.main_names()}})).unwrap();
    big.push("Island".to_string());
    refused(&json!({"constructed": true, "format": "bo3", "decks": {"p0": big, "p1": d.main_names()}}), "65 cards in the deck");
    // A main under 60 (CR 100.2a) and a sideboard over 15 (100.4a) are refused in best-of-three as in best-of-one.
    let mut short = l.main_names();
    short.truncate(59);
    refused(&fixed(&l, &d, json!({"decks.p1": short})), "p1's constructed deck: 59 cards in the deck (CR 100.2a: at least 60)");
    let mut sb16 = side_names(&l);
    sb16.push("Dismember".to_string());
    refused(&fixed(&l, &d, json!({"decks.p0_sideboard": sb16})), "16 cards in the sideboard (CR 100.4a: at most 15)");
    // The four-copy limit over main and sideboard together still holds (100.4a): a third Disruptor Flute in the side.
    let mut flutes = side_names(&l);
    flutes[0] = "Disruptor Flute".to_string();
    flutes[1] = "Disruptor Flute".to_string();
    refused(&fixed(&l, &d, json!({"decks.p0_sideboard": flutes})), "5 copies of Disruptor Flute");
}

/// Sideboard `pool` from `start` to `target` by the canonical path (ENGINE.md 36.2): `Err` at the first option not
/// offered.
fn sideboard_to(pool: &MatchPool, start: &[u8], target: &[u8]) -> Result<Vec<u8>, String> {
    let mut sb = Sideboarding::new(0, start.to_vec(), pool);
    let offered = |sb: &Sideboarding, a: u16| sb.decision.options.iter().any(|o| o.action == a);
    for k in 0..pool.list.len() {
        if target[k] == start[k] {
            continue;
        }
        let a = act::LIBRARY + k as u16;
        if !offered(&sb, a) {
            return Err(format!("entry {k} not offered"));
        }
        assert!(!sb.choose(pool, a).unwrap());
        if sb.decision.kind == DecisionKind::SideboardCount {
            let a = act::NUMBER + target[k] as u16;
            if !offered(&sb, a) {
                return Err(format!("entry {k}: count {} not offered", target[k]));
            }
            assert!(!sb.choose(pool, a).unwrap());
        }
    }
    if !offered(&sb, act::PASS) {
        return Err("PASS not offered".into());
    }
    assert!(sb.choose(pool, act::PASS).unwrap());
    Ok(sb.counts)
}

#[test]
fn the_lands_76_is_sideboarded_to_exactly_61_cards() {
    let (main, side) = ids(&lands());
    let (pool, start) = MatchPool::constructed(&main, &side);
    assert_eq!((pool.n_cards, pool.min_deck, pool.max_deck), (pool.list.len(), 61, 61));
    assert!(pool.list.len() <= 64 && pool.list.iter().all(|e| e.1 <= 31), "the list block's 64 entries, NUMBER's 31 copies");
    assert_eq!(size(&start), 61);
    let prior = ConstructedPrior::new(&main, &side);
    let mut rng = Rng::new(61);
    let (mut paths, mut walks, mut decisions) = (0, 0, 0);
    let mut swaps = BTreeSet::new();
    for _ in 0..300 {
        let s = prior.sample(&DeckWant { db: db(), n: 61, shown: &[], companion: None, tilt: None, pick: None, seen: None }, &mut rng);
        let (_, target) = MatchPool::constructed(&s.deck, &s.sideboard);
        assert_eq!(size(&target), 61);
        assert_eq!(sideboard_to(&pool, &start, &target).unwrap(), target);
        swaps.insert(prior.swaps_of(&s.deck));
        paths += 1;
        // one card more or fewer than 61: no path
        for (k, dlt) in [(0usize, 1i32), (pool.list.len() - 1, -1), (3, 1), (5, -1)] {
            let mut t = target.clone();
            let v = t[k] as i32 + dlt;
            if (0..=pool.list[k].1 as i32).contains(&v) {
                t[k] = v as u8;
                assert!(sideboard_to(&pool, &start, &t).is_err(), "a deck of {} reached", size(&t));
            }
        }
    }
    assert!(swaps.len() >= 12, "the prior's draws cover the swap counts: {swaps:?}");
    for _ in 0..600 {
        let mut sb = Sideboarding::new(0, start.clone(), &pool);
        loop {
            let pass = sb.decision.options.iter().any(|o| o.action == act::PASS);
            if sb.decision.kind == DecisionKind::Sideboard {
                assert_eq!(pass, size(&sb.counts) == 61, "PASS offered at {}", size(&sb.counts));
            }
            let o = &sb.decision.options[rng.below(sb.decision.options.len() as u32) as usize];
            decisions += 1;
            if sb.choose(&pool, o.action).unwrap() {
                break;
            }
        }
        assert_eq!(size(&sb.counts), 61);
        assert!(sb.counts.iter().zip(&pool.list).all(|(&c, e)| c <= e.1), "more copies than the 76 has");
        walks += 1;
    }
    println!("Lands 76: {paths} canonical paths (swap counts {swaps:?}), {walks} random walks ({:.1} decisions a walk)", decisions as f64 / walks as f64);
}

/// Every decision sequence of sideboarding `pool` from `start` (DFS): the decks reached by `PASS`.
fn reachable(pool: &MatchPool, start: Vec<u8>) -> Vec<Vec<u8>> {
    fn go(pool: &MatchPool, sb: Sideboarding, out: &mut Vec<Vec<u8>>) {
        assert!(!sb.decision.options.is_empty(), "a dead end");
        for o in &sb.decision.options {
            let mut x = sb.clone();
            if x.choose(pool, o.action).unwrap() {
                out.push(x.counts.clone());
            } else {
                go(pool, x, out);
            }
        }
    }
    let mut out = Vec::new();
    go(pool, Sideboarding::new(0, start, pool), &mut out);
    out
}

#[test]
fn cr_100_4a_sideboarding_a_main_above_the_minimum_reaches_every_deck_of_its_size_exactly_once() {
    // A small pool whose main is one card more than another (7 and 6, the same sideboard): the DFS reaches every count
    // vector within the copies of exactly the main's size, each by one path - as `cr_100_4a_constructed_sideboarding_
    // reaches_every_legal_deck_exactly_once` (constructed_bo3.rs) for a main of 6.
    let (a, b, island, c, e) = (10u16, 11, 12, 13, 14);
    for main in [vec![a, a, a, b, island, c], vec![a, a, a, b, island, c, c]] {
        let side = vec![a, b, c, e];
        let n = main.len();
        let (pool, start) = MatchPool::constructed(&main, &side);
        assert_eq!((pool.min_deck, pool.max_deck), (n, n));
        let copies: Vec<u8> = pool.list.iter().map(|e| e.1).collect();
        let mut want = BTreeSet::new();
        let mut c = vec![0u8; copies.len()];
        loop {
            if size(&c) == n {
                want.insert(c.clone());
            }
            let mut k = 0;
            while k < c.len() && c[k] == copies[k] {
                c[k] = 0;
                k += 1;
            }
            if k == c.len() {
                break;
            }
            c[k] += 1;
        }
        let got = reachable(&pool, start.clone());
        let set: BTreeSet<Vec<u8>> = got.iter().cloned().collect();
        assert_eq!(set.len(), got.len(), "a deck reached twice (main of {n})");
        assert_eq!(set, want, "main of {n}");
        println!("a main of {n}: {} decks of exactly {n}, each reached once", got.len());
    }
}

#[test]
fn the_constructed_prior_of_a_hidden_61_meets_its_contract() {
    // ENGINE.md 120.2 at n = 61: uniform over the swap count 0..=15, then uniform over the 61s with it; conditioned
    // exactly on the shown cards; a tilt of 1 the untilted draw; the expected fill the draws' mean; a request of
    // another size refused (the prior is of the registered main's size).
    let (main, side) = ids(&lands());
    let prior = ConstructedPrior::new(&main, &side);
    assert_eq!(prior.decks_by_swaps().len(), 16);
    let mut rng = Rng::new(272);
    let plain = DeckWant { db: db(), n: 61, shown: &[], companion: None, tilt: None, pick: None, seen: None };
    let n = 3200;
    let mut hist = [0usize; 16];
    for _ in 0..n {
        let s = prior.sample(&plain, &mut rng);
        check_sample(&prior, &plain, &s).unwrap();
        assert_eq!((s.deck.len(), s.sideboard.len()), (61, 15));
        hist[prior.swaps_of(&s.deck)] += 1;
    }
    for (s, &h) in hist.iter().enumerate() {
        assert!((h as f64 - 200.0).abs() < 70.0, "swaps {s}: {h} of {n} ({hist:?})");
    }
    println!("swaps of {n} untilted 61s: {hist:?}");
    let id = |s: &str| db().expect_id(s).unwrap();
    // two Torpor Orbs and a Damping Sphere (sideboard), a fourth Forest is not in the 76 (three): three Forests shown
    let mut sh = vec![id("Torpor Orb"), id("Torpor Orb"), id("Damping Sphere"), id("Forest"), id("Forest"), id("Forest"), id("Dark Depths")];
    sh.sort_unstable();
    let want = DeckWant { db: db(), n: 61, shown: &sh, companion: None, tilt: None, pick: None, seen: None };
    for _ in 0..600 {
        let s = prior.sample(&want, &mut rng);
        check_sample(&prior, &want, &s).unwrap();
        assert!(holds(&s.deck, &sh));
        assert!(prior.swaps_of(&s.deck) >= 3, "three sideboard cards shown: three swaps at least");
    }
    let one = |_d: u16| 1.0;
    let tilted1 = DeckWant { db: db(), n: 61, shown: &sh, companion: None, tilt: Some(&one), pick: None, seen: None };
    let (mut r1, mut r2) = (Rng::new(9), Rng::new(9));
    for _ in 0..50 {
        assert_eq!(prior.sample(&want, &mut r1), prior.sample(&tilted1, &mut r2));
    }
    let fill = prior.expected_fill(&want);
    assert!((fill.iter().sum::<f64>() - (61 - sh.len()) as f64).abs() < 1e-6, "the fill is the 61's other cards");
    let k = 4000;
    let mut mean = vec![0.0f64; db().len()];
    for _ in 0..k {
        let s = prior.sample(&want, &mut rng);
        let mut left = s.deck.clone();
        for x in &sh {
            left.remove(left.binary_search(x).unwrap());
        }
        for d in left {
            mean[d as usize] += 1.0 / k as f64;
        }
    }
    for (d, (&f, &m)) in fill.iter().zip(&mean).enumerate() {
        assert!((f - m).abs() < 0.08, "{}: expected fill {f:.3}, drawn {m:.3}", db().def(d as u16).name);
    }
    // a 60 of the 76 is no deck of this prior: refused loudly
    let sixty = DeckWant { db: db(), n: 60, shown: &[], companion: None, tilt: None, pick: None, seen: None };
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| prior.sample(&sixty, &mut Rng::new(1))));
    assert!(r.is_err(), "the constructed prior dealt a deck of another size than its main's");
    // the dealt prior (ENGINE.md 154) at n = 61: swaps within its range
    let dealt = ConstructedPrior::dealt(&main, &side, 2, 6);
    for _ in 0..400 {
        let s = dealt.sample(&plain, &mut rng);
        check_sample(&dealt, &plain, &s).unwrap();
        assert!((2..=6).contains(&dealt.swaps_of(&s.deck)) && s.deck.len() == 61);
    }
}

/// Plays match `sp` from seed `seed` (heuristic and random mixed; with `game_episodes`, across the episodes) calling
/// `each` before every decision with the chance keys of the step that led there; returns the games played.
fn play(sp: &MtgSpec, seed: u64, mut each: impl FnMut(&MtgAz, &[u64])) -> u8 {
    let mut g = MtgAz::new_game(sp, 2, seed).unwrap();
    let mut h = Heuristic::new(seed);
    let mut rng = Rng::new(seed ^ 0x272);
    let mut keys: Vec<u64> = Vec::new();
    for _ in 0..400_000 {
        let Some(_) = g.to_move() else {
            if !g.m.game_episodes {
                return g.m.game_no;
            }
            match g.next_episode(None).unwrap() {
                Next::Continue(n) => {
                    g = n;
                    continue;
                }
                Next::Over(_) => return g.m.game_no,
            }
        };
        each(&g, &keys);
        let l = legal(&g);
        let a = if g.sideboarding().is_none() && rng.below(3) != 0 { h.choose(&g.g, &l) } else { l[rng.below(l.len() as u32) as usize] };
        keys.clear();
        g.step(a, &mut keys).unwrap();
    }
    panic!("seed {seed}: no end");
}

#[test]
fn lands_against_delver_best_of_three_plays_the_61_in_every_game_and_deals_hidden_61s() {
    // Whole matches of the Lands 76 against the Delver 75 (both seats): every game each seat plays its registered
    // main's size (61, 60) with the rest of its list as its sideboard (15), `KEY_GAME` says so, every later game's
    // world of either viewer deals the opponent a legal deck of their registered size holding what the viewer saw,
    // the viewer's whole encoding and the decider's actions kept; game 1's worlds keep the registered main.
    let (l, d) = (lands(), delver());
    let (mut games, mut keys_seen, mut worlds, mut hidden_61, mut other) = (0u32, 0u32, 0u32, 0u32, 0u32);
    for seed in 0..12u64 {
        let (a, b) = if seed % 2 == 0 { (&l, &d) } else { (&d, &l) };
        let n = [a.main_names().len(), b.main_names().len()];
        let sp = parse(&fixed(a, b, json!({"max_turns": if seed % 3 == 2 { 12 } else { 60 }}))).unwrap();
        let mut last_game = 0u8;
        let played = play(&sp, seed, |g, keys| {
            for &k in keys.iter().filter(|&&k| key_kind(k) == KEY_GAME) {
                let p = key_payload(k);
                assert_eq!([p & 0xFF, p >> 8 & 0xFF, p >> 16 & 0xFF, p >> 24 & 0xFF], [n[0] as u64, n[1] as u64, 15, 15], "seed {seed}: KEY_GAME");
                keys_seen += 1;
            }
            if g.sideboarding().is_some() {
                return;
            }
            if g.m.game_no != last_game {
                last_game = g.m.game_no;
                games += 1;
                for q in 0..2u8 {
                    assert_eq!((g.m.deck(q).len(), g.g.sideboard_size(q), size(&g.m.decks[q as usize])), (n[q as usize], 15, n[q as usize]), "seed {seed} game {last_game}: P{q}");
                }
            }
            if g.g.turn().number % 4 != 3 {
                return;
            }
            let p = g.to_move().unwrap();
            for viewer in [p, 1 - p] {
                let o = 1 - viewer as u8;
                let seen = shown(g, viewer as u8);
                let mut w = g.clone();
                w.determinize(viewer, &mut zero_search::rng::Rng::new(seed * 1000 + g.g.turn().number as u64 * 2 + viewer as u64));
                if viewer == p {
                    assert_eq!(legal(&w), legal(g), "seed {seed}: the decider's actions");
                }
                assert!(enc(&w, viewer) == enc(g, viewer), "seed {seed} game {}: the viewer's encoding", g.m.game_no);
                let world = game_deck(&w, o);
                assert_eq!(world, w.m.deck(o));
                assert_eq!(world.len(), n[o as usize], "seed {seed}: a world's deck of the opponent's registered size");
                assert!(holds(&world, &seen), "seed {seed}: the world's deck holds what the viewer saw");
                assert_eq!(w.m.pools[o as usize].list, g.m.pools[o as usize].list);
                assert!(w.m.decks[o as usize].iter().zip(&w.m.pools[o as usize].list).all(|(&c, e)| c <= e.1));
                assert_eq!(w.g.sideboard_size(o), 15);
                w.g.check_invariants().unwrap();
                w.g.check_knowledge().unwrap();
                worlds += 1;
                if g.m.game_no == 1 {
                    assert_eq!(world, game_deck(g, o), "game 1: the registered main");
                } else {
                    hidden_61 += (n[o as usize] == 61) as u32;
                    other += (world != game_deck(g, o)) as u32;
                }
            }
        });
        assert!((2..=3).contains(&played), "seed {seed}: a best-of-three of {played} games");
    }
    println!("{games} games, {keys_seen} KEY_GAME keys, {worlds} worlds ({hidden_61} later-game worlds dealing a hidden 61, {other} with another deck than the real one)");
    assert!(keys_seen >= 12 && hidden_61 >= 20 && other >= 20, "{keys_seen} {hidden_61} {other}");
}

/// A choice table of decklist `d` (ENGINE.md 168.3): its list in first-seen order, each context's deck the main with
/// random one-for-one swaps (seeded) - so of the main's size - and `extra` cards added to every choice's first entry
/// that has room (`extra` != 0: a table of another size).
fn table_of(d: &Decklist, seed: u64, extra: i32) -> Value {
    let (main, side) = (d.main_names(), side_names(d));
    let (mut names, mut copies, mut base): (Vec<String>, Vec<u8>, Vec<u8>) = (Vec::new(), Vec::new(), Vec::new());
    for (i, n) in main.iter().chain(&side).enumerate() {
        let k = names.iter().position(|x| x == n).unwrap_or_else(|| {
            names.push(n.clone());
            copies.push(0);
            base.push(0);
            names.len() - 1
        });
        copies[k] += 1;
        if i < main.len() {
            base[k] += 1;
        }
    }
    let mut r = Rng::new(seed);
    let mut choices = Vec::new();
    for game in 2..=3u8 {
        for first in 0..2u8 {
            for lead in -1..=1i8 {
                for od in 0..2u8 {
                    let mut c = base.clone();
                    for _ in 0..1 + r.below(12) {
                        let out: Vec<usize> = (0..c.len()).filter(|&k| c[k] > 0).collect();
                        let inn: Vec<usize> = (0..c.len()).filter(|&k| c[k] < copies[k]).collect();
                        let (o, i) = (out[r.below(out.len() as u32) as usize], inn[r.below(inn.len() as u32) as usize]);
                        if o != i {
                            c[o] -= 1;
                            c[i] += 1;
                        }
                    }
                    if extra > 0 {
                        let k = (0..c.len()).find(|&k| c[k] < copies[k]).unwrap();
                        c[k] += 1;
                    } else if extra < 0 {
                        let k = (0..c.len()).find(|&k| c[k] > 0).unwrap();
                        c[k] -= 1;
                    }
                    choices.push(json!([game, first, lead, od, c]));
                }
            }
        }
    }
    let list: Vec<Value> = names.iter().zip(&copies).map(|(n, k)| json!([n, k])).collect();
    json!({"version": format!("t{seed}"), "list": list, "choices": choices})
}

#[test]
fn a_choice_table_of_61s_is_followed_and_a_table_of_another_size_refused() {
    let (l, d) = (lands(), delver());
    let t = table_of(&l, 61, 0);
    // a table of 61s on the Lands seat: parsed, followed to each context's 61 (teacher decisions at its sideboardings)
    let sp = parse(&fixed(&l, &d, json!({"decks.p0_table": t, "max_turns": 12, "game_episodes": true}))).unwrap();
    assert_eq!(sp.seat_tables[0].as_ref().unwrap().size, 61);
    let (mut reached, mut taught) = (0u32, 0u32);
    for seed in 0..30u64 {
        let mut g = MtgAz::new(&sp, seed).unwrap();
        assert!(g.m.table[0].is_some() && g.m.table[1].is_none());
        let mut rng = Rng::new(seed ^ 0x61);
        let mut target: Option<Vec<u8>> = None;
        let mut game = g.m.game_no;
        for _ in 0..200_000 {
            if g.m.game_no != game && g.sideboarding().is_none() {
                game = g.m.game_no;
                if let Some(x) = target.take() {
                    assert_eq!(&g.m.decks[0][..], &x[..], "seed {seed} game {game}: not its context's 61");
                    assert_eq!(g.m.deck(0).len(), 61);
                    reached += 1;
                }
            }
            let Some(p) = g.to_move() else {
                match g.next_episode(None).unwrap() {
                    Next::Continue(n) => {
                        g = n;
                        continue;
                    }
                    Next::Over(_) => break,
                }
            };
            let lg = legal(&g);
            let mut pi = Vec::new();
            let teach = g.teacher_policy(&lg, &mut pi);
            let a = if teach {
                assert_eq!(p, 0, "seed {seed}: only the table's seat is taught");
                let s = g.sideboarding().unwrap();
                let lead = g.m.wins[0] as i8 - g.m.wins[1] as i8;
                let want = g.m.table[0].as_ref().unwrap().choice(g.m.game_no + 1, s.turn == 0, lead, g.m.is_dealt(1)).to_vec();
                assert_eq!(size(&want), 61);
                target = Some(want);
                taught += 1;
                lg[pi.iter().position(|&w| w == 1.0).unwrap()]
            } else {
                lg[rng.below(lg.len() as u32) as usize]
            };
            g.step(a, &mut Vec::new()).unwrap();
        }
    }
    println!("{taught} teacher decisions, {reached} table 61s reached");
    assert!(reached >= 20, "{reached}");
    let _ = context_index(2, true, 0, false);
    // a table of 60s or of 62s over the Lands 76's list: not of the 61's pool (its size), refused naming both sizes
    refused(&fixed(&l, &d, json!({"decks.p0_table": table_of(&l, 7, -1)})), "61 cards here, the table's 60");
    refused(&fixed(&l, &d, json!({"decks.p0_table": table_of(&l, 7, 1)})), "61 cards here, the table's 62");
    // the Delver's table of 60s still fits the Delver seat beside a 61
    parse(&fixed(&l, &d, json!({"decks.p1_table": table_of(&d, 8, 0)}))).unwrap();
    // choices of two sizes, or under 60 (CR 100.2a), refused in the table itself
    let mut mixed = table_of(&l, 9, 0);
    let mut c0: Vec<u64> = mixed["choices"][0][4].as_array().unwrap().iter().map(|x| x.as_u64().unwrap()).collect();
    let k = c0.iter().position(|&x| x > 0).unwrap();
    c0[k] -= 1;
    mixed["choices"][0][4] = json!(c0);
    refused(&fixed(&l, &d, json!({"decks.p0_table": mixed})), "has 61 cards, another choice 60");
    let mut small = table_of(&l, 10, 0);
    for c in small["choices"].as_array_mut().unwrap() {
        let mut v: Vec<u64> = c[4].as_array().unwrap().iter().map(|x| x.as_u64().unwrap()).collect();
        let mut drop = 2;
        for x in v.iter_mut() {
            while *x > 0 && drop > 0 {
                *x -= 1;
                drop -= 1;
            }
        }
        c[4] = json!(v);
    }
    refused(&fixed(&l, &d, json!({"decks.p0_table": small})), "has 59 cards (CR 100.2a: at least 60)");
}

#[test]
fn dealt_decks_of_the_61_stay_61_within_their_range() {
    // `dealt_decks` per seat (ENGINE.md 154, 168.1), around the main and around a table of 61s (261): every dealt deck
    // of the Lands seat is a legal 61 of its 76 within the swap range of its base, the Delver's a legal 60; the other
    // viewer's worlds deal them hidden at the registered sizes.
    let (l, d) = (lands(), delver());
    let (main, _) = ids(&l);
    let mut main_counts: Option<Vec<u8>> = None;
    let (mut dealt, mut worlds) = (0u32, 0u32);
    for (around, extra) in [("main", json!({})), ("table", json!({"decks.p0_table": table_of(&l, 31, 0)}))] {
        let mut v = fixed(&l, &d, extra);
        v["dealt_decks"] = json!({"p": 1.0, "swaps": [1, 8], "per_seat": true, "around": around});
        v["max_turns"] = json!(12);
        let sp = parse(&v).unwrap();
        for seed in 0..16u64 {
            let mut last = 0u8;
            play(&sp, seed, |g, _| {
                if g.sideboarding().is_some() {
                    return;
                }
                let counts0 = main_counts.get_or_insert_with(|| MatchPool::constructed(&main, &ids(&l).1).1).clone();
                if g.m.game_no != last {
                    last = g.m.game_no;
                    assert!(g.m.is_dealt(0) && g.m.is_dealt(1));
                    assert_eq!((g.m.deck(0).len(), g.m.deck(1).len()), (61, 60), "seed {seed} game {last}");
                    if last >= 2 {
                        dealt += 1;
                        if around == "main" {
                            // swaps against the registered main: the cards beyond its counts
                            let sw: usize = g.m.decks[0].iter().zip(&counts0).map(|(&c, &m)| c.saturating_sub(m) as usize).sum();
                            assert!((1..=8).contains(&sw), "seed {seed}: a dealt 61 of {sw} swaps");
                        }
                    }
                }
                if last >= 2 && g.g.turn().number == 3 {
                    let p = g.to_move().unwrap();
                    for viewer in [p, 1 - p] {
                        let mut w = g.clone();
                        w.determinize(viewer, &mut zero_search::rng::Rng::new(seed ^ 0xDEA1 ^ viewer as u64));
                        assert!(enc(&w, viewer) == enc(g, viewer));
                        assert_eq!((w.m.deck(0).len(), w.m.deck(1).len()), (61, 60));
                        worlds += 1;
                    }
                }
            });
        }
    }
    println!("{dealt} dealt later games, {worlds} worlds");
    assert!(dealt >= 30 && worlds >= 20, "{dealt} {worlds}");
}

#[test]
fn a_best_of_three_metagame_plays_the_lands_61() {
    let decks: Vec<Value> = [lands(), delver()].iter().map(|d| json!({"name": d.name, "main": d.main_names(), "sideboard": side_names(d)})).collect();
    let v = json!({"max_turns": 12, "constructed": true, "format": "bo3", "metagame": {"decks": decks, "weights": [[1, 1], [1, 1]]}});
    let sp = parse(&v).unwrap();
    let mut lands_seats = 0;
    for seed in 0..8u64 {
        let pr = sp.pairing(seed).unwrap();
        play(&sp, seed, |g, _| {
            for q in 0..2u8 {
                let want = if pr.names[q as usize].as_ref() == "Lands" { 61 } else { 60 };
                assert_eq!(g.m.deck(q).len(), want, "seed {seed}: {}", pr.names[q as usize]);
            }
        });
        lands_seats += pr.names.iter().filter(|n| n.as_ref() == "Lands").count();
    }
    assert!(lands_seats > 0);
    // a metagame deck under 60 is refused (CR 100.2a)
    let mut short = v.clone();
    let mut m: Vec<Value> = short["metagame"]["decks"][0]["main"].as_array().unwrap().clone();
    m.truncate(59);
    short["metagame"]["decks"][0]["main"] = json!(m);
    refused(&short, "59 cards in the deck (CR 100.2a: at least 60)");
}
