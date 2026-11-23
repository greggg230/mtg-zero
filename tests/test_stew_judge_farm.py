"""The stew's judge on the farm (mtgz.stew.judge_farm, docs/ENGINE.md 249): the campaigns a phase's judge jobs map onto
(a paired job, the specialist against itself, fixed seats as the halves of a paired campaign - one campaign for both
orientations of arm M, a discarded half for arm A and the mirrors), the proof that a campaign's games are the local
job's (the engine's game seeds, the worker's seat order), the farm job's seats against the local judge's dict for
dict with its refusals by name, the runner (every check before any put, a window of campaigns, the result files the
local process writes, resume by result file and by campaign name, a campaign of other inputs refused), one backend per
generation, and the Ops call (the runner's refusal and failure read from its log). No farm, worker, network or GPU:
fake checkpoints (the decisions read run configs and file hashes) and fake campaign steps, as tests/test_h2h_farm.py."""
import json
import os
import sys

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
import mtgz.az  # noqa: E402,F401
from mtgz.az import farm_campaign as FC, h2h_farm as HF  # noqa: E402
from mtgz.stew import config as C  # noqa: E402
from mtgz.stew import judge as J  # noqa: E402
from mtgz.stew import judge_farm as JF  # noqa: E402
from mtgz.stew.state import jload, jsave  # noqa: E402
from zero import farm_eval as FE, h2h  # noqa: E402

PRESET = 'delver-mirror-bo3-dealt'
DEL, DD, SNS = 'izzet_delver_8016541', 'doomsday_8027957', 'sneak_show_8014220'
AL = {DEL: 'izzet', DD: 'doomsday', SNS: 'sneak'}
JC = {'matches': 8, 'gen_matches': 4, 'seed': 0, 'gen_over_gen': True, 'main_arms': True}


def spec_fn(p, q, c):
    return json.dumps({'decks': [p, q], 'chooser': c})


class FakeState:
    """The few State methods plan_jobs and summarize read; every net a file of a run with a config."""

    def __init__(self, root, decks=(DEL, DD, SNS), cands=(DEL,)):
        self.root = root
        self._d = [{'name': d, 'share': 1, 'alias': AL[d]} for d in decks]
        self._c = list(cands)

    def decks(self):
        return self._d

    def candidates(self):
        return self._c

    def ckpt(self, name, gen):
        return os.path.join(self.root, 'models', f"{name.replace(':', '_').replace('|', '_')}_{gen}.pt")

    def version(self, name, gen):
        return {'path': self.ckpt(name, gen)}

    def abs(self, p):
        return p


def make_run(tmp_path, name='spec', **over):
    """A run directory with a saved config (a run saves its resolved settings: the preset's values are in it, as
    zero.h2h.run_rule reads them) and the rule checkpoint."""
    run = tmp_path / name
    (run / 'models').mkdir(parents=True)
    cfg = dict(mtgz.az.PRESETS[PRESET], preset=PRESET, eval_every=16, **over)
    (run / 'config.json').write_text(json.dumps(cfg))
    (run / 'models' / 'iter_0000.pt').write_bytes(b'rule')
    return str(run)


def play_of(ck, sims=8, **over):
    """The play settings Stew.judge_play gives the jobs, of the specialists' checkpoint `ck`."""
    return dict({'seat': h2h.run_rule(ck), 'seat_keys': h2h.run_seat_keys(ck), 'sims': sims, 'device': 'cuda',
                 'threads': 16, 'groups': 1, 'chance_k': 2, 'precision': None, 'max_steps': 5000, 'sparse': False},
                **over)


def planned(tmp_path, phase='judge_deck_gap', **over):
    run = make_run(tmp_path)
    ck = os.path.join(run, 'models', 'iter_0000.pt')
    st = FakeState(run)
    jobs = J.plan_jobs(phase, st, 0, JC, spec_fn, play_of(ck, **over))
    for j in jobs:
        for p in (j['a'], j['b']):
            if not os.path.exists(p):
                with open(p, 'wb') as f:
                    f.write(os.path.basename(p).encode())
    return st, ck, jobs


