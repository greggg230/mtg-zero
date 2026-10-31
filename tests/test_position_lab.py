"""tools/position_lab.py (docs/ENGINE.md 76): spots rebuilt from replays, the start block's deals (paired
across candidates, the actual hand kept, pins held, the belief's direction), forced lines through
zero-core's driver, the site-id mapping. CPU only, no network (zero.infer.UniformEvaluator)."""
import glob
import gzip
import json
import os
import random
import sys

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import position_lab as L  # noqa: E402

import mtgz.az as A  # noqa: E402

E = L.engine()
FOW = 'Force of Will'


def spec(log=False):
    sp = json.loads(A.SPEC_DELVER) if isinstance(A.SPEC_DELVER, str) else dict(A.SPEC_DELVER)
    if log:
        sp['log'] = True
    return sp


def fake_replay(seed=7, moves=60, pick=0):
    """A zero-replay/1-shaped record of a random Delver mirror game: the frames of its multi-option
    decisions (as the recorder stores them), the states at those frames (the log on)."""
    rep = {'format': 'zero-replay/1', 'game': 'mtg', 'seed': seed, 'game_index': 0,
           'spec': json.dumps(spec(log=True)), 'title': 'Generation 1 #1', 'frames': [], 'states': []}
    from zero import mcts
    g = E.Game('mtg', rep['spec'], 2, L.replay_seed(rep))
    while not g.over and len(g.legal()) == 1:
        mcts.transition(g, g.legal()[0])
    r = random.Random(seed + pick)
    for _ in range(moves):
        if g.over:
            break
        acts = list(g.legal())
        a = acts[r.randrange(len(acts))]
        rep['frames'].append({'s': len(rep['states']), 'p': g.to_move, 'k': 'nn', 'acts': acts, 'a': a})
        rep['states'].append(json.loads(g.export_state()))
        mcts.transition(g, a)
    return rep


def lab_game(spot, **start):
    return E.Game('mtg', L.start_spec(spot, start), 2, 0)


def hand(g, p):
    st = json.loads(g.export_state())
    return sorted(c['name'] for c in st['players'][p]['hand'])


def libs(g):
    return json.loads(g.export_state())['lab']['libraries']


@pytest.fixture(scope='module')
def spot():
    rep = fake_replay()
    s = L.spot_from_replay(rep, 40, 'fake.json.gz')
    assert len(hand(L.spot_game(s), 1 - s['decider'])) >= 3, 'a hand to deal'
    return s


def test_the_spot_rebuilt_matches_the_replay_frame(spot):
    g = L.check_spot(spot)
    assert g.to_move == spot['decider'] and list(g.legal()) == spot['recorded']['acts']
    # a frame whose state does not match is an error, never another spot
    bad = dict(spot, state=dict(spot['state'], turn=spot['state']['turn'] + 1))
    with pytest.raises(SystemExit, match='differs from the replay frame'):
        L.check_spot(bad)


REAL = sorted(glob.glob(os.path.join(L.REPLAYS, '*.json.gz')))


@pytest.mark.skipif(not REAL, reason='no main-line replays on this machine')
def test_real_replays_rebuild_at_every_tenth_frame():
    for path in REAL[-3:]:
        rep = L.load_replay(path)
        for k in range(0, len(rep['frames']), 10):
            L.check_spot(L.spot_from_replay(rep, k, path))


def test_paired_deals_are_identical_across_candidates(spot):
    for mode, extra in (('uniform', {}), ('actual', {})):
        for ds in range(5):
            worlds = []
            for forced in ([], ['*'], [spot['recorded']['a']], ['Pass priority', '*']):
                g = lab_game(spot, deal=mode, deal_seed=ds, forced=forced, **extra)
                st = json.loads(g.export_state())
                worlds.append((st['players'][0]['hand'], st['players'][1]['hand'], st['lab']['libraries']))
            assert all(w == worlds[0] for w in worlds), (mode, ds)
    # another deal seed, another world
    a = libs(lab_game(spot, deal='uniform', deal_seed=1))
    b = libs(lab_game(spot, deal='uniform', deal_seed=2))
    assert a != b


