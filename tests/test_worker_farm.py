"""mtgz-worker's farm jobs (zero-farm; docs/ENGINE.md 114.1-114.4): fuzz jobs over `aw/1` (the engine's fuzz
loops in supervised chunk processes), the reproduction string tools/fuzz_farm.py would write, the per-machine
self-test (`reference`), eval jobs (byte-identical to the Python evaluation, check-config --eval, the game
package's own settings), and - with a fuzz-profile build - injected panics, aborts, stack overflows and hangs
isolated to their seed while the job goes on. Skipped without a built worker (`cargo build --release -p
mtgz_worker`; env MTGZ_WORKER), the self-test without onnx/onnxscript/onnxruntime, the fault tests without the
fuzz-profile build (`cargo build --profile fuzz -p mtgz_worker`; env MTGZ_WORKER_FUZZ)."""
import json
import os
import subprocess
import sys
import threading
import time

import numpy as np
import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'tools'))


def _exe(var, profile):
    exe = 'mtgz-worker.exe' if os.name == 'nt' else 'mtgz-worker'
    cands = [os.environ.get(var, '')]
    if os.environ.get('CARGO_TARGET_DIR'):
        cands.append(os.path.join(os.environ['CARGO_TARGET_DIR'], profile, exe))
    cands += [os.path.join(ROOT, 'target', profile, exe), os.path.join('E:/cargo-target/mtg-zero/farm', profile, exe)]
    return next((c for c in cands if c and os.path.exists(c)), None)


WORKER = _exe('MTGZ_WORKER', 'release')
FUZZ_WORKER = _exe('MTGZ_WORKER_FUZZ', 'fuzz')
pytestmark = pytest.mark.skipif(WORKER is None, reason='mtgz-worker is not built (cargo build --release -p mtgz_worker)')


class Serve:
    """`mtgz-worker serve` driven as the agent drives it: JSON lines in, events out."""

    def __init__(self, exe, cwd, env=None):
        e = dict(os.environ)
        e.update(env or {})
        self.p = subprocess.Popen([exe, 'serve'], cwd=cwd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                  text=True, encoding='utf-8', env=e)
        self.events = []
        self.err = []
        self.t = threading.Thread(target=self._read, daemon=True)
        self.t.start()
        threading.Thread(target=lambda: self.err.extend(self.p.stderr), daemon=True).start()

    def _read(self):
        for ln in self.p.stdout:
            if ln.strip():
                self.events.append(json.loads(ln))

    def send(self, v):
        self.p.stdin.write(json.dumps(v) + '\n')
        self.p.stdin.flush()

    def wait_for(self, ev, timeout=600, n=1):
        t0 = time.time()
        while time.time() - t0 < timeout:
            got = [e for e in self.events if e.get('ev') == ev]
            if len(got) >= n:
                return got[n - 1]
            if self.p.poll() is not None and not self.t.is_alive():
                break
            time.sleep(0.05)
        raise AssertionError(f'no {ev} event: {self.events[-5:]} {"".join(self.err)[-2000:]}')

    def finish(self, timeout=600):
        self.p.wait(timeout)
        self.t.join(10)
        return self.p.returncode


def fuzz_job(tmp_path, **kw):
    j = {'op': 'job', 'lease_id': 'l_test', 'kind': 'fuzz', 'mode': 'games', 'env': {}, 'first': 10_000_000, 'count': 30,
         'threads': 2, 'out_dir': 'out', 'model': None, 'reference': None, 'runtime_dir': None, 'chunk': 10}
    j.update(kw)
    return j


def test_a_fuzz_job_over_aw1(tmp_path):
    w = Serve(WORKER, tmp_path)
    hello = w.wait_for('hello', 60)
    assert hello['aw'] == 1 and hello['kinds'] == ['selfplay', 'eval', 'fuzz'] and hello['game'] == 'mtg'
    w.send(fuzz_job(tmp_path))
    end = w.wait_for('end')
    assert w.finish() == 0
    assert end['reason'] == 'done' and end['unfinished'] == {'seeds': []} and end['stats']['seeds'] == 30, end
    assert sum(e['n'] for e in w.events if e['ev'] == 'fin') == 30
    assert not [e for e in w.events if e['ev'] in ('failure', 'error')]
    final = json.loads((tmp_path / 'out' / 'fuzz-final.json').read_text(encoding='utf-8'))
    assert final['final'] is True and final['done'] == 30 and final['seeds'] == [[10_000_000, 10_000_030]] and final['failures'] == []
    # the agent builds and uploads its own summary from `end` (agent 0.1.3): the worker announces none unless asked
    assert not [e for e in w.events if e['ev'] in ('records', 'shard')]


