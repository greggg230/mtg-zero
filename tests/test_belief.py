"""The belief target (docs/ENGINE.md 66; zero-core docs/az.md 15): the decider's opponent's hand by
card identity, through the bindings and zero-core's driver (recorded with the samples when on, the
hand size it sums to public), its card names as the spectator view writes them, the baselines' public
facts read back from the features, the run setting off by default, and a replay of a checkpoint with
the head (the `belief` field per network decision)."""
import collections
import json

import numpy as np
import pytest

import mtgz.az as P
from mtgz.az import belief as B
from mtgz.az import games as G
from mtgz.az.infer import UniformEvaluator, drive

SPEC = P.SPEC_DELVER


def positions(n=40, seed=0, spec=SPEC):
    """Mid-game states of random play (the game, its decider)."""
    rng = np.random.default_rng(seed)
    out = []
    s = 0
    while len(out) < n:
        g = G.engine().Game('mtg', spec, 2, s)
        s += 1
        for _ in range(int(rng.integers(1, 120))):
            if g.over:
                break
            legal = g.legal()
            g.step(legal[int(rng.integers(len(legal)))])
        if not g.over:
            out.append(g)
    return out


def test_the_package_declares_belief_and_the_engine_computes_it():
    info = G.info('mtg')
    assert [tuple(t) for t in info['aux_layout']] == [('belief', 1024)] and info['aux_width'] == 1024
    assert P.PACKAGE.aux_of('mtg') == [['belief', 1024, 'multinomial']]
    assert G.info('team_draft')['aux_layout'] == []
    from mtgz.az.run import DEFAULTS
    assert DEFAULTS['aux_belief_w'] == 0.0


@pytest.mark.parametrize('spec', [SPEC, '{"max_turns": 60}'])
def test_the_target_is_the_opponents_hand_by_name_and_never_in_the_features(spec):
    nm = B.names()
    differ = 0
    for g in positions(30, 1, spec):
        me = g.to_move
        st = json.loads(g.export_state())
        opp = [p for p in st['players'] if p['id'] == 1 - me][0]
        t = np.asarray(g.aux_target(me))
        assert collections.Counter({nm[i]: int(t[i]) for i in np.flatnonzero(t)}) == \
            collections.Counter(c['name'] for c in opp['hand'])
        x = np.asarray(g.encode(me))
        n, hidden, known, cloud = B.public(x[None])
        assert n[0] == len(opp['hand'])
        # what determinize may deal covers the true hand
        prior = B.uniform_prior(x[None])[0]
        assert np.all(prior[t > 0] > 0) and abs(prior.sum() - len(opp['hand'])) < 1e-3
        for k in range(2):
            d = g.clone()
            d.determinize(me, 1000 + k)
            assert np.array_equal(np.asarray(d.encode(me)), x)
            differ += not np.array_equal(np.asarray(d.aux_target(me)), t)
    assert differ > 20


def test_selfplay_records_it_and_it_sums_to_the_public_hand_size():
    seat = G.nn_seat('m', 4, selfplay=True, sims_fast=2, p_full=0.5, temp_moves=10, sims_minor=2)
    rows = {}
    for aux in (False, True):
        kw = {'aux': True} if aux else {}
        drv = G.Driver('mtg', [[seat, seat]] * 2, spec=SPEC, concurrency=2, seed=3, record=True, threads=2, **kw)
        drive(drv, {'m': UniformEvaluator()})
        rows[aux] = drv.take_samples()
    s0, s1 = rows[False], rows[True]
    assert 'aux' not in s0 and s1['aux'].shape == (len(s1['player']), 1024)
    for k in s0:
        assert np.array_equal(s0[k], s1[k]), k
    n, _, _, _ = B.public(s1['x'])
    assert np.array_equal(s1['aux'].sum(1), n)
    assert (n > 0).mean() > 0.8


def test_render_and_counts():
    nm = B.names()
    fow = nm.index('Force of Will')
    daze = nm.index('Daze')
    p = np.zeros(1024)
    p[fow], p[daze], p[0] = 0.5, 0.3, 0.2
    st = {'players': [{'id': 0, 'hand': [{}] * 3}, {'id': 1, 'hand': [{}] * 4}]}
    out = B.render({'belief': p}, st, 0, {})
    assert out == {'belief': {'for': 1, 'n': 4, 'cards': [['Force of Will', 2.0], ['Daze', 1.2], [nm[0], 0.8]]}}
    assert B.render({'belief': p}, st, 1, {})['belief']['n'] == 3
    st['players'][1]['hand'] = []
    assert B.render({'belief': p}, st, 0, {}) == {'belief': {'for': 1, 'n': 0, 'cards': []}}


def test_a_replay_of_a_checkpoint_with_the_head_shows_belief(tmp_path):
    import torch
    from zero import replay
    from zero.net import make_net, save
    torch.manual_seed(0)
    m = make_net(G.info('mtg'), 'mtg', 'tokens', 16, 1, heads=2, aux=P.PACKAGE.aux_of('mtg'))
    with torch.no_grad():
        for q in m.aux_head.parameters():
            q.add_(0.05 * torch.randn_like(q))
    ck = str(tmp_path / 'iter_0000.pt')
    save(m, ck, {'iter': 0})
    reps = replay.record(ck, opp='random', games=1, sims=4, spec=SPEC, chance_k=0, seed=2, log=lambda *a: None)
    nn = [f for f in reps[0]['frames'] if f['k'] == 'nn']
    assert nn and all('belief' in f and 'aux' not in f for f in nn)
    for f in nn:
        b = f['belief']
        st = reps[0]['states'][f['s']]
        opp = [p for p in st['players'] if p['id'] == b['for']][0]
        assert b['for'] == 1 - f['p'] and b['n'] == len(opp['hand']) and len(b['cards']) <= 12
        assert all(isinstance(c[0], str) for c in b['cards'])
        assert sum(c[1] for c in b['cards']) <= b['n'] + 1e-6
    assert [s['aux'] for s in reps[0]['seats'] if s['kind'] == 'nn'] == [['belief']]
    from mtgz.az.replays import summary
    assert summary(reps[0])['belief'] is True
