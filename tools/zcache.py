"""The artifact cache (docs/ENGINE.md 167): the Rust builds of a checkout, kept by the hash of every Rust input, so a
commit that changes no Rust input (a Python, tools or docs commit, a merge of such) reuses its parent's builds - no
cargo at all.

    python tools/zcache.py key pyd                         # this checkout's key for a kind (or why it has none)
    python tools/zcache.py fetch pyd --dest mtgz/_engine.pyd   # exit 0 hit (copied + verified), 1 miss, 3 uncachable
    python tools/zcache.py store pyd --src mtgz/_engine.pyd --key <the key before the build>
    python tools/zcache.py worker --target-dir <dir>       # the release mtgz-worker: fetch, else cargo build + store
    python tools/zcache.py core-pyd                        # zero-core's zero._engine (the pinned core): fetch, else build
    python tools/zcache.py ls | gc [--max-gb 40] | check-scope --target-dir <dir>

Kinds: `pyd` (mtgz/_engine.pyd / .so: tools/build_py.sh), `worker` (target/release/mtgz-worker[.exe]: gate.py's
worker-build), `core-pyd` (third_party/zero-core/python/zero/_engine.*: gate.py's core-build).

The key H (sha256) of a kind hashes, for this checkout:
  * the git blob ids of every tracked file under the Rust inputs - engine/, card_ir/, py/, worker/, data/ (the card IR,
    decklists and tables the engine compiles in with include_str!), Cargo.toml, Cargo.lock, .cargo/,
    rust-toolchain(.toml) - and the zero-core gitlink (the pinned commit names every core file); core-pyd: the pin alone;
  * the toolchain (`rustc -vV`), the build's environment (RUSTFLAGS, CARGO_ENCODED_RUSTFLAGS, CARGO_BUILD_*,
    CARGO_PROFILE_*, CARGO_TARGET_*, ~/.cargo/config.toml), the platform, the profile (release), and for a Python
    module the interpreter's version (PyO3 builds for one);
  * the kind and this tool's SCHEMA.
A checkout with an uncommitted or untracked file under those paths, or a submodule not at its pin or not clean, has
no key: nothing is fetched or stored (it builds as before). `check-scope` reads cargo's own dep-info (.d files) of a
build and refuses when the build read a file outside the hashed paths - the guard against a key that misses an input.

Entries are content-addressed and immutable: <cache>/<kind>-<H[:32]>/ with manifest.json (the full key, its inputs,
each file's sha256, the commit and checkout it was built from) and the files, written to a temp dir and renamed into
place once (an existing entry is never replaced). A fetch checks the manifest's key and every file's sha256, copies,
and checks the copy. The cache never serves an entry for another key. Eviction: least recently used first (a fetch
touches the entry) until the cache is under --max-gb (default MTGZ_ARTIFACTS_MAX_GB or 40), after every store.
Cache root: $MTGZ_ARTIFACTS, else E:/zero-artifacts/mtg-zero on Windows, ~/.cache/zero-artifacts/mtg-zero elsewhere.
MTGZ_ARTIFACT_CACHE=0 turns it off (every fetch misses, nothing is stored).

What the cached builds do not carry: a panic's source location in a zero-core crate names the absolute path of the
checkout that built it (cargo passes path dependencies outside the workspace by absolute path) - messages only, never
behaviour. Test binaries are NOT cached: they compile env!("CARGO_MANIFEST_DIR") in (digests.txt and other test data
are read from the building checkout), so one checkout's test binary would test another's files. They stay in the
warm worktrees' own target dirs (tools/warm_pool.py), where the same path rebuilds only what changed.
"""
import argparse
import hashlib
import json
import os
import platform
import shutil
import stat
import subprocess
import sys
import time
import uuid

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCHEMA = 1
WIN = sys.platform == 'win32'
RUST_PATHS = ('engine', 'card_ir', 'py', 'worker', 'data', 'Cargo.toml', 'Cargo.lock', '.cargo', 'rust-toolchain',
              'rust-toolchain.toml')