# ------------------------------------------------------------------ the units
def test_judge_jobs_map_onto_campaigns(tmp_path):
    st, ck, jobs = planned(tmp_path)
    us = JF.units(jobs)
    by = {u['id']: u for u in us}
    kinds = {}
    for u in us:
        kinds[u['id'][0]] = kinds.get(u['id'][0], 0) + 1
    assert kinds == {'G': 3, 'B': 8, 'A': 20} and sum(len(u['jobs']) for u in us) == len(jobs)
    # gen over gen: the job's own paired match, its game ids
    g = by['G.izzet-doomsday']
    assert (g['paired'], g['games'], g['one'], g['jobs'], g['discard']) == (
        True, 4, False, [{'id': 'G.izzet-doomsday', 'take': 'all'}], 0)
    # arm B, the specialist against itself: one network on both seats, unpaired, its game ids
    b = by['B.izzet-doomsday.c1']
    assert (b['paired'], b['games'], b['one'], b['jobs'][0]['take'], b['discard']) == (False, 2, True, 'all', 0)
    # arm A: fixed seats of two networks = one half of a paired campaign of twice the matches, the other half discarded
    a0, a1 = by['A.izzet-doomsday.s0.c0'], by['A.izzet-doomsday.s1.c0']
    assert (a0['paired'], a0['games'], a0['matches'], a0['jobs'][0]['take'], a0['discard']) == (True, 4, 2, 'even', 2)
    assert (a1['jobs'][0]['take'], a1['discard']) == ('odd', 2) and a0['a'] != a1['a'], 'two deck stews: two campaigns'
    assert all(u['seed'] == j['seed'] and u['spec'] == j['spec']
               for u in us for j in jobs if j['id'] == u['jobs'][0]['id'])
    assert 'seat-swapped halves of 20 fixed-seat campaigns' in JF.plan_line(us)
    # arm M: both orientations of a pairing share net, opponent, spec and seed - one campaign, nothing discarded;
    # a mirror's orientations have their own seeds - a campaign each, half discarded
    st, ck, jobs = planned(tmp_path / 'm', phase='judge')
    us = JF.units(jobs)
    by = {u['id']: u for u in us}
    m = by['M.izzet-doomsday.s0.c0+M.izzet-doomsday.s1.c0']
    assert m['jobs'] == [{'id': 'M.izzet-doomsday.s0.c0', 'take': 'even'}, {'id': 'M.izzet-doomsday.s1.c0', 'take': 'odd'}]
    assert (m['paired'], m['games'], m['discard']) == (True, 4, 0)
    assert by['M.izzet-izzet.s0.c0']['discard'] == 2 and by['M.izzet-izzet.s1.c1']['jobs'][0]['take'] == 'odd'
    assert len(us) == 4 + 8 and sum(u['discard'] for u in us) == 8      # 4 mirror campaigns; 2 pairs x 2 x 2 choosers
    # a second job of one layout on the same nets, spec and seed is a planning error, not a silent overwrite
    with pytest.raises(ValueError, match='second job'):
        JF.units(jobs + [dict(jobs[0], id='M.dup')])


def test_a_campaigns_games_are_the_local_jobs(tmp_path):
    """The mapping's two facts, from the engine and the worker's seat rule: pair k of a paired campaign has the random
    streams of match k of an unpaired driver with the same seed, and its even game seats [A, B], its odd game [B, A]."""
    from mtgz import _engine as engine
    for seed in (0, 1, 3405691582, 2 ** 32 - 1):
        for k in range(50):
            local = engine.game_seed_of(seed, k, False)          # (the local job's match k: fixed seats, unpaired)
            assert engine.game_seed_of(seed, 2 * k, True) == local == engine.game_seed_of(seed, 2 * k + 1, True)
            assert engine.game_seed_of(seed, k, True) == engine.game_seed_of(seed, k ^ 1, True)   # (a paired job's own)
    a, b = {'net': 1}, {'net': 2}
    assert [FE.seats_of([a, b], g) for g in range(4)] == [[a, b], [b, a], [a, b], [b, a]]
    # the rows a job takes: 'even' = the a0 job's matches (A in seat 0), 'odd' = the a1 job's (A in seat 1)
    rows = [{'gid': g, 'result': [float(g), 0.0], 'over': True, 'seats': FE.seats_of([a, b], g),
             'rec': {'steps': [1, 2] + [{'next': 0}, 3] * (g % 3)}} for g in range(6)]
    assert [(r['gid'], r['result'][0], r['games']) for r in JF.job_rows('even', rows)] == [(0, 0.0, 1), (1, 2.0, 3), (2, 4.0, 2)]
    assert [(r['gid'], r['result'][0], r['games']) for r in JF.job_rows('odd', rows)] == [(0, 1.0, 2), (1, 3.0, 1), (2, 5.0, 3)]
    assert [r['gid'] for r in JF.job_rows('all', rows)] == list(range(6))
    assert all(r['decisions'] is None for r in JF.job_rows('all', rows)), 'a farm row carries no decision count'


