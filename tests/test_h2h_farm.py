"""zero.h2h's farm backend (docs/ENGINE.md 217, mtgz.az.h2h_farm; zero-core docs/az.md 69): the farm job whose seats are
the local match's (each side's own settings as its seat's set, checked dict for dict), the conditions in order with
their reasons (local-only options, run configs, the machine's farm keys, a dirty checkout, no release, the build's
check-config, the size under auto), the campaign names (deterministic, `cmp-h2h-...`), the runner (a campaign a batch,
its rows as zero.h2h's with their records, the line's keys, a state of other inputs refused), tools/farm_campaign.py
submit's --chance-k. No farm, worker, network or GPU is needed: fake checkpoints (the decisions read only run configs and
file hashes) and fake campaign steps."""
import json
import os
import sys

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
import mtgz.az  # noqa: E402,F401
from mtgz.az import farm_campaign as C, h2h_farm as HF  # noqa: E402
from zero import h2h, package  # noqa: E402

PRESET = 'delver-mirror-bo3-dealt'


def _run(tmp_path, name='run', **over):
    run = tmp_path / name
    (run / 'models').mkdir(parents=True)
    for i in (0, 16):
        (run / 'models' / f'iter_{i:04d}.pt').write_bytes(f'{name}-{i}'.encode())
    cfg = dict({'preset': PRESET, 'eval_every': 16, 'eval_spec': mtgz.az.PRESETS[PRESET]['eval_spec']}, **over)
    (run / 'config.json').write_text(json.dumps(cfg))
    return str(run)


def _ck(run, i):
    return os.path.join(run, 'models', f'iter_{i:04d}.pt')


def _args(*argv):
    a = h2h.parser().parse_args([str(x) for x in argv])
    return a, h2h.sides_of(a), a.sprt is not None or a.target_se is not None


def test_the_backend_is_registered_with_its_options():
    assert package.active().h2h_backend_of() is HF.BACKEND and HF.BACKEND.name == 'farm'
    a = h2h.parser().parse_args(['x.pt', 'y.pt'])
    assert (a.backend, a.farm_min_work, a.farm_deadline_h, a.farm_share) == ('auto', HF.MIN_WORK, None, None)


def test_the_farm_jobs_seats_are_the_local_matchs(tmp_path):
    run = _run(tmp_path)
    a16, a0 = _ck(run, 16), _ck(run, 0)
    # A vs B of one run, no options: the seats of before, gens 1 and 2
    a, sides, _ = _args(a16, a0, '--games', '200', '--paired', '--chance-k', '0')
    cfg, seats, b = HF.job(a, sides)
    assert seats == [{'net': 1}, {'net': 2}] and b == a0
    assert (cfg['eval_trace'], cfg['sparse'], cfg['max_steps'], cfg['chance_k'], cfg['eval_sims']) == (1, 0, 5000, 0, 64)
    assert C.farm_seats(cfg, seats) == HF.local_seats(a, sides)
    # per-side options: each seat's set; one network on both seats (b None)
    a, sides, _ = _args(a16, a16, '--a-infoset-merge', '1', '--a-leaf-value-view', 'root', '--b-sims', '16')
    cfg, seats, b = HF.job(a, sides)
    assert b is None and seats == [{'net': 1, 'set': {'eval_infoset_merge': 1, 'eval_leaf_value_view': 'root'}},
                                   {'net': 1, 'set': {'eval_sims': 16}}]
    got = C.farm_seats(cfg, seats)
    assert got == HF.local_seats(a, sides) and got[0]['infoset_merge'] and got[1]['sims'] == 16
    # B of a run that evaluates otherwise (its cache, its rule): B's own settings are its set - the local match's seat
    other = _run(tmp_path, 'other', eval_cache=1234, eval_select='visits')
    a, sides, _ = _args(a16, _ck(other, 16))
    cfg, seats, b = HF.job(a, sides)
    assert seats[0] == {'net': 1} and seats[1]['net'] == 2 and seats[1]['set']['eval_cache'] == 1234
    assert C.farm_seats(cfg, seats) == HF.local_seats(a, sides)
    # a combination zero.h2h refuses is the reason
    a, sides, _ = _args(a16, a16, '--a-sims', '8')
    a.a_infoset_merge, a.a_leaf_value_view = 1, 'mover'
    with pytest.raises(HF.NotHere, match='leaf'):
        HF.job(a, sides)


