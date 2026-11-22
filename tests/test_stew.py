"""The stew's orchestrator (mtgz.stew, tools/stew.py; docs/ENGINE.md 222.2) on fakes: no process is started, no
network is loaded, no farm is called. FakeOps stands in for mtgz.stew.control.Ops: a training turn writes its
checkpoints, shards and metrics at once, distill writes its output and an `extra` with per-source rows, the judge
writes results with the jobs' hashes."""
import json
import os

import pytest

import mtgz.az  # noqa: F401
from mtgz.stew import config as C
from mtgz.stew import judge as J
from mtgz.stew import sources as S
from mtgz.stew.orchestrator import Refused, Stew
from mtgz.stew.state import jload, jsave

DEL, DD, SNS = 'izzet_delver_8016541', 'doomsday_8027957', 'sneak_show_8014220'
META3 = [DEL, DD, SNS]


# ------------------------------------------------------------------ fakes
class FakeOps:
    def __init__(self, tmp):
        self.wt = str(tmp)
        self.py = 'python'
        self.calls = []
        self.farm = {}
        self.live = []
        self.alive = {}
        self.status = {}
        self.distill_fail = 0
        self.judge_played = []
        self.train_noop = False
        self.on_supervisor = None
        self.sources_of = {}
        self.engine_specs = True

    def cmdline(self, cmd):
        return ' '.join(str(c) for c in cmd)

    def check_code(self, sha=None):
        return 'f' * 40

    def fork_new_run(self, seed, run, log):
        self.calls.append(('fork', seed, run))
        os.makedirs(os.path.join(run, 'models'), exist_ok=True)
        with open(seed, 'rb') as f, open(os.path.join(run, 'models', 'iter_0000.pt'), 'wb') as g:
            g.write(f.read())

    def resolve(self, run, saved):
        return dict(saved)

    def check_heads(self, ckpt, cfg):
        self.calls.append(('heads', ckpt))

    def check_spec(self, game, spec):
        self.calls.append(('spec', game))
        if self.engine_specs:                       # (the engine's own parse: a refusal raises ValueError)
            from mtgz.stew.control import Ops
            Ops.check_spec(self, game, spec)

    def launch_cmd(self, run, args):
        return ['python', 'tools/supervise.py', run] + list(args)

    def launch(self, run, args, log):
        target = int(args[args.index('--iters') + 1])
        self.calls.append(('launch', os.path.basename(run), target))
        if self.on_supervisor is not None:           # (the STOP test: alive until stopped)
            self.alive[run] = True
            return
        if self.train_noop:
            return
        train(run, self.newest(run) or 0, target)

    def supervisor(self, run):
        if self.on_supervisor is not None:
            self.on_supervisor(self, run)
        return {'state': 'done'}, self.alive.get(run, False)

    def newest(self, run):
        d = os.path.join(run, 'models')
        its = [int(f[5:9]) for f in os.listdir(d) if f.startswith('iter_') and f.endswith('.pt')] if os.path.isdir(d) else []
        return max(its) if its else None

    def run_status(self, run):
        return {'state': self.status.get(run, 'running')}

    def pause_boundary(self, run, log, timeout=3600):
        self.calls.append(('pause', run))
        self.status[run] = 'paused'
        return 'paused'

    def resume(self, run, log):
        self.calls.append(('resume', run))
        self.status[run] = 'running'
        return 'running'

    def clear_control(self, run):
        self.calls.append(('clear', os.path.basename(run)))

    def write_stop(self, run):
        self.calls.append(('stop', os.path.basename(run)))
        self.alive[run] = False

    def live_supervisors(self):
        return list(self.live)

    def farm_state(self, name):
        return self.farm.get(name)

    def farm_set(self, name, state):
        self.calls.append(('farm', name, state))
        old = self.farm.get(name)
        if old is not None:
            self.farm[name] = state
        return old

    def farm_gates(self, run, ckpt, out, log):
        self.calls.append(('gates', os.path.basename(run)))
        cfg = jload(os.path.join(run, 'config.json'))
        jsave(os.path.join(out, 'summary.json'), {'passed': True, 'worker': cfg.get('pool_worker')})
        return 0

    def distill_cmd(self, sources, out, cli):
        return ['python', 'tools/distill.py', '--sources', sources, out] + list(cli)

    def distill(self, sources, out, cli, log, stop=None):
        self.calls.append(('distill', os.path.basename(out), os.path.basename(sources)))
        if self.distill_fail:
            self.distill_fail -= 1
            raise RuntimeError('distill crashed (a test)')
        doc = jload(sources)
        assert doc['student']['init'] and os.path.exists(doc['student']['init'])
        for s in doc['sources']:
            assert os.path.exists(s['teacher']), s['teacher']
        os.makedirs(os.path.dirname(out), exist_ok=True)
        with open(out, 'wb') as f:
            f.write(json.dumps(doc, sort_keys=True).encode())
        self.sources_of[out] = doc
        return 0

    def read_extra(self, ckpt):
        doc = self.sources_of.get(ckpt) or json.loads(open(ckpt, 'rb').read())
        names = [s['name'] for s in doc['sources']]
        return {'distill': {'step': 50, 'sources': [{'name': n, 'rows': 200, 'selected': 100, 'used': 100} for n in names],
                            'heldout': [{'step': 50, 'policy_kl': 0.05, 'top1': 0.9,
                                         'sources': {n: {'policy_kl': 0.05, 'top1': 0.9} for n in names}}]}}

    def judge_cmd(self, job, out):
        return ['python', '-m', 'mtgz.stew.judge', 'play', job, out]

    def judge_jobs(self, jobs, jdir, procs, stop=None):
        out = {}
        for j in jobs:
            p = os.path.join(jdir, j['id'] + '.json')
            r = jload(p)
            if not J.valid_result(j, r):
                self.judge_played.append(j['id'])
                rows = [{'gid': g, 'result': [1.0, 0.0] if (j['seed'] + g + (j['chooser'] or 0)) % 3 else [0.0, 1.0],
                         'over': True, 'games': 2 + g % 2} for g in range(j['games'])]
                r = {'v': 1, 'id': j['id'], 'hash': J.job_hash(j), 'rows': rows, 'seconds': 1.0, 'unfinished': 0}
                jsave(p, r)
            out[j['id']] = r
        return out


