//! Performance numbers for ENGINE.md 17 (single thread, uniform random play
//! between random decks of supported cards).
//!
//! `cargo test --release -p engine --test perf -- --ignored --nocapture`

mod common;

use engine::rng::Rng;
use engine::{Game, GameSpec};
use std::hint::black_box;
use std::time::Instant;

fn decks(seed: u64) -> (Vec<u16>, Vec<u16>) {
    let db = common::db();
    let mut rng = Rng::new(seed ^ 0xDEC5);
    (common::random_deck(db, &mut rng), common::random_deck(db, &mut rng))
}

#[test]
#[ignore]
fn measure() {
    let db = common::db();
    let games = 4000u64;
    let all_decks: Vec<_> = (0..games).map(decks).collect();

    // Random play: whole games including setup.
    let mut legal = Vec::new();
    let mut keys = Vec::new();
    let mut applies = 0u64;
    let mut snapshots: Vec<Game> = Vec::new();
    let t = Instant::now();
    for seed in 0..games {
        let (d0, d1) = &all_decks[seed as usize];
        let mut g = Game::new(db, GameSpec::default(), [d0, d1], seed).unwrap();
        let mut rng = Rng::new(seed + 77);
        let mut i = 0u32;
        while !g.is_over() {
            g.legal_actions(&mut legal);
            let a = legal[rng.below(legal.len() as u32) as usize];
            keys.clear();
            g.apply(a, &mut keys).unwrap();
            applies += 1;
            i += 1;
            if seed < 200 && i.is_multiple_of(7) && !g.is_over() {
                snapshots.push(g.clone());
            }
        }
        black_box(g.outcome());
    }
    let secs = t.elapsed().as_secs_f64();
    println!(
        "random play: {games} games in {secs:.2} s = {:.0} games/s; {applies} decisions, {:.0} decisions/s, {:.2} us/decision",
        games as f64 / secs,
        applies as f64 / secs,
        secs * 1e6 / applies as f64
    );

    // Clone cost over mid-game states.
    let reps = 200;
    let t = Instant::now();
    for _ in 0..reps {
        for s in &snapshots {
            black_box(s.clone());
        }
    }
    let n = (reps * snapshots.len()) as f64;
    println!(
        "clone: {:.0} ns mean over {} mid-game states (size_of::<Game>() = {} bytes)",
        t.elapsed().as_secs_f64() * 1e9 / n,
        snapshots.len(),
        std::mem::size_of::<Game>()
    );

    // Clone + determinize (what the search does once per simulation).
    let mut rng = Rng::new(5);
    let t = Instant::now();
    for _ in 0..20 {
        for s in &snapshots {
            let mut d = s.clone();
            d.determinize(d.to_move().unwrap(), &mut rng);
            black_box(&d);
        }
    }
    println!(
        "clone + determinize: {:.0} ns mean",
        t.elapsed().as_secs_f64() * 1e9 / (20 * snapshots.len()) as f64
    );

    // determinize alone, on states cloned beforehand (the clones and their
    // drops, and so the allocator, out of the timing).
    let mut rng = Rng::new(5);
    let mut total = 0.0;
    for _ in 0..20 {
        let mut copies: Vec<Game> = snapshots.clone();
        let t = Instant::now();
        for d in copies.iter_mut() {
            let v = d.to_move().unwrap();
            d.determinize(v, &mut rng);
        }
        total += t.elapsed().as_secs_f64();
        black_box(&copies);
    }
    println!("determinize alone: {:.0} ns mean", total * 1e9 / (20 * snapshots.len()) as f64);

    // legal_actions (a copy of the cached options).
    let t = Instant::now();
    for _ in 0..reps {
        for s in &snapshots {
            s.legal_actions(&mut legal);
            black_box(&legal);
        }
    }
    println!("legal_actions: {:.0} ns mean", t.elapsed().as_secs_f64() * 1e9 / n);
}