def test_fuzz_jobs_refuse_what_their_loop_would_ignore(tmp_path):
    # (FUZZ_POOL=delver's matches are constructed best-of-three since ENGINE.md 120: a valid mode now)
    for mode, env, why in [('delver_sb-matches', {'FUZZ_POOL': 'delver_sb'}, 'games only'),
                           ('x-matches', {'FUZZ_DECK_TILT': '2'}, 'needs FUZZ_POOL=delver'),
                           ('games', {'FUZZ_POOL': 'eoe'}, "fuzz_farm.py's"),
                           ('x-events', {'FUZZ_POOL': 'eoe'}, 'reads no settings'),
                           ('eoe-games', {'FUZZ_POOL': 'eoe', 'FUZZ_DECK_TILT': '2'}, 'would be ignored'),
                           ('x-games', {'FUZZ_CHANCE': '0'}, 'a number >= 1'),
                           ('nonsense', {}, '<name>-games'),
                           ('x-games', {'ZERO_FARM_TEST_FAULT': 'panic:1'}, 'test faults')]:
        w = Serve(WORKER, tmp_path)
        w.wait_for('hello', 60)
        w.send(fuzz_job(tmp_path, mode=mode, env=env))
        e = w.wait_for('error', 60)
        assert w.finish() == 1 and e['fatal'] is True and why in e['message'], (mode, env, e)


def test_the_repro_is_fuzz_farms(tmp_path):
    import fuzz_farm as FF
    sha = '0123456789abcdef0123456789abcdef01234567'
    for mode, m in FF.MODES.items():
        if m['kind'] not in ('fuzz', 'draft'):
            continue
        args = [WORKER, 'fuzz-check', '--mode', mode, '--seed', '10450007', '--commit', sha, '--profile', 'release']
        for k, v in m['env'].items():
            args += ['--env', f'{k}={v}']
        p = subprocess.run(args, capture_output=True, text=True, encoding='utf-8', timeout=120)
        assert p.returncode == 0, p.stderr
        got = json.loads(p.stdout)
        want = FF.repro_of(FF.Job(mode, 10450007, 1), 10450007, sha)
        assert got['repro'] == want, (mode, got['repro'], want)
        assert got['min_chunk'] == m['min'], mode


def test_yield_resume_and_quit_keep_the_seeds_that_passed(tmp_path):
    w = Serve(WORKER, tmp_path)
    w.wait_for('hello', 60)
    w.send(fuzz_job(tmp_path, count=4000, chunk=1000, threads=2))
    w.wait_for('fin', 120)
    t0 = time.time()
    w.send({'op': 'yield'})
    w.wait_for('yielded', 10)
    assert time.time() - t0 < 2.0
    time.sleep(0.5)                                   # (a fin of seeds that passed before the suspension may follow)
    n0 = sum(e['n'] for e in w.events if e['ev'] == 'fin')
    time.sleep(2.0)
    assert sum(e['n'] for e in w.events if e['ev'] == 'fin') == n0  # suspended: nothing plays
    w.send({'op': 'resume'})
    w.wait_for('resumed', 10)
    w.wait_for('fin', 120, n=len([e for e in w.events if e['ev'] == 'fin']) + 1)
    w.send({'op': 'quit'})
    end = w.wait_for('end', 120)
    assert w.finish() == 0 and end['reason'] == 'quit'
    done = sum(e['n'] for e in w.events if e['ev'] == 'fin')
    left = sum(b - a for a, b in end['unfinished']['seeds'])
    assert 0 < done < 4000 and done + left == 4000, (done, left)


