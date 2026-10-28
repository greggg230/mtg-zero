"""The constructed Delver mirror (docs/ENGINE.md 54) through the bindings: the pinned deck and its
loader (`mtgz.az.decklists`), the presets `delver-mirror` / `delver-smoke` (the spec: both seats the
pinned 60, best-of-one, constructed; the evaluation spec is the spec), the game through the engine
module (encode, determinize), the heuristic yardstick against random in the Rust driver, self-play
records, and the cube's specs unchanged (the legacy cards unknown to them)."""
import json

import numpy as np
import pytest

from mtgz.az import decklists
from mtgz.az import games as G
from mtgz.az import protocol
from mtgz.az.evaldecks import eval_spec
from mtgz.az.infer import UniformEvaluator, drive


def test_the_pinned_deck_and_its_loader():
    d = decklists.load(decklists.IZZET_DELVER)
    assert (d['player'], d['event'], d['date']) == ('i_b_TRUE', 'Legacy Challenge 32', '2026-10-23')
    assert d['source'] == 'https://www.mtggoldfish.com/deck/8016541'
    main, side = decklists.names(d['main']), decklists.names(d['sideboard'])
    assert len(main) == 60 and len(side) == 15
    assert main.count('Murktide Regent') == 2 and main.count('Prismari Charm') == 1
    with pytest.raises(ValueError):
        decklists.names([[0, 'Island']])


def test_the_presets():
    import mtgz.az as A
    for name in ('delver-mirror', 'delver-smoke'):
        p = A.PRESETS[name]
        s = json.loads(p['spec'])
        assert s['constructed'] is True and s['decks']['p0'] == s['decks']['p1']
        assert len(s['decks']['p0']) == 60 and 'p0_sideboard' not in s['decks']
        assert s.get('format', 'bo1') == 'bo1' and s['max_turns'] == 60 and s['mulligans'] is True
        # the decks are fixed: evaluation plays the same spec
        assert p['eval_spec'] == p['spec'] == eval_spec(p['spec'])
    m, three = A.PRESETS['delver-mirror'], A.PRESETS['mtg-3']
    for k in ('sims', 'sims_fast', 'p_full', 'sims_minor', 'hidden', 'blocks', 'heads', 'ff', 'arch', 'games',
              'yardstick', 'eval_games', 'eval_random_games', 'elo_games', 'elo_opponents', 'policy_target'):
        assert m[k] == three[k], k
    assert m['yardstick'] == 'heuristic'


def test_the_mirror_through_the_bindings():
    import mtgz.az as A
    E = G.engine()
    rng = np.random.default_rng(2)
    g = E.Game('mtg', A.SPEC_DELVER, 2, 11)
    steps = 0
    while not g.over:
        legal = g.legal()
        p = g.to_move
        x = np.asarray(g.encode(p))
        assert x.shape == (G.info('mtg')['num_features'],) and np.isfinite(x).all()
        if steps % 20 == 0:
            d = g.clone()
            d.determinize(p, 7)
            assert list(d.legal()) == list(legal)
            assert np.array_equal(np.asarray(d.encode(p)), x)
        g.step(int(legal[rng.integers(len(legal))]))
        steps += 1
        assert steps < 20000
    assert abs(sum(g.result()) - 1) < 1e-6
    # The cube's card table doesn't know the legacy cards; constructed needs decklists.
    s = json.loads(A.SPEC_DELVER)
    del s['constructed']
    with pytest.raises(Exception):
        E.Game('mtg', json.dumps(s), 2, 1)
    with pytest.raises(Exception):
        E.Game('mtg', '{"constructed": true}', 2, 1)
    with pytest.raises(Exception):
        E.Game('mtg', json.dumps(dict(json.loads(A.SPEC_DELVER), format='bo3')), 2, 1)


def test_the_yardstick_beats_random_in_the_mirror():
    import mtgz.az as A
    h, r = G.seat_dict('heuristic'), G.seat_dict('random')
    drv = G.Driver('mtg', [[h, r], [r, h]] * 8, spec=A.SPEC_DELVER, concurrency=16, threads=2, seed=4,
                   max_steps=20000)
    drive(drv, {})
    res = drv.take_results()
    assert len(res) == 16 and all(x['over'] for x in res)
    score = sum(x['result'][x['gid'] % 2] for x in res) / len(res)
    assert score > 0.6, score


def test_mirror_selfplay_records_samples():
    import mtgz.az as A
    seat = G.nn_seat('m', 4, selfplay=True, sims_fast=2, p_full=0.5, temp_moves=10)
    drv = G.Driver('mtg', [[seat, seat]] * 2, spec=A.SPEC_DELVER, concurrency=2, threads=1, seed=5, record=True,
                   max_steps=20000)
    parts = []
    drive(drv, {'m': UniformEvaluator()}, on_batch=lambda lb: parts.append(drv.take_samples()))
    parts.append(drv.take_samples())
    s = protocol.concat_samples(parts)
    protocol.validate_samples(s, G.info('mtg')['num_features'])
    assert len(drv.take_results()) == 2
