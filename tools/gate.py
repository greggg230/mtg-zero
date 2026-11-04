"""The tiered verification gate (feedback_tiered_gates, user-approved 2026-10-29; docs/ENGINE.md
70): one command per tier, each running exactly that tier's recipe instead of an agent's ad-hoc
shell script. Replaces the hand-written gate_<name>.sh scripts (same recipes, two bugs fixed):

  * **No shared log file.** Each step gets its own log, opened and written by this process alone
    (never `tee`d into by a child too): the old scripts' `cmd | tee /dev/stderr | grep ...` could
    truncate the log with NUL bytes when the child and `tee` raced on the same file.
  * **Positive evidence, not absence of FAILED.** `grep -q FAILED && fail` passes silently when a
    build fails before any test runs (no "test result:" line at all, so no FAILED line either).
    Every checker here requires a positive marker (a "test result: ok" line, a "combined digest"
    line, "N passed", "0 failed of N configurations", ...) and fails loudly when it is missing.

Run from WITHIN the checkout to gate (a pinned worktree the caller creates first, e.g. `git
worktree add --detach ../mtg-zero-wt/run-<sha> <sha>`, exactly as the old scripts did) - this tool
never creates or manages a worktree itself, the same contract as tools/build_py.sh and
tools/search_smoke.py. `--sha` labels the report and the target dir, and (if the checkout's HEAD
differs) fails loudly rather than silently gating the wrong commit.

    python tools/gate.py docs
    python tools/gate.py python
    python tools/gate.py encoding --expect-digest 65e8a5155bd2000c --expect-digest0 c62a5ce5f290814e
    python tools/gate.py engine --expect-digest <the agent's full digest> --expect-digest0 <...>
    python tools/gate.py search-core --baseline-root ../other-checkout
    python tools/gate.py optin-core --baseline-root ../other-checkout
    python tools/gate.py repin --expect-digest <main's recorded digest> --spec delver --preset delver-mirror
    python tools/gate.py <tier> --dry-run      # print the plan, run nothing

Writes `gates/<sha>.json` (sha, zero-core pin, tier, digests, test counts per binary, timings,
log paths + sha256, the cargo target dir, pass/fail + reason per step) and one log per step under
`gates/logs/`. CPU only (CUDA_VISIBLE_DEVICES=-1, never ''), CARGO_BUILD_JOBS=4 by default, below
normal process priority; a Mac part of a tier (tools/mac.sh) runs in a thread alongside its
desktop counterpart so neither waits on the other needlessly - pass --no-mac to skip Mac steps
(they are marked skipped, not failed, in the report).

A checkout marker in the cargo target dir (`.gate_checkout.json`) records which checkout last
built there; a second checkout pointed at the same CARGO_TARGET_DIR is refused instead of silently
reusing the first one's relative dep-info paths (reference_cargo_shared_target_dir.md: this is
exactly how a scratch worktree's build quietly became the real worktree's build before).
"""
import argparse
import dataclasses
import glob
import json
import os
import re
import shutil
import subprocess
import sys
import threading
import uuid
import time
from datetime import datetime, timezone

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CORE = os.path.join(ROOT, 'third_party', 'zero-core')
GATES_DIR = os.path.join(ROOT, 'gates')
WIN = sys.platform == 'win32'


class GateError(Exception):
    """A configuration or setup problem (not a step failure): aborts before running anything."""


def iso_now():
    return datetime.now(timezone.utc).isoformat()


def sha256_file(path):
    h = __import__('hashlib').sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()


def git(args, cwd=ROOT, check=True):
    p = subprocess.run(['git', *args], cwd=cwd, capture_output=True, text=True)
    if check and p.returncode:
        raise GateError(f'git {" ".join(args)} ({cwd}): {p.stderr.strip()}')
    return p.stdout.strip()


def slug(name):
    return re.sub(r'[^a-z0-9]+', '-', name.lower()).strip('-')


def lazy_run_dir(ctx, tag):
    """A unique path under gates/tmp for a throwaway run-loop smoke, WITHOUT creating it (dry-run
    must not touch disk; `zero.run` makes the directory itself on first write)."""
    return os.path.join(GATES_DIR, 'tmp', f'gate-{ctx.tier}-{tag}-{ctx.sha_short}-{uuid.uuid4().hex[:8]}')


# ---------------------------------------------------------------------------- checkers (positive evidence)

TEST_RESULT_RE = re.compile(r'^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; \d+ ignored; \d+ measured; '
                             r'\d+ filtered out', re.M)


def check_cargo_test(output, rc):
    """Requires at least one "test result:" line (a compiler error prints none - that is a FAILED,
    never a silent pass) and zero failures across every line found (`cargo test` with several
    `--test` targets prints one such line per binary)."""
    matches = TEST_RESULT_RE.findall(output)
    if not matches:
        return False, 'no "test result:" line in the output (did it even compile?)', {}
    failed = sum(int(m[2]) for m in matches)
    passed = sum(int(m[1]) for m in matches)
    ok = rc == 0 and failed == 0 and all(m[0] == 'ok' for m in matches)
    reason = f'{passed} passed, {failed} failed across {len(matches)} binary/binaries' if not ok else \
        f'{passed} passed ({len(matches)} binary/binaries)'
    return ok, reason, {'passed': passed, 'failed': failed, 'binaries': len(matches)}


