"""The published farm builds of MTG Zero's commits (docs/ENGINE.md 196): the index `tools/farm_release.py` writes - one
entry per released commit: its release worker build, the CUDA alternate, the runtime packs, the gate lines, the macOS
build (`mac.release`, docs/ENGINE.md 216; a run alternate only after the run's settings pass the platform check:
`pool_settings` with the run's settings, docs/ENGINE.md 218) - and the lookups launchers use ("the build for my commit").

    from mtgz.az import farm_builds as FB
    e = FB.lookup('HEAD')                 # the entry for this checkout's commit, or None
    FB.pool_settings(e, cfg=run_cfg)      # {'pool_build', 'pool_runtime', 'pool_worker', 'pool_alternates', 'pool_variants'}

Where: `<ZERO_BIG_DISK>/zero-farm-builds/index.json` (the desktop's copies of registered builds sit beside it as
`<build_id>/`), or $MTGZ_FARM_BUILDS (a directory). A machine without either has no index (lookups answer None).

A commit's entry is its own release, else - `lookup` - the release of another commit with the same Rust inputs (the
same `rust_key`: every tracked file under the engine's Rust paths, the zero-core pin, the toolchain and the farm build's
flags; the worker executable would be byte-identical), marked `"exact": false`.

`python -m mtgz.az.run ... --pool-build published[:<rev>]` takes the ids from here (`published_pool`): the run's
settings get the build of the checkout's commit (or <rev>'s) and, unless given, its runtime pack, its signed
executable, its alternates (CUDA, DirectML, and macOS when the run's settings replay exactly there) and the GPU variants
those alternates play (docs/ENGINE.md 218).

`published_for_run` is the game package's `farm_build` hook (zero-core docs/az.md 70): a run whose self-play stays on
this machine beside a published build of its commit says why (`selfplay_local_reason`) at its first start.
"""
import hashlib
import json
import os
import subprocess
import time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SCHEMA = 1
FARM_RUSTFLAGS = '-C target-feature=+crt-static'     # tools/farm_build.py's cargo_build
# the engine's Rust inputs (mtgz.az.ENGINE_PATHS = tools/zcache.py's RUST_PATHS)
RUST_PATHS = ('engine', 'card_ir', 'py', 'worker', 'data', 'Cargo.toml', 'Cargo.lock', '.cargo', 'rust-toolchain',
              'rust-toolchain.toml')


def builds_dir():
    """The directory of the desktop's copies of registered builds and the index, or None."""
    d = os.environ.get('MTGZ_FARM_BUILDS')
    if d:
        return d
    from zero import local_config
    return local_config.big_disk_path('zero-farm-builds')


def index_path():
    d = builds_dir()
    return os.path.join(d, 'index.json') if d else None


def git(root, *args):
    p = subprocess.run(['git', *args], cwd=root, capture_output=True, text=True)
    if p.returncode:
        raise ValueError(f'git {" ".join(args)} ({root}): {p.stderr.strip()[:300]}')
    return p.stdout.strip()


def resolve(rev, root=None):
    """A revision (a sha, a branch, HEAD) of `root`'s repository -> its full commit sha."""
    return git(root or ROOT, 'rev-parse', '--verify', f'{rev}^{{commit}}')


_TOOLCHAIN = {}


def toolchain(root):
    if root not in _TOOLCHAIN:
        p = subprocess.run(['rustc', '-vV'], cwd=root, capture_output=True, text=True)
        if p.returncode:
            raise ValueError(f'rustc -vV: {p.stderr.strip()[:200]}')
        _TOOLCHAIN[root] = p.stdout.strip()
    return _TOOLCHAIN[root]


def rust_key(sha, root=None):
    """The farm worker's inputs at commit `sha` (no checkout needed): the git tree of every Rust path, the zero-core
    pin, the toolchain, the farm build's profile and flags -> sha256 hex. Two commits with one key build one executable
    (tools/zcache.py's rule for its `worker` kind, read from the commit instead of a checkout)."""
    root = root or ROOT
    tree = git(root, 'ls-tree', '-r', '--full-tree', sha, '--', *RUST_PATHS)
    inp = {'schema': SCHEMA, 'kind': 'farm-worker', 'profile': 'release', 'rustflags': FARM_RUSTFLAGS,
           'tree': hashlib.sha256(tree.encode()).hexdigest(), 'zero_core': git(root, 'rev-parse', f'{sha}:third_party/zero-core'),
           'toolchain': toolchain(root)}
    return hashlib.sha256(json.dumps(inp, sort_keys=True).encode()).hexdigest()


