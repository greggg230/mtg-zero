"""zero.games (game types of `mtgz._engine`, seats, external players, the driver wrapper) for
MTG Zero. MTG has no external player: its yardsticks are the built-in Rust bots `heuristic` and
`random` (engine/src/az/bots.rs).

Seats (one per player of a game), as strings in run settings:
  * 'nn' / an `NNSeat` dict  - a network seat (search settings, evaluator key)
  * 'random', 'heuristic'    - the built-in Rust bots
"""
from zero.games import *  # noqa: F401,F403
from zero.games import EXTERNALS, LeafBatch, MAX_SEATS, engine, external, register_external  # noqa: F401
