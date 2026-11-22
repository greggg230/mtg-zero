"""tools/rowkit.py and the analysis tools on it (docs/ENGINE.md 125): on a game with variable-length rows (layout v7's
`mtg`) every row a tool evaluates is whole - its head, its tail tokens and, for a decider's row, its pointer overrides -
and on a game without them (a layout v5 / v6 engine's `mtg`; here `mtg_eoe`, layout v6, and the toy) the helpers take
the call of before. The tools' searches (root_view, gumbel_eval, mull_floor, belief_search, delver_blunders) run on a
Delver mirror position whose rows have tail tokens, with a small token network of this build's layout."""
import importlib.util
import os
import sys

import numpy as np
import pytest
import torch

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import mtgz.az as A  # noqa: E402
from mtgz.az import games as G  # noqa: E402

import rowkit  # noqa: E402


def _tool(name):
    spec = importlib.util.spec_from_file_location(name, os.path.join(ROOT, 'tools', name + '.py'))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def delver_position(seed=3, steps=60):
    """A Delver mirror position after `steps` random actions whose decider's row has tail tokens."""
    E = G.engine()
    rng = np.random.default_rng(seed)
    g = E.Game('mtg', A.SPEC_DELVER, 2, seed)
    rk = rowkit.kind('mtg')
    k = 0
    while not g.over:
        legal = g.legal()
        if k >= steps and len(legal) >= 2 and len(rowkit.row(g, g.to_move, rk)[1][0]['t_seg']):
            return g
        g.step(int(legal[rng.integers(len(legal))]))
        k += 1
    raise AssertionError('no position with tail tokens')


@pytest.fixture(scope='module')
def ev():
    from zero.infer import NetEvaluator
    from zero.net import make_net
    torch.manual_seed(0)
    return NetEvaluator(make_net(G.info('mtg'), 'mtg', 'tokens', 16, 1, heads=2, aux=A.PACKAGE.aux_of('mtg')), 'cpu')


def test_the_rows_of_a_game_with_and_without_tails():
    rk = rowkit.kind('mtg')
    assert rk is not None and rk.widths and rk.pointers
    g = delver_position()
    x, rr = rowkit.row(g, g.to_move, rk, opts=True)
    assert x.shape == (G.info('mtg')['num_features'],) and len(rr[0]['t_seg']) > 0
    assert len(rr[1][0]) == len(rr[1][1])                            # (the overrides: option index, token)
    # without variable-length rows: `encode`, no rr
    for game, spec in (('mtg_eoe', A.SPEC_EOE), ('toy', None)):
        assert rowkit.kind(game) is None
        h = G.engine().Game(game, spec, 2, 1) if spec else G.engine().Game(game, None, 2, 1)
        x0, rr0 = rowkit.row(h, h.to_move, None)
        assert rr0 is None and np.array_equal(x0, np.asarray(h.encode(h.to_move), np.float32))
        assert rowkit.kw(None, [rr0]) == {}
        # (a search's leaves, the calls of before: no rows)
        for rv in (False, True):
            t = h.search_tree(True, 1)
            r = rowkit.simulate(t, h, 1.0, 0, rv, None)
            assert r is not None and r[4] is None and r[5] is None and (r[3] is None) == (not rv or r[0] == t.viewer)


def test_evaluate_takes_the_tails_and_overrides(ev):
    """A decider's row evaluated whole: its tail moves the answer (a row's head alone is another input)."""
    rk = rowkit.kind('mtg')
    g = delver_position()
    x, rr = rowkit.row(g, g.to_move, rk, opts=True)
    legal = list(g.legal())
    whole = rowkit.evaluate(ev, [x], [legal], rk, [rr])
    bare = rowkit.evaluate(ev, [x], [legal], rk, [({'t_seg': np.zeros(0, np.uint16), 't_slot': np.zeros(0, np.uint32),
                                                    't_val': np.zeros(0, np.float32)}, rr[1])])
    assert abs(float(np.sum(whole[0])) - 1) < 1e-4
    assert not np.array_equal(whole[1], bare[1]), 'the tail tokens reach the value'
    with pytest.raises(ValueError):
        ev.evaluate(x[None], np.full(1, 2, np.int32), np.asarray([0, len(legal)], np.int32), np.asarray(legal, np.int32))
    belief = rowkit.aux(ev, [x], rk, [rr])['belief']
    assert belief.shape == (1, {n: int(w) for n, w, _ in ev.aux_layout}['belief'])


def test_the_tools_searches_take_whole_rows(ev):
    g = delver_position()
    acts = set(int(a) for a in g.legal())
    rv = _tool('root_view')
    for view in ('mover', 'root'):
        trees = rv.search_all([g.clone(), g.clone()], ev, 6, view, [1, 2])
        assert all(t.root_tot >= 1 and set(t.root_acts) <= acts for t in trees)
    ge = _tool('gumbel_eval')
    trees = ge.search_all([g.clone()], ev, 6, 'root', [3], 'gumbel')
    assert trees[0].root_tot >= 1
    bs = _tool('belief_search')
    t = g.search_tree(True, 5)
    bs.search([t], g, ev, 6)
    assert t.root_tot >= 1
    mf = _tool('mull_floor')
    trees = mf.search_all([(g, 0, 0)], ev, 6, [0], 1)
    assert trees[0][3].root_tot >= 1
    db = _tool('delver_blunders')
    seat = {'select': 'visits', 'leaf_value_view': 'root', 'belief_tilt': 1.0, 'belief_clip': 20.0, 'belief_mix': 0.25}
    assert set(db.search_many(ev, [g.clone()], [7], 6, seat)) <= acts


def test_samples_with_tails_for_the_value_and_the_training(ev):
    """A driver's samples (their tails and overrides): root_view's values of them as value-only rows, and a training
    forward with their tails and overrides resolved (`rowkit.train_kw`, as a run's window)."""
    from zero.infer import UniformEvaluator, drive
    seat = G.nn_seat('m', 4, selfplay=True, sims_fast=2, p_full=0.5, temp_moves=10)
    drv = G.Driver('mtg', [[seat, seat]], spec=A.SPEC_DELVER, concurrency=1, threads=1, seed=9, record=True, max_steps=20000)
    parts = []
    drive(drv, {'m': UniformEvaluator()}, on_batch=lambda lb: parts.append(drv.take_samples()))
    parts.append(drv.take_samples())
    from zero.protocol import concat_samples
    s = concat_samples([p for p in parts if p is not None and len(p['player'])])
    assert len(s['t_seg']) > 0
    rv = _tool('root_view')
    rows = np.arange(min(64, len(s['player'])))
    v = rv.values(ev, s, rows)
    assert v.shape == (len(rows),) and np.isfinite(v).all()
    m = ev.model
    x = s['x'][rows].astype(np.float32)
    kw = rowkit.train_kw(m, s, rows, x)
    assert set(kw) == {'tail', 'ov'}
    with torch.no_grad():
        plog, vlog, _ = m(torch.from_numpy(x), torch.full((len(rows),), 2), **kw)
    assert torch.isfinite(vlog).all()