def load(path=None):
    """The index ({"schema", "builds": [entry, ...]}, oldest first); an empty one when there is none."""
    path = path or index_path()
    if not path or not os.path.exists(path):
        return {'schema': SCHEMA, 'builds': []}
    with open(path, encoding='utf-8') as f:
        ix = json.load(f)
    if ix.get('schema') != SCHEMA or not isinstance(ix.get('builds'), list):
        raise ValueError(f'{path}: not a farm builds index of schema {SCHEMA}')
    return ix


class Lock:
    """An exclusive lock file (`<path>`: the holder's pid); a holder that is gone is taken over."""

    def __init__(self, path, wait=600.0):
        self.path, self.wait = path, wait

    def __enter__(self):
        t0 = time.time()
        while True:
            try:
                fd = os.open(self.path, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
                os.write(fd, str(os.getpid()).encode())
                os.close(fd)
                return self
            except FileExistsError:
                if not _alive(_read_pid(self.path)):
                    try:
                        os.remove(self.path)
                    except OSError:
                        pass
                    continue
                if time.time() - t0 > self.wait:
                    raise TimeoutError(f'{self.path} is held by pid {_read_pid(self.path)}')
                time.sleep(0.5)

    def __exit__(self, *exc):
        try:
            if _read_pid(self.path) == os.getpid():
                os.remove(self.path)
        except OSError:
            pass


def _read_pid(path):
    try:
        with open(path, encoding='utf-8') as f:
            return int(f.read().strip() or 0)
    except (OSError, ValueError):
        return 0


def _alive(pid):
    if not pid:
        return False
    try:
        import psutil
        return psutil.pid_exists(pid)
    except ImportError:
        try:
            os.kill(pid, 0)
            return True
        except OSError:
            return False


def _index_for_write(path):
    path = path or index_path()
    if not path:
        raise ValueError('no farm builds directory (ZERO_BIG_DISK in ~/.config/zero/local.env, or MTGZ_FARM_BUILDS)')
    os.makedirs(os.path.dirname(path), exist_ok=True)
    return path


def _write(ix, path):
    tmp = f'{path}.{os.getpid()}.tmp'
    with open(tmp, 'w', encoding='utf-8', newline='\n') as f:
        json.dump(ix, f, indent=1)
        f.write('\n')
    os.replace(tmp, path)


def add(entry, path=None):
    """Append `entry` to the index (under its lock; written to a temp file and renamed into place)."""
    path = _index_for_write(path)
    with Lock(path + '.lock'):
        ix = load(path)
        ix['builds'].append(entry)
        _write(ix, path)
    return entry


def update(build_id, fields, only=None, path=None):
    """Set `fields` on every published entry of worker build `build_id` (one executable: the release of the commit that
    built it and the entries of the commits that reused it) for which `only(entry)` holds (every one when None) - in
    place, under the index's lock -> how many entries changed. tools/farm_release.py records a release's macOS part
    this way when it comes after the entry (docs/ENGINE.md 216)."""
    path = _index_for_write(path)
    with Lock(path + '.lock'):
        ix = load(path)
        hit = [e for e in ix['builds'] if e.get('published') and e.get('build_id') == build_id and (only is None or only(e))]
        for e in hit:
            e.update(fields)
        if hit:
            _write(ix, path)
    return len(hit)


def mac_build(e):
    """An entry's published macOS release build id (its `mac.release`, zero-farm PROTOCOL 11), else None."""
    rel = ((e or {}).get('mac') or {}).get('release') or {}
    return rel.get('build_id') if rel.get('build_id') and rel.get('published') else None


def find(ix, commit=None, key=None, runtime=None):
    """The newest published entry of `commit` (exact), else of `key` (the same Rust inputs); `runtime`: only entries for
    that CPU runtime pack."""
    ok = [e for e in ix['builds'] if e.get('published') and (runtime is None or e.get('runtime') == runtime)]
    if commit:
        hit = [e for e in ok if e.get('commit') == commit]
        if hit:
            return dict(hit[-1], exact=True)
    if key:
        hit = [e for e in ok if e.get('rust_key') == key]
        if hit:
            return dict(hit[-1], exact=False)
    return None


def lookup(rev='HEAD', root=None, path=None, runtime=None):
    """The published build for revision `rev` of `root`'s repository (module docs) -> its entry (with `exact` and
    `asked`: the full sha looked up), or None."""
    root = root or ROOT
    sha = resolve(rev, root)
    ix = load(path)
    e = find(ix, commit=sha, runtime=runtime)
    if e is None and ix['builds']:
        e = find(ix, key=rust_key(sha, root), runtime=runtime)
    return None if e is None else dict(e, asked=sha)


# the GPU variant each alternate of a release plays (zero-core docs/az.md 67, 46.10; docs/ENGINE.md 211, 213.2): the
# CUDA alternate the gen's graph in float16, AMD's DirectML alternate the fast-matmul graph in float32 - what
# legacy-meta3-r1 runs with and what match campaigns name by default (farm_campaign.submit, ENGINE.md 215)
VARIANT_OF = {'cuda': 'gpu=fp16', 'dml': 'dml=matmul@amd'}
MAC_PLATFORM = 'macos-arm64'


def mac_check(e, cfg):
    """May a run with settings `cfg` list entry `e`'s published macOS build as an alternate? -> (True, None) or (False,
    why). The farm pool's own platform check (zero.farm_pool.platform_check: the release worker's `check-config
    --platform macos-arm64`, the game's libm rule, zero-core docs/az.md 62) - a run whose settings it refuses would have
    its pool stopped (docs/ENGINE.md 218), so such a run gets no macOS alternate."""
    mac = mac_build(e)
    if not mac:
        return False, ('the release has no published macOS build' + (describe_mac(e) and f' ({describe_mac(e)[2:]})'))
    if cfg is None:
        return False, f'macOS build {mac}: no run settings to check for macOS (the platform check needs them)'
    w = e.get('worker')
    if not w or not os.path.isfile(w):
        return False, f'macOS build {mac}: the release worker {w} that judges the settings is not on this machine'
    import shutil
    import tempfile
    from zero import farm_pool as FP
    d = tempfile.mkdtemp(prefix='mtgz-mac-check-')
    try:
        FP.platform_check(w, dict(cfg), d, MAC_PLATFORM, log=lambda m: None)
    except SystemExit as x:                    # (farm_pool.Refused: a libm refusal, a worker too old to judge)
        return False, f'macOS build {mac}: {getattr(x, "reason", None) or x}'
    finally:
        shutil.rmtree(d, ignore_errors=True)
    return True, None


def pool_settings(e, alternates=True, cfg=None, log=None):
    """A build entry -> the farm pool's run settings (zero-core docs/az.md 45): pool_build, pool_runtime, pool_worker
    (the desktop's copy of the signed executable) and, with `alternates`, pool_alternates - the CUDA alternate, the
    DirectML one for AMD GPUs (zero-farm PROTOCOL rev 22), then the published macOS build when the run's settings `cfg`
    pass the platform check (`mac_check`; '' when there is none) - and pool_variants naming the variant each GPU
    alternate plays (`VARIANT_OF`: 'gpu=fp16' with a CUDA alternate, 'dml=matmul@amd' with a DirectML one; left out
    with neither): without 'dml=matmul@amd' an AMD PC runs the gen's default graph, which DirectML refuses, on its CPU.
    A route left out is said through `log` (docs/ENGINE.md 218)."""
    say = log or (lambda m: None)
    out = {'pool_build': e['build_id'], 'pool_runtime': e['runtime'], 'pool_worker': e['worker']}
    if alternates:
        alts, variants = [], []
        for k in ('cuda', 'dml'):
            b = (e.get(k) or {}).get('build_id')
            if b:
                alts.append(b)
                variants.append(VARIANT_OF[k])
        ok, why = mac_check(e, cfg)
        if ok:
            alts.append(mac_build(e))
        else:
            say(f'farm build: no macOS alternate - {why}')
        out['pool_alternates'] = ','.join(alts)
        if variants:
            out['pool_variants'] = ','.join(variants)
    return out


def run_settings(argv):
    """A run's settings as `python -m mtgz.az.run argv` will resolve them (zero.run: DEFAULTS < preset < its
    config.json < the flags), for the platform check of `pool_settings`; None (said) when they cannot be resolved (the
    run then fails on its own at its start)."""
    try:
        from zero import run as R
        a = R.parse(argv)
        saved = {}
        p = os.path.join(a.run, 'config.json')
        if os.path.exists(p):
            with open(p, encoding='utf-8') as f:
                saved = json.load(f)
        return R.resolve_config(a, saved)
    except (SystemExit, ValueError, OSError) as x:
        print(f'farm build: the run\'s settings could not be resolved here ({x}): no macOS alternate', flush=True)
        return None


def published_for_run(run, cfg):
    """The game package's `farm_build` hook (zero-core docs/az.md 70): the published farm build of this checkout's
    commit (`lookup('HEAD')`), described for the run's log with how to use it, or None (none, or no index here)."""
    e = lookup('HEAD')
    if e is None:
        return None
    return f'{describe(e)}; use it with --selfplay-remote pool:<farm run> --pool-build published'


def describe_mac(e):
    """The entry's macOS part in a few words ('' when the entry has none and no failure)."""
    m = e.get('mac') or {}
    rel = m.get('release') or {}
    if rel.get('build_id'):
        return f", macOS build {rel['build_id']}" + ('' if rel.get('published') else ' (signed, not published)')
    if m.get('failed'):
        return f", NO macOS build (failed at {m['failed']})"
    return ''


def describe(e):
    c = e.get('cuda') or {}
    return (f"commit {e.get('asked', e['commit'])[:12]}: build {e['build_id']} (runtime {e['runtime']}"
            + (f", CUDA alternate {c['build_id']} for {c['runtime']}" if c.get('build_id') else ', no CUDA alternate')
            + (f", DirectML alternate {(e.get('dml') or {})['build_id']}" if (e.get('dml') or {}).get('build_id') else '')
            + describe_mac(e)
            + (')' if e.get('exact', True) else f"; released for {e['commit'][:12]}, the same Rust inputs)"))


def _flag(argv, name):
    """(index, value) of `--name V` / `--name=V` in argv, else (None, None)."""
    for i, a in enumerate(argv):
        if a == name and i + 1 < len(argv):
            return i, argv[i + 1]
        if a.startswith(name + '='):
            return i, a.split('=', 1)[1]
    return None, None


def published_pool(argv, root=None, path=None):
    """A run's command line with `--pool-build published` (or `published:<rev>`) replaced by the published build of the
    checkout's commit (or <rev>'s) and its --pool-runtime, --pool-worker, --pool-alternates and --pool-variants added
    unless given (`pool_settings` with the run's settings: `run_settings`; `python -m mtgz.az.run` does it: the ids are
    saved in config.json like any flag). SystemExit when there is none."""
    i, v = _flag(argv, '--pool-build')
    if v is None or not (v == 'published' or v.startswith('published:')):
        return argv
    rev = v.split(':', 1)[1] if ':' in v else 'HEAD'
    e = lookup(rev, root, path)
    if e is None:
        raise SystemExit(f'--pool-build {v}: no published farm build for {rev} ({index_path()}; '
                         f'python tools/farm_release.py release {rev})')
    out = list(argv)
    if out[i] == '--pool-build':
        out[i + 1] = e['build_id']
    else:
        out[i] = f'--pool-build={e["build_id"]}'
    for k, val in pool_settings(e, cfg=run_settings(out) if mac_build(e) else None, log=print).items():
        flag = '--' + k.replace('_', '-')
        if k != 'pool_build' and _flag(out, flag)[1] is None:
            out += [flag, val]
    print(f'farm build: {describe(e)}', flush=True)
    return out