@pytest.mark.skipif(FUZZ_WORKER is None, reason='no fuzz-profile mtgz-worker (cargo build --profile fuzz -p mtgz_worker)')
def test_crashing_and_hanging_seeds_are_isolated_and_the_job_goes_on(tmp_path):
    f0 = 10_000_000
    faults = f'panic:{f0 + 3},abort:{f0 + 11},overflow:{f0 + 17},hang:{f0 + 26}'
    w = Serve(FUZZ_WORKER, tmp_path, env={'ZERO_FARM_ALLOW_TEST_FAULTS': '1'})
    hello = w.wait_for('hello', 60)
    assert hello['kinds'] == ['fuzz']
    w.send(fuzz_job(tmp_path, count=30, chunk=30, threads=2, env={'ZERO_FARM_TEST_FAULT': faults}, seed_timeout_s=5))
    end = w.wait_for('end', 1200)
    assert w.finish() == 0
    fails = {e['seed']: e for e in w.events if e['ev'] == 'failure'}
    assert sorted(fails) == [f0 + 3, f0 + 11, f0 + 17, f0 + 26], fails.keys()
    assert end['reason'] == 'done' and end['unfinished'] == {'seeds': []} and end['stats']['seeds'] == 30
    p = fails[f0 + 3]
    assert p['site'].endswith('crates/worker/src/fuzz.rs:' + p['site'].rsplit(':', 1)[1]) and 'an injected panic' in p['panic']
    assert fails[f0 + 11]['site'].startswith('crash exit ') and fails[f0 + 17]['site'].startswith('crash exit ')
    assert fails[f0 + 26]['site'] == 'HANG games' and 'no exit after' in fails[f0 + 26]['panic']
    for s, e in fails.items():
        assert e['mode'] == 'games' and f'FUZZ_FIRST={s} FUZZ_GAMES=1 FUZZ_LOG=1 cargo test --profile fuzz' in e['repro'], e['repro']
        assert e['repro'].startswith('(at ') and f'ZERO_FARM_TEST_FAULT={faults}' in e['repro']


def _onnx_ok():
    try:
        import onnx  # noqa: F401
        import onnxruntime  # noqa: F401
        import onnxscript  # noqa: F401
        import torch  # noqa: F401
        return True
    except ImportError:
        return False


