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


# ---------------------------------------------------------------- the twins' ids (docs/ENGINE.md 157)

def swapped(tr, x, y, k):
    """The trace with card ids x and y exchanged in every state from step k on: the old engine moved the other twin."""
    m = {x: y, y: x}
    steps = [dict(st, state=RR.renamed(st['state'], m)) if j >= k else st for j, st in enumerate(tr['steps'])]
    return dict(tr, steps=steps, final=dict(tr['final'], state=RR.renamed(tr['final']['state'], m)))


def where(st, i):
    """Card id i's place in the exported state: ('library', player, position) / (zone, player) / None."""
    for p, (pl, kn) in enumerate(zip(st['players'], st['known'])):
        if i in kn['order']:
            return ('library', p, kn['order'].index(i))
        for z in ('hand', 'graveyard', 'exile', 'battlefield'):
            if any(o['id'] == i for o in pl.get(z) or []):
                return (z, p)
    return None


def twins_in(st, names, zone):
    """Pairs of one player's twins (the same definition) both in `zone` of the state: library cards at two places
    nobody knows, or permanents."""
    out = []
    for p, (pl, kn) in enumerate(zip(st['players'], st['known'])):
        if zone == 'library':
            known = {i for i, _, _ in kn['library']}
            ids = [x for j, x in enumerate(kn['order']) if j not in known]
        else:
            ids = [o['id'] for o in pl.get(zone) or [] if o.get('owner') == p]
        for a in range(len(ids)):
            for b in range(a + 1, len(ids)):
                if ids[a] < len(names) and ids[b] < len(names) and names[ids[a]] == names[ids[b]]:
                    out.append((ids[a], ids[b]))
    return out


SWAP_SPOT = 'tamiyo_flip_precombat:r3-2b-float550_0550_s32:3:43'


def test_the_walk_pairs_hidden_twins_anew_and_keeps_the_pairing():
    """Two library twins nobody can tell apart, exchanged from a step on (and followed wherever they go next: a draw,
    a cast): the walk reaches the position, pairing them anew where they were hidden, and the spot notes it."""
    sp = BY_ID[SWAP_SPOT]
    tr = RR.trace_spot(E, sp)
    names = RR.deck_names(sp)
    k = len(tr['steps']) // 3
    pairs = twins_in(tr['steps'][k]['state'], names, 'library')
    assert pairs, 'no library twins to exchange'
    moved = [(x, y) for x, y in pairs if where(tr['final']['state'], x) != where(tr['steps'][k]['state'], x)
             or where(tr['final']['state'], y) != where(tr['steps'][k]['state'], y)]
    x, y = (moved or pairs)[0]
    r = RR.translate(E, sp, swapped(tr, x, y, k))
    assert r['actions'] == sp['actions'] and r['inserted'] == []
    assert [m for _, m in r['twins']] == [{x: y, y: x}] and r['twins'][0][0] == k
    new = RR.rerecorded(sp, tr, r, 1, sp['sig'], sp['pos'])
    assert new['rerecord']['twins'] == [[k, {str(x): y, str(y): x}]]


def test_the_walk_never_pairs_cards_a_player_can_tell_apart():
    """No new pairing for two library cards of different definitions, nor for two permanents (public: told apart by
    their place on the battlefield and their history) - the state differs."""
    sp = BY_ID[SWAP_SPOT]
    tr = RR.trace_spot(E, sp)
    names = RR.deck_names(sp)
    k = len(tr['steps']) // 3
    lib = tr['steps'][k]['state']['known'][0]['order']
    other = next(b for b in lib[1:] if names[b] != names[lib[0]])
    with pytest.raises(RR.Fail, match='the state differs'):
        RR.translate(E, sp, swapped(tr, lib[0], other, k))
    # two twin permanents (the shortest spot whose game has them)
    for sp in sorted(L['spots'], key=lambda s: len(s['actions'])):
        tr = RR.trace_spot(E, sp)
        names = RR.deck_names(sp)
        j = next((j for j, st in enumerate(tr['steps']) if twins_in(st['state'], names, 'battlefield')), None)
        if j is not None:
            break
    assert j is not None, 'no spot with two twin permanents'
    x, y = twins_in(tr['steps'][j]['state'], names, 'battlefield')[0]
    with pytest.raises(RR.Fail, match='the state differs'):
        RR.translate(E, sp, swapped(tr, x, y, j))



def top_known(st, p):
    """The top run of places of player p's library that p knows: [(place, mask, name)]."""
    lib = {e[0]: e for e in st['known'][p]['library']}
    out = []
    while len(out) in lib and lib[len(out)][1] & (1 << p):
        out.append(lib[len(out)])
    return out


def rearranged(tr, k, p, swap_names=True):
    """The trace with player p's top two library places exchanged at step k only (ids, and the places' knowledge and
    names when `swap_names`; else only a name changed - another multiset)."""
    st = copy.deepcopy(tr['steps'][k]['state'])
    kn = st['known'][p]
    lib = {e[0]: e for e in kn['library']}
    if swap_names:
        kn['order'][0], kn['order'][1] = kn['order'][1], kn['order'][0]
        lib[0][1:], lib[1][1:] = lib[1][1:], lib[0][1:]
    else:
        lib[0][2] = lib[1][2]
    steps = list(tr['steps'])
    steps[k] = dict(steps[k], state=st)
    return dict(tr, steps=steps)


def a_step(kinds):
    """(spot, its trace, step k, decider) of the shortest spot with a decision of `kinds` at which its decider knows
    its library's top two places, two cards of different definitions."""
    for sp in sorted(L['spots'], key=lambda s: len(s['actions'])):
        tr = RR.trace_spot(E, sp)
        for k, st in enumerate(tr['steps']):
            p = st['player']
            top = top_known(st['state'], p) if st['kind'] in kinds else []
            if len(top) >= 2 and top[0][2] != top[1][2]:
                return sp, tr, k, p
    raise AssertionError(f'no spot with a {kinds} decision over two known cards')


def test_a_pile_put_back_may_hold_its_waiting_cards_elsewhere():
    """At a pile decision (PutOnTop / PutOnBottom), the decider's not-yet-placed cards may wait in other places of the
    pile (152.1 moved them from the card index to the place): accepted, noted, nothing else relaxed - and the same
    rearrangement at another decision, or another multiset at the pile, is refused."""
    sp, tr, k, p = a_step(RR.PILE)
    r = RR.translate(E, sp, rearranged(tr, k, p))
    assert r['actions'] == sp['actions'] and r['pile'] == [k] and r['twins'] == []
    with pytest.raises(RR.Fail, match='the state differs'):
        RR.translate(E, sp, rearranged(tr, k, p, swap_names=False))
    sp, tr, k, p = a_step(('Priority',))
    with pytest.raises(RR.Fail, match='the state differs'):
        RR.translate(E, sp, rearranged(tr, k, p))
