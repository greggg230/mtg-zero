"""Thompson-sampled sideboard plans (docs/ENGINE.md 274, run setting `matchup_explore`): the matchup job's member plans
(each bootstrap member's plain argmax climb, beside the guarded table, which they leave exactly as it was), the dealt
opponent's belief unchanged (worlds around the table's OWN choice, the engine's base), the tables placed in the spec and
parsed by the engine, the engine's per-seat draw as the game log records it (against an independent reimplementation
of the draw), the hook's job command, the stew's settings, and the sideboarding health's member and played plans."""
import collections
import json
import os
import sys

import numpy as np
import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'tests'))
import mtgz.az as A  # noqa: E402
from mtgz.az import decklists  # noqa: E402
from mtgz.az import games as G  # noqa: E402
from mtgz.az import matchup as MU  # noqa: E402
from mtgz.stew import sbhealth as SB  # noqa: E402
from mtgz.stew.state import jload, jsave  # noqa: E402

from test_dealt_around import CC, CD, MC, MD, _logged, _record, _table  # noqa: E402

M64 = (1 << 64) - 1


# ------------------------------------------------------------------ the engine's draw, reimplemented
def _split64(x):
    z = (x + 0x9E3779B97F4A7C15) & M64
    z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & M64
    z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & M64
    return z ^ (z >> 31)


class _Xoshiro:
    def __init__(self, seed):
        x, s = seed, []
        for _ in range(4):
            x = (x + 0x9E3779B97F4A7C15) & M64
            s.append(_split64(x))
        self.s = s if any(s) else [1, 0, 0, 0]

    def next(self):
        s = self.s
        rot = lambda v, k: ((v << k) | (v >> (64 - k))) & M64  # noqa: E731
        r = (rot((s[0] + s[3]) & M64, 23) + s[0]) & M64
        t = (s[1] << 17) & M64
        s[2] ^= s[0]
        s[3] ^= s[1]
        s[1] ^= s[2]
        s[0] ^= s[3]
        s[2] ^= t
        s[3] = rot(s[3], 45)
        return r


