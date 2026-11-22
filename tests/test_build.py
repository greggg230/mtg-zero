"""mtgz.az.build (docs/ENGINE.md 95): building an engine position from a description, and the inverse,
through the PyO3 module - the round trip at random decisions of the Delver mirror and of Delver against
the MTGO test deck, the leak test (other seeds, the same encoding), the documented example, the
description checks, and a search on a built game. CPU only, no network (a uniform evaluator)."""
import collections
import copy
import json
import random

import numpy as np
import pytest

import mtgz.az.build as B

E = B.engine()

MTGO_TEST_DECK = (['Duress'] * 4 + ['Galvanic Discharge'] * 4 + ['Cut Down'] * 2 + ['Abrade'] + ['Figure of Destiny']
                  + ['Aether Spellbomb'] + ['Expedition Map'] + ['Talisman of Creativity'] * 2 + ['Mountain'] * 22
                  + ['Swamp'] * 22)


def bits(g, viewer):
    return B.row_bits(g, viewer)


def play_and_round_trip(p1_deck, games, seed0, share=0.15):
    """Random games; at a share of the multi-option decisions, the round trip from the decider's view.
    -> (positions built, Counter of the decisions the schema does not build)."""
    sj = B.spec_json(p1_deck=p1_deck)
    rng = random.Random(seed0)
    built, not_built = 0, collections.Counter()
    for gi in range(games):
        g = E.Game('mtg', sj, 2, seed0 + gi)
        for _ in range(1500):
            if g.over:
                break
            legal = list(g.legal())
            if len(legal) > 1 and rng.random() < share:
                v = g.to_move
                try:
                    B.describe_from_engine(g, v)
                except ValueError as e:
                    not_built[str(e).split(' (')[0]] += 1
                else:
                    d, bad = B.round_trip(g, v, seed=gi + 1, p1_deck=p1_deck)
                    assert not bad, (bad, json.dumps(d))
                    built += 1
            g.step(rng.choice(legal))
    return built, not_built


# The rare states the schema does not build, by name (each a documented limit of `az::build`).
RARE = {
    # a look at the opponent's library card, then a shuffle (ENGINE.md 192; reached by the mirror's random games
    # since ENGINE.md 227 moved their payments)
    "unsupported: a cloud entry naming a card its owner never saw",
}


def test_round_trip_the_delver_mirror():
    built, not_built = play_and_round_trip(None, 30, 100, share=0.2)
    assert built >= 600, (built, not_built)
    # only decisions inside a cast or a resolution are not built (and the rare states, by name)
    assert all(k.startswith('unsupported: the decision') or k in RARE for k in not_built), not_built


def test_round_trip_delver_against_the_mtgo_test_deck():
    built, not_built = play_and_round_trip(MTGO_TEST_DECK, 24, 300, share=0.2)
    assert built >= 400, (built, not_built)
    assert all(k.startswith('unsupported: the decision') or k in RARE for k in not_built), not_built


def test_the_documented_example():
    g = B.build_game(B.EXAMPLE, seed=3)
    st = json.loads(g.export_state())
    assert g.to_move == 0
    texts = [o['t'] for o in st['decision']['options']]
    for t in ('Pass priority', 'Cast Daze', 'Cast Force of Will', 'Cast Lightning Bolt'):
        assert t in texts, texts
    assert st['stack'][0]['name'] == 'Lightning Bolt'
    assert st['stack'][0]['targets'][0]['name'] == "Dragon's Rage Channeler"
    d = B.describe_from_engine(g, 0)
    assert d['players'][1]['hand_size'] == 4 and d['players'][1]['hand'] == ['Wasteland']
    # the description written back is the example (in the engine's own wording: every field, its defaults left out)
    g2 = B.build_game(d, seed=3)
    assert np.array_equal(bits(g2, 0), bits(g, 0))
    assert B.describe_from_engine(g2, 0) == d


