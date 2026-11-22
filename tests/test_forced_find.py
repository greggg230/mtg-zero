"""The forced find (docs/ENGINE.md 180, mtgz.az.forced_find): the spec helpers, the schedule and its selfplay_specs hook
(chained after the matchup model's), where it is dropped (the evaluation spec, the recorded matches, the census, the
yardsticks' derived best of one) and where it is refused (keep_h2h, an explicit bo1_spec, the cells, combo_stats play, a
run's eval_spec), the presets, the engine's parse through the bindings, and tools/combo_stats.py's classes of the Oracle
on random-move games with the forced find. No network, no process started."""
import json
import os
import random
import sys

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import mtgz.az as A  # noqa: E402
from mtgz import _engine as E  # noqa: E402
from mtgz.az import decklists as D, evaldecks, forced_find as FF  # noqa: E402

DD, SNS, DELVER = 'doomsday_8027957', 'sneak_show_8014220', 'izzet_delver_8016541'


class FakeRun:
    def __init__(self, cfg, run='r'):
        self.cfg = cfg
        self.run = run
        self.lines = []

    def log(self, s):
        self.lines.append(s)


def test_spec_helpers():
    s = FF.with_forced_find(A.SPEC_META3, 0.25)
    d = json.loads(s)
    assert d['forced_find'] == {'spell': 'Doomsday', 'card': "Thassa's Oracle", 'p': 0.25, 'unless_in_hand': True}
    assert json.loads(FF.strip(s)) == json.loads(A.SPEC_META3)
    assert FF.strip(A.SPEC_META3) == A.SPEC_META3
    FF.refuse(A.SPEC_META3, 'x')
    with pytest.raises(ValueError, match='forced_find'):
        FF.refuse(s, 'x')
    with pytest.raises(ValueError):
        FF.with_forced_find(A.SPEC_META3, 1.5)
    E.mtg_spec_check(s)


def test_the_schedule():
    assert FF.p_at({}, 5) is None and FF.p_at(None, 5) is None
    sch = {'start': 1.0, 'end': 0.0, 'from': 10, 'iters': 100}
    assert [FF.p_at(sch, i) for i in (0, 10, 60, 110, 500)] == [1.0, 1.0, 0.5, 0.0, 0.0]
    assert FF.p_at({'start': 0.3}, 999) == 0.3
    assert FF.p_at({'start': 0.3, 'end': 0.3}, 7) == 0.3
    for bad in ({'end': 0.0}, {'start': 2}, {'start': 1, 'iters': -1}, {'start': 1, 'from': 1.5}, {'start': 1, 'x': 1},
                {'start': 1.0, 'end': 0.0}, {'start': True}):
        with pytest.raises(ValueError):
            FF.check_schedule(bad)


def test_the_hook_writes_the_iterations_p_and_chains_after_the_matchup_model():
    spec = FF.with_forced_find(A.SPEC_META3, 1.0)
    R = FakeRun({'spec': spec, 'eval_spec': A.SPEC_META3, 'forced_find_schedule': {}, 'matchup_every': 0})
    assert D.selfplay_spec(R, 3) is None                      # no schedule: the run's spec (its own p)
    R.cfg['forced_find_schedule'] = {'start': 1.0, 'end': 0.0, 'iters': 4}
    got = [json.loads(D.selfplay_spec(R, it))['forced_find']['p'] for it in range(6)]
    assert got == [1.0, 0.75, 0.5, 0.25, 0.0, 0.0]
    assert D.selfplay_spec(R, 2) == D.selfplay_spec(R, 2)     # every call for one iteration answers the same
    sp = json.loads(D.selfplay_spec(R, 2))
    assert {k: v for k, v in sp.items() if k != 'forced_find'} == json.loads(A.SPEC_META3)
    assert any('forced_find p = 0.5' in ln for ln in R.lines)
    from zero.run import EncodingMismatch
    # a schedule without the option in the spec, and a run whose eval_spec carries it: refused (exit 78)
    bad = FakeRun({'spec': A.SPEC_META3, 'forced_find_schedule': {'start': 1.0}, 'matchup_every': 0})
    with pytest.raises(EncodingMismatch, match='forced_find'):
        D.selfplay_spec(bad, 0)
    bad = FakeRun({'spec': spec, 'eval_spec': spec, 'forced_find_schedule': {}, 'matchup_every': 0})
    with pytest.raises(EncodingMismatch, match='eval_spec'):
        D.selfplay_spec(bad, 0)
    bad = FakeRun({'spec': spec, 'forced_find_schedule': {'start': 3}, 'matchup_every': 0})
    with pytest.raises(EncodingMismatch):
        D.selfplay_spec(bad, 0)
    # the hook's specs are the engine's
    for it in range(3):
        E.mtg_spec_check(D.selfplay_spec(R, it))