DIGEST_RE = re.compile(r'combined digest ([0-9a-f]+)')


def make_digest_checker(expect=None):
    def checker(output, rc):
        m = DIGEST_RE.search(output)
        if rc != 0 or not m:
            return False, 'no "combined digest" line in the output (did it compile/run?)', {}
        d = m.group(1)
        if expect and d != expect:
            return False, f'digest {d} != expected {expect}', {'digest': d}
        return True, f'digest {d}' + (' (= expected)' if expect else ' (recorded)'), {'digest': d}
    return checker


def check_pytest(output, rc):
    m = re.search(r'(\d+) passed', output)
    bad = re.search(r'(\d+) (failed|error)', output)
    if not m:
        return False, 'no "N passed" line in the output (collection error or crash?)', {}
    if rc != 0 or (bad and int(bad.group(1)) > 0):
        return False, f'{bad.group(1)} {bad.group(2)}' if bad else f'exit {rc}', {}
    return True, f'{m.group(1)} passed', {'passed': int(m.group(1))}


def check_pycompile(output, rc):
    return rc == 0, ('compiled cleanly' if rc == 0 else output.strip().splitlines()[-1:] or f'exit {rc}'), {}


SMOKE_RE = re.compile(r'search smoke: (\d+) failed of (\d+) configurations')


def check_search_smoke(output, rc):
    m = SMOKE_RE.search(output)
    if not m:
        return False, 'no "search smoke: ... failed of ... configurations" summary line', {}
    bad, total = int(m.group(1)), int(m.group(2))
    ok = rc == 0 and bad == 0
    return ok, (f'0/{total} configurations failed' if ok else f'{bad}/{total} configurations failed'), \
        {'configurations': total, 'failed': bad}


def check_h2h_unfinished(output, rc):
    if rc != 0:
        return False, f'exit {rc}', {}
    m = re.search(r'"unfinished":\s*(\d+)', output)
    if not m:
        return False, 'no "unfinished" field in the h2h output', {}
    n = int(m.group(1))
    return n == 0, (f'checkpoint loads and plays (unfinished: {n})' if n == 0 else f'{n} unfinished'), {}


def check_checkpoint_compat(output, rc):
    if rc != 0:
        return False, f'check_checkpoint_compat exit {rc}: ' + output.strip().splitlines()[-1:][0] if output.strip() \
            else f'exit {rc}', {}
    return True, 'identical across both checkouts', {}


BUILD_OK_RE = re.compile(r'^\s*ok\s+(\S+): build \S+ \(\d+ s\)\s*$', re.M)
WITH_CORE_RE = re.compile(r'^=== (\S+): worktree .* with zero-core ([0-9a-f]+)\s*$', re.M)


def make_build_only_checker(names, core_pin=None):
    """smoke_all.sh --with-this-core --build-only's positive evidence: an "ok <game>: build ..." summary
    line for EVERY game the step names (and, with a known pin, its "worktree ... with zero-core <sha>"
    line matching this checkout's core) - not just exit 0 / GREEN, which smoke_all printed with no
    game at all when it found none (ENGINE.md 90)."""
    def checker(output, rc):
        built = set(BUILD_OK_RE.findall(output)) - {'core'}       # ("ok core: build zero._engine" is the core's)
        missing = [n for n in names if n not in built]
        if missing:
            return False, (f'no "ok <game>: build" line for {", ".join(missing)} (built: '
                           f'{", ".join(sorted(built)) or "none"}) - a game never built is not a pass'), \
                {'built': sorted(built)}
        if core_pin:
            cores = dict(WITH_CORE_RE.findall(output))
            wrong = [n for n in names if not core_pin.startswith(cores.get(n) or 'x')]
            if wrong:
                return False, (f'{", ".join(wrong)}: not built in a worktree with zero-core {core_pin[:12]} '
                               f'(seen: {cores or "none"})'), {'built': sorted(built)}
        if rc != 0:
            return False, f'exit {rc}: ' + (output.strip().splitlines() or ['?'])[-1][:300], {'built': sorted(built)}
        return True, f'{len(names)} game(s) built against this core: {", ".join(names)}', {'built': sorted(built)}
    return checker


def make_metrics_checker(run_dir, want_games=None, want_iters=1):
    """A run-loop smoke's real positive evidence: metrics.jsonl itself (not log text) - every
    iteration present, no unfinished games, the expected game count."""
    def checker(output, rc):
        if rc != 0:
            return False, f'exit {rc}', {}
        mfile = os.path.join(run_dir, 'metrics.jsonl')
        if not os.path.exists(mfile):
            return False, 'no metrics.jsonl written', {}
        rows = [json.loads(line) for line in open(mfile, encoding='utf-8')]
        iters = [r['iter'] for r in rows]
        if iters != list(range(want_iters + 1)):
            return False, f'iterations {iters} != {list(range(want_iters + 1))}', {}
        sp = rows[-1]['selfplay']
        if sp.get('unfinished', 0) != 0:
            return False, f'{sp["unfinished"]} unfinished games', {}
        if want_games is not None and sp.get('games') != want_games:
            return False, f'{sp.get("games")} games != expected {want_games}', {}
        return True, f'{sp.get("games")} games, 0 unfinished', {'games': sp.get('games')}
    return checker


