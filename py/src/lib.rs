// Python module mtgz._engine: zero-core's PyO3 classes for the toy, MTG and the team draft
// (zero-core docs/search.md 4; the games: engine/src/az, docs/ENGINE.md 21 and 41), and the sealed
// helpers (docs/ENGINE.md 101: the pool generator, the builder, a sealed spec's seats, the EoE
// table's names; `mtgz.sealed.engine` wraps them). The module is zero-core's `engine_module!`
// spelled out, so it can add the sealed functions after zero-core's.
// mimalloc for every Rust allocation in this module (the engine, the search trees, the driver):
// faster than the Windows heap for the engine's many small allocations (docs/ENGINE.md 17).
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

static GAMES: &[zero_py::GameEntry] = &[
    zero_py::GameEntry::of::<zero_search::toy::ToyGame>("toy"),
    zero_py::GameEntry::of::<engine::az::MtgAz>("mtg"),
    zero_py::GameEntry::of::<engine::az::draft::TeamDraftAz>("team_draft"),
    // Edge of Eternities sealed: `mtg` on the EoE table, its own identity space (ENGINE.md 105)
    zero_py::GameEntry::of::<engine::az::eoe::MtgEoeAz>("mtg_eoe"),
];

#[pymodule]
fn _engine(m: &Bound<'_, PyModule>) -> PyResult<()> {
    zero_py::init_module(m, GAMES)?;
    m.add_function(wrap_pyfunction!(sealed_pool, m)?)?;
    m.add_function(wrap_pyfunction!(sealed_build, m)?)?;
    m.add_function(wrap_pyfunction!(sealed_seats, m)?)?;
    m.add_function(wrap_pyfunction!(sealed_candidates, m)?)?;
    m.add_function(wrap_pyfunction!(eoe_lookup, m)?)?;
    m.add_function(wrap_pyfunction!(game_log, m)?)?;
    m.add_function(wrap_pyfunction!(game_log_path, m)?)?;
    Ok(())
}

fn verr(e: String) -> PyErr {
    PyValueError::new_err(e)
}

/// A generated EoE sealed pool (the Rust port of `mtgz.sealed.collation.generate`), as JSON:
/// {"boosters": [[[slot, name], ...], ...], "promo", "cards", "seed", "collation"}.
#[pyfunction]
#[pyo3(signature = (seed, collation, boosters=6, prerelease_promo=false, paradox_haze=false))]
fn sealed_pool(seed: u64, collation: &str, boosters: usize, prerelease_promo: bool, paradox_haze: bool) -> PyResult<String> {
    engine::az::sealed::pool_json(seed, collation, boosters, prerelease_promo, paradox_haze).map_err(verr)
}

/// The randomised sealed builder on a pool of card names (seeded; `settings` a JSON object or
/// ""), as JSON: {"deck", "sideboard", "colors", "splash", "lands", ...}.
#[pyfunction]
#[pyo3(signature = (pool, seed, settings=""))]
fn sealed_build(pool: Vec<String>, seed: u64, settings: &str) -> PyResult<String> {
    engine::az::sealed::build_names_json(&pool, seed, settings).map_err(verr)
}

/// The two seats (pool, deck, sideboard, build) of a sealed `mtg` spec's game with this seed,
/// as JSON.
#[pyfunction]
fn sealed_seats(spec: &str, seed: u64) -> PyResult<String> {
    engine::az::sealed::seats_json(spec, seed).map_err(verr)
}

/// A deck-choice spec's candidates of a game (docs/ENGINE.md 107.2), as JSON: per seat {"pool_seed",
/// "explore": j or null, "candidates": [build, ...]} (null for a given deck).
#[pyfunction]
fn sealed_candidates(spec: &str, seed: u64) -> PyResult<String> {
    engine::az::sealed::candidates_json(spec, seed).map_err(verr)
}

/// Each name's definition in the EoE card table, or why it has none, as JSON {name: id | error}.
#[pyfunction]
fn eoe_lookup(names: Vec<String>) -> String {
    engine::az::sealed::lookup_json(&names)
}

/// The EoE sealed game log (docs/ENGINE.md 107.1): every finished `mtg_eoe` game started while it is
/// set appends a JSON line to `path` (its directory created); None stops it.
#[pyfunction]
#[pyo3(signature = (path=None))]
fn game_log(path: Option<&str>) -> PyResult<()> {
    engine::az::game_log::set(path).map_err(verr)
}

/// The game log's file, or None while it is off.
#[pyfunction]
fn game_log_path() -> Option<String> {
    engine::az::game_log::path()
}
