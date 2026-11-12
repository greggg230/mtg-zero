//! The random-actions fuzz of whole `team_draft` events, `engine/tests/draft.rs`'s
//! `fuzz_random_events` as library code (docs/ENGINE.md 114.1; the farm's `draft-events` mode), and
//! the helpers it shares with the draft tests.
//!
//! The random-actions fuzz of whole events (`DRAFT_FUZZ=n` events, 4v4, 2v2,
//! 3v3 and 1v1; the deck minimum 20, 0, 40 or random): the invariants after
//! every action; the decider's encoding
//! finite and within `encode::FEATURE_BOUND` at every decision, and every
//! world's (ENGINE.md 46); every ~40 actions
//! `determinize` for the decider and another viewer (knowledge, features,
//! options, invariants); at one decision of every 8th event every offered
//! option completes the event; the event resolved with random results (every
//! 50th by heuristic-played matches: a match that panics in the `mtg` engine
//! is recorded with its spec, the event resolved with random results, and the
//! test fails at the end listing every such match). `DRAFT_FUZZ_FIRST=f`:
//! events from f on, each on its own random streams (a failing one prints its
//! rerun).

use super::{Hooks, Run, SeedOutcome};
use crate::az::draft::{DraftSpec, Phase, TeamDraftAz, DECK_MIN, DECK_SIZE, DONE};
use crate::az::draft_bots;
use crate::az::draft_encode::NUM_FEATURES;
use crate::rng::Rng;
use zero_search::game::SearchGame;

/// A draft spec from its JSON (a test's: unwrapped).
pub fn spec(json: &str) -> DraftSpec {
    DraftSpec::parse(json).unwrap()
}

/// The legal actions.
pub fn legal(g: &TeamDraftAz) -> Vec<u16> {
    let mut v = Vec::new();
    g.legal_actions(&mut v);
    v
}

/// `viewer`'s encoding.
pub fn enc(g: &TeamDraftAz, viewer: u8) -> Vec<f32> {
    let mut x = vec![0.0; NUM_FEATURES];
    g.encode(viewer as usize, &mut x);
    x
}

/// zero-core's generator (what `determinize` takes).
pub fn zr(seed: u64) -> zero_search::rng::Rng {
    zero_search::rng::Rng::new(seed)
}

/// Random actions to the deferred terminal.
pub fn finish_random(g: &mut TeamDraftAz, rng: &mut Rng) -> u64 {
    let mut keys = Vec::new();
    let mut n = 0;
    while g.to_move().is_some() {
        let l = legal(g);
        assert!(!l.is_empty());
        let a = l[rng.below(l.len() as u32) as usize];
        g.step(a, &mut keys).unwrap();
        n += 1;
        assert!(n < 100_000);
    }
    n
}

/// Every value of a draft encoding is finite and within the bound (ENGINE.md
/// 46).
pub fn assert_bounded(x: &[f32], e: u64, i: u64, viewer: u8) {
    let bound = crate::az::encode::FEATURE_BOUND;
    if let Some((k, v)) = x.iter().enumerate().find(|(_, v)| !(v.is_finite() && v.abs() <= bound)) {
        panic!("event {e} step {i}: viewer {viewer}'s feature {k} is {v} (bound {bound})");
    }
}

/// How [`random_events`] ended, and how many heuristic-played matches panicked in the `mtg` engine
/// (each listed in the report; the test fails on any).
pub struct EventsEnd {
    pub run: Run,
    pub match_panics: usize,
}