# ---------------------------------------------------------------------------- steps and execution

@dataclasses.dataclass
class Step:
    name: str
    argv: list
    cwd: str = None
    env: dict = None
    checker: object = check_cargo_test        # callable(output, rc) -> (passed, reason, extra)
    mac: bool = False
    group: str = None                          # steps sharing a group run concurrently
    skip_reason: str = None                    # set -> the step is skipped, not run
    cleanup: object = None                     # callable() run after the step (e.g. rmtree a tmp dir)


@dataclasses.dataclass
class StepResult:
    name: str
    argv: list
    cwd: str
    seconds: float
    rc: object
    log: str
    log_sha256: str
    passed: bool
    reason: str
    extra: dict
    mac: bool = False
    skipped: bool = False


def run_step(ctx, step):
    if step.skip_reason:
        return StepResult(step.name, step.argv, step.cwd or ctx.root, 0.0, None, None, None, True,
                           f'skipped: {step.skip_reason}', {}, mac=step.mac, skipped=True)
    log_path = os.path.join(ctx.logs_dir, f'{ctx.sha_short}-{ctx.tier}-{slug(step.name)}.log')
    env = dict(os.environ)
    env.update(ctx.base_env)
    # DIGEST_ENCODING / DIGEST_IDS must never leak between steps: each step states what it wants.
    env.pop('DIGEST_ENCODING', None)
    env.pop('DIGEST_IDS', None)
    for k, v in (step.env or {}).items():   # None = unset (e.g. the gate's own CARGO_TARGET_DIR)
        if v is None:
            env.pop(k, None)
        else:
            env[k] = v
    popen_kwargs = {}
    if WIN:
        popen_kwargs['creationflags'] = subprocess.BELOW_NORMAL_PRIORITY_CLASS
    t0 = time.time()
    try:
        p = subprocess.run(step.argv, cwd=step.cwd or ctx.root, env=env, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, text=True, errors='replace', timeout=ctx.step_timeout,
                            **popen_kwargs)
        out, rc = p.stdout, p.returncode
    except subprocess.TimeoutExpired as e:
        out = (e.stdout or '') + f'\n[gate.py: killed after {ctx.step_timeout}s timeout]\n'
        rc = 'timeout'
    seconds = time.time() - t0
    with open(log_path, 'w', encoding='utf-8', errors='replace') as f:
        f.write(f'$ {" ".join(step.argv)}\ncwd: {step.cwd or ctx.root}\nenv overrides: {step.env or {}}\n\n')
        f.write(out)
    if rc == 'timeout':
        passed, reason, extra = False, f'timed out after {ctx.step_timeout}s', {}
    else:
        passed, reason, extra = step.checker(out, rc)
    # the checker may need files the step wrote (e.g. a run directory's metrics.jsonl): clean up
    # only AFTER checking, and only on success (a failure's run dir stays for debugging).
    if step.cleanup and passed:
        try:
            step.cleanup()
        except OSError:
            pass
    return StepResult(step.name, step.argv, step.cwd or ctx.root, seconds, rc, log_path, sha256_file(log_path),
                       passed, reason, extra, mac=step.mac)


def run_steps(ctx, steps, dry_run):
    if dry_run:
        print_plan(ctx, steps)
        return [], True
    results = []
    ok = True
    i = 0
    while i < len(steps):
        group = steps[i].group
        batch = [steps[i]]
        j = i + 1
        if group is not None:
            while j < len(steps) and steps[j].group == group:
                batch.append(steps[j])
                j += 1
        if len(batch) == 1:
            r = run_step(ctx, batch[0])
            results.append(r)
            report_step(r)
            if not r.passed and not r.skipped:
                ok = False
                break
        else:
            out = [None] * len(batch)

            def worker(k, s):
                out[k] = run_step(ctx, s)
            threads = [threading.Thread(target=worker, args=(k, s)) for k, s in enumerate(batch)]
            for t in threads:
                t.start()
            for t in threads:
                t.join()
            for r in out:
                results.append(r)
                report_step(r)
            if any(not r.passed and not r.skipped for r in out):
                ok = False
                break
        i = j if len(batch) > 1 else i + 1
    return results, ok


def report_step(r):
    tag = 'SKIP' if r.skipped else ('PASS' if r.passed else 'FAIL')
    mac = ' [mac]' if r.mac else ''
    print(f'{tag:4s}{mac} {r.name} ({r.seconds:.1f}s): {r.reason}', flush=True)


def print_plan(ctx, steps):
    print(f'plan for tier {ctx.tier!r} (sha {ctx.sha}, target dir {ctx.target_dir}):')
    i = 0
    while i < len(steps):
        s = steps[i]
        group = s.group
        same = [s]
        j = i + 1
        while group is not None and j < len(steps) and steps[j].group == group:
            same.append(steps[j])
            j += 1
        prefix = '  (parallel) ' if len(same) > 1 else '  '
        for s2 in same:
            mac = ' [mac, via tools/mac.sh]' if s2.mac else ''
            if s2.skip_reason:
                print(f'{prefix}- {s2.name}{mac}: SKIPPED ({s2.skip_reason})')
            else:
                env = f', env {s2.env}' if s2.env else ''
                print(f'{prefix}- {s2.name}{mac}: {" ".join(s2.argv)}  (cwd={s2.cwd or ctx.root}{env})')
        i = j if len(same) > 1 else i + 1