def train(run, lo, hi):
    """A fake turn: checkpoints, complete data iterations and metrics rows lo + 1 .. hi."""
    for k in range(lo + 1, hi + 1):
        with open(os.path.join(run, 'models', f'iter_{k:04d}.pt'), 'wb') as f:
            f.write(f'{run} {k}'.encode())
        d = os.path.join(run, 'data', f'iter_{k:04d}')
        os.makedirs(d, exist_ok=True)
        open(os.path.join(d, 'r00_000.npz'), 'wb').write(b'x' * k)
        jsave(os.path.join(d, 'DONE.json'), {'iter': k})
        with open(os.path.join(run, 'metrics.jsonl'), 'a', encoding='utf-8') as f:
            f.write(json.dumps({'iter': k, 'selfplay': {'games': 16, 'samples': 2400}, 'seconds': 60}) + '\n')


def make_source(tmp, decks=META3, weights=None):
    """A source run: config.json (the meta3 best-of-one metagame with the forced find), checkpoints 0 and 7, data 5..7."""
    from mtgz.az import decklists as D
    from mtgz.az.forced_find import with_forced_find
    src = os.path.join(str(tmp), 'source')
    os.makedirs(os.path.join(src, 'models'))
    spec = with_forced_find(D.metagame_spec(decks, fmt='bo1', weights=weights), 1.0)
    cfg = {'preset': 'legacy-meta3-smoke', 'game': 'mtg', 'spec': spec, 'eval_spec': D.metagame_spec(decks),
           'games': 2444, 'lr': 1e-4, 'eval_every': 16, 'elo_anchors': 'a=x.pt', 'eval_remote': 'pool:src-eval',
           'yardsticks': {'cells_games': 200}, 'selfplay_remote': 'pool:src', 'pool_build': 'b' * 24,
           'pool_worker': 'w.exe', 'matchup_every': 4, 'forced_find_schedule': {'start': 1.0, 'end': 0.0},
           'publish': {}, 'max_steps': 60000, 'keep_data': 6}
    jsave(os.path.join(src, 'config.json'), cfg)
    for k in (0, 7):
        open(os.path.join(src, 'models', f'iter_{k:04d}.pt'), 'wb').write(f'source {k}'.encode())
    for k in (5, 6, 7):
        d = os.path.join(src, 'data', f'iter_{k:04d}')
        os.makedirs(d)
        open(os.path.join(d, 'r00_000.npz'), 'wb').write(b'y')
        jsave(os.path.join(d, 'DONE.json'), {})
    return src