def test_the_actual_mode_keeps_the_real_hand(spot):
    real = L.spot_game(spot)
    d = spot['decider']
    orders = set()
    for ds in range(8):
        g = lab_game(spot, deal='actual', deal_seed=ds)
        st, rs = json.loads(g.export_state()), json.loads(real.export_state())
        st.pop('lab')
        rs.pop('lab')
        assert st == rs, 'everything a spectator sees (both hands) is the real game'
        for p in (0, 1):
            assert sorted(libs(g)[p]) == sorted(libs(real)[p])
        orders.add(tuple(libs(g)[d]))
    assert len(orders) > 4, "the decider's unknown library order is dealt again"


def test_pinned_cards_are_in_hand_and_excluded_cards_are_not(spot):
    o = 1 - spot['decider']
    for ds in range(25):
        h = hand(lab_game(spot, deal='uniform', deal_seed=ds, hold=[FOW], exclude=['Daze', 'Lightning Bolt']), o)
        assert FOW in h and 'Daze' not in h and 'Lightning Bolt' not in h, h
    free = [hand(lab_game(spot, deal='uniform', deal_seed=ds), o) for ds in range(25)]
    assert any(FOW not in h for h in free)
    with pytest.raises(Exception, match='no deal can give'):
        lab_game(spot, deal='uniform', deal_seed=0, hold=[FOW] * 5)


def test_belief_mode_deals_toward_the_prediction(spot):
    """A constructed belief (the opponent's hand mostly Force of Will) against the uniform deal: FoW in
    the dealt hands far more often, and a mix share of uniform deals in between."""
    from mtgz.az import belief as B
    names = B.names()
    pred = [1e-4] * B.D
    pred[names.index(FOW)] = 0.5
    tot = sum(pred)
    pred = [p / tot for p in pred]
    o = 1 - spot['decider']

    def fow(**kw):
        return sum(hand(lab_game(spot, deal_seed=ds, **kw), o).count(FOW) for ds in range(120))
    uni, mixed, bel = fow(deal='uniform'), fow(deal='belief', pred=pred, mix=0.5), fow(deal='belief', pred=pred)
    assert bel > 1.8 * uni + 5 and uni < mixed < bel, (uni, mixed, bel)


def test_no_start_block_is_the_game_of_before():
    sp = json.dumps(spec())
    g = E.Game('mtg', sp, 2, 3)
    assert 'lab' not in json.loads(g.export_state())
    # an empty prefix with nothing dealt is the same game, move for move
    h = E.Game('mtg', L.start_spec({'spec': spec(), 'seed': 3, 'actions': []}, {'deal': 'none'}), 2, 99)
    from zero import mcts
    while not g.over and len(g.legal()) == 1:
        mcts.transition(g, g.legal()[0])
    r = random.Random(1)
    for _ in range(150):
        if g.over:
            break
        assert list(g.legal()) == list(h.legal())
        a, b = json.loads(g.export_state()), json.loads(h.export_state())
        b.pop('lab')
        assert a == b
        x = r.choice(list(g.legal()))
        mcts.transition(g, x)
        mcts.transition(h, x)


