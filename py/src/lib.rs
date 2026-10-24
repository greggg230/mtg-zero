// Python module mtgz._engine: zero-core's PyO3 classes for the toy and MTG (zero-core
// docs/search.md 4; the game: engine/src/az, docs/ENGINE.md 21).
// mimalloc for every Rust allocation in this module (the engine, the search trees, the driver):
// faster than the Windows heap for the engine's many small allocations (docs/ENGINE.md 17).
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

zero_py::engine_module!(_engine, {
    "toy" => zero_search::toy::ToyGame,
    "mtg" => engine::az::MtgAz,
});