def make_cfg(tmp, src, decks=META3, cands=(DEL,), **recipe):
    seed = os.path.join(str(tmp), 'pb262.pt')
    open(seed, 'wb').write(b'mirror seed')
    r = {'turn_iters': 2, 'distill_iters': 2, 'judge': {'matches': 8, 'gen_matches': 4, 'device': 'cpu'},
         'hold_after': ['judge_deck_gap@0'], 'distill_cli': ['--device', 'cpu']}
    r.update(recipe)
    cfg = {'name': 'tst', 'decks': [{'name': d, 'share': 1} for d in decks], 'candidates': list(cands),
           'source': {'run': src, 'iter': 'newest'}, 'seeds': {f'{DEL}|{DEL}': seed}, 'recipe': r,
           'yield_runs': [{'run': os.path.join(str(tmp), 'meta3'), 'farm_runs': ['src', 'src-eval']}]}
    os.makedirs(os.path.join(str(tmp), 'meta3'), exist_ok=True)
    jsave(os.path.join(str(tmp), 'meta3', 'config.json'), {})
    p = os.path.join(str(tmp), 'stew_cfg.json')
    jsave(p, cfg)
    return p


@pytest.fixture
def stew(tmp_path, monkeypatch):
    src = make_source(tmp_path)
    cfgp = make_cfg(tmp_path, src)
    ops = FakeOps(tmp_path)
    ops.farm.update({'src': 'active', 'src-eval': 'active'})
    monkeypatch.setattr(Stew, 'judge_play', lambda self: {'seat': {}, 'sims': 4})
    s = Stew.init(os.path.join(str(tmp_path), 'stew'), cfgp, ops=ops, log=lambda m: None)
    s.poll_s = 0.0
    return s


# ------------------------------------------------------------------ config
def test_pairings_count_and_order():
    decks = [{'name': d, 'share': 1, 'alias': C.default_alias(d)} for d in META3]
    for k in range(1, 4):
        cands = META3[:k]
        ps = C.pairings(decks, cands)
        assert len(ps) == k * 3 - k * (k - 1) // 2
        assert all(a in cands or b in cands for a, b in ps)
    assert C.pairings(decks, [DEL]) == [(DEL, DEL), (DEL, DD), (DEL, SNS)]
    assert C.canon(decks, SNS, DEL) == (DEL, SNS)
    assert C.farm_run('first', decks, DEL, DD) == 'first-izzet-doomsday'
    assert C.uncovered_cells(decks, C.pairings(decks, [DEL])) == [(DD, DD), (DD, SNS), (SNS, DD), (SNS, SNS)]


def test_config_refusals(tmp_path):
    good = {'name': 'x', 'decks': META3, 'candidates': [DEL], 'source': {'run': 'r'}}
    C.normalize(good)
    bad = [dict(good, foo=1), dict(good, name='X Y'), dict(good, candidates=['nope']), dict(good, candidates=[]),
           dict(good, decks=[{'name': DEL, 'share': 0}]), dict(good, recipe={'settings': {'spec': '{}'}}),
           dict(good, recipe={'judge': {'matches': 6}}), dict(good, recipe={'hold_after': ['judge@x']}),
           dict(good, recipe={'format': 'bo1', 'game_episodes': True}), dict(good, recipe={'start_game': 2}),
           dict(good, seeds={f'{DD}|{SNS}': 'x.pt'}), dict(good, decks=['no_such_deck']),
           dict(good, recipe={'judge': {'backend': 'farm'}}), dict(good, recipe={'distill': {'nope': 1}})]
    for b in bad:
        with pytest.raises(C.ConfigError):
            C.normalize(b)
    n = C.normalize(dict(good, seeds={f'{DEL}|{DEL}': 's.pt'}))
    assert n['recipe']['format'] == 'bo3' and n['recipe']['start_game'] == 1 and n['recipe']['game_episodes']
    assert n['recipe']['judge']['matches'] == 400 and n['recipe']['turn_iters'] == 32