def test_local_only_options_are_reasons():
    for argv, want in ((['--lookahead', 'plies=2'], '--lookahead is local-only'),
                       (['--b-lookahead', 'plies=2'], '--b-lookahead is local-only'),
                       (['--a-graphs', '1'], '--a-graphs is local-only'),
                       (['--precision', 'bf16'], '--precision bf16 is local-only'),
                       (['--move-log', '5'], '--move-log is local-only')):
        a = h2h.parser().parse_args(['x.pt', 'y.pt'] + argv)
        assert HF.local_only(a).startswith(want), argv
    for argv in ([], ['--precision', 'fp32'], ['--device', 'cuda', '--conc', '512', '--threads', '6', '--sparse', '1']):
        assert HF.local_only(h2h.parser().parse_args(['x.pt', 'y.pt'] + argv)) is None


@pytest.fixture
def farm_ok(monkeypatch, tmp_path):
    """Every machine condition met: keys, a clean checkout, a release with every route, the build's check-config."""
    w = tmp_path / 'worker.exe'
    w.write_bytes(b'w')
    entry = {'commit': 'c' * 40, 'build_id': '5e4023e25a380247f08fc3bb', 'runtime': 'd' * 24, 'worker': str(w),
             'cuda': {'build_id': '1' * 24}, 'dml': {'build_id': '2' * 24},
             'mac': {'release': {'build_id': '3' * 24, 'published': True}}, 'exact': True}
    calls = []
    monkeypatch.setattr(C, 'STATE_ROOT', str(tmp_path / 'campaigns'))
    monkeypatch.setattr(C, 'EXPORT_CACHE', str(tmp_path / 'campaigns' / '_exports'))
    monkeypatch.setattr(HF, 'can_put', lambda: None)
    monkeypatch.setattr(HF, 'tracked_changes', lambda root=None: [])
    monkeypatch.setattr(HF, 'release_of', lambda root=None: entry)
    monkeypatch.setattr(HF, 'no_onnx', lambda: [])

    def job_settings(worker, build_id, cfg, d, log, samples=False, seats=None):
        calls.append((worker, build_id, seats))
        return {k: v for k, v in cfg.items() if k in ('eval_sims', 'chance_k', 'eval_spec', 'eval_trace', 'game')}
    monkeypatch.setattr(C, 'job_settings', job_settings)
    return entry, calls


