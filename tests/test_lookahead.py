"""The exhaustive lookahead per information set on MTG (docs/ENGINE.md 80; zero-core docs/search.md 3.9):
the position API on Delver positions (`mtgz.az.lookahead.evaluate_position`: its lines' labels and
reveals), the hooks it needs (`minor_decision`, `active_seat`), a lookahead seat in a match through
`mtgz.az.h2h` (zero.h2h's --a-lookahead), and its settings refused when bad."""
import json

import numpy as np
import pytest

import mtgz.az as P
from mtgz.az import games as G
from mtgz.az import lookahead as LA
from mtgz.az.infer import UniformEvaluator

SPEC = P.SPEC_DELVER


class Ev:
    """UniformEvaluator behind NetEvaluator.evaluate's contract (the position API calls that), with a
    belief row's auxiliary output uniform over the aux width."""

    def __init__(self):
        self.u = UniformEvaluator()
        self.w = G.info('mtg')['aux_width']

    def evaluate(self, x, n_players, opt_off, opt_action, extra=None):
        lb = type('LB', (), {'opt_off': opt_off, 'B': len(n_players), 'n_players': np.asarray(n_players)})()
        pri, val, mar = self.u(lb)
        nb = 0 if extra is None else int((np.asarray(extra) == 2).sum())
        if nb:
            return pri, val, mar, np.full((nb, self.w), 1.0 / self.w, np.float32)
        return pri, val, mar


def a_position(seed=4, steps=30):
    """A Delver position some moves in (the first decision with >= 3 options after `steps`)."""
    from zero import mcts
    g = G.engine().Game('mtg', SPEC, 2, seed)
    rng = np.random.default_rng(seed)
    k = 0
    while not g.over:
        legal = g.legal()
        if k >= steps and len(legal) >= 3:
            return g
        mcts.transition(g, int(legal[int(rng.integers(len(legal)))]))
        k += 1
    raise AssertionError('the game ended first')


def test_evaluate_position_on_a_delver_position_with_labels_and_reveals():
    g = a_position()
    st = {'plies': 2, 'worlds': 3}
    r = LA.evaluate_position(g, Ev(), st, seeds=[1, 2, 3])
    labels = {int(o['a']): o['t'] for o in json.loads(g.export_state())['decision']['options']}
    assert sorted(o['action'] for o in r['options']) == sorted(g.legal())
    for o in r['options']:
        assert o['label'] == labels[o['action']] and o['n'] == 3
        assert o['line'][0]['a'] == o['action'] and all(isinstance(e['label'], str) for e in o['line'])
    assert r['outcome'] == r['options'][0]['action']
    # the belief from the evaluator's auxiliary output: one row, the worlds dealt by it
    rb = LA.evaluate_position(g, Ev(), st, seeds=[1, 2, 3], belief_tilt=1.0, belief_mix=0.25)
    assert rb['belief'] is not None and len(rb['belief']) == G.info('mtg')['aux_width']
    assert LA.reveal([(1 << 56) | (0 << 48) | 7, (3 << 56) | (1 << 48) | 2, (4 << 56)]) == ['P1 drew', 'P2 looked']


def test_the_hooks_the_lookahead_needs():
    g = a_position(seed=9, steps=12)
    st = json.loads(g.export_state())
    # (the 'next_turn' horizon reads the active seat: a Delver game has turns)
    r = LA.evaluate_position(g, Ev(), {'horizon': 'next_turn', 'max_plies': 3, 'worlds': 2}, seeds=[1, 2])
    assert r['stats']['nodes'] > 0
    assert st['active'] in (0, 1)


def test_a_lookahead_seat_in_a_match(tmp_path, capsys):
    from zero.net import make_net, save
    m = make_net(G.info('mtg'), 'mtg', 'tokens', 16, 1, heads=2)
    path = str(tmp_path / 'm.pt')
    save(m, path, {'iter': 0})
    from mtgz.az import h2h
    h2h.main([path, path, '--games', '2', '--sims', '2', '--chance-k', '0', '--threads', '2', '--paired', '--spec', SPEC,
              '--a-lookahead', 'plies=1,worlds=2,budget=5000', '--select', 'visits'])
    out = json.loads(capsys.readouterr().out.strip().splitlines()[-1])
    assert out['games'] == 2 and out['lookahead_a']['plies'] == 1 and out['look']['decisions'] > 0
    with pytest.raises(SystemExit):
        h2h.main([path, path, '--games', '2', '--sims', '2', '--a-lookahead', 'plies=1,wolds=2'])