# ------------------------------------------------------------------ the farm job's seats
def test_the_farm_jobs_seats_are_the_local_judges(tmp_path):
    st, ck, jobs = planned(tmp_path)
    us = {u['id']: u for u in JF.units(jobs)}
    u = us['A.izzet-doomsday.s0.c0']
    cfg, seats, b = JF.unit_job(u, ck, FC)
    assert seats == [{'net': 1}, {'net': 2}] and b == u['b']
    assert (cfg['eval_sims'], cfg['chance_k'], cfg['c_puct'], cfg['max_steps'], cfg['eval_trace']) == (8, 2, 1.0, 5000, 1)
    assert json.loads(cfg['eval_spec']) == json.loads(u['spec'])
    want = [dict(s) for s in J.job_seats(u['play']).values()]
    for s in want:
        s.pop('key')
    assert FC.farm_seats(cfg, seats) == want == JF.local_seats(u['play']), 'the seats the local process plays'
    # the specialist against itself: one network, no second checkpoint
    cfg, seats, b = JF.unit_job(us['B.izzet-doomsday.c0'], ck, FC)
    assert seats == [{'net': 1}, {'net': 1}] and b is None
    # refused by name: torch's precision; a seat the farm would play differently; a stream that hangs on the game id
    with pytest.raises(JF.FarmRefused, match='precision.*local-only'):
        JF.unit_job(dict(u, play=dict(u['play'], precision='bf16')), ck, FC)
    other = dict(u['play'], seat=dict(u['play']['seat'], root_min_visits=3))
    with pytest.raises(JF.FarmRefused, match=r"seat A: the farm would play .*root_min_visits.*local judge vs farm"):
        JF.unit_job(dict(u, play=other), ck, FC)
    with pytest.raises(JF.FarmRefused, match=r"would play \{'sims': \(8, 16\)\}"):
        JF.unit_job(u, ck, _Sims16(FC))
    gid = dict(u['play'], seat=dict(u['play']['seat'], view_samples=0.5))
    with pytest.raises(JF.FarmRefused, match='view_samples.*game id'):
        JF.unit_job(dict(u, play=gid), ck, FC)
    # a rule checkpoint without a run config
    with pytest.raises(JF.FarmRefused, match='no run config'):
        JF.unit_job(u, str(tmp_path / 'nowhere' / 'models' / 'iter_0000.pt'), FC)
    # a run whose config names only its preset: zero.h2h.run_rule (the local judge's seat) reads the saved file, the
    # farm job the resolved settings - the preset's root belief would be played on the farm only. Refused, by name.
    bare = tmp_path / 'bare'
    (bare / 'models').mkdir(parents=True)
    (bare / 'config.json').write_text(json.dumps({'preset': PRESET}))
    bck = str(bare / 'models' / 'iter_0000.pt')
    with pytest.raises(JF.FarmRefused, match='belief_tilts'):
        JF.unit_job(dict(u, play=play_of(bck)), bck, FC)


class _Sims16:
    """farm_campaign with seats of another simulation count (a farm job that would not be the local judge's)."""

    def __init__(self, fc):
        self.fc = fc
        self.CampaignError = fc.CampaignError

    def match_settings(self, *a, **kw):
        return self.fc.match_settings(*a, **kw)

    def farm_seats(self, cfg, seats):
        return self.fc.farm_seats(dict(cfg, eval_sims=16), seats)

    def ckpt_game(self, a, b=None):
        return self.fc.ckpt_game(a, b)


