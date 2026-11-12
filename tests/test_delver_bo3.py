"""The Delver mirror's constructed best-of-three (docs/ENGINE.md 120) through the bindings: the presets
`delver-mirror-bo3` and `delver-bo3-smoke` (both the full 75 - its five sideboard cards outside the cube are
supported, ENGINE.md 122 - and one episode per game, ENGINE.md 121; 129.2), the deck target and its tilt declared as
the EoE presets do, a match through the engine module (sideboarding between games, determinize keeping the
decider's view, the deck-tilted deal in a later game), and the heuristic yardstick against random in matches."""
import json

import numpy as np
import pytest

from mtgz.az import decklists
from mtgz.az import games as G
from mtgz.az.evaldecks import eval_spec
from mtgz.az.infer import drive

# the Delver 75's sideboard cards outside the cube (docs/ENGINE.md 122)
OUTSIDE_THE_CUBE = ('Consign to Memory', 'Hydroblast', 'Meltdown', 'Price of Progress', 'Rough // Tumble')


def test_the_bo3_presets():
    import mtgz.az as A
    full, smoke = A.PRESETS['delver-mirror-bo3'], A.PRESETS['delver-bo3-smoke']
    d = decklists.load(decklists.IZZET_DELVER)
    for p, base in ((full, A.PRESETS['delver-mirror']), (smoke, A.PRESETS['delver-smoke'])):
        side = decklists.names(d['sideboard'])
        s = json.loads(p['spec'])
        assert s['constructed'] is True and s['format'] == 'bo3' and s['max_turns'] == 60 and s['mulligans'] is True
        # one episode per game (docs/ENGINE.md 121: each game's rows labelled by its own outcome), in the
        # evaluation spec too
        assert s['game_episodes'] is True and json.loads(p['eval_spec'])['game_episodes'] is True
        assert s['decks']['p0'] == s['decks']['p1'] == decklists.names(d['main'])
        assert s['decks']['p0_sideboard'] == s['decks']['p1_sideboard'] == side
        # the decks are fixed: evaluation plays the same matches
        assert p['eval_spec'] == p['spec'] == eval_spec(p['spec'])
        # the deck target and its tilt, as the EoE presets declare them; no deck reader
        assert p['aux_deck_w'] == 0.5 and p.get('reader_deck', 0) == 0
        assert p['selfplay_belief_tilts'] == p['eval_belief_tilts'] == {'deck': 1.0}
        assert p['max_steps'] == 60_000
        for k, v in base.items():
            if k not in ('spec', 'eval_spec', 'aux_deck_w', 'selfplay_belief_tilts', 'eval_belief_tilts', 'max_steps'):
                assert p[k] == v, k
    assert len(json.loads(full['spec'])['decks']['p0_sideboard']) == 15
    assert smoke['spec'] == full['spec']
    with pytest.raises(ValueError):
        decklists.mirror_bo3_spec(sideboard=[[4, 'Pyroblast']])
    # without the key: one episode per match, as before
    assert 'game_episodes' not in json.loads(decklists.mirror_bo3_spec())


def test_the_full_preset_builds_with_the_full_75():
    import mtgz.az as A
    E = G.engine()
    g = E.Game('mtg', A.PRESETS['delver-mirror-bo3']['spec'], 2, 1)
    assert g.chained
    side = json.loads(A.PRESETS['delver-mirror-bo3']['spec'])['decks']['p0_sideboard']
    for c in OUTSIDE_THE_CUBE:
        assert c in side, c
    # a card the engine does not support is still refused by name (no silent drop, no substitution)
    bad = json.loads(A.PRESETS['delver-mirror-bo3']['spec'])
    bad['decks']['p1_sideboard'] = bad['decks']['p1_sideboard'][:-1] + ['Not A Real Card']
    with pytest.raises(Exception, match='Not A Real Card'):
        E.Game('mtg', json.dumps(bad), 2, 1)


def whole_row(g, p):
    """`p`'s whole row (layout v7, docs/ENGINE.md 125.12): the head, the tail tokens (a later game's `my_revealed`
    among them: a head-only `encode` refuses such a row) and, for the decider, the pointer overrides."""
    x, t = g.encode_row(p)
    parts = [np.asarray(x, np.float32), np.asarray(t['t_seg']), np.asarray(t['t_slot']), np.asarray(t['t_val'], np.float32)]
    if p == g.to_move:
        parts += [np.asarray(a) for a in g.option_overrides(p)]
    return parts


def same_row(a, b):
    return len(a) == len(b) and all(np.array_equal(u, v) for u, v in zip(a, b))


def test_a_constructed_match_through_the_bindings():
    import mtgz.az as A
    E = G.engine()
    rng = np.random.default_rng(3)
    spec = A.PRESETS['delver-bo3-smoke']['spec']
    g = E.Game('mtg', spec, 2, 7)
    steps, checked, later = 0, 0, 0
    pred = np.full(2048, 1.0 / 1024, dtype=np.float32)
    res = None
    while True:
        if g.to_move is None:
            # a game's end ends its episode (docs/ENGINE.md 121): the next one begins with the sideboarding
            g, res = g.next_episode(conceded=None)
            if res is not None:
                break
            later += 1
            continue
        legal = g.legal()
        p = g.to_move
        x = whole_row(g, p)
        assert np.isfinite(x[0]).all() and np.isfinite(x[3]).all()
        if steps % 25 == 0:
            d = g.clone()
            d.determinize(p, steps)
            assert list(d.legal()) == list(legal)
            assert same_row(whole_row(d, p), x)
            # the deck-tilted deal (docs/ENGINE.md 106): allowed in a constructed best-of-three (it deals the
            # later games' hidden 60s; in game 1 the open deal)
            t = g.clone()
            t.determinize_with(p, steps, pred, 0.0, 20.0, {'deck': 1.0})
            assert list(t.legal()) == list(legal)
            assert same_row(whole_row(t, p), x)
            checked += 1
        g.step(int(legal[rng.integers(len(legal))]))
        steps += 1
        assert steps < 60000
    assert abs(sum(res[:2]) - 1) < 1e-6 and checked > 10 and later >= 1


def test_the_yardstick_beats_random_in_bo3_matches():
    import mtgz.az as A
    h, r = G.seat_dict('heuristic'), G.seat_dict('random')
    drv = G.Driver('mtg', [[h, r], [r, h]] * 4, spec=A.PRESETS['delver-bo3-smoke']['spec'], concurrency=8,
                   threads=2, seed=4, max_steps=60000)
    drive(drv, {})
    res = drv.take_results()
    assert len(res) == 8 and all(x['over'] for x in res)
    score = sum(x['result'][x['gid'] % 2] for x in res) / len(res)
    assert score > 0.6, score
