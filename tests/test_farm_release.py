"""The farm release tooling (docs/ENGINE.md 196): the package's declared setting classes = the worker's table, the
builds index and its lookups (mtgz.az.farm_builds), `--pool-build published`, tools/farm_release.py's reuse of a build
of the same Rust inputs, tools/farm_release_watch.py's choice of head and its no-retry rule, tools/ab.py's
`"pool_build": "published"`; the macOS part of every release (docs/ENGINE.md 216): its switches, the backfill of an
entry without one, a failure that keeps the published builds (exit 3), the watcher's retries. No build, signature, Mac
or network: temporary git repositories and indexes, the steps that build or reach the Mac replaced."""
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
                                   '/mtgz-worker.exe', 'pool_alternates': 'c' * 24, 'pool_variants': 'gpu=fp16'}
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
                   '--pool-worker', 'X:/builds/' + 'a' * 24 + '/mtgz-worker.exe', '--pool-variants', 'gpu=fp16']
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
    flags = ['--no-slot', '--normal-priority', '--no-cuda', '--no-mac']      # (the macOS part: the tests below)
    assert FR.main(['release', c2, *flags]) == 0
    e = FB.load()['builds'][-1]
    assert (e['commit'], e['build_id'], e['reused_from']) == (c2, 'a' * 24, c1) and built == []
    assert FR.main(['release', c2, *flags]) == 0
    assert len(FB.load()['builds']) == 2
    assert FR.main(['release', c3, *flags]) == 1
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
                 'pool_worker': 'X:/builds/' + 'a' * 24 + '/mtgz-worker.exe', 'pool_variants': 'gpu=fp16'}
    assert ab.published_build(s, {'lr': 1}) == {'lr': 1}
    with pytest.raises(SystemExit, match='no published farm build'):
        ab.published_build(s, {'pool_build': f'published:{c3}'})


# ------------------------------------------------------------------------------------------------ the macOS part (216)

MAC_RT = 'f' * 24
FLAGS = ['--no-slot', '--normal-priority', '--no-cuda']


def _mac_env(tmp_path, fbx, monkeypatch, release_json=None):
    """A temporary repository with c1 released (no macOS part), release.json naming a macOS pack (its manifest in the
    builds directory), the reference files present, and every step that reaches, builds on or publishes for the Mac
    replaced by a recorder (`fail[name]` makes that step raise) -> (FR, FB, commits, calls, fail, root)."""
    import farm_release as FR
    FB = fbx
    root, cs, _ = _repo(tmp_path)
    monkeypatch.setattr(FR, 'ROOT', root)
    monkeypatch.setattr(FB, 'ROOT', root)
    FB.add(_entry(cs[0], FB.rust_key(cs[0], root), cuda=None))
    b = FB.builds_dir()
    with open(os.path.join(b, 'release.json'), 'w') as f:
        # (mac_fuzz off unless a test turns it on: these tests are about the macOS part's release build; the default -
        # on since docs/ENGINE.md 219 - is test_the_macos_switches')
        json.dump(dict({'cpu_runtime': 'r' * 24, 'mac_runtime': MAC_RT, 'mac_fuzz': False}, **(release_json or {})), f)
    os.makedirs(os.path.join(b, MAC_RT))
    with open(os.path.join(b, MAC_RT, 'manifest.json'), 'w') as f:
        json.dump({'platform': 'macos-arm64', 'version': 'ort-mac'}, f)
    os.makedirs(os.path.join(b, 'reference', 'export'))
    for n in ('config.json', 'checkpoint.pt', os.path.join('export', 'model.onnx'), os.path.join('export', 'reference.npz')):
        open(os.path.join(b, 'reference', n), 'w').close()
    calls = {'reach': 0, 'checkout': [], 'build': [], 'gates': 0, 'publish': [], 'plan': 0}
    fail = {}

    def step(name):
        if name in fail:
            raise fail[name]

    def reach(self):
        calls['reach'] += 1
        step('reach')

    def build(self, profile):
        step('build')
        bid = f'{len(calls["build"])}'.rjust(24, 'm')
        calls['build'].append((profile, bid))
        return {'build_id': bid, 'dir': os.path.join(str(tmp_path), 'dist', bid), 'mac_path': f'mtgz-macfarm/builds/{bid}/mtgz-worker',
                'profile': profile}

    def gates(self, mac, fz):
        calls['gates'] += 1
        step('gates')
        return {'compat': {'passed': True}, 'records': {'passed': True}}

    def publish(self, d):
        step('publish')
        calls['publish'].append(os.path.basename(d))
        return os.path.join(self.builds, os.path.basename(d))

    def plan(self, exe, mac, fz, cuda):
        calls['plan'] += 1
        step('plan')
        return {'runs': [], 'campaigns': [], 'applied': False}

    monkeypatch.setattr(FR.Release, 'mac_reach', reach)
    monkeypatch.setattr(FR.Release, 'mac_build', build)
    monkeypatch.setattr(FR.Release, 'mac_gates', gates)
    monkeypatch.setattr(FR.Release, 'publish', publish)
    monkeypatch.setattr(FR.Release, 'mac_plan', plan)
    monkeypatch.setattr(FR.Release, 'checkout', lambda self, sha: calls['checkout'].append(sha))
    monkeypatch.setattr(FR.Release, 'modules', lambda self: None)
    return FR, FB, cs, calls, fail, root