# ------------------------------------------------------------------ the runner
@pytest.fixture
def farm(monkeypatch, tmp_path):
    """Every machine condition met and the campaign steps faked: submit keeps a state, collect answers None `waits`
    times a campaign, games gives rows whose results name their game id."""
    w = tmp_path / 'worker.exe'
    w.write_bytes(b'w')
    entry = {'commit': 'c' * 40, 'build_id': '5e4023e25a380247f08fc3bb', 'runtime': 'd' * 24, 'worker': str(w),
             'cuda': {'build_id': '1' * 24}, 'exact': True}
    log = {'put': [], 'checked': [], 'collect': [], 'events': [], 'waits': 0, 'sleeps': 0}
    monkeypatch.setattr(FC, 'STATE_ROOT', str(tmp_path / 'campaigns'))
    monkeypatch.setattr(FC, 'EXPORT_CACHE', str(tmp_path / 'campaigns' / '_exports'))
    monkeypatch.setattr(HF, 'can_put', lambda: None)
    monkeypatch.setattr(HF, 'tracked_changes', lambda root=None: [])
    monkeypatch.setattr(HF, 'release_of', lambda root=None: entry)
    monkeypatch.setattr(HF, 'no_onnx', lambda: [])

    def job_settings(worker, build_id, cfg, d, lg, samples=False, seats=None):
        log['checked'].append(seats)
        log['events'].append('check')
        return dict(cfg)

    def submit(name, ck, games, settings, seats, seed, build_id, worker, runtime, b=None, paired=True, share=None,
               deadline_s=None, meta=None, client=None, log=None, **kw):
        run = FC.slug(name)
        d = FC.state_dir(run)
        resumed = os.path.exists(os.path.join(d, 'state.json'))
        mine = dict(name=name, run=run, a=ck, b=b, games=games, seats=seats, seed=seed, paired=paired, share=share,
                    deadline_s=deadline_s, meta=meta, build=build_id, resumed=resumed, kw=kw,
                    spec=settings['eval_spec'], sims=settings['eval_sims'])
        farm_log['put'].append(mine)
        farm_log['events'].append('put')
        if not resumed:
            FC.save_state(d, {'run': run, 'meta': meta, 'games': games, 'seats': seats, 'paired': paired,
                              'seed': seed, 'left': farm_log['waits']})
        return d

    def collect(d, client=None, threads=4, wrap=None, describe_games=True, wait=True, log=None, **kw):
        assert wait is False and not describe_games and wrap is HF.topup_wrap
        st = FC.load_state(d)
        farm_log['collect'].append(st['run'])
        if st['left'] > 0:
            st['left'] -= 1
            FC.save_state(d, st)
            return None
        return {'run': st['run'], 'coverage': {'games': st['games'], 'done': st['games'], 'farm': st['games'] - 1,
                                               'local': 1, 'failed': [], 'missing': []},
                'digest': 'd' * 16, 'spot': {'checked': 1, 'ok': 1, 'of': st['games']}, 'seconds': 120.0,
                'build_id': entry['build_id'], 'by_route': {'cpu': {'games': st['games'] - 1}, 'local': {'games': 1}}}

    def games(d):
        st = FC.load_state(d)
        sw = FE.check_seats(st['seats'])
        return [{'gid': g, 'pair': g // 2 if st['paired'] else None, 'seats': FE.seats_of(sw, g),
                 'result': [1.0, 0.0] if g % 3 else [0.0, 1.0], 'margin': 1.0, 'rounds': 5, 'over': g != 1,
                 'digest': 'x', 'rec': {'gid': g, 'steps': [0, {'next': 1}, 0] + [{'next': 0}, 0] * (g % 2)}}
                for g in range(st['games'])]
    farm_log = log
    monkeypatch.setattr(FC, 'job_settings', job_settings)
    monkeypatch.setattr(FC, 'submit', submit)
    monkeypatch.setattr(FC, 'collect', collect)
    monkeypatch.setattr(FC, 'games', games)
    return entry, log


class NoRun:
    """A coordinator that knows no run."""

    def get_or_none(self, path):
        return None


def plan_of(tmp_path, st, ck, jobs, **over):
    return dict({'v': JF.UNIT_V, 'stew': 'tst', 'gen': 0, 'phase': 'judge', 'dir': str(tmp_path / 'judge' / 'gen_000'),
                 'rule_ckpt': ck, 'format': 'bo3', 'share': None, 'deadline_h': None, 'at_once': 3, 'threads': 5,
                 'jobs': jobs}, **over)