@pytest.mark.skipif(not _onnx_ok(), reason='needs torch, onnx, onnxscript and onnxruntime')
def test_the_selftest_guards_the_network(tmp_path):
    """A small random MTG network: its reference set from real driver batches (the worker's own recording) with
    torch's answers; through `serve` the worker answers it before playing (selftest ok, a game, its records), and with
    one prior of the set off by 1e-3 it reports the difference and stops with a fatal error before any game."""
    import onnxruntime
    import torch
    import mtgz.az  # noqa: F401
    from zero import games as G, net as N, onnx_export as OX, onnx_parity as OP
    from zero import run as R
    ns = type('A', (), {k: None for k in R.DEFAULTS})()
    cfg = R.resolve_config(ns, {'preset': 'mtg-smoke'})
    cfg.update(conc=4, threads=1, groups=1)
    cp = tmp_path / 'config.json'
    cp.write_text(json.dumps(cfg), encoding='utf-8')
    rec = tmp_path / 'batches.npz'
    p = subprocess.run([WORKER, 'selfplay', '--settings', str(cp), '--uniform', '--games', '2', '--threads', '1', '--out', str(tmp_path / 'u'),
                        '--record-batches', '200', '--record-file', str(rec)], capture_output=True, text=True, encoding='utf-8', timeout=1800)
    assert p.returncode == 0, p.stderr[-2000:]
    torch.manual_seed(3)
    m = N.make_net(G.info(cfg['game']), cfg['game'], arch='tokens', hidden=32, blocks=1, heads=2)
    for q in m.parameters():
        q.data.add_(0.2 * torch.randn_like(q))
    info = {'checkpoint': 'test/1', 'checkpoint_sha256': '9f' * 32, 'checkpoint_iter': '1'}
    OX.export(m, str(tmp_path / 'model.onnx'), 'packed', info)
    ref = tmp_path / 'reference.npz'
    meta = OX.write_reference(m, [OP.Rows.load(str(rec))], str(ref), 64, info)
    assert meta['rows'] == 64 and OX.check_reference(str(tmp_path / 'model.onnx'), str(ref), 1)['max_abs'] < 2e-5
    z = dict(np.load(ref))
    z['priors'] = z['priors'].copy()
    z['priors'][5] += 1e-3
    with open(tmp_path / 'bad.npz', 'wb') as f:
        np.savez(f, **z)
    rt = os.path.join(os.path.dirname(onnxruntime.__file__), 'capi')
    settings = json.loads(cp.read_text(encoding='utf-8'))
    import hashlib
    file_sha = hashlib.sha256((tmp_path / 'model.onnx').read_bytes()).hexdigest()
    # the job line's sha256 is the model FILE's (zero-core docs/az.md 44.3): another one ends the job before the graph
    # is read - no selftest, no game; the error names the job's hash, never the file's
    d = tmp_path / 'wrongsha'
    d.mkdir()
    w = Serve(WORKER, d)
    w.wait_for('hello', 60)
    w.send({'op': 'job', 'lease_id': 'l_wrongsha', 'kind': 'selfplay', 'settings': settings, 'games': 1, 'gid_base': 0, 'seed': 5,
            'model': str(tmp_path / 'model.onnx'), 'reference': str(ref), 'sha256': '9f' * 32, 'threads': 1, 'out_dir': 'out',
            'runtime_dir': rt})
    e = w.wait_for('error', 300)
    assert w.finish() == 1 and e['fatal'] is True and "not the job's" in e['message'] and file_sha not in e['message'], e
    assert not [x for x in w.events if x['ev'] in ('selftest', 'fin', 'shard', 'records')]
    for name, reference, ok in (('good', ref, True), ('bad', tmp_path / 'bad.npz', False)):
        d = tmp_path / name
        d.mkdir()
        w = Serve(WORKER, d)
        w.wait_for('hello', 60)
        w.send({'op': 'job', 'lease_id': 'l_' + name, 'kind': 'selfplay', 'settings': settings, 'games': 1, 'gid_base': 0, 'seed': 5,
                'model': str(tmp_path / 'model.onnx'), 'reference': str(reference), 'sha256': file_sha.upper() if ok else file_sha,
                'threads': 1, 'out_dir': 'out', 'runtime_dir': rt})
        st = w.wait_for('selftest', 300)
        assert st['provider'] == 'cpu' and st['rows'] == 64 and st['ok'] is ok, st
        if ok:
            assert st['max_abs'] < 2e-5
            end = w.wait_for('end', 1200)
            assert w.finish() == 0 and end['reason'] == 'done' and end['stats']['selftest_max_abs'] == st['max_abs']
            assert [e for e in w.events if e['ev'] == 'records']    # (a served self-play job's default: ENGINE.md 114.6)
            assert w.events.index(st) < w.events.index(next(e for e in w.events if e['ev'] == 'fin'))
        else:
            assert st['items']['priors'] > 2e-5 and abs(st['max_abs'] - 1e-3) < 1e-4
            e = w.wait_for('error', 300)
            assert w.finish() == 1 and e['fatal'] is True and 'selftest failed' in e['message']
            assert not [x for x in w.events if x['ev'] in ('fin', 'shard', 'records')] and not (d / 'out').exists()


# ------------------------------------------------------------------ eval jobs (ENGINE.md 114.4)

def _smoke_cfg(tmp_path, **kw):
    import mtgz.az  # noqa: F401
    from zero import farm_eval as FE, run as R
    ns = type('A', (), {k: None for k in R.DEFAULTS})()
    cfg = R.resolve_config(ns, {'preset': 'mtg-smoke'})
    cfg.update(conc=8, threads=1, groups=1, eval_sims=4, **kw)
    cfg = FE.resolve_eval_spec(cfg)
    cp = tmp_path / 'settings.json'
    cp.write_text(json.dumps(cfg), encoding='utf-8')
    return cfg, cp