def test_the_macos_switches():
    """On by default with its gates, publish and the fuzz build (docs/ENGINE.md 219), without the Mac's speed windows;
    release.json turns each off (or on), a flag wins for one run."""
    import farm_release as FR
    s = dict(FR.DEFAULTS)
    sw = lambda argv, s: (lambda a: (a.mac, a.mac_gates, a.mac_publish, a.mac_fuzz, a.speed_mac))(  # noqa: E731
        FR.mac_switches(FR.parser().parse_args(['release', 'x', *argv]), s))
    assert sw([], s) == (True, True, True, True, False)
    assert sw(['--no-mac-gates', '--speed-mac', '--no-mac-fuzz'], s) == (True, False, True, False, True)
    assert sw(['--no-mac'], s)[0] is False and sw(['--no-mac-publish', '--no-speed-mac'], s)[2:] == (False, True, False)
    off = dict(s, mac=False, mac_speed=True, mac_fuzz=False)
    assert sw([], off) == (False, True, True, False, True) and sw(['--mac', '--mac-fuzz'], off)[::3] == (True, True)


def test_when_an_entry_has_its_macos_part():
    import farm_release as FR
    s = {'mac_runtime': MAC_RT}
    rel = {'build_id': 'm' * 24, 'published': 'X:/b/m'}
    assert FR.mac_complete({'mac': {'release': rel, 'runtime': MAC_RT}}, s)
    assert not FR.mac_complete({'mac': None}, s) and not FR.mac_complete({}, s)
    assert not FR.mac_complete({'mac': {'failed': 'mac-reach', 'error': 'x'}}, s), 'a failure is no macOS part'
    unpub = {'mac': {'release': {'build_id': 'm' * 24}, 'runtime': MAC_RT}}
    assert not FR.mac_complete(unpub, s) and FR.mac_complete(unpub, s, publish=False)
    assert not FR.mac_complete({'mac': {'release': rel, 'runtime': 'o' * 24}}, s), 'built for another macOS pack'
    assert FR.mac_complete({'mac': {'release': rel}}, s), '(an entry from before the runtime was recorded)'
    assert not FR.mac_complete({'mac': {'release': rel}}, s, fuzz=True)
    assert FR.mac_complete({'mac': {'release': rel, 'fuzz': {'build_id': 'z' * 24}}}, s, fuzz=True)


