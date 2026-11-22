"""The metagame spec (docs/ENGINE.md 176) on the Python side: `decklists.metagame_spec` / `pair_spec`, the presets
`legacy-meta3` / `legacy-meta3-smoke`, the evaluation spec (the metagame kept), the engine's parse and refusals through
the bindings, each game's pairing in its exported state, and the replay viewer's per-game mains."""
import json

import pytest

import mtgz.az as A
from mtgz.az import decklists as D
from mtgz.az import games as G
from mtgz.az import replays as R
from mtgz.az.evaldecks import eval_spec


def test_metagame_spec_shape_and_weights():
    s = json.loads(D.metagame_spec())
    assert s == {'max_turns': 60, 'mulligans': True, 'constructed': True, 'metagame': s['metagame']}
    assert [e['name'] for e in s['metagame']['decks']] == list(D.META3)
    assert all(len(e['main']) == 60 and 'sideboard' not in e for e in s['metagame']['decks'])
    assert 'weights' not in s['metagame']                                              # uniform: the engine's default
    b = json.loads(D.metagame_spec(fmt='bo3', game_episodes=True))
    assert b['format'] == 'bo3' and b['game_episodes'] is True
    assert all(len(e['main']) == 60 and len(e['sideboard']) == 15 for e in b['metagame']['decks'])
    w = json.loads(D.metagame_spec(weights={(D.LEGACY_DOOMSDAY, D.SNEAK_SHOW): 3, (D.IZZET_DELVER, D.IZZET_DELVER): 0}))
    assert w['metagame']['weights'] == [[0, 1, 1], [1, 1, 3], [1, 1, 1]]
    assert json.loads(D.metagame_spec(weights=[[1, 2, 3]] * 3))['metagame']['weights'] == [[1, 2, 3]] * 3
    for bad in ({('x', 'y'): 2}, [[1, 1], [1, 1]], [[0] * 3] * 3, [[1, 1.5, 1]] * 3, [[1, -1, 1]] * 3, [[True] * 3] * 3):
        with pytest.raises(ValueError):
            D.metagame_spec(weights=bad)
    with pytest.raises(ValueError):
        D.metagame_spec([D.IZZET_DELVER, D.IZZET_DELVER])
    with pytest.raises(ValueError):
        D.metagame_spec(game_episodes=True)                                          # a best-of-three setting


def test_pair_spec_is_the_metagame_cell():
    """pair_spec(a, b) is the metagame spec with that cell's lists as `decks` (the engine's Metagame::pair_json)."""
    for fmt in ('bo1', 'bo3'):
        m = json.loads(D.metagame_spec(fmt=fmt))
        ds = m.pop('metagame')['decks']
        for i, a in enumerate(D.META3):
            for j, b in enumerate(D.META3):
                want = dict(m, decks={'p0': ds[i]['main'], 'p1': ds[j]['main']})
                if fmt == 'bo3':
                    want['decks'].update(p0_sideboard=ds[i]['sideboard'], p1_sideboard=ds[j]['sideboard'])
                assert json.loads(D.pair_spec(a, b, fmt=fmt)) == want, (fmt, a, b)


def test_the_presets():
    p, smoke = A.PRESETS['legacy-meta3'], A.PRESETS['legacy-meta3-smoke']
    assert p['spec'] == smoke['spec'] == A.SPEC_META3 == D.metagame_spec(D.META3, fmt='bo1')
    # every evaluation plays the metagame (no "decks": "eval")
    assert p['eval_spec'] == smoke['eval_spec'] == eval_spec(p['spec']) == p['spec']
    m = A.PRESETS['delver-mirror']
    for k in ('sims', 'sims_fast', 'p_full', 'sims_minor', 'hidden', 'blocks', 'heads', 'ff', 'arch', 'games', 'batch',
              'lr', 'window', 'yardstick', 'eval_games', 'elo_games', 'policy_target', 'infer_device', 'shard_sparse'):
        assert p[k] == m[k], k
    assert p['publish'] == {} and p['yardsticks'] == {}                                  # the Delver mirror's companions off
    s = A.PRESETS['mtg-smoke']
    assert (smoke['hidden'], smoke['blocks'], smoke['sims'], smoke['iters']) == (s['hidden'], s['blocks'], s['sims'], s['iters'])


def test_the_engine_parses_and_refuses():
    e = G.engine()
    e.mtg_spec_check(A.SPEC_META3)
    e.mtg_spec_check(D.metagame_spec(fmt='bo3', game_episodes=True))
    e.mtg_spec_check(D.metagame_spec(fmt='bo3', game_episodes=True, start_game=2, weights=[[1, 0, 0], [0, 1, 0], [0, 0, 1]]))
    # dealt decks in the best-of-three metagame (docs/ENGINE.md 223: the stew's pairings); best of one still refuses them
    e.mtg_spec_check(json.dumps(dict(json.loads(D.metagame_spec(fmt='bo3')), dealt_decks={'p': 0.5, 'swaps': [0, 4]})))
    for spec, why in [
        (D.metagame_spec(fmt='bo3', game_episodes=True, start_game=2), 'start_game 2 is for mirrors'),
        (json.dumps(dict(json.loads(D.metagame_spec(fmt='bo1')), dealt_decks={'p': 0.5, 'swaps': [0, 4]})), 'dealt_decks'),
        (json.dumps(dict(json.loads(D.metagame_spec(fmt='bo3')), keep_deck=[0])), 'keep_deck'),
        (json.dumps(dict(json.loads(A.SPEC_META3), hidden_decklists=True)), 'hidden_decklists'),
        (json.dumps(dict(json.loads(A.SPEC_META3), constructed=False)), 'constructed'),
    ]:
        with pytest.raises(ValueError, match=why):
            e.mtg_spec_check(spec)


def test_each_game_plays_and_names_its_pairing():
    e = G.engine()
    meta = json.loads(A.SPEC_META3)
    cells = set()
    for seed in range(60):
        g = e.Game('mtg', A.SPEC_META3, 2, seed)
        st = json.loads(g.export_state())
        p = st['match']['pairing']
        assert p['names'] == [meta['metagame']['decks'][i]['name'] for i in p['decks']]
        cells.add(tuple(p['decks']))
        # the replay viewer's mains are the game's pairing's
        mains = R.main_decks(meta, st)
        assert mains == {q: R._tally(meta['metagame']['decks'][i]['main']) for q, i in enumerate(p['decks'])}
        # the same seed again: the same pairing
        assert json.loads(e.Game('mtg', A.SPEC_META3, 2, seed).export_state())['match']['pairing'] == p
    assert len(cells) == 9
    with pytest.raises(ValueError, match='pairing'):
        R.main_decks(meta)                                                       # a metagame's decks need the game
    # a fixed spec's state has no pairing
    assert 'pairing' not in json.loads(e.Game('mtg', D.pair_spec(D.IZZET_DELVER, D.SNEAK_SHOW), 2, 1).export_state())['match']