def test_the_runner_plays_the_jobs_as_campaigns_and_writes_their_results(tmp_path, farm):
    entry, log = farm
    st, ck, jobs = planned(tmp_path, phase='judge')
    plan = plan_of(tmp_path, st, ck, jobs)
    log['waits'] = 1
    sleeps = []
    out = JF.run(plan, client=NoRun(), log=lambda m: None, sleep=sleeps.append)
    assert out == {'units': 12, 'played': 12, 'skipped': 0}
    # every unit was checked (the seats, the build's check-config) before the first campaign was put
    assert log['events'][:12] == ['check'] * 12 and log['events'][12] == 'put' and len(log['put']) == 12
    assert sleeps and all(s == 60.0 for s in sleeps), 'the runner waits between pulls while nothing came in'
    by = {p['meta']['unit']: p for p in log['put']}
    m = by['M.izzet-doomsday.s0.c0+M.izzet-doomsday.s1.c0']
    j0 = next(j for j in jobs if j['id'] == 'M.izzet-doomsday.s0.c0')
    assert (m['games'], m['paired'], m['seed'], m['seats']) == (4, True, j0['seed'], [{'net': 1}, {'net': 2}])
    assert (m['a'], m['b'], m['spec'], m['sims']) == (j0['a'], j0['b'], j0['spec'], 8)
    assert m['share'] == HF.AUTO_SHARE and m['build'] == entry['build_id'] and 'alternates' not in m['kw']
    assert m['deadline_s'] == 3600 + 0.25 * 4 * 3 * 8, 'a best-of-three match counted as its three games'
    assert m['run'].startswith('cmp-stew-tst-g000-m-') and len(m['run']) <= 63
    assert m['meta']['cmd'] == 'stew-judge' and len(m['meta']['stew_key']) == 64
    assert len({p['run'] for p in log['put']}) == 12
    # the result files: the local process's, valid for the same jobs; the campaign's rows by the job's take
    res = JF.results_in(plan['dir'])
    assert set(res) == {j['id'] for j in jobs}
    for j in jobs:
        r = res[j['id']]
        assert J.valid_result(j, r) and JF.valid_result(j, r) and r['v'] == J.JOB_V and r['backend'] == 'farm'
        assert set(r) >= {'v', 'id', 'hash', 'seconds', 'select', 'unfinished', 'rows'}
        assert set(r['rows'][0]) == {'gid', 'result', 'over', 'games', 'decisions'}
    r0, r1 = res['M.izzet-doomsday.s0.c0'], res['M.izzet-doomsday.s1.c0']
    assert [x['result'] for x in r0['rows']] == [[0.0, 1.0], [1.0, 0.0]]       # campaign games 0, 2
    assert [x['result'] for x in r1['rows']] == [[1.0, 0.0], [0.0, 1.0]]       # campaign games 1, 3
    assert [x['games'] for x in r0['rows']] == [2, 2] and [x['games'] for x in r1['rows']] == [3, 3]
    assert (r0['unfinished'], r1['unfinished'], r0['seconds']) == (0, 1, 60.0)
    assert r0['farm']['take'] == 'even' and r1['farm']['take'] == 'odd' and r0['farm']['campaign'] == m['run']
    assert r0['farm']['coverage'] == {'farm': 3, 'local': 1, 'failed': 0} and r0['farm']['discarded'] == 0
    assert res['M.izzet-izzet.s1.c0']['farm']['discarded'] == 2
    assert 'm_' not in json.dumps(res), 'no machine id in a result file'
    # the report's statistics read them as the local judge's
    s = J.summarize(st, 0, res)
    assert s['backend'] == ['farm'] and s['matches'] == sum(j['games'] for j in jobs)
    assert s['cells'][f'{DEL}|{DD}']['M']['matches'] == 8
    # again: every unit has its results - nothing is checked, put or collected
    n = (len(log['put']), len(log['collect']), len(log['checked']))
    assert JF.run(plan, client=NoRun(), log=lambda m: None, sleep=sleeps.append) == {'units': 12, 'played': 0, 'skipped': 12}
    assert (len(log['put']), len(log['collect']), len(log['checked'])) == n
    # a kill after the campaign was put, before its results were written: the same name resumes that campaign only
    os.remove(os.path.join(plan['dir'], 'M.izzet-doomsday.s1.c0.json'))
    out = JF.run(plan, client=NoRun(), log=lambda m: None, sleep=sleeps.append)
    assert out == {'units': 12, 'played': 1, 'skipped': 11}
    assert log['put'][-1]['run'] == m['run'] and log['put'][-1]['resumed'] and len(log['put']) == 13
    assert JF.valid_result(j0, jload(os.path.join(plan['dir'], 'M.izzet-doomsday.s0.c0.json')))


