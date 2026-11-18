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
            'opp_library': 40, 'opp_top_known': False, 'fetch_available': False, 'fetch_in_hand': 0,
            'fetch_on_battlefield': 0}
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
    # a fetchland it can't use before the delayed draw (in hand off its land drop, tapped on its own turn): a blunder
    ({'bauble': 'mishra', 'fetch_in_hand': 1}, (None, True, 'self_with_unusable_fetch')),
    ({'bauble': 'mishra', 'fetch_on_battlefield': 1}, (None, True, 'self_with_unusable_fetch')),
    ({'bauble': 'mishra', 'fetch_in_hand': 2, 'opp_top_known': True}, (None, False, 'ok_opponent_top_known')),
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
          opp_top_mask=0, active=None, step='main1', lands_played=0, tapped=(), bf_extra=(), decision=None):
    pl = [None, None]
    pl[p] = {'hand': [{'name': n} for n in my_hand], 'library': 30, 'lands_played': lands_played,
             'battlefield': [{'name': n, 'controller': p, 'tapped': n in tapped} for n in (*my_bf, *bf_extra)]}
    pl[1 - p] = {'hand': [{'name': n} for n in opp_hand], 'battlefield': [], 'library': opp_lib, 'lands_played': 0}
    known = [None, None]
    known[p] = {'hand': [], 'library': []}
    known[1 - p] = {'hand': [[i, n] for i, n in enumerate(known_opp_hand)],
                    'library': [[0, opp_top_mask, 'Island']] if opp_top_mask else []}
    st = {'active': p if active is None else active, 'step': step, 'turn': 3, 'players': pl, 'known': known, 'stack': []}
    if decision is not None:
        st['decision'] = decision
    return st


def mishra_self(st, p=0):
    """The row of a Mishra's Bauble at itself by `p` from state `st` (the Reader's)."""
    R = GB.Reader()
    R.step(1, st, {'kind': 'Priority', 'player': p}, [f"T3 Main1 P{p} activates Mishra's Bauble (Activated(0))"])
    R.step(2, {}, {'kind': 'Target', 'player': p, 'options': [{}, {}]}, [f'T3 Main1 target P{p}'])
    (r,) = R.finish()
    return r


@pytest.mark.parametrize('k, playable, crackable, want', [
    # in hand, its own turn, main 1, its land drop unused: it can play the fetch and crack it before the draw
    ({'my_hand': ('Polluted Delta',)}, True, 0, 'ok_self_with_fetch'),
    ({'my_hand': ('Polluted Delta',), 'step': 'upkeep'}, True, 0, 'ok_self_with_fetch'),      # main 1 to come
    ({'my_hand': ('Polluted Delta',), 'step': 'declare_blockers'}, True, 0, 'ok_self_with_fetch'),   # main 2 to come
    ({'my_hand': ('Polluted Delta',), 'step': 'main2'}, True, 0, 'ok_self_with_fetch'),
    # in hand, its land drop already played this turn: the delayed draw comes first - a blunder
    ({'my_hand': ('Polluted Delta',), 'lands_played': 1}, False, 0, 'self_with_unusable_fetch'),
    # in hand on the opponent's turn (two of them): no land play before its upkeep's draw - a blunder
    ({'my_hand': ('Misty Rainforest', 'Polluted Delta'), 'active': 1, 'step': 'end'}, False, 0,
     'self_with_unusable_fetch'),
    ({'my_hand': ('Misty Rainforest',), 'active': 1, 'step': 'main1'}, False, 0, 'self_with_unusable_fetch'),
    # in hand in its own end step: no main phase left this turn
    ({'my_hand': ('Polluted Delta',), 'step': 'end'}, False, 0, 'self_with_unusable_fetch'),
    # an extra land drop (Fastbond, the engine's card data): a second land this turn is still a play
    ({'my_hand': ('Polluted Delta',), 'lands_played': 1, 'bf_extra': ('Fastbond',)}, True, 0, 'ok_self_with_fetch'),
    # an uncracked fetch on the battlefield: crackable at instant speed before the draw - the control
    ({'my_bf': ('Flooded Strand',)}, False, 1, 'ok_self_with_fetch'),
    ({'my_bf': ('Flooded Strand',), 'lands_played': 1, 'step': 'end'}, False, 1, 'ok_self_with_fetch'),
    ({'my_bf': ('Flooded Strand',), 'active': 1}, False, 1, 'ok_self_with_fetch'),
    # ... tapped: on its own turn it stays tapped past the opponent's upkeep (a blunder); on the opponent's turn it
    # untaps in the activator's untap step, before its upkeep's draw trigger resolves (crackable)
    ({'my_bf': ('Flooded Strand',), 'tapped': ('Flooded Strand',)}, False, 0, 'self_with_unusable_fetch'),
    ({'my_bf': ('Flooded Strand',), 'tapped': ('Flooded Strand',), 'active': 1}, False, 1, 'ok_self_with_fetch'),
    # no fetchland at all
    ({'my_hand': ('Island',)}, False, 0, 'self_without_fetch'),
])
def test_the_fetchland_timing(k, playable, crackable, want):
    r = mishra_self(state(**k))
    assert (r['fetch_in_hand_playable'], r['fetch_crackable'], r['blunder_reason']) == (playable, crackable, want)
    assert r['fetch_available'] == (want == 'ok_self_with_fetch')
    assert r['blunder'] == (want != 'ok_self_with_fetch')


def test_the_land_drop_agrees_with_the_engines_offer():
    play = {'kind': 'Priority', 'player': 0, 'options': [{'t': 'Pass priority'}, {'t': 'Play Polluted Delta'}]}
    no_play = {'kind': 'Priority', 'player': 0, 'options': [{'t': 'Pass priority'}]}
    c = GB.conditions(state(my_hand=('Polluted Delta',), decision=play), 0)
    assert c['land_drop_available'] and c['fetch_in_hand_playable']
    c = GB.conditions(state(my_hand=('Polluted Delta',), lands_played=1, decision=no_play), 0)
    assert not c['land_drop_available'] and not c['fetch_available']
    with pytest.raises(ValueError):       # the engine offers a land play the rule says is spent
        GB.conditions(state(my_hand=('Polluted Delta',), lands_played=1, decision=play), 0)
    with pytest.raises(ValueError):       # ... or withholds one the rule says is open
        GB.conditions(state(my_hand=('Polluted Delta',), decision=no_play), 0)
    with pytest.raises(ValueError):
        GB.conditions(state(step='Main1'), 0)


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
    assert a['land_drop_available'] and a['fetch_in_hand_playable'] and a['lands_played'] == 0
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
    # ... and in a game that stopped there (a resignation at the target decision): a row with no target, excluded
    (e,) = R.finish(False)
    assert (e['target'], e['excluded'], e['blunder'], e['opp_targetable']) == (None, 'game_ended_before_target', None, False)


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
