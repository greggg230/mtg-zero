"""One command for a published farm build of a commit (docs/ENGINE.md 196): build, sign, the CUDA alternate, the GPU
self-test, the farm gates on a reference run, publish, and the index launchers look builds up in.

    python tools/farm_release.py release <commit | worktree> [--force] [--regate] [--no-publish] [--no-slot]
    python tools/farm_release.py lookup <commit> [--json | --pool-args]
    python tools/farm_release.py list [N]
    python tools/farm_release.py reference --config <run>/config.json --checkpoint <run>/models/iter_N.pt
                                          --export <dir with model.onnx + reference.npz of that checkpoint>
    python tools/farm_release.py config                    # the settings in force (release.json + defaults)

`release` runs unattended and stops at the first failing step with "farm_release: FAILED at <step>: ..." (exit 1):

1. **reuse**: the commit's farm worker inputs (`mtgz.az.farm_builds.rust_key`: every Rust path's git tree, the
   zero-core pin, the toolchain, the farm build's flags - tools/zcache.py's rule, read from the commit) equal a
   published entry's -> that build is the commit's too: an index entry `reused_from` it, nothing built, signed or
   published (its gates stand; `--regate` runs the farm gates again on this commit's Python). A commit already in
   the index is done (`--force`: release it again).
2. **checkout**: the release worktree (`<the main checkout's sibling mtg-zero-wt>/farm-release`, its own target dir
   `<ZERO_BIG_DISK>/cargo-target/mtg-zero/farm-release`; created once, reused) at the commit: forced, untracked files
   removed, the zero-core submodule at the pin and clean. A worktree argument must be clean; its HEAD is released.
3. **modules**: mtgz/_engine and zero/_engine there (the artifact cache, else built) for the gates' Python side.
4. **build**: this checkout's tools/farm_build.py `--root <release worktree> --profile release --runtime-id <CPU pack>`
   (signed, not published) -> the build id.
5. **cuda**: `farm_build.py --resign <that build> --runtime-id <CUDA pack> --min-agent <cuda_min_agent>` (the CUDA
   alternate: the same executable signed for the CUDA pack, zero-farm PROTOCOL 9.1) -> its build id.
6. **cuda-selftest**: the CUDA alternate's `probe-providers --runtime-dir <CUDA pack> --model/--reference <the
   reference export> --providers cuda,cpu --reps 2`: both providers must pass their self-test (seconds of GPU; the
   CUDA provider's max_abs ~1e-6 on real exports). `--no-cuda` skips 5-6 (no alternate).
7. **template**: zero.farm_pool.play_settings with the signed worker on the reference config (the farm pool's own path
   to a self-play template: the worker's check-config, with the declared setting classes when the build announces
   them, zero-core docs/az.md 61) - the refusal that once forced a rebuild shows here.
8. **gates**: the commit's tools/farm_gates.py on the reference config and checkpoint with the signed worker and the
   CPU pack (gate 2: the worker vs the Python remote worker, gate 3: records replayed), CPU only.
9. **publish**: both build directories (`farm_build.py --publish-dir`), then copied to `<builds>/<build_id>/` (the
   desktop's copies of registered builds: what `pool_worker` names).
10. **index**: the entry appended to `<builds>/index.json` (mtgz.az.farm_builds): commit, rust_key, build and CUDA
   ids, runtimes, workers, the gate lines, each step's seconds. The full result also in
   `<builds>/releases/<sha12>.json` (a failure too, with its step).

Publishing makes a build AVAILABLE: no run is attached or changed. Heavy steps (build, modules, gates) each hold a slot
of tools/slotd.sh (`--no-slot`: not), and the whole release runs below normal priority. One release at a time on the
machine (`<builds>/.release.lock`).

Settings: `<builds>/release.json` (machine-local, untracked; `config` prints what is in force): cpu_runtime,
cuda_runtime, cuda_min_agent, python (the gates' interpreter: torch + onnx + onnxruntime; default this one), url,
reference {config, checkpoint, export} (default `<builds>/reference/`, written by `reference`). A runtime pack's
directory is `<builds>/<runtime id>/`. `<builds>` = mtgz.az.farm_builds.builds_dir() ($MTGZ_FARM_BUILDS, else
$ZERO_BIG_DISK/zero-farm-builds).

`lookup <commit>` prints the build for a commit (its own release, else one of the same Rust inputs; exit 1 when none);
`--pool-args` as the farm pool's run flags (`--pool-build ... --pool-runtime ... --pool-worker ... --pool-alternates
...`), `--json` the entry. In a run's command line `--pool-build published` does the same (mtgz.az.run), and an A/B
spec's `"pool_build": "published"` (tools/ab.py).
"""
import argparse
import datetime
import json
import os
import shutil
import subprocess
import sys
import time

