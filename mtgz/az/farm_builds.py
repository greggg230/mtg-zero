"""The published farm builds of MTG Zero's commits (docs/ENGINE.md 196): the index `tools/farm_release.py` writes - one
entry per released commit: its release worker build, the CUDA alternate, the runtime packs, the gate lines - and the
lookups launchers use ("the build for my commit").

    from mtgz.az import farm_builds as FB
    e = FB.lookup('HEAD')                 # the entry for this checkout's commit, or None
    FB.pool_settings(e)                   # {'pool_build', 'pool_runtime', 'pool_worker', 'pool_alternates'}

Where: `<ZERO_BIG_DISK>/zero-farm-builds/index.json` (the desktop's copies of registered builds sit beside it as
`<build_id>/`), or $MTGZ_FARM_BUILDS (a directory). A machine without either has no index (lookups answer None).

A commit's entry is its own release, else - `lookup` - the release of another commit with the same Rust inputs (the
same `rust_key`: every tracked file under the engine's Rust paths, the zero-core pin, the toolchain and the farm build's
flags; the worker executable would be byte-identical), marked `"exact": false`.

`python -m mtgz.az.run ... --pool-build published[:<rev>]` takes the ids from here (`published_pool`): the run's
settings get the build of the checkout's commit (or <rev>'s) and, unless given, its runtime pack, its signed
executable and its CUDA alternate.
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


def add(entry, path=None):
    """Append `entry` to the index (under its lock; written to a temp file and renamed into place)."""
    path = path or index_path()
    if not path:
        raise ValueError('no farm builds directory (ZERO_BIG_DISK in ~/.config/zero/local.env, or MTGZ_FARM_BUILDS)')
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with Lock(path + '.lock'):
        ix = load(path)
        ix['builds'].append(entry)
        tmp = f'{path}.{os.getpid()}.tmp'
        with open(tmp, 'w', encoding='utf-8', newline='\n') as f:
            json.dump(ix, f, indent=1)
            f.write('\n')
        os.replace(tmp, path)
    return entry


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


def pool_settings(e, alternates=True):
    """A build entry -> the farm pool's run settings (zero-core docs/az.md 45): pool_build, pool_runtime, pool_worker
    (the desktop's copy of the signed executable) and, with `alternates`, pool_alternates (the CUDA alternate; '' when
    there is none)."""
    out = {'pool_build': e['build_id'], 'pool_runtime': e['runtime'], 'pool_worker': e['worker']}
    if alternates:
        out['pool_alternates'] = (e.get('cuda') or {}).get('build_id') or ''
    return out


def describe(e):
    c = e.get('cuda') or {}
    return (f"commit {e.get('asked', e['commit'])[:12]}: build {e['build_id']} (runtime {e['runtime']}"
            + (f", CUDA alternate {c['build_id']} for {c['runtime']}" if c.get('build_id') else ', no CUDA alternate')
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
    checkout's commit (or <rev>'s) and its --pool-runtime, --pool-worker and --pool-alternates added unless given
    (`python -m mtgz.az.run` does it: the ids are saved in config.json like any flag). SystemExit when there is none."""
    i, v = _flag(argv, '--pool-build')
    if v is None or not (v == 'published' or v.startswith('published:')):
        return argv
    rev = v.split(':', 1)[1] if ':' in v else 'HEAD'
    e = lookup(rev, root, path)
    if e is None:
        raise SystemExit(f'--pool-build {v}: no published farm build for {rev} ({index_path()}; '
                         f'python tools/farm_release.py {rev})')
    out = list(argv)
    if out[i] == '--pool-build':
        out[i + 1] = e['build_id']
    else:
        out[i] = f'--pool-build={e["build_id"]}'
    for k, val in pool_settings(e).items():
        flag = '--' + k.replace('_', '-')
        if k != 'pool_build' and _flag(out, flag)[1] is None:
            out += [flag, val]
    print(f'farm build: {describe(e)}', flush=True)
    return out