def test_the_conditions_in_order(tmp_path, monkeypatch, farm_ok):
    entry, calls = farm_ok
    run = _run(tmp_path)
    a16 = _ck(run, 16)
    big = [a16, a16, '--paired', '--games', '200', '--sims', '64', '--a-infoset-merge', '1', '--a-leaf-value-view',
           'root', '--b-infoset-merge', '0']
    ok, why, runner = HF.BACKEND.plan(*_args(*big)[:1], 'auto', *_args(*big)[1:])
    assert ok and why.startswith('campaign cmp-h2h-run-16v16-')
    assert ', build 5e4023e2 + CUDA/DML/macOS; estimate: farm ~5 setup + ~30 queue + ~47 play = ~83 min, local ~284 min ' \
           "of this machine's CPU" in why
    assert calls[-1][1] == entry['build_id'] and calls[-1][2][0]['set']['eval_infoset_merge'] == 1
    # under auto a small match plays here; asked for the farm it goes there whatever its size
    small = [a16, a16, '--paired', '--games', '8', '--sims', '64']
    a, sides, seq = _args(*small)
    ok, why, _ = HF.BACKEND.plan(a, 'auto', sides, seq)
    assert not ok and why.startswith('512 game-simulations, below the farm\'s minimum (--farm-min-work 12800')
    assert why.endswith("local ~11 min of this machine's CPU")
    assert HF.BACKEND.plan(a, 'farm', sides, seq)[0]
    a, sides, seq = _args(*small, '--farm-min-work', '0')
    assert HF.BACKEND.plan(a, 'auto', sides, seq)[0]
    # a sequential match counts its most games and names its campaigns a batch each
    a, sides, seq = _args(a16, a16, '--paired', '--games', '50', '--sprt', '0,20', '--max-games', '400')
    ok, why, runner = HF.BACKEND.plan(a, 'auto', sides, seq)
    assert ok and seq and '-b0 and one a batch after it' in why
    # the reasons, each in its turn
    a, sides, seq = _args(*big, '--b-lookahead', 'plies=2')
    assert HF.BACKEND.plan(a, 'auto', sides, seq)[1].startswith('--b-lookahead is local-only')
    bare = tmp_path / 'bare' / 'models'
    bare.mkdir(parents=True)
    (bare / 'iter_0001.pt').write_bytes(b'z')
    a, sides, seq = _args(a16, bare / 'iter_0001.pt', '--games', '400')
    a.b = str(bare / 'iter_0001.pt')
    assert 'side B (iter_0001.pt) has no run config' in HF.BACKEND.plan(a, 'auto', sides, seq)[1]
    a, sides, seq = _args(*big)
    monkeypatch.setattr(HF, 'can_put', lambda: 'no farm admin token on this machine (x)')
    assert HF.BACKEND.plan(a, 'auto', sides, seq)[1] == 'no farm admin token on this machine (x)'
    monkeypatch.setattr(HF, 'can_put', lambda: None)
    monkeypatch.setattr(HF, 'tracked_changes', lambda root=None: [' M mtgz/az/mcts.py'])
    assert HF.BACKEND.plan(a, 'auto', sides, seq)[1].startswith(
        'the checkout has uncommitted changes to tracked files ( M mtgz/az/mcts.py)')
    monkeypatch.setattr(HF, 'tracked_changes', lambda root=None: [])

    def no_release(root=None):
        raise HF.NotHere('no published farm build for 0123456789ab (x; python tools/farm_release.py release 0123456789ab)')
    monkeypatch.setattr(HF, 'release_of', no_release)
    assert 'python tools/farm_release.py release 0123456789ab' in HF.BACKEND.plan(a, 'farm', sides, seq)[1]
    monkeypatch.setattr(HF, 'release_of', lambda root=None: entry)

    def refuses(*x, **k):
        raise C.CampaignError('worker build 5e40 does not play per-seat settings')
    monkeypatch.setattr(C, 'job_settings', refuses)
    assert 'refuses the job: worker build 5e40 does not play per-seat settings' in HF.BACKEND.plan(a, 'auto', sides, seq)[1]
    # a Python without onnx: only while an export is still needed
    monkeypatch.setattr(HF, 'no_onnx', lambda: ['onnx'])
    assert HF.BACKEND.plan(a, 'auto', sides, seq)[1].startswith('this Python cannot export the networks for the farm (no '
                                                                'onnx;')
    monkeypatch.setattr(HF, 'missing_exports', lambda *x, **k: [])
    assert 'refuses the job' in HF.BACKEND.plan(a, 'auto', sides, seq)[1]


def test_release_of_names_the_command(tmp_path, monkeypatch):
    from mtgz.az import farm_builds as FB
    monkeypatch.setattr(FB, 'lookup', lambda rev, root=None: None)
    monkeypatch.setattr(FB, 'resolve', lambda rev, root=None: 'a' * 40)
    with pytest.raises(HF.NotHere, match=r'python tools/farm_release.py release aaaaaaaaaaaa'):
        HF.release_of()
    monkeypatch.setattr(FB, 'lookup', lambda rev, root=None: {'build_id': 'b' * 24, 'worker': str(tmp_path / 'none.exe')})
    with pytest.raises(HF.NotHere, match='not on this machine'):
        HF.release_of()


def test_tracked_changes_of_a_scratch_checkout(tmp_path):
    import subprocess
    r = str(tmp_path / 'g')
    os.makedirs(r)
    for cmd in (['git', 'init', '-q'], ['git', 'config', 'user.email', 't@example.com'], ['git', 'config', 'user.name', 't']):
        subprocess.run(cmd, cwd=r, check=True)
    with open(os.path.join(r, 'f.txt'), 'w') as f:
        f.write('a\n')
    subprocess.run(['git', 'add', 'f.txt'], cwd=r, check=True)
    subprocess.run(['git', 'commit', '-q', '-m', 'x'], cwd=r, check=True)
    with open(os.path.join(r, 'u.txt'), 'w') as f:
        f.write('untracked\n')
    assert HF.tracked_changes(r) == []
    with open(os.path.join(r, 'f.txt'), 'w') as f:
        f.write('b\n')
    assert HF.tracked_changes(r) == [' M f.txt']


