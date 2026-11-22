"""tools/gate.py: the optin-core build-only step (docs/ENGINE.md 90) names the other games explicitly
(found beside the main checkout from any worktree), requires an "ok <game>: build" line for each, and
never shares this repo's cargo target dir; and the gate never passes a check it did not perform
(ENGINE.md 91): a missing essential input refuses (exit 2) unless the caller opts out by name, an
opted-out step is named in the final line, a digest without an expected value is never PASSED, every
checker wants its positive marker, pytest's FINAL summary line decides, and the core pytest's flaky
policy retries only tests/test_remoteplay.py failures, alone. Nothing is built here: commands are
stubbed (gate.execute), except one tiny python probe for the environment."""
import json
import os
import sys
import time
import types

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import gate as G  # noqa: E402

GAMES = ('glassroad-zero', 'caverna-zero', 'kingdombuilder-zero')
PIN = 'aa10733df214c6464ca5c940018b9b41712b54c1'
SHA = '276f948e6d31' + 'ab' * 14
D_FULL = 'f6543eb2b1b8752f'
D_FREE = '0123456789abcdef'

# what the gate's step printed on e098aff (smoke_all found no game from the worktree's submodule)
NO_GAMES = """=== core: cargo test
test result: ok. 123 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.54s

=== smoke_all summary (core aa10733)
  ok    core: cargo test (4 s)
  ok    core: build zero._engine (17 s)
  ok    core: pytest (703 s)
GREEN
"""


def built(names, core='aa10733', fail=(), counted=True):
    lines = [f'=== {n}: worktree E:/zero-smoke/x/20261104-082647-{n}-wt with zero-core {core}' for n in names]
    lines += ['', '=== smoke_all summary (core aa10733)']
    for n in names:
        lines.append(f'  {"FAIL" if n in fail else "ok  "}  {n}: build {n[:2]}z._engine (30 s)')
        lines.append(f'  skip  {n}: cargo test, pytest, smoke run (--build-only)')
    if counted:
        lines.append(f'  games: {len(names)} of {len(names)} checked ({" ".join(names)})')
    lines.append(f'{len(fail)} step(s) FAILED' if fail else 'GREEN')
    return '\n'.join(lines) + '\n'


def test_the_checker_wants_every_game_built():
    check = G.make_build_only_checker(list(GAMES), PIN)
    ok, reason, _ = check(NO_GAMES, 0)
    assert not ok and 'glassroad-zero, caverna-zero, kingdombuilder-zero' in reason and 'built: none' in reason
    assert check(built(GAMES), 0)[0]
    ok, reason, _ = check(built(GAMES[:2]), 0)
    assert not ok and 'kingdombuilder-zero' in reason
    assert not check(built(GAMES, fail=('caverna-zero',)), 1)[0]
    assert not check(built(GAMES), 1)[0]                       # every game built, but some step failed
    ok, reason, _ = check(built(GAMES, core='1234567'), 0)     # built against another core
    assert not ok and 'with zero-core aa10733df214' in reason
    ok, reason, _ = check(built(GAMES, counted=False), 0)      # no "games: 3 of 3 checked" summary line
    assert not ok and 'games: 3 of 3 checked' in reason


@pytest.fixture
def fake_main(tmp_path, monkeypatch):
    """This checkout as a worktree whose main checkout is tmp/mtg-zero (its .git the common dir)."""
    monkeypatch.setattr(G, 'ROOT', str(tmp_path / 'mtg-zero-wt' / 'gate'))
    monkeypatch.setattr(G, 'git', lambda args, cwd=None, check=True: str(tmp_path / 'mtg-zero' / '.git')
                        if '--git-common-dir' in args and cwd is None else '')
    for g in GAMES:
        (tmp_path / g / 'third_party' / 'zero-core').mkdir(parents=True)
    return tmp_path


def test_default_games_are_found_beside_the_main_checkout(fake_main):
    assert G.default_other_games() == [str(fake_main / g) for g in GAMES]
    os.rename(fake_main / GAMES[2], fake_main / 'moved')
    with pytest.raises(G.GateError, match='kingdombuilder-zero'):
        G.default_other_games()


def test_the_step_names_the_games_and_has_its_own_targets(fake_main, tmp_path):
    ctx = types.SimpleNamespace(core_pin=PIN)
    step = G.build_only_step(ctx, types.SimpleNamespace(other_games=None))
    assert step.argv[-3:] == [str(fake_main / g) for g in GAMES] and '--with-this-core' in step.argv
    assert step.env['SKIP_CORE'] == '1' and step.env['SMOKE_TARGET'] == 'mtgz-gate-gate'
    assert step.env['SMOKE_TARGET'] in step.env['SMOKE_DIR'] and step.env['CARGO_TARGET_DIR'] is None
    with pytest.raises(G.GateError, match='no such checkout'):
        G.build_only_step(ctx, types.SimpleNamespace(other_games=[str(tmp_path / 'nope')]))


def probe_ctx(tmp_path, base_env):
    return types.SimpleNamespace(logs_dir=str(tmp_path), sha_short='0' * 12, tier='t', root=str(tmp_path),
                                 step_timeout=60, base_env=base_env)


def test_a_none_env_value_unsets_the_variable(tmp_path):
    ctx = probe_ctx(tmp_path, {'CARGO_TARGET_DIR': 'E:/shared', 'SMOKE_X': 'a'})
    probe = [sys.executable, '-c', 'import os; print("ctd=%s x=%s" % (os.environ.get("CARGO_TARGET_DIR"), '
             'os.environ.get("SMOKE_X")))']
    seen = {}
    step = G.Step('probe', probe, env={'CARGO_TARGET_DIR': None},
                  checker=lambda out, rc: (seen.setdefault('out', out) is not None, '', {}))
    assert G.run_step(ctx, step).passed
    assert 'ctd=None x=a' in seen['out']


def test_the_callers_digest_pytest_path_and_unpinned_settings_never_reach_a_step(tmp_path, monkeypatch):
    for k, v in (('DIGEST_WRITE', '1'), ('DIGEST_RANDOM', '10'), ('PYTEST_ADDOPTS', '-k nothing'),
                 ('PYTHONPATH', 'C:/elsewhere'), ('ZERO_CORE_UNPINNED', '1')):
        monkeypatch.setenv(k, v)
    names = ('DIGEST_WRITE', 'DIGEST_RANDOM', 'DIGEST_ENCODING', 'PYTEST_ADDOPTS', 'PYTHONPATH', 'ZERO_CORE_UNPINNED')
    probe = [sys.executable, '-c', 'import os; print(" ".join(k + "=" + str(os.environ.get(k)) for k in '
             + repr(names) + '))']
    seen = {}
    step = G.Step('probe', probe, env={'DIGEST_ENCODING': '0'},       # a step's own setting stays
                  checker=lambda out, rc: (seen.setdefault('out', out) is not None, '', {}))
    assert G.run_step(probe_ctx(tmp_path, {}), step).passed
    assert seen['out'].split() == ['DIGEST_WRITE=None', 'DIGEST_RANDOM=None', 'DIGEST_ENCODING=0',
                                   'PYTEST_ADDOPTS=None', 'PYTHONPATH=None', 'ZERO_CORE_UNPINNED=None']


# ---------------------------------------------------------------------------- real-format outputs

def BUILD(root):
    return ('   Compiling mtgz_py v0.1.0 (py)\n    Finished `release` profile [optimized + debuginfo] target(s) in 1m 17s\n'
            f"mtgz._engine OK: {os.path.join(root, 'mtgz', '_engine.pyd')} | games: ['toy', 'mtg', 'team_draft']\n")


def CORE_BUILD(core):
    return (f'    Finished `release` profile [optimized] target(s) in 17s\n'
            f"zero._engine OK: {os.path.join(core, 'python', 'zero', '_engine.pyd')} | games: ['toy']\n")


def DIGEST(d, games=(2000, 500)):
    return (f'running 1 test\n{games[0]} random + {games[1]} heuristic games in 12.1 s; combined digest {d}\n'
            'test behaviour_digests ... ok\n\n'
            'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 12.06s\n')


D_FORMATS = {'constructed': '63f58560ad7b3df0', 'eoe': 'b36df9098a38d794', 'driver': '837d42c28214befe',
             'metagame': '0123456789abcdef'}


def FORMATS(digests=None, passed=4):
    """digest_formats' output as the four tests print it (ENGINE.md 148.1, 176), in the order they finished."""
    d = dict(D_FORMATS, **(digests or {}))
    return ('running 4 tests\n'
            'driver (8 threads, 2 groups of 8 games): 20000 batches, 90491 rows, 1251 samples (the first at batch 7532), '
            '5 results (the first at batch 12020); sims 85674, leaves 90491, cache hits 51316, view rows 10061, belief '
            'rows 2159, busy 4.1 s (47.9 us a simulation)\n'
            f'driver: 20000 batches, 1251 samples, 5 results in 5.2 s; combined digest {d["driver"]}\n'
            'test driver_digest ... ok\n'
            f'constructed: 570 games and matches in 27.9 s; combined digest {d["constructed"]}\n'
            'test constructed_digests ... ok\n'
            f'metagame: 225 best-of-one games and 63 best-of-three matches in 20.1 s; combined digest {d["metagame"]}\n'
            'test metagame_digests ... ok\n'
            f'eoe: 160 games in 38.4 s; combined digest {d["eoe"]}\n'
            'test eoe_digests ... ok\n\n'
            f'test result: ok. {passed} passed; 0 failed; 0 ignored; 0 measured; {30 - passed} filtered out; '
            'finished in 38.43s\n')


D_DEALT = '8cb56319b1746be3'


def DEALT(d=D_DEALT, n=80):
    """dealt_decks.rs::dealt_digests' output (ENGINE.md 154), the binary's ten other tests filtered out."""
    return ('running 1 test\n'
            f'dealt_digests: {n} matches in 2.8 s; combined digest {d}\n'
            'test dealt_digests ... ok\n\n'
            'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 10 filtered out; finished in 2.83s\n')


def CARGO(*counts, failed=0):
    return ''.join(f'running {n} tests\n\ntest result: {"FAILED" if failed else "ok"}. {n} passed; {failed} failed; '
                   f'0 ignored; 0 measured; 0 filtered out; finished in 4.57s\n\n' for n in counts)


PYTEST_OK = '.....................................................................    [100%]\n69 passed in 102.45s (0:01:42)\n'


def COMPAT(old, new, samples=1114, identical=True):
    side = lambda tag, root: (f'{tag}: {root} (63.8 s; engine {os.path.join(root, "mtgz", "_engine.pyd")})\n'
                              f'  visits: {samples} samples, records 6bb82850b165d917, results 6895e1adaf287176\n'
                              f'  cq: {samples} samples, records caf64df11fd4e38e, results 6895e1adaf287176\n'
                              '  vs random: 7.0/16, results 56a12cd987281708\n')
    res = {'game': 'mtg', 'spec': '{"max_turns": 60}', 'evaluator': 'random', 'identical': identical,
           'differences': [] if identical else ['targets.cq: a != b']}
    return side('old', old) + side('new', new) + json.dumps(res) + '\n'


def SMOKE(n, bad=0):
    return f'ok    seed 0 random: 400 games (0 unfinished), 159784 decisions\nsearch smoke: {bad} failed of {n} configurations, 107 s\n'


def MAC(sha, body, after=None):
    return (f'[mac] mac-host somebranch @ {sha}: cargo test --release --no-fail-fast\n' + body
            + f'[mac] done @ {after or sha} (exit 0)\n')


def EVAL_CHECKOUT(sha):
    wt = f'/Users/user/mtg-zero-wt/run-{sha[:12]}'
    return (f'[mac] mac-host run-{sha[:12]} @ {sha}: set -euo pipefail\nbash tools/build_py.sh\n'
            f"    Finished `release` profile [optimized + debuginfo] target(s) in 41.12s\n"
            f"mtgz._engine OK: {wt}/mtgz/_engine.so | games: ['toy', 'mtg', 'team_draft']\n"
            f'mtgz remote: {wt} is {sha[:12]}, unchanged, its engine built there\neval checkout ready: {wt}\n'
            f'[mac] done @ {sha} (exit 0)\n')


H2H = ('# batch {"games": 4, "unfinished": 0}\n'
       '{"a": "x.pt", "b": "x.pt", "game": "mtg", "sims": 16, "games": 8, "a_score": 0.5, "unfinished": 0}\n')

REMOTEPLAY = 'tests/test_remoteplay.py::test_cotrain_the_local_driver_is_never_starved'


def PYTEST_FAILED(*ids, passed=301, errors=()):
    lines = ['....F...                                                                 [100%]',
             '================================== FAILURES ===================================',
             '___________________________ test_x ___________________________',
             'E   AssertionError: starved',
             '=========================== short test summary info ===========================']
    lines += [f'FAILED {i} - AssertionError: starved' for i in ids] + [f'ERROR {i} - RuntimeError' for i in errors]
    counts = [f'{len(ids)} failed'] if ids else []
    counts += [f'{passed} passed', '3 skipped', '1 warning']
    if errors:
        counts.append(f'{len(errors)} error' + ('s' if len(errors) > 1 else ''))
    return '\n'.join(lines + [', '.join(counts) + ' in 881.28s (0:14:41)']) + '\n'


WORKER_BUILT = '   Compiling mtgz_worker v0.1.0\n    Finished `release` profile [optimized] target(s) in 61.20s\n'