def test_index_update_and_the_macos_build_in_describe(tmp_path, fbx):
    FB = fbx
    FB.add(_entry('1' * 40, 'k'))
    FB.add(dict(_entry('2' * 40, 'k'), reused_from='1' * 40))
    FB.add(_entry('3' * 40, 'j', bid='b' * 24))
    FB.add(dict(_entry('4' * 40, 'k'), published=False))
    mac = {'release': {'build_id': 'm' * 24, 'published': 'X:/b/m'}}
    assert FB.update('a' * 24, {'mac': mac}) == 2, 'every published entry of the build, nothing else'
    ix = FB.load()['builds']
    assert [bool(e.get('mac')) for e in ix] == [True, True, False, False]
    assert FB.update('a' * 24, {'mac': {'failed': 'x'}}, only=lambda e: not FB.mac_build(e)) == 0
    assert FB.mac_build(ix[0]) == 'm' * 24 and FB.mac_build(ix[2]) is None
    assert 'macOS build ' + 'm' * 24 in FB.describe(ix[1])
    assert '(signed, not published)' in FB.describe(dict(ix[1], mac={'release': {'build_id': 'm' * 24}}))
    assert 'NO macOS build (failed at mac-build)' in FB.describe(dict(ix[2], mac={'failed': 'mac-build'}))
    assert 'macOS' not in FB.describe(ix[2])


def test_release_backfills_the_macos_part_of_a_reused_entry_once(tmp_path, fbx, monkeypatch, capsys):
    """A commit reusing a build whose entry has no macOS part: the entry, then only the macOS part - built at the
    commit the Windows build was built at, gated, published, recorded on every entry of the build. Run again (or for
    the other commit): nothing built."""
    FR, FB, (c1, c2, c3), calls, fail, root = _mac_env(tmp_path, fbx, monkeypatch)
    assert FR.main(['release', c2, *FLAGS]) == 0
    ix = FB.load()['builds']
    assert [e['commit'] for e in ix] == [c1, c2] and ix[1]['reused_from'] == c1
    assert calls['checkout'] == [c1], 'built at built_at: one engine with the Windows build (its engine_commit)'
    mid = calls['build'][0][1]
    assert [p for p, _ in calls['build']] == ['release'] and calls['gates'] == 1 and calls['publish'] == [mid]
    for e in ix:
        m = e['mac']
        assert (m['release']['build_id'], m['runtime'], m['commit']) == (mid, MAC_RT, c1) and m['release']['published']
        assert m['gates'] == {'compat': {'passed': True}, 'records': {'passed': True}} and 'failed' not in m
    assert FB.mac_build(FB.lookup(c2, root)) == mid and 'macOS build ' + mid in FB.describe(FB.lookup(c2, root))
    out = capsys.readouterr().out
    assert json.dumps({'build_id': mid, 'published': True}) in out and 'MACOS PART FAILED' not in out
    notes = open(os.path.join(FB.builds_dir(), 'releases', c2[:12], 'notes.txt'), encoding='utf-8').read()
    assert f'macOS build {mid} published, MAC FARM GATES PASSED (compat, records)' in notes
    for c in (c2, c1, c2):
        assert FR.main(['release', c, *FLAGS]) == 0
    assert len(calls['build']) == 1 and calls['reach'] == 1 and len(FB.load()['builds']) == 2, 'idempotent'
    # --no-mac: the macOS part left alone (here: nothing to do anyway); --no-mac-publish wants no publish
    assert FR.main(['release', c2, *FLAGS, '--no-mac']) == 0 and len(calls['build']) == 1


def test_an_entry_gets_the_macos_build_another_entry_of_its_build_has(tmp_path, fbx, monkeypatch):
    """An older entry of the same build without the macOS part (a reuse before its build's own entry got one): the
    other entry's is copied, nothing reached or built."""
    FR, FB, (c1, c2, c3), calls, fail, root = _mac_env(tmp_path, fbx, monkeypatch)
    mac = {'release': {'build_id': 'x' * 24, 'published': 'X:/b/x'}, 'runtime': MAC_RT, 'commit': c1}
    assert FB.update('a' * 24, {'mac': mac}) == 1
    FB.add(dict(_entry(c2, FB.rust_key(c2, root), cuda=None), reused_from=c1))
    assert FB.lookup(c2, root).get('mac') is None
    assert FR.main(['release', c2, *FLAGS]) == 0
    assert FB.lookup(c2, root)['mac'] == mac and calls['reach'] == 0 and calls['build'] == []