# ---------------------------------------------------------------------------- target guard

def target_guard(target_dir, checkout_root):
    """Refuse a CARGO_TARGET_DIR already marked for a different checkout (one checkout's relative
    dep-info silently satisfying another's build is the bug reference_cargo_shared_target_dir.md
    describes); mark a fresh one for this checkout."""
    target_dir = os.path.abspath(target_dir)
    checkout_root = os.path.abspath(checkout_root)
    os.makedirs(target_dir, exist_ok=True)
    marker = os.path.join(target_dir, '.gate_checkout.json')
    norm = lambda p: os.path.normcase(os.path.normpath(p))
    if os.path.exists(marker):
        with open(marker, encoding='utf-8') as f:
            info = json.load(f)
        if norm(info.get('checkout', '')) != norm(checkout_root):
            raise GateError(
                f'CARGO_TARGET_DIR {target_dir} is marked for checkout {info.get("checkout")!r}, not '
                f'{checkout_root!r}: refusing (a shared target dir between checkouts can silently reuse the '
                f'other one\'s relative dep-info paths - reference_cargo_shared_target_dir.md). Use a separate '
                f'CARGO_TARGET_DIR for this checkout, or --target-dir to point at one already marked for it.')
    else:
        with open(marker, 'w', encoding='utf-8') as f:
            json.dump({'checkout': checkout_root, 'created': iso_now()}, f, indent=1)
    return target_dir


# ---------------------------------------------------------------------------- context

class Ctx:
    def __init__(self, args):
        self.tier = args.tier
        self.root = ROOT
        self.jobs = args.jobs
        self.step_timeout = args.step_timeout
        head = git(['rev-parse', 'HEAD'])
        if args.sha and args.sha != head and not head.startswith(args.sha) and not args.sha.startswith(head):
            raise GateError(f"--sha {args.sha} does not match this checkout's HEAD {head}: gate.py runs IN the "
                             f"checkout to gate (a pinned worktree the caller creates first), it does not create "
                             f"one. `git -C {ROOT} worktree add --detach <dir> {args.sha}` and run gate.py there.")
        self.sha = args.sha or head
        self.sha_short = self.sha[:12]
        if os.path.exists(os.path.join(CORE, '.git')):
            self.core_pin = git(['rev-parse', 'HEAD'], cwd=CORE, check=False) or None
        else:
            self.core_pin = None
        default_target = os.environ.get('CARGO_TARGET_DIR') or os.path.join(
            'E:/cargo-target/mtg-zero' if WIN else os.path.join(ROOT, 'target'),
            os.path.basename(ROOT) if os.path.basename(ROOT) != 'mtg-zero' else 'gate')
        self.target_dir = os.path.abspath(args.target_dir or default_target)
        self.logs_dir = os.path.join(GATES_DIR, 'logs')
        os.makedirs(self.logs_dir, exist_ok=True)
        self.base_env = {
            'CUDA_VISIBLE_DEVICES': '-1',              # an empty string does NOT hide the GPU on Windows
            'CARGO_TARGET_DIR': self.target_dir,
            'CARGO_BUILD_JOBS': str(self.jobs),
            'PYTHONIOENCODING': 'utf-8',
            'RUST_BACKTRACE': '1',
        }
        self.no_mac = args.no_mac


def python_exe():
    return sys.executable


def mac_step(name, remote_cmd, env=None):
    """A Mac part of a tier: `bash tools/mac.sh '<remote_cmd>'`, which pushes HEAD and runs
    `remote_cmd` on the checkout's exact commit there (ENGINE.md 44: bit-identical results). Marked
    `mac=True` so --no-mac / the report can tell it apart from a desktop step."""
    full = remote_cmd if not env else ' '.join(f'{k}={v}' for k, v in env.items()) + ' ' + remote_cmd
    return Step(name, ['bash', 'tools/mac.sh', full], checker=lambda out, rc: (
        rc == 0, (out.strip().splitlines() or ['(no output)'])[-1][:300], {}), mac=True)


# ---------------------------------------------------------------------------- tiers

def tier_docs(ctx, args):
    files = args.files or sorted(glob.glob(os.path.join(ROOT, 'tools', '*.py')))
    rel = [os.path.relpath(f, ROOT) for f in files]
    return [Step('py_compile touched tools', [python_exe(), '-m', 'py_compile', *rel], checker=check_pycompile)]


def tier_python(ctx, args):
    run_dir = lazy_run_dir(ctx, 'smoke')
    preset = args.smoke_preset or 'mtg-smoke'
    games = {'mtg-smoke': 16, 'delver-smoke': 16}.get(preset)
    steps = [
        Step('pytest', [python_exe(), '-m', 'pytest', 'tests', '-q', '-p', 'no:cacheprovider'], checker=check_pytest),
        Step(f'run-loop smoke ({preset})',
             [python_exe(), '-m', 'mtgz.az.run', '--run', run_dir, '--preset', preset, '--iters', '1'],
             checker=make_metrics_checker(run_dir, want_games=games, want_iters=1),
             cleanup=lambda: shutil.rmtree(run_dir, ignore_errors=True)),
    ]
    return steps


