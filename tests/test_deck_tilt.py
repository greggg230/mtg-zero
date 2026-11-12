"""The deck tilt (docs/ENGINE.md 106): the search deals the opponent's unseen deck from the deck prediction, zero-core's
per-target tilt `belief_tilts.deck` (its docs/search.md 3.8.1). The presets, the run's seats, a deck-tilted deal through
the bindings (the decider's view, options and encoding kept; a function of the seed and the prediction), and the refusal
on open decklists."""
import json

import numpy as np
import pytest

from mtgz import az as P
from mtgz.az.games import row
from zero import games as G
from zero import run
from zero.run import DEFAULTS, Run

D = 1024


def test_the_presets_and_seats():
    smoke, big = P.PRESETS['eoe-smoke'], P.PRESETS['eoe-1']
    assert smoke['selfplay_belief_tilts'] == smoke['eval_belief_tilts'] == {'deck': 1.0}
    assert big['selfplay_belief_tilts'] == big['eval_belief_tilts'] == {'deck': 1.0}
    for name in ('eoe-smoke', 'eoe-1'):
        c = run.resolve_config(run.parse(['--run', 'x', '--preset', name]), {})
        assert c['selfplay_belief_tilt'] == c['eval_belief_tilt'] == 0.0, 'the hand tilt stays off'
        r = type('R', (), {'cfg': c})()
        for selfplay in (True, False):
            s = Run.nn_seat(r, 'm', 16, selfplay=selfplay)
            assert s['belief_tilt'] == 0.0 and s['belief_tilts'] == c['selfplay_belief_tilts' if selfplay else 'eval_belief_tilts']
    # every other preset: no deck tilt (the seats of before), but the constructed best-of-three's (docs/ENGINE.md
    # 120: the tilt deals their later games' hidden 60s; tests/test_delver_bo3.py)
    for name, p in P.PRESETS.items():
        if p.get('game') != 'mtg_eoe' and name not in ('delver-mirror-bo3', 'delver-bo3-smoke'):
            assert not p.get('selfplay_belief_tilts') and not p.get('eval_belief_tilts'), name
    assert DEFAULTS['selfplay_belief_tilts'] == DEFAULTS['eval_belief_tilts'] == {}


def _position(spec, seed, game='mtg_eoe'):
    g = G.engine().Game(game, spec, 2, seed)
    rng = np.random.default_rng(seed)
    while json.loads(g.export_state())['turn'] < 3:
        legal = list(g.legal())
        g.step(int(legal[rng.integers(len(legal))]))
    return g


def test_a_deck_tilted_deal_through_the_bindings():
    g = _position(P.SPEC_EOE, 11)
    v = g.to_move
    x, legal = row(g, v), list(g.legal())
    rng = np.random.default_rng(1)
    pred = rng.random(2 * D).astype(np.float32) ** 6
    pred[:D] /= pred[:D].sum()
    pred[D:] /= pred[D:].sum()
    decks = set()
    for s in range(24):
        w = g.clone()
        w.determinize_with(v, s, pred, 0.0, 20.0, {'deck': 1.0})
        assert np.array_equal(row(w, v), x) and list(w.legal()) == legal
        again = g.clone()
        again.determinize_with(v, s, pred, 0.0, 20.0, {'deck': 1.0})
        assert np.array_equal(np.asarray(w.aux_target(v)), np.asarray(again.aux_target(v))), 'a function of the seed'
        decks.add(np.asarray(w.aux_target(v))[D:].tobytes())
    assert len(decks) > 1
    with pytest.raises(ValueError):
        g.clone().determinize_with(v, 1, pred, 0.0, 20.0, {'hand': 1.0})        # not a target of the game


def test_the_deck_tilt_on_open_decklists_is_refused():
    g = _position(P.SPEC, 12, game='mtg')
    pred = np.full(2 * D, 1.0 / D, np.float32)
    with pytest.raises(BaseException, match='the deck tilt deals hidden decklists'):
        g.clone().determinize_with(g.to_move, 1, pred, 0.0, 20.0, {'deck': 1.0})