def test_a_failed_macos_part_keeps_the_published_builds_and_a_later_run_backfills_it(tmp_path, fbx, monkeypatch, capsys):
    """The Mac unreachable, a build that raises, a failing gate: each recorded in the entry (no `release`, the step, the
    error), result.json and the notes, the last line loud, exit 3 - and the entry's builds stand. The Mac back: the next
    run builds only the macOS part; a plan that cannot be made is noted, not a failure."""
    FR, FB, (c1, c2, c3), calls, fail, root = _mac_env(tmp_path, fbx, monkeypatch)
    fail['reach'] = FR.Failed('mac-reach', 'the Mac does not answer over ssh (exit 255: timed out) - asleep, offline or refused')
    assert FR.main(['release', c2, *FLAGS]) == FR.EXIT_MAC_FAILED == 3
    e = FB.lookup(c2, root)
    assert (e['commit'], e['build_id'], e['published'], e['reused_from']) == (c2, 'a' * 24, True, c1), 'the entry stands'
    assert e['mac']['failed'] == 'mac-reach' and 'release' not in e['mac'] and 'asleep' in e['mac']['error']
    assert calls['build'] == [] and calls['checkout'] == []
    out = capsys.readouterr().out.strip().splitlines()
    assert out[-1].startswith('farm_release: MACOS PART FAILED at mac-reach: ') and '(exit 3)' in out[-1]
    assert 'NO macOS build (failed at mac-reach)' in out[-2] and json.loads(out[-3])['mac'] == {'failed': 'mac-reach'}
    d = os.path.join(FB.builds_dir(), 'releases', c2[:12])
    r = json.load(open(os.path.join(d, 'result.json'), encoding='utf-8'))
    assert r['mac_failed']['step'] == 'mac-reach' and r['entry']['build_id'] == 'a' * 24 and 'failed' not in r
    assert 'MACOS PART FAILED at mac-reach' in open(os.path.join(d, 'notes.txt'), encoding='utf-8').read()
    # anything a step raises (not a Failed) is the part's failure too, named by the step that ran
    del fail['reach']
    fail['build'] = RuntimeError('scp: connection lost')
    assert FR.main(['release', c2, *FLAGS]) == 3
    m = FB.lookup(c2, root)['mac']
    assert m['failed'] == 'mac-build' and 'RuntimeError: scp: connection lost' in m['error']
    # a failing Mac gate: the build is named, never published
    del fail['build']
    fail['gates'] = FR.Failed('mac-gates', 'MAC FARM GATES FAILED: {"compat": false}')
    assert FR.main(['release', c2, *FLAGS]) == 3
    m = FB.lookup(c2, root)['mac']
    assert m['failed'] == 'mac-gates' and m['built'] == {'release': calls['build'][-1][1]} and calls['publish'] == []
    assert all(e['mac']['failed'] == 'mac-gates' for e in FB.load()['builds']), 'the failure on every entry of the build'
    # the Mac back: the macOS part only, the failure replaced on every entry; the plan's own failure is only noted
    del fail['gates']
    fail['plan'] = RuntimeError('GET /v1/admin/run: 502')
    assert FR.main(['release', c2, *FLAGS]) == 0
    for e in FB.load()['builds']:
        assert e['mac']['release']['published'] and 'failed' not in e['mac'] and '502' in e['mac']['plan']['error']
    n = len(calls['build'])
    assert FR.main(['release', c2, *FLAGS]) == 0 and len(calls['build']) == n