ENV_PREFIXES = ('CARGO_PROFILE_', 'CARGO_TARGET_', 'CARGO_BUILD_')
ENV_EXACT = ('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'RUSTC', 'RUSTC_WRAPPER', 'CARGO_INCREMENTAL')
# not build inputs: where and how many (the dir is the checkout's own; jobs change nothing built)
ENV_IGNORED = ('CARGO_TARGET_DIR', 'CARGO_BUILD_JOBS')
KINDS = ('pyd', 'worker', 'core-pyd')
WORKER_EXE = 'mtgz-worker.exe' if WIN else 'mtgz-worker'


class Uncachable(Exception):
    """This checkout has no key for the kind (uncommitted changes, an unpinned core, the cache turned off)."""


def cache_root():
    r = os.environ.get('MTGZ_ARTIFACTS')
    if r:
        return r
    return 'E:/zero-artifacts/mtg-zero' if WIN else os.path.expanduser('~/.cache/zero-artifacts/mtg-zero')


def git(root, *args):
    p = subprocess.run(['git', *args], cwd=root, capture_output=True, text=True)
    if p.returncode:
        raise Uncachable(f'git {" ".join(args)} ({root}): {p.stderr.strip()[:300]}')
    return p.stdout


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()


def toolchain():
    p = subprocess.run(['rustc', '-vV'], capture_output=True, text=True)
    if p.returncode:
        raise Uncachable(f'rustc -vV: {p.stderr.strip()[:200]}')
    return p.stdout.strip()


def build_env():
    env = {k: v for k, v in os.environ.items()
           if (k in ENV_EXACT or k.startswith(ENV_PREFIXES)) and k not in ENV_IGNORED}
    cfg = os.path.join(os.path.expanduser('~'), '.cargo', 'config.toml')
    if os.path.isfile(cfg):
        env['~/.cargo/config.toml'] = sha256_file(cfg)
    return dict(sorted(env.items()))


def python_id(python):
    p = subprocess.run([python, '-c', 'import sys, sysconfig; print(sys.version.split()[0], sysconfig.get_platform(), '
                                      'sysconfig.get_config_var("SOABI") or sysconfig.get_config_var("EXT_SUFFIX"))'],
                       capture_output=True, text=True)
    if p.returncode:
        raise Uncachable(f'{python}: {p.stderr.strip()[:200]}')
    return p.stdout.strip()


def core_dir(root):
    return os.path.join(root, 'third_party', 'zero-core')


def core_state(root):
    """The pinned zero-core commit, checked out there and clean (else Uncachable)."""
    pin = git(root, 'rev-parse', 'HEAD:third_party/zero-core').strip()
    core = core_dir(root)
    if not os.path.exists(os.path.join(core, '.git')):
        raise Uncachable('third_party/zero-core is not initialised')
    head = git(core, 'rev-parse', 'HEAD').strip()
    if head != pin:
        raise Uncachable(f'zero-core is at {head[:12]}, the pin is {pin[:12]} (an unpinned core is never cached)')
    dirty = git(core, 'status', '--porcelain', '--untracked-files=all').strip()
    if dirty:
        raise Uncachable(f'zero-core has uncommitted or untracked files: {dirty.splitlines()[0]} ...')
    return pin


def key_inputs(root, kind, python=None):
    """-> the inputs of the kind's key (a dict, hashed as canonical JSON). Uncachable when there is no key."""
    if os.environ.get('MTGZ_ARTIFACT_CACHE', '1') == '0':
        raise Uncachable('MTGZ_ARTIFACT_CACHE=0')
    if os.environ.get('ZERO_CORE_UNPINNED', '0') == '1':
        raise Uncachable('ZERO_CORE_UNPINNED=1 (a core that is not the pin)')
    if kind not in KINDS:
        raise Uncachable(f'unknown kind {kind!r} (one of {", ".join(KINDS)})')
    inp = {'schema': SCHEMA, 'kind': kind, 'profile': 'release', 'toolchain': toolchain(), 'env': build_env(),
           'platform': f'{sys.platform} {platform.machine()}', 'zero_core': core_state(root)}
    if kind in ('pyd', 'core-pyd'):
        inp['python'] = python_id(python or os.environ.get('PYTHON') or sys.executable)
    if kind == 'core-pyd':
        return inp
    paths = [p for p in RUST_PATHS if os.path.exists(os.path.join(root, p))]
    dirty = git(root, 'status', '--porcelain', '--untracked-files=all', '--ignore-submodules=all', '--', *paths).strip()
    if dirty:
        raise Uncachable(f'uncommitted or untracked Rust inputs: {", ".join(dirty.splitlines()[:3])} ...')
    tree = git(root, 'ls-tree', '-r', '--full-tree', 'HEAD', '--', *paths)
    inp['tree'] = hashlib.sha256(tree.encode()).hexdigest()
    inp['tree_files'] = len(tree.splitlines())
    return inp


def key_of(inputs):
    return hashlib.sha256(json.dumps(inputs, sort_keys=True).encode()).hexdigest()


def entry_dir(kind, key):
    return os.path.join(cache_root(), f'{kind}-{key[:32]}')


def files_of(kind):
    """The files an entry of `kind` holds (as stored)."""
    if kind == 'worker':
        return [WORKER_EXE]
    return ['_engine.pyd' if WIN else '_engine.so']


def read_manifest(d):
    with open(os.path.join(d, 'manifest.json'), encoding='utf-8') as f:
        return json.load(f)


def place(src, dest):
    """Copy `src` to `dest`; a Windows-locked `dest` (a loaded .pyd) is renamed out of the way first, as
    build_engine.sh does (running processes keep the old file)."""
    os.makedirs(os.path.dirname(os.path.abspath(dest)), exist_ok=True)
    tmp = f'{dest}.{uuid.uuid4().hex[:8]}.part'
    shutil.copyfile(src, tmp)
    try:
        os.replace(tmp, dest)
    except PermissionError:
        old = f'{dest}.{int(time.time())}.old'
        os.replace(dest, old)
        os.replace(tmp, dest)
        print(f'note: {dest} was in use; the old file was moved to {old}')


def fetch(root, kind, dest, python=None):
    """-> (hit, message). Never serves an entry whose manifest key or file hashes differ."""
    try:
        key = key_of(key_inputs(root, kind, python))
    except Uncachable as e:
        return None, f'artifact cache: {kind} uncachable here ({e})'
    d = entry_dir(kind, key)
    if not os.path.isdir(d):
        return False, f'artifact cache: miss {kind} {key[:16]}'
    try:
        m = read_manifest(d)
    except (OSError, ValueError) as e:
        return False, f'artifact cache: {kind} {key[:16]}: unreadable manifest ({e}); treated as a miss'
    if m.get('key') != key or m.get('kind') != kind:
        return False, f'artifact cache: {d} holds key {str(m.get("key"))[:16]}, not {key[:16]}: refused'
    name = files_of(kind)[0]
    src = os.path.join(d, name)
    want = (m.get('files') or {}).get(name)
    if not want or not os.path.isfile(src) or sha256_file(src) != want:
        return False, f'artifact cache: {d}/{name} does not match its manifest: refused'
    place(src, dest)
    if sha256_file(dest) != want:
        return False, f'artifact cache: the copy {dest} does not match {want[:16]}: refused'
    try:
        os.utime(os.path.join(d, 'manifest.json'))         # (the LRU clock)
    except OSError:
        pass
    return True, (f'artifact cache: hit {kind} {key[:16]} -> {dest} (sha256 {want[:16]}, built from '
                  f'{str(m.get("commit"))[:12]} in {m.get("checkout")})')


def store(root, kind, src, key_before=None, python=None):
    """Store `src` as the kind's entry for this checkout's key (unchanged since `key_before`, the key when the build
    started) -> message. An existing entry is kept, never replaced."""
    try:
        inputs = key_inputs(root, kind, python)
    except Uncachable as e:
        return f'artifact cache: not stored ({e})'
    key = key_of(inputs)
    if key_before and key_before != key:
        return f'artifact cache: not stored (the key changed during the build: {key_before[:16]} -> {key[:16]})'
    d = entry_dir(kind, key)
    if os.path.isdir(d):
        return f'artifact cache: {kind} {key[:16]} already stored'
    name = files_of(kind)[0]
    tmp = os.path.join(cache_root(), f'.tmp-{uuid.uuid4().hex}')
    os.makedirs(tmp)
    try:
        shutil.copyfile(src, os.path.join(tmp, name))
        digest = sha256_file(os.path.join(tmp, name))
        if digest != sha256_file(src):
            raise OSError(f'{src} changed while it was copied')
        commit = git(root, 'rev-parse', 'HEAD').strip() if kind != 'core-pyd' else inputs['zero_core']
        m = {'kind': kind, 'key': key, 'inputs': inputs, 'files': {name: digest}, 'commit': commit,
             'checkout': os.path.abspath(root), 'created': time.strftime('%Y-%m-%dT%H:%M:%S%z')}
        with open(os.path.join(tmp, 'manifest.json'), 'w', encoding='utf-8') as f:
            json.dump(m, f, indent=1, sort_keys=True)
        for p in (name, 'manifest.json'):
            os.chmod(os.path.join(tmp, p), stat.S_IREAD)
        try:
            os.rename(tmp, d)
        except OSError:
            if os.path.isdir(d):                     # another build stored the same key first: keep theirs
                return f'artifact cache: {kind} {key[:16]} already stored'
            raise
    finally:
        if os.path.isdir(tmp):
            rmtree(tmp)
    msg = f'artifact cache: stored {kind} {key[:16]} ({os.path.getsize(src) / 1e6:.1f} MB, sha256 {digest[:16]})'
    gc_msg = gc(float(os.environ.get('MTGZ_ARTIFACTS_MAX_GB', '40')))
    return msg + (f'; {gc_msg}' if gc_msg else '')


def rmtree(path):
    def onerr(func, p, _):
        os.chmod(p, stat.S_IWRITE)
        func(p)
    shutil.rmtree(path, onerror=onerr)


def entries():
    """-> [(dir, bytes, last used)] of the cache's entries."""
    out = []
    root = cache_root()
    if not os.path.isdir(root):
        return out
    for name in os.listdir(root):
        d = os.path.join(root, name)
        if name.startswith('.') or not os.path.isdir(d):
            continue
        size = sum(os.path.getsize(os.path.join(d, f)) for f in os.listdir(d))
        try:
            used = os.path.getmtime(os.path.join(d, 'manifest.json'))
        except OSError:
            used = 0.0
        out.append((d, size, used))
    return out


def gc(max_gb):
    """Evict least recently used entries until the cache holds at most `max_gb` -> a message, or '' when none went.
    Also removes temp dirs a killed store left behind (older than an hour)."""
    root = cache_root()
    if os.path.isdir(root):
        for name in os.listdir(root):
            p = os.path.join(root, name)
            if name.startswith('.tmp-') and time.time() - os.path.getmtime(p) > 3600:
                rmtree(p)
    es = sorted(entries(), key=lambda e: e[2])
    total = sum(e[1] for e in es)
    gone = []
    while es and total > max_gb * 1e9:
        d, size, _ = es.pop(0)
        try:
            rmtree(d)
        except OSError:
            continue                              # (in use right now: the next gc)
        total -= size
        gone.append(os.path.basename(d))
    return f'evicted {len(gone)} least recently used ({", ".join(gone)}); {total / 1e9:.1f} GB kept' if gone else ''


def dep_files(target_dir, kind):
    """The source files cargo's dep-info says the kind's build read (the .d file next to the artifact)."""
    name = {'pyd': 'mtgz_py.d', 'worker': 'mtgz-worker.d'}[kind]
    path = os.path.join(target_dir, 'release', name)
    if not os.path.isfile(path):
        raise FileNotFoundError(f'{path}: no dep-info (build the kind into this target dir first)')
    text = open(path, encoding='utf-8', errors='replace').read().replace('\\\n', ' ')
    files = []
    for line in text.splitlines():
        _, sep, deps = line.partition(': ')
        if not sep:
            continue
        cur = ''
        for tok in deps.split(' '):
            if tok.endswith('\\') and not tok.endswith('\\\\'):
                cur += tok[:-1] + ' '                   # an escaped space inside a path
                continue
            cur += tok
            if cur:
                files.append(cur)
            cur = ''
    return sorted(set(files))


def check_scope(root, target_dir, kinds=('pyd', 'worker')):
    """-> problems: every file a build read is under the hashed Rust paths, inside the pinned zero-core, or outside
    the checkout entirely (cargo's registry, the toolchain: named by Cargo.lock and the toolchain id)."""
    rootn = os.path.normcase(os.path.abspath(root))
    core = os.path.normcase(os.path.abspath(core_dir(root)))
    allowed = [os.path.normcase(os.path.join(rootn, p)) for p in RUST_PATHS]
    problems, seen = [], 0
    for kind in kinds:
        for f in dep_files(target_dir, kind):
            p = os.path.normcase(os.path.abspath(f if os.path.isabs(f) else os.path.join(root, f)))
            seen += 1
            if not p.startswith(rootn + os.sep):
                continue
            if p.startswith(core + os.sep):
                continue
            if any(p == a or p.startswith(a + os.sep) for a in allowed):
                continue
            problems.append(f'{kind}: {f} is read by the build but not in the key\'s paths {RUST_PATHS}')
    return problems, seen


def cmd_worker(a):
    """The release worker into <target>/release: fetched by key, else built (cargo) and stored."""
    root = a.root
    dest = os.path.join(a.target_dir, 'release', WORKER_EXE)
    hit, msg = fetch(root, 'worker', dest)
    print(msg, flush=True)
    if hit:
        return 0
    try:
        key0 = key_of(key_inputs(root, 'worker'))
    except Uncachable:
        key0 = None
    env = dict(os.environ, CARGO_TARGET_DIR=a.target_dir)
    p = subprocess.run(['cargo', 'build', '--release', '-p', 'mtgz_worker'], cwd=root, env=env)
    if p.returncode:
        return p.returncode
    if key0:
        print(store(root, 'worker', dest, key_before=key0), flush=True)
    return 0


def cmd_core_pyd(a):
    """zero-core's zero._engine for its pinned commit: fetched by key, else built by the core's tools/build_py.sh and
    stored; then imported (the core-build step's "zero._engine OK: ..." line)."""
    core = core_dir(a.root)
    py = os.environ.get('PYTHON') or sys.executable
    dest = os.path.join(core, 'python', 'zero', '_engine.pyd' if WIN else '_engine.so')
    hit, msg = fetch(a.root, 'core-pyd', dest, python=py)
    print(msg, flush=True)
    if not hit:
        try:
            key0 = key_of(key_inputs(a.root, 'core-pyd', py))
        except Uncachable:
            key0 = None
        p = subprocess.run(['bash', 'tools/build_py.sh'], cwd=core)
        if p.returncode:
            return p.returncode
        if key0:
            print(store(a.root, 'core-pyd', dest, key_before=key0, python=py), flush=True)
        return 0
    code = ("import sys; sys.path.insert(0, sys.argv[1]); import importlib; e = importlib.import_module('zero._engine'); "
            "print('zero._engine OK:', e.__file__, '| games:', e.game_names())")
    return subprocess.run([py, '-c', code, os.path.join(core, 'python')]).returncode


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0], formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest='cmd', required=True)
    for name in ('key', 'fetch', 'store'):
        s = sub.add_parser(name)
        s.add_argument('kind', choices=KINDS)
        s.add_argument('--root', default=ROOT)
        s.add_argument('--python', default=None)
        if name == 'fetch':
            s.add_argument('--dest', required=True)
        if name == 'store':
            s.add_argument('--src', required=True)
            s.add_argument('--key', default=None, help='the key when the build started (refused if it changed)')
    s = sub.add_parser('worker')
    s.add_argument('--root', default=ROOT)
    s.add_argument('--target-dir', required=True)
    s = sub.add_parser('core-pyd')
    s.add_argument('--root', default=ROOT)
    sub.add_parser('ls')
    s = sub.add_parser('gc')
    s.add_argument('--max-gb', type=float, default=float(os.environ.get('MTGZ_ARTIFACTS_MAX_GB', '40')))
    s = sub.add_parser('check-scope')
    s.add_argument('--root', default=ROOT)
    s.add_argument('--target-dir', required=True)
    a = ap.parse_args(argv)
    if a.cmd == 'key':
        try:
            print(key_of(key_inputs(a.root, a.kind, a.python)))
            return 0
        except Uncachable as e:
            print(f'uncachable: {e}', file=sys.stderr)
            return 3
    if a.cmd == 'fetch':
        hit, msg = fetch(a.root, a.kind, a.dest, a.python)
        print(msg, flush=True)
        return 0 if hit else (3 if hit is None else 1)
    if a.cmd == 'store':
        print(store(a.root, a.kind, a.src, a.key, a.python), flush=True)
        return 0
    if a.cmd == 'worker':
        return cmd_worker(a)
    if a.cmd == 'core-pyd':
        return cmd_core_pyd(a)
    if a.cmd == 'ls':
        es = sorted(entries(), key=lambda e: -e[2])
        for d, size, used in es:
            try:
                m = read_manifest(d)
            except (OSError, ValueError):
                m = {}
            print(f'{os.path.basename(d)}  {size / 1e6:8.1f} MB  used {time.strftime("%Y-%m-%d %H:%M", time.localtime(used))}  '
                  f'from {str(m.get("commit"))[:12]}')
        print(f'{len(es)} entries, {sum(e[1] for e in es) / 1e9:.2f} GB in {cache_root()}')
        return 0
    if a.cmd == 'gc':
        print(gc(a.max_gb) or 'nothing to evict')
        return 0
    if a.cmd == 'check-scope':
        problems, seen = check_scope(a.root, a.target_dir)
        for p in problems:
            print(p)
        print(f'check-scope: {seen} files read by the builds, {len(problems)} outside the key')
        return 1 if problems else 0
    return 2


if __name__ == '__main__':
    sys.exit(main())
