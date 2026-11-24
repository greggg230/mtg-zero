"""tools/zcache.py and tools/warm_pool.py (docs/ENGINE.md 167). The cache: the key changes with every Rust input and
with nothing else, a checkout with uncommitted Rust inputs has no key, entries are immutable and served only to their
own key (a tampered file is refused), and eviction is least recently used. The pool: a lease is exclusive, a dead
holder's (or a reused pid's) lease is taken over, and acquire's lease outlives the process that asked (its detached
holder keeps it until it is released or its ttl ends). A throwaway git repo stands in for a checkout; nothing is
built."""
import json
import os
import subprocess
import sys
import time

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import warm_pool as W  # noqa: E402
import zcache as Z  # noqa: E402


def sh(cwd, *args):
    subprocess.run(['git', *args], cwd=cwd, check=True, capture_output=True)


@pytest.fixture
def repo(tmp_path, monkeypatch):
    r = tmp_path / 'repo'
    (r / 'engine' / 'src').mkdir(parents=True)
    (r / 'engine' / 'src' / 'lib.rs').write_text('pub fn f() {}\n')
    (r / 'data').mkdir()
    (r / 'data' / 'cards.json').write_text('{}\n')
    (r / 'mtgz').mkdir()
    (r / 'mtgz' / 'run.py').write_text('x = 1\n')
    (r / 'Cargo.toml').write_text('[workspace]\n')
    sh(r, 'init', '-q')
    sh(r, '-c', 'user.email=t@t', '-c', 'user.name=t', 'add', '-A')
    sh(r, '-c', 'user.email=t@t', '-c', 'user.name=t', 'commit', '-q', '-m', 'one')
    monkeypatch.setattr(Z, 'core_state', lambda root: 'c' * 40)
    monkeypatch.setattr(Z, 'toolchain', lambda: 'rustc 1.95.0')
    monkeypatch.setattr(Z, 'python_id', lambda py: '3.13.14 win-amd64')
    monkeypatch.setenv('MTGZ_ARTIFACTS', str(tmp_path / 'cache'))
    for k in ('RUSTFLAGS', 'MTGZ_ARTIFACT_CACHE', 'ZERO_CORE_UNPINNED'):
        monkeypatch.delenv(k, raising=False)
    return r


def commit(r, path, text):
    p = r / path
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(text)
    sh(r, '-c', 'user.email=t@t', '-c', 'user.name=t', 'add', '-A')
    sh(r, '-c', 'user.email=t@t', '-c', 'user.name=t', 'commit', '-q', '-m', path)


def key(r, kind='worker'):
    return Z.key_of(Z.key_inputs(str(r), kind))


def test_the_key_follows_the_rust_inputs_and_nothing_else(repo, monkeypatch):
    k0 = key(repo)
    commit(repo, 'mtgz/run.py', 'x = 2\n')                       # a Python commit: the same key
    commit(repo, 'docs/ENGINE.md', '# notes\n')                  # a docs commit too
    assert key(repo) == k0
    commit(repo, 'engine/src/lib.rs', 'pub fn f() { }\n')        # an engine change: a new key
    k1 = key(repo)
    assert k1 != k0
    commit(repo, 'data/cards.json', '{"a": 1}\n')                # data the engine compiles in
    assert key(repo) != k1
    k2 = key(repo)
    monkeypatch.setenv('RUSTFLAGS', '-C target-cpu=native')       # the build's flags
    assert key(repo) != k2
    monkeypatch.delenv('RUSTFLAGS')
    monkeypatch.setattr(Z, 'toolchain', lambda: 'rustc 1.96.0')  # another toolchain
    assert key(repo) != k2
    monkeypatch.setattr(Z, 'toolchain', lambda: 'rustc 1.95.0')
    monkeypatch.setattr(Z, 'core_state', lambda root: 'd' * 40)  # another zero-core pin
    assert key(repo) != k2
    monkeypatch.setattr(Z, 'core_state', lambda root: 'c' * 40)
    assert key(repo, 'pyd') != k2                                 # each kind its own key