def test_campaign_names_are_deterministic_and_their_own(tmp_path, farm_ok):
    run = _run(tmp_path, 'legacy-meta3-r1')
    argv = [_ck(run, 16), _ck(run, 16), '--paired', '--games', '200', '--a-sims', '128']
    names = []
    for extra in ([], [], ['--seed', '1'], ['--games', '202'], ['--b-sims', '32']):
        a, sides, seq = _args(*argv, *extra)
        ok, why, r = HF.BACKEND.plan(a, 'farm', sides, seq)
        names.append(r.name_of(0, a.games, a.seed))
    assert names[0] == names[1] and len(set(names)) == 4
    for n in names:
        s = C.slug(n)
        assert s.startswith('cmp-h2h-legacy-meta3-r1-16v16-') and len(s) <= 63 and C.SLUG_RE.match(s)
    a, sides, seq = _args(*argv, '--sprt', '0,20')
    r = HF.BACKEND.plan(a, 'farm', sides, seq)[2]
    assert r.name_of(3, 200, 3 * 7919).endswith('-b3') and r.name_of(3, 200, 3 * 7919) != r.name_of(2, 200, 2 * 7919)


def test_deadlines_and_work():
    a = h2h.parser().parse_args(['x.pt', 'y.pt', '--games', '200', '--sims', '64'])
    assert HF.work_of(a, False) == 12800 and HF.deadline_s(a, 200) == 3600 + 0.25 * 12800
    a = h2h.parser().parse_args(['x.pt', 'y.pt', '--games', '2400', '--sims', '64', '--a-sims', '128'])
    assert HF.work_of(a, False) == 2400 * 96 and HF.deadline_s(a, 2400) == 12 * 3600
    a = h2h.parser().parse_args(['x.pt', 'y.pt', '--games', '100', '--target-se', '0.02', '--farm-deadline-h', '0.5'])
    assert HF.work_of(a, True) == 400 * 64 and HF.deadline_s(a, 100) == 1800


def test_the_runner_plays_a_campaign_a_batch(tmp_path, monkeypatch, farm_ok):
    run = _run(tmp_path)
    a16 = _ck(run, 16)
    a, sides, seq = _args(a16, a16, '--paired', '--games', '4', '--sims', '8', '--a-sims', '16', '--threads', '3')
    r = HF.BACKEND.plan(a, 'farm', sides, seq)[2]
    root = tmp_path / 'campaigns'
    monkeypatch.setattr(C, 'STATE_ROOT', str(root))
    put = []

    def submit(name, ck, games, settings, seats, seed, build_id, worker, runtime, b=None, paired=True, share=None,
               deadline_s=None, meta=None, client=None, log=None, **kw):
        d = C.state_dir(C.slug(name))
        put.append(dict(name=name, games=games, seats=seats, seed=seed, b=b, share=share, deadline_s=deadline_s,
                        meta=meta, kw=kw))
        C.save_state(d, {'run': C.slug(name), 'meta': meta, 'created': 100.0, 'submitted': 110.0,
                         'first_result': 140.0})
        return d

    def collect(d, client=None, threads=4, describe_games=True, log=None, **kw):
        assert threads == 3 and not describe_games
        return {'coverage': {'games': 4, 'done': 4, 'farm': 3, 'local': 1, 'failed': [], 'missing': []},
                'machines': {'m_abc': 3, 'local': 1}, 'digest': 'd' * 16, 'spot': {'checked': 1, 'ok': 1, 'of': 4},
                'alternates': [{'build_id': '1' * 24}], 'alternates_note': None}

    seats = r.seats

    def games(d):
        from zero import farm_eval as FE
        return [{'gid': g, 'pair': g // 2, 'seats': FE.seats_of(seats, g), 'result': [1.0, 0.0], 'margin': 2.0,
                 'rounds': 3, 'over': True, 'digest': 'x', 'rec': {'gid': g}} for g in range(4)]
    monkeypatch.setattr(C, 'submit', submit)
    monkeypatch.setattr(C, 'collect', collect)
    monkeypatch.setattr(C, 'games', games)
    monkeypatch.setattr(HF.Runner, '_check_name', lambda self, run, key, client: None)
    rows = r.play(0, 4, 0)
    assert [x['gid'] for x in rows] == [0, 1, 2, 3] and rows[1]['scores'] == [2.0, 0.0] and rows[0]['rec'] == {'gid': 0}
    p = put[0]
    assert p['games'] == 4 and p['seed'] == 0 and p['b'] is None and p['share'] == HF.AUTO_SHARE == 0.5
    assert p['seats'][0] == {'net': 1, 'set': {'eval_sims': 16}} and p['meta']['cmd'] == 'h2h'
    assert p['deadline_s'] == 3600 + 0.25 * 4 * 12 and 'alternates' not in p['kw'], 'the release alternates (default)'
    info = r.info()
    from zero import gamerec as GR
    assert info['campaign'] == C.slug(p['name']) and info['build'] == '5e4023e25a380247f08fc3bb'
    assert info['routes'] == ['5e4023e25a380247f08fc3bb', '1' * 24]
    assert info['coverage'] == {'farm': 3, 'local': 1, 'failed': 0}
    assert info['machines'] == {GR.machine_tag('m_abc'): 3, 'local': 1} and 'm_abc' not in json.dumps(info)
    assert (info['queue_s'], info['games_digest'], info['spot']) == (30.0, 'd' * 16, {'checked': 1, 'ok': 1})
    # a state of that name with other inputs is refused, not resumed
    monkeypatch.undo()
    monkeypatch.setattr(C, 'STATE_ROOT', str(root))
    key = HF.campaign_key(a, r.play_settings, r.seats, r.e['build_id'], 0, 4, 0, False)
    with pytest.raises(C.CampaignError, match='other inputs'):
        r._check_name(C.slug(p['name']), 'f' * 64, None)
    r._check_name(C.slug(p['name']), key, None)


