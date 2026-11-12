//! mtgz-worker: MTG Zero's native worker (zero-farm; zero-core's `zero_worker`, docs/az.md 42
//! there; docs/ENGINE.md 114 here). The games `py/src/lib.rs` registers for Python, without PyO3, and
//! the engine's fuzz loops (`fuzz`: engine::fuzz, ENGINE.md 114.1): `mtgz-worker serve` (the
//! agent<->worker protocol aw/1), `selfplay ...`, `eval ...`, `fuzz ...`, `fuzz-check ...`, `check-config
//! <config.json> [--eval]`, `driver-cfg`, `eval-batches`, `info` (zero_worker::proto).
//!
//! The release build runs self-play, eval (ENGINE.md 114.4) and fuzz jobs; the fuzz-profile build (release
//! + debug assertions + overflow checks) runs fuzz jobs only.
mod fuzz;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

static GAMES: &[zero_worker::GameEntry] = &[
    zero_worker::GameEntry::of::<zero_search::toy::ToyGame>("toy"),
    zero_worker::GameEntry::of::<engine::az::MtgAz>("mtg"),
    zero_worker::GameEntry::of::<engine::az::draft::TeamDraftAz>("team_draft"),
    // Edge of Eternities sealed: `mtg` on the EoE table, its own identity space (ENGINE.md 105)
    zero_worker::GameEntry::of::<engine::az::eoe::MtgEoeAz>("mtg_eoe"),
];

/// The game package's own run settings (`mtgz.az`'s `GamePackage.settings`: `mtgz.sealed.rolling.SETTINGS` and
/// `REAL_DECKS_SETTINGS`, ENGINE.md 112 / 113) and how a worker treats them (check-config; tests/test_worker_farm.py
/// checks the list against the package's).
static PACKAGE_SETTINGS: zero_worker::classify::PackageSettings = &[
    ("evaluator_every", "python_only", "the deck evaluator retrained inside the run: the selfplay_specs hook (mtgz.sealed.rolling, Python) changes the self-play spec between phases; remote self-play refuses it too"),
    ("evaluator_window", "param:evaluator_every", "the retrain's window"),
    ("evaluator_min_window", "param:evaluator_every", "the retrain's first window"),
    ("evaluator_val_iters", "param:evaluator_every", "the retrain's validation iterations"),
    ("evaluator_kind", "param:evaluator_every", "the retrained model"),
    ("evaluator_pair_base", "param:evaluator_every", "a pair evaluator's strength model"),
    ("evaluator_threads", "param:evaluator_every", "the retrain's threads"),
    ("evaluator_half_life", "param:evaluator_every", "the retrain's recency weighting"),
    ("deckchoice_eval_games", "trainer", "the deck-choice evaluation of an evaluated iteration (on the desktop)"),
    ("real_decks_every", "trainer", "the extra evaluation against real decks (on the desktop)"),
    ("real_decks_pools", "trainer", "the extra evaluation against real decks"),
    ("real_decks_games", "trainer", "the extra evaluation against real decks"),
    ("real_decks_builder", "trainer", "the extra evaluation against real decks"),
];

static WORKER: zero_worker::Worker = zero_worker::Worker {
    games: GAMES,
    repo: "mtg",
    // (a fuzz-profile build's debug assertions make self-play slow: it fuzzes only)
    selfplay: !matches!(fuzz::PROFILE.as_bytes(), b"fuzz"),
    fuzz: Some(&fuzz::MtgFuzz),
    package_settings: PACKAGE_SETTINGS,
};

fn main() {
    std::process::exit(zero_worker::main_with(&WORKER))
}
