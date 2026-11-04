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


def MAC(sha, body):
    return f'[mac] Greggs-MacBook-Pro somebranch @ {sha[:9]}: cargo test --release --no-fail-fast\n' + body


def EVAL_CHECKOUT(sha):
    wt = f'/Users/greggkeithley/mtg-zero-wt/run-{sha[:12]}'
    return (f'[mac] Greggs-MacBook-Pro run-{sha[:12]} @ {sha[:9]}: set -euo pipefail\nbash tools/build_py.sh\n'
            f"    Finished `release` profile [optimized + debuginfo] target(s) in 41.12s\n"
            f"mtgz._engine OK: {wt}/mtgz/_engine.so | games: ['toy', 'mtg', 'team_draft']\n"
            f'mtgz remote: {wt} is {sha[:12]}, unchanged, its engine built there\neval checkout ready: {wt}\n')


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


# ---------------------------------------------------------------------------- the fake gate

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
        if 'build_py.sh' in cmd:
            return 'core-build' if 'third_party' in cmd or self.cwd == self.core else 'build'
        if 'smoke_all.sh' in cmd:
            return 'build-only'
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
        return {'build': BUILD(G.ROOT), 'core-build': CORE_BUILD(self.core), 'digest0': DIGEST(D_FREE),
                'digest': DIGEST(D_FULL), 'cargo1': CARGO(34), 'cargo': CARGO(123, 3, 0, 0), 'pytest': PYTEST_OK,
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
                                     no_mac=args.no_mac, dirty=[], baseline=None)
    monkeypatch.setattr(G, 'Ctx', fake_ctx)
    fake = Fake(str(core))
    fake.diff = 'docs/ENGINE.md\n'
    monkeypatch.setattr(G, 'execute', fake)
    other = [str(tmp_path / g) for g in GAMES]
    for p in other:
        os.makedirs(p)

    def run(*argv, capsys=None):
        rc = G.main(list(argv))
        out, err = capsys.readouterr() if capsys else ('', '')
        rpath = os.path.join(str(tmp_path / 'gates'), f'{SHA[:12]}.json')
        report = json.load(open(rpath, encoding='utf-8')) if os.path.exists(rpath) else None
        if report:
            os.remove(rpath)
        return rc, out, err, report
    run.fake, run.baseline, run.other = fake, str(baseline), other
    return run


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
    assert [s['status'] for s in report['steps']] == ['passed', 'passed']        # py-compile, tree-unchanged


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
    rc, out, _, _ = gate('engine', '--expect-digest', D_FULL, '--expect-digest0', D_FREE, '--no-mac', capsys=capsys)
    assert rc == 0 and 'mac-recheck (--no-mac: no --tested-sha, so whether the recheck was needed was not decided)' \
        in final(out)


def test_the_engine_tiers_mac_recheck_is_decided_by_the_tested_sha(gate, capsys):
    eng = ('engine', '--expect-digest', D_FULL, '--expect-digest0', D_FREE)
    rc, out, _, report = gate(*eng, '--tested-sha', SHA, capsys=capsys)        # gated directly: not needed, not a skip
    assert rc == 0 and final(out).startswith('GATE PASSED engine') and 'N/A' in out and report['clean']
    gate.fake.diff = 'engine/src/game.rs\ndocs/ENGINE.md\n'
    rc, out, _, report = gate(*eng, '--tested-sha', '1234567', capsys=capsys)  # overlapping engine/ files: the Mac runs
    assert rc == 0 and 'mac' in [k for k, _ in gate.fake.calls] and final(out).startswith('GATE PASSED engine')
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
    assert 'not 276f948e6' in check(MAC('9' * 40, CARGO(900)), 0)[1]
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
    assert rc == 1 and 'tree-unchanged: tracked files changed during the gate: M engine/tests/data/digests.txt' \
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
    assert step.argv[3:5] == ['tests', '-q'] and '-rfE' in step.argv
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
    assert kinds == ['build', 'cargo', 'compat', 'cargo1', 'cargo1', 'build-only', 'core-build', 'core-pytest',
                     'core-pytest']
    assert rc == 0 and final(out).startswith('GATE PASSED WITH FLAKY RETRIES optin-core')
    assert report['flaky'] == [f'core-pytest: {REMOTEPLAY}'] and report['baseline']['head'] == 'c' * 40
    gate.fake.core_pytest = []
    rc, out, _, _ = gate('search-core', '--baseline-root', gate.baseline, capsys=capsys)
    assert rc == 0 and final(out).startswith('GATE PASSED search-core')
    assert [k for k, _ in gate.fake.calls][-8:] == ['build', 'cargo', 'pytest', 'compat', 'smoke', 'run', 'core-build',
                                                    'core-pytest']


def test_a_stale_build_fails_the_core_tier_before_the_comparison(gate, capsys):
    gate.fake.set('build', '    Finished `release` profile\nImportError: DLL load failed\n', 0)
    rc, out, _, report = gate('optin-core', '--baseline-root', gate.baseline, '--other-games', *gate.other,
                              capsys=capsys)
    assert rc == 1 and [k for k, _ in gate.fake.calls] == ['build']
    assert 'build: no "mtgz._engine OK' in final(out) and 'not run: core-cargo-test, checkpoint-compat' in final(out)


# ---------------------------------------------------------------------------- dry runs

@pytest.mark.parametrize('argv, on_success', [
    (('docs',), 'on success: GATE PASSED (every step on positive evidence)'),
    (('python', '--allow-skip', 'build'), 'on success: GATE PASSED WITH SKIPS - skipped: build (--allow-skip build'),
    ((*ENC, '--checkpoint', __file__), 'on success: GATE PASSED (every step'),
    (('engine', '--expect-digest', D_FULL, '--record-digest', 'encoding-free', '--tested-sha', SHA),
     'recorded, not compared: digest-encoding-free (--record-digest)'),
    (('search-core', '--allow-skip', 'checkpoint-compat'), 'skipped: checkpoint-compat (--allow-skip'),
    (('repin', '--expect-digest', D_FULL, '--spec', 'delver', '--preset', 'delver-mirror'), 'on success: GATE PASSED ('),
])
def test_dry_run_prints_the_plan_and_what_a_pass_would_say(gate, capsys, argv, on_success):
    rc, out, _, report = gate(*argv, '--dry-run', capsys=capsys)
    assert rc == 0 and on_success in out and report is None and not gate.fake.calls
    assert 'passes on:' in out