def test_campaigns_go_out_a_window_at_a_time(tmp_path, farm):
    entry, log = farm
    st, ck, jobs = planned(tmp_path, phase='judge')
    log['waits'] = 2
    out_now = []

    def sleep(s):
        done = {c for c in log['collect'] if log['collect'].count(c) > 2}
        out_now.append(len({p['run'] for p in log['put']} - done))
    JF.run(plan_of(tmp_path, st, ck, jobs, at_once=2, deadline_h=1.5, share=0.3), client=NoRun(), log=lambda m: None,
           sleep=sleep)
    assert out_now and max(out_now) <= 2 and len(log['put']) == 12
    assert {p['deadline_s'] for p in log['put']} == {5400.0} and {p['share'] for p in log['put']} == {0.3}


def test_a_refusal_puts_nothing(tmp_path, farm, monkeypatch):
    entry, log = farm
    st, ck, jobs = planned(tmp_path, phase='judge')
    # one unit the farm would not play as the local judge: the whole phase is refused before any put
    bad = [dict(j, play=dict(j['play'], precision='bf16')) if j['id'] == 'M.izzet-sneak.s1.c1' else j for j in jobs]
    with pytest.raises(JF.FarmRefused, match='M.izzet-sneak.*precision'):
        JF.run(plan_of(tmp_path, st, ck, bad), client=NoRun(), log=lambda m: None)
    assert log['put'] == []
    # the machine's conditions, with h2h_farm's reasons
    monkeypatch.setattr(HF, 'tracked_changes', lambda root=None: [' M mtgz/stew/judge.py'])
    with pytest.raises(JF.FarmRefused, match='uncommitted changes'):
        JF.run(plan_of(tmp_path, st, ck, jobs), client=NoRun(), log=lambda m: None)
    monkeypatch.setattr(HF, 'tracked_changes', lambda root=None: [])

    def no_release(root=None):
        raise HF.NotHere('no published farm build for abcdef012345 (python tools/farm_release.py release abcdef012345)')
    monkeypatch.setattr(HF, 'release_of', no_release)
    with pytest.raises(JF.FarmRefused, match='no published farm build'):
        JF.run(plan_of(tmp_path, st, ck, jobs), client=NoRun(), log=lambda m: None)
    monkeypatch.setattr(HF, 'release_of', lambda root=None: entry)
    monkeypatch.setattr(HF, 'can_put', lambda: 'no farm admin token on this machine')
    with pytest.raises(JF.FarmRefused, match='admin token'):
        JF.run(plan_of(tmp_path, st, ck, jobs), client=NoRun(), log=lambda m: None)
    monkeypatch.setattr(HF, 'can_put', lambda: None)

    def refuse(worker, build_id, cfg, d, lg, samples=False, seats=None):
        raise FC.CampaignError('check-config: unknown spec key "chooser"')
    monkeypatch.setattr(FC, 'job_settings', refuse)
    with pytest.raises(JF.FarmRefused, match='refuses the job.*chooser'):
        JF.run(plan_of(tmp_path, st, ck, jobs), client=NoRun(), log=lambda m: None)
    monkeypatch.setattr(HF, 'no_onnx', lambda: ['onnx'])
    with pytest.raises(JF.FarmRefused, match='cannot export'):
        JF.run(plan_of(tmp_path, st, ck, jobs), client=NoRun(), log=lambda m: None)
    assert log['put'] == []