def test_no_macos_pack_is_a_loud_failure_not_a_silent_skip(tmp_path, fbx, monkeypatch, capsys):
    FR, FB, (c1, c2, c3), calls, fail, root = _mac_env(tmp_path, fbx, monkeypatch, release_json={'mac_runtime': None})
    assert FR.main(['release', c2, *FLAGS]) == 3
    assert FB.lookup(c2, root)['mac']['failed'] == 'mac-build' and 'no mac_runtime' in FB.lookup(c2, root)['mac']['error']
    assert calls['reach'] == 0 and 'MACOS PART FAILED at mac-build' in capsys.readouterr().out
    with open(os.path.join(FB.builds_dir(), 'release.json'), 'w') as f:
        json.dump({'cpu_runtime': 'r' * 24, 'mac': False}, f)
    assert FR.main(['release', c2, *FLAGS]) == 0, '"mac": false in release.json: no macOS part'


def test_a_new_build_is_published_before_its_macos_part(tmp_path, fbx, monkeypatch, capsys):
    """A commit with new Rust inputs: its index entry (the Windows build) is written before the Mac is reached, so a
    Mac that does not answer costs only the macOS part (exit 3); the next run backfills it at that commit."""
    FR, FB, (c1, c2, c3), calls, fail, root = _mac_env(tmp_path, fbx, monkeypatch)
    real = subprocess.run

    def run(cmd, *a, **k):
        if isinstance(cmd, list) and str(cmd[0]).endswith('mtgz-worker.exe'):     # (the new worker's `info`)
            return subprocess.CompletedProcess(cmd, 0, stdout=json.dumps({'features': ['rolling'], 'zero_onnx': [1, 2]}), stderr='')
        return real(cmd, *a, **k)
    monkeypatch.setattr(FR.subprocess, 'run', run)
    monkeypatch.setattr(FR, 'preflight', lambda s, cuda=True: None)
    monkeypatch.setattr(FR.Release, 'farm_build', lambda self, step, *a: ('n' * 24, os.path.join(str(tmp_path), 'dist', 'mtg-new')))
    monkeypatch.setattr(FR.Release, 'template', lambda self, exe, bid: {'play': 1})
    monkeypatch.setattr(FR.Release, 'gates', lambda self, exe: {'passed': True, 'line': 'FARM GATES PASSED: test'})
    seen = []

    def reach(self):
        seen.append([(e['commit'], e['build_id']) for e in FB.load()['builds']])
        if 'reach' in fail:
            raise fail['reach']
    monkeypatch.setattr(FR.Release, 'mac_reach', reach)
    fail['reach'] = FR.Failed('mac-reach', 'asleep')
    assert FR.main(['release', c3, *FLAGS, '--no-speed']) == 3
    assert seen == [[(c1, 'a' * 24), (c3, 'n' * 24)]], 'the entry is in the index before the Mac is reached'
    e = FB.lookup(c3, root)
    assert e['published'] and e['build_id'] == 'n' * 24 and e['mac']['failed'] == 'mac-reach' and calls['publish'] == ['mtg-new']
    del fail['reach']
    assert FR.main(['release', c3, *FLAGS]) == 0
    assert calls['checkout'][-1] == c3 and FB.mac_build(FB.lookup(c3, root)) == calls['build'][-1][1]
    assert FB.lookup(c1, root).get('mac') is None, 'another build\'s entry untouched'