def tier_encoding(ctx, args):
    steps = [
        Step('build', ['bash', 'tools/build_py.sh'], checker=lambda out, rc: (rc == 0, f'exit {rc}', {})),
        Step('digest encoding-free (DIGEST_ENCODING=0)',
             ['cargo', 'test', '--release', '-p', 'engine', '--test', 'digest', '--', '--ignored', '--nocapture'],
             env={'DIGEST_ENCODING': '0'}, checker=make_digest_checker(args.expect_digest0)),
        Step('digest full',
             ['cargo', 'test', '--release', '-p', 'engine', '--test', 'digest', '--', '--ignored', '--nocapture'],
             checker=make_digest_checker(args.expect_digest)),
        Step('cargo test az', ['cargo', 'test', '--release', '-p', 'engine', '--test', 'az']),
    ]
    if args.checkpoint:
        steps.append(Step(
            'checkpoint-load (mtgz.az.h2h)',
            [python_exe(), '-m', 'mtgz.az.h2h', args.checkpoint, args.checkpoint, '--games', '8', '--sims', '16',
             '--spec', args.checkpoint_spec or '{"max_turns": 60}', '--device', 'cpu', '--threads', '4',
             '--select', 'visits'], checker=check_h2h_unfinished))
    else:
        steps.append(Step('checkpoint-load (mtgz.az.h2h)', [], skip_reason='no --checkpoint given'))
    steps.append(Step('pytest', [python_exe(), '-m', 'pytest', 'tests', '-q', '-p', 'no:cacheprovider'],
                       checker=check_pytest))
    # NO search smoke (the uniform evaluator cannot see an encoding change).
    return steps


def tier_engine(ctx, args):
    steps = [
        Step('build', ['bash', 'tools/build_py.sh'], checker=lambda out, rc: (rc == 0, f'exit {rc}', {})),
        Step('digest encoding-free (DIGEST_ENCODING=0)',
             ['cargo', 'test', '--release', '-p', 'engine', '--test', 'digest', '--', '--ignored', '--nocapture'],
             env={'DIGEST_ENCODING': '0'}, checker=make_digest_checker(args.expect_digest0)),
        Step('digest full',
             ['cargo', 'test', '--release', '-p', 'engine', '--test', 'digest', '--', '--ignored', '--nocapture'],
             checker=make_digest_checker(args.expect_digest)),
        Step('cargo test az', ['cargo', 'test', '--release', '-p', 'engine', '--test', 'az']),
        Step('cargo test limits', ['cargo', 'test', '--release', '-p', 'engine', '--test', 'limits']),
        Step('cargo test hidden', ['cargo', 'test', '--release', '-p', 'engine', '--test', 'hidden']),
        Step('pytest', [python_exe(), '-m', 'pytest', 'tests', '-q', '-p', 'no:cacheprovider'], checker=check_pytest),
        Step('smoke: 1 desktop config (random, seed 0)',
             [python_exe(), 'tools/search_smoke.py', '--specs', 'random', '--seeds', '0'],
             checker=check_search_smoke),
    ]
    recheck_reason = None
    if args.tested_sha and args.tested_sha != ctx.sha:
        touched = git(['diff', '--name-only', args.tested_sha, ctx.sha]).splitlines()
        overlap = [f for f in touched if f.startswith(('engine/', 'card_ir/'))]
        if overlap:
            step = mac_step('full suite again on the Mac (merged sha differs, overlapping files)',
                             'cargo test --release --no-fail-fast')
            steps.append(step)
        else:
            recheck_reason = f'--tested-sha {args.tested_sha[:12]} != {ctx.sha_short} but no engine/card_ir overlap'
    else:
        recheck_reason = 'sha == --tested-sha (or --tested-sha not given): no Mac recheck needed'
    if recheck_reason:
        steps.append(Step('full suite again on the Mac (merged sha differs, overlapping files)', [],
                           skip_reason=recheck_reason))
    return steps