def test_a_farm_h2h_definition_holds_the_play_settings_only(tmp_path):
    """zero.farm_h2h.play_settings with the real worker (ENGINE.md 114.5): a run's settings minus everything the
    worker's own check-config --eval does not call play - EVAL_READS, no trainer-only key, nothing path-like (zero-farm
    PROTOCOL.md 7: farm-release define refuses those in a template) -, checked again by the worker, the same seats; the
    worker must be the build named (its manifest's id); a definition with them validates as farm-release checks it."""
    import hashlib
    import shutil
    import mtgz.az  # noqa: F401
    from zero import farm_def as FD, farm_h2h as FH, farm_eval as FE
    cfg, _ = _smoke_cfg(tmp_path)
    cfg = dict(cfg, resolver_init='runs/x/models/iter_0001.pt', game='mtg')        # a trainer-only path
    d = tmp_path / 'build'
    d.mkdir()
    shutil.copy2(WORKER, d / 'mtgz-worker.exe')
    raw = json.dumps({'manifest': 1, 'kind': 'worker', 'game': 'mtg', 'entry': 'mtgz-worker.exe', 'files': []}).encode()
    (d / 'manifest.json').write_bytes(raw)
    bid = hashlib.sha256(raw).hexdigest()[:24]
    play = FH.play_settings(str(d / 'mtgz-worker.exe'), bid, cfg, str(tmp_path / 'st'))
    reads = json.loads(subprocess.run([WORKER, 'check-config', str(tmp_path / 'st' / 'settings.json'), '--eval', '--json'],
                                      capture_output=True, text=True, check=True).stdout)['eval_reads']
    assert set(play) == set(reads) and len(play) < len(cfg) and 'resolver_init' not in play and 'spec' not in play
    FD.check_no_paths(play, 'settings')
    assert FE.seat_dicts(play, [{'net': 1}, {'net': 2}]) == FE.seat_dicts(cfg, [{'net': 1}, {'net': 2}])
    ck = {'model_sha256': 'ab' * 32, 'model_size': 10, 'reference_sha256': None, 'reference_size': None}
    FD.definition('run', 'h2h-x', 1, 'mtg', [bid], None, {'1': ck, '2': ck},
                  {'eval': {'settings': play, 'seats': [{'net': 1}, {'net': 2}], 'paired': True}}, created=1)
    with pytest.raises(ValueError):                                                # (what farm-release would refuse)
        FD.definition('run', 'h2h-x', 1, 'mtg', [bid], None, {'1': ck, '2': ck},
                      {'eval': {'settings': cfg, 'seats': [{'net': 1}, {'net': 2}], 'paired': True}}, created=1)
    with pytest.raises(SystemExit, match='not the eval-capable worker build'):     # another build's worker
        FH.play_settings(WORKER, bid, cfg, str(tmp_path / 'st2'))


def test_eval_check_config_and_the_package_settings(tmp_path):
    """check-config --eval: a run's settings pass (the evaluation's read, the rest unused - the game package's own
    settings too, which the worker lists: every one of mtgz's GamePackage.settings is known); an external bot, an
    unset eval_spec are refused."""
    import mtgz.az  # noqa: F401
    from zero import package
    cfg, cp = _smoke_cfg(tmp_path)
    pkg = set(package.active().settings)
    assert pkg and pkg <= set(cfg), 'the resolved settings hold the package\'s own'
    for args in ([], ['--eval']):
        p = subprocess.run([WORKER, 'check-config', str(cp), '--json'] + args, capture_output=True, text=True, encoding='utf-8')
        out = json.loads(p.stdout)
        assert p.returncode == 0 and not out['failures'], out['failures']
        st = {r['key']: r['status'] for r in out['settings']}
        assert all(st[k] in ('off', 'trainer') for k in pkg), {k: st[k] for k in pkg}
    assert set(out['eval_reads']) >= {'eval_sims', 'eval_spec', 'eval_select'}
    p = subprocess.run([WORKER, 'check-config', str(cp), '--eval', '--seats', '[{"net": 1}, {"bot": "ext:jsbot"}]'],
                       capture_output=True, text=True, encoding='utf-8')
    assert p.returncode != 0 and 'external bot' in p.stderr
    bad = tmp_path / 'bad.json'
    bad.write_text(json.dumps(dict(cfg, eval_spec='')), encoding='utf-8')
    p = subprocess.run([WORKER, 'check-config', str(bad), '--eval'], capture_output=True, text=True, encoding='utf-8')
    assert p.returncode == 1 and 'eval_spec is unset' in p.stdout


