"""The game database's Bauble targeting measure (tools/gamedb_bauble.py, docs/gamedb.md "Bauble targeting",
docs/ENGINE.md 194): the user's blunder rules case by case; the reader on hand-made play-by-play; and on random-move
Delver mirror games ingested into a database - one row per Bauble activation the play-by-play shows, each row's
conditions the engine state's at its activating action (recomputed here from a fresh replay), its classification the
rules', the view and the per-iteration series. Needs duckdb and pyarrow (skipped without)."""
import json
import os
import random
import sys

import pytest

pytest.importorskip('duckdb')
pytest.importorskip('pyarrow')

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import mtgz.az  # noqa: E402,F401
from mtgz import _engine as E  # noqa: E402
import gamedb as GDB  # noqa: E402
import gamedb_bauble as GB  # noqa: E402
from zero import gamerec as GR  # noqa: E402

from test_combo_stats import DELVER, random_row  # noqa: E402
from test_gamedb import combo_file, table  # noqa: E402


def row(**k):
    base = {'bauble': 'urza', 'target': 'self', 'opp_targetable': True, 'opp_hand': 5, 'opp_hand_all_known': False,
            'opp_library': 40, 'opp_top_known': False, 'fetch_available': False}
    base.update(k)
    return base


@pytest.mark.parametrize('k, want', [
    # Urza's Bauble
    ({}, (None, True, 'self_with_unknown_opponent_hand')),
    ({'target': 'opponent'}, (None, False, 'ok_target_opponent')),
    ({'opp_hand': 0, 'opp_hand_all_known': True}, (None, False, 'ok_opponent_hand_empty')),
    ({'opp_hand_all_known': True}, (None, False, 'ok_opponent_hand_known')),
    ({'fetch_available': True}, (None, True, 'self_with_unknown_opponent_hand')),     # a fetch is no excuse for Urza's
    ({'opp_targetable': False}, ('opponent_not_targetable', None, None)),
    # Mishra's Bauble
    ({'bauble': 'mishra'}, (None, True, 'self_without_fetch')),
    ({'bauble': 'mishra', 'fetch_available': True}, (None, False, 'ok_self_with_fetch')),
    ({'bauble': 'mishra', 'opp_top_known': True}, (None, False, 'ok_opponent_top_known')),
    ({'bauble': 'mishra', 'opp_top_known': True, 'fetch_available': True}, (None, False, 'ok_opponent_top_known')),
    ({'bauble': 'mishra', 'opp_hand_all_known': True}, (None, True, 'self_without_fetch')),   # hand knowledge: Urza's only
    ({'bauble': 'mishra', 'target': 'opponent'}, (None, False, 'ok_target_opponent')),
    ({'bauble': 'mishra', 'opp_library': 0}, ('opponent_library_empty', None, None)),
    ({'bauble': 'mishra', 'opp_library': 0, 'target': 'opponent'}, ('opponent_library_empty', None, None)),
    ({'bauble': 'mishra', 'opp_targetable': False}, ('opponent_not_targetable', None, None)),
])
def test_the_blunder_rules(k, want):
    assert GB.classify(row(**k)) == want


def state(p=0, opp_hand=('Daze', 'Brainstorm'), known_opp_hand=(), my_hand=('Ponder',), my_bf=(), opp_lib=40,
          opp_top_mask=0):
    pl = [None, None]
    pl[p] = {'hand': [{'name': n} for n in my_hand], 'battlefield': [{'name': n, 'controller': p} for n in my_bf],
             'library': 30}
    pl[1 - p] = {'hand': [{'name': n} for n in opp_hand], 'battlefield': [], 'library': opp_lib}
    known = [None, None]
    known[p] = {'hand': [], 'library': []}
    known[1 - p] = {'hand': [[i, n] for i, n in enumerate(known_opp_hand)],
                    'library': [[0, opp_top_mask, 'Island']] if opp_top_mask else []}
    return {'active': p, 'players': pl, 'known': known}