def test_a_campaign_of_other_inputs_is_refused_not_resumed(tmp_path, farm):
    entry, log = farm
    st, ck, jobs = planned(tmp_path, phase='judge')
    plan = plan_of(tmp_path, st, ck, jobs)
    JF.run(plan, client=NoRun(), log=lambda m: None, sleep=lambda s: None)
    run = log['put'][0]['run']
    state = FC.load_state(FC.state_dir(run))
    FC.save_state(FC.state_dir(run), dict(state, meta=dict(state['meta'], stew_key='f' * 64)))
    for j in jobs:
        os.remove(os.path.join(plan['dir'], j['id'] + '.json'))
    n = len(log['put'])
    with pytest.raises(JF.FarmRefused, match='other inputs'):
        JF.run(plan, client=NoRun(), log=lambda m: None, sleep=lambda s: None)
    assert len(log['put']) == n
    # a run of that name on the coordinator without a state here is another machine's
    import shutil
    shutil.rmtree(FC.state_dir(run))

    class HasRun:
        def get_or_none(self, path):
            return {'state': 'active'} if path.endswith('/' + run) else None
    with pytest.raises(JF.FarmRefused, match='no state for it'):
        JF.run(plan, client=HasRun(), log=lambda m: None, sleep=lambda s: None)
    # other inputs give other names: another seed, generation, build or network
    u = JF.units(jobs)[0]
    sha = FC.file_sha256
    k = JF.unit_key(plan, u, [{'net': 1}, {'net': 2}], 'b1', sha)
    assert k == JF.unit_key(plan, u, [{'net': 1}, {'net': 2}], 'b1', sha)
    assert len({k, JF.unit_key(plan, dict(u, seed=u['seed'] + 1), [{'net': 1}, {'net': 2}], 'b1', sha),
                JF.unit_key(dict(plan, gen=1), u, [{'net': 1}, {'net': 2}], 'b1', sha),
                JF.unit_key(plan, u, [{'net': 1}, {'net': 2}], 'b2', sha),
                JF.unit_key(plan, dict(u, play=dict(u['play'], sims=16)), [{'net': 1}, {'net': 2}], 'b1', sha)}) == 5


def test_a_campaign_whose_rows_are_not_the_units_fails(tmp_path, farm, monkeypatch):
    entry, log = farm
    st, ck, jobs = planned(tmp_path, phase='judge')
    plan = plan_of(tmp_path, st, ck, jobs)
    good = FC.games

    def wrong_seats(d):
        rows = good(d)
        rows[1]['seats'] = rows[0]['seats']
        return rows
    monkeypatch.setattr(FC, 'games', wrong_seats)
    with pytest.raises(FC.CampaignError, match='game 1 was seated'):
        JF.run(plan, client=NoRun(), log=lambda m: None, sleep=lambda s: None)
    monkeypatch.setattr(FC, 'games', lambda d: good(d)[:-1])
    with pytest.raises(FC.CampaignError, match='games of'):
        JF.run(plan, client=NoRun(), log=lambda m: None, sleep=lambda s: None)
    assert JF.results_in(plan['dir']) == {}


# ------------------------------------------------------------------ one backend per generation
def test_one_backend_per_generation(tmp_path):
    jd = str(tmp_path / 'gen_000')
    assert JF.backend_of(jd) is None
    assert JF.claim_backend(jd, 'farm', 'judge_deck_gap@0') == 'farm'
    rec = jload(os.path.join(jd, 'backend.json'))
    assert rec['backend'] == 'farm' and rec['first'] == 'judge_deck_gap@0'
    assert JF.claim_backend(jd, 'farm', 'judge@0') == 'farm' and jload(os.path.join(jd, 'backend.json')) == rec
    with pytest.raises(JF.BackendMix, match="backend 'farm' and recipe.judge.backend is 'local'"):
        JF.claim_backend(jd, 'local')
    # a generation judged before the farm backend: results without the key were played here
    old = str(tmp_path / 'gen_001')
    jsave(os.path.join(old, 'G.x.json'), {'v': 1, 'id': 'G.x', 'hash': 'h', 'rows': []})
    jsave(os.path.join(old, 'G.x.job.json'), {'id': 'G.x'})
    assert JF.backend_of(old) == 'local'
    with pytest.raises(JF.BackendMix):
        JF.claim_backend(old, 'farm')
    assert not os.path.exists(os.path.join(old, 'backend.json'))
    assert JF.claim_backend(old, 'local') == 'local' and jload(os.path.join(old, 'backend.json'))['backend'] == 'local'
    # a result of the other backend in the directory is refused whatever the record says
    jsave(os.path.join(old, 'A.y.json'), {'v': 1, 'id': 'A.y', 'hash': 'h', 'rows': [], 'backend': 'farm'})
    with pytest.raises(JF.BackendMix, match='A.y'):
        JF.claim_backend(old, 'local')
    # ... and a local result is no farm result, though its hash is the job's
    job = {'id': 'G.x', 'games': 0, 'seed': 1}
    r = {'v': 1, 'id': 'G.x', 'hash': J.job_hash(job), 'rows': []}
    assert J.valid_result(job, r) and not JF.valid_result(job, r) and JF.valid_result(job, dict(r, backend='farm'))


