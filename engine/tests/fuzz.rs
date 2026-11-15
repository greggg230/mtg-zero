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

use engine::fuzz::games::{
    self, constructed_bo3_mode, constructed_mode, delver_post_spec, eoe_mode, hidden_decks_mode, match_spec, new_game, play, play_hidden, play_match,
    play_match_spec, pool_mode, replay_match, sealed_mode, sealed_unplayable, HiddenStats, PoolMode, POOL_OVERRIDE,
};
use engine::fuzz::{NoHooks, Run};

/// ENGINE.md 147.8 (lane G's F1, 148.6): the default pool's game 2122 passes every check of `play`. After a mana ability
/// activated through `ActivateMana` (CR 605.3a) the next `ActivateMana` returned silently to `Priority` when no source
/// was left - a fact of the decider's hidden hand (a Spirit Guide) - so two worlds of the other player's showed it
/// `Priority` and `ActivateMana` after equal keys and equal legal actions (the open-decklist information-set walk).
/// Now the one-option `ActivateMana` (`PASS`) is asked and keyed (`KEY_AUTO`). Default pool only (another pool's seed
/// 2122 is another game); the cross pool's games 13536 and 19515 are the same finding (147.8).
#[test]
fn activate_mana_end_is_keyed_default_seed_2122() {
    if pool_mode() != PoolMode::Default || hidden_decks_mode() {
        return;
    }
    let p = play(2122, true);
    assert!(!p.actions.is_empty());
}

/// ENGINE.md 160: the same finding in the interactions pool (`FUZZ_POOL=interactions`), games 2031 (after a
/// `ChooseMode` of P0's: which mana ability of a permanent, inside `ActivateMana`) and 13518 (after a `ChooseMana`,
/// Karakas's ability on the stack) - the walk's decisions `Priority` / `ActivateMana`, the hand slot's ability the same
/// action id in both worlds. Reported on branches without 147.8 (`fix/hidden-refs` 3ca4fb2d, `fix/encoding-gate`);
/// they pass with 147.8's keyed end. Interactions pool only (another pool's seeds are other games).
#[test]
fn activate_mana_end_is_keyed_interactions_seeds() {
    if pool_mode() != PoolMode::Interactions || hidden_decks_mode() {
        return;
    }
    for seed in [2031u64, 13518] {
        let p = play(seed, true);
        assert!(!p.actions.is_empty(), "seed {seed}");
    }
}

/// ENGINE.md 161: default-pool games whose open-decklist walk found two worlds of P1's keyed apart only by P0's forced
/// answer naming a hidden card by its place, with equal views (160.4's count, on 296a45b8): 318 and 692 (`ExileCost`, a
/// lone blue card for Force of Will's exile at hand slots 2 / 4 and 2 / 3), 2456 and 2704 (`PayMana`, a lone Spirit
/// Guide's ability by its hand slot, `HAND_ABILITY` + 1 / 2 and + 3 / 4). They pass every check of `play`, the walk's
/// failing forced-answer check included, with the place masked in P1's worlds (`Game::forced_answer_seen`).
#[test]
fn forced_answers_naming_a_hidden_place_are_masked_default_seeds() {
    if pool_mode() != PoolMode::Default || hidden_decks_mode() {
        return;
    }
    for seed in [318u64, 692, 2456, 2704] {
        let p = play(seed, true);
        assert!(!p.actions.is_empty(), "seed {seed}");
    }
}

/// ENGINE.md 140.10: the hidden-decklist games whose checks compared the deck's objects alone (a companion put into
/// the hand and shuffled away traded identities with a deck card in a world: "the viewer's own cards", "the match's
/// deck of the opponent", "the sampled deck lacks a shown ..."), replayed with every check of `play_hidden`: seed 331
/// (this branch's worlds), 2679, 3191, 3279, 3671, 5095, 6247, 6375, 6419, 7943 and 9671 (the rule-invariant
/// campaign's on 267ae603). The default pool only (the seeds are its games).
#[test]
fn hidden_decklist_worlds_keep_the_deck_whatever_object_holds_the_companion() {
    if pool_mode() != PoolMode::Default || sealed_mode() {
        return;
    }
    for seed in [331u64, 2679, 3191, 3279, 3671, 5095, 6247, 6375, 6419, 7943, 9671] {
        std::panic::catch_unwind(|| {
            play_hidden(seed);
        })
        .unwrap_or_else(|e| panic!("hidden-decklist seed {seed} failed (rerun: FUZZ_HIDDEN_DECKS=1 FUZZ_FIRST={seed} FUZZ_GAMES=1): {e:?}"));
    }
}

/// ENGINE.md 162: `FUZZ_POOL=cross`'s post-sideboard match 98 (a Doomsday mirror, P0 keeping its deck) passes every
/// check of `play_match_spec`. In game 2 P1 knew P0's whole 60 (R = the deck: no free card) and saw P0 find a listed
/// Underground Sea by a search; the sighting's terms were a = cnt F = 0, b = 0 - "no listed copy stood" - so the find
/// asked a fifth Sea and the constructed prior panicked "no deck holds what the viewer knows". Now a = cnt. Run in
/// every pool (its own pool for its thread, over `FUZZ_POOL`), not with hidden decklists.
#[test]
fn a_listed_card_s_find_with_the_deck_known_cross_post_sideboard_match_98() {
    if hidden_decks_mode() {
        return;
    }
    POOL_OVERRIDE.with(|c| c.set(Some(PoolMode::Cross)));
    let r = std::panic::catch_unwind(|| {
        let mut stats = HiddenStats::default();
        play_match_spec(98, &delver_post_spec(98), true, &mut stats)
    });
    POOL_OVERRIDE.with(|c| c.set(None));
    let p = r.unwrap_or_else(|e| panic!("cross post-sideboard match 98 failed (rerun: FUZZ_POOL=cross FUZZ_GAMES=0 FUZZ_FIRST=98 FUZZ_MATCHES=1): {e:?}"));
    assert!(p.games >= 2, "game 2 was played");
}

