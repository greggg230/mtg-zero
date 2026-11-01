"""`mtgz.az.lookahead`: zero-core's exhaustive lookahead per information set (zero.lookahead of the pinned
core, its docs/search.md 3.9; docs/ENGINE.md 80) for MTG Zero - the settings (`parse`, `settings`, `tag`),
the position API `evaluate_position` (the Delver Lab's call: per root option its value per dealt world,
mean +- sd, the modal line with labels) with MTG's reveals in the lines (`reveal`: the draws and looks a
move showed, from its chance keys, engine/src/hidden.rs).

A lookahead seat in a match: `python -m mtgz.az.h2h A B --a-lookahead plies=6,worlds=8,opp=best,budget=200000
...` (zero.h2h's flags). The game's hooks: `minor_decision` (mana payments: the network's top prior, no ply),
`active_seat` (the horizon 'next_turn'), `determinize_with` (the worlds dealt by the belief head).
"""
import mtgz.az  # noqa: F401  (registers the game package)
from zero import lookahead as core
from zero.lookahead import KEYS, drive, parse, replay_line, settings, tag  # noqa: F401

# engine/src/hidden.rs: a chance key = kind << 56 | player << 48 | payload
KEY_DRAW, KEY_REVEAL, KEY_LOOK = 1, 2, 3


def reveal(keys, player=None):
    """What a move showed, from its chance keys - the draws and looks, as the lab's lines annotate them
    (delver-lab `xply.EngineOps.step`): ['P1 drew', 'P2 looked'] (who saw it; the card stays in the
    replayed world: a line is one world's). Public reveals (a card played from a hand) are left out."""
    out = []
    for k in keys:
        kind, p = int(k) >> 56, (int(k) >> 48) & 0xFF
        if kind == KEY_DRAW:
            out.append(f'P{p + 1} drew')
        elif kind == KEY_LOOK:
            out.append(f'P{p + 1} looked')
    return out


def evaluate_position(game, ev, settings_, **kw):
    """zero.lookahead.evaluate_position with MTG's `reveal` in the lines (unless given)."""
    kw.setdefault('reveal', reveal)
    return core.evaluate_position(game, ev, settings_, **kw)
