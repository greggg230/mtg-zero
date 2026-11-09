"""Sealed deckbuilding data (DESIGN.md 9, milestone S2; docs/sealed_data.md).

* `mtgz.sealed.cards`: the Edge of Eternities card list (`data/sealed/eoe_set.json`: sheet,
  rarity, colours), the engine's card table and its support list.
* `mtgz.sealed.seventeen_lands`: the 17lands EoE sealed pools, builds and games
  (`data/sealed/eoe_17lands_sealed.json.gz`). A YARDSTICK ONLY (DESIGN.md 0): never a training
  target, reward or prior.
* `mtgz.sealed.collation`: a seeded sealed-pool generator with two EoE Play Booster collations,
  `paper` (Wizards' published booster) and `arena` (Arena's boosters as 17lands shows them).

Pure Python (no engine module needed).
"""
