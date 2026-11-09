"""The Delver canary library (data/delver_canary_library.json; docs/blunders.md 6a) on this checkout's engine,
and its re-record across a rules change (tools/delver_rerecord.py; docs/ENGINE.md 100.8). A library is
recorded for one engine's rules: when RULES_EPOCH moves (tools/position_lab.py), these tests fail until it is
re-recorded (or stamped, when every spot still reaches its position) - never a silent skip."""
import copy
import os
import sys

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import delver_blunders as DB  # noqa: E402
import delver_rerecord as RR  # noqa: E402

E = DB.load_engine(ROOT)
L = DB.load_library(DB.LIBRARY_PATH)
BY_ID = {sp['id']: sp for sp in L['spots']}


def test_the_library_is_recorded_under_the_current_rules():
    epoch = DB.rules_epoch(ROOT)
    assert L.get('rules_epoch') == epoch, (
        f'data/delver_canary_library.json was recorded under rules epoch {L.get("rules_epoch")}, the rules are now '
        f'{epoch} (tools/position_lab.py RULES_EPOCH): `python tools/delver_rerecord.py check --stamp` when every spot '
        f'still reaches its position, else `python tools/delver_rerecord.py rerecord --old-engine <a checkout of '
        f'{(L.get("engine") or {}).get("commit")}>` (docs/ENGINE.md 100.8)')
    assert L['recorded'][-1]['rules_epoch'] == epoch and L['recorded'][-1]['engine'] == L['engine']


def test_every_spot_reaches_its_recorded_position():
    bad = RR.check_spots(E, L)
    assert not bad, f'{len(bad)} of {len(L["spots"])} canary spots do not reach their position on this engine: {bad}'
    assert len({sp['id'] for sp in L['spots']}) == len(L['spots'])
    assert not set(BY_ID) & set(L.get('dropped') or {})


def old_engine_trace(sp):
    """The spot's trace on this engine with the decisions its re-record inserted taken out: what the old engine
    (which never asked them) recorded, up to the new engine's words."""
    tr = RR.trace_spot(E, sp)
    ins = {j for js in sp['rerecord']['inserted'].values() for j in js}
    return dict(tr, steps=[s for j, s in enumerate(tr['steps']) if j not in ins]), ins


@pytest.mark.parametrize('sid', [
    'tamiyo_flip_precombat:r3-2b-float550_0550_s32:28:192',   # priority passes and a covering payment
    'chump_block:r3-2b-float550_0550_s32:87:94',              # three trigger orders, each told by the next state
    'burn_no_kill:r3-2b-float550_0550_s32:74:192',            # an order no recorded state shows
    'bauble_trick_missed:r3-2b-float550_0550_s32:1966:56',    # an old-path index (k_main1) mapped
])
def test_the_walk_answers_the_new_questions_as_the_old_engine_did(sid):
    sp = BY_ID[sid]
    old, ins = old_engine_trace(sp)
    assert ins and len(old['steps']) == sp['rerecord']['old_steps']
    r = RR.translate(E, sp, old)
    assert r['actions'] == sp['actions']
    assert {j for j, _, _ in r['inserted']} == ins
    assert r['order_unseen'] == sp['rerecord'].get('order_unseen', [])
    # the library's spot is what rerecord writes from it (the old-path indices from the recording engine's)
    back = dict(sp)
    for key, k in (sp['rerecord'].get('old_indices') or {}).items():
        back[key] = k
    st = old['final']['state']
    new = RR.rerecorded(back, old, r, sp['rerecord']['epoch'], DB.spot_sig(st), DB.spot_pos(st))
    assert new == sp


def test_the_walk_keeps_a_path_that_still_replays():
    sp = BY_ID['tamiyo_flip_precombat:r3-2b-float550_0550_s32:3:43']
    r = RR.translate(E, sp, RR.trace_spot(E, sp))
    assert r['actions'] == sp['actions'] and r['inserted'] == [] and r['matched'] == list(range(len(sp['actions'])))


def test_the_walk_refuses_another_position():
    sp = BY_ID['tamiyo_flip_precombat:r3-2b-float550_0550_s32:13:64']
    old, _ = old_engine_trace(sp)
    other = copy.deepcopy(old)
    other['steps'][30]['state']['players'][1]['life'] -= 1            # the old game was another game
    with pytest.raises(RR.Fail, match='the state differs'):
        RR.translate(E, sp, other)
    cast = next(j for j, s in enumerate(old['steps']) if (s['t'] or '').startswith('Cast '))
    lost = dict(old, steps=old['steps'][:cast] + old['steps'][cast + 1:])   # a real move missing: no path
    with pytest.raises(RR.Fail, match='no path on this engine reaches the position'):
        RR.translate(E, sp, lost)
    gone = dict(sp, watch='Cast Lightning Bolt')                       # the probed decision must be the same
    with pytest.raises(RR.Fail, match='not offered at the spot'):
        RR.translate(E, gone, old)
