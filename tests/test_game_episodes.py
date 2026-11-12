"""Best-of-three with one episode per game (docs/ENGINE.md 121, `"game_episodes": true`; zero-core docs/az.md 42): the
`mtg` game through the bindings and zero-core's driver from Python - one row per match whose samples (the arrays the
shard writer writes) take their own game's result, the sideboarding before game g among game g's -, the evaluation
spec keeps the key (Run.eval_chained), the remote worker's hello fingerprint. Without the key: no row gains a key."""
import json

import numpy as np
import pytest

import mtgz.az  # noqa: F401  (registers the package)
from mtgz.az import evaldecks
from mtgz.az import games as G
from mtgz.az.infer import UniformEvaluator, drive

BO3 = {'max_turns': 60, 'eval_decks': 16, 'eval_seed': 1, 'format': 'bo3'}
GAMES = dict(BO3, game_episodes=True)


def rows_of(drv):
    from zero.workqueue import split_games
    rows = []

    def on_batch(lb):
        rows.extend(split_games(drv.take_results(), drv.take_samples()))
    drive(drv, {'m': UniformEvaluator()}, on_batch=on_batch)
    rows.extend(split_games(drv.take_results(), drv.take_samples()))
    return sorted(rows, key=lambda rs: rs[0]['gid'])


def sideboarding(x):
    """The match block's 'sideboarding now' flag (encode.rs mtch::SIDEBOARDING, float 5 of the layout's 'match'
    globals) of feature rows x."""
    from mtgz.az import LAYOUT
    off = next(g['offset'] for g in LAYOUT['globals'] if g['name'] == 'match')
    return x[:, off + 5] > 0.5


def test_the_spec_and_the_bindings():
    E = G.engine()
    with pytest.raises(ValueError, match='best-of-three'):
        E.Game('mtg', json.dumps({'game_episodes': True}), 2, 1)
    g = E.Game('mtg', json.dumps(GAMES), 2, 1)
    assert g.chained and g.can_concede and not E.Game('mtg', json.dumps(BO3), 2, 1).chained
    assert json.loads(evaldecks.eval_spec(json.dumps(GAMES)))['game_episodes'] is True
    assert G.chained('mtg', evaldecks.eval_spec(json.dumps(GAMES))) and not G.chained('mtg', json.dumps(BO3))
    # a concession mid-game: the next episode is the sideboarding, the loser first (CR 104.3a, 103.1)
    rng = np.random.default_rng(2)
    for _ in range(30):
        legal = g.legal()
        g.step(legal[rng.integers(len(legal))])
    p = g.to_move
    nxt, res = g.next_episode(conceded=p)
    assert res is None and nxt.scores() == ([0.0, 1.0] if p == 0 else [1.0, 0.0])
    assert nxt.to_move == p and not nxt.can_concede


def test_every_sample_takes_its_own_games_result():
    seat = G.nn_seat('m', 4, selfplay=True, temp_moves=1000)
    drv = G.Driver('mtg', [[seat, seat]] * 12, spec=json.dumps(GAMES), concurrency=12, seed=5, record=True, threads=4,
                   max_steps=60_000)
    rows = rows_of(drv)
    assert len(rows) == 12, 'one row per match'
    n_sb = 0
    for r, s in rows:
        eps = r['episodes']
        assert r['over'] and 2 <= len(eps) <= 3
        wins = [sum(e['result'][p] > 0.75 for e in eps) for p in (0, 1)]
        assert max(wins) >= 2 or len(eps) == 3
        assert r['result'] == ([1.0, 0.0] if wins[0] > wins[1] else [0.0, 1.0] if wins[1] > wins[0] else [0.5, 0.5])
        assert r['samples'] == sum(e['samples'] for e in eps) == len(s['player'])
        a = 0
        for k, e in enumerate(eps):
            b = a + e['samples']
            pl = s['player'][a:b]
            for j in range(2):
                want = np.asarray(e['result'], np.float32)[(pl + j) % 2]
                assert np.array_equal(s['z'][a:b, j], want), (r['gid'], k)
            # the sideboarding before game k + 1 is game k + 1's: never among game 1's rows
            sb = sideboarding(s['x'][a:b])
            assert k > 0 or not sb.any(), r['gid']
            n_sb += int(sb.sum())
            a = b
    assert n_sb > 0
    # without the key: rows of before (the whole match one episode, no `episodes`)
    plain = rows_of(G.Driver('mtg', [[seat, seat]] * 2, spec=json.dumps(BO3), concurrency=2, seed=5, record=True,
                             threads=2, max_steps=60_000))
    assert all('episodes' not in r for r, _ in plain)


def test_the_evaluation_and_the_remote_hello_see_the_episodes():
    from zero.remote import eval_episodes
    from zero.remoteplay import _episodes_of
    from mtgz.az import run
    cfg = dict(run.DEFAULTS, game='mtg', spec=json.dumps(GAMES), eval_spec=evaldecks.eval_spec(json.dumps(GAMES)))
    assert _episodes_of(cfg) == 'chained' and eval_episodes(cfg) == 'chained'
    plain = dict(cfg, spec=json.dumps(BO3), eval_spec=evaldecks.eval_spec(json.dumps(BO3)))
    assert _episodes_of(plain) == '' and eval_episodes(plain) == ''
    R = run.Run.__new__(run.Run)
    R.cfg = cfg
    assert R.eval_chained()
