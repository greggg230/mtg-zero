"""The stew (docs/ENGINE.md 239): specialists trained by self-play on one pairing each, deck stews and a main stew
distilled upward from them, judged every generation. `tools/stew.py` is the command line.

* `config`   - stew.json: the field, the candidates, the source run, the recipe, the code; the pairings and shares.
* `state`    - state.json: the nets' lineage and the phase machine (atomic writes).
* `control`  - the real operations: run control (fork_run, supervise, pause_run), the farm coordinator, distill,
               the judge's match processes. Tests replace it with fakes.
* `sources`  - the distillation sources files of the deck stews and the main stew (the contract of 239.1).
* `judge`    - the judging: matches where the deck follows the net, best-of-three units on common deals, the
               statistics, the matchup table, the reports.
* `orchestrator` - the phases and the loop.
"""