/// M8b (ENGINE.md 36.8): the match layer's cost per decision in best-of-one
/// (`MtgAz`: step, legal actions, clone, encode) against the bare `Game`
/// (apply, legal actions, clone, `encode_features`: the M8a `MtgAz`), on
/// the same games (heuristic against random, the spec's decks); and the
/// decisions of best-of-three matches (heuristic against heuristic, random).
#[test]
#[ignore]
fn match_layer() {
    use engine::az::{bots::Heuristic, MtgAz, MtgSpec, NUM_FEATURES};
    use zero_search::game::SearchGame;
    let sp = MtgSpec::default();
    let n = 300u64;
    let mut x = vec![0.0f32; NUM_FEATURES];
    let (mut t_game, mut t_match, mut decisions) = (0.0f64, 0.0f64, 0u64);
    for round in 0..2 {
        for seed in 0..n {
            // The actions: heuristic (seat seed % 2) against random.
            let mut g = MtgAz::new_game(&sp, 2, seed).unwrap();
            let mut h = Heuristic::new(seed);
            let mut rng = Rng::new(seed);
            let (mut l, mut k) = (Vec::new(), Vec::new());
            let mut acts = Vec::new();
            while let Some(p) = g.to_move() {
                g.legal_actions(&mut l);
                let a = if p as u64 == seed % 2 { h.choose(&g.g, &l) } else { l[rng.below(l.len() as u32) as usize] };
                g.step(a, &mut k).unwrap();
                acts.push(a);
            }
            let replay_game = |x: &mut [f32]| {
                let [(d0, s0), (d1, s1)] = sp.pools(seed);
                let mut g = Game::with_sideboards(engine::db::CardDb::cube(), GameSpec::default(), [&d0, &d1], [&s0, &s1], seed).unwrap();
                let (mut l, mut k) = (Vec::new(), Vec::new());
                for &a in &acts {
                    g.legal_actions(&mut l);
                    let c = g.clone();
                    black_box(&c);
                    x.fill(0.0);
                    g.encode_features(g.to_move().unwrap(), x);
                    g.apply(a, &mut k).unwrap();
                }
            };
            let replay_match = |x: &mut [f32]| {
                let mut g = MtgAz::new_game(&sp, 2, seed).unwrap();
                let (mut l, mut k) = (Vec::new(), Vec::new());
                for &a in &acts {
                    g.legal_actions(&mut l);
                    let c = g.clone();
                    black_box(&c);
                    x.fill(0.0);
                    g.encode(g.to_move().unwrap(), x);
                    g.step(a, &mut k).unwrap();
                }
            };
            // Alternate the order to even out caches.
            let t = Instant::now();
            if round == 0 { replay_game(&mut x) } else { replay_match(&mut x) }
            let e1 = t.elapsed().as_secs_f64();
            let t = Instant::now();
            if round == 0 { replay_match(&mut x) } else { replay_game(&mut x) }
            let e2 = t.elapsed().as_secs_f64();
            if round == 0 {
                t_game += e1;
                t_match += e2;
            } else {
                t_match += e1;
                t_game += e2;
            }
            decisions += acts.len() as u64;
        }
    }
    println!(
        "best-of-one, {decisions} decisions: Game {:.2} us, MtgAz {:.2} us per decision (legal, clone, encode, step)",
        t_game / decisions as f64 * 1e6,
        t_match / decisions as f64 * 1e6
    );
    // Decisions per best-of-three match.
    let sp3 = MtgSpec::parse(r#"{"format": "bo3"}"#).unwrap();
    for (label, bots) in [("heuristic vs heuristic", [true, true]), ("random vs random", [false, false])] {
        let (mut d, mut games, mut multi) = (0u64, 0u64, 0u64);
        for seed in 0..200u64 {
            let mut g = MtgAz::new_game(&sp3, 2, seed).unwrap();
            let mut hs = [Heuristic::new(seed), Heuristic::new(seed + 1)];
            let mut rng = Rng::new(seed);
            let (mut l, mut k) = (Vec::new(), Vec::new());
            while let Some(p) = g.to_move() {
                g.legal_actions(&mut l);
                let a = match g.sideboarding() {
                    Some(sb) if bots[p] => engine::az::bots::sideboard(sb, &l),
                    _ if bots[p] => hs[p].choose(&g.g, &l),
                    _ => l[rng.below(l.len() as u32) as usize],
                };
                d += 1;
                multi += (l.len() > 1) as u64;
                g.step(a, &mut k).unwrap();
            }
            games += g.m.game_no as u64;
        }
        println!(
            "best-of-three, {label}: {:.2} games, {:.0} decisions ({:.0} with two options or more) per match",
            games as f64 / 200.0,
            d as f64 / 200.0,
            multi as f64 / 200.0
        );
    }
}