/// ENGINE.md 167: a sharded fuzz's end-of-run checks. `tools/fuzz_shards.py` runs `fuzz_random_games` /
/// `fuzz_random_matches` over disjoint seed ranges with `FUZZ_SHARD=1` (each shard prints its `FUZZ_TOTALS` and leaves
/// the checks over the whole run to this test), sums the shards' counters and runs this test with
/// `FUZZ_TOTALS_FILE=<the merged totals>`: the same checks as one run of all the seeds, on the totals. Without the
/// variable it checks nothing (the gate wants its "fuzz totals: ... checked" lines as positive evidence).
#[test]
fn fuzz_shard_totals() {
    let Ok(path) = std::env::var("FUZZ_TOTALS_FILE") else {
        return;
    };
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("FUZZ_TOTALS_FILE={path}: {e}"));
    for line in games::check_merged(&text).unwrap_or_else(|e| panic!("FUZZ_TOTALS_FILE={path}: {e}")) {
        println!("{line}");
    }
}

#[test]
fn fuzz_random_games() {
    let n: u64 = std::env::var("FUZZ_GAMES").ok().and_then(|s| s.parse().ok()).unwrap_or(300);
    // `FUZZ_FIRST=s` starts at seed s (to rerun a failing seed alone).
    let first: u64 = std::env::var("FUZZ_FIRST").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    if let Run::Failed { seed, payload: e } = games::random_games(first, n, &mut NoHooks) {
        panic!("seed {seed} failed (rerun: FUZZ_FIRST={seed} FUZZ_GAMES=1 FUZZ_LOG=1): {e:?}")
    }
}

/// ENGINE.md 142.10 (the Fable review's X3): interactions games 5,682, 6,025, 7,790 and 12,725
/// reach picks in progress in two tasks of the agenda - Etali, Primal Conqueror's trigger keeps the
/// cards it exiled as picks while a free cast from among them pays a cost that picks objects (5,682:
/// Abhorrent Oculus exiling six graveyard cards) - a loud failure of the encoding before. Each
/// plays through with every check, and its rows have the enclosing task's picks as `links` tokens
/// with their depth (`lk::DEPTH`); 5,682's for both viewers.
#[test]
fn enclosing_picks_of_etali_s_casts_are_encoded() {
    use engine::az::encode::CODE;
    use engine::az::tail_encode::{lk, seg, Row};
    POOL_OVERRIDE.with(|c| c.set(Some(PoolMode::Interactions)));
    for seed in [5682, 6025, 7790, 12725] {
        let p = play(seed, true);
        let mut g = new_game(seed);
        let mut keys = Vec::new();
        let mut deep = [0u32; 2];
        for &x in &p.actions {
            for v in 0..2u8 {
                deep[v as usize] += Row::of(&g, v)
                    .tokens(seg::LINKS)
                    .iter()
                    .filter(|(_, t)| (t[CODE + lk::KIND + lk::K_PICKED] == 1.0 || t[CODE + lk::KIND + lk::K_PICKED_DEF] == 1.0) && t[CODE + lk::DEPTH] > 0.0)
                    .count() as u32;
            }
            g.apply(x, &mut keys).unwrap();
        }
        assert_eq!(g.fingerprint(), p.fingerprint, "seed {seed}: the replay");
        println!("seed {seed}: an enclosing task's pick tokens by viewer {deep:?}");
        assert!(deep[0] + deep[1] > 0, "seed {seed}: no enclosing task's pick token");
        if seed == 5682 {
            assert!(deep[0] > 0 && deep[1] > 0, "seed {seed}: {deep:?}");
        }
    }
    POOL_OVERRIDE.with(|c| c.set(None));
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
    if (constructed_mode() && !constructed_bo3_mode()) || eoe_mode() || sealed_mode() {
        return;
    }
    for seed in 2000..2012 {
        let a = play_match(seed, false);
        let (fp, results, keys) = replay_match(seed, &match_spec(seed), &a.actions);
        assert_eq!((fp, results), (a.fingerprint, a.results), "match {seed}");
        assert_eq!(keys, a.keys, "match {seed}: chance keys differ");
        assert_eq!(play_match(seed, false).fingerprint, a.fingerprint);
        // the post-sideboard matches too (ENGINE.md 130)
        if constructed_bo3_mode() {
            let sp = delver_post_spec(seed);
            let a = play_match_spec(seed, &sp, false, &mut HiddenStats::default());
            let (fp, results, keys) = replay_match(seed, &sp, &a.actions);
            assert_eq!((fp, results, &keys), (a.fingerprint, a.results, &a.keys), "post-sideboard match {seed}");
            assert_eq!(play_match_spec(seed, &sp, false, &mut HiddenStats::default()).fingerprint, a.fingerprint);
        }
    }
}