def test_the_estimate_reads_the_farms_pace_and_the_export_cache(tmp_path, monkeypatch):
    root = tmp_path / 'campaigns'
    monkeypatch.setattr(C, 'STATE_ROOT', str(root))
    monkeypatch.setattr(C, 'EXPORT_CACHE', str(root / '_exports'))
    assert HF.farm_history() == (None, None)
    for k, (q, play, sims, local) in enumerate(((600, 1200, 64, 0), (1200, 2400, 64, 0), (60, 60, 64, 5))):
        C.save_state(str(root / f'cmp-x{k}'), {'games': 200, 'settings': {'eval_sims': sims}, 'seats': [{'net': 1}, {'net': 2}],
                                               'submitted': 1000.0, 'first_result': 1000.0 + q,
                                               'result': {'seconds': q + play, 'coverage': {'local': local}}})
    q, rate = HF.farm_history()
    assert q == 20.0 and rate == 200 * 64 / 20.0, 'medians of the campaigns without local games'
    run = _run(tmp_path)
    a = h2h.parser().parse_args([_ck(run, 16), _ck(run, 16), '--games', '200', '--device', 'cuda'])
    cfg = {'eval_spec': json.dumps({'x': 1})}
    assert HF.estimate(a, False, cfg, None) == ("estimate: farm ~5 setup + ~20 queue + ~20 play = ~45 min, local ~5 min of "
                                                "this machine's GPU")
    from zero import games as G
    key = __import__('hashlib').sha256(f'{C.file_sha256(_ck(run, 16))}|{G.spec_json(cfg["eval_spec"])}|1'.encode()).hexdigest()[:20]
    os.makedirs(root / '_exports' / key)
    for f in ('export.json', 'variant-matmul.json', 'variant-fp16.json'):
        (root / '_exports' / key / f).write_text('{}')
    assert HF.estimate(a, False, cfg, None).startswith('estimate: farm ~0 setup + ~20 queue')


def test_the_top_up_takes_a_throttle_slot_beside_a_live_run(monkeypatch):
    from zero import runctl
    monkeypatch.setattr(runctl, 'live_supervisors', lambda: [{'pid': 1, 'run': 'r'}])
    w = HF.topup_wrap()
    assert w[-1].endswith(os.path.join('tools', 'slotd.sh')) and os.path.basename(w[0]).lower().startswith('bash')
    monkeypatch.setattr(runctl, 'live_supervisors', lambda: [])
    assert HF.topup_wrap() is None


def test_farm_campaign_submit_takes_chance_k(tmp_path, monkeypatch):
    sys.path.insert(0, os.path.join(ROOT, 'tools'))
    import farm_campaign as FC
    run = _run(tmp_path)
    got = {}

    def match_settings(a, sims, spec, tilts=None, ckpt_b=None, chance_k=2, sides=None, **kw):
        got['chance_k'] = chance_k
        raise C.CampaignError('stop here')
    monkeypatch.setattr(C, 'match_settings', match_settings)
    monkeypatch.setattr(FC, 'find_worker', lambda b, d=None: ('w.exe', {'requires_runtime': 'r' * 24}))
    from zero import farm_def as FD
    cl = FD.Client('http://fake', token='t', transport=lambda *x: (200, b'{}'))
    for extra, want in (([], 2), (['--chance-k', '0'], 0)):
        with pytest.raises(SystemExit):
            FC.main(['submit', 'x', '--a', _ck(run, 16), '--games', '8', '--build', 'b' * 24,
                     '--state-root', str(tmp_path / 's')] + extra, client=cl)
        assert got['chance_k'] == want


