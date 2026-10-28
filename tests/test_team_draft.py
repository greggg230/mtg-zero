"""The game `team_draft` (docs/ENGINE.md 41, 43, DESIGN.md 2.3) through the bindings: its info, a
whole event of random actions (encode, determinize keeping the decider's view and options), the
deferred terminal (the result is refused until the matches are played; zero-core's driver parks the
event there and resolves it with the matches' results, ENGINE.md 43), the spec, deckbuilding as picks
(ENGINE.md 47), and the token network reading layout draft_v2 on real positions and a driver's leaf
batch."""
import json

import numpy as np
import pytest

from mtgz.az import games as G
from mtgz.az import protocol


def test_info_and_spec():
    info = G.info('team_draft')
    assert info['num_actions'] == 1024 and info['num_features'] == 14492
    assert info['min_players'] == 2 and info['max_players'] == 8
    assert info['bots'] == ['random', 'consensus']
    E = G.engine()
    for spec, players in (('{"bogus": 1}', 8), ('{"packs": 4}', 8), ('{}', 3), ('{}', 10), ('{"rounds": 3}', 4),
                          ('{"deck_min": 41}', 4)):
        with pytest.raises(Exception):
            E.Game('team_draft', spec, players, 1)


def test_an_event_through_the_bindings():
    E = G.engine()
    nf = G.info('team_draft')['num_features']
    rng = np.random.default_rng(1)
    for players in (8, 4):
        g = E.Game('team_draft', '{}', players, 3)
        steps = 0
        while not g.over:
            legal = list(g.legal())
            assert legal == sorted(set(legal)) and max(legal) < 1024
            p = g.to_move
            x = np.asarray(g.encode(p))
            assert x.shape == (nf,) and np.isfinite(x).all()
            if steps % 50 == 0:
                d = g.clone()
                d.determinize(p, 7 + steps)
                assert list(d.legal()) == legal
                assert np.array_equal(np.asarray(d.encode(p)), x)          # nothing hidden in the features
            g.step(int(legal[rng.integers(len(legal))]))
            steps += 1
        # 45 picks per seat; deckbuilding: at least 22 pool cards (DONE from then on), at most 40, and
        # five counts (ENGINE.md 47)
        assert players * (45 + 22 + 5) <= steps <= players * (45 + 40 + 5)
        assert g.to_move is None and g.deferred and g.team_size == players // 2
        # the deferred terminal: no result until the matches are played (zero-core's bindings refuse it)
        with pytest.raises(ValueError, match='deferred terminal'):
            g.result()
        ms = json.loads(g.deferred_payload())['matches']
        assert len(ms) == {8: 12, 4: 4}[players]
        basics = {'Plains', 'Island', 'Swamp', 'Mountain', 'Forest'}
        for p in ms[0]['players']:
            assert len(p['deck']) == 40 and len(p['deck']) - sum(c in basics for c in p['deck']) >= 22
            assert len(p['deck']) - sum(c in basics for c in p['deck']) + len(p['sideboard']) == 45


def test_a_driver_parks_the_event_until_its_matches_are_played():
    """zero-core's deferred hook (ENGINE.md 43): the driver parks the event at its deferred terminal
    with the matches as its payload, refuses a malformed result and emits the row once resolved."""
    r = G.seat_dict('random')
    drv = G.Driver('team_draft', [[r] * 4], spec='{"packs": 1, "pack_size": 4}', concurrency=1, threads=1, seed=1,
                   max_steps=10_000)
    from mtgz.az.infer import drive
    drive(drv, {})
    assert drv.pending == 1 and drv.take_results() == []
    (gid, payload), = drv.take_deferred()
    n = len(json.loads(payload)['matches'])
    assert n == 4
    with pytest.raises(ValueError):
        drv.resolve(gid, [[1, 0]])
    assert drv.pending == 1
    drv.resolve(gid, [[1, 0]] * (n - 1) + [{'result': [0, 1], 'scores': [0, 2]}])     # team A wins 3-1
    row, = drv.take_results()
    assert row['result'] == [0.5, 0.0, 0.5, 0.0] and row['scores'] == [3, 1, 3, 1] and drv.finished


def test_token_network_reads_layout_draft_v2():
    import torch
    import mtgz.az as A
    from zero.infer import NetEvaluator
    from zero.net import make_net, tokens_layout
    info = G.info('team_draft')
    lay = tokens_layout(A.LAYOUT_DRAFT, info['num_features'], info['num_actions'])
    assert A.LAYOUT_DRAFT['features'] == info['num_features'] and lay['vocab'] == 1024
    segs = {s['name']: s for s in lay['segments']}
    assert segs['pack']['count'] == 15 and segs['pool']['count'] == 45 and [b['count'] for b in lay['bags']] == [13]
    assert all(s['id_actions'] == 0 for s in A.LAYOUT_DRAFT['segments'])
    names = A.LAYOUT_DRAFT['cards']
    torch.manual_seed(0)
    m = make_net(info, 'team_draft', 'tokens', 32, 1, heads=2)
    E = G.engine()
    rng = np.random.default_rng(2)
    g = E.Game('team_draft', '{}', 8, 5)
    checked = 0
    while not g.over and checked < 60:
        legal = list(g.legal())
        x = torch.from_numpy(np.asarray(g.encode(g.to_move)))[None]
        pack = segs['pack']
        for s in range(pack['count']):
            o = pack['offset'] + s * pack['width']
            if x[0, o + pack['present']] > 0.5:
                ident = int(m._identity(x[0, o:o + pack['width']][None], pack['id'])[0])
                assert ident in legal and names[ident]                  # the slot's card is the pick action
                checked += 1
        with torch.no_grad():
            pol, val, mar = m(x, torch.tensor([8]))
        assert torch.isfinite(pol).all() and torch.isfinite(val).all()
        g.step(int(legal[rng.integers(len(legal))]))
    assert checked >= 60
    seat = G.nn_seat('m', 4)
    drv = G.Driver('team_draft', [[seat] * 8] * 2, spec='{}', concurrency=2, threads=1, seed=4, max_steps=10_000)
    lb = drv.next_batch()
    pri, val, mar = NetEvaluator(m)(lb)
    protocol.validate_evaluation(lb, pri, val, mar)
