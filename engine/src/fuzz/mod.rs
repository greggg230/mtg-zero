//! The engine's fuzz loops as library code (docs/ENGINE.md 114.1): the random-play fuzzer of games
//! and best-of-three matches ([`games`], from `engine/tests/fuzz.rs`) and the random `team_draft`
//! events ([`draft`], from `engine/tests/draft.rs`), with the fixtures they share ([`cards`], from
//! `engine/tests/common`).
//!
//! The tests `fuzz_random_games`, `fuzz_random_matches` and `fuzz_random_events` are thin wrappers
//! over [`games::random_games`], [`games::random_matches`] and [`draft::random_events`]; zero-farm's
//! worker (`mtgz-worker`, a fuzz job's chunks) calls the same functions. So the same seeds play the
//! same games, with the same fingerprints and the same printed lines, under `cargo test` and on a
//! friend's PC; `tools/fuzz_farm.py` drives the tests unchanged.
//!
//! A run is (settings, first seed, count): the settings are the `FUZZ_*` variables the module docs
//! of [`games`] list, read through [`env::var`] - the process environment (the tests), or a farm job's
//! own settings ([`env::use_only`], the worker's chunk process). Each seed's outcome goes to a
//! [`Hooks`] as it ends; a seed whose play panics ends the run there ([`Run::Failed`]), as the test
//! stopped at it.

pub mod cards;
pub mod draft;
pub mod games;

/// The fuzz loops' settings.
pub mod env {
    use std::collections::BTreeMap;
    use std::sync::OnceLock;

    static ONLY: OnceLock<BTreeMap<String, String>> = OnceLock::new();

    /// Setting `k`: `std::env::var(k)`, or, once [`use_only`] was called in this process, `k` in its
    /// settings (absent: `NotPresent`; the process environment is not read then).
    pub fn var(k: &str) -> Result<String, std::env::VarError> {
        match ONLY.get() {
            Some(m) => m.get(k).cloned().ok_or(std::env::VarError::NotPresent),
            None => std::env::var(k),
        }
    }

    /// From now on this process's fuzz settings are `settings` and nothing else (a farm job's `env`,
    /// in the worker's chunk process). Once per process.
    pub fn use_only(settings: BTreeMap<String, String>) -> Result<(), String> {
        ONLY.set(settings).map_err(|_| "the fuzz settings of this process are already set".to_string())
    }
}

/// One seed's outcome (a game, a match or a draft event that passed every check).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeedOutcome {
    /// The final state's fingerprint (0: a draft event, or a seed not played).
    pub fingerprint: u64,
    /// The result as `{:?}` prints it (a game's `Outcome`, a match's results, an event's
    /// `EventOutcome`), or why the seed was not played.
    pub result: String,
    /// A game's turns, a match's games; 0 for an event.
    pub turns: u64,
    /// Actions taken.
    pub decisions: u64,
}

impl SeedOutcome {
    /// A seed the loop leaves out (`FUZZ_SKIP`, a sealed pool this checkout can't play).
    pub fn not_played(why: &str) -> SeedOutcome {
        SeedOutcome { fingerprint: 0, result: why.to_string(), turns: 0, decisions: 0 }
    }
}

/// What a caller sees of a run, seed by seed.
pub trait Hooks {
    /// Before seed `seed` (also one the loop leaves out): false stops the run here ([`Run::Stopped`];
    /// nothing more is played and no report is printed).
    fn before(&mut self, _seed: u64) -> bool {
        true
    }
    /// Seed `seed` passed (or was left out: [`SeedOutcome::not_played`]).
    fn passed(&mut self, _seed: u64, _o: &SeedOutcome) {}
    /// A heuristic-played match of draft event `event` panicked in the `mtg` engine (`msg`: its
    /// panic message's first 160 characters, `spec`: the match's spec); the event goes on with a
    /// drawn result ([`draft::random_events`]).
    fn match_panic(&mut self, _event: u64, _match: usize, _msg: &str, _spec: &str) {}
}

/// The tests' hooks: nothing.
pub struct NoHooks;

impl Hooks for NoHooks {}

/// How a run ended.
pub enum Run {
    /// Every seed passed; the chunk's report was printed and its checks held.
    Done,
    /// Seed `seed` panicked (`payload`, as `catch_unwind` gave it); nothing after it was played.
    Failed { seed: u64, payload: Box<dyn std::any::Any + Send> },
    /// [`Hooks::before`] stopped the run before seed `at`.
    Stopped { at: u64 },
}