def test_a_standalone_cells_comparison_keeps_its_backend(tmp_path, monkeypatch, farm_ok):
    """run_yardsticks.py cells-match / ab.py judge (docs/ENGINE.md 217): auto plays on the farm unless the comparison's
    earlier games (its cache's baselines, its rows file) were played here; farm earlier and impossible now: refused."""
    from mtgz.az import yardsticks as Y
    entry, _ = farm_ok
    run = _run(tmp_path)
    ck, ref = _ck(run, 16), _ck(run, 0)
    ys = Y.normalize({'bo1_games': 0, 'blunders': False, 'cells_games': 4, 'cells_refs': ['ref=' + ref],
                      'cells_decks': ['izzet_delver_8016541', 'doomsday_8027957']})
    jobs = []
    monkeypatch.setattr(C, 'match_job', lambda rule_ck, a, b, sims, spec, tilts=None: (
        jobs.append((rule_ck, a, b)) or ({'eval_spec': spec}, {}, [{'net': 1}, {'net': 1 if a == b else 2}])))
    cdir = str(tmp_path / 'cache')
    assert Y.standalone_backend(ck, ref, ys, cdir, 'local')[0] == 'local'
    backend, why, farm = Y.standalone_backend(ck, ref, ys, cdir, 'auto')
    assert backend == 'farm' and why == 'build 5e4023e2 + CUDA/DML/macOS'
    assert farm['own_base'] and farm['rule_ck'] == ck and farm['pool']['build_id'] == entry['build_id']
    assert (ck, ck, ref) in jobs and (ck, ref, ref) in jobs, 'every chooser match and its baseline checked'
    os.makedirs(cdir)
    with open(Y.base_file(cdir, 'old'), 'w') as f:
        json.dump({'results': {}, 'time': 1}, f)                 # (an earlier baseline played here)
    assert Y.standalone_backend(ck, ref, ys, cdir, 'auto')[:2] == (
        'local', "kept: the comparison's earlier games (1 baselines / rows in its cache and rows file) were played here")
    assert Y.standalone_backend(ck, ref, ys, cdir, 'farm')[0] == 'farm'
    with open(Y.base_file(cdir, 'old'), 'w') as f:
        json.dump({'results': {}, 'time': 1, 'backend': 'farm'}, f)
    assert Y.standalone_backend(ck, ref, ys, cdir, 'auto')[0] == 'farm'
    monkeypatch.setattr(HF, 'tracked_changes', lambda root=None: [' M x.py'])
    with pytest.raises(SystemExit, match='earlier games were played on the farm'):
        Y.standalone_backend(ck, ref, ys, cdir, 'auto')
    os.remove(Y.base_file(cdir, 'old'))
    b, why, _ = Y.standalone_backend(ck, ref, ys, cdir, 'auto')
    assert b == 'local' and why.startswith('the checkout has uncommitted changes')
    with pytest.raises(SystemExit, match='backend farm refused'):
        Y.standalone_backend(ck, ref, ys, cdir, 'farm')


def test_a_lane_class_replaces_the_share_only_when_set(tmp_path):
    """submit(job_class=...): the run PUT's eval batch (or a samples job's self-play) carries `class` and no `share` only
    when a class is given (the coordinator's backfill lane, zero-farm rev 25); None: the bodies of before."""
    st = {'build_game': 'mtg', 'build_id': 'b' * 24, 'runtime': 'r' * 24, 'priority': 0.5, 'batch': 'c1', 'games': 8,
          'gens': {'1': {}}, 'games_per_lease': 'auto', 'seed': 0, 'settings': {'x': 1}, 'seats': [{'net': 1}] * 2,
          'paired': True, 'share': 0.5, 'job': {'kind': 'play', 'trace': True, 'samples': False}}
    ev = C.run_body(st)['eval']
    assert ev['share'] == 0.5 and 'class' not in ev
    ev = C.run_body(dict(st, job_class='backfill'))['eval']
    assert ev['class'] == 'backfill' and 'share' not in ev
    sp = C.run_body(dict(st, job_class='backfill', job={'kind': 'play', 'trace': True, 'samples': True}))['selfplay']
    assert sp['class'] == 'backfill' and 'share' not in sp
    assert C.run_body(dict(st, job={'kind': 'play', 'trace': True, 'samples': True}))['selfplay']['share'] == 0.5
    import inspect
    assert inspect.signature(C.submit).parameters['job_class'].default is None