TOOLS = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(TOOLS)
sys.path.insert(0, ROOT)
sys.path.insert(0, os.path.join(ROOT, 'third_party', 'zero-core', 'python'))
sys.path.insert(0, TOOLS)
import local_config  # noqa: E402

DEFAULTS = {
    'cpu_runtime': 'd531424bd1a16307d491418c',     # ort-1.31.1-cpu+msvc (ENGINE.md 114.10)
    'cuda_runtime': '855d21629a0246f0f91e6790',    # cuda-12.8-cudnn-9.28-ort-1.24.4 (farm-cuda, PROTOCOL 9)
    'cuda_min_agent': '0.2.11',
    'python': None,
    'url': 'https://farm.greggg230.com',
    'reference': None,
}
SLOT_WAIT = 4 * 3600


class Failed(Exception):
    def __init__(self, step, msg):
        super().__init__(f'{step}: {msg}')
        self.step = step


def fb():
    from mtgz.az import farm_builds
    return farm_builds


def builds_dir():
    d = fb().builds_dir()
    if not d:
        raise SystemExit('farm_release: no builds directory ($MTGZ_FARM_BUILDS, or ZERO_BIG_DISK in ~/.config/zero/local.env)')
    return d


def settings():
    d = builds_dir()
    s = dict(DEFAULTS)
    p = os.path.join(d, 'release.json')
    if os.path.exists(p):
        with open(p, encoding='utf-8') as f:
            s.update(json.load(f))
    ref = s.get('reference') or {}
    rd = os.path.join(d, 'reference')
    s['reference'] = {'config': ref.get('config') or os.path.join(rd, 'config.json'),
                      'checkpoint': ref.get('checkpoint') or os.path.join(rd, 'checkpoint.pt'),
                      'export': ref.get('export') or os.path.join(rd, 'export')}
    s['python'] = s.get('python') or sys.executable
    return s


def git(*a, cwd=None, check=True):
    p = subprocess.run(['git', *a], cwd=cwd or ROOT, capture_output=True, text=True)
    if check and p.returncode:
        raise RuntimeError(f'git {" ".join(a)} ({cwd}): {p.stderr.strip()[:400]}')
    return p.stdout.strip()


def main_checkout():
    return os.path.dirname(os.path.normpath(git('rev-parse', '--path-format=absolute', '--git-common-dir')))


def release_worktree():
    wt = os.environ.get('MTGZ_RELEASE_WORKTREE') or os.path.join(os.path.dirname(main_checkout()), 'mtg-zero-wt', 'farm-release')
    big = local_config.big_disk_path('cargo-target', 'mtg-zero', 'farm-release')
    return wt, os.environ.get('MTGZ_RELEASE_TARGET') or big or os.path.join(wt, 'target')


def below_normal():
    if os.name == 'nt':
        import ctypes
        k = ctypes.windll.kernel32
        k.SetPriorityClass(k.GetCurrentProcess(), 0x00004000)     # BELOW_NORMAL_PRIORITY_CLASS (children inherit it)
    else:
        try:
            os.nice(10)
        except OSError:
            pass


def now():
    return datetime.datetime.now(datetime.timezone.utc).astimezone().isoformat(timespec='seconds')