@pytest.mark.skipif(not _onnx_ok(), reason='needs torch, onnx, onnxscript and onnxruntime')
def test_an_eval_job_is_the_python_evaluation(tmp_path):
    """An eval job over aw/1 (PROTOCOL.md 5.2) with two small random MTG networks: one self-test per network (with
    its gen), rows of whole pairs in 'evals' files, and every row byte-identical to the Python path - Run.play_match's
    driver (zero.farm_eval.match_driver) with zero-core's OnnxEvaluator on the same graphs (groups 1); a lease of the
    match's second pair plays those games; a network against the heuristic bot."""
    import hashlib
    import onnxruntime
    import torch
    import mtgz.az  # noqa: F401
    from zero import farm_eval as FE, games as G, net as N, onnx_export as OX, onnx_parity as OP
    from zero.infer import drive
    from zero.onnx_eval import OnnxEvaluator
    cfg, cp = _smoke_cfg(tmp_path)
    rec = tmp_path / 'batches.npz'
    p = subprocess.run([WORKER, 'selfplay', '--settings', str(cp), '--uniform', '--games', '2', '--threads', '1', '--out',
                        str(tmp_path / 'u'), '--record-batches', '100', '--record-file', str(rec)],
                       capture_output=True, text=True, encoding='utf-8', timeout=1800)
    assert p.returncode == 0, p.stderr[-2000:]
    models = {}
    for gen, seed in ((1, 3), (2, 4)):
        torch.manual_seed(seed)
        m = N.make_net(G.info(cfg['game']), cfg['game'], arch='tokens', hidden=32, blocks=1, heads=2)
        for q in m.parameters():
            q.data.add_(0.2 * torch.randn_like(q))
        info = {'checkpoint': f'test/{gen}', 'checkpoint_sha256': f'{gen}f' * 32, 'checkpoint_iter': str(gen)}
        path = str(tmp_path / f'{gen}.onnx')
        OX.export(m, path, 'packed', info)
        ref = str(tmp_path / f'{gen}.ref.npz')
        OX.write_reference(m, [OP.Rows.load(str(rec))], ref, 48, info)
        models[gen] = {'model': path, 'reference': ref, 'sha256': hashlib.sha256(open(path, 'rb').read()).hexdigest()}
    rt = os.path.join(os.path.dirname(onnxruntime.__file__), 'capi')
    seats = [{'net': 1}, {'net': 2}]

    def job(name, games, gid_base, seats_):
        d = tmp_path / name
        d.mkdir()
        w = Serve(WORKER, d)
        w.wait_for('hello', 60)
        w.send({'op': 'job', 'lease_id': 'l_' + name, 'kind': 'eval', 'settings': cfg, 'seats': seats_, 'paired': True,
                'games': games, 'gid_base': gid_base, 'seed': 11, 'models': {str(g): x for g, x in models.items()},
                'model': None, 'reference': None, 'threads': 1, 'infer_threads': 1, 'out_dir': 'out', 'runtime_dir': rt})
        end = w.wait_for('end', 1800)
        assert w.finish() == 0 and end['reason'] == 'done' and not end['unfinished']['gids'], end
        rows = {}
        for e in w.events:
            if e['ev'] == 'evals':
                text = (d / 'out' / e['path']).read_text(encoding='utf-8')
                for ln in text.splitlines():
                    rows[json.loads(ln)['gid']] = ln
        return w.events, rows

    ev, rows = job('all', 4, 0, seats)
    tests = [e for e in ev if e['ev'] == 'selftest']
    assert sorted(t['gen'] for t in tests) == [1, 2] and all(t['ok'] for t in tests)
    assert sorted(rows) == [0, 1, 2, 3]
    # the Python path, the same graphs (one intra-op thread on both sides)
    evs = {k: OnnxEvaluator(models[s['net']]['model'], threads=1) for k, s in zip(('m', 'o'), seats)}
    drv = FE.match_driver(FE.job_cfg(cfg, threads=1), seats, 4, 11, paired=True)
    drive(drv, evs)
    want = {r['gid']: FE.row_json(FE.row(r, seats, True)) for r in drv.take_results()}
    assert rows == want
    # a lease of the second pair: those games
    _, tail = job('tail', 2, 2, seats)
    assert tail == {g: want[g] for g in (2, 3)}
    # a network against the heuristic bot
    _, bot = job('bot', 2, 0, [{'net': 2}, {'bot': 'heuristic'}])
    assert json.loads(bot[1])['seats'] == [{'bot': 'heuristic'}, {'net': 2}]