def tier_search_core(ctx, args):
    if not os.path.isdir(CORE):
        raise GateError(f'{CORE} is not initialised (bash tools/build_py.sh first)')
    configs = (args.configs or 'random,eval,match,nomull').split(',')
    steps = [
        Step('zero-core cargo test', ['cargo', 'test', '--release'], cwd=CORE),
        Step('zero-core pytest', [python_exe(), '-m', 'pytest', 'tests', '-q', '-p', 'no:cacheprovider'], cwd=CORE,
             checker=check_pytest),
        Step('this repo: pytest', [python_exe(), '-m', 'pytest', 'tests', '-q', '-p', 'no:cacheprovider'],
             checker=check_pytest),
    ]
    if args.baseline_root:
        steps.append(Step(
            'check_checkpoint_compat goldens',
            [python_exe(), os.path.join(CORE, 'tools', 'check_checkpoint_compat.py'), '--old', args.baseline_root,
             '--new', ROOT, '--package', 'mtgz', '--net', 'random', '--game', 'mtg', '--spec',
             args.checkpoint_spec or '{"max_turns": 60}'], checker=check_checkpoint_compat))
    else:
        steps.append(Step('check_checkpoint_compat goldens', [], skip_reason='no --baseline-root given'))
    steps.append(Step(f'smoke: {len(configs)}-config of the pinning game ({",".join(configs)})',
                       [python_exe(), 'tools/search_smoke.py', '--specs', ','.join(configs), '--seeds', '0'],
                       checker=check_search_smoke))
    run_dir = lazy_run_dir(ctx, 'smoke')
    steps.append(Step('run-loop smoke (mtg-smoke)',
                       [python_exe(), '-m', 'mtgz.az.run', '--run', run_dir, '--preset', 'mtg-smoke', '--iters', '1'],
                       checker=make_metrics_checker(run_dir, want_games=16, want_iters=1),
                       cleanup=lambda: shutil.rmtree(run_dir, ignore_errors=True)))
    # smoke_all for the OTHER zero-core games is zero-core-side (its own tools/smoke_all.sh
    # --build-only), run by the orchestrator against the moving core commit - not this repo's gate.
    return steps


def tier_optin_core(ctx, args):
    if not os.path.isdir(CORE):
        raise GateError(f'{CORE} is not initialised (bash tools/build_py.sh first)')
    feature_tests = (args.feature_tests or 'az,hidden').split(',')
    steps = [Step('core tests', ['cargo', 'test', '--release'], cwd=CORE)]
    if args.baseline_root:
        steps.append(Step(
            'checkpoint_compat byte identity',
            [python_exe(), os.path.join(CORE, 'tools', 'check_checkpoint_compat.py'), '--old', args.baseline_root,
             '--new', ROOT, '--package', 'mtgz', '--net', 'random', '--game', 'mtg', '--spec',
             args.checkpoint_spec or '{"max_turns": 60}'], checker=check_checkpoint_compat))
    else:
        steps.append(Step('checkpoint_compat byte identity', [], skip_reason='no --baseline-root given'))
    for t in feature_tests:
        steps.append(Step(f'feature tests: cargo test --test {t}', ['cargo', 'test', '--release', '-p', 'engine',
                                                                     '--test', t]))
    steps.append(build_only_step(ctx, args))
    return steps


OTHER_GAMES = ('glassroad-zero', 'caverna-zero', 'kingdombuilder-zero')


def repo_name(path):
    """smoke_all.sh's name for a game checkout: its main repo's directory (a worktree's too)."""
    common = git(['rev-parse', '--path-format=absolute', '--git-common-dir'], cwd=path, check=False)
    return os.path.basename(os.path.dirname(os.path.normpath(common))) if common else \
        os.path.basename(os.path.normpath(path))


def default_other_games():
    """The other zero-core games' repos, beside this repo's MAIN checkout (from the git common dir, so
    it works from any worktree - the checkout under gate always is one), else beside this checkout.
    Every one of OTHER_GAMES must be found: a game that is not there is a setup error, never a skip."""
    roots = []
    common = os.path.normpath(git(['rev-parse', '--path-format=absolute', '--git-common-dir'], check=False) or '.')
    if os.path.basename(common) == '.git':
        roots.append(os.path.dirname(os.path.dirname(common)))
    if os.path.dirname(ROOT) not in roots:
        roots.append(os.path.dirname(ROOT))
    found, missing = [], []
    for g in OTHER_GAMES:
        hit = next((os.path.join(r, g) for r in roots if os.path.exists(os.path.join(r, g, 'third_party', 'zero-core'))),
                   None)
        if hit:
            found.append(hit)
        else:
            missing.append(g)
    if missing:
        raise GateError(f'the build-only step builds {", ".join(OTHER_GAMES)}; not found beside '
                         f'{" or ".join(roots)}: {", ".join(missing)} (clone them there, or pass --other-games)')
    return found


def build_only_step(ctx, args):
    """zero-core's smoke_all.sh --with-this-core --build-only on the other games, named EXPLICITLY:
    its own discovery found none from a pinning checkout's third_party/zero-core before zero-core
    fix/smoke-all-games and printed GREEN anyway (ENGINE.md 90). SKIP_CORE=1: the core's cargo tests
    are this tier's 'core tests' step, and its ~15 min pytest is not part of this tier's recipe (it
    rode along inside this step and failed it on load flakes). Own SMOKE_TARGET / SMOKE_DIR per
    checkout, and the gate's CARGO_TARGET_DIR unset so no game ever builds into this repo's target."""
    if args.other_games:
        # (each --other-games value is one checkout path, never split: 'C:/Users/Gregg Keithley/...' has a space)
        games = [os.path.abspath(p) for p in args.other_games if p]
        bad = [p for p in games if not os.path.isdir(p)]
        if bad:
            raise GateError(f'--other-games: no such checkout: {", ".join(bad)}')
    else:
        games = default_other_games()
    names = [repo_name(p) for p in games]
    smoke_target = 'mtgz-gate-' + slug(os.path.basename(ROOT))
    smoke_dir = (f'E:/zero-smoke/{smoke_target}' if WIN else os.path.join(GATES_DIR, 'tmp', smoke_target))
    return Step(f'build-only: the other games ({", ".join(names)})',
                ['bash', os.path.join(CORE, 'tools', 'smoke_all.sh'), '--with-this-core', '--build-only', *games],
                cwd=CORE, env={'SKIP_CORE': '1', 'SMOKE_TARGET': smoke_target, 'SMOKE_DIR': smoke_dir,
                               'CARGO_TARGET_DIR': None},
                checker=make_build_only_checker(names, ctx.core_pin))


