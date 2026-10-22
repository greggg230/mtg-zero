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
