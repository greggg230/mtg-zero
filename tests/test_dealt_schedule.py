"""The dealt share's schedule (docs/ENGINE.md 254, mtgz.az.dealt_schedule): the schedule's checks and p, its
selfplay_specs hook (chained after the matchup model's table and before the forced find's p), its refusals, the package
setting and the engine's parse of the hook's specs through the bindings. No network, no process started."""
import json
import os
import sys

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
import mtgz.az as A  # noqa: E402
from mtgz import _engine as E  # noqa: E402
from mtgz.az import decklists as D, dealt_schedule as DS, matchup as MU  # noqa: E402

DD, DELVER = 'doomsday_8027957', 'izzet_delver_8016541'
SCHED = {'start': 0.85, 'end': 0.15, 'from': 64, 'iters': 128}


class FakeRun:
    def __init__(self, cfg, run='r'):
        self.cfg = cfg
        self.run = run
        self.lines = []

    def log(self, s):
        self.lines.append(s)


def test_the_schedule():
    assert DS.p_at({}, 5) is None and DS.p_at(None, 5) is None
    assert [DS.p_at(SCHED, i) for i in (0, 64, 96, 128, 192, 500)] == [0.85, 0.85, 0.675, 0.5, 0.15, 0.15]
    assert DS.p_at({'start': 0.3}, 999) == 0.3 and DS.p_at({'start': 0.3, 'end': 0.3}, 7) == 0.3
    for bad in ([1], {'end': 0.1}, {'start': 1.5}, {'start': 0.5, 'end': 0.0}, {'start': 0}, {'start': True}, {'start': 0.5, 'end': 0.1},
                {'start': 0.5, 'iters': -1}, {'start': 0.5, 'from': 1.5}, {'start': 0.5, 'until': 3}):
        with pytest.raises(ValueError):
            DS.check_schedule(bad)


def test_the_hook_writes_the_iterations_p_and_leaves_the_rest_of_the_spec():
    spec = A.SPEC_DELVER_BO3_VARIED
    R = FakeRun({'spec': spec, 'dealt_decks_schedule': {}, 'matchup_every': 0})
    assert D.selfplay_spec(R, 3) is None                      # no schedule: the run's spec (its own p)
    R.cfg['dealt_decks_schedule'] = {'start': 0.8, 'end': 0.2, 'from': 1, 'iters': 4}
    got = [json.loads(D.selfplay_spec(R, it))['dealt_decks'] for it in range(7)]
    assert [g['p'] for g in got] == [0.8, 0.8, 0.65, 0.5, 0.35, 0.2, 0.2]
    assert all(g['swaps'] == [0, 15] and g['per_seat'] is True for g in got)     # only p moves
    assert D.selfplay_spec(R, 2) == D.selfplay_spec(R, 2)     # every call for one iteration answers the same
    sp = json.loads(D.selfplay_spec(R, 3))
    assert {k: v for k, v in sp.items() if k != 'dealt_decks'} == {k: v for k, v in json.loads(spec).items() if k != 'dealt_decks'}
    assert any('dealt_decks p = 0.5' in ln for ln in R.lines)
    for it in range(7):                                       # the hook's specs are the engine's
        E.mtg_spec_check(D.selfplay_spec(R, it))


def test_the_refusals():
    from zero.run import EncodingMismatch
    spec = A.SPEC_DELVER_BO3_VARIED
    # a schedule on a spec that deals no decks
    bad = FakeRun({'spec': A.SPEC_DELVER_BO3, 'dealt_decks_schedule': {'start': 0.5}, 'matchup_every': 0})
    with pytest.raises(EncodingMismatch, match='dealt_decks'):
        D.selfplay_spec(bad, 0)
    # a malformed schedule
    bad = FakeRun({'spec': spec, 'dealt_decks_schedule': {'start': 3}, 'matchup_every': 0})
    with pytest.raises(EncodingMismatch):
        D.selfplay_spec(bad, 0)
    # p 0 (the engine's share is in (0, 1]; the matchup model learns from the dealt seats): a floor is required
    bad = FakeRun({'spec': spec, 'dealt_decks_schedule': {'start': 0.85, 'end': 0.0, 'iters': 4}, 'matchup_every': 0})
    with pytest.raises(EncodingMismatch, match='floor'):
        D.selfplay_spec(bad, 0)
    with pytest.raises(ValueError, match='dealt_decks.p'):
        E.mtg_spec_check(json.dumps(dict(json.loads(spec), dealt_decks={'p': 0.0, 'swaps': [0, 15], 'per_seat': True})))


def test_the_chain_keeps_the_matchup_table_and_the_forced_find(tmp_path, monkeypatch):
    # the matchup model's spec (a choice table) passes through with the iteration's p
    spec = A.SPEC_DELVER_BO3_VARIED
    with_table = json.dumps(dict(json.loads(spec), sideboard_table={'version': 'v0002'}))
    monkeypatch.setattr(MU, 'selfplay_spec', lambda R, it: with_table if it >= 4 else None)
    R = FakeRun({'spec': spec, 'dealt_decks_schedule': {'start': 0.8, 'end': 0.2, 'iters': 4}, 'matchup_every': 2})
    s2, s4 = json.loads(D.selfplay_spec(R, 2)), json.loads(D.selfplay_spec(R, 4))
    assert 'sideboard_table' not in s2 and s2['dealt_decks']['p'] == 0.5
    assert s4['sideboard_table'] == {'version': 'v0002'} and s4['dealt_decks']['p'] == 0.2
    # schedule off: the matchup model's spec, unchanged
    R.cfg['dealt_decks_schedule'] = {}
    assert D.selfplay_spec(R, 4) == with_table and D.selfplay_spec(R, 2) is None
    # a stew specialist's spec (the pairing's metagame spec with dealt decks and the forced find): both schedules write
    monkeypatch.undo()
    pair, _ = A.pair_varied_specs(DELVER, DD)
    R = FakeRun({'spec': pair, 'matchup_every': 0, 'dealt_decks_schedule': dict(SCHED), 'forced_find_schedule': {}})
    s = json.loads(D.selfplay_spec(R, 128))
    assert s['dealt_decks'] == dict(A.PAIR_DEALT, p=0.5) and s['forced_find'] == json.loads(pair)['forced_find']
    assert {k: v for k, v in s.items() if k != 'dealt_decks'} == {k: v for k, v in json.loads(pair).items() if k != 'dealt_decks'}
    E.mtg_spec_check(json.dumps(s))


def test_the_package_setting():
    assert A.PACKAGE.settings['dealt_decks_schedule'] == {} and A.SETTING_CLASSES['dealt_decks_schedule'] == 'trainer'
    assert all('dealt_decks_schedule' not in p or not p['dealt_decks_schedule'] for p in A.PRESETS.values()), 'off in every preset'