def test_other_seeds_give_the_same_encoding():
    worlds = [B.build_game(B.EXAMPLE, seed=s) for s in range(1, 9)]
    e0 = bits(worlds[0], 0)
    hands = set()
    for w in worlds:
        assert np.array_equal(bits(w, 0), e0)
        assert list(w.legal()) == list(worlds[0].legal())
        assert B.viewer_view(json.loads(w.export_state()), 0, True) == B.viewer_view(json.loads(worlds[0].export_state()), 0, True)
        hands.add(tuple(sorted(c['name'] for c in json.loads(w.export_state())['players'][1]['hand'])))
    # the opponent's three unknown cards are dealt again; the known Wasteland stays
    assert len(hands) > 1 and all('Wasteland' in h for h in hands)
    # the deal seed in the description wins over the game seed
    d = dict(B.EXAMPLE, deal_seed=5)
    a, b = B.build_game(d, seed=1), B.build_game(d, seed=2)
    assert json.loads(a.export_state())['players'][1]['hand'] == json.loads(b.export_state())['players'][1]['hand']


def test_descriptions_are_checked():
    bad = copy.deepcopy(B.EXAMPLE)
    bad['players'][0]['graveyard'] = ['Lightning Bolt'] * 4
    with pytest.raises(ValueError, match='no Lightning Bolt left to place'):
        B.build_game(bad)
    bad = copy.deepcopy(B.EXAMPLE)
    bad['players'][0]['library_size'] = 50
    with pytest.raises(ValueError, match='does not fit the description'):
        B.build_game(bad)
    bad = copy.deepcopy(B.EXAMPLE)
    bad['players'][1]['hand_sise'] = 4
    with pytest.raises(ValueError, match='unknown key'):
        B.build_game(bad)
    # an MTGO list for player 1 that the description does not fit
    with pytest.raises(ValueError, match="P1's deck has no"):
        B.build_game(B.EXAMPLE, p1_deck=MTGO_TEST_DECK)


def test_a_search_on_a_built_game():
    from zero.parity import search_rs

    def uniform(x, acts, n, tail=None, ov=None):    # (layout v7: the evaluator gets the row's tail and overrides)
        return np.full(len(acts), 1.0 / len(acts), np.float32), np.full(n, 0.5, np.float32), np.zeros(n, np.float32)

    g = B.build_game(B.EXAMPLE, seed=3)
    visits, win = search_rs(g, uniform, 64, seed=1)
    assert visits.sum() >= 60 and len(visits) == len(g.legal())


def test_then_reaches_a_decision_inside_a_resolution():
    # we (P0) cast Brainstorm; the opponent Dazed it (returning their Island) and passed: we pass too,
    # Daze resolves, and the engine asks us whether to pay {1} - the decision MTGO asks
    d = {
        "viewer": 0, "turn": 6, "active": 0, "step": "main1", "priority": 0, "passes": 1, "starting": 1,
        "players": [
            {"life": 20, "hand": ["Force of Will", "Ponder"], "library_size": 55, "spells_cast": 1,
             "noncreature_cast": 1, "spells_game": 1},
            {"life": 20, "hand_size": 5, "hand": ["Island"], "library_size": 53, "spells_cast": 1,
             "noncreature_cast": 1, "spells_game": 1},
        ],
        "battlefield": [{"name": "Island", "owner": 0}, {"name": "Volcanic Island", "owner": 0, "tapped": True},
                        {"name": "Volcanic Island", "owner": 1}],
        "exile": [],
        "stack": [{"spell": "Brainstorm", "controller": 0},
                  {"spell": "Daze", "controller": 1, "alt": 1, "targets": [{"req": 0, "obj": "s0"}]}],
        "then": ["Pass priority"],
    }
    g = B.build_game(d, seed=4)
    st = json.loads(g.export_state())
    assert st['decision']['player'] == 0 and st['decision']['kind'] == 'YesNo', st['decision']
    assert sorted(o['t'] for o in st['decision']['options']) == ['No', 'Yes']
    assert [x['name'] for x in st['stack']] == ['Brainstorm', 'Daze']
    # a label that names no option (or two) is an error, never a guess
    with pytest.raises(ValueError, match='is not an option'):
        B.build_game(dict(d, then=['Cast Daze']))
