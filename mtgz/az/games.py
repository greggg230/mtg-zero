"""zero.games (game types of `mtgz._engine`, seats, external players, the driver wrapper) for
MTG Zero. MTG has no external player: its yardsticks are the built-in Rust bots `heuristic` and
`random` (engine/src/az/bots.rs).

Seats (one per player of a game), as strings in run settings:
  * 'nn' / an `NNSeat` dict  - a network seat (search settings, evaluator key)
  * 'random', 'heuristic'    - the built-in Rust bots
"""
from zero.games import *  # noqa: F401,F403
from zero.games import EXTERNALS, LeafBatch, MAX_SEATS, engine, external, register_external  # noqa: F401

def row(g, viewer):
    """`viewer`'s whole row of game `g` as one flat float64 array (docs/ENGINE.md 125): the head (`encode_row`'s, layout
    v7; a game without tail segments: `encode`), each tail token's segment, slot and floats, then the pointer
    overrides of its options. Equal rows, equal arrays: the hidden-information checks compare worlds by it (`encode`
    refuses a row with a tail token: zero-core R1)."""
    import numpy as np
    try:
        x, t = g.encode_row(viewer)
    except AttributeError:
        return np.asarray(g.encode(viewer), np.float64)
    parts = [np.asarray(x, np.float64), [-1e9]]
    seg, slot, val = np.asarray(t['t_seg']), np.asarray(t['t_slot']), np.asarray(t['t_val'], np.float64)
    parts += [seg.astype(np.float64), slot.astype(np.float64), val, [-2e9]]
    if g.to_move is not None and g.to_move == viewer:
        o, k = g.option_overrides(viewer)
        parts += [np.asarray(o, np.float64), np.asarray(k, np.float64)]
    return np.concatenate([np.asarray(p, np.float64) for p in parts])