def FUZZ_SHARDS(pool, games, hidden=False, passed=True, matches_skipped=None):
    """tools/fuzz_shards.py's last two lines (ENGINE.md 167)."""
    checked = [f'games over {games}'] + ([] if hidden or matches_skipped else [f'matches over {max(20, games // 20)}'])
    s = {'pool': pool, 'hidden': hidden, 'first': 0, 'games': games, 'matches': 0 if hidden else max(20, games // 20),
         'shards': 8, 'passed': passed, 'seconds': 301.0, 'slowest_shard_s': 280.0, 'checked': checked, 'rest_passed': 8,
         'post': None, 'matches_skipped': matches_skipped, 'logs': 'gates/tmp/fz', 'problems': [] if passed else ['x']}
    line = (f'fuzz shards: PASSED - {pool}: {games} games, ...; merged checks: {", ".join(checked)}; 8 other fuzz tests'
            if passed else f'fuzz shards: FAILED - {pool}: games[0,250): exit 101: seed 17 failed (rerun: FUZZ_FIRST=17 ...)')
    return line + '\n' + json.dumps({'fuzz_shards': s}) + '\n'
FARM_SUMMARY = {'preset': 'delver-mirror-bo3-dealt', 'games': 6, 'passed': True,
                'gate2': {'passed': True, 'result': {'games': 6, 'rows': 1210, 'batches_compared': 150, 'identical': True}},
                'gate3': {'passed': True, 'result': {'games': 6, 'rows': 1190, 'setups': 6, 'identical': True}},
                'features': {'features': ['rolling'], 'build_id': 'a' * 24, 'missing': [], 'passed': True}}
FARM_COMPAT_OK = (json.dumps(FARM_SUMMARY) + '\nfarm compat: PASSED - delver-mirror-bo3-dealt: gate 2 (worker vs the Python '
                  'remote worker) identical: 6 games, 1210 rows, 150 batches; gate 3 (records -> trainer rows) identical: '
                  '6 games, 1190 rows, 6 decks compared; features rolling announced (build aaaa) (gates/tmp/x)\n')


# ---------------------------------------------------------------------------- the fake gate

MAC_MODULE = "mtgz._engine OK: /Users/someone/mtg-zero-wt/detached-ccc/mtgz/_engine.so | games: ['toy', 'mtg']\n"

class Fake:
    """gate.execute stubbed: a canned real-format output per kind of step (override with `set`), a queue
    of scripted (output, rc) answers for the core pytest, and every call recorded."""

    def __init__(self, core):
        self.core = core
        self.calls = []
        self.over = {}
        self.core_pytest = []

    def set(self, kind, out, rc=0):
        self.over[kind] = (out, rc)

    def kind(self, argv, env):
        cmd = ' '.join(argv)
        if argv[1:2] == ['-c'] and 'py_compile' in argv[2]:     # (its file list names every tool)
            return 'pycompile'
        if 'mac.sh' in cmd:
            return 'mac-eval' if '--eval-checkout' in argv else 'mac'
        if 'zcache.py' in cmd:                           # (the artifact cache's fetch-or-build: ENGINE.md 167)
            return 'core-build' if 'core-pyd' in argv else 'worker-build'
        if 'fuzz_shards.py' in cmd:
            return 'fuzz-shards'
        if '--no-run' in argv:
            return 'test-build'
        if 'build_py.sh' in cmd:
            return 'core-build' if 'third_party' in cmd or self.cwd == self.core else 'build'
        if 'smoke_all.sh' in cmd:
            return 'build-only'
        if 'mtgz_worker' in argv:
            return 'worker-build'
        if 'farm_compat.py' in cmd:
            return 'farm-compat'
        if 'check_checkpoint_compat.py' in cmd:
            return 'compat'
        if 'search_smoke.py' in cmd:
            return 'smoke'
        if 'mtgz.az.run' in cmd:
            return 'run'
        if 'mtgz.az.h2h' in cmd:
            return 'h2h'
        if 'pytest' in argv:
            return 'core-pytest' if self.cwd == self.core else 'pytest'
        if '--test' in argv and 'digest_formats' in argv:
            return 'formats'
        if '--test' in argv and 'dealt_decks' in argv:
            return 'dealt'
        if '--test' in argv and 'digest' in argv:
            return 'digest0' if env.get('DIGEST_ENCODING') == '0' else 'digest'
        if '--test' in argv:
            return 'cargo1'
        return 'cargo'

    def __call__(self, argv, cwd, env, timeout):
        self.cwd = cwd
        k = self.kind(argv, env)
        self.calls.append((k, list(argv)))
        if k == 'core-pytest' and self.core_pytest:
            return self.core_pytest.pop(0)
        if k in self.over:
            return self.over[k]
        if k == 'mac' and 'tools/slotd.sh' in argv[-1]:           # an offloaded step (ENGINE.md 166): its Mac output
            inner = argv[-1]
            if 'tools/build_py.sh' in inner and G.MAC_BUILT not in inner:   # (the module built there once: ENGINE.md 238)
                return MAC(SHA, MAC_MODULE), 0
            if '--no-run' in inner:                                   # (the test binaries built there: ENGINE.md 167)
                return MAC(SHA, '    Finished `release` profile [optimized + debuginfo] target(s) in 1.20s\n'), 0
            if 'fuzz_shards.py' in inner:
                pool = inner.split('--pool ')[1].split()[0]
                games = int(inner.split('--games ')[1].split()[0])
                return MAC(SHA, FUZZ_SHARDS(pool, games, '--hidden' in inner)), 0
            if 'mtgz.az.run' in inner:
                rows = [json.dumps({'iter': i, 'selfplay': {'games': 16, 'unfinished': 0}}) for i in range(2)]
                return MAC(SHA, 'iter 1 done\n' + ''.join(G.METRICS_PREFIX + r + '\n' for r in rows)), 0
            if 'pytest' in inner:
                return MAC(SHA, PYTEST_OK), 0
            if 'search_smoke.py' in inner:
                return MAC(SHA, SMOKE(len(inner.split('--specs ')[1].split()[0].split(',')))), 0
            if 'digest_formats' in inner:
                return MAC(SHA, FORMATS()), 0
            if 'dealt_decks' in inner:
                return MAC(SHA, DEALT()), 0
            if '--test digest' in inner:
                return MAC(SHA, DIGEST(D_FREE if 'DIGEST_ENCODING=0' in inner else D_FULL)), 0
            if '--test fuzz' in inner:
                return MAC(SHA, FUZZ_OUT), 0
            return MAC(SHA, CARGO(34) if '--test' in inner else CARGO(123, 3, 0, 0)), 0
        if k == 'run':                                    # the run-loop smoke writes its metrics
            run = argv[argv.index('--run') + 1]
            os.makedirs(run, exist_ok=True)
            with open(os.path.join(run, 'metrics.jsonl'), 'w', encoding='utf-8') as f:
                for i in range(2):
                    f.write(json.dumps({'iter': i, 'selfplay': {'games': 16, 'unfinished': 0}}) + '\n')
            return 'iter 1 done\n', 0
        if k == 'smoke':
            return SMOKE(len(argv[argv.index('--specs') + 1].split(','))), 0
        if k == 'compat':
            return COMPAT(argv[argv.index('--old') + 1], argv[argv.index('--new') + 1]), 0
        if k == 'pycompile':
            return f'py_compile: {len(argv) - 3} file(s) compiled\n', 0
        if k == 'worker-build':                           # cargo writes the executable
            os.makedirs(os.path.dirname(self.worker), exist_ok=True)
            with open(self.worker, 'wb') as f:
                f.write(b'worker')
            return WORKER_BUILT, 0
        if k == 'farm-compat':
            return FARM_COMPAT_OK, 0
        if k == 'test-build':
            return '    Finished `release` profile [optimized + debuginfo] target(s) in 1.20s\n', 0
        if k == 'fuzz-shards':
            return FUZZ_SHARDS(argv[argv.index('--pool') + 1], int(argv[argv.index('--games') + 1]), '--hidden' in argv), 0
        return {'build': BUILD(G.ROOT), 'core-build': CORE_BUILD(self.core), 'digest0': DIGEST(D_FREE),
                'digest': DIGEST(D_FULL), 'formats': FORMATS(), 'dealt': DEALT(), 'cargo1': CARGO(34), 'cargo': CARGO(123, 3, 0, 0), 'pytest': PYTEST_OK,
                'core-pytest': '.' * 60 + '\n308 passed, 3 skipped, 1 warning in 739.67s (0:12:19)\n',
                'h2h': H2H, 'build-only': built(GAMES, core=PIN[:7]), 'mac': MAC(SHA, CARGO(900, 12, 0)),
                'mac-eval': EVAL_CHECKOUT(SHA)}[k], 0


@pytest.fixture
def gate(tmp_path, monkeypatch):
    """main() against a fake checkout context (no git, a clean tree, the report and logs in tmp) and a
    stubbed gate.execute; returns run(argv) -> (exit status, stdout, stderr, the report or None)."""
    core = tmp_path / 'core'
    (core / '.git').mkdir(parents=True)
    monkeypatch.setattr(G, 'CORE', str(core))
    monkeypatch.setattr(G, 'GATES_DIR', str(tmp_path / 'gates'))
    monkeypatch.setattr(G, 'tracked_changes', lambda: [])
    monkeypatch.setattr(G, 'untracked_files', lambda: [])
    baseline = tmp_path / 'merge-main'
    (baseline / 'mtgz').mkdir(parents=True)
    (baseline / 'mtgz' / '_engine.pyd').write_bytes(b'old engine')

    def fake_git(args, cwd=None, check=True):
        if args[:1] == ['rev-parse'] and args[-1] == 'HEAD':
            return 'c' * 40 if cwd else SHA                  # the baseline is another commit
        if 'HEAD:third_party/zero-core' in args:
            return PIN
        if args[:2] == ['rev-parse', '--verify']:
            return args[-1].split('^')[0].ljust(40, '0')
        if args[:2] == ['diff', '--name-only']:
            return fake.diff
        return ''
    monkeypatch.setattr(G, 'git', fake_git)

    def fake_ctx(args):
        return types.SimpleNamespace(tier=args.tier, root=G.ROOT, jobs=2, step_timeout=60, sha=SHA, sha_short=SHA[:12],
                                     core_pin=PIN, core_head=PIN, target_dir=str(tmp_path / 'target'),
                                     logs_dir=str(tmp_path / 'gates' / 'logs'), base_env={'CUDA_VISIBLE_DEVICES': '-1'},
                                     no_mac=args.no_mac, dirty=[], baseline=None, slot_wait=args.slot_wait,
                                     throttle=False, placement=None,
                                     core_pytest_shards=args.core_pytest_shards)
    monkeypatch.setattr(G, 'Ctx', fake_ctx)
    fake = Fake(str(core))
    # placement (ENGINE.md 166): no live run and no Mac unless a test says so (never this machine's real state)
    fake.live, fake.mac = [], (False, 'the Mac: stubbed off')
    monkeypatch.setattr(G, 'live_runs', lambda: list(fake.live))
    monkeypatch.setattr(G, 'mac_probe', lambda *a, **k: fake.mac)
    # the privacy check (tools/privacy_check.py) passes unless a test says otherwise (its own test runs the real one)
    fake.privacy = (True, 'privacy check: 1 file(s), 0 finding(s) (stubbed)')
    monkeypatch.setattr(G, 'privacy_scan', lambda root: fake.privacy)
    fake.worker = str(tmp_path / 'target' / 'release' / G.WORKER_EXE)
    fake.diff = 'docs/ENGINE.md\n'
    monkeypatch.setattr(G, 'execute', fake)
    other = [str(tmp_path / g) for g in GAMES]
    for p in other:
        os.makedirs(p)

    def run(*argv, capsys=None):
        rc = G.main(list(argv))
        out, err = capsys.readouterr() if capsys else ('', '')
        rpath = os.path.join(str(tmp_path / 'gates'), f'{SHA[:12]}-{argv[0]}.json')
        report = json.load(open(rpath, encoding='utf-8')) if os.path.exists(rpath) else None
        if report:
            os.remove(rpath)
        return rc, out, err, report
    run.fake, run.baseline, run.other, run.tmp = fake, str(baseline), other, tmp_path
    return run


def suite_report(gates_dir, sha, over=None):
    """A suite tier report of `sha` as gate.py writes it (gates/<sha12>-suite.json), its logs written beside it."""
    logs = os.path.join(gates_dir, 'logs')
    os.makedirs(logs, exist_ok=True)
    steps = []
    for sid, extra in (('cargo-test-engine', {}), ('fuzz-default', {'fuzz_games': 20000})):
        log = os.path.join(logs, f'{sha[:12]}-suite-{sid}.log')
        with open(log, 'w', encoding='utf-8') as f:
            f.write(f'{sid} output\n')
        steps.append({'id': sid, 'status': 'passed', 'log': log, 'log_sha256': G.sha256_file(log), **extra})
    r = {'tier': 'suite', 'sha': sha, 'zero_core_pin': PIN, 'passed': True, 'clean': True, 'started': '2026-11-12T10:00',
         'final': f'GATE PASSED suite {sha[:12]}', 'steps': steps}
    r.update(over or {})
    path = os.path.join(gates_dir, f'{sha[:12]}-suite.json')
    with open(path, 'w', encoding='utf-8') as f:
        json.dump(r, f)
    return path


def final(out):
    return [line for line in out.splitlines() if line.startswith('GATE ')][-1]


ENC = ('encoding', '--expect-digest', D_FULL, '--expect-digest0', D_FREE)


# ---------------------------------------------------------------------------- rule 1: essential inputs, named skips

@pytest.mark.parametrize('argv, needs', [
    (ENC, '--checkpoint'),
    (('encoding', '--expect-digest', D_FULL, '--checkpoint', __file__), '--expect-digest0'),
    (('engine', '--expect-digest', D_FULL, '--expect-digest0', D_FREE), '--tested-sha'),
    (('search-core',), '--baseline-root'),
    (('optin-core',), '--baseline-root'),
    (('repin', '--spec', 'delver', '--preset', 'delver-mirror'), '--expect-digest'),
    (('repin', '--expect-digest', D_FULL), '--spec, --preset'),
])
def test_a_missing_essential_input_refuses_before_anything_runs(gate, capsys, argv, needs):
    for dry in ((), ('--dry-run',)):
        rc, out, err, report = gate(*argv, *dry, capsys=capsys)
        assert rc == 2 and needs in err and report is None and not gate.fake.calls
        assert 'GATE' not in out


@pytest.mark.parametrize('argv, says', [
    (('docs', '--baseline-root', '../x'), '--baseline-root is not used by the docs tier'),
    (('repin', '--expect-digest', D_FULL, '--spec', 'd', '--preset', 'p', '--record-digest', 'full'),
     '--record-digest is not used by the repin tier'),
    (('repin', '--expect-digest', D_FULL, '--spec', 'd', '--preset', 'p', '--expect-digest0', D_FREE),
     '--expect-digest0 is not used by the repin tier'),
    ((*ENC, '--allow-skip', 'checkpoint-compat'), 'no skippable step of that name'),
    ((*ENC, '--checkpoint', __file__, '--allow-skip', 'checkpoint-load'), 'nothing to skip'),
    ((*ENC, '--record-digest', 'full', '--allow-skip', 'checkpoint-load'), 'compare or record, not both'),
    ((*ENC, '--checkpoint', 'no/such.pt'), 'no such file'),
])
def test_contradictory_or_ignored_options_refuse(gate, capsys, argv, says):
    rc, _, err, _ = gate(*argv, capsys=capsys)
    assert rc == 2 and says in err and not gate.fake.calls


def test_a_digest_must_look_like_one(gate, capsys):
    with pytest.raises(SystemExit) as e:
        gate('repin', '--expect-digest', 'f6543eb', '--spec', 'd', '--preset', 'p')
    assert e.value.code == 2 and '16-hex-digit' in capsys.readouterr().err


def test_a_dirty_tree_refuses_and_an_allowed_one_is_named(gate, capsys, monkeypatch):
    monkeypatch.setattr(G, 'tracked_changes', lambda: [' M tools/gate.py', ' M third_party/zero-core'])
    rc, _, err, _ = gate('docs', capsys=capsys)
    assert rc == 2 and 'uncommitted changes' in err and 'third_party/zero-core' in err and not gate.fake.calls
    rc, out, _, report = gate('docs', '--allow-dirty', capsys=capsys)
    assert rc == 0 and final(out).startswith('GATE PASSED WITH SKIPS docs')
    assert 'clean-tree (--allow-dirty: 2 uncommitted change(s)' in final(out) and not report['clean']


def test_the_baseline_must_be_another_built_commit(gate, capsys, tmp_path, monkeypatch):
    rc, _, err, _ = gate('search-core', '--baseline-root', G.ROOT, capsys=capsys)
    assert rc == 2 and 'is this checkout' in err
    empty = tmp_path / 'unbuilt'
    empty.mkdir()
    rc, _, err, _ = gate('search-core', '--baseline-root', str(empty), capsys=capsys)
    assert rc == 2 and 'no built mtgz/_engine.pyd' in err
    same = G.git
    monkeypatch.setattr(G, 'git', lambda args, cwd=None, check=True: SHA if args == ['rev-parse', 'HEAD']
                        else same(args, cwd, check))
    rc, _, err, _ = gate('search-core', '--baseline-root', gate.baseline, capsys=capsys)
    assert rc == 2 and 'would be vacuous' in err


def test_a_clean_pass_says_only_gate_passed(gate, capsys):
    rc, out, _, report = gate('docs', capsys=capsys)
    assert rc == 0 and final(out).startswith('GATE PASSED docs 276f948e6d31 in ') and 'SKIPS' not in final(out)
    assert report['clean'] and report['final'] == final(out) and report['skipped'] == []
    assert [s['status'] for s in report['steps']] == ['passed', 'passed', 'passed']   # privacy, py-compile, tree-unchanged
    assert report['steps'][0]['id'] == 'privacy'


def test_a_privacy_finding_fails_the_gate_before_any_step(gate, capsys):
    """AGENTS.md "Private details": tools/privacy_check.py runs first in every tier; a finding fails the gate and no
    step runs."""
    gate.fake.privacy = (False, 'exit 1: docs/x.md:3: [tailscale-ip] ... (fix the file)')
    rc, out, _, report = gate('python', capsys=capsys)
    assert rc == 1 and final(out).startswith('GATE FAILED python ') and 'privacy: exit 1: docs/x.md:3' in final(out)
    assert 'not run: build, worker-build, pytest, run-loop-smoke' in final(out)
    assert gate.fake.calls == [] and [s['id'] for s in report['steps']] == ['privacy']


def test_the_privacy_check_runs_the_checkouts_tool(tmp_path):
    """privacy_scan reads tools/privacy_check.py's last line: 0 findings passes, anything else fails with the findings."""
    tools = tmp_path / 'tools'
    tools.mkdir()
    (tools / 'privacy_check.py').write_text("print('privacy check: 3 file(s), 0 finding(s) (x)')\n")
    assert G.privacy_scan(str(tmp_path)) == (True, 'privacy check: 3 file(s), 0 finding(s)')
    (tools / 'privacy_check.py').write_text("import sys; print('a.md:1: [r] y'); print('privacy check: 3 file(s), "
                                            "1 finding(s) (x)'); sys.exit(1)\n")
    ok, why = G.privacy_scan(str(tmp_path))
    assert not ok and why.startswith('exit 1: a.md:1: [r] y | privacy check: 3 file(s), 1 finding(s)')


def test_an_allowed_skip_is_named_in_the_final_line(gate, capsys):
    rc, out, _, report = gate(*ENC, '--allow-skip', 'checkpoint-load', capsys=capsys)
    line = final(out)
    assert rc == 0 and line.startswith('GATE PASSED WITH SKIPS encoding')
    assert 'skipped: checkpoint-load (--allow-skip checkpoint-load: no --checkpoint' in line
    assert 'SKIP     checkpoint-load' in out and 'h2h' not in [k for k, _ in gate.fake.calls]
    assert report['passed'] and not report['clean'] and report['skipped'] == [line.split('skipped: ')[1].split(';')[0]]
    step = {s['id']: s for s in report['steps']}['checkpoint-load']
    assert step['status'] == 'skipped' and not step['passed']


def test_no_mac_skips_are_named_too(gate, capsys):
    rc, out, _, _ = gate('repin', '--expect-digest', D_FULL, '--spec', 'delver', '--preset', 'delver-mirror',
                         '--no-mac', capsys=capsys)
    assert rc == 0 and final(out).startswith('GATE PASSED WITH SKIPS repin') and 'mac-eval-checkout (--no-mac)' in final(out)
    eng = ('engine', '--expect-digest', D_FULL, '--expect-digest0', D_FREE, '--no-mac')
    rc, _, err, _ = gate(*eng, capsys=capsys)
    assert rc == 2 and 'gates/276f948e6d31-suite.json' in err.replace('\\', '/') and '--allow-skip suite-evidence' in err
    rc, out, _, _ = gate(*eng, '--allow-skip', 'suite-evidence', capsys=capsys)
    assert rc == 0 and 'mac-recheck (--no-mac: no --tested-sha, so whether the recheck was needed was not decided)' \
        in final(out) and 'suite-evidence (--allow-skip suite-evidence: the full engine suite and the 2k fuzz of ' \
        '276f948e6d31 are asserted, not evidenced)' in final(out)


def test_the_engine_tiers_mac_recheck_is_decided_by_the_tested_sha(gate, capsys):
    eng = ('engine', '--expect-digest', D_FULL, '--expect-digest0', D_FREE)
    suite_report(str(gate.tmp / 'gates'), SHA)
    suite_report(str(gate.tmp / 'gates'), '1234567'.ljust(40, '0'))
    rc, out, _, report = gate(*eng, '--tested-sha', SHA, capsys=capsys)        # gated directly: not needed, not a skip
    assert rc == 0 and final(out).startswith('GATE PASSED engine') and 'N/A' in out and report['clean']
    gate.fake.diff = 'engine/src/game.rs\ndocs/ENGINE.md\n'
    rc, out, _, report = gate(*eng, '--tested-sha', '1234567', capsys=capsys)  # overlapping engine/ files: the Mac runs
    assert rc == 0 and 'mac' in [k for k, _ in gate.fake.calls] and final(out).startswith('GATE PASSED engine')
    gate.fake.set('mac', MAC(SHA, CARGO(900, 12, 0), after='9' * 40))          # switched under the command
    rc, out, _, _ = gate(*eng, '--tested-sha', '1234567', capsys=capsys)
    assert rc == 1 and 'mac-recheck: the Mac\'s checkout moved to 999999999999 during the command' in final(out)
    gate.fake.set('mac', CARGO(900, 12, 0))                                     # no "[mac] ... @ <sha>" line
    rc, out, _, _ = gate(*eng, '--tested-sha', '1234567', capsys=capsys)
    assert rc == 1 and 'mac-recheck: no "[mac]' in final(out)
    gate.fake.set('mac', MAC(SHA, 'error: could not compile `engine`\n'))     # no "test result:" line
    rc, out, _, _ = gate(*eng, '--tested-sha', '1234567', capsys=capsys)
    assert rc == 1 and 'no "test result:" line' in final(out)


# ---------------------------------------------------------------------------- rule 2: digests

def test_a_digest_without_an_expected_value_is_recorded_never_passed(gate, capsys):
    rc, out, _, report = gate('encoding', '--expect-digest0', D_FREE, '--record-digest', 'full',
                              '--allow-skip', 'checkpoint-load', capsys=capsys)
    line = final(out)
    assert rc == 0 and line.startswith('GATE PASSED WITH SKIPS encoding')
    assert f'recorded, not compared: digest-full = {D_FULL} (--record-digest)' in line
    assert f'RECORDED digest full' in out and 'PASS     digest full' not in out
    step = {s['id']: s for s in report['steps']}['digest-full']
    assert step['status'] == 'recorded' and not step['passed'] and 'NOT compared' in step['reason']
    assert report['digests']['full'] == {'value': D_FULL, 'status': 'recorded'}
    with pytest.raises(ValueError):
        G.make_digest_checker(None)                       # neither an expected value nor record-only


def test_the_digest_checker():
    check = G.make_digest_checker(D_FULL)
    assert check(DIGEST(D_FULL), 0)[:2] == (True, f'digest {D_FULL} (= expected)')
    assert not check(DIGEST(D_FREE), 0)[0]                                      # another digest
    assert 'not the full 2000 + 500' in check(DIGEST(D_FULL, games=(10, 500)), 0)[1]      # DIGEST_RANDOM=10
    assert 'RE-RECORDED' in check(DIGEST(D_FULL) + 'wrote engine/tests/data/digests.txt\n', 0)[1]
    assert 'did not pass' in check(DIGEST(D_FULL).replace('test result: ok.', 'nothing'), 0)[1]
    assert not check(DIGEST(D_FULL), 101)[0]                                    # the recorded-file assert failed
    assert not check('error[E0425]: cannot find value\n', 101)[0]
    assert not check(DIGEST(D_FULL) * 2, 0)[0]


# ---------------------------------------------------------------------------- rule 3: positive markers

def test_the_build_checker_wants_this_checkouts_module():
    check = G.make_build_checker('mtgz._engine', os.path.join(G.ROOT, 'mtgz'))
    assert check(BUILD(G.ROOT), 0)[0]
    assert 'no "mtgz._engine OK' in check('    Finished `release` profile\nTraceback: ImportError\n', 0)[1]
    assert 'not from' in check(BUILD('C:/elsewhere/mtg-zero'), 0)[1]
    assert not check(BUILD(G.ROOT), 1)[0]
    assert not check(BUILD(G.ROOT).replace("'toy', 'mtg', 'team_draft'", ''), 0)[0]
    core = G.make_build_checker('zero._engine', os.path.join('C:/core', 'python', 'zero'))
    assert core(CORE_BUILD('C:/core'), 0)[0] and not core(BUILD('C:/core'), 0)[0]


def test_the_cargo_checker():
    check, one = G.make_cargo_checker(), G.make_cargo_checker(single=True)
    assert check(CARGO(123, 3, 0, 0), 0)[0] and one(CARGO(34), 0)[0]
    assert not check('error: could not compile `engine`\n', 101)[0]
    assert '0 tests passed' in check(CARGO(0, 0), 0)[1]
    assert 'expected exactly 1' in one(CARGO(34, 2), 0)[1]
    assert not check(CARGO(33, failed=1), 101)[0]
    assert 'exit 101' in check(CARGO(123), 101)[1]                              # a later target failed to build


def test_the_compat_checker():
    old, new = 'C:/wt/merge-main', 'C:/wt/gate'
    check = G.make_compat_checker(old, new)
    assert check(COMPAT(old, new), 0)[0]
    assert 'NOT identical' in check(COMPAT(old, new, identical=False), 1)[1]
    assert 'no {"identical"' in check(COMPAT(old, new).rsplit('{', 1)[0], 0)[1]  # rc 0 but no result line
    assert 'no "old:' in check(COMPAT(old, new).split('new:', 1)[1], 0)[1]
    assert 'no samples' in check(COMPAT(old, new, samples=0), 0)[1]
    assert 'not C:/wt/gate' in check(COMPAT(old, 'C:/wt/other'), 0)[1]
    assert not check(COMPAT(old, new), 1)[0]


def test_the_mac_checkers():
    check = G.make_mac_checker(SHA, G.make_cargo_checker())
    assert check(MAC(SHA, CARGO(900, 12)), 0)[0]
    assert 'no "[mac]' in check(CARGO(900), 0)[1]
    assert 'was at 999999999999, not 276f948e6d31' in check(MAC('9' * 40, CARGO(900)), 0)[1]
    # the sha the desktop sent is not enough: the Mac's own HEAD (full), and again after the command
    assert 'no "[mac] <host>' in check(MAC(SHA, CARGO(900)).replace(SHA, SHA[:9], 1), 0)[1]
    assert 'no "[mac] done @' in check(MAC(SHA, CARGO(900)).replace('[mac] done', 'mac done'), 0)[1]
    assert 'moved to 999999999999' in check(MAC(SHA, CARGO(900), after='9' * 40), 0)[1]
    ev = G.make_eval_checkout_checker(SHA)
    good = EVAL_CHECKOUT(SHA)
    assert ev(good, 0)[0] and not ev(good, 1)[0]
    assert 'eval checkout ready' in ev(good.replace('eval checkout ready', 'nope'), 0)[1]
    assert 'unchanged' in ev(good.replace('unchanged', 'CHANGED'), 0)[1]
    assert 'build line' in ev(good.replace('mtgz._engine OK', 'mtgz._engine FAILED'), 0)[1]
    assert not ev(EVAL_CHECKOUT('9' * 40), 0)[0]


def test_the_other_checkers_want_their_markers():
    smoke = G.make_search_smoke_checker(4)
    assert smoke(SMOKE(4), 0)[0] and not smoke(SMOKE(4, bad=1), 1)[0]
    assert 'expected 4' in smoke(SMOKE(0), 0)[1] and 'expected 4' in smoke(SMOKE(1), 0)[1]
    h2h = G.make_h2h_checker(8)
    assert h2h(H2H, 0)[0]
    assert '2 of 8 games unfinished' in h2h(H2H.replace('"unfinished": 0}\n', '"unfinished": 2}\n'), 0)[1]
    assert 'expected 8' in h2h(H2H.replace('"games": 8', '"games": 7'), 0)[1]
    assert not h2h('# batch {"games": 8, "unfinished": 0}\n', 0)[0]             # only a batch line
    pyc = G.make_pycompile_checker(3)
    assert pyc('py_compile: 3 file(s) compiled\n', 0)[0]
    assert not pyc('', 0)[0] and not pyc('py_compile: 2 file(s) compiled\n', 0)[0]


def test_the_metrics_checker_wants_the_counts(tmp_path):
    def rows(*sp):
        with open(tmp_path / 'metrics.jsonl', 'w', encoding='utf-8') as f:
            for i, s in enumerate(sp):
                f.write(json.dumps({'iter': i, 'selfplay': s}) + '\n')
    check = G.make_metrics_checker(str(tmp_path), want_games=16)
    rows({'games': 16, 'unfinished': 0}, {'games': 16, 'unfinished': 0})
    assert check('', 0)[0]
    rows({'games': 16}, {'games': 16})
    assert 'no self-play games / unfinished' in check('', 0)[1]
    rows({'games': 0, 'unfinished': 0}, {'games': 0, 'unfinished': 0})
    assert not G.make_metrics_checker(str(tmp_path))('', 0)[0]                  # unknown preset: still > 0 games


def test_a_step_rewriting_a_tracked_file_fails_the_gate(gate, capsys, monkeypatch):
    states = iter([[], [], [' M engine/tests/data/digests.txt']])
    monkeypatch.setattr(G, 'tracked_changes', lambda: next(states))
    rc, out, _, report = gate('docs', capsys=capsys)
    assert rc == 1 and 'tree-unchanged: the checkout changed during the gate: M engine/tests/data/digests.txt' \
        in final(out) and not report['passed']


# ---------------------------------------------------------------------------- rule 4: pytest's summary

@pytest.mark.parametrize('line, counts', [
    ('69 passed in 102.45s (0:01:42)', {'passed': 69}),
    ('1 failed, 301 passed, 3 skipped, 1 warning in 881.28s (0:14:41)',
     {'failed': 1, 'passed': 301, 'skipped': 3, 'warning': 1}),
    ('========== 2 failed, 5 passed, 1 error in 3.10s ==========', {'failed': 2, 'passed': 5, 'error': 1}),
    ('!!! Interrupted !!!\n3 errors in 0.52s', {'error': 3}),
    ('5 passed, 2 warnings in 0.10s', {'passed': 5, 'warning': 2}),
    ('no tests ran in 0.01s', {}),
    ('Traceback (most recent call last):\nKeyboardInterrupt', None),
])
def test_the_pytest_summary_parser(line, counts):
    assert G.parse_pytest_summary('collected 9 items\n' + line + '\n') == counts


def test_check_pytest_reads_the_final_summary_only():
    assert G.check_pytest(PYTEST_OK, 0)[:2] == (True, '69 passed')
    trap = 'test_x printed: 5 passed in 0.1s\n.F.\n1 failed, 2 passed in 1.00s\n'  # the first "N passed" is a lie
    assert G.check_pytest(trap, 1)[:2] == (False, '1 failed, 0 error(s), 2 passed')
    assert not G.check_pytest('4 passed, 1 error in 2.00s\n', 1)[0]
    assert G.check_pytest('no tests ran in 0.01s\n', 5)[1] == 'no tests ran'
    assert not G.check_pytest('3 skipped in 0.20s\n', 0)[0]                     # nothing passed
    assert 'no pytest summary line' in G.check_pytest('Fatal Python error: Aborted\n', 3221226505)[1]
    assert 'exit 3' in G.check_pytest(PYTEST_OK, 3)[1]


# ---------------------------------------------------------------------------- rule 5: the core pytest, flaky policy

@pytest.fixture
def core_step(tmp_path, monkeypatch):
    core = tmp_path / 'core'
    core.mkdir()
    monkeypatch.setattr(G, 'CORE', str(core))
    monkeypatch.setattr(G, 'GATES_DIR', str(tmp_path / 'gates'))
    monkeypatch.setattr(G, 'WIN', False)                       # its --basetemp under tmp, not E:
    ctx = types.SimpleNamespace(logs_dir=str(tmp_path), sha_short=SHA[:12], tier='optin-core', root=G.ROOT,
                                step_timeout=60, base_env={'CUDA_VISIBLE_DEVICES': '-1'})
    build, step = G.core_steps(ctx)
    assert build.id == 'core-build' and step.id == 'core-pytest' and step.cwd == str(core)
    assert step.argv[3:5] == ['tests', '-q'] and '-rfEs' in step.argv
    calls = []

    def script(*answers):
        queue = list(answers)

        def fake(argv, cwd, env, timeout):
            calls.append(list(argv))
            assert cwd == str(core) and env['CUDA_VISIBLE_DEVICES'] == '-1'
            return queue.pop(0)
        monkeypatch.setattr(G, 'execute', fake)
    return types.SimpleNamespace(ctx=ctx, step=step, calls=calls, script=script)


def test_only_remoteplay_failures_are_retried_alone_and_a_pass_is_recorded(core_step):
    core_step.script((PYTEST_FAILED(REMOTEPLAY), 1), ('.\n1 passed in 41.20s\n', 0))
    r = G.run_step(core_step.ctx, core_step.step)
    assert r.status == 'passed' and r.extra['flaky'] == [REMOTEPLAY]
    assert 'flaky, passed on retry alone: ' + REMOTEPLAY in r.reason
    assert len(core_step.calls) == 2 and core_step.calls[1][3] == REMOTEPLAY   # the one test, alone
    assert any(a.startswith('--basetemp=') and a.endswith('-retry1') for a in core_step.calls[1])
    assert 'flaky retry 1/1' in open(r.log, encoding='utf-8').read()
    line = G.final_line('optin-core', SHA[:12], True, [r], [], 900, 'gates/x.json')
    assert line.startswith('GATE PASSED WITH FLAKY RETRIES optin-core')
    assert f'flaky, passed on retry: core-pytest: {REMOTEPLAY}' in line


def test_any_other_failure_fails_without_a_retry(core_step):
    other = 'tests/test_loop.py::test_resume'
    for out in (PYTEST_FAILED(other), PYTEST_FAILED(REMOTEPLAY, other),
                PYTEST_FAILED(passed=300, errors=('tests/test_remoteplay.py',))):   # a collection error
        core_step.calls.clear()
        core_step.script((out, 1))
        r = G.run_step(core_step.ctx, core_step.step)
        assert r.status == 'failed' and 'not retried' in r.reason and len(core_step.calls) == 1


def test_a_failed_retry_fails_the_step(core_step):
    second = 'tests/test_remoteplay.py::test_pause_mid_phase_then_resume'
    core_step.script((PYTEST_FAILED(REMOTEPLAY, second), 1), ('.\n1 passed in 30.00s\n', 0),
                     (PYTEST_FAILED(second, passed=0), 1))
    r = G.run_step(core_step.ctx, core_step.step)
    assert r.status == 'failed' and 'failed again: ' + second in r.reason and len(core_step.calls) == 3
    assert [x['passed'] for x in r.extra['retries']] == [True, False]


def test_the_digest_formats_checker():
    """digest_formats (ENGINE.md 148.1, 157, 176): exactly one line each of constructed, eoe, driver and metagame, no
    re-record, the binary's one "test result: ok. 4 passed" line + exit 0 - the four digests named in the reason and the
    report."""
    check = G.check_digest_formats
    ok, why, extra = check(FORMATS(), 0)
    assert ok and extra['formats'] == D_FORMATS and all(f'{n} {d}' in why for n, d in D_FORMATS.items())
    out = FORMATS()
    assert not check(out.replace('eoe: 160', 'eoe (encodings off): 160'), 0)[0]          # a line missing
    ok, why, _ = check(out + f'constructed: x; combined digest {D_FORMATS["constructed"]}\n', 0)
    assert not ok and '5 "<name>' in why                                                  # a line twice
    ok, why, _ = check(out.replace('test result: ok. 4 passed', 'wrote E:/x/engine/tests/data/digests_eoe.txt\n'
                                   'test result: ok. 4 passed'), 0)
    assert not ok and 'RE-RECORDED' in why                                                # DIGEST_WRITE
    ok, why, _ = check(FORMATS(passed=3), 0)
    assert not ok and '3 passed, not the 4' in why
    assert not check(out.replace('test result: ok. 4 passed; 0 failed', 'test result: FAILED. 3 passed; 1 failed'), 101)[0]
    assert not check(out.replace('metagame: 225', 'metagame (encodings off): 225'), 0)[0]   # the metagame line missing
    assert not check(out, 1)[0]
    assert 'did it compile' in check('error[E0425]: cannot find value\n', 101)[1]


def test_the_dealt_digest_checker():
    """dealt_digests (ENGINE.md 154, 157): exactly one combined-digest line, no re-record, the binary's one "test
    result: ok. 1 passed" line + exit 0."""
    check = G.check_digest_dealt
    ok, why, extra = check(DEALT(), 0)
    assert ok and extra['formats'] == {'dealt': D_DEALT} and D_DEALT in why and '80 matches' in why
    assert 'expected exactly one' in check(DEALT() + DEALT(), 0)[1]
    assert 'expected exactly one' in check('error[E0425]: cannot find value\n', 101)[1]
    ok, why, _ = check(DEALT().replace('test dealt_digests ... ok', 'wrote E:/x/engine/tests/data/digests_dealt.txt\ntest dealt_digests ... ok'), 0)
    assert not ok and 'RE-RECORDED' in why
    assert not check(DEALT().replace('test result: ok. 1 passed; 0 failed', 'test result: FAILED. 0 passed; 1 failed'), 101)[0]
    assert not check(DEALT(), 1)[0]


def test_the_digest_formats_step_runs_in_the_behaviour_tiers_and_reports_its_digests(gate, capsys):
    """The step (its DIGEST_* the caller's never: step_env) in the encoding, engine, repin, search-core and optin-core
    tiers; the report's digests name the three; a recorded file that differs fails the gate."""
    rc, out, _, report = gate(*ENC, '--allow-skip', 'checkpoint-load', capsys=capsys)
    calls = [a for k, a in gate.fake.calls if k in ('formats', 'dealt')]
    assert rc == 0 and calls == [G.FORMATS_ARGV, G.DEALT_ARGV] and 'PASS     digest formats' in out and 'PASS     digest dealt' in out
    assert {k: v['value'] for k, v in report['digests'].items() if k in D_FORMATS} == D_FORMATS
    assert report['digests']['dealt'] == {'value': D_DEALT, 'status': 'passed'}
    for argv in (('engine', '--expect-digest', D_FULL, '--expect-digest0', D_FREE, '--no-mac', '--allow-skip',
                  'suite-evidence'),
                 ('repin', '--expect-digest', D_FULL, '--spec', 'delver', '--preset', 'delver-mirror')):
        gate.fake.calls.clear()
        gate(*argv, capsys=capsys)
        assert {'formats', 'dealt'} <= {k for k, _ in gate.fake.calls}, argv
    gate.fake.calls.clear()
    gate.fake.set('formats', FORMATS().replace('test driver_digest ... ok', 'test driver_digest ... FAILED')
                  .replace('test result: ok. 4 passed; 0 failed', 'test result: FAILED. 3 passed; 1 failed'), 101)
    rc, out, _, _ = gate(*ENC, '--allow-skip', 'checkpoint-load', capsys=capsys)
    assert rc == 1 and 'digest-formats: the digest_formats tests did not pass' in final(out)


def test_a_failure_count_that_does_not_add_up_is_not_retried():
    out = PYTEST_FAILED(REMOTEPLAY).replace('1 failed, 301 passed', '2 failed, 300 passed')
    assert G.flaky_retry_candidates(out) == (None, 'the summary counts 2 failure(s)/error(s) but the short summary '
                                                   'lists 1')
    assert G.flaky_retry_candidates(PYTEST_FAILED(REMOTEPLAY)) == ([REMOTEPLAY], None)
    assert G.flaky_retry_candidates(PYTEST_FAILED(REMOTEPLAY).split('=== short')[0] +
                                    '1 failed, 301 passed in 1.00s\n')[0] is None


def test_the_core_tiers_build_and_run_the_core_suite_and_name_a_flaky_retry(gate, capsys):
    gate.fake.core_pytest = [(PYTEST_FAILED(REMOTEPLAY, passed=307), 1), ('.\n1 passed in 41.20s\n', 0)]
    rc, out, _, report = gate('optin-core', '--baseline-root', gate.baseline, '--other-games', *gate.other,
                              capsys=capsys)
    kinds = [k for k, _ in gate.fake.calls]
    # the builds in order, then every check at once (ENGINE.md 167); the flaky retry right after its pytest
    assert kinds[:4] == ['build', 'worker-build', 'core-build', 'test-build']
    assert sorted(kinds[4:]) == sorted(['cargo', 'compat', 'formats', 'dealt', 'farm-compat', 'cargo1', 'cargo1',
                                        'build-only', 'core-pytest', 'core-pytest'])
    assert rc == 0 and final(out).startswith('GATE PASSED WITH FLAKY RETRIES optin-core')
    assert report['flaky'] == [f'core-pytest: {REMOTEPLAY}'] and report['baseline']['head'] == 'c' * 40
    gate.fake.core_pytest = []
    rc, out, _, _ = gate('search-core', '--baseline-root', gate.baseline, capsys=capsys)
    assert rc == 0 and final(out).startswith('GATE PASSED search-core')
    kinds = [k for k, _ in gate.fake.calls][-13:]
    assert kinds[:4] == ['build', 'worker-build', 'core-build', 'test-build']
    assert sorted(kinds[4:]) == sorted(['cargo', 'pytest', 'farm-compat', 'compat', 'formats', 'dealt', 'smoke', 'run',
                                        'core-pytest'])


def test_a_stale_build_fails_the_core_tier_before_the_comparison(gate, capsys):
    gate.fake.set('build', '    Finished `release` profile\nImportError: DLL load failed\n', 0)
    rc, out, _, report = gate('optin-core', '--baseline-root', gate.baseline, '--other-games', *gate.other,
                              capsys=capsys)
    assert rc == 1 and [k for k, _ in gate.fake.calls] == ['build']
    assert 'build: no "mtgz._engine OK' in final(out) and \
        'not run: worker-build, core-build, test-build, core-cargo-test, checkpoint-compat' in final(out)


# ---------------------------------------------------------------------------- dry runs

@pytest.mark.parametrize('argv, on_success', [
    (('docs',), 'on success: GATE PASSED (every step on positive evidence)'),
    (('python', '--allow-skip', 'build'), 'on success: GATE PASSED WITH SKIPS - skipped: build (--allow-skip build'),
    ((*ENC, '--checkpoint', __file__), 'on success: GATE PASSED (every step'),
    (('engine', '--expect-digest', D_FULL, '--record-digest', 'encoding-free', '--tested-sha', SHA, '--allow-skip',
      'suite-evidence'), 'recorded, not compared: digest-encoding-free (--record-digest)'),
    (('search-core', '--allow-skip', 'checkpoint-compat'), 'skipped: checkpoint-compat (--allow-skip'),
    (('repin', '--expect-digest', D_FULL, '--spec', 'delver', '--preset', 'delver-mirror'), 'on success: GATE PASSED ('),
])
def test_dry_run_prints_the_plan_and_what_a_pass_would_say(gate, capsys, argv, on_success):
    rc, out, _, report = gate(*argv, '--dry-run', capsys=capsys)
    assert rc == 0 and on_success in out and report is None and not gate.fake.calls
    assert 'passes on:' in out


# ---------------------------------------------------------------------------- the Fable review's fixes (ENGINE.md 143)

def test_each_tier_has_its_own_report(gate, capsys):
    """B10: gates/<sha>.json was one file per commit whatever the tier: a later docs pass overwrote the commit's
    engine report (passed or FAILED), and "don't re-run a full suite on a sha that already passed it" read it."""
    gdir = gate.tmp / 'gates'
    rc = G.main(['repin', '--expect-digest', D_FULL, '--spec', 'delver', '--preset', 'delver-mirror', '--no-mac'])
    assert rc == 0
    assert G.main(['docs']) == 0
    names = sorted(p.name for p in gdir.glob('*.json'))
    assert names == [f'{SHA[:12]}-docs.json', f'{SHA[:12]}-repin.json']
    assert json.loads((gdir / f'{SHA[:12]}-repin.json').read_text())['tier'] == 'repin'
    assert final(capsys.readouterr().out).endswith(f'{SHA[:12]}-docs.json')


def test_a_step_timeout_bounds_its_grandchildren(tmp_path):
    """B11: subprocess.run(timeout=) killed only the direct child, then read the pipe to its end - which a grandchild
    still held (Windows): a step killed after 2 s returned when the grandchild exited, 12 s later, and the
    orphans kept the CPU. The timeout now bounds the whole tree, and the grandchild is gone."""
    psutil = pytest.importorskip('psutil')
    child = tmp_path / 'child.py'
    pidfile = tmp_path / 'grandchild.pid'
    child.write_text('import subprocess, sys, time\n'
                     'p = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"])\n'
                     f'open({str(pidfile)!r}, "w").write(str(p.pid))\n'
                     'print("started", flush=True)\n'
                     'time.sleep(60)\n')
    t0 = time.time()
    out, rc = G.execute([sys.executable, str(child)], str(tmp_path), dict(os.environ), 8)
    took = time.time() - t0
    assert rc == 'timeout' and 'started' in out and 'killed after 8s timeout' in out
    assert took < 20, took                                            # (the grandchild alone: 30 s)
    gpid = int(pidfile.read_text())
    deadline = time.time() + 5
    while psutil.pid_exists(gpid) and time.time() < deadline:
        time.sleep(0.1)
    assert not psutil.pid_exists(gpid) or psutil.Process(gpid).status() == psutil.STATUS_ZOMBIE


def test_a_process_holding_the_output_after_the_command_exited_is_bounded(tmp_path):
    """... and a step whose command exited while a process it started still held its output: that one is killed at
    the deadline (the step times out) instead of holding the gate for ever."""
    pytest.importorskip('psutil')
    child = tmp_path / 'child.py'
    child.write_text('import subprocess, sys, time\n'
                     'subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"])\n'
                     'time.sleep(1.5)\nprint("parent done", flush=True)\n')
    t0 = time.time()
    out, rc = G.execute([sys.executable, str(child)], str(tmp_path), dict(os.environ), 8)
    assert rc == 'timeout' and 'parent done' in out and time.time() - t0 < 25          # (the grandchild: 60 s)
    out, rc = G.execute([sys.executable, '-c', 'print("plain")'], str(tmp_path), dict(os.environ), 30)
    assert (out.strip(), rc) == ('plain', 0)


def _repo(tmp_path):
    import subprocess
    r = tmp_path / 'repo'
    (r / 'tests').mkdir(parents=True)
    (r / 'tests' / 'test_a.py').write_text('x = 1\n')
    (r / '.gitignore').write_text('*.pyd\n/gates/\n')
    for args in (['init', '-q'], ['add', '-A'], ['-c', 'user.name=t', '-c', 'user.email=t@t', 'commit', '-q', '-m', 'x']):
        subprocess.run(['git', '-C', str(r), *args], check=True, capture_output=True)
    return r


def test_untracked_files_where_a_step_picks_them_up_count(tmp_path):
    """B12: the gate's "the commit, unchanged" ran `git status --untracked-files=no`: an untracked tests/test_*.py,
    conftest.py, engine/tests/*.rs or module under mtgz/ was collected, compiled or imported by the steps and is in
    no commit. Untracked files there now count; a stray file elsewhere (a crash dump) or an ignored one does not."""
    r = _repo(tmp_path)
    assert G.checkout.changes(str(r)) == []
    (r / 'tests' / 'test_zz_probe.py').write_text('')
    (r / 'conftest.py').write_text('')
    (r / 'engine' / 'tests').mkdir(parents=True)
    (r / 'engine' / 'tests' / 'zz.rs').write_text('')
    (r / 'bash.exe.stackdump').write_text('')                           # not where a build or test looks
    (r / 'mtgz').mkdir()
    (r / 'mtgz' / '_engine.pyd').write_text('')                          # ignored (the build's own output)
    (r / 'gates').mkdir()
    (r / 'gates' / 'x.json').write_text('')
    assert G.checkout.changes(str(r)) == ['?? conftest.py', '?? engine/tests/zz.rs', '?? tests/test_zz_probe.py']
    (r / 'tests' / 'test_a.py').write_text('x = 2\n')
    assert G.checkout.changes(str(r))[0] == ' M tests/test_a.py'


def test_an_untracked_file_refuses_the_gate_and_one_made_during_it_fails_it(gate, capsys, monkeypatch):
    monkeypatch.setattr(G, 'untracked_files', lambda: ['?? tests/test_zz_infra_probes.py'])
    rc, _, err, _ = gate('docs', capsys=capsys)
    assert rc == 2 and 'not exactly its commit (?? tests/test_zz_infra_probes.py)' in err and not gate.fake.calls
    rc, out, _, report = gate('docs', '--allow-dirty', capsys=capsys)
    assert rc == 0 and 'clean-tree (--allow-dirty: 1 uncommitted change(s)' in final(out) and not report['clean']
    states = iter([[], [], ['?? tests/test_new.py']])
    monkeypatch.setattr(G, 'untracked_files', lambda: next(states))
    rc, out, _, _ = gate('docs', capsys=capsys)
    assert rc == 1 and 'the checkout changed during the gate: ?? tests/test_new.py' in final(out)


def test_head_moving_during_the_gate_fails_it(gate, capsys, monkeypatch):
    same = G.git
    monkeypatch.setattr(G, 'git', lambda args, cwd=None, check=True: 'd' * 40 if args == ['rev-parse', 'HEAD'] and
                        cwd is None else same(args, cwd, check))
    rc, out, _, _ = gate('docs', capsys=capsys)
    assert rc == 1 and f'HEAD moved during the gate: {SHA[:12]} -> dddddddddddd' in final(out)


def test_the_suite_tier_runs_the_full_suite_and_the_sharded_fuzz(gate, capsys):
    """I1: the engine tier asserted the full suite and the fuzz through --tested-sha (the agent's word). The suite tier
    runs them; its report is what the engine tier now checks. ENGINE.md 167 (the user's D1): the test binaries built
    once, then the suite and a sharded 2,000-game fuzz per pool (tools/fuzz_shards.py) at once."""
    rc, out, _, report = gate('suite', '--fuzz-pools', 'default,delver,default+hidden', capsys=capsys)
    assert rc == 0 and final(out).startswith('GATE PASSED suite') and report['clean']
    calls = [a for k, a in gate.fake.calls]
    assert calls[0] == ['cargo', 'test', '--release', '-p', 'engine', '--no-run']
    assert ['cargo', 'test', '--release', '-p', 'engine', '--no-fail-fast'] in calls and len(calls) == 5
    fz = [a for k, a in gate.fake.calls if k == 'fuzz-shards']
    assert sorted((a[a.index('--pool') + 1], a[a.index('--games') + 1], '--hidden' in a) for a in fz) == \
        [('default', '2000', False), ('default', '2000', True), ('delver', '2000', False)]
    steps = {s['id']: s for s in report['steps']}
    assert steps['fuzz-default']['fuzz_games'] == 2000 and steps['fuzz-delver']['status'] == 'passed'
    assert steps['fuzz-default-hidden']['status'] == 'passed'
    # its own report is the engine tier's evidence for this commit
    path = os.path.join(str(gate.tmp / 'gates'), f'{SHA[:12]}-suite.json')
    with open(path, 'w', encoding='utf-8') as f:
        json.dump(report, f)
    gate.fake.over.clear()
    rc, out, _, rep = gate('engine', '--expect-digest', D_FULL, '--expect-digest0', D_FREE, '--tested-sha', SHA,
                           capsys=capsys)
    assert rc == 0 and final(out).startswith('GATE PASSED engine') and rep['clean']
    ev = {s['id']: s for s in rep['steps']}['suite-evidence']
    assert ev['status'] == 'passed' and '3 fuzz run(s) of 2000 games' in ev['reason'] and os.path.exists(ev['log'])
    rc, out, _, _ = gate('suite', '--dry-run', capsys=capsys)
    assert rc == 0 and '(parallel)' in out and 'fuzz_shards.py' in out


def test_the_fuzz_shards_checker_wants_positive_evidence():
    check = G.make_fuzz_shards_checker(2000, 'default', False)
    ok, why, extra = check(FUZZ_SHARDS('default', 2000), 0)
    assert ok and extra['fuzz_games'] == 2000 and extra['fuzz_shards']['shards'] == 8
    assert 'not 2000 of default' in check(FUZZ_SHARDS('default', 300), 0)[1]
    assert 'not 2000 of default' in check(FUZZ_SHARDS('delver', 2000), 0)[1]
    assert 'not 2000 of default' in check(FUZZ_SHARDS('default', 2000, hidden=True), 0)[1]
    assert 'FAILED' in check(FUZZ_SHARDS('default', 2000, passed=False), 1)[1]
    assert 'expected exactly 1' in check(FUZZ_SHARDS('default', 2000).splitlines()[0] + '\n', 0)[1]
    no_matches = FUZZ_SHARDS('default', 2000).replace(', "matches over 100"', '')
    assert 'covered' in check(no_matches, 0)[1]                      # the matches' merged checks never ran
    assert G.make_fuzz_shards_checker(2000, 'eoe', False)(FUZZ_SHARDS('eoe', 2000, matches_skipped='games only'), 0)[0]
    assert G.make_fuzz_shards_checker(2000, 'default', True)(FUZZ_SHARDS('default', 2000, hidden=True), 0)[0]
    assert 'exit 1' in check(FUZZ_SHARDS('default', 2000), 1)[1]


def test_an_old_20k_suite_report_is_still_evidence(gate, capsys):
    """A suite report of the 20k gate before ENGINE.md 167 has more fuzz games than the gate asks for: it counts."""
    suite_report(str(gate.tmp / 'gates'), SHA)
    rc, out, _, rep = gate('engine', '--expect-digest', D_FULL, '--expect-digest0', D_FREE, '--tested-sha', SHA,
                           capsys=capsys)
    assert rc == 0 and '1 fuzz run(s) of 20000 games' in {s['id']: s for s in rep['steps']}['suite-evidence']['reason']


def test_the_card_ir_tests_run_only_when_the_commit_touches_the_pipeline(monkeypatch):
    """ENGINE.md 170.7: tests/ir (~8 min of CPU) runs only when the gated commit changes a file of IR_TEST_PATHS
    since main; with no main to compare with, it runs."""
    def fake_git(changed, has_main=True, on_main=False):
        def git(args, cwd=None, check=True):
            if args[:2] == ['rev-parse', '--verify']:
                return 'f' * 40 if has_main or not args[3].startswith(('main', 'origin/main')) else ''
            if args[0] == 'merge-base':
                return 'b' * 40
            if args[0] == 'rev-parse':
                return 'b' * 40 if on_main else 'c' * 40
            if args[:2] == ['diff', '--name-only']:
                return chr(10).join(changed)
            raise AssertionError(args)
        return git
    for changed, want in ((['tools/gate.py', 'docs/ENGINE.md'], False), (['ir/mtgz_ir/cards/ir.py'], True),
                          (['card_ir/src/lib.rs'], True), (['tests/ir/test_pools.py'], True),
                          (['data/cards/eoe.ir.json'], True), (['tools/codegen/gen_card_ir.py'], True)):
        monkeypatch.setattr(G, 'git', fake_git(changed))
        assert G.ir_tests_needed('c' * 40)[0] is want, changed
    monkeypatch.setattr(G, 'git', fake_git(['README.md'], on_main=True))      # a commit on main: its own change
    assert G.ir_tests_needed('c' * 40)[0] is False
    monkeypatch.setattr(G, 'git', fake_git(['README.md'], has_main=False))
    assert G.ir_tests_needed('c' * 40) == (True, 'no main branch to compare with')
    # the plan: not needed -> tests/ir left out of the shards (or ignored), with a NOT NEEDED step saying why
    monkeypatch.setattr(G, 'git', fake_git(['README.md']))
    monkeypatch.setattr(G, 'read_durations', lambda: {})
    ctx_ns = types.SimpleNamespace(pytest_shards=3, sha='c' * 40, sha_short='c' * 12)
    steps = G.pytest_step(ctx=ctx_ns)
    seen = [a for s in steps for a in s.argv if a.startswith('tests/')]
    assert sorted(seen) == G.pytest_files(include_ir=False) and not any(a.startswith('tests/ir/') for a in seen)
    assert [s.id for s in steps if s.not_needed] == ['pytest-ir']
    one = G.pytest_step(ctx=types.SimpleNamespace(pytest_shards=1, sha='c' * 40, sha_short='c' * 12))
    assert '--ignore=tests/ir' in one[0].argv and one[1].not_needed
    monkeypatch.setattr(G, 'git', fake_git(['ir/data/legacy_delver.json']))
    steps = G.pytest_step(ctx=ctx_ns)
    assert sorted(a for s in steps for a in s.argv if a.startswith('tests/')) == G.pytest_files()
    assert not any(s.not_needed for s in steps)


def test_the_python_tier_shards_pytest_beside_the_smoke(gate, capsys, monkeypatch):
    """ENGINE.md 167: pytest as --pytest-shards steps over disjoint test files, each its own process and basetemp, every
    tests/test_*.py in exactly one, run at once with the run-loop smoke after the (cached) builds."""
    monkeypatch.setattr(G, 'read_durations', lambda: {})       # (not this machine's measured seconds)
    files = G.pytest_files()
    assert 'tests/test_gate.py' in files and 'tests/ir/test_pipeline.py' in files     # (the card-IR tests: ENGINE.md 170)
    parts = G.pytest_partition(files, 4, {})
    assert sorted(f for p, d in parts for f in p) == files and len(parts) == 4 and G.pytest_coverage(files, parts) is None
    top = [f for f in files if not f.startswith('tests/ir/')]      # (the weights below are scaled to these files)
    heavy = max(top, key=lambda f: os.path.getsize(os.path.join(ROOT, f)))
    parts2 = G.pytest_partition(top, 4, {heavy: 10_000.0})
    assert ([heavy], []) in parts2                             # its measured weight gets it a shard of its own
    # a file longer than a shard's share, its tests measured: split by its tests, every test once, a test added since
    # (unmeasured) in the part that runs the file
    w = {heavy: 900.0, **{f'{heavy}::t{i}': 100.0 for i in range(9)}}
    parts3 = G.pytest_partition(top, 4, w)
    assert G.pytest_coverage(top, parts3) is None
    base = next((p, d) for p, d in parts3 if heavy in p)
    named = [e for p, _ in parts3 for e in p if e.startswith(heavy + '::')]
    assert sorted(base[1]) == sorted(named) and 6 <= len(named) <= 7          # (three or four parts, ~225 s each)
    assert G.pytest_coverage(top, [(p, []) for p, _ in parts3])        # (the named tests would run twice)
    assert G.junit_node(types.SimpleNamespace(get={'classname': 'tests.test_x.TestA', 'name': 'test_y[1]'}.get)) == \
        ('tests/test_x.py::TestA::test_y[1]', 'tests/test_x.py')
    rc, out, _, _ = gate('python', '--pytest-shards', '3', capsys=capsys)
    assert rc == 0
    ctx_ns = types.SimpleNamespace(pytest_shards=3, sha_short=SHA[:12])
    steps = G.pytest_step(ctx=ctx_ns)
    assert len(steps) == 3 and all(s.heavy for s in steps)
    seen = [a for s in steps for a in s.argv if a.startswith('tests/')]
    assert sorted(seen) == files
    assert len({next(a for a in s.argv if a.startswith('--basetemp=')) for s in steps}) == 3
    # beside a live run (ENGINE.md 166): each shard's Mac command runs its files but the desktop-only ones, and one
    # desktop part (beside the first shard with a Mac command) runs those - together every test once
    macs = [s.mac_cmd(None)[0][2] for s in steps if s.mac_cmd]
    desk = [s.desktop_part for s in steps if s.desktop_part]
    assert len(desk) == 1 and desk[0].argv[3:3 + len(G.PYTEST_DESKTOP_ONLY)] == list(G.PYTEST_DESKTOP_ONLY)
    desk_files = {a for a in G.PYTEST_DESKTOP_ONLY if '::' not in a}
    on_mac = [t for m in macs for t in m.split() if t.startswith('tests/')]
    assert sorted(on_mac) == sorted(f for f in files if f not in desk_files)
    assert all(m.startswith('bash tools/build_py.sh && ') for m in macs)


def test_the_worker_build_takes_the_cached_worker_by_its_line(tmp_path):
    exe = str(tmp_path / G.WORKER_EXE)
    open(exe, 'wb').write(b'w')
    digest = G.sha256_file(exe)
    check = G.make_worker_build_checker(exe)
    hit = f'artifact cache: hit worker 0123456789abcdef -> {exe} (sha256 {digest[:16]}, built from 72e5a0df6485 in x)\n'
    ok, why, extra = check(hit, 0)
    assert ok and extra['worker_cache'] == 'hit 0123456789abcdef' and 'artifact cache' in why
    assert 'but' in check(hit.replace(digest[:16], 'f' * 16), 0)[1]       # the file is not the one the cache placed
    assert check(WORKER_BUILT, 0)[2]['worker_cache'] == 'built'


def test_the_fuzz_checker_wants_the_20k_games():
    check = G.make_fuzz_checker(20000)
    tail = 'test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 9.00s\n'
    assert check('20000 games: wins [1, 2], draws 0\n' + tail, 0)[0]
    assert 'not 20000' in check('300 games: wins [1, 2], draws 0\n' + tail, 0)[1]       # a caller's FUZZ_GAMES lost
    assert '--nocapture' in check(tail, 0)[1]
    assert 'did not pass' in check('20000 games: wins [1, 2]\n' + tail.replace('ok. 4 passed; 0', 'FAILED. 3 passed; 1'),
                                   101)[1]


def test_a_suite_report_must_be_real_evidence(gate, capsys):
    gdir = str(gate.tmp / 'gates')
    eng = ('engine', '--expect-digest', D_FULL, '--expect-digest0', D_FREE, '--tested-sha', SHA)
    for over, says in (({'tier': 'engine'}, "its tier is 'engine'"), ({'sha': 'e' * 40}, 'it gates eeeeeeeeeeee'),
                       ({'clean': False, 'final': 'GATE PASSED WITH SKIPS suite'}, 'did not pass cleanly'),
                       ({'zero_core_pin': 'f' * 40}, None)):
        suite_report(gdir, SHA, over)
        rc, _, err, _ = gate(*eng, capsys=capsys)
        if says:
            assert rc == 2 and says in err and not gate.fake.calls, (over, err)
        else:
            assert rc == 0                      # (the fake git knows no pin of the sha: not compared here)
    path = suite_report(gdir, SHA)
    log = json.load(open(path))['steps'][1]['log']
    with open(log, 'a', encoding='utf-8') as f:
        f.write('edited\n')
    rc, _, err, _ = gate(*eng, capsys=capsys)
    assert rc == 2 and 'fuzz-default log' in err and 'changed since the report' in err
    os.remove(log)
    rc, _, err, _ = gate(*eng, capsys=capsys)
    assert rc == 2 and 'is gone' in err
    rc, _, err, _ = gate(*eng, '--suite-report', os.path.join(gdir, 'nope.json'), capsys=capsys)
    assert rc == 2 and 'cannot read it' in err
    suite_report(gdir, SHA)
    rc, _, err, _ = gate(*eng, '--allow-skip', 'suite-evidence', capsys=capsys)
    assert rc == 2 and 'check it or skip it, not both' in err


def test_the_callers_fuzz_settings_never_reach_a_step(tmp_path, monkeypatch):
    for k, v in (('FUZZ_GAMES', '10'), ('FUZZ_SKIP', '0-19999'), ('FUZZ_POOL', 'delver')):
        monkeypatch.setenv(k, v)
    probe = [sys.executable, '-c', 'import os; print(" ".join(k + "=" + str(os.environ.get(k)) for k in '
             '("FUZZ_GAMES", "FUZZ_SKIP", "FUZZ_POOL")))']
    seen = {}
    step = G.Step('probe', probe, env={'FUZZ_GAMES': '20000'},
                  checker=lambda out, rc: (seen.setdefault('out', out) is not None, '', {}))
    assert G.run_step(probe_ctx(tmp_path, {}), step).passed
    assert seen['out'].split() == ['FUZZ_GAMES=20000', 'FUZZ_SKIP=None', 'FUZZ_POOL=None']


def test_a_detached_checkout_builds_into_its_own_target_dir(tmp_path):
    """B7: zero-core's build_engine.sh defaults CARGO_TARGET_DIR to <base>/<branch>, and the branch of EVERY detached
    checkout (the pinned run-<sha> worktrees, the *-base ones) is `HEAD`: they all built into <base>/HEAD. build_py.sh
    now names a detached checkout's dir after its directory (and refuses a dir marked for another checkout)."""
    import shutil
    import subprocess
    bash = shutil.which('bash')
    if not bash:
        pytest.skip('no bash')
    with open(os.path.join(ROOT, 'tools', 'build_py.sh'), encoding='utf-8') as f:
        sh = f.read().replace('\r\n', '\n')
    block = sh[sh.index('if [ -z "${CARGO_TARGET_DIR:-}" ]; then'):]
    block = block[:block.index('\nfi\n') + 4]
    r = _repo(tmp_path)

    def target(env_ctd=None):
        env = {k: v for k, v in os.environ.items() if k != 'CARGO_TARGET_DIR'}
        if env_ctd:
            env['CARGO_TARGET_DIR'] = env_ctd
        script = f'ROOT={str(r).replace(chr(92), "/")!r}; TDEF=E:/base\n{block}echo "$CARGO_TARGET_DIR"\n'
        return subprocess.run([bash, '-c', script], capture_output=True, text=True, env=env, check=True).stdout.strip()
    subprocess.run(['git', '-C', str(r), 'checkout', '-q', '-b', 'feat/x'], check=True)
    assert target() == 'E:/base/feat_x'
    subprocess.run(['git', '-C', str(r), 'checkout', '-q', '--detach'], check=True)
    assert target() == 'E:/base/detached-repo'                           # not E:/base/HEAD
    assert target('E:/mine') == 'E:/mine'                                # the caller's own choice stands
    assert 'target_guard.py' in sh and sh.index('target_guard.py') < sh.index('bash "$CORE/tools/build_engine.sh"')
    # (the artifact cache is asked after the guard, and a hit never reaches build_engine.sh: ENGINE.md 167)
    assert sh.index('target_guard.py') < sh.index('zcache.py" fetch pyd') < sh.index('bash "$CORE/tools/build_engine.sh"')


def test_head_is_read_from_the_git_files(tmp_path):
    """mtgz/checkout.read_head (B15's import-time HEAD: no git process in every process that imports the package):
    a branch's loose ref, a packed one, a detached HEAD, a worktree - each = `git rev-parse HEAD`."""
    import subprocess

    def head(root):
        return subprocess.run(['git', '-C', str(root), 'rev-parse', 'HEAD'], capture_output=True, text=True,
                              check=True).stdout.strip()
    r = _repo(tmp_path)
    assert G.checkout.read_head(str(r)) == head(r)
    subprocess.run(['git', '-C', str(r), 'pack-refs', '--all'], check=True)
    assert not (r / '.git' / 'refs' / 'heads' / 'master').exists() and not (r / '.git' / 'refs' / 'heads' / 'main').exists()
    assert G.checkout.read_head(str(r)) == head(r)
    wt = tmp_path / 'wt'
    subprocess.run(['git', '-C', str(r), 'worktree', 'add', '-q', '--detach', str(wt)], check=True)
    assert G.checkout.read_head(str(wt)) == head(wt)
    subprocess.run(['git', '-C', str(wt), 'checkout', '-q', '-b', 'feat/y'], check=True)
    assert G.checkout.read_head(str(wt)) == head(wt)
    assert G.checkout.read_head(str(tmp_path / 'nothing')) is None


# ---------------------------------------------------------------------------- farm compatibility (ENGINE.md 155)

FARM_TIERS = ((*ENC, '--allow-skip', 'checkpoint-load'),
              ('engine', '--expect-digest', D_FULL, '--expect-digest0', D_FREE, '--no-mac', '--allow-skip', 'suite-evidence'),
              ('search-core', '--allow-skip', 'checkpoint-compat'),
              ('optin-core', '--allow-skip', 'checkpoint-compat'),
              ('repin', '--expect-digest', D_FULL, '--spec', 'delver', '--preset', 'delver-mirror', '--no-mac'))


@pytest.mark.parametrize('argv', FARM_TIERS)
def test_every_engine_encoding_and_search_tier_builds_the_worker_and_checks_farm_compat(gate, capsys, argv):
    """"All builds need to be compatible with farm" (the user, 2026-11-14): the release worker is built from the commit
    and tools/farm_compat.py runs with it in each tier that covers engine, encoding or search changes; the worker is
    built before pytest (whose worker tests run it) and the step passes on the tool's own line."""
    gate.fake.calls.clear()
    if argv[0] == 'optin-core':
        argv = (*argv, '--other-games', *gate.other)
    rc, out, _, report = gate(*argv, capsys=capsys)
    kinds = [k for k, _ in gate.fake.calls]
    assert rc == 0 and kinds.count('worker-build') == 1 and kinds.count('farm-compat') == 1, (argv, kinds)
    if 'pytest' in kinds:
        assert kinds.index('worker-build') < kinds.index('pytest')
    assert kinds.index('worker-build') < kinds.index('farm-compat')
    fc = next(a for k, a in gate.fake.calls if k == 'farm-compat')
    assert fc[fc.index('--worker') + 1] == gate.fake.worker
    steps = {x['id']: x for x in report['steps']}
    assert steps['farm-compat']['status'] == 'passed' and steps['worker-build']['status'] == 'passed'
    assert steps['farm-compat']['farm_compat']['gate3']['setups'] == 6
    assert steps['worker-build']['worker'] == gate.fake.worker and len(steps['worker-build']['worker_sha256']) == 64


def test_the_docs_tier_builds_no_worker_and_the_python_tier_builds_it_for_pytest(gate, capsys):
    """docs: nothing built. python: the release worker built before pytest (its worker tests run it; before, they fell
    back to another checkout's stale build), no farm-compat step."""
    gate.fake.calls.clear()
    gate('docs', capsys=capsys)
    assert not {'worker-build', 'farm-compat'} & {k for k, _ in gate.fake.calls}
    gate.fake.calls.clear()
    rc, _, _, _ = gate('python', capsys=capsys)
    kinds = [k for k, _ in gate.fake.calls]
    # (pytest and the run-loop smoke run at once after the builds: ENGINE.md 167)
    assert rc == 0 and kinds[:2] == ['build', 'worker-build'] and sorted(kinds[2:]) == ['pytest', 'run']


def test_a_worker_that_breaks_farm_compat_fails_the_gate(gate, capsys):
    bad = dict(FARM_SUMMARY, passed=False)
    gate.fake.set('farm-compat', json.dumps(bad) + '\nfarm compat: FAILED - delver-mirror-bo3-dealt: gate 2 (worker vs the '
                  'Python remote worker) DIFFERENT or failed: 6 games, 1210 rows, 150 batches; gate 3 ...\n', 1)
    rc, out, _, report = gate(*FARM_TIERS[1], capsys=capsys)
    assert rc == 1 and final(out).startswith('GATE FAILED engine') and 'farm-compat: farm compat FAILED' in final(out)
    assert {x['id']: x['status'] for x in report['steps']}['farm-compat'] == 'failed'


def test_a_failed_worker_build_fails_before_pytest(gate, capsys):
    gate.fake.set('worker-build', 'error[E0425]: cannot find value `x` in this scope\n', 101)
    rc, out, _, _ = gate(*FARM_TIERS[1], capsys=capsys)
    kinds = [k for k, _ in gate.fake.calls]
    assert rc == 1 and kinds == ['build', 'worker-build'] and 'worker-build: exit 101' in final(out)


def test_the_farm_compat_checker_wants_positive_evidence():
    ok, why, extra = G.check_farm_compat(FARM_COMPAT_OK, 0)
    assert ok and why.startswith('delver-mirror-bo3-dealt: gate 2') and extra['farm_compat']['gate2']['rows'] == 1210
    assert G.check_farm_compat('', 0)[:2] == (False, '0 "farm compat:" lines (expected exactly 1)')
    assert not G.check_farm_compat(FARM_COMPAT_OK + FARM_COMPAT_OK.splitlines()[-1] + '\n', 0)[0]
    assert G.check_farm_compat(FARM_COMPAT_OK, 1)[1] == '"farm compat: PASSED" but exit 1'
    lying = FARM_COMPAT_OK.replace('"games": 6, "passed": true', '"games": 6, "passed": false')
    assert lying != FARM_COMPAT_OK and not G.check_farm_compat(lying, 0)[0]
    no_json = FARM_COMPAT_OK.splitlines()[-1] + '\n'
    assert not G.check_farm_compat(no_json, 0)[0], 'the line without its JSON summary is not evidence'
    # ENGINE.md 195: a worker that loses the feature `rolling` fails the step, even under a PASSED line
    for feats in ({'features': [], 'build_id': 'a' * 24, 'missing': ['rolling'], 'passed': True},
                  {'features': ['rolling'], 'build_id': None, 'missing': [], 'passed': False}, None):
        s = dict(FARM_SUMMARY)
        if feats is None:
            del s['features']
        else:
            s['features'] = feats
        bad = json.dumps(s) + '\n' + FARM_COMPAT_OK.splitlines()[-1] + '\n'
        ok, why, _ = G.check_farm_compat(bad, 0)
        assert not ok and 'rolling' in why, (feats, why)


def test_the_worker_build_checker(tmp_path):
    exe = str(tmp_path / G.WORKER_EXE)
    check = G.make_worker_build_checker(exe)
    assert check(WORKER_BUILT, 0)[:2] == (False, f'no {exe} after the build')
    open(exe, 'wb').write(b'w')
    ok, _, extra = check(WORKER_BUILT, 0)
    assert ok and extra['worker_sha256'] == G.sha256_file(exe)
    assert check('   Compiling mtgz_worker\n', 0)[:2] == (False, 'no "Finished `release` profile" line')
    assert check(WORKER_BUILT, 101)[:2] == (False, 'exit 101')


FUZZ_OUT = ('running 4 tests\n20000 games: wins [9803, 10075], draws 122, aborted 0, mean turns 31.8\n'
            'test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 9.10s\n')

# ---------------------------------------------------------------------------- placement (ENGINE.md 166)

LIVE = [{'pid': 4242, 'run': 'E:/mtg-zero/runs/delver-bo3-pb2'}]


def kinds(fake):
    return [(k, ' '.join(a)) for k, a in fake.calls]


def test_without_a_live_run_nothing_moves_and_nothing_waits_for_a_slot(gate, capsys):
    rc, out, _, report = gate('python', capsys=capsys)
    assert rc == 0 and final(out).startswith('GATE PASSED python')
    assert not any('slotd.sh' in c or 'mac.sh' in c for _, c in kinds(gate.fake))
    assert report['placement']['offload'] == 'desktop' and report['placement']['throttle'] is False
    assert {s['placement'] for s in report['steps'] if s['id'] not in ('tree-unchanged', 'privacy')} == {'desktop'}


def test_beside_a_live_run_the_heavy_steps_go_to_the_mac_and_the_rest_waits_for_a_slot(gate, capsys):
    gate.fake.live, gate.fake.mac = LIVE, (True, 'the Mac: load 6, 40 GB available')
    rc, out, _, report = gate('python', capsys=capsys)
    assert rc == 0 and final(out).startswith('GATE PASSED python'), out
    calls = kinds(gate.fake)
    by = {k: [c for kk, c in calls if kk == k] for k in ('build', 'worker-build', 'pytest', 'mac')}
    assert all(c.startswith('bash tools/slotd.sh ') for c in by['build'] + by['worker-build'] + by['pytest'])
    mac = by['mac']
    # (the module built on the Mac once, before the pytest and the smoke that import it there at once: ENGINE.md 238)
    assert len(mac) == 3 and all('nice -n 10' in c and 'tools/slotd.sh' in c and '-u PYTEST_ADDOPTS' in c for c in mac)
    assert mac[0].endswith("bash -c 'bash tools/build_py.sh'") and all(G.MAC_BUILT in c for c in mac[1:])
    ids = [s['id'] for s in report['steps']]
    assert ids.index('worker-build') < ids.index('mac-build') < min(ids.index('pytest'), ids.index('run-loop-smoke'))
    pytest_mac = next(c for c in mac if 'pytest' in c)
    assert 'bash tools/build_py.sh && python -m pytest tests' in pytest_mac
    for a in G.PYTEST_DESKTOP_ONLY:                   # the Mac leaves out exactly what the desktop part runs
        assert (f'--deselect={a}' if '::' in a else f'--ignore={a}') in pytest_mac
        assert a in by['pytest'][0]
    steps = {s['id']: s for s in report['steps']}
    assert steps['pytest']['placement'] == 'mac' and steps['pytest']['mac']
    assert steps['run-loop-smoke']['placement'] == 'mac' and steps['run-loop-smoke']['reason'] == '16 games, 0 unfinished'
    assert steps['pytest-desktop']['placement'] == G.THROTTLED and steps['build']['placement'] == G.THROTTLED
    assert steps['mac-build']['placement'] == 'mac' and '/mtgz/_engine.so' in steps['mac-build']['reason']
    assert report['placement']['live_runs'] == LIVE and 'placement: 3 step(s) on the Mac' in out


def test_no_mac_keeps_everything_on_the_desktop_but_throttled(gate, capsys):
    gate.fake.live, gate.fake.mac = LIVE, (True, 'the Mac: load 6')
    rc, out, _, report = gate('python', '--no-mac', capsys=capsys)
    assert rc == 0 and not any('mac.sh' in c for _, c in kinds(gate.fake))
    assert all(s['placement'] == G.THROTTLED for s in report['steps'] if s['id'] not in ('tree-unchanged', 'privacy'))
    assert '--no-mac: nothing on the Mac' in report['placement']['summary']
    assert all(c.startswith('bash tools/slotd.sh ') for k, c in kinds(gate.fake))


def test_an_unreachable_mac_leaves_the_steps_on_the_desktop_throttled(gate, capsys):
    gate.fake.live, gate.fake.mac = LIVE, (False, 'the Mac (x) did not answer: TimeoutExpired')
    rc, out, _, report = gate('python', capsys=capsys)
    assert rc == 0 and not any('mac.sh' in c for _, c in kinds(gate.fake))
    assert 'did not answer' in report['placement']['summary']
    rc, out, err, report = gate('python', '--offload', 'mac', capsys=capsys)
    assert rc == 2 and '--offload mac: the Mac (x) did not answer' in err


def test_a_mac_step_that_never_reached_the_mac_runs_on_the_desktop(gate, capsys):
    gate.fake.live, gate.fake.mac = LIVE, (True, 'the Mac: up')
    gate.fake.set('mac', 'ssh: connect to host 192.0.2.10 port 22: Connection timed out\n', 255)
    rc, out, _, report = gate('python', capsys=capsys)
    assert rc == 0, out
    steps = {s['id']: s for s in report['steps']}
    assert steps['pytest']['placement'] == G.THROTTLED and steps['run-loop-smoke']['placement'] == G.THROTTLED
    log = open(steps['pytest']['log'], encoding='utf-8').read()
    assert 'the Mac was not reached' in log and 'Connection timed out' in log


def test_tests_the_live_run_fence_skipped_make_it_a_pass_with_skips(gate, capsys):
    gate.fake.set('pytest', '..s.\n=========================== short test summary info ===========================\n'
                  'SKIPPED [2] tests/test_loop.py:100: live-run fence: 1 live run(s) on this machine (E:/r); the test '
                  'starts supervisors\n69 passed, 2 skipped in 10.00s\n')
    rc, out, _, report = gate('python', capsys=capsys)
    assert rc == 0 and final(out).startswith('GATE PASSED WITH SKIPS python'), final(out)
    assert 'pytest: 2 live-run-hazard test(s) (fenced' in final(out)
    assert {s['id']: s for s in report['steps']}['pytest']['fenced'] == 2


def test_the_suite_on_the_mac_carries_its_fuzz_settings_and_nothing_of_the_callers(gate, capsys, monkeypatch):
    monkeypatch.setenv('FUZZ_SKIP', '5')
    gate.fake.live, gate.fake.mac = LIVE, (True, 'the Mac: up')
    rc, out, _, report = gate('suite', '--fuzz-pools', 'default,delver', capsys=capsys)
    assert rc == 0, out
    mac = [c for k, c in kinds(gate.fake) if k == 'mac']
    assert len(mac) == 4 and all('FUZZ_SKIP' not in c.split('unset')[-1].split('bash tools/slotd.sh')[0] for c in mac)
    # (the sharded fuzz on the Mac's checkout, ENGINE.md 167: its pool and games on the command line, the shards' own
    # FUZZ_* set by tools/fuzz_shards.py, the caller's never)
    fuzz = [c for c in mac if 'fuzz_shards.py' in c]
    assert len(fuzz) == 2 and all('--games 2000' in c for c in fuzz) and any('--pool delver' in c for c in fuzz)
    assert all(s['placement'] == 'mac' for s in report['steps'] if s['id'] not in ('tree-unchanged', 'privacy'))
    assert report['passed'] and report['clean']        # (the engine tier's suite evidence wants exactly that)


def test_remote_command_scrubs_and_keeps_the_step_env():
    cmd = G.remote_command(['cargo', 'test', '--release'], {'DIGEST_ENCODING': '0', 'X': None},
                           os.path.join(G.ROOT, 'third_party', 'zero-core'))
    assert 'unset "$v"' in cmd and '-u PYTHONPATH' in cmd and '-u X' in cmd and 'DIGEST_ENCODING=0' in cmd
    assert "cd third_party/zero-core && cargo test --release" in cmd and cmd.count('bash tools/slotd.sh nice -n 10') == 1
    assert G.remote_command(['bash', '-c', 'a && b']).endswith("bash -c 'a && b'")


def test_mac_never_reached_and_fenced_counts():
    assert G.mac_never_reached(MAC(SHA, 'x\n')) is None
    assert 'no "[mac]' in G.mac_never_reached('ssh: no route\n')
    assert G.fenced_tests('SKIPPED [3] a.py:1: live-run fence: x\nSKIPPED [1] b.py:2: other\n'
                          'SKIPPED [1] c.py:3: live-run fence: y\n') == 4


# ---------------------------------------------------------------------------- ENGINE.md 238: the Mac at once, core shards

def test_the_mac_module_checker():
    ok = G.check_mac_module(MAC_MODULE, 0)
    assert ok[0] and ok[2]['module'].endswith('/mtgz/_engine.so')
    assert G.check_mac_module("mtgz._engine OK: C:\\x\\mtgz\\_engine.pyd | games: ['mtg']\n", 0)[0]   # (the desktop fallback)
    assert not G.check_mac_module(MAC_MODULE, 1)[0]
    assert not G.check_mac_module('   Compiling engine\nerror: could not compile\n', 0)[0]
    assert not G.check_mac_module('mtgz._engine OK: /x/mtgz/_engine.so | games: []\n', 0)[0]
    assert not G.check_mac_module('zero._engine OK: /x/python/zero/_engine.so | games: [1]\n', 0)[0]


def test_mac_steps_that_never_import_the_module_get_no_build_step(gate, capsys):
    """The module is built on the Mac ahead of a group of steps that import it; a tier whose Mac steps never import it
    (the suite: cargo and the fuzz) gets no such step."""
    gate.fake.live, gate.fake.mac = LIVE, (True, 'the Mac: load 6, 40 GB available')
    rc, out, _, report = gate('suite', capsys=capsys)
    assert rc == 0, out
    assert 'mac-build' not in [s['id'] for s in report['steps']]


def test_a_mac_shard_has_its_own_basetemp_removed_after(monkeypatch):
    monkeypatch.setattr(G, 'read_durations', lambda *a: {})
    steps = G.pytest_step(ctx=types.SimpleNamespace(pytest_shards=3, sha_short=SHA[:12]))
    macs = [s.mac_cmd(None)[0][2] for s in steps if s.mac_cmd]
    bases = [m.split('--basetemp=')[1].split()[0] for m in macs]
    assert len(set(bases)) == 3 and all(b.startswith(f'gates/tmp/pytest-{SHA[:12]}-') for b in bases)
    # its junit file read on the Mac (the tests' seconds printed for the durations file: ENGINE.md 238.9), then removed
    assert all(f'--junitxml={b}.xml; rc=$?; python tools/gate.py --junit-seconds {b}.xml; rm -rf {b} {b}.xml; exit $rc; }}'
               in m for m, b in zip(macs, bases))
    assert all(s.mac_checker is not None for s in steps if s.mac_cmd)
    assert all('{ mkdir -p gates/tmp && python -m pytest ' in m for m in macs)      # (pytest makes no parents)


def test_the_jobs_default_is_the_machines_setting(tmp_path, monkeypatch):
    monkeypatch.delenv('CARGO_BUILD_JOBS', raising=False)
    monkeypatch.setenv('MTGZ_SLOTS_HOME', str(tmp_path))
    assert G.env_jobs() == 4                                    # no slots.conf
    (tmp_path / 'slots.conf').write_text('# settings\nslots=4\n jobs = 10\n', encoding='utf-8')
    assert G.env_jobs() == 10
    monkeypatch.setenv('CARGO_BUILD_JOBS', '6')                 # the caller's own wins
    assert G.env_jobs() == 6


def test_the_core_pytest_runs_in_shards_every_test_once(tmp_path, monkeypatch):
    """ENGINE.md 238: zero-core's pytest as --core-pytest-shards steps over disjoint parts (the desktop's: heavy, never
    a Mac command), each its own process, --basetemp and junit file, with the flaky policy; its durations in their own
    file."""
    core = tmp_path / 'core'
    (core / 'tests').mkdir(parents=True)
    names = sorted([f'tests/test_{c}.py' for c in 'abcdefg'] + [REMOTEPLAY.split('::')[0]])
    for k, n in enumerate(names):
        (core / n).write_text('#' * (2000 * (k + 1)), encoding='utf-8')
    (core / 'tests' / 'conftest.py').write_text('', encoding='utf-8')
    monkeypatch.setattr(G, 'CORE', str(core))
    monkeypatch.setattr(G, 'GATES_DIR', str(tmp_path / 'gates'))
    monkeypatch.setattr(G, 'WIN', False)
    seen = []
    monkeypatch.setattr(G, 'read_durations', lambda name=None: seen.append(name) or {})
    ctx = types.SimpleNamespace(sha_short=SHA[:12], core_pytest_shards=3)
    build, *shards = G.core_steps(ctx)
    assert build.id == 'core-build' and [s.id for s in shards] == [f'core-pytest-{k}-of-3' for k in (1, 2, 3)]
    assert seen == [G.CORE_PYTEST_DURATIONS] and G.CORE_PYTEST_DURATIONS != G.PYTEST_DURATIONS
    assert all(s.cwd == str(core) and s.heavy and s.mac_cmd is None and s.flaky.file == G.FLAKY_FILE for s in shards)
    assert sorted(a for s in shards for a in s.argv if a.startswith('tests/')) == names
    bases = [next(a for a in s.argv if a.startswith('--basetemp=')) for s in shards]
    xmls = [next(a for a in s.argv if a.startswith('--junitxml=')) for s in shards]
    assert len(set(bases)) == 3 and len(set(xmls)) == 3
    retry = shards[1].flaky.argv(REMOTEPLAY, 1)
    assert REMOTEPLAY in retry and bases[1] + '-retry1' in retry and not any(a.startswith('--junitxml') for a in retry)
    # a measured file longer than a shard's share is split by its tests, each test in one shard
    w = {names[0]: 900.0, **{f'{names[0]}::t{i}': 100.0 for i in range(9)}}
    monkeypatch.setattr(G, 'read_durations', lambda name=None: w)
    _, *split = G.core_steps(ctx)
    named = [a for s in split for a in s.argv if a.startswith(names[0] + '::')]
    desel = [a[len('--deselect='):] for s in split for a in s.argv if a.startswith('--deselect=')]
    assert named and sorted(named) == sorted(desel)
    # one shard, or a core without two test files: the one step, as before
    one = G.core_steps(types.SimpleNamespace(sha_short=SHA[:12], core_pytest_shards=1))
    assert [s.id for s in one] == ['core-build', 'core-pytest'] and one[1].argv[3:5] == ['tests', '-q']
    for n in names[1:]:
        os.remove(core / n)
    assert [s.id for s in G.core_steps(ctx)] == ['core-build', 'core-pytest']


def test_the_core_tier_runs_every_core_shard_and_a_failed_one_fails_the_gate(gate, capsys):
    core = gate.fake.core
    os.makedirs(os.path.join(core, 'tests'))
    for c in 'abcd':
        with open(os.path.join(core, 'tests', f'test_{c}.py'), 'w', encoding='utf-8') as f:
            f.write('#' * 4000)
    args = ('optin-core', '--baseline-root', gate.baseline, '--other-games', *gate.other, '--core-pytest-shards', '2')
    rc, out, _, report = gate(*args, capsys=capsys)
    assert rc == 0, out
    assert [k for k, _ in gate.fake.calls].count('core-pytest') == 2
    ids = [s['id'] for s in report['steps']]
    assert 'core-pytest-1-of-2' in ids and 'core-pytest-2-of-2' in ids and 'core-pytest' not in ids
    gate.fake.calls.clear()
    gate.fake.core_pytest = [('.\n9 passed in 1.00s\n', 0), (PYTEST_FAILED('tests/test_a.py::test_x', passed=3), 1)]
    rc, out, _, report = gate(*args, capsys=capsys)
    assert rc == 1 and final(out).startswith('GATE FAILED optin-core') and 'core-pytest-' in final(out)


def test_a_mac_shards_seconds_reach_the_durations_file(tmp_path, monkeypatch, capsys):
    """ENGINE.md 238.9: a shard on the Mac prints its junit file's seconds (`gate.py --junit-seconds`), and its checker
    merges them into the durations file once the shard passed - never after a failure, and a line that is not a JSON
    object of numbers is ignored."""
    xml = tmp_path / 'shard.xml'
    xml.write_text('<testsuites><testsuite>'
                   '<testcase classname="tests.test_x" name="test_a" time="1.5"/>'
                   '<testcase classname="tests.test_x.TestC" name="test_b[1]" time="0.25"/>'
                   '<testcase classname="tests.ir.test_y" name="test_c" time="2"/>'
                   '</testsuite></testsuites>', encoding='utf-8')
    assert G.main(['--junit-seconds', str(xml)]) == 0
    line = capsys.readouterr().out.strip()
    assert line.startswith(G.SECONDS_PREFIX) and '\n' not in line
    want = {'tests/test_x.py::test_a': 1.5, 'tests/test_x.py::TestC::test_b[1]': 0.25, 'tests/ir/test_y.py::test_c': 2.0}
    assert G.parse_seconds(f'...\n3 passed in 4.00s\n{line}\n[mac] done\n') == want
    assert G.main(['--junit-seconds', str(tmp_path / 'missing.xml')]) == 0 and capsys.readouterr().out == ''
    assert G.parse_seconds('no such line') == {} and G.parse_seconds(G.SECONDS_PREFIX + '{broken') == {}
    assert G.parse_seconds(G.SECONDS_PREFIX + '{"t": "x"}') == {} and G.parse_seconds(G.SECONDS_PREFIX + '[1]') == {}
    monkeypatch.setattr(G, 'durations_dir', lambda: str(tmp_path))
    (tmp_path / G.PYTEST_DURATIONS).write_text(json.dumps({'tests/test_x.py': 9.0, 'tests/test_x.py::test_a': 9.0,
                                                           'tests/test_old.py': 3.0}), encoding='utf-8')
    check = G.recording(G.check_pytest)
    out = f'.\n3 passed in 4.00s\n{line}\n'
    assert check(out.replace('3 passed', '1 failed, 2 passed'), 1)[0] is False          # a failed shard records nothing
    assert json.load(open(tmp_path / G.PYTEST_DURATIONS, encoding='utf-8'))['tests/test_x.py'] == 9.0
    assert check(out, 0)[0] is True
    d = json.load(open(tmp_path / G.PYTEST_DURATIONS, encoding='utf-8'))
    assert d['tests/test_x.py'] == 1.75 and d['tests/test_x.py::test_a'] == 1.5 and d['tests/ir/test_y.py'] == 2.0
    assert d['tests/test_old.py'] == 3.0 and not os.path.exists(tmp_path / (G.PYTEST_DURATIONS + '.lock'))
    assert check('.\n3 passed in 4.00s\n', 0)[0] is True                                # no line: nothing to record
    # the desktop's own way is unchanged: the junit file read, merged and removed
    G.record_durations(str(xml))
    assert not xml.exists() and json.load(open(tmp_path / G.PYTEST_DURATIONS, encoding='utf-8'))['tests/test_x.py'] == 1.75