def test_the_watcher_retries_a_failed_macos_part(tmp_path, fbx, monkeypatch):
    """Exit 3: the head is released (done) with its macOS part pending - retried when due, at most --mac-retries times,
    then given up (logged); a newer head's release takes over; a retry that succeeds clears it."""
    import types
    import farm_release as FR
    import farm_release_watch as W
    assert W.MAC_FAILED == FR.EXIT_MAC_FAILED
    root, (c1, c2, c3), _ = _repo(tmp_path)
    monkeypatch.setattr(W, 'ROOT', root)
    P = W.paths()
    head, rcs, ran = [c1], [], []
    monkeypatch.setattr(W, 'head', lambda fetch=True, P=None: head[0])
    monkeypatch.setattr(W, 'release', lambda P, sha, py: (ran.append(sha), (rcs.pop(0), ['farm_release: a line']))[1])
    a = types.SimpleNamespace(no_fetch=True, python=sys.executable, mac_retry_every=1e9, mac_retries=2)
    st = {}
    rcs[:] = [3]
    W.poll(P, st, a)
    assert ran == [c1] and st['done'][c1]['mac_failed'] and st['mac_pending']['head'] == c1 and st['last']['ok']
    assert 'failed' not in st, 'released: the head is not a failed one'
    W.poll(P, st, a)
    assert ran == [c1], 'not due yet'
    a.mac_retry_every = 0
    rcs[:] = [3, 1]
    W.poll(P, st, a)
    W.poll(P, st, a)
    assert ran == [c1] * 3 and st['mac_pending']['gave_up'] and st['mac_pending']['attempts'] == 3
    W.poll(P, st, a)
    assert len(ran) == 3, 'given up: no more retries for this head'
    log = open(P['log'], encoding='utf-8').read()
    assert 'released ' + c1[:12] + ' WITHOUT ITS MACOS BUILD' in log and 'FAILED again' in log and 'FAILED (exit 1)' in log
    assert 'GIVING UP after 2 retries' in log
    head[0] = c3
    rcs[:] = [3, 0]
    W.poll(P, st, a)
    assert st['mac_pending']['head'] == c3 and st['mac_pending']['attempts'] == 1
    W.poll(P, st, a)
    assert 'mac_pending' not in st and st['done'][c3]['mac_failed'] is False and ran[-2:] == [c3, c3]
    assert 'macOS part of ' + c3[:12] + ' backfilled' in open(P['log'], encoding='utf-8').read()


# ------------------------------------------------------------------ docs/ENGINE.md 218: pool_settings' variants and macOS
def test_pool_settings_variants_and_the_macos_alternate(tmp_path, fbx, monkeypatch):
    """pool_settings names the variant each GPU alternate plays (CUDA 'gpu=fp16', DirectML 'dml=matmul@amd', none
    without them) and lists the release's macOS build last - only when it is published and the run's settings pass the
    pool's own platform check (zero.farm_pool.platform_check, run by the release worker here); left out, it says why."""
    from zero import farm_pool as FP
    FB = fbx
    worker = tmp_path / 'mtgz-worker.exe'
    worker.write_bytes(b'stand-in')
    mac = 'f' * 24
    e = dict(_entry('c' * 40, 'k'), worker=str(worker), dml={'build_id': 'm' * 24},
             mac={'release': {'build_id': mac, 'published': True}})
    cfg = {'game': 'mtg', 'sims': 64}
    seen = []

    def check(w, c, d, platform, log=None):
        seen.append((w, c, platform, os.path.isdir(d)))
    monkeypatch.setattr(FP, 'platform_check', check)
    notes = []
    ps = FB.pool_settings(e, cfg=cfg, log=notes.append)
    assert ps['pool_alternates'] == ','.join(['c' * 24, 'm' * 24, mac]) and ps['pool_variants'] == 'gpu=fp16,dml=matmul@amd'
    assert seen == [(str(worker), cfg, 'macos-arm64', True)] and notes == []
    # no run settings (lookup --pool-args, a launcher that has none): no macOS alternate, said
    ps = FB.pool_settings(e, log=notes.append)
    assert ps['pool_alternates'] == 'c' * 24 + ',' + 'm' * 24 and 'no run settings' in notes[-1]

    def refuse(w, c, d, platform, log=None):
        raise FP.Refused(f'the run\'s settings would replay differently on {platform} (the platform\'s libm): exp in x',
                         'leave the macos-arm64 alternate out of pool_alternates')
    monkeypatch.setattr(FP, 'platform_check', refuse)
    ps = FB.pool_settings(e, cfg=cfg, log=notes.append)
    assert mac not in ps['pool_alternates'] and 'replay differently on macos-arm64' in notes[-1]
    monkeypatch.setattr(FP, 'platform_check', check)
    # not published / failed / no worker here: left out, said
    for m, why in (({'release': {'build_id': mac, 'published': False}}, 'signed, not published'),
                   ({'failed': 'mac-reach'}, 'failed at mac-reach'), (None, 'no published macOS build')):
        ps = FB.pool_settings(dict(e, mac=m), cfg=cfg, log=notes.append)
        assert mac not in ps['pool_alternates'] and why in notes[-1], (m, notes[-1])
    ps = FB.pool_settings(dict(e, worker=str(tmp_path / 'nowhere.exe')), cfg=cfg, log=notes.append)
    assert mac not in ps['pool_alternates'] and 'is not on this machine' in notes[-1]
    # the variants follow the GPU alternates
    assert FB.pool_settings(dict(e, dml=None, mac=None))['pool_variants'] == 'gpu=fp16'
    assert FB.pool_settings(dict(e, cuda=None, mac=None))['pool_variants'] == 'dml=matmul@amd'
    assert 'pool_variants' not in FB.pool_settings(dict(e, cuda=None, dml=None, mac=None))
    assert set(FB.pool_settings(e, alternates=False)) == {'pool_build', 'pool_runtime', 'pool_worker'}