def test_where_it_is_dropped_and_where_refused():
    spec = FF.with_forced_find(A.SPEC_META3, 0.5)
    assert 'forced_find' not in json.loads(evaldecks.eval_spec(spec))
    from mtgz.az import replays, sideboard_census, keep_h2h, yardsticks
    assert 'forced_find' not in json.loads(replays.spec_with_log(spec))
    assert 'forced_find' not in json.loads(sideboard_census.census_spec({'spec': spec}))
    # the yardsticks' best of one: a run's spec is played without it; an explicit bo1_spec with it is refused
    assert 'forced_find' not in json.loads(yardsticks.bo1_spec({'spec': spec}, {}))
    with pytest.raises(ValueError, match='forced_find'):
        yardsticks.bo1_spec({'spec': spec}, {'bo1_spec': spec})
    assert 'forced_find' not in json.loads(yardsticks._cell_spec(DD, DELVER, 0, {'cells_max_turns': 60, 'cells_format': 'bo1'}))
    with pytest.raises(ValueError, match='forced_find'):
        keep_h2h.half_specs(FF.with_forced_find(D.mirror_bo3_spec(), 1.0))
    import combo_stats as C
    with pytest.raises(ValueError, match='forced_find'):
        FF.refuse(FF.with_forced_find(C.deck_spec(DD, DELVER), 1.0), 'combo_stats play')


def test_the_engine_refuses_where_it_cannot_apply():
    dd_delver = C_deck_spec(DD, DELVER)
    E.mtg_spec_check(FF.with_forced_find(dd_delver, 0.0))
    for spec, why in ((FF.with_forced_find(C_deck_spec(DELVER, DELVER), 1.0), 'no deck'),
                      (FF.with_forced_find(D.pairing_spec(D.IZZET_DELVER, D.SNEAK_SHOW), 1.0), 'no deck'),
                      (FF.with_forced_find(dd_delver, 1.0, spell='Brainstorm'), 'pile'),
                      (json.dumps({'forced_find': {'spell': 'Doomsday', 'card': "Thassa's Oracle", 'p': 1}}), 'constructed')):
        with pytest.raises(Exception, match=why):
            E.mtg_spec_check(spec)


def C_deck_spec(a, b):
    import combo_stats as C
    return C.deck_spec(a, b)


def test_the_presets():
    w, s = A.PRESETS['legacy-meta3-ddwheel'], A.PRESETS['legacy-meta3-ddwheel-smoke']
    base, smoke = A.PRESETS['legacy-meta3'], A.PRESETS['legacy-meta3-smoke']
    for p, b in ((w, base), (s, smoke)):
        sp = json.loads(p['spec'])
        assert sp['forced_find'] == {'spell': 'Doomsday', 'card': "Thassa's Oracle", 'p': 1.0, 'unless_in_hand': True}
        assert {k: v for k, v in sp.items() if k != 'forced_find'} == json.loads(A.SPEC_META3)
        assert p['eval_spec'] == A.SPEC_META3 and 'forced_find' not in p['eval_spec']
        assert {k: v for k, v in p.items() if k not in ('spec', 'forced_find_schedule')} == {k: v for k, v in b.items() if k != 'spec'}
        FF.check_schedule(p['forced_find_schedule'])
        E.mtg_spec_check(p['spec'])
    assert w['forced_find_schedule'] == {'start': 1.0, 'end': 0.0, 'from': 0, 'iters': 100}
    assert 'forced_find_schedule' in A.PACKAGE.settings and A.PACKAGE.settings['forced_find_schedule'] == {}
    assert 'weights' not in json.loads(w['spec'])['metagame']        # uniform: the launch decides