def test_the_reader_on_play_by_play():
    R = GB.Reader()
    st = state(my_hand=('Polluted Delta', 'Brainstorm'), opp_top_mask=0b10)       # P0 activates; P1's top known to P1
    R.step(5, st, {'kind': 'Priority', 'player': 0}, ["T2 Main1 P0 activates Mishra's Bauble (Activated(0))"])
    R.step(6, st, {'kind': 'Target', 'player': 0, 'options': [{'t': 'Target P1'}, {'t': 'Target P2'}]},
           ['T2 Main1 target P0', "T2 Main1 Mishra's Bauble Battlefield -> Graveyard"])
    # an Urza's Bauble whose target the engine answered alone (one legal target: the opponent untargetable)
    st2 = state(p=1, opp_hand=('Daze',))
    R.step(9, st2, {'kind': 'Priority', 'player': 1}, ["T3 Main1 P1 activates Urza's Bauble (Activated(0))", 'T3 Main1 target P1'])
    rows = R.finish()
    assert [(r['action'], r['activator'], r['bauble'], r['target']) for r in rows] == [(5, 0, 'mishra', 'self'), (9, 1, 'urza', 'self')]
    a, b = rows
    assert a['opp_targetable'] and a['fetch_in_hand'] == 1 and a['fetch_available'] and a['brainstorm_in_hand']
    assert a['manipulation_in_hand'] == ['Brainstorm'] and not a['opp_top_known']    # (the owner's knowledge, not P0's)
    assert (a['excluded'], a['blunder'], a['blunder_reason']) == (None, False, 'ok_self_with_fetch')
    assert not b['opp_targetable'] and b['excluded'] == 'opponent_not_targetable' and b['blunder'] is None
    # the activator's knowledge: the opponent's top card known to the activator, every opponent hand card known
    R = GB.Reader()
    R.step(1, state(opp_top_mask=0b01), {}, ["T1 Main1 P0 activates Mishra's Bauble (Activated(0))"])
    R.step(2, {}, {'kind': 'Target', 'player': 0, 'options': [{}, {}]}, ['T1 Main1 target P0'])
    R.step(3, state(known_opp_hand=('Daze', 'Brainstorm')), {}, ["T1 Main1 P0 activates Urza's Bauble (Activated(0))"])
    R.step(4, {}, {'kind': 'Target', 'player': 0, 'options': [{}, {}]}, ['T1 Main1 target P0'])
    c, d = R.finish()
    assert c['opp_top_known'] and c['blunder_reason'] == 'ok_opponent_top_known'
    assert d['opp_hand_all_known'] and d['opp_hand_known'] == 2 and d['blunder_reason'] == 'ok_opponent_hand_known'
    # an activation that never chose a target in a game that ended is a broken read
    R = GB.Reader()
    R.step(1, state(), {}, ["T1 Main1 P0 activates Urza's Bauble (Activated(0))"])
    with pytest.raises(ValueError):
        R.finish(True)
    assert R.finish(False) == []


@pytest.fixture(scope='module')
def mirror(tmp_path_factory):
    base = tmp_path_factory.mktemp('gamedb_bauble')
    p = str(base / 'm.b000.games.json.gz')
    combo_file(p, (DELVER, DELVER), 314, 60, random.Random(11))
    run = str(base / 'run')
    GDB.import_combo(run, [p], it=1, log=lambda *a: None)
    db = str(base / 'db')
    GDB.ingest(run, db, sample=1.0, log=lambda *a: None)
    return run, db


def test_random_mirror_games(mirror):
    run, db = mirror
    con = GDB.connect(db)
    rows = table(con, 'SELECT * FROM bauble_activations ORDER BY game_id, idx')
    acts = table(con, "SELECT game_id, action, player, card FROM events WHERE type = 'activate' AND card LIKE '%Bauble' "
                      'ORDER BY game_id, seq')
    assert len(rows) >= 10, 'random Delver mirror games activate Baubles'
    assert [(r['game_id'], r['action'], r['activator']) for r in rows] == [(a['game_id'], a['action'], a['player']) for a in acts]
    assert {r['kind'] for r in rows} == {'selfplay'} and all(r['pairing'] == f'{DELVER} v {DELVER}' for r in rows)
    # every row's conditions are the engine state's at its activating action, its classification the rules'
    specs, recs = GR.read_iteration(run, 1)
    by_id = {GDB.game_id(r): r for r in recs}
    for r in rows:
        rec = by_id[r['game_id']]
        sp = json.loads(specs[rec['spec']])
        sp['log'] = True
        g = E.Game('mtg', json.dumps(sp), 2, int(rec['seed']))
        for a in GDB.actions_of(rec)[:r['action']]:
            g.step(int(a))
        st = json.loads(g.export_state())
        want = GB.conditions(st, r['activator'])
        assert {k: r[k] for k in want} == want
        assert (r['excluded'], r['blunder'], r['blunder_reason']) == GB.classify(r)
    # the series: per iteration and Bauble, its counts the rows'
    S = GDB.bauble_blunder_series(run, db=db)
    tot = next(x for x in S if x['bauble'] == 'all')
    assert tot['activations'] == len(rows) and tot['blunders'] == sum(1 for r in rows if r['blunder'])
    assert tot['counted'] == sum(1 for r in rows if r['excluded'] is None)
    assert sum(x['activations'] for x in S if x['bauble'] != 'all') == len(rows)
    if tot['counted']:
        assert tot['blunder_share'] == pytest.approx(tot['blunders'] / tot['counted'])
    assert GDB.bauble_blunder_series(run, db=db, kind='selfplay') == S
    assert GDB.bauble_blunder_series(run, db=db, kind='eval') == []


def test_cluster_share():
    assert GB.cluster_share([]) == (None, None)
    assert GB.cluster_share([(1, 2)]) == (0.5, None)
    p, se = GB.cluster_share([(1, 1), (0, 1), (1, 1), (0, 1)])
    assert p == 0.5 and se == pytest.approx((4 / 3 * 4 * 0.25) ** 0.5 / 4)
