"""The Delver mirror's constructed best-of-three (docs/ENGINE.md 120) through the bindings: the presets
`delver-mirror-bo3` (the full 75, refused while five of its sideboard cards are unsupported, naming them) and
`delver-bo3-smoke` (the main and the 75's supported sideboard cards), the deck target and its tilt declared as
the EoE presets do, a match through the engine module (sideboarding between games, determinize keeping the
decider's view, the deck-tilted deal in a later game), and the heuristic yardstick against random in matches."""
import json

import numpy as np
import pytest

from mtgz.az import decklists
from mtgz.az import games as G
from mtgz.az.evaldecks import eval_spec
from mtgz.az.infer import drive

UNSUPPORTED = ('Consign to Memory', 'Hydroblast', 'Meltdown', 'Price of Progress', 'Rough // Tumble')


def test_the_bo3_presets():
    import mtgz.az as A
    full, smoke = A.PRESETS['delver-mirror-bo3'], A.PRESETS['delver-bo3-smoke']
    d = decklists.load(decklists.IZZET_DELVER)
    for p, base, side in ((full, A.PRESETS['delver-mirror'], decklists.names(d['sideboard'])),
                          (smoke, A.PRESETS['delver-smoke'], decklists.names(decklists.DELVER_SMOKE_SIDEBOARD))):
        s = json.loads(p['spec'])
        assert s['constructed'] is True and s['format'] == 'bo3' and s['max_turns'] == 60 and s['mulligans'] is True
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
    assert len(json.loads(smoke['spec'])['decks']['p0_sideboard']) == 8
    with pytest.raises(ValueError):
        decklists.mirror_bo3_spec(sideboard=[[4, 'Pyroblast']])


def test_the_full_preset_refuses_naming_the_unsupported_cards():
    import mtgz.az as A
    E = G.engine()
    with pytest.raises(Exception) as e:
        E.Game('mtg', A.PRESETS['delver-mirror-bo3']['spec'], 2, 1)
    for c in UNSUPPORTED:
        assert c in str(e.value), (c, str(e.value))
    # every card of the smoke's sideboard is supported (its spec plays)
    E.Game('mtg', A.PRESETS['delver-bo3-smoke']['spec'], 2, 1)


def test_a_constructed_match_through_the_bindings():
    import mtgz.az as A
    E = G.engine()
    rng = np.random.default_rng(3)
    spec = A.PRESETS['delver-bo3-smoke']['spec']
    g = E.Game('mtg', spec, 2, 7)
    steps, checked, later = 0, 0, 0
    pred = np.full(2048, 1.0 / 1024, dtype=np.float32)
    while not g.over:
        legal = g.legal()
        p = g.to_move
        x = np.asarray(g.encode(p))
        assert np.isfinite(x).all()
        if steps % 25 == 0:
            d = g.clone()
            d.determinize(p, steps)
            assert list(d.legal()) == list(legal)
            assert np.array_equal(np.asarray(d.encode(p)), x)
            # the deck-tilted deal (docs/ENGINE.md 106): allowed in a constructed best-of-three (it deals the
            # later games' hidden 60s; in game 1 the open deal)
            t = g.clone()
            t.determinize_with(p, steps, pred, 0.0, 20.0, {'deck': 1.0})
            assert list(t.legal()) == list(legal)
            assert np.array_equal(np.asarray(t.encode(p)), x)
            checked += 1
        g.step(int(legal[rng.integers(len(legal))]))
        steps += 1
        assert steps < 60000
    assert abs(sum(g.result()) - 1) < 1e-6 and checked > 10


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