def random_row(spec, driver_seed, gid, rng, max_moves=4000):
    g = E.Game('mtg', spec, 2, E.game_seed_of(driver_seed, gid, False))
    moves = []
    while g.to_move is not None and len(moves) < max_moves:
        legal = list(g.legal())
        a = rng.choice(legal)
        moves.append([g.to_move, a, legal])
        g.step(a)
    over = g.to_move is None
    return {'gid': gid, 'over': over, 'aborted': False, 'result': [float(x) for x in g.result()] if over else [0.5, 0.5],
            'decisions': len(moves), 'moves': moves}


def test_combo_stats_reads_the_oracle_at_each_resolution():
    import combo_stats as C
    spec = FF.with_forced_find(C.deck_spec(DD, DELVER), 1.0)
    rng = random.Random(1)
    seen = {}
    m = {'label': 't', 'batch': 0, 'driver_seed': 5, 'decks': [DD, DELVER], 'sims': [1, 1], 'spec': spec,
         'forced_find': json.loads(spec)['forced_find']}
    rows = []
    for gid in range(400):
        s = C.scan_row(E, m, random_row(spec, 5, gid, rng))
        for x in s['doomsday']:
            assert x['ff'] in C.FF_CLASSES and x['ff_drawn'] is True            # p 1: every resolution drawn
            assert (x['ff'] in ('voluntary', 'forced')) == x['oracle_in_pile']
            seen[x['ff']] = seen.get(x['ff'], 0) + 1
        rows.append(dict(s, wall_s_per_game=1.0, cpu_s_per_game=1.0, batch_failures=0))
        if seen.get('forced', 0) >= 2:
            break
    assert seen.get('forced', 0) >= 2, seen
    assert 'missed' not in seen                                                   # a drawn resolution never misses
    rep = C.summarize(rows)
    o = rep['doomsday']['p0']['oracle']
    assert o['forced'] == seen['forced'] and sum(o.values()) == rep['doomsday']['p0']['resolved']
    t = C.table(rep)
    assert 'SELF-PLAY with the forced find' in t and 'in its own piles' in t
    # without the forced find: the state's classes, no draw
    spec0 = C.deck_spec(DD, DELVER)
    m0 = dict(m, spec=spec0, forced_find=None, driver_seed=7)
    rng = random.Random(2)
    n = 0
    for gid in range(300):
        s = C.scan_row(E, m0, random_row(spec0, 7, gid, rng))
        for x in s['doomsday']:
            assert x['ff'] != 'forced' and x['ff_drawn'] is None
            assert (x['ff'] == 'voluntary') == x['oracle_in_pile']
            n += 1
        if n >= 3:
            break
    assert n >= 1


def test_a_drawn_resolutions_decisions_are_recorded_without_their_policy():
    """docs/ENGINE.md 180.12: self-play records (a recording seat, uniform priors and values, traced as the farm's worker
    traces them) of the Doomsday mirror at p 1 - every decision of a drawn resolution is a value-only row (no options,
    no policy target), every other decision of the seat to move has its policy; the trainer's path (`replay_rows`)
    accepts the records and rebuilds exactly those rows without options. The classes are read from the rebuilt game's
    play-by-play, not from the hook. A recording driver with a uniform evaluator in this process: no network."""
    import combo_stats as C
    r = C.rows('uniform', [DD, DD], 2, 20, 1.0, threads=2, log=lambda *a: None)
    t = r['rows']
    assert r['ok'], r
    assert t['drawn']['policy'] == 0 and t['drawn']['value_only'] > 0, t
    assert t['undrawn']['value_only'] == 0 and t['other']['value_only'] == 0 and t['other']['policy'] > 0, t
    assert r['replayed']['without_options'] == t['drawn']['value_only'], r