def test_pool_build_published_checks_the_runs_settings_for_macos(tmp_path, fbx, monkeypatch, capsys):
    """`--pool-build published` resolves the run's settings as zero.run will (its config.json < the flags) for the
    macOS platform check, and adds the macOS build only when they pass; the variants and the reason are printed."""
    from zero import farm_pool as FP
    FB = fbx
    root, (c1, c2, c3), g = _repo(tmp_path)
    worker = tmp_path / 'w.exe'
    worker.write_bytes(b'x')
    FB.add(dict(_entry(c3, FB.rust_key(c3, root)), worker=str(worker), mac={'release': {'build_id': 'f' * 24,
                                                                                         'published': True}}))
    run = tmp_path / 'run'
    run.mkdir()
    (run / 'config.json').write_text(json.dumps({'preset': 'mtg-smoke', 'sims': 12}))
    seen = []
    monkeypatch.setattr(FP, 'platform_check', lambda w, c, d, p, log=None: seen.append(c))
    out = FB.published_pool(['--run', str(run), '--pool-build', 'published', '--eval-sims', '6'], root)
    assert seen and seen[0]['sims'] == 12 and seen[0]['eval_sims'] == 6 and seen[0]['preset'] == 'mtg-smoke'
    alts = out[out.index('--pool-alternates') + 1]
    assert alts == 'c' * 24 + ',' + 'f' * 24 and out[out.index('--pool-variants') + 1] == 'gpu=fp16'

    def refuse(w, c, d, p, log=None):
        raise FP.Refused('the run\'s settings would replay differently on macos-arm64: exp', 'leave it out')
    monkeypatch.setattr(FP, 'platform_check', refuse)
    out = FB.published_pool(['--run', str(run), '--pool-build', 'published'], root)
    assert out[out.index('--pool-alternates') + 1] == 'c' * 24
    assert 'no macOS alternate - macOS build ' + 'f' * 24 + ": the run's settings would replay differently" in \
        capsys.readouterr().out


