// Python module mtgz._engine: zero-core's PyO3 classes for the toy, MTG and the team draft
// (zero-core docs/search.md 4; the games: engine/src/az, docs/ENGINE.md 21 and 41).
// mimalloc for every Rust allocation in this module (the engine, the search trees, the driver):
// faster than the Windows heap for the engine's many small allocations (docs/ENGINE.md 17).
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

zero_py::engine_module!(_engine, {
    "toy" => zero_search::toy::ToyGame,
    "mtg" => engine::az::MtgAz,
    "team_draft" => engine::az::draft::TeamDraftAz,
});