# ------------------------------------------------------------------ init and the specialists' settings
def test_init_layout_and_specialist_settings(stew, tmp_path):
    st = stew.st
    assert [d['name'] for d in st.decks()] == META3
    assert jload(stew.p('stew.json'))['name'] == 'tst'
    for name in ['main'] + [C.deck_net(d) for d in META3]:
        assert os.path.exists(st.ckpt(name, 0))
    assert open(st.ckpt('main', 0), 'rb').read() == b'source 7'
    assert st.d['archive']['iters'] == [5, 6, 7] and os.path.exists(stew.p('archive', 'config.json'))
    sp = [k for k in st.d['nets'] if k.startswith('spec:')]
    assert sp == [C.spec_name(DEL, DEL), C.spec_name(DEL, DD), C.spec_name(DEL, SNS)]
    mirror = st.net(C.spec_name(DEL, DEL))
    assert st.version(C.spec_name(DEL, DEL), 0)['seed'].endswith('pb262.pt')
    assert open(st.ckpt(C.spec_name(DEL, DD), 0), 'rb').read() == b'source 7'
    cfg = jload(os.path.join(st.abs(mirror['run']), 'config.json'))
    spec = json.loads(cfg['spec'])
    assert spec['format'] == 'bo3' and spec['game_episodes'] is True and 'start_game' not in spec
    assert [d['name'] for d in spec['metagame']['decks']] == [DEL] and spec['metagame']['weights'] == [[1]]
    assert 'forced_find' not in spec and cfg['forced_find_schedule'] == {}
    assert cfg['eval_spec'] == cfg['spec']
    cfg2 = jload(os.path.join(st.abs(st.net(C.spec_name(DEL, DD))['run']), 'config.json'))
    m2 = json.loads(cfg2['spec'])['metagame']
    assert [d['name'] for d in m2['decks']] == [DEL, DD] and m2['weights'] == [[0, 1], [1, 0]]
    assert all('sideboard' in d for d in m2['decks'])
    for k, v in {'eval_every': 100000, 'elo_anchors': '', 'eval_remote': '', 'yardsticks': {}, 'publish': {},
                 'eval_games': 0, 'eval_random_games': 0, 'elo_games': 0,
                 'matchup_every': 4, 'games': 2444, 'lr': 1e-4,
                 'selfplay_remote': 'pool:tst-izzet-doomsday'}.items():
        assert cfg2[k] == v, k
    assert any('forced_find dropped' in n for n in mirror['notes'])
    assert ('heads', os.path.join(st.abs(mirror['run']), 'models', 'iter_0000.pt')) in stew.ops.calls
    # init again: nothing new
    n = len(stew.ops.calls)
    Stew.init(stew.dir, os.path.join(str(tmp_path), 'stew_cfg.json'), ops=stew.ops, log=lambda m: None)
    assert [c for c in stew.ops.calls[n:] if c[0] in ('fork',)] == []


def test_recipe_settings_override(tmp_path, monkeypatch):
    src = make_source(tmp_path)
    cfgp = make_cfg(tmp_path, src, settings={'games': 900, 'eval_belief_tilts': {'deck': 1.0}},
                    pair_settings={f'{DD}|{DEL}': {'lr': 5e-5}})
    s = Stew.init(os.path.join(str(tmp_path), 'stew'), cfgp, ops=FakeOps(tmp_path), log=lambda m: None)
    cfg = jload(os.path.join(s.st.abs(s.st.net(C.spec_name(DEL, DD))['run']), 'config.json'))
    assert cfg['games'] == 900 and cfg['eval_belief_tilts'] == {'deck': 1.0} and cfg['lr'] == 5e-5
    cfg3 = jload(os.path.join(s.st.abs(s.st.net(C.spec_name(DEL, SNS))['run']), 'config.json'))
    assert cfg3['lr'] == 1e-4