def tier_repin(ctx, args):
    if not args.expect_digest:
        raise GateError('the repin tier requires --expect-digest (main\'s recorded value): the whole point of '
                         'this tier is that a re-pin changes nothing behaviourally, and that needs a known-good '
                         'value to compare against, not just "it ran".')
    spec = args.spec or 'delver'
    preset = args.preset or 'delver-mirror'
    steps = [
        Step('build (this checkout\'s own target)', ['bash', 'tools/build_py.sh'],
             checker=lambda out, rc: (rc == 0, f'exit {rc}', {})),
        Step('digest equals main\'s recorded value',
             ['cargo', 'test', '--release', '-p', 'engine', '--test', 'digest', '--', '--ignored', '--nocapture'],
             checker=make_digest_checker(args.expect_digest)),
        Step(f'1-config smoke on the run\'s spec ({spec}, preset {preset})',
             [python_exe(), 'tools/search_smoke.py', '--specs', spec, '--seeds', '0', '--preset', preset],
             checker=check_search_smoke, group='parallel-mac'),
        Step('mac.sh --eval-checkout', ['bash', 'tools/mac.sh', '--eval-checkout'], mac=True, group='parallel-mac',
             checker=lambda out, rc: (rc == 0 and 'eval checkout ready' in out,
                                      (out.strip().splitlines() or ['?'])[-1][:300], {})),
    ]
    # watch_run.sh is deliberately NOT run here: it watches a LIVE run directory after the re-pin
    # takes effect (no natural pass/fail endpoint for a one-shot gate) - an operator step after
    # this gate passes, documented in --help and ENGINE.md 70, not a scripted check.
    return steps


TIERS = {
    'docs': (tier_docs, 'docs / tools not on the run path: py_compile of touched tools (~0 min).'),
    'python': (tier_python, 'Python run-loop/settings: pytest + a tiny run-loop smoke (~3 min).'),
    'encoding': (tier_encoding, 'encoding-only (layout unchanged): build + DIGEST_ENCODING=0 digest unchanged + '
                 'full digest recorded + --test az + checkpoint-load + pytest. NO search smoke (~7 min).'),
    'engine': (tier_engine, 'engine rules/cards (orchestrator half): fresh build, digests == the agent\'s report, '
               'az/limits/hidden, pytest, 1 desktop smoke config; a Mac full-suite recheck only if the merged sha '
               'differs from the tested sha with overlapping engine/card_ir files (~10 min).'),
    'search-core': (tier_search_core, 'search/core behaviour: core cargo + pytest + check_checkpoint_compat goldens '
                    '+ 4-config smoke of the pinning game + run-loop smoke (~20 min).'),
    'optin-core': (tier_optin_core, 'opt-in core feature with byte-identical defaults: core tests + '
                   'checkpoint_compat byte identity + feature tests + build-only of the other games (~10 min).'),
    'repin': (tier_repin, 're-pin onto an already-verified commit: own-target build + digest == main\'s recorded '
              'value + 1-config smoke on the run\'s spec + mac.sh --eval-checkout, in parallel (~5 min).'),
}


# ---------------------------------------------------------------------------- report

def write_report(ctx, args, steps_planned, results, ok, started, seconds):
    report = {
        'sha': ctx.sha, 'sha_short': ctx.sha_short, 'zero_core_pin': ctx.core_pin, 'tier': ctx.tier,
        'started': started, 'seconds': round(seconds, 1), 'cargo_target_dir': ctx.target_dir,
        'passed': ok, 'digests': {}, 'test_counts': {},
        'steps': [],
    }
    for r in results:
        step = {'name': r.name, 'cmd': r.argv, 'cwd': r.cwd, 'seconds': round(r.seconds, 2), 'rc': r.rc,
                'log': r.log, 'log_sha256': r.log_sha256, 'passed': r.passed, 'reason': r.reason,
                'skipped': r.skipped, 'mac': r.mac}
        report['steps'].append(step)
        if r.extra.get('digest'):
            key = 'encoding_free' if 'encoding-free' in r.name or 'DIGEST_ENCODING' in r.name else 'full'
            report['digests'][key] = r.extra['digest']
        if 'passed' in r.extra and 'failed' in r.extra:
            report['test_counts'][r.name] = {'passed': r.extra['passed'], 'failed': r.extra['failed'],
                                              'binaries': r.extra.get('binaries')}
    os.makedirs(GATES_DIR, exist_ok=True)
    path = os.path.join(GATES_DIR, f'{ctx.sha_short}.json')
    with open(path, 'w', encoding='utf-8') as f:
        json.dump(report, f, indent=1)
    return path


# ---------------------------------------------------------------------------- main