/// `fuzz_random_events` (module docs): `events` events from `first` (`DRAFT_FUZZ_FIRST`: each event on
/// random streams of its own) or, with `first` None, events `0..events` on one stream; then the report.
/// An event that panics ends the run there ([`Run::Failed`]: no report); a heuristic-played match
/// that panics goes to [`Hooks::match_panic`] and into the report, and the event goes on.
pub fn random_events(first: Option<u64>, events: u64, hooks: &mut dyn Hooks) -> EventsEnd {
    let mut rng = Rng::new(0xF022);
    let mut zrng = zr(0xF022);
    let (mut steps, mut worlds, mut options_done, mut searched) = (0u64, 0u64, 0u64, [0u64; 2]);
    let mut team_wins = [0u64; 3];
    let mut heuristic_matches = 0u64;
    let mut match_panics: Vec<String> = Vec::new();
    // decks, their pool cards, card choices (DONE offered at) by minimum: the default / other
    let mut decks = [[0u64; 4]; 2];
    let t0 = std::time::Instant::now();
    for e in first.unwrap_or(0)..first.unwrap_or(0) + events {
        if !hooks.before(e) {
            return EventsEnd { run: Run::Stopped { at: e }, match_panics: match_panics.len() };
        }
        if first.is_some() {
            // (each event its own streams: it replays alone)
            let s = 0xF022 ^ e.wrapping_mul(0x9E37_79B9_7F4A_7C15);
            rng = Rng::new(s);
            zrng = zr(s ^ 1);
        }
        let mut outcome = None;
        let one = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let players = [8usize, 4, 8, 2, 8, 6][e as usize % 6];
            let min = [DECK_MIN as u32, 0, 40, rng.below(41)][(e / 6) as usize % 4];
            let mut g = TeamDraftAz::new(&spec(&format!(r#"{{"deck_min": {min}}}"#)), players, 0x5EED_0000 + e).unwrap();
            // (an event is 45 picks and 25-46 deckbuilding decisions per seat)
            let check_at = if e % 8 == 0 { rng.below(80 * players as u32) as u64 } else { u64::MAX };
            let mut i = 0u64;
            let mut keys = Vec::new();
            let mut x = vec![0.0f32; NUM_FEATURES];
            while let Some(d) = g.to_move() {
                let l = legal(&g);
                assert!(!l.is_empty() && l.windows(2).all(|w| w[0] < w[1]) && l.iter().all(|&a| (a as usize) < 1024));
                x.fill(0.0);
                g.encode(d, &mut x);
                assert_bounded(&x, e, i, d as u8);
                if l.len() >= 2 {
                    searched[(g.phase() == Phase::Build) as usize] += 1;
                }
                if g.phase() == Phase::Build && !g.build(d as u8).unwrap().cards_done() {
                    decks[(min != DECK_MIN as u32) as usize][2] += 1;
                    decks[(min != DECK_MIN as u32) as usize][3] += l.contains(&DONE) as u64;
                    assert_eq!(l.contains(&DONE), g.build(d as u8).unwrap().pool_cards() >= min as usize);
                }
                if i % 40 == 17 {
                    for v in [d as u8, rng.below(players as u32) as u8] {
                        let mut w = g.clone();
                        w.determinize(v as usize, &mut zrng);
                        w.check_invariants().unwrap_or_else(|err| panic!("event {e}: world for {v}: {err}"));
                        let mask = g.knowing(v);
                        assert_eq!(w.knowledge(mask), g.knowledge(mask));
                        let xw = enc(&w, v);
                        assert_bounded(&xw, e, i, v);
                        assert_eq!(xw, enc(&g, v), "event {e} step {i}: viewer {v}");
                        assert_eq!(w.to_move(), g.to_move());
                        if v == d as u8 {
                            assert_eq!(legal(&w), l);
                        }
                        // the world plays on to its terminal
                        if rng.below(8) == 0 {
                            finish_random(&mut w, &mut rng);
                            w.check_invariants().unwrap();
                        }
                        worlds += 1;
                    }
                }
                if i == check_at {
                    for &a in &l {
                        let mut w = g.clone();
                        w.step(a, &mut keys).unwrap();
                        finish_random(&mut w, &mut rng);
                        w.check_invariants().unwrap();
                        assert!(w.deferred());
                        options_done += 1;
                    }
                }
                let a = l[rng.below(l.len() as u32) as usize];
                keys.clear();
                g.step(a, &mut keys).unwrap();
                g.check_invariants().unwrap_or_else(|err| panic!("event {e} step {i}: {err}"));
                i += 1;
            }
            steps += i;
            assert!(g.deferred());
            for s in 0..players as u8 {
                let b = g.build(s).unwrap();
                assert!(b.pool_cards() >= min as usize && b.size() == DECK_SIZE);
                decks[(min != DECK_MIN as u32) as usize][0] += 1;
                decks[(min != DECK_MIN as u32) as usize][1] += b.pool_cards() as u64;
            }
            let n = g.matches().unwrap().len();
            if e % 50 == 7 {
                // Random decks are playable: the heuristic plays the event's matches
                // (one at a time: a panic of the `mtg` engine is recorded with the
                // match's spec, and fails the test at the end).
                let (max_turns, format) = (g.ev.max_turns, g.ev.format);
                let mut res = Vec::new();
                for (i, setup) in g.matches().unwrap().iter().enumerate() {
                    let played = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        draft_bots::play_match_heuristic(setup, max_turns, format, e, i)
                    }));
                    match played {
                        Ok(Ok(m)) => res.push(m.result),
                        Ok(Err(err)) => panic!("event {e} match {i}: {err}"),
                        Err(p) => {
                            let msg = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
                            let short = msg.chars().take(160).collect::<String>();
                            let spec = draft_bots::match_spec(setup, max_turns, format);
                            hooks.match_panic(e, i, &short, &spec);
                            match_panics.push(format!("event {e} match {i} (seed {e}): {}\n    spec: {}", short, spec));
                            res.push([0.5, 0.5]);
                        }
                    }
                }
                heuristic_matches += res.len() as u64;
                g.resolve(&res).unwrap();
            } else {
                let res: Vec<[f32; 2]> = (0..n).map(|_| [[1.0, 0.0], [0.0, 1.0], [0.5, 0.5]][rng.below(3) as usize]).collect();
                g.resolve(&res).unwrap();
            }
            let r = g.result();
            assert!((r.iter().sum::<f32>() - 1.0).abs() < 1e-5);
            outcome = Some(SeedOutcome { fingerprint: 0, result: format!("{:?}", g.outcome().unwrap()), turns: 0, decisions: i });
            team_wins[match g.outcome().unwrap().team[0] {
                x if x == 1.0 => 0,
                x if x == 0.0 => 1,
                _ => 2,
            }] += 1;
        }));
        match one {
            Err(payload) => return EventsEnd { run: Run::Failed { seed: e, payload }, match_panics: match_panics.len() },
            Ok(()) => hooks.passed(e, outcome.as_ref().expect("an event that passed has its outcome")),
        }
    }
    println!(
        "{events} events, {steps} actions, searched picks {} deckbuilding {}, {worlds} worlds, {options_done} options completed, {heuristic_matches} heuristic-played matches, \
         team A wins / losses / draws {team_wins:?}, {:.1} s",
        searched[0],
        searched[1],
        t0.elapsed().as_secs_f64()
    );
    for p in &match_panics {
        println!("MTG ENGINE PANIC in a heuristic-played match: {p}");
    }
    for (k, what) in ["minimum 22 (the default)", "minimum 0, 40 or random"].iter().enumerate() {
        let [n, cards, choices, done] = decks[k];
        println!(
            "random decks, {what}: {n}, {:.2} pool cards each, {:.2} card choices per seat ({:.2} with DONE offered)",
            cards as f64 / n.max(1) as f64,
            choices as f64 / n.max(1) as f64,
            done as f64 / n.max(1) as f64
        );
    }
    EventsEnd { run: Run::Done, match_panics: match_panics.len() }
}