def test_spec_extras_and_matchup_settings_come_from_the_recipe(tmp_path):
    src = make_source(tmp_path)
    dealt = {'p': 0.85, 'swaps': [0, 15], 'per_seat': True}
    mm = {'matchup_every': 16, 'matchup_window': 32, 'matchup_min_window': 8, 'matchup_half_life': 16.0,
          'matchup_worlds': 8000, 'matchup_table_min_gain': 0.005, 'matchup_swap_z': 2.0, 'matchup_members': 20}
    cfgp = make_cfg(tmp_path, src, settings=mm, spec_extra={'dealt_decks': dealt},
                    deck_spec_extra={DD: {'table_key': 1}})
    ops = FakeOps(tmp_path)
    # the engine checks every specialist's spec at init, not at the run's start: dealt decks per seat in the
    # best-of-three metagame parse (pb3's sideboarding for two 75s), a per-deck key the engine does not know is refused
    with pytest.raises(ValueError, match='table_key'):
        Stew.init(os.path.join(str(tmp_path), 'stew'), cfgp, ops=ops, log=lambda m: None)
    ops.engine_specs = False
    s = Stew.init(os.path.join(str(tmp_path), 'stew'), cfgp, ops=ops, log=lambda m: None)
    cfg = jload(os.path.join(s.st.abs(s.st.net(C.spec_name(DEL, DD))['run']), 'config.json'))
    spec, ev = json.loads(cfg['spec']), json.loads(cfg['eval_spec'])
    assert spec['dealt_decks'] == dealt and 'dealt_decks' not in ev
    assert [d.get('table_key') for d in spec['metagame']['decks']] == [None, 1]
    assert all('table_key' not in d for d in ev['metagame']['decks'])
    assert ev['metagame'] == dict(spec['metagame'], decks=ev['metagame']['decks'])
    assert all(cfg[k] == v for k, v in mm.items())
    for bad in ({'spec_extra': {'metagame': {}}}, {'deck_spec_extra': {DD: {'main': []}}},
                {'deck_spec_extra': {'nope': {}}}):
        with pytest.raises(C.ConfigError):
            C.normalize({'name': 'x', 'decks': META3, 'candidates': [DEL], 'source': {'run': 'r'}, 'recipe': bad})
    # (make_cfg rewrites the one config file: last) without the unknown key, the engine's check at init passes
    ok = make_cfg(tmp_path, src, settings=mm, spec_extra={'dealt_decks': dealt})
    Stew.init(os.path.join(str(tmp_path), 'stew-ok'), ok, ops=FakeOps(tmp_path), log=lambda m: None)


def test_init_refuses_another_config(stew, tmp_path):
    p = os.path.join(str(tmp_path), 'other.json')
    c = jload(os.path.join(str(tmp_path), 'stew_cfg.json'))
    c['recipe']['turn_iters'] = 5
    jsave(p, c)
    with pytest.raises(Refused):
        Stew.init(stew.dir, p, ops=stew.ops)