def test_forced_lines_through_the_driver(spot):
    """zero-core's driver plays lab games from queued setups (UniformEvaluator, 2 simulations): every row
    tagged, the forced line taken (its single option stepped without a search), deals paired."""
    from zero.infer import UniformEvaluator
    names = dict(L.options(L.spot_game(spot)))
    a = spot['recorded']['a']
    cands = [{'name': 'actual', 'forced': [names[a]]}, {'name': 'pass', 'forced': ['Pass priority', '*']}]
    if names[a] == 'Pass priority':
        cands[1] = {'name': 'other', 'forced': [next(t for t in names.values() if t != 'Pass priority')]}
    modes = [('uniform', {}), ('actual', {})]
    out = {}
    for ci, cand in enumerate(cands):
        rows, perf, specs = L.play_one(spot, modes, cand, ci, 0, UniformEvaluator(), {}, 1.0, 2, 3, seed=5, conc=8,
                                       threads=1, move_log=40)
        assert sorted(rows) == sorted(f'0|{m}|{ci}|{i}' for m, _ in modes for i in range(3))
        assert perf['games'] == 6 and perf['cut'] == 0
        for t, r in rows.items():
            assert r['over'] and r['tag'] == t
            info = L.diagnose(specs[t], r, spot['decider'])
            assert info['broken'] is None
            # the decider's first logged move is the forced option, alone
            first = next(m for m in r['moves'] if m[0] == spot['decider'])
            assert len(first[2]) == 1
        out[ci] = specs
    # the same deal i for both candidates (their specs differ only in the forced line)
    for m, _ in modes:
        for i in range(3):
            s0 = json.loads(out[0][f'0|{m}|0|{i}'])['start']
            s1 = json.loads(out[1][f'0|{m}|1|{i}'])['start']
            assert s0['deal_seed'] == s1['deal_seed'] and s0['forced'] != s1['forced']


def test_summaries_pair_by_deal():
    spot = {'decider': 1, 'state': {'turn': 4}}
    rows = {}
    for ci, wins in enumerate(([1, 0, 1, 1], [1, 0, 0, 1])):
        for i, w in enumerate(wins):
            rows[f'0|m|{ci}|{i}'] = {'result': [1 - w, w], 'rounds': 10, 'over': True, 'decisions': 50}
    res, diffs = L.summarize(spot, 'm', [{'name': 'x'}, {'name': 'y'}], rows, 4, 0)
    assert res[0]['score'] == 0.75 and res[1]['score'] == 0.5 and res[0]['best']
    assert res[1]['vs_best']['diff'] == -0.25 and res[1]['turns_from_spot'] == 6
    assert diffs[1] == [0.0, 0.0, -1.0, 0.0]


def test_site_ids_map_to_the_publishers_files(tmp_path):
    run = tmp_path / 'r3-x'
    (run / 'replays').mkdir(parents=True)
    for k in range(2):
        rep = {'game_index': k, 'title': f'Generation 312 #{k + 1}'}
        with gzip.open(run / 'replays' / f'20261031-102752_{k:03d}.json.gz', 'wt', encoding='utf-8') as f:
            json.dump(rep, f)
    log = tmp_path / 'pub.log'
    ck = str(run / 'models' / 'iter_0312.pt').replace('/', '\\')
    log.write_text(f'10:23:59 recording 2 games of iteration 312 ({ck})\n10:27:52 recorded 2 games in 233s (1 simulations)\n'
                   "10:27:53   uploaded {'ok': True, 'id': '183828fc4d00'}\n10:27:53   uploaded {'ok': True, 'id': '641c86f53e36'}\n",
                   encoding='utf-8')
    assert L.site_index(str(log))['641c86f53e36'][1:3] == (312, 1)
    path = L.replay_for_site_id('641c86f53e36', str(log))
    assert path.endswith('20261031-102752_001.json.gz')
    with pytest.raises(SystemExit, match='not in the publisher log'):
        L.replay_for_site_id('000000000000', str(log))


def test_candidates_parse():
    assert L.parse_cand("Cast Mishra's Bauble; Pass priority ;Activate Clue [12]")['forced'] == \
        ["Cast Mishra's Bauble", 'Pass priority', 12]
    c = L.parse_cand('{"name": "x", "forced": ["*"], "follow": {"rule": "bauble_fetch"}}')
    assert c['follow'] == {'rule': 'bauble_fetch'} and c['name'] == 'x'
