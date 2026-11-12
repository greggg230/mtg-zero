//! mtgz-worker: MTG Zero's native self-play worker (zero-farm; zero-core's `zero_worker`, docs/az.md 42
//! there; docs/ENGINE.md 114 here). The games `py/src/lib.rs` registers for Python, without PyO3:
//! `mtgz-worker serve` (the agent<->worker protocol aw/1), `selfplay ...`, `check-config <config.json>`,
//! `driver-cfg`, `eval-batches`, `info` (zero_worker::proto).
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

static GAMES: &[zero_worker::GameEntry] = &[
    zero_worker::GameEntry::of::<zero_search::toy::ToyGame>("toy"),
    zero_worker::GameEntry::of::<engine::az::MtgAz>("mtg"),
    zero_worker::GameEntry::of::<engine::az::draft::TeamDraftAz>("team_draft"),
    // Edge of Eternities sealed: `mtg` on the EoE table, its own identity space (ENGINE.md 105)
    zero_worker::GameEntry::of::<engine::az::eoe::MtgEoeAz>("mtg_eoe"),
];

fn main() {
    std::process::exit(zero_worker::main(GAMES, "mtg"))
}