# ------------------------------------------------------------------ the loop
def test_generation_zero_holds_after_the_deck_gap(stew):
    ops = stew.ops
    assert stew.run() == 'held'
    st = stew.st
    assert st.tag() == 'distil_main@0' and st.d['held']['tag'] == 'judge_deck_gap@0'
    assert jload(stew.p('PAUSE'))['tag'] == 'judge_deck_gap@0'
    # turns: round-robin in field order, B = 2 each, farm run active only during the turn
    assert [c for c in ops.calls if c[0] == 'launch'] == [
        ('launch', C.spec_dir(DEL, DEL), 2), ('launch', C.spec_dir(DEL, DD), 2), ('launch', C.spec_dir(DEL, SNS), 2)]
    fc = [c[1:] for c in ops.calls if c[0] == 'farm' and c[1].startswith('tst-')]
    assert fc[:2] == [('tst-izzet-izzet', 'active'), ('tst-izzet-izzet', 'paused')]
    assert st.d['farm_to_pause'] == []
    for sn in (C.spec_name(DEL, DEL), C.spec_name(DEL, DD), C.spec_name(DEL, SNS)):
        v = st.version(sn, 1)
        assert v['iter'] == 2 and v['train']['games'] == 32 and v['train']['rows_per_match'] == 150.0
    # the yield run paused at its boundary before the GPU, given back at the hold; its farm runs too
    assert ('pause', os.path.join(os.path.dirname(stew.dir), 'meta3')) in ops.calls
    assert ops.calls.index(('pause', os.path.join(os.path.dirname(stew.dir), 'meta3'))) < \
        ops.calls.index(('launch', C.spec_dir(DEL, DEL), 2))
    assert ops.calls[-1][0] in ('resume', 'farm') and ops.farm['src'] == 'active'
    assert st.d['yield'] == {'runs': {}, 'farm': {}}
    # the deck stews: one source per specialist with the deck, weight = the opponent's share
    dd = jload(stew.sources_path(C.deck_net(DD), 0))
    assert [s['name'] for s in dd['sources']] == [C.spec_name(DEL, DD)]
    assert dd['sources'][0]['select'] == {'actor': DD} and dd['student']['init'] == st.ckpt(C.deck_net(DD), 0)
    de = jload(stew.sources_path(C.deck_net(DEL), 0))
    assert [s['select'] for s in de['sources']] == [{'actor': DEL}] * 3
    assert [s['teacher'] for s in de['sources']] == [st.ckpt(C.spec_name(DEL, y), 1) for y in META3]
    assert de['sources'][0]['data'][0].endswith('rows/gen_001/' + C.spec_dir(DEL, DEL) + '/data/iter_0001')
    assert os.path.exists(stew.p('rows', 'gen_001', C.spec_dir(DEL, DD), 'config.json'))
    ps = st.version(C.deck_net(DD), 1)['distill']['per_source'][0]
    assert ps['selected'] == 100 and ps['policy_kl'] == 0.05 and ps['weight'] == round(1 / 3, 6)
    # the report so far: gen over gen, the matchup table, the gaps
    rep = jload(stew.p('reports', 'gen_000.json'))
    assert set(rep['judge']['gen_over_gen']) == {C.spec_name(DEL, DEL), C.spec_name(DEL, DD), C.spec_name(DEL, SNS)}
    cells = rep['judge']['cells']
    assert set(cells) == {f'{DEL}|{DEL}', f'{DEL}|{DD}', f'{DD}|{DEL}', f'{DEL}|{SNS}', f'{SNS}|{DEL}'}
    assert cells[f'{DEL}|{DEL}']['B'] == {'p': 0.5, 'se': 0.0, 'mirror': True}
    assert 'gap_A_B' in cells[f'{DEL}|{DD}'] and 'gap_M_A' not in cells[f'{DEL}|{DD}']
    assert 'specialists' in rep['judge']['field'][DEL]
    assert os.path.exists(stew.p('reports', 'gen_000.md'))
    # paused: run refuses until PAUSE goes
    with pytest.raises(Refused):
        stew.run()


def test_the_loop_resumes_after_the_hold_into_generation_one(stew):
    assert stew.run() == 'held'
    os.remove(stew.p('PAUSE'))
    assert stew.run(max_phases=3) == 'done'
    st = stew.st
    assert st.tag() == 'distil_decks@1'
    main = jload(stew.sources_path('main', 0))
    names = [s['name'] for s in main['sources']]
    assert names == [f'main:{DEL}|{DEL}@{DEL}', f'main:{DEL}|{DD}@{DEL}', f'main:{DEL}|{DD}@{DD}',
                     f'main:{DEL}|{SNS}@{DEL}', f'main:{DEL}|{SNS}@{SNS}', 'retention']
    by = {s['name']: s for s in main['sources']}
    assert by[f'main:{DEL}|{DD}@{DD}']['teacher'] == st.ckpt(C.deck_net(DD), 1)
    assert by[f'main:{DEL}|{DD}@{DD}']['select'] == {'actor': DD}
    ret = by['retention']
    assert ret['teacher'] == st.ckpt('main', 0) and ret['weight'] == pytest.approx(4 / 9, abs=1e-5)
    assert sorted(map(tuple, ret['select']['not_pairing'])) == sorted([(DEL, DEL), (DEL, DD), (DEL, SNS)])
    assert sum(s['weight'] for s in main['sources']) == pytest.approx(1.0, abs=1e-4)
    assert by[f'main:{DEL}|{DEL}@{DEL}']['weight'] == pytest.approx(1 / 9, abs=1e-5)
    rep = jload(stew.p('reports', 'gen_000.json'))
    assert 'gap_M_A' in rep['judge']['cells'][f'{DEL}|{DD}'] and 'main stew' in rep['judge']['field'][DEL]
    # generation 1's turns continue each run from its version-1 iteration
    assert [c for c in stew.ops.calls if c[0] == 'launch'][-3:] == [
        ('launch', C.spec_dir(DEL, DEL), 4), ('launch', C.spec_dir(DEL, DD), 4), ('launch', C.spec_dir(DEL, SNS), 4)]
    assert st.version(C.spec_name(DEL, DD), 2)['iter'] == 4
    # the next hold point is generation 0's only
    assert st.d['held'] is None and not os.path.exists(stew.p('PAUSE'))


