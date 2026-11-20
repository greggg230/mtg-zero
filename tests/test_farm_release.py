"""The farm release tooling (docs/ENGINE.md 196): the package's declared setting classes = the worker's table, the
builds index and its lookups (mtgz.az.farm_builds), `--pool-build published`, tools/farm_release.py's reuse of a build
of the same Rust inputs, tools/farm_release_watch.py's choice of head and its no-retry rule, tools/ab.py's
`"pool_build": "published"`. No build, signature or network: temporary git repositories and indexes."""
import json
import os
import re
import subprocess
import sys

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'tools'))


def test_package_setting_classes_equal_the_workers_table():
    """mtgz's GamePackage.setting_classes (what the trainer declares) and worker/src/main.rs PACKAGE_SETTINGS (what a
    build knows) name the same settings with the same classes, every package setting has one."""
    import mtgz.az  # noqa: F401
    from zero import package
    pkg = package.active()
    with open(os.path.join(ROOT, 'worker', 'src', 'main.rs'), encoding='utf-8') as f:
        src = f.read()
    table = src[src.index('static PACKAGE_SETTINGS'):]
    table = table[:table.index('];')]
    rust = dict(re.findall(r'\("([a-z0-9_]+)", "([a-z_:]+)",', table))
    assert rust == pkg.setting_classes
    assert set(pkg.setting_classes) == set(pkg.settings)


def _repo(tmp_path):
    """A git repository with an engine file, a docs file and a zero-core gitlink."""
    r = tmp_path / 'repo'
    r.mkdir()
    g = lambda *a: subprocess.run(['git', *a], cwd=r, check=True, capture_output=True, text=True).stdout.strip()  # noqa: E731
    g('init', '-q')
    g('config', 'user.email', 't@example.com')
    g('config', 'user.name', 't')
    (r / 'engine').mkdir()
    (r / 'engine' / 'lib.rs').write_text('fn a() {}\n')
    (r / 'docs.md').write_text('a\n')
    g('add', '.')
    g('update-index', '--add', '--cacheinfo', f'160000,{"1" * 40},third_party/zero-core')
    g('commit', '-q', '-m', 'one')
    c1 = g('rev-parse', 'HEAD')
    (r / 'docs.md').write_text('b\n')
    g('commit', '-qm', 'docs only', 'docs.md')
    c2 = g('rev-parse', 'HEAD')
    (r / 'engine' / 'lib.rs').write_text('fn b() {}\n')
    g('commit', '-qm', 'engine', 'engine/lib.rs')
    c3 = g('rev-parse', 'HEAD')
    return str(r), (c1, c2, c3), g


def _entry(commit, key, bid='a' * 24, cuda='c' * 24):
    return {'commit': commit, 'built_at': commit, 'rust_key': key, 'build_id': bid, 'runtime': 'r' * 24,
            'worker': f'X:/builds/{bid}/mtgz-worker.exe', 'cuda': {'build_id': cuda, 'runtime': 'u' * 24} if cuda else None,
            'published': True, 'created': '2026-11-17T00:00:00', 'gates': {'line': 'FARM GATES PASSED'}}


@pytest.fixture
def fbx(tmp_path, monkeypatch):
    import mtgz.az  # noqa: F401
    from mtgz.az import farm_builds as FB
    monkeypatch.setenv('MTGZ_FARM_BUILDS', str(tmp_path / 'builds'))
    monkeypatch.setattr(FB, 'toolchain', lambda root: 'rustc test')
    return FB


def test_rust_key_lookup_and_pool_settings(tmp_path, fbx):
    FB = fbx
    root, (c1, c2, c3), _ = _repo(tmp_path)
    k1, k2, k3 = (FB.rust_key(c, root) for c in (c1, c2, c3))
    assert k1 == k2 != k3, 'a docs-only commit keeps the key, an engine change moves it'
    assert FB.lookup(c1, root) is None and FB.load()['builds'] == []
    FB.add(_entry(c1, k1))
    e = FB.lookup(c1, root)
    assert e['exact'] and e['asked'] == c1 and e['build_id'] == 'a' * 24
    e2 = FB.lookup(c2, root)
    assert not e2['exact'] and e2['commit'] == c1 and e2['asked'] == c2, 'the same Rust inputs: the build of c1'
    assert FB.lookup(c3, root) is None
    assert FB.pool_settings(e) == {'pool_build': 'a' * 24, 'pool_runtime': 'r' * 24, 'pool_worker': 'X:/builds/' + 'a' * 24 +
                                   '/mtgz-worker.exe', 'pool_alternates': 'c' * 24}
    FB.add(dict(_entry(c3, k3, bid='b' * 24, cuda=None)))
    assert FB.pool_settings(FB.lookup(c3, root))['pool_alternates'] == ''
    # the DirectML alternate (AMD GPUs, zero-farm PROTOCOL rev 22) after the CUDA one; alone when there is no CUDA one
    assert FB.pool_settings(dict(e, dml={'build_id': 'm' * 24}))['pool_alternates'] == 'c' * 24 + ',' + 'm' * 24
    assert FB.pool_settings(dict(e, cuda=None, dml={'build_id': 'm' * 24}))['pool_alternates'] == 'm' * 24
    assert 'DirectML alternate ' + 'm' * 24 in FB.describe(dict(e, dml={'build_id': 'm' * 24}))
    assert FB.find(FB.load(), commit=c1, runtime='z' * 24) is None
    FB.add(dict(_entry(c2, k2, bid='d' * 24), published=False))
    assert FB.lookup(c2, root)['build_id'] == 'a' * 24, 'an unpublished entry is never looked up'