def draw(seed, seat, share, n):
    """docs/ENGINE.md 274's draw: a stream of the match's seed per seat; a uniform (53 bits) below the share, then a member
    uniform by rejection; None: the table's own."""
    r = _Xoshiro(_split64(seed ^ 0x74035A3F1E000274 ^ _split64(seat + 1)))
    if (r.next() >> 11) / float(1 << 53) >= share:
        return None
    lim = (M64 // n) * n
    while True:
        x = r.next()
        if x < lim:
            return x % n


def _members(E, k=4, seed=0):
    r = np.random.default_rng(seed)
    return [{'w': r.normal(size=E).tolist(), 'c': r.normal(size=4).tolist(), 'U': [], 'M': [], 'dim': 0}
            for _ in range(k)]


# ------------------------------------------------------------------ the matchup job
def test_member_plans_are_each_members_argmax_and_leave_the_table_as_it_was():
    names = [f'c{i}' for i in range(len(CC))]
    mem = _members(len(CC), k=5, seed=11)
    plain, pd = MU.choice_table(mem, names, CC, MC, (1, 2), 'v1', worlds=300, seed=5, log=lambda s: None)
    assert 'explore' not in plain
    t, d = MU.choice_table(mem, names, CC, MC, (1, 2), 'v1', worlds=300, seed=5, log=lambda s: None, explore=0.5)
    assert t['choices'] == plain['choices'] and d == pd, 'the guarded table is unchanged'
    e = t['explore']
    assert e['share'] == 0.5 and len(e['members']) == 5
    for k, ch in enumerate(e['members']):
        assert [tuple(c[:4]) for c in ch] == MU.contexts()
        # dim 0: a member's objective increases with its own w . dA, so its plain climb ends at its exact strength
        # optimum (a separable objective: a one-swap local optimum is global)
        want = MU.strength_optimum([mem[k]], CC, MC)
        assert all(c[4] == want for c in ch), (k, ch[0][4], want)
    assert len({tuple(ch[0][4]) for ch in e['members']}) > 1, 'the members disagree'
    # (the plain argmax climb, z 0, by hand on member 0 of one context)
    rng = np.random.default_rng(5)
    S = sum(CC) - sum(MC)
    B = MU.sample_prior(CC, MC, 0, S, 300, rng) - np.asarray(MC, dtype=np.float64)
    F = MU.Objective(mem, B, 2, 0, 0, 0)
    assert MU.confident_climb(F.member(0), CC, MC, 0.0)[0] == e['members'][0][0][4]


def test_pair_member_plans_per_deck():
    P = MU.Pairing([{'name': 'C', 'list': [[f'c{i}', k] for i, k in enumerate(CC)], 'main': MC, 'index': 0},
                    {'name': 'D', 'list': [[f'd{i}', k] for i, k in enumerate(CD)], 'main': MD, 'index': 1}], False)
    r = np.random.default_rng(6)
    mem = [{'wC': r.normal(size=len(CC)).tolist(), 'wD': r.normal(size=len(CD)).tolist(),
            'c': [0.1, 0.2, 0, 0, 0, 0, 0], 'UC': [], 'UD': [], 'W': [], 'dim': 0} for _ in range(4)]
    plain, _ = MU.pair_choice_tables(mem, P, (1, 2), 'v2', worlds=300, seed=1, log=lambda s: None)
    tabs, _ = MU.pair_choice_tables(mem, P, (1, 2), 'v2', worlds=300, seed=1, log=lambda s: None, explore=1.0)
    for t, p, key, copies, main in zip(tabs, plain, ('wC', 'wD'), (CC, CD), (MC, MD)):
        assert t['choices'] == p['choices']
        assert t['explore']['share'] == 1.0 and len(t['explore']['members']) == 4
        for k, ch in enumerate(t['explore']['members']):
            want = MU.strength_optimum([{'w': mem[k][key]}], copies, main)
            assert all(c[4] == want for c in ch)


def test_a_dealt_opponents_worlds_stay_around_the_tables_own_choice(monkeypatch):
    """The engine deals a dealt seat around its table's OWN choice, never its drawn plan (docs/ENGINE.md 274): with
    explore the job draws its opp_dealt 1 worlds around exactly the bases it drew without - the members' plans never."""
    names = [f'c{i}' for i in range(len(CC))]
    mem = _members(len(CC), k=4, seed=3)
    calls = _record(monkeypatch)
    plain, _ = MU.choice_table(mem, names, CC, MC, (1, 2), 'v1', worlds=300, seed=5, log=lambda s: None, around='table')
    n0 = list(calls)
    calls.clear()
    t, _ = MU.choice_table(mem, names, CC, MC, (1, 2), 'v1', worlds=300, seed=5, log=lambda s: None, around='table',
                           explore=0.5)
    assert calls == n0 and t['choices'] == plain['choices']
    by = {tuple(c[:4]): c[4] for c in t['choices']}
    assert {tuple(c[1]) for c in calls} == {tuple(by[MU.opponent_context(g, f, l)]) for (g, f, l, o) in MU.contexts()
                                            if o == 1}
    member_bases = {tuple(c[4]) for ch in t['explore']['members'] for c in ch}
    assert member_bases - {tuple(c[1]) for c in calls}, 'some member plan is no base (the test means something)'


def test_the_engine_parses_the_placed_tables_and_logs_each_seats_draw(tmp_path):
    probe = decklists.mirror_bo3_spec(max_turns=2, game_episodes=True, start_game=2,
                                      dealt_decks={'p': 0.3, 'swaps': [1, 4], 'per_seat': True})
    specs, _ = _logged(tmp_path / 'probe', probe, [0])
    lists, main = specs[0]['lists'][0], specs[0]['mains'][0]
    table = _table(lists, main, 7)
    table['explore'] = {'share': 0.5, 'members': [_table(lists, main, 8 + k)['choices'] for k in range(3)]}
    spec = MU.with_table(probe, table)
    assert json.loads(spec)['sideboard_table']['explore']['share'] == 0.5
    MU.check_table(probe, table)
    E = G.engine()
    for bad in ({'share': 0, 'members': table['explore']['members']}, {'share': 0.5, 'members': []},
                {'share': 0.5, 'members': [table['choices'][:23]]}, {'share': 0.5, 'members': table['explore']['members'],
                                                                     'x': 1}):
        with pytest.raises(Exception, match='explore'):
            E.mtg_spec_check(MU.with_table(probe, dict(table, explore=bad)))
    specs, games = _logged(tmp_path / 'run', spec, range(300))
    assert specs[0]['explore'] == [{'share': 0.5, 'members': 3}] * 2
    seen = collections.Counter()
    for x in games:
        for q in (0, 1):
            m = draw(x['seed'], q, 0.5, 3)
            assert x['table'][q] == ['around7', m], (x['seed'], q, x['table'])
            seen[m] += 1
    n = sum(seen.values())
    assert n > 300 and abs(seen[None] / n - 0.5) < 5 * (0.25 / n) ** 0.5 and all(seen[k] for k in range(3)), seen
    # without explore: no table field, no explore key - the lines are what they were
    specs0, games0 = _logged(tmp_path / 'plain', MU.with_table(probe, _table(lists, main, 7)), range(20))
    assert 'explore' not in specs0[0] and all('table' not in x for x in games0)


# ------------------------------------------------------------------ the settings, the hook, the stew
def test_the_setting_is_off_by_default_and_reaches_the_job(tmp_path, monkeypatch):
    assert MU.SETTINGS['matchup_explore'] == 0.0
    for name in ('delver-mirror-bo3-varied', 'legacy-pair-bo3-varied', 'legacy-pair-bo3-varied-smoke'):
        assert not A.PRESETS[name].get('matchup_explore'), name
    cmds = []

    class P:
        pid = 1

        def poll(self):
            return 0

    def popen(cmd, **kw):
        cmds.append(cmd)
        return P()
    monkeypatch.setattr(MU.subprocess, 'Popen', popen)

    class R:
        run = str(tmp_path / 'run')
        logs = []

        def log(self, m):
            self.logs.append(m)
    for ex, want in ((0.0, None), (0.25, '0.25')):
        r = R()
        r.run = str(tmp_path / f'run{ex}')
        os.makedirs(MU.mdir(r.run))
        jsave(os.path.join(MU.mdir(r.run), 'start.json'), {'first_iter': 1})
        r.cfg = {'matchup_every': 1, 'matchup_window': 4, 'matchup_min_window': 2, 'matchup_val_iters': 1,
                 'matchup_explore': ex}
        MU.launch_if_due(r, 3)
        cmd = cmds[-1]
        assert (cmd[cmd.index('--explore') + 1] if '--explore' in cmd else None) == want
    r = R()
    r.run = str(tmp_path / 'runbad')
    os.makedirs(MU.mdir(r.run))
    jsave(os.path.join(MU.mdir(r.run), 'start.json'), {'first_iter': 1})
    r.cfg = {'matchup_every': 1, 'matchup_window': 4, 'matchup_min_window': 2, 'matchup_val_iters': 1,
             'matchup_explore': 1.5}
    with pytest.raises(ValueError, match='matchup_explore'):
        MU.launch_if_due(r, 3)


def test_the_stew_sets_it_through_recipe_settings(tmp_path):
    from mtgz.stew import config as C
    from mtgz.stew.orchestrator import Stew
    from test_stew import DD, DEL, FakeOps, make_cfg, make_source
    src = make_source(tmp_path)
    cfgp = make_cfg(tmp_path, src, settings={'matchup_every': 16, 'matchup_explore': 0.5},
                    pair_settings={f'{DD}|{DEL}': {'matchup_explore': 1.0}})
    s = Stew.init(os.path.join(str(tmp_path), 'stew'), cfgp, ops=FakeOps(tmp_path), log=lambda m: None)
    cfg = jload(os.path.join(s.st.abs(s.st.net(C.spec_name(DEL, DD))['run']), 'config.json'))
    assert cfg['matchup_explore'] == 1.0
    cfg2 = jload(os.path.join(s.st.abs(s.st.net(C.spec_name(DEL, DEL))['run']), 'config.json'))
    assert cfg2['matchup_explore'] == 0.5


# ------------------------------------------------------------------ sideboarding health
def test_sb_health_reports_member_plans_and_played_plans(tmp_path):
    from test_stew_sbhealth import DEL, deck75, table_of
    names, counts, main = deck75(DEL)
    t = table_of(DEL, 'v0016')
    i_p, i_f, i_h, i_t = (names.index(x) for x in ('Pyroblast', 'Force of Will', 'Hydroblast', 'Wasteland'))
    plan = list(main)
    plan[i_f] -= 1
    plan[i_p] += 1
    plan2 = list(plan)
    plan2[i_t] -= 1
    plan2[i_h] += 1
    t['explore'] = {'share': 0.5, 'members': [[[g, f, l, o, plan] for g, f, l, o in MU.contexts()],
                                              [[g, f, l, o, plan] for g, f, l, o in MU.contexts()],
                                              [[g, f, l, o, plan2] for g, f, l, o in MU.contexts()],
                                              [[g, f, l, o, list(main)] for g, f, l, o in MU.contexts()]]}
    run = str(tmp_path / 'spec-mirror')
    jsave(os.path.join(run, 'matchup', 'v0016.json'), {'format': MU.FORMAT, 'version': 'v0016', 'iter': 16,
                                                       'list': [[n, k] for n, k in zip(names, counts)], 'main': main,
                                                       'table': t, 'explore': 0.5})
    d = SB.table_plan(run, DEL)['decks'][DEL]
    assert d['state'] == 'all main'
    th = d['thompson']
    assert th['share'] == 0.5 and th['members'] == 4 and th['boarding'] == 3
    assert th['top'][0] == {'changes': {'Force of Will': -1, 'Pyroblast': 1}, 'share': 0.5}
    # played plans from the game log: two table seats on member 0's plan, one on the table's own (the main)
    os.makedirs(os.path.join(run, 'games'))
    lists = [[n, k] for n, k in zip(names, counts)]
    sp = {'kind': 'bo3-spec', 'v': 1, 'spec': 'S', 'lists': [lists, lists], 'mains': [main, main],
          'dealt_decks': {'p': 0.85, 'swaps': [0, 15], 'per_seat': True}, 'table': 'v0016',
          'explore': [{'share': 0.5, 'members': 4}] * 2}
    lines = [sp]
    for k, (decks, tab, mode) in enumerate([([plan, plan], [['v0016', 0], ['v0016', 1]], ['table', 'table']),
                                            ([main, plan2], [['v0016', None], ['v0016', 2]], ['table', 'dealt'])]):
        lines.append({'v': 1, 'kind': 'bo3', 'spec': 'S', 'seed': k, 'game': 2, 'starter': 0, 'chooser': 0,
                      'wins': [1, 0], 'dealt': [False, mode[1] == 'dealt'], 'mode': mode, 'decks': decks,
                      'result': [1.0, 0.0], 'aborted': False, 'turns': 9, 'post_board': True, 'time': 10.0 + k,
                      'table': tab})
    with open(os.path.join(run, 'games', 'games-1.jsonl'), 'w', encoding='utf-8') as f:
        for x in lines:
            f.write(json.dumps(x) + '\n')
    with open(os.path.join(run, 'metrics.jsonl'), 'w', encoding='utf-8') as f:
        f.write(json.dumps({'iter': 16, 'time': 100.0}) + '\n')
    pp = SB.plans_from_log(run, 16, 16)
    assert pp == {'p0': {'seats': 2, 'boarding': 1, 'members': {'m0': 1, 'own': 1},
                         'top': [{'changes': {'Force of Will': -1, 'Pyroblast': 1}, 'share': 0.5},
                                 {'changes': {}, 'share': 0.5}]},
                  'p1': {'seats': 1, 'boarding': 1, 'members': {'m1': 1},
                         'top': [{'changes': {'Force of Will': -1, 'Pyroblast': 1}, 'share': 1.0}]}}
    # the database's rows (no member index there)
    db = SB.played_plans([{'deck': 'D', 'changes': SB._db_changes(['1 Pyroblast'], ['1 Force of Will'])},
                          {'deck': 'D', 'changes': ()}])
    assert db == {'D': {'seats': 2, 'boarding': 1, 'top': [{'changes': {'Force of Will': -1, 'Pyroblast': 1}, 'share': 0.5},
                                                           {'changes': {}, 'share': 0.5}]}}
    h = {'alerts': [], 'specialists': {'spec:a|a': {'table': {'version': 'v0016', 'versions': 1, 'decks': {DEL: d}},
                                                    'played': {'source': 'game log (local self-play only)', 'decks': pp},
                                                    'play': {'note': 'x'}, 'card_use': {'note': 'y'}}}}
    md = '\n'.join(SB.md_lines(h))
    assert '### Thompson-sampled plans (docs/ENGINE.md 274)' in md
    assert '| 0.5 | 3 of 4 | +1 Pyroblast -1 Force of Will 50%' in md
    assert '| p0 | 2 | 1 | m0 1, own 1 | +1 Pyroblast -1 Force of Will 50%; main 50% |' in md