def test_uncommitted_rust_inputs_have_no_key(repo, monkeypatch):
    (repo / 'engine' / 'src' / 'lib.rs').write_text('pub fn g() {}\n')
    with pytest.raises(Z.Uncachable, match='uncommitted or untracked'):
        key(repo)
    sh(repo, 'checkout', '--', '.')
    (repo / 'engine' / 'src' / 'new.rs').write_text('\n')
    with pytest.raises(Z.Uncachable):
        key(repo)
    os.remove(repo / 'engine' / 'src' / 'new.rs')
    (repo / 'mtgz' / 'scratch.py').write_text('\n')               # outside the Rust inputs: no matter
    key(repo)
    for env in ('MTGZ_ARTIFACT_CACHE=0', 'ZERO_CORE_UNPINNED=1'):
        k, v = env.split('=')
        monkeypatch.setenv(k, v)
        with pytest.raises(Z.Uncachable, match=k):
            key(repo)
        monkeypatch.delenv(k)


def test_store_fetch_immutable_and_verified(repo, tmp_path):
    exe = tmp_path / 'built' / Z.WORKER_EXE
    exe.parent.mkdir()
    exe.write_bytes(b'worker v1')
    k0 = key(repo)
    assert 'stored worker' in Z.store(str(repo), 'worker', str(exe), key_before=k0)
    exe.write_bytes(b'another build of the same inputs')
    assert 'already stored' in Z.store(str(repo), 'worker', str(exe))          # never replaced
    dest = tmp_path / 'out' / Z.WORKER_EXE
    hit, msg = Z.fetch(str(repo), 'worker', str(dest))
    assert hit and dest.read_bytes() == b'worker v1' and msg.startswith(f'artifact cache: hit worker {k0[:16]} -> ')
    commit(repo, 'engine/src/lib.rs', 'pub fn h() {}\n')
    hit, msg = Z.fetch(str(repo), 'worker', str(dest))                         # another key: a miss, never v1
    assert hit is False and 'miss' in msg
    assert 'changed during the build' in Z.store(str(repo), 'worker', str(exe), key_before=k0)
    # a tampered entry is refused
    sh(repo, 'reset', '-q', '--hard', 'HEAD~1')
    d = Z.entry_dir('worker', k0)
    f = os.path.join(d, Z.WORKER_EXE)
    os.chmod(f, 0o600)
    with open(f, 'wb') as fh:
        fh.write(b'tampered')
    hit, msg = Z.fetch(str(repo), 'worker', str(dest))
    assert hit is False and 'does not match its manifest' in msg
    m = Z.read_manifest(d)
    assert m['key'] == k0 and m['kind'] == 'worker' and m['inputs']['tree_files'] == 3


def test_eviction_is_least_recently_used(repo, tmp_path, monkeypatch):
    big = tmp_path / 'big'
    for i in range(3):
        commit(repo, 'engine/src/lib.rs', f'pub fn f{i}() {{}}\n')
        big.write_bytes(b'x' * 1000)
        Z.store(str(repo), 'worker', str(big))
        d = Z.entry_dir('worker', key(repo))
        os.utime(os.path.join(d, 'manifest.json'), (time.time() - 100 + i, time.time() - 100 + i))
    names = sorted(os.path.basename(e[0]) for e in Z.entries())
    es = sorted(Z.entries(), key=lambda e: e[2])
    oldest = es[0][0]
    msg = Z.gc((es[1][1] + es[2][1] + 10) / 1e9)               # room for the two most recently used
    left = [os.path.basename(e[0]) for e in Z.entries()]
    assert 'evicted 1' in msg and len(left) == 2 and os.path.basename(oldest) not in left and set(left) < set(names)


def test_check_scope_reads_cargo_dep_info(repo, tmp_path):
    rel = tmp_path / 'target' / 'release'
    rel.mkdir(parents=True)
    (rel / 'mtgz_py.d').write_text(f'{rel}\\mtgz_py.dll: engine\\src\\lib.rs data\\cards.json C:\\Users\\x\\.cargo\\registry\\a.rs\n')
    (rel / 'mtgz-worker.d').write_text(f'{rel}\\mtgz-worker.exe: engine\\src\\lib.rs tools\\gen.py\n')
    problems, seen = Z.check_scope(str(repo), str(tmp_path / 'target'))
    assert seen == 5 and len(problems) == 1 and 'tools' in problems[0] and problems[0].startswith('worker')


