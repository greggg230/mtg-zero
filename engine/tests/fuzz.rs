//! Random-play fuzzer (ENGINE.md 15): complete games between random decks
//! of supported cards, checking after every action that nothing panics, the
//! structural invariants hold (every card in exactly one zone), legal
//! actions are sorted, unique and non-empty, and the game terminates; that
//! the same seed and actions replay bit for bit; that a clone continues
//! identically; and that `determinize` keeps the decider's view.
//!
//! `FUZZ_GAMES=n` sets the number of games (default 300).

mod common;

use engine::rng::Rng;
use engine::{Game, GameSpec, Outcome, NUM_ACTIONS};

struct Played {
    actions: Vec<u16>,
    keys: Vec<u64>,
    fingerprint: u64,
    outcome: Outcome,
    turns: u32,
}

fn decks(seed: u64) -> (Vec<u16>, Vec<u16>) {
    let db = common::db();
    let mut rng = Rng::new(seed ^ 0xDEC5);
    (common::random_deck(db, &mut rng), common::random_deck(db, &mut rng))
}

fn new_game(seed: u64) -> Game {
    let (d0, d1) = decks(seed);
    // `FUZZ_LOG=1` keeps the game log, so a panic message shows the game.
    let log = std::env::var("FUZZ_LOG").is_ok();
    Game::new(common::db(), GameSpec { log, ..GameSpec::default() }, [&d0, &d1], seed).unwrap()
}

/// Play one game with random actions; run the extra checks along the way.
fn play(seed: u64, checks: bool) -> Played {
    let mut g = new_game(seed);
    let mut rng = Rng::new(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 1);
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
        if checks && rng.below(40) == 0 {
            check_clone(&g, a, seed);
            check_determinize(&g, seed, &mut rng);
        }
        g.apply(a, &mut keys).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        g.check_invariants().unwrap_or_else(|e| panic!("seed {seed} step {steps}: {e}"));
        actions.push(a);
        steps += 1;
        assert!(steps < 100_000, "seed {seed}: game does not end");
    }
    Played { actions, keys, fingerprint: g.fingerprint(), outcome: g.outcome().unwrap(), turns: g.turn().number }
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
    let viewer = g.to_move().unwrap();
    let mut d = g.clone();
    d.determinize(viewer, rng);
    let (mut l1, mut l2) = (Vec::new(), Vec::new());
    g.legal_actions(&mut l1);
    d.legal_actions(&mut l2);
    assert_eq!(l1, l2, "seed {seed}: determinize changed the decider's actions");
    assert_eq!(g.battlefield(), d.battlefield());
    for &c in g.battlefield() {
        assert_eq!(g.obj(c).def, d.obj(c).def);
    }
    d.check_invariants().unwrap();
    let mut legal = Vec::new();
    let mut keys = Vec::new();
    for _ in 0..30 {
        if d.is_over() {
            break;
        }
        d.legal_actions(&mut legal);
        d.apply(legal[rng.below(legal.len() as u32) as usize], &mut keys).unwrap();
        d.check_invariants().unwrap_or_else(|e| panic!("seed {seed} (determinized): {e}"));
    }
}

#[test]
fn fuzz_random_games() {
    let n: u64 = std::env::var("FUZZ_GAMES").ok().and_then(|s| s.parse().ok()).unwrap_or(300);
    // `FUZZ_FIRST=s` starts at seed s (to rerun a failing seed alone).
    let first: u64 = std::env::var("FUZZ_FIRST").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    let (mut wins, mut draws, mut turns, mut decisions) = ([0u32; 2], 0u32, 0u64, 0u64);
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
    }
    println!(
        "{n} games: wins {wins:?}, draws {draws}, mean turns {:.1}, mean decisions {:.1}",
        turns as f64 / n as f64,
        decisions as f64 / n as f64
    );
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
