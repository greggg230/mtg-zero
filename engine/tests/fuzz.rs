//! Random-play fuzzer (ENGINE.md 15): complete games between random decks
//! of supported cards, and best-of-three matches, with every check after
//! every action. The loops live in the engine library, `engine::fuzz::games`
//! (ENGINE.md 114.1; its module docs list every `FUZZ_*` setting and what
//! each mode checks), so zero-farm's worker runs the same code; these tests
//! run them with the process environment's settings (`tools/fuzz_farm.py`
//! drives them unchanged), and check that games and matches replay.
//!
//! `FUZZ_GAMES=n` sets the number of games (default 300), `FUZZ_FIRST=s` the
//! first seed (to rerun a failing seed alone), `FUZZ_MATCHES=n` the matches
//! (default `FUZZ_GAMES` / 20, at least 20).

use engine::fuzz::games::{self, constructed_mode, eoe_mode, match_spec, new_game, play, play_match, sealed_mode, sealed_unplayable};
use engine::fuzz::{NoHooks, Run};

#[test]
fn fuzz_random_games() {
    let n: u64 = std::env::var("FUZZ_GAMES").ok().and_then(|s| s.parse().ok()).unwrap_or(300);
    // `FUZZ_FIRST=s` starts at seed s (to rerun a failing seed alone).
    let first: u64 = std::env::var("FUZZ_FIRST").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    if let Run::Failed { seed, payload: e } = games::random_games(first, n, &mut NoHooks) {
        panic!("seed {seed} failed (rerun: FUZZ_FIRST={seed} FUZZ_GAMES=1 FUZZ_LOG=1): {e:?}")
    }
}

#[test]
fn same_seed_and_actions_replay_identically() {
    for seed in 1000..1030 {
        if sealed_unplayable(seed).is_some() {
            continue;
        }
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

#[test]
fn fuzz_random_matches() {
    let games: u64 = std::env::var("FUZZ_GAMES").ok().and_then(|s| s.parse().ok()).unwrap_or(300);
    let n: u64 = std::env::var("FUZZ_MATCHES").ok().and_then(|s| s.parse().ok()).unwrap_or((games / 20).max(20));
    let first: u64 = std::env::var("FUZZ_FIRST").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    if let Run::Failed { seed, payload: e } = games::random_matches(first, n, &mut NoHooks) {
        panic!("match {seed} failed (rerun: FUZZ_FIRST={seed} FUZZ_MATCHES=1): {e:?}")
    }
}

#[test]
fn same_seed_and_actions_replay_matches_identically() {
    use engine::az::MtgAz;
    use zero_search::game::SearchGame;
    if constructed_mode() || eoe_mode() || sealed_mode() {
        return;
    }
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
