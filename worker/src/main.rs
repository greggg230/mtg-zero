//! mtgz-worker: MTG Zero's native worker (zero-farm; zero-core's `zero_worker`, docs/az.md 42
//! there; docs/ENGINE.md 114 here). The games `py/src/lib.rs` registers for Python, without PyO3, and
//! the engine's fuzz loops (`fuzz`: engine::fuzz, ENGINE.md 114.10): `mtgz-worker serve` (the
//! agent<->worker protocol aw/1), `selfplay ...`, `fuzz ...`, `fuzz-check ...`, `check-config
//! <config.json>`, `driver-cfg`, `eval-batches`, `info` (zero_worker::proto).
//!
//! The release build runs self-play and fuzz jobs; the fuzz-profile build (release + debug assertions
//! + overflow checks) runs fuzz jobs only.
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

static WORKER: zero_worker::Worker = zero_worker::Worker {
    games: GAMES,
    repo: "mtg",
    // (a fuzz-profile build's debug assertions make self-play slow: it fuzzes only)
    selfplay: !matches!(fuzz::PROFILE.as_bytes(), b"fuzz"),
    fuzz: Some(&fuzz::MtgFuzz),
};

fn main() {
    std::process::exit(zero_worker::main_with(&WORKER))
}