def test_a_crash_resumes_without_redoing_work(stew):
    stew.cfg['recipe']['hold_after'] = []
    stew.ops.distill_fail = 1
    with pytest.raises(RuntimeError):
        stew.run(max_phases=3)
    assert stew.st.tag() == 'distil_decks@0' and stew.st.d['errors']
    n_launch = len([c for c in stew.ops.calls if c[0] == 'launch'])
    assert stew.run(max_phases=2) == 'done'
    assert len([c for c in stew.ops.calls if c[0] == 'launch']) == n_launch
    played = list(stew.ops.judge_played)
    # the judge phase again (a kill after its results, before the state moved): nothing replayed
    stew.st.d['phase'] = 'judge_deck_gap'
    stew.run(only='judge_deck_gap')
    assert stew.ops.judge_played == played


def test_step_runs_only_the_next_phase(stew):
    with pytest.raises(Refused):
        stew.run(only='distil_decks')
    assert stew.run(only='train') == 'step'
    assert stew.st.tag() == 'distil_decks@0'


def test_stop_during_a_turn_pauses_the_specialist_at_its_boundary(stew):
    ops = stew.ops
    calls = {'n': 0}

    def sup(o, run):
        calls['n'] += 1
        if calls['n'] == 3:
            jsave(stew.p('STOP'), {})
    ops.on_supervisor = sup
    assert stew.run() == 'stopped'
    st = stew.st
    assert st.d['turn']['spec'] == C.spec_name(DEL, DEL) and st.d['turn']['target'] == 2
    assert ('stop', C.spec_dir(DEL, DEL)) in ops.calls
    run = st.abs(st.net(C.spec_name(DEL, DEL))['run'])
    assert ('pause', run) in ops.calls
    assert st.d['farm_to_pause'] == [] and ops.farm['src'] == 'active'
    # resumed: the same target, the leftover control files cleared first
    ops.on_supervisor = None
    stew.cfg['recipe']['hold_after'] = []
    assert stew.run(max_phases=1) == 'done'
    assert [c for c in ops.calls if c[0] == 'launch'][:2] == [('launch', C.spec_dir(DEL, DEL), 2)] * 2
    assert ('clear', C.spec_dir(DEL, DEL)) in ops.calls


def test_a_turn_without_progress_fails_loudly(stew, monkeypatch):
    import mtgz.stew.orchestrator as O
    monkeypatch.setattr(O, 'GRACE_S', -1.0)
    stew.ops.train_noop = True
    with pytest.raises(Refused, match='not 2'):
        stew.run()
    assert stew.st.tag() == 'train@0' and stew.st.version(C.spec_name(DEL, DEL), 1) is None


def test_refuses_a_live_supervisor_it_does_not_own(stew, tmp_path):
    stew.ops.live = [{'pid': 1, 'run': 'E:/elsewhere/runs/x'}]
    with pytest.raises(Refused, match='elsewhere'):
        stew.run()
    stew.cfg['beside_runs'] = ['E:/elsewhere/runs/x']
    stew.ops.live = [{'pid': 1, 'run': os.path.join(str(tmp_path), 'meta3')},
                     {'pid': 2, 'run': 'runs/' + C.spec_dir(DEL, DD), 'cwd': stew.dir},
                     {'pid': 3, 'run': 'E:/elsewhere/runs/x'}]
    assert stew.run(max_phases=1) == 'done'
    assert not [c for c in stew.ops.calls if c[0] in ('pause', 'resume') and 'elsewhere' in c[1]]


def test_a_yield_run_paused_before_the_stew_stays_paused(stew, tmp_path):
    y = os.path.join(str(tmp_path), 'meta3')
    stew.ops.status[y] = 'paused'
    stew.ops.farm['src'] = 'paused'
    stew.run(max_phases=1)
    assert ('pause', y) not in stew.ops.calls and ('resume', y) not in stew.ops.calls
    assert stew.ops.farm['src'] == 'paused'