class Release:
    def __init__(self, a, s):
        self.a, self.s = a, s
        self.builds = builds_dir()
        self.result = {'commit': None, 'started': now(), 'steps': {}, 'seconds': {}}
        self.log_dir = None

    def say(self, m):
        print(f'[farm_release {time.strftime("%H:%M:%S")}] {m}', flush=True)

    def run(self, step, cmd, cwd=ROOT, env=None, slot=False, log=None, timeout=None):
        """A step's command; its output to <log dir>/<log>.log; non-zero -> Failed(step) with the tail."""
        if slot and not self.a.no_slot:
            cmd = ['bash', os.path.join(TOOLS, 'slotd.sh')] + list(cmd)
            env = dict(env or os.environ, MTGZ_SLOT_WAIT_MAX=str(SLOT_WAIT))
        path = os.path.join(self.log_dir, f'{log or step}.log')
        self.say(f'{step}: {subprocess.list2cmdline(cmd)} (log {path})')
        with open(path, 'a', encoding='utf-8') as f:
            p = subprocess.run(cmd, cwd=cwd, env=env, stdout=f, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL,
                               timeout=timeout)
        out = open(path, encoding='utf-8', errors='replace').read()
        if p.returncode:
            raise Failed(step, f'exit {p.returncode}; the end of {path}:\n{out[-3000:]}')
        return out

    def timed(self, step, f, *args):
        t0 = time.time()
        r = f(*args)
        self.result['seconds'][step] = round(time.time() - t0, 1)
        self.say(f'{step}: done in {self.result["seconds"][step]:.0f} s')
        return r

    # ------------------------------------------------------------------ steps
    def checkout(self, sha):
        wt, target = release_worktree()
        main = main_checkout()
        if not os.path.exists(os.path.join(wt, '.git')):
            git('worktree', 'add', '--detach', wt, sha, cwd=main)
        else:
            git('checkout', '--detach', '--force', sha, cwd=wt)
            git('clean', '-fdq', cwd=wt)
        git('-c', 'protocol.file.allow=always', 'submodule', 'update', '--init', '--force', 'third_party/zero-core', cwd=wt)
        git('clean', '-fdq', cwd=os.path.join(wt, 'third_party', 'zero-core'))
        head = git('rev-parse', 'HEAD', cwd=wt)
        dirty = git('status', '--porcelain', '--untracked-files=no', cwd=wt)
        if head != sha or dirty:
            raise Failed('checkout', f'{wt} is not exactly {sha[:12]} (HEAD {head[:12]}; {dirty[:300]})')
        self.wt, self.target = wt, target
        return wt

    def modules(self):
        """mtgz/_engine and zero/_engine of the commit, for the gates' Python (the artifact cache, else built)."""
        py = self.s['python']
        env = dict(os.environ, CARGO_TARGET_DIR=self.target, PYTHON=py)
        pyd = os.path.join('mtgz', '_engine.pyd' if os.name == 'nt' else '_engine.so')
        p = subprocess.run([py, 'tools/zcache.py', 'fetch', 'pyd', '--dest', pyd], cwd=self.wt, env=env, capture_output=True, text=True)
        self.say(f'modules: mtgz pyd {"from the cache" if p.returncode == 0 else "to build"}: {(p.stdout + p.stderr).strip()[-200:]}')
        if p.returncode != 0:
            self.run('modules', ['bash', 'tools/build_py.sh'], cwd=self.wt, env=env, slot=True, log='modules-build_py')
        self.run('modules', [py, 'tools/zcache.py', 'core-pyd'], cwd=self.wt, env=env, slot=True, log='modules-core_pyd')

    def farm_build(self, step, *args):
        env = dict(os.environ, CARGO_TARGET_DIR=self.target)
        env.pop('ZERO_PROBE_VARIANTS', None)
        out = self.run(step, [sys.executable, os.path.join(TOOLS, 'farm_build.py'), *args, '--url', self.s['url'],
                              '--out', self.dist], env=env, slot=step == 'build')
        bid = [ln.split()[1] for ln in out.splitlines() if ln.startswith('build_id ')]
        d = [ln[len('build directory '):].strip() for ln in out.splitlines() if ln.startswith('build directory ')]
        if not bid or not d:
            raise Failed(step, f'no build_id / build directory in its output:\n{out[-1500:]}')
        return bid[-1], d[-1]

    def runtime_dir(self, rid, step):
        d = os.path.join(self.builds, rid)
        if not os.path.exists(os.path.join(d, 'manifest.json')):
            raise Failed(step, f'runtime pack {rid}: no {d} (copy the signed pack directory there)')
        return d

    def cuda_selftest(self, cuda_dir):
        ref = self.s['reference']['export']
        rt = self.runtime_dir(self.s['cuda_runtime'], 'cuda-selftest')
        exe = os.path.join(cuda_dir, 'mtgz-worker.exe')
        env = dict(os.environ, PATH=rt + os.pathsep + os.environ.get('PATH', ''))   # cuDNN loads its parts by name
        cmd = [exe, 'probe-providers', '--runtime-dir', rt, '--model', os.path.join(ref, 'model.onnx'), '--reference',
               os.path.join(ref, 'reference.npz'), '--providers', 'cuda,cpu', '--reps', '2']
        self.say(f'cuda-selftest: {subprocess.list2cmdline(cmd)}')
        p = subprocess.run(cmd, capture_output=True, text=True, env=env, stdin=subprocess.DEVNULL, timeout=600)
        with open(os.path.join(self.log_dir, 'cuda-selftest.log'), 'w', encoding='utf-8') as f:
            f.write(p.stdout + '\n' + p.stderr)
        try:
            probe = json.loads(p.stdout)
        except ValueError:
            raise Failed('cuda-selftest', f'exit {p.returncode}: {p.stderr[-1500:]}') from None
        res = {r['provider']: r for r in probe.get('results') or []}
        if p.returncode or probe.get('providers') != ['cuda', 'cpu'] or not all(res.get(x, {}).get('ok') for x in ('cuda', 'cpu')):
            raise Failed('cuda-selftest', f'providers {probe.get("providers")} (want cuda and cpu passing): '
                         f'{ {k: (v.get("ok"), v.get("why")) for k, v in res.items()} } {p.stderr[-800:]}')
        out = {x: {'max_abs': res[x]['selftest']['max_abs'], 'rows_per_s': res[x].get('rows_per_s')} for x in ('cuda', 'cpu')}
        out['gpu'] = (res['cuda'].get('gpu') or {}).get('vendor')
        return out

    def template(self, exe, bid):
        """zero.farm_pool.play_settings with the signed worker on the reference config (step 7)."""
        out = os.path.join(self.log_dir, 'template')
        code = ('import json, sys; sys.path.insert(0, "."); import mtgz.az; from zero import farm_pool as FP, farm_h2h as FH\n'
                'cfg = json.load(open(sys.argv[1], encoding="utf-8")); lines = []\n'
                'play = FP.play_settings(sys.argv[2], sys.argv[3], cfg, sys.argv[4], lines.append)\n'
                'K = getattr(FH, "CLASSES_KEY", "setting_classes"); d = play.get(K)\n'
                'print(json.dumps({"play": len([k for k in play if k != K]), "settings": len(cfg), '
                '"declared": None if d is None else len(d), "notes": lines}))\n')
        txt = self.run('template', [self.s['python'], '-c', code, self.s['reference']['config'], exe, bid, out], cwd=self.wt)
        return json.loads([ln for ln in txt.splitlines() if ln.startswith('{')][-1])

    def gates(self, exe):
        ref = self.s['reference']
        out = os.path.join(self.log_dir, 'gates')
        env = dict(os.environ, CUDA_VISIBLE_DEVICES='')           # CPU only (the desktop's GPU trains)
        self.run('gates', [self.s['python'], 'tools/farm_gates.py', '--config', ref['config'], '--checkpoint', ref['checkpoint'],
                           '--worker', exe, '--runtime-dir', self.runtime_dir(self.s['cpu_runtime'], 'gates'), '--out', out],
                 cwd=self.wt, env=env, slot=True)
        with open(os.path.join(out, 'summary.json'), encoding='utf-8') as f:
            sm = json.load(f)
        if not sm.get('passed'):
            raise Failed('gates', f'FARM GATES FAILED: {json.dumps(sm)[:1500]}')
        g2, g3 = (sm['gate2'].get('result') or {}), (sm['gate3'].get('result') or {})
        line = (f'FARM GATES PASSED: gate 2 {g2.get("matches", g2.get("games"))} matches, {g2.get("rows")} rows, '
                f'{g2.get("batches")} batches identical ({sm["gate2"]["seconds"]:.0f} s); gate 3 {g3.get("matches", g3.get("games"))} matches, '
                f'{g3.get("rows")} rows identical ({sm["gate3"]["seconds"]:.0f} s)')
        return {'passed': True, 'line': line, 'gate2': {k: g2.get(k) for k in ('matches', 'games', 'rows', 'batches', 'identical')},
                'gate3': {k: g3.get(k) for k in ('matches', 'games', 'rows', 'identical', 'arrays')}, 'summary': os.path.join(out, 'summary.json')}

    def publish(self, d):
        """Publish a signed build directory, then copy it to <builds>/<build_id>/ -> that copy."""
        bid, _ = self.farm_build('publish', '--publish-dir', d)
        dest = os.path.join(self.builds, bid)
        if not os.path.exists(dest):
            shutil.copytree(d, dest + '.part')
            os.replace(dest + '.part', dest)
        return dest

    # ------------------------------------------------------------------ the release
    def go(self, target):
        FB = fb()
        if os.path.isdir(target):
            if git('status', '--porcelain', '--untracked-files=no', cwd=target):
                raise Failed('resolve', f'{target} has uncommitted changes: commit first (a build names its commit)')
            sha = git('rev-parse', 'HEAD', cwd=target)
        else:
            sha = FB.resolve(target, ROOT)
        r = self.result
        r['commit'] = sha
        self.log_dir = os.path.join(self.builds, 'releases', sha[:12])
        os.makedirs(self.log_dir, exist_ok=True)
        key = FB.rust_key(sha, ROOT)
        r['rust_key'] = key
        r['core_commit'] = git('rev-parse', f'{sha}:third_party/zero-core')
        ix = FB.load()
        rt = self.s['cpu_runtime']
        have = FB.find(ix, commit=sha, runtime=rt)
        if have and not self.a.force:
            self.say(f'{sha[:12]} is released already: {FB.describe(have)}')
            return dict(have, already=True)
        same = FB.find(ix, key=key, runtime=rt)
        if same and not self.a.force and (same.get('cuda') or {}).get('runtime') in (None, self.s['cuda_runtime']) \
                and bool(same.get('cuda')) != self.a.no_cuda:
            self.say(f'{sha[:12]} changed no Rust input since {same["commit"][:12]}: its build {same["build_id"]} is this commit\'s')
            e = {k: v for k, v in same.items() if k not in ('exact', 'asked')}
            e.update(commit=sha, reused_from=same.get('reused_from') or same['commit'], created=now(), seconds={})
            if self.a.regate:
                preflight(self.s, cuda=False)
                self.timed('checkout', self.checkout, sha)
                self.timed('modules', self.modules)
                e['template'] = self.timed('template', self.template, e['worker'], e['build_id'])
                e['gates'] = self.timed('gates', self.gates, e['worker'])
                e['seconds'] = r['seconds']
            return FB.add(e)
        preflight(self.s, cuda=not self.a.no_cuda)
        self.dist = os.path.join(self.target_root(), 'farm-build', 'dist')
        self.timed('checkout', self.checkout, sha)
        self.timed('modules', self.modules)
        bid, bdir = self.timed('build', self.farm_build, 'build', '--root', self.wt, '--profile', 'release', '--runtime-id', rt)
        r['steps']['build'] = {'build_id': bid, 'dir': bdir}
        exe = os.path.join(bdir, 'mtgz-worker.exe')
        info = json.loads(subprocess.run([exe, 'info', '--ort-dll', 'none'], capture_output=True, text=True, check=True).stdout)
        cuda = None
        if not self.a.no_cuda:
            cid, cdir = self.timed('cuda', self.farm_build, 'cuda', '--resign', bdir, '--runtime-id', self.s['cuda_runtime'],
                                   '--min-agent', self.s['cuda_min_agent'])
            r['steps']['cuda'] = {'build_id': cid, 'dir': cdir}
            st = self.timed('cuda-selftest', self.cuda_selftest, cdir)
            cuda = {'build_id': cid, 'runtime': self.s['cuda_runtime'], 'min_agent': self.s['cuda_min_agent'], 'selftest': st}
        tpl = self.timed('template', self.template, exe, bid)
        gates = self.timed('gates', self.gates, exe)
        if self.a.no_publish:
            self.say('--no-publish: signed and checked, not published (no index entry)')
            return dict(r, build_id=bid, cuda=cuda, template=tpl, gates=gates, published=False)
        worker = self.timed('publish', self.publish, bdir)
        if cuda:
            cuda['worker'] = os.path.join(self.timed('publish-cuda', self.publish, r['steps']['cuda']['dir']), 'mtgz-worker.exe')
        e = {'commit': sha, 'built_at': sha, 'rust_key': key, 'core_commit': r['core_commit'], 'build_id': bid,
             'version': os.path.basename(os.path.normpath(bdir)), 'runtime': rt, 'worker': os.path.join(worker, 'mtgz-worker.exe'),
             'cuda': cuda, 'features': info.get('features') or [], 'zero_onnx': info.get('zero_onnx'), 'template': tpl,
             'gates': gates, 'reference': {k: local_config.portable_path(v) for k, v in self.s['reference'].items()},
             'url': self.s['url'], 'published': True, 'created': now(), 'seconds': r['seconds']}
        return FB.add(e)

    def target_root(self):
        return release_worktree()[1]


