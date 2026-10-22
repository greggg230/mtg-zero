"""MTG's parts of the AlphaZero stack on top of zero-core: the engine module (`mtgz._engine`
registers the toy and `mtg`), the `mtg` game through the bindings (actions, encode, determinize,
force_chance, bots in the Rust driver), the run presets, and the evaluation deck set
(`mtgz.az.evaldecks`). The game-independent tests live in zero-core's tests/; the engine's in
engine/tests (cargo test)."""
import json

import numpy as np
import pytest

from mtgz.az import evaldecks
from mtgz.az import games as G
from mtgz.az import protocol, run
from mtgz.az.infer import UniformEvaluator, drive

SPEC = '{"max_turns": 60}'


def test_engine_module_registers_the_games():
    from mtgz import _engine as E
    assert G.engine() is E and E.game_names() == ['toy', 'mtg']
    info = G.info('mtg')
    assert info['num_actions'] == 1024 and info['num_features'] == 13532
    assert info['min_players'] == info['max_players'] == 2
    assert info['bots'] == ['random', 'heuristic']


def test_game_through_the_bindings():
    E = G.engine()
    rng = np.random.default_rng(1)
    g = E.Game('mtg', SPEC, 2, 7)
    steps = 0
    while not g.over:
        legal = g.legal()
        assert len(legal) >= 1 and list(legal) == sorted(set(legal)) and max(legal) < 1024
        p = g.to_move
        x = np.asarray(g.encode(p))
        assert x.shape == (13532,) and np.isfinite(x).all()
        if steps % 25 == 0:
            d = g.clone()
            d.determinize(p, 99)
            assert list(d.legal()) == list(legal)
            assert np.array_equal(np.asarray(d.encode(p)), x)          # no hidden information
        g.step(int(legal[rng.integers(len(legal))]))
        steps += 1
        assert steps < 20000
    r = g.result()
    assert abs(sum(r) - 1) < 1e-6 and g.rounds >= 1
    with pytest.raises(Exception):
        E.Game('mtg', '{"bogus": 1}', 2, 1)
    with pytest.raises(Exception):
        E.Game('mtg', SPEC, 3, 1)


def test_bots_in_the_rust_driver():
    """heuristic vs random through the driver (the evaluation path): the heuristic wins."""
    h, r = G.seat_dict('heuristic'), G.seat_dict('random')
    drv = G.Driver('mtg', [[h, r], [r, h]] * 6, spec=SPEC, concurrency=12,
                   threads=2, seed=3, max_steps=20000)
    drive(drv, {})
    res = drv.take_results()
    assert len(res) == 12 and all(r['over'] for r in res)
    score = sum(r['result'][r['gid'] % 2] for r in res) / len(res)
    assert score > 0.7, score


def test_selfplay_records_samples():
    seat = G.nn_seat('m', 4, selfplay=True, sims_fast=2, p_full=0.5, temp_moves=10)
    drv = G.Driver('mtg', [[seat, seat]] * 2, spec=SPEC, concurrency=2, threads=1, seed=5, record=True,
                   max_steps=20000)
    parts = []
    drive(drv, {'m': UniformEvaluator()}, on_batch=lambda lb: parts.append(drv.take_samples()))
    parts.append(drv.take_samples())
    s = protocol.concat_samples(parts)
    protocol.validate_samples(s, 13532)
    assert len(s['x']) > 50
    res = drv.take_results()
    assert len(res) == 2 and all(r['over'] for r in res)


def test_presets_and_defaults():
    assert {'toy-smoke', 'toy-1', 'mtg-smoke', 'mtg-1'} <= set(run.PRESETS)
    cfg = run.resolve_config(run.parse(['--run', 'x', '--preset', 'mtg-smoke']), {})
    assert cfg['game'] == 'mtg' and cfg['yardstick'] == 'heuristic'
    assert json.loads(cfg['spec'])['max_turns'] == 60
    cfg = run.resolve_config(run.parse(['--run', 'x', '--preset', 'mtg-1']), {})
    assert cfg['game'] == 'mtg' and cfg['iters'] >= 4 and cfg['infer_device'] == 'cpu'


def test_eval_spec():
    assert json.loads(evaldecks.eval_spec('')) == {'decks': 'eval'}
    s = json.loads(evaldecks.eval_spec('{"max_turns": 50, "eval_decks": 4}'))
    assert s == {'max_turns': 50, 'eval_decks': 4, 'decks': 'eval'}
    lists = '{"decks": {"p0": ["Forest"], "p1": ["Island"]}}'
    assert json.loads(evaldecks.eval_spec(lists)) == json.loads(lists)
    # the engine accepts it and plays the fixed set: every game is one of eval_decks pairs
    E = G.engine()
    E.Game('mtg', evaldecks.eval_spec(SPEC), 2, 1)


def test_evaluation_drivers_use_the_eval_decks():
    """mtgz.az.run patches Run.driver: drivers that record nothing (evaluation matches) get the
    evaluation spec, self-play drivers the run's spec."""
    assert getattr(run.Run.driver, '_mtgz_eval_decks', False)
    seen = []
    real = run.G.Driver

    def spy(game, seats, spec=None, **kw):
        seen.append((kw.get('record'), spec))

    class Fake:
        cfg = dict(run.DEFAULTS, game='mtg', spec=SPEC)
    f = Fake()
    try:
        run.G.Driver = spy
        run.Run.driver(f, [['heuristic', 'random']], seed=1)
        run.Run.driver(f, [['heuristic', 'random']], seed=1, record=True)
    finally:
        run.G.Driver = real
    assert json.loads(seen[0][1])['decks'] == 'eval' and seen[0][0] is False
    assert seen[1] == (True, SPEC)
    assert f.cfg['spec'] == SPEC