def test_pool_build_published_in_a_run_command_line(tmp_path, fbx):
    FB = fbx
    root, (c1, c2, c3), g = _repo(tmp_path)
    g('checkout', '-q', c2)                       # (HEAD: a docs commit after c1)
    argv = ['--preset', 'x', '--pool-build', 'published', '--pool-alternates', '']
    with pytest.raises(SystemExit, match='no published farm build'):
        FB.published_pool(argv, root)
    FB.add(_entry(c1, FB.rust_key(c1, root)))
    out = FB.published_pool(argv, root)
    assert out == ['--preset', 'x', '--pool-build', 'a' * 24, '--pool-alternates', '', '--pool-runtime', 'r' * 24,
                   '--pool-worker', 'X:/builds/' + 'a' * 24 + '/mtgz-worker.exe']
    assert FB.published_pool([f'--pool-build=published:{c1[:10]}'], root)[0] == '--pool-build=' + 'a' * 24
    assert FB.published_pool(['--pool-build', 'e' * 24], root) == ['--pool-build', 'e' * 24]
    with pytest.raises(SystemExit):
        FB.published_pool(['--pool-build', f'published:{c3}'], root)


def test_release_reuses_a_build_of_the_same_rust_inputs(tmp_path, fbx, monkeypatch):
    """`farm_release.py release` of a commit whose Rust inputs equal a published build's: an index entry naming that
    build (`reused_from`), nothing built, signed or published; again: 'released already'; a commit with other Rust inputs
    goes to the build path (here stopped at its preflight: no reference)."""
    import farm_release as FR
    FB = fbx
    root, (c1, c2, c3), _ = _repo(tmp_path)
    monkeypatch.setattr(FR, 'ROOT', root)
    monkeypatch.setattr(FB, 'ROOT', root)
    FB.add(_entry(c1, FB.rust_key(c1, root), cuda=None))
    with open(os.path.join(FB.builds_dir(), 'release.json'), 'w') as f:
        json.dump({'cpu_runtime': 'r' * 24}, f)
    built = []
    monkeypatch.setattr(FR.Release, 'checkout', lambda self, sha: built.append(sha))
    assert FR.main(['release', c2, '--no-slot', '--normal-priority', '--no-cuda']) == 0
    e = FB.load()['builds'][-1]
    assert (e['commit'], e['build_id'], e['reused_from']) == (c2, 'a' * 24, c1) and built == []
    assert FR.main(['release', c2, '--no-slot', '--normal-priority', '--no-cuda']) == 0
    assert len(FB.load()['builds']) == 2
    assert FR.main(['release', c3, '--no-slot', '--normal-priority', '--no-cuda']) == 1
    r = json.load(open(os.path.join(FB.builds_dir(), 'releases', c3[:12], 'result.json')))
    assert r['failed'] == 'preflight' and 'reference' in r['error'] and built == []
    assert FR.main(['lookup', c2]) == 0 and FR.main(['lookup', c3]) == 1


def test_the_watcher_takes_the_newest_head_once(tmp_path, fbx, monkeypatch):
    import farm_release_watch as W
    root, (c1, c2, c3), g = _repo(tmp_path)
    monkeypatch.setattr(W, 'ROOT', root)
    assert W.newest(c1, c3) == c3 and W.newest(c3, c1) == c3 and W.newest(None, c2) == c2 and W.newest(c2, None) == c2
    P = W.paths()
    heads = iter([c1, c1, c3, c3])
    monkeypatch.setattr(W, 'head', lambda fetch=True, P=None: next(heads))
    ran = []
    rc = {c1: 0, c3: 1}
    monkeypatch.setattr(W, 'release', lambda P, sha, py: (ran.append(sha), (rc[sha], ['line']))[1])
    a = type('A', (), {'no_fetch': True, 'python': sys.executable})()
    st = {}
    for _ in range(4):
        W.poll(P, st, a)
    assert ran == [c1, c3], 'each head once; a failed head is not retried'
    assert st['failed'] == c3 and c1 in st['done'] and not st['last']['ok']
    log = open(P['log'], encoding='utf-8').read()
    assert 'released ' + c1[:12] in log and 'FAILED ' + c3[:12] in log


def test_ab_pool_build_published(tmp_path, fbx, monkeypatch):
    import ab
    FB = fbx
    root, (c1, c2, c3), _ = _repo(tmp_path)
    FB.add(_entry(c1, FB.rust_key(c1, root)))
    s = {'code': {'worktree': root, 'sha': c2[:10]}}
    o = ab.published_build(s, {'pool_build': 'published', 'pool_alternates': '', 'lr': 1})
    assert o == {'pool_build': 'a' * 24, 'pool_alternates': '', 'lr': 1, 'pool_runtime': 'r' * 24,
                 'pool_worker': 'X:/builds/' + 'a' * 24 + '/mtgz-worker.exe'}
    assert ab.published_build(s, {'lr': 1}) == {'lr': 1}
    with pytest.raises(SystemExit, match='no published farm build'):
        ab.published_build(s, {'pool_build': f'published:{c3}'})