def preflight(s, cuda=True):
    """What the checking steps need, before anything is built: the reference files, the gates' interpreter."""
    for k in ('config', 'checkpoint') + (('export',) if cuda else ()):
        if not os.path.exists(s['reference'][k]):
            raise Failed('preflight', f'the reference {k} {s["reference"][k]} is missing (farm_release.py reference ...)')
    p = subprocess.run([s['python'], '-c', 'import torch, onnx, onnxruntime'], capture_output=True, text=True)
    if p.returncode:
        raise Failed('preflight', f'{s["python"]} lacks torch / onnx / onnxruntime (the gates need them): set "python" in '
                                  f'release.json')


def cmd_release(a):
    s = settings()
    if not a.normal_priority:
        below_normal()
    R = Release(a, s)
    t0 = time.time()
    try:
        with fb().Lock(os.path.join(R.builds, '.release.lock'), wait=a.lock_wait):
            e = R.go(a.target)
    except Failed as x:
        R.result.update(failed=x.step, error=str(x), finished=now())
        save_result(R)
        print(f'farm_release: FAILED at {x.step}: {x}', file=sys.stderr, flush=True)
        return 1
    except Exception as x:          # (anything else is a failure too: loud, recorded)
        R.result.update(failed='error', error=f'{type(x).__name__}: {x}', finished=now())
        save_result(R)
        print(f'farm_release: FAILED: {type(x).__name__}: {x}', file=sys.stderr, flush=True)
        raise
    R.result.update(entry=e, finished=now(), total_s=round(time.time() - t0, 1))
    save_result(R)
    print(json.dumps({k: e.get(k) for k in ('commit', 'build_id', 'runtime', 'worker', 'cuda', 'reused_from', 'already',
                                            'published', 'seconds')}))
    g = (e.get('gates') or {}).get('line')
    print(f'farm_release: {fb().describe(dict(e, exact=True))}' + (f'; {g}' if g else ''), flush=True)
    return 0