def test_the_farm_build_hook_and_the_runs_start(tmp_path, fbx, monkeypatch):
    """mtgz's `farm_build` hook (zero-core docs/az.md 70): the published build of this checkout's commit, described with
    how to use it, else None; a fresh start without a farm pool and without a reason is then refused, a resumed one
    warns, a reason (or $ZERO_SELFPLAY_LOCAL_REASON, which the test suite sets) lets it start."""
    from zero import package, run as R
    FB = fbx
    root, (c1, c2, c3), g = _repo(tmp_path)
    monkeypatch.setattr(FB, 'ROOT', root)
    assert FB.published_for_run(str(tmp_path), {}) is None
    FB.add(_entry(c3, FB.rust_key(c3, root)))
    d = FB.published_for_run(str(tmp_path), {})
    assert d.startswith(f'commit {c3[:12]}: build ' + 'a' * 24) and '--pool-build published' in d
    assert package.active().published_farm_build(str(tmp_path), {}) == d
    monkeypatch.delenv(R.LOCAL_REASON_ENV, raising=False)
    cfg = dict(R.DEFAULTS, game='mtg')
    with pytest.raises(R.FarmUseRefused, match='build ' + 'a' * 24):
        R.check_farm_use(str(tmp_path), cfg, True, lambda m: None)
    lines = []
    R.check_farm_use(str(tmp_path), cfg, False, lines.append)
    assert lines[-1].startswith('WARNING: a published farm build exists for this commit')
    R.check_farm_use(str(tmp_path), dict(cfg, selfplay_local_reason='a timing A/B'), True, lines.append)
    assert ': a timing A/B - beside a published farm build of this commit (commit ' in lines[-1]
    assert R.PRESETS['mtg-smoke']['selfplay_local_reason'] and R.PRESETS['delver-smoke']['selfplay_local_reason']


def test_the_mac_gates_name_the_commit_for_the_local_fuzz_build(tmp_path, monkeypatch):
    """The local fuzz build (a bare cargo build, no manifest.json) gets this commit through ZERO_FARM_ENGINE_COMMIT, so
    its failures' repros say "(at <sha>)" like the Mac's published build - farm_gates part (d) compares them; without it
    the desktop said "(at unknown)" and the macOS part of 50205980's release failed on four identical faults."""
    import farm_release as FR
    import farm_gates as G
    sha = 'ab' * 20
    r = object.__new__(FR.Release)
    r.s = {'python': 'py', 'reference': {'config': 'c.json', 'export': str(tmp_path)}}
    r.log_dir, r.wt = str(tmp_path), str(tmp_path)
    seen = {}

    def run(self, step, cmd, cwd=None, env=None, **kw):
        seen.update(step=step, cmd=cmd, env=env)
        os.makedirs(os.path.join(self.log_dir, 'mac-gates'), exist_ok=True)
        with open(os.path.join(self.log_dir, 'mac-gates', 'summary.json'), 'w', encoding='utf-8') as f:
            json.dump({'passed': True, 'parts': {}}, f)
    monkeypatch.setattr(FR.Release, 'run', run)
    monkeypatch.setattr(FR.Release, 'local_fuzz_worker', lambda self: 'fz.exe')
    monkeypatch.setattr(FR.Release, 'mac_runtime_name', lambda self: ('rt', 'pack'))
    monkeypatch.setattr(FR, 'git', lambda *a, **k: sha if a[:2] == ('rev-parse', 'HEAD') else '')
    r.mac_gates({'mac_path': 'm/w'}, {'mac_path': 'm/fz'})
    assert seen['env']['ZERO_FARM_ENGINE_COMMIT'] == sha and seen['env']['CUDA_VISIBLE_DEVICES'] == ''
    assert seen['cmd'][seen['cmd'].index('--fuzz-worker') + 1] == 'fz.exe'
    # what part (d) then compares: both sides' repros normalised to the same text
    fin = lambda at: {'mode': 'games', 'failures': [{'seed': 505, 'site': 'crash exit -6', 'panic': None,  # noqa: E731
                                                     'repro': f'(at {at}) FUZZ_FIRST=505 cargo test'}]}
    assert G.fault_failures(fin(sha[:12]))['505']['repro'] == G.fault_failures(fin(sha))['505']['repro']
    assert G.fault_failures(fin('unknown'))['505']['repro'] != G.fault_failures(fin(sha))['505']['repro']