def build_parser():
    ap = argparse.ArgumentParser(
        prog='gate.py', formatter_class=argparse.RawDescriptionHelpFormatter,
        description=__doc__.split('\n\n')[0],
        epilog='Tiers:\n' + '\n'.join(f'  {name:12s} {desc}' for name, (_, desc) in TIERS.items()))
    ap.add_argument('tier', choices=sorted(TIERS))
    ap.add_argument('--sha', default=None, help="the commit being gated (default: this checkout's HEAD); must "
                     "match the checkout (gate.py never creates a worktree)")
    ap.add_argument('--tested-sha', default=None, help='engine tier: the sha the agent actually fuzzed/tested, if '
                     'different from --sha (a merge); triggers the Mac recheck when it touched engine/ or card_ir/')
    ap.add_argument('--expect-digest', default=None, help='the full combined digest a digest step must equal '
                     '(omit to just record it)')
    ap.add_argument('--expect-digest0', default=None, help='the DIGEST_ENCODING=0 combined digest a digest step '
                     'must equal (omit to just record it)')
    ap.add_argument('--checkpoint', default=None, help='encoding tier: a .pt checkpoint for the h2h load/play check')
    ap.add_argument('--checkpoint-spec', default=None, help='the mtg spec JSON for --checkpoint / '
                     'check_checkpoint_compat (default the smoke spec, max_turns 60)')
    ap.add_argument('--baseline-root', default=None, help='search-core/optin-core: another checkout to compare '
                     'against with check_checkpoint_compat (omit to skip that step)')
    ap.add_argument('--spec', default=None, help='repin tier: the search_smoke spec of the run being re-pinned '
                     '(default delver)')
    ap.add_argument('--preset', default=None, help='repin tier: the run preset (default delver-mirror)')
    ap.add_argument('--configs', default=None, help='search-core tier: comma-separated search_smoke specs for the '
                     '4-config smoke (default random,eval,match,nomull)')
    ap.add_argument('--feature-tests', default=None, help='optin-core tier: comma-separated cargo test --test '
                     'targets for the new feature (default az,hidden)')
    ap.add_argument('--other-games', nargs='+', action='extend', default=None, help='optin-core tier: the game '
                     'checkouts for the build-only step, one path per argument (quote a path with a space; the '
                     'option may repeat; default: glassroad-zero, caverna-zero and kingdombuilder-zero beside this '
                     'repo\'s main checkout, all three required); the step passes each "ok <game>: build" line only')
    ap.add_argument('--files', nargs='*', default=None, help='docs tier: the touched tool files to py_compile '
                     '(default: every tools/*.py)')
    ap.add_argument('--smoke-preset', default=None, help='python tier: the run-loop smoke preset (default '
                     'mtg-smoke)')
    ap.add_argument('--jobs', type=int, default=4, help='CARGO_BUILD_JOBS (default 4)')
    ap.add_argument('--target-dir', default=None, help='override CARGO_TARGET_DIR (default: the env var if set, '
                     'else E:/cargo-target/mtg-zero/<this checkout\'s dir name>)')
    ap.add_argument('--step-timeout', type=float, default=3600.0, help='seconds before a single step is killed as '
                     'hung (default 3600)')
    ap.add_argument('--no-mac', action='store_true', help='skip Mac steps even if the tier wants one (marked '
                     'skipped, not failed, in the report)')
    ap.add_argument('--dry-run', action='store_true', help='print the plan; run nothing, write no report')
    return ap


def main(argv=None):
    args = build_parser().parse_args(argv)
    try:
        ctx = Ctx(args)
    except GateError as e:
        print(f'gate.py: {e}', file=sys.stderr)
        return 2
    fn, _ = TIERS[args.tier]
    try:
        steps = fn(ctx, args)
    except GateError as e:
        print(f'gate.py: {e}', file=sys.stderr)
        return 2
    if args.no_mac:
        for s in steps:
            if s.mac and not s.skip_reason:
                s.skip_reason = '--no-mac'
    if args.dry_run:
        print_plan(ctx, steps)
        return 0
    needs_cargo = any(s.argv and s.argv[0] == 'cargo' for s in steps) or any(
        s.argv and 'build_py.sh' in ' '.join(s.argv) for s in steps)
    if needs_cargo:
        try:
            target_guard(ctx.target_dir, ctx.root)
        except GateError as e:
            print(f'gate.py: {e}', file=sys.stderr)
            return 2
    os.makedirs(os.path.join(GATES_DIR, 'tmp'), exist_ok=True)
    print(f'gate.py {args.tier}: sha {ctx.sha_short}, core pin {(ctx.core_pin or "none")[:12]}, target '
          f'{ctx.target_dir}', flush=True)
    started = iso_now()
    t0 = time.time()
    results, ok = run_steps(ctx, steps, dry_run=False)
    seconds = time.time() - t0
    path = write_report(ctx, args, steps, results, ok, started, seconds)
    print(('GATE PASSED' if ok else 'GATE FAILED') + f' {args.tier} {ctx.sha_short} in {seconds:.0f}s; report {path}',
          flush=True)
    return 0 if ok else 1


if __name__ == '__main__':
    sys.exit(main())