def save_result(R):
    if not R.log_dir:
        return
    with open(os.path.join(R.log_dir, 'result.json'), 'w', encoding='utf-8', newline='\n') as f:
        json.dump(R.result, f, indent=1)


def cmd_lookup(a):
    FB = fb()
    e = FB.lookup(a.commit, ROOT)
    if e is None:
        print(f'no published farm build for {a.commit} ({FB.index_path()})', file=sys.stderr)
        return 1
    if a.json:
        print(json.dumps(e, indent=1))
    elif a.pool_args:
        print(' '.join(f'--{k.replace("_", "-")} {v if v else chr(39) * 2}' for k, v in FB.pool_settings(e).items()))
    else:
        print(FB.describe(e))
    return 0


def cmd_list(a):
    for e in fb().load()['builds'][-a.n:]:
        g = (e.get('gates') or {}).get('line', '')
        print(f"{e['created'][:16]} {e['commit'][:12]} build {e['build_id']} cuda {(e.get('cuda') or {}).get('build_id', '-')}"
              + (f" (reused from {e['reused_from'][:12]})" if e.get('reused_from') else '') + (f'  {g}' if g else ''))
    return 0


def cmd_reference(a):
    d = os.path.join(builds_dir(), 'reference')
    os.makedirs(os.path.join(d, 'export'), exist_ok=True)
    for src, dst in ((a.config, 'config.json'), (a.checkpoint, 'checkpoint.pt'),
                     (os.path.join(a.export, 'model.onnx'), os.path.join('export', 'model.onnx')),
                     (os.path.join(a.export, 'reference.npz'), os.path.join('export', 'reference.npz'))):
        shutil.copyfile(src, os.path.join(d, dst + '.part'))
        os.replace(os.path.join(d, dst + '.part'), os.path.join(d, dst))
    with open(os.path.join(d, 'source.json'), 'w', encoding='utf-8', newline='\n') as f:
        json.dump({'config': a.config, 'checkpoint': a.checkpoint, 'export': a.export, 'copied': now()}, f, indent=1)
    print(f'reference copied into {d}')
    return 0


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    sub = ap.add_subparsers(dest='cmd', required=True)
    r = sub.add_parser('release')
    r.add_argument('target', help='a commit (sha, branch) or a clean worktree')
    r.add_argument('--force', action='store_true', help='build and publish again even when released or reusable')
    r.add_argument('--regate', action='store_true', help='a reused build: run the template check and the farm gates on this commit')
    r.add_argument('--no-publish', action='store_true', help='sign and check only (no publish, no index entry)')
    r.add_argument('--no-cuda', action='store_true', help='no CUDA alternate (and no GPU self-test)')
    r.add_argument('--no-slot', action='store_true', help='heavy steps without tools/slotd.sh')
    r.add_argument('--normal-priority', action='store_true')
    r.add_argument('--lock-wait', type=float, default=6 * 3600)
    lk = sub.add_parser('lookup')
    lk.add_argument('commit', nargs='?', default='HEAD')
    lk.add_argument('--json', action='store_true')
    lk.add_argument('--pool-args', action='store_true')
    ls = sub.add_parser('list')
    ls.add_argument('n', nargs='?', type=int, default=20)
    rf = sub.add_parser('reference')
    rf.add_argument('--config', required=True)
    rf.add_argument('--checkpoint', required=True)
    rf.add_argument('--export', required=True, help='a directory with model.onnx and reference.npz of the checkpoint')
    sub.add_parser('config')
    a = ap.parse_args(argv)
    if a.cmd == 'config':
        print(json.dumps(settings(), indent=1))
        return 0
    return {'release': cmd_release, 'lookup': cmd_lookup, 'list': cmd_list, 'reference': cmd_reference}[a.cmd](a)


if __name__ == '__main__':
    sys.exit(main())