def test_add_deck_joins_at_the_next_generation(tmp_path, monkeypatch):
    src = make_source(tmp_path)
    cfgp = make_cfg(tmp_path, src, decks=[DEL, DD], hold_after=[])
    ops = FakeOps(tmp_path)
    monkeypatch.setattr(Stew, 'judge_play', lambda self: {'seat': {}})
    s = Stew.init(os.path.join(str(tmp_path), 'stew'), cfgp, ops=ops, log=lambda m: None)
    s.poll_s = 0.0
    assert [k for k in s.st.d['nets'] if k.startswith('spec:')] == [C.spec_name(DEL, DEL), C.spec_name(DEL, DD)]
    s.run(max_phases=1)                         # train@0 done
    s.add_deck(SNS)
    assert s.st.d['pending'][0]['name'] == SNS and s.st.d['pending'][0]['share'] == 1
    with pytest.raises(Refused):
        s.add_deck(SNS)
    s.run(max_phases=5)                         # the rest of generation 0, then train@1 applies it
    st = s.st
    assert SNS in C.deck_names(st.decks()) and st.d['pending'] == []
    assert st.version(C.deck_net(SNS), 1)['parents'] == ['main@1']
    assert open(st.ckpt(C.deck_net(SNS), 1), 'rb').read() == open(st.ckpt('main', 1), 'rb').read()
    sn = C.spec_name(DEL, SNS)
    assert st.net(sn)['joined'] == 1 and st.version(sn, 1)['iter'] == 0
    assert st.version(sn, 2)['iter'] == 2 and st.net(sn)['farm_run'] == 'tst-izzet-sneak'
    # generation 1's deck stew of Delver now distils from three specialists
    s.run(max_phases=1)
    de = jload(s.sources_path(C.deck_net(DEL), 1))
    assert [x['name'] for x in de['sources']] == [C.spec_name(DEL, DEL), C.spec_name(DEL, DD), sn]


def test_add_deck_while_a_run_is_live_writes_a_request(stew):
    jsave(stew.lock_path(), {'pid': os.getpid(), 'started': __import__('psutil').Process().create_time()})
    try:
        Stew(stew.dir, ops=stew.ops, log=lambda m: None)
        stew.st.d['field']['decks'] = stew.st.d['field']['decks'][:2]
        stew.add_deck(SNS)
        assert os.listdir(stew.p('requests')) and stew.st.d['pending'] == []
        stew.read_requests()
        assert stew.st.d['pending'][0]['name'] == SNS
    finally:
        os.remove(stew.lock_path())


def test_plan_names_the_commands(stew):
    txt = stew.plan()
    assert 'tools/supervise.py' in txt and '--iters 2' in txt and 'pause_run.py' in txt
    stew.run(max_phases=1)
    assert 'tools/distill.py --sources' in stew.plan()
    stew.run(max_phases=1)
    txt = stew.plan()
    assert 'jobs' in txt and 'best-of-three matches' in txt and 'HOLD' in txt
    assert 'generation 0' in stew.status()


# ------------------------------------------------------------------ sources on another field
def test_main_sources_with_a_deck_outside_the_archive(stew):
    st = stew.st
    stew.run(max_phases=3)
    os.remove(stew.p('PAUSE'))
    st.d['field']['decks'].append({'name': 'hogaak_x', 'share': 1, 'alias': 'hogaak'})
    st.add_version(C.deck_net('hogaak_x'), {'gen': 1, 'path': st.version(C.deck_net(DD), 1)['path'], 'sha256': ''})
    st.d['field']['candidates'] = [DEL]
    st.d['nets'][C.spec_name(DEL, 'hogaak_x')] = {'run': 'x', 'versions': []}
    snaps = {k: ['d'] for k in [(DEL, DEL), (DEL, DD), (DEL, SNS), (DEL, 'hogaak_x')]}
    doc, note = S.main_sources(st, stew.cfg, 0, snaps, stew.p('archive'), jload(stew.p('archive', 'config.json')))
    by = {s['name']: s for s in doc['sources']}
    assert 'hogaak_x' in note and 'retention' in by
    flat = {tuple(sorted(p)) for p in by['retention']['select']['not_pairing']}
    assert flat == {tuple(sorted(p)) for p in [(DEL, DEL), (DEL, DD), (DEL, SNS)]}
    # 16 cells: 7 covered, 9 uncovered of which 4 served (no Hogaak rows): retention 4 / (16 - 5)
    assert by['retention']['weight'] == pytest.approx(4 / 11, abs=1e-5)
    assert sum(s['weight'] for s in doc['sources']) == pytest.approx(1.0, abs=1e-4)
