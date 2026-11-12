"""Sealed deckbuilding data (DESIGN.md 9, milestone S2; docs/sealed_data.md).

* `mtgz.sealed.cards`: the Edge of Eternities card list (`data/sealed/eoe_set.json`: sheet,
  rarity, colours), the engine's card table and its support list.
* `mtgz.sealed.seventeen_lands`: the 17lands EoE sealed pools, builds and games
  (`data/sealed/eoe_17lands_sealed.json.gz`). A YARDSTICK ONLY (DESIGN.md 0): never a training
  target, reward or prior.
* `mtgz.sealed.collation`: a seeded sealed-pool generator with two EoE Play Booster collations,
  `paper` (Wizards' published booster) and `arena` (Arena's boosters as 17lands shows them).

Pure Python (no engine module needed). With the engine module (`tools/build_py.sh`):

* `mtgz.sealed.engine`: the Rust collations, builder and sealed seats (docs/ENGINE.md 101).
* `mtgz.sealed.yardstick`: the 17lands pools and human builds as sealed specs (ENGINE.md 101.4).
* `mtgz.sealed.compare`: paired deck comparison and racing against a field, the S4 verify step and
  the S5 yardstick (ENGINE.md 104; `python -m mtgz.sealed.compare`).
* `mtgz.sealed.batch`: every pool's and build's race games in one batched driver, with an evaluator
  whose answers depend on each row alone (ENGINE.md 110; `compare ... --batched`).
* `mtgz.sealed.evaluator`: S4a, the deck evaluator trained on a run's logged games, card ranking v0 and
  the picker (ENGINE.md 108; `python -m mtgz.sealed.evaluator`); v2's pairwise kind (P(deck A beats deck
  B), scores against any named field), recency weighting and the torch-free export with its numpy
  reference scorer (ENGINE.md 111).
* `mtgz.sealed.scorer`: the engine's scorer of that export (the sealed spec's deck choice by an evaluator) checked
  against the numpy reference and torch (ENGINE.md 112; `python -m mtgz.sealed.scorer`).
* `mtgz.sealed.rolling`: a run's evaluator versions, retrained in a background job, and the deck-choice evaluation
  (ENGINE.md 112: zero-core's game-package hooks; `python -m mtgz.sealed.rolling`).
"""