# ---------------------------------------------------------------------------- the warm pool's leases

@pytest.fixture
def pool(tmp_path, monkeypatch):
    monkeypatch.setenv('MTGZ_WARM_ROOT', str(tmp_path / 'pool'))
    return tmp_path / 'pool'


def test_a_lease_is_exclusive_and_a_dead_holders_is_taken_over(pool):
    assert W.try_lease(1, 'a', 'abc')
    assert not W.try_lease(1, 'b', 'abc')                            # held by this (live) process
    info = json.load(open(W.lease_path(1), encoding='utf-8'))
    assert info['pid'] == os.getpid() and info['label'] == 'a'
    W.release(1)
    assert W.try_lease(1, 'b', 'abc')
    W.release(1)
    # a holder that is gone
    p = subprocess.Popen([sys.executable, '-c', 'pass'])
    p.wait()
    assert W.try_lease(2, 'dead', 'x', holder={'pid': p.pid, 'started': 1.0})
    assert W.try_lease(2, 'next', 'x')
    W.release(2)
    # a live pid that is not the holder any more (Windows reuses pids): its creation time differs
    assert W.try_lease(3, 'reused', 'x', holder={'pid': os.getpid(), 'started': 12345.0})
    assert W.try_lease(3, 'next', 'x')
    W.release(3)


def hand_over(ttl):
    """A short-lived process takes slot 1's lease and hands it to a holder, as `acquire` does, then exits (a Git Bash
    command's forked shell, an agent's tool call) -> its pid: the interpreter's own (`os.getpid()` there), which is not
    `Popen.pid` when `sys.executable` is a venv's python.exe on Windows (a launcher that runs the interpreter as its
    child)."""
    code = ('import os, sys; sys.path.insert(0, sys.argv[1]); import warm_pool as W; '
            'assert W.try_lease(1, "gate", "abc"); W.spawn_holder(W.lease_path(1), float(sys.argv[2])); '
            'print(os.getpid())')
    p = subprocess.Popen([sys.executable, '-c', code, os.path.join(ROOT, 'tools'), str(ttl)],
                         stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    out, err = p.communicate(timeout=60)
    assert p.returncode == 0, err
    return int(out.strip().splitlines()[-1])


def gone_within(pid, seconds):
    t0 = time.time()
    while time.time() - t0 < seconds:
        if W.proc_started(pid) is None:
            return True
        time.sleep(0.2)
    return False


def test_an_acquired_lease_outlives_the_process_that_asked(pool):
    asker = hand_over(ttl=600)
    try:
        info = W.read_lease(W.lease_path(1))
        assert info['held_for'] == asker and info['pid'] != asker and info['label'] == 'gate'
        assert W.lease_live(info)
        assert not W.try_lease(1, 'next', 'x')                        # the asker is gone; its holder keeps the slot
    finally:
        W.release(1)
    assert gone_within(info['pid'], 15)                               # released: the holder leaves
    assert W.try_lease(1, 'next', 'x')
    W.release(1)


def test_a_held_lease_ends_at_its_ttl(pool):
    hand_over(ttl=1)
    info = W.read_lease(W.lease_path(1))
    try:
        assert gone_within(info['pid'], 15)
        assert W.read_lease(W.lease_path(1)) is None                  # released by its holder
    finally:
        W.release(1)


def test_a_holder_takes_only_the_lease_it_was_handed(pool):
    assert W.try_lease(1, 'mine', 'x')
    assert W.hold(W.lease_path(1), 60, from_pid=os.getpid() + 1) == 1   # another process's lease: nothing taken
    assert W.read_lease(W.lease_path(1))['pid'] == os.getpid()
    W.release(1)


def test_each_slot_has_its_own_target_dir(pool, monkeypatch):
    monkeypatch.setenv('MTGZ_WARM_TARGETS', 'T:/t')
    assert {W.target_of(k) for k in range(1, 5)} == {os.path.join('T:/t', f'warm-{k}') for k in range(1, 5)}
    assert W.slot_path(2) == os.path.join(str(pool), 'warm-2')
