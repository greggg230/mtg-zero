// Python module mtgz._engine: zero-core's PyO3 classes for the toy and MTG (zero-core
// docs/search.md 4; the game: engine/src/az, docs/ENGINE.md 21).
zero_py::engine_module!(_engine, {
    "toy" => zero_search::toy::ToyGame,
    "mtg" => engine::az::MtgAz,
});