# ------------------------------------------------------------------ the Ops call
def test_ops_runs_the_farm_runner_and_reads_its_refusal(tmp_path, monkeypatch):
    from mtgz.stew.control import FarmRefused, Interrupted, Ops, OpsError
    cfg = C.normalize({'name': 'x', 'decks': [DEL, DD], 'candidates': [DEL], 'source': {'run': 'r'}}, check_lists=False)
    ops = Ops(cfg, str(tmp_path), log=lambda m: None)
    job = {'id': 'G.a-b', 'games': 2, 'seed': 1}
    plan = {'v': 1, 'phase': 'judge', 'dir': str(tmp_path / 'judge' / 'gen_000'), 'jobs': [job]}
    seen = {}

    def run_long(cmd, log_path, stop=None, poll=2.0):
        seen['cmd'] = cmd
        with open(log_path, 'a', encoding='utf-8') as f:
            f.write(seen.get('says', '') + '\n')
        if seen.get('raise'):
            raise Interrupted('x')
        return seen['rc']
    monkeypatch.setattr(ops, 'run_long', run_long)
    seen.update(rc=2, says='REFUSED: G.a-b: seat A: the farm would play other seats')
    with pytest.raises(FarmRefused, match='^G.a-b: seat A'):
        ops.judge_farm(plan)
    assert seen['cmd'][1:4] == ['-m', 'mtgz.stew.judge_farm', 'run'] and seen['cmd'][4].endswith('judge.plan.json')
    assert jload(seen['cmd'][4]) == plan, 'the plan is the runner\'s input, in the generation\'s judge/farm directory'
    assert os.path.dirname(seen['cmd'][4]) == os.path.join(plan['dir'], 'farm')
    seen.update(rc=1, says='FAILED: campaign cmp-x: games failed')
    with pytest.raises(OpsError, match='exit 1.*games failed'):
        ops.judge_farm(plan)
    seen.update(rc=0, says='ok')
    with pytest.raises(OpsError, match='without a valid farm result'):
        ops.judge_farm(plan)
    rows = [{'gid': 0, 'result': [1.0, 0.0]}, {'gid': 1, 'result': [0.0, 1.0]}]
    jsave(os.path.join(plan['dir'], 'G.a-b.json'), {'v': 1, 'id': 'G.a-b', 'hash': J.job_hash(job), 'rows': rows})
    with pytest.raises(OpsError, match='without a valid farm result'):
        ops.judge_farm(plan)                       # (a local result: not the farm's)
    jsave(os.path.join(plan['dir'], 'G.a-b.json'), {'v': 1, 'id': 'G.a-b', 'hash': J.job_hash(job), 'rows': rows,
                                                    'backend': 'farm'})
    assert ops.judge_farm(plan)['G.a-b']['backend'] == 'farm'
    seen['raise'] = True
    with pytest.raises(Interrupted):
        ops.judge_farm(plan, stop=lambda: True)


def test_the_runners_command_line_exits_2_on_a_refusal(tmp_path, farm, capsys):
    entry, log = farm
    st, ck, jobs = planned(tmp_path, phase='judge')
    bad = [dict(j, play=dict(j['play'], precision='bf16')) for j in jobs]
    p = str(tmp_path / 'plan.json')
    jsave(p, plan_of(tmp_path, st, ck, bad))
    assert JF.main(['run', p]) == 2
    assert capsys.readouterr().out.strip().splitlines()[-1].startswith('REFUSED: ')
    assert log['put'] == []
