"""The tiered verification gate (feedback_tiered_gates, user-approved 2026-10-29; docs/ENGINE.md
70, 90 and 91): one command per tier, each running exactly that tier's recipe instead of an agent's
ad-hoc shell script. It can never pass a check it did not perform:

  * **No shared log file.** Each step gets its own log, opened and written by this process alone
    (never `tee`d into by a child too): the old scripts' `cmd | tee /dev/stderr | grep ...` could
    truncate the log with NUL bytes when the child and `tee` raced on the same file.
  * **Positive evidence, not absence of FAILED.** `grep -q FAILED && fail` passes silently when a
    build fails before any test runs (no "test result:" line at all, so no FAILED line either).
    Every step needs its own positive marker ON TOP OF exit 0 (ENGINE.md 91 lists each one): a
    build's "mtgz._engine OK: <this checkout>/mtgz/..." line, cargo's "test result: ok. N passed"
    lines (N > 0, 0 failed), pytest's FINAL summary line (0 failed, 0 errors, N > 0 passed), the
    digest line over the full 2,000 + 500 games, check_checkpoint_compat's {"identical": true}
    with samples on both sides, a Mac step's "[mac] ... @ <this sha>" line, "0 failed of N
    configurations" with N the configurations asked for, ... A missing marker fails loudly.
  * **No silent skips.** A tier's essential inputs are required (encoding: --expect-digest,
    --expect-digest0, --checkpoint; engine: --expect-digest, --expect-digest0, --tested-sha;
    search-core and optin-core: --baseline-root; repin: --expect-digest, --spec, --preset): without
    one, gate.py refuses before running anything (exit 2) unless the caller opts out BY NAME -
    `--allow-skip <step>`, `--record-digest <which>` (record without comparing), `--no-mac`,
    `--allow-dirty`. An opted-out step is SKIPPED / RECORDED (never PASSED) in the report, and the
    final line names it: "GATE PASSED WITH SKIPS <tier> <sha> ... - skipped: <step> (<why>)". An
    option the tier does not use is refused too (it would be ignored, and the check it names
    would not run).
  * **The commit, unchanged.** The checkout's tracked files must match HEAD (the zero-core
    submodule at the pin) - else the gate would check uncommitted code under the commit's name -
    and must still match it afterwards (a step that rewrites a tracked file, e.g. re-recorded
    digests, fails the gate). A caller's DIGEST_*, PYTEST_ADDOPTS, PYTHONPATH and
    ZERO_CORE_UNPINNED never reach a step.
  * **The core's own suite** (search-core, optin-core): zero-core's engine module built with its
    tools/build_py.sh, then its pytest as its own step ("core pytest"). If the ONLY failures are in
    tests/test_remoteplay.py (it times real threads: zero-core AGENTS.md), each failed test is
    rerun alone once; the step passes only if every one passes alone, recorded as "flaky, passed
    on retry" in the report and the final line. Any other failure fails the step.

Run from WITHIN the checkout to gate (a pinned worktree the caller creates first, e.g. `git
worktree add --detach ../mtg-zero-wt/run-<sha> <sha>`, exactly as the old scripts did) - this tool
never creates or manages a worktree itself, the same contract as tools/build_py.sh and
tools/search_smoke.py. `--sha` (optional) must match the checkout's HEAD or the gate refuses
(never silently gates the wrong commit).

    python tools/gate.py docs
    python tools/gate.py python
    python tools/gate.py encoding --expect-digest <full> --expect-digest0 <encoding-free> --checkpoint <a .pt>
    python tools/gate.py engine --expect-digest <the agent's full> --expect-digest0 <...> --tested-sha <the agent's sha>
    python tools/gate.py search-core --baseline-root ../other-checkout
    python tools/gate.py optin-core --baseline-root ../other-checkout
    python tools/gate.py repin --expect-digest <main's recorded digest> --spec delver --preset delver-mirror
    python tools/gate.py <tier> --dry-run      # print the plan (or the refusal); run nothing

Exit status: 0 passed ("GATE PASSED ...", or "GATE PASSED WITH SKIPS / WITH FLAKY RETRIES ..."
naming every opted-out step and every flaky retry), 1 failed ("GATE FAILED ..."), 2 refused (a
setup problem or a missing essential input; nothing ran).

Writes `gates/<sha>.json` (sha, zero-core pin, tier, options, the final line, each step's status -
passed / failed / skipped / recorded / not needed - with its reason, command, log path + sha256 and
seconds, digests, test counts, skips, flaky retries, the baseline checkout, the cargo target dir)
and one log per step under `gates/logs/`. CPU only (CUDA_VISIBLE_DEVICES=-1, never ''),
CARGO_BUILD_JOBS from the environment (else 4; --jobs), below normal process priority (idle when
gate.py itself runs at idle); a Mac part of a tier (tools/mac.sh) runs in a thread alongside its
desktop counterpart where the recipe says so.

A checkout marker in the cargo target dir (`.gate_checkout.json`) records which checkout last
built there; a second checkout pointed at the same CARGO_TARGET_DIR is refused instead of silently
reusing the first one's relative dep-info paths (reference_cargo_shared_target_dir.md: this is
exactly how a scratch worktree's build quietly became the real worktree's build before).
"""
import argparse
import dataclasses
import glob
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import threading
import time
import uuid
from datetime import datetime, timezone

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CORE = os.path.join(ROOT, 'third_party', 'zero-core')
GATES_DIR = os.path.join(ROOT, 'gates')
WIN = sys.platform == 'win32'
FLAKY_FILE = 'tests/test_remoteplay.py'   # zero-core AGENTS.md: "times real threads and can fail on a loaded machine"


class GateError(Exception):
    """A configuration or setup problem, or a missing essential input (not a step failure): exit 2
    before anything runs."""


def iso_now():
    return datetime.now(timezone.utc).isoformat()


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()


def git(args, cwd=None, check=True):
    p = subprocess.run(['git', *args], cwd=cwd or ROOT, capture_output=True, text=True)
    if check and p.returncode:
        raise GateError(f'git {" ".join(args)} ({cwd or ROOT}): {p.stderr.strip()}')
    return p.stdout.strip()


def slug(name):
    return re.sub(r'[^a-z0-9]+', '-', name.lower()).strip('-')


def norm(path):
    return os.path.normcase(os.path.normpath(os.path.abspath(path)))


def show(argv):
    return ' '.join(f'"{a}"' if (not a or ' ' in a) else a for a in argv)


def last_line(output):
    return (output.strip().splitlines() or ['(no output)'])[-1][:300]


def lazy_run_dir(ctx, tag):
    """A unique path under gates/tmp for a throwaway run-loop smoke, WITHOUT creating it (dry-run
    must not touch disk; `zero.run` makes the directory itself on first write)."""
    return os.path.join(GATES_DIR, 'tmp', f'gate-{ctx.tier}-{tag}-{ctx.sha_short}-{uuid.uuid4().hex[:8]}')


# ---------------------------------------------------------------------------- checkers (positive evidence)
# A checker: (output, rc) -> (ok, reason, extra). ok needs the step's positive marker AND exit 0; a
# missing marker is a failure with its own reason, never a pass.

TEST_RESULT_RE = re.compile(r'^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; \d+ ignored; \d+ measured; '
                            r'\d+ filtered out', re.M)


def make_cargo_checker(single=False):
    """cargo test's positive evidence: "test result:" lines (a compiler error prints none - that is a
    FAILED, never a silent pass), every one "ok" with 0 failed, more than 0 tests passed in all, and
    exit 0. `single` (a `--test <name>` step: one test binary) wants exactly one such line - a filter
    or a target that ran nothing is not a pass either way."""
    def checker(output, rc):
        matches = TEST_RESULT_RE.findall(output)
        if not matches:
            return False, 'no "test result:" line in the output (did it even compile?)', {}
        failed = sum(int(m[2]) for m in matches)
        passed = sum(int(m[1]) for m in matches)
        extra = {'passed': passed, 'failed': failed, 'binaries': len(matches)}
        if failed or not all(m[0] == 'ok' for m in matches):
            return False, f'{passed} passed, {failed} failed across {len(matches)} binary/binaries', extra
        if single and len(matches) != 1:
            return False, f'{len(matches)} "test result:" lines for one test target (expected exactly 1)', extra
        if passed == 0:
            return False, f'0 tests passed ({len(matches)} binary/binaries ran nothing)', extra
        if rc != 0:
            return False, f'exit {rc} although {passed} passed (a later target failed to build or run?)', extra
        return True, f'{passed} passed ({len(matches)} binary/binaries)', extra
    return checker


check_cargo_test = make_cargo_checker()

DIGEST_RE = re.compile(r'^(\d+) random \+ (\d+) heuristic games in [\d.]+ s; combined digest ([0-9a-f]{16})\s*$', re.M)
DIGEST_GAMES = (2000, 500)      # engine/tests/digest.rs's defaults (DIGEST_RANDOM / DIGEST_HEURISTIC unset)
DIGEST_ARGV = ['cargo', 'test', '--release', '-p', 'engine', '--test', 'digest', '--', '--ignored', '--nocapture']


def make_digest_checker(expect=None, record_only=False):
    """The digest test's positive evidence: exactly ONE "N random + M heuristic games in ... s;
    combined digest <16 hex>" line over the full 2,000 + 500 games, no "wrote ...digests.txt"
    (DIGEST_WRITE re-records instead of comparing), its "test result: ok. 1 passed" line, exit 0 -
    and the digest equal to `expect`. Without `expect` only an explicit `record_only`
    (--record-digest) accepts it, and run_step reports that as RECORDED, never PASSED."""
    if not expect and not record_only:
        raise ValueError('a digest step compares with an expected value or is explicitly record-only')

    def checker(output, rc):
        ms = DIGEST_RE.findall(output)
        if not ms:
            return False, 'no "... heuristic games in ... s; combined digest <16 hex>" line (did it compile/run?)', {}
        if len(ms) != 1:
            return False, f'{len(ms)} combined digest lines (expected exactly 1)', {}
        n_r, n_h, d = int(ms[0][0]), int(ms[0][1]), ms[0][2]
        extra = {'digest': d}
        if (n_r, n_h) != DIGEST_GAMES:
            return False, (f'digest {d} over {n_r} random + {n_h} heuristic games, not the full '
                           f'{DIGEST_GAMES[0]} + {DIGEST_GAMES[1]}'), extra
        if re.search(r'^wrote .*digests\.txt', output, re.M):
            return False, f'digest {d}: the test RE-RECORDED tests/data/digests.txt instead of comparing', extra
        ok, why, _ = make_cargo_checker(single=True)(output, rc)
        if not ok:
            return False, f'digest {d}, but the digest test did not pass: {why}', extra
        if expect and d != expect:
            return False, f'digest {d} != expected {expect}', extra
        return True, (f'digest {d} (= expected)' if expect else f'digest {d} recorded, NOT compared'), extra
    return checker


PYTEST_WORD = r'(?:failed|passed|skipped|deselected|xfailed|xpassed|warnings?|errors?|rerun)'
PYTEST_TIME = r'in \d+(?:\.\d+)?s(?: \(\d+:\d\d:\d\d\))?'
PYTEST_COUNTS_RE = re.compile(rf'(\d+ {PYTEST_WORD}(?:, \d+ {PYTEST_WORD})*) {PYTEST_TIME}')
PYTEST_NONE_RE = re.compile(rf'no tests ran {PYTEST_TIME}')


def parse_pytest_summary(output):
    """pytest's FINAL summary line -> its counts: "1 failed, 301 passed, 3 skipped, 1 warning in
    881.28s (0:14:41)" -> {'failed': 1, 'passed': 301, 'skipped': 3, 'warning': 1} (with or without
    the "=" fill -q leaves out; "errors"/"warnings" counted as 'error'/'warning'); {} for "no tests
    ran in ..."; None when there is no summary line at all (a crash, a timeout, a dead
    interpreter). The LAST such line wins - never the first "N passed" anywhere in the output."""
    last = None
    for line in output.splitlines():
        s = line.strip().strip('=').strip()
        if PYTEST_NONE_RE.fullmatch(s):
            last = {}
            continue
        m = PYTEST_COUNTS_RE.fullmatch(s)
        if m:
            counts = {}
            for part in m.group(1).split(', '):
                n, word = part.split(' ')
                word = {'errors': 'error', 'warnings': 'warning'}.get(word, word)
                counts[word] = counts.get(word, 0) + int(n)
            last = counts
    return last


def check_pytest(output, rc):
    """pytest's positive evidence: its final summary line (parse_pytest_summary) with 0 failed,
    0 errors and more than 0 passed, and exit 0."""
    c = parse_pytest_summary(output)
    if c is None:
        return False, 'no pytest summary line ("N passed in ...s") in the output (crash, collection error, timeout?)', {}
    if not c:
        return False, 'no tests ran', {}
    passed, failed, errors, skipped = (c.get(k, 0) for k in ('passed', 'failed', 'error', 'skipped'))
    extra = {'passed': passed, 'failed': failed, 'errors': errors, 'skipped': skipped}
    if failed or errors:
        return False, f'{failed} failed, {errors} error(s), {passed} passed', extra
    if passed == 0:
        return False, f'0 passed ({", ".join(f"{v} {k}" for k, v in c.items())})', extra
    if rc != 0:
        return False, f'exit {rc} although the summary says {passed} passed', extra
    return True, f'{passed} passed' + (f', {skipped} skipped' if skipped else ''), extra


SHORT_SUMMARY_RE = re.compile(r'^=+ short test summary info =+\s*$', re.M)
FAIL_LINE_RE = re.compile(r'^(FAILED|ERROR) (\S+)(?: - .*)?\s*$')


def flaky_retry_candidates(output, flaky_file=FLAKY_FILE):
    """-> (the node ids to rerun alone, None) when EVERY failure of a pytest run is a test in
    `flaky_file`, else (None, why not). The failures are read from the last "short test summary
    info" block (its FAILED / ERROR lines), and there must be exactly as many as the summary line's
    failed + errors - a count that does not add up, or a collection error (a path, not a test), is
    never retried."""
    c = parse_pytest_summary(output)
    if not c:
        return None, 'no pytest summary line'
    bad = c.get('failed', 0) + c.get('error', 0)
    if not bad:
        return None, 'no failures'
    heads = list(SHORT_SUMMARY_RE.finditer(output))
    if not heads:
        return None, 'no "short test summary info" block to read the failures from'
    lines = [m.groups() for m in map(FAIL_LINE_RE.match, output[heads[-1].end():].splitlines()) if m]
    if len(lines) != bad:
        return None, f'the summary counts {bad} failure(s)/error(s) but the short summary lists {len(lines)}'
    want = flaky_file.replace('\\', '/')
    ids = []
    for kind, nid in lines:
        path, sep, _ = nid.partition('::')
        path = path.replace('\\', '/')
        if not sep:
            return None, f'{kind} {nid}: a collection error, not a test'
        if path != want and not path.endswith('/' + want):
            return None, f'{kind} {nid} is not in {flaky_file}'
        if nid not in ids:
            ids.append(nid)
    return ids, None


PYCOMPILE_CODE = ("import py_compile, sys; fs = sys.argv[1:]; [py_compile.compile(f, doraise=True) for f in fs]; "
                  "print('py_compile: %d file(s) compiled' % len(fs))")


def make_pycompile_checker(n):
    """The docs tier: "py_compile: N file(s) compiled" (printed only after every file compiled) for
    exactly the N files asked for, and exit 0."""
    def checker(output, rc):
        m = re.search(r'^py_compile: (\d+) file\(s\) compiled\s*$', output, re.M)
        if not m:
            return False, 'no "py_compile: N file(s) compiled" line: ' + last_line(output), {}
        if int(m.group(1)) != n:
            return False, f'{m.group(1)} file(s) compiled, expected {n}', {}
        if rc != 0:
            return False, f'exit {rc}', {}
        return True, f'{n} file(s) compiled cleanly', {}
    return checker


SMOKE_RE = re.compile(r'search smoke: (\d+) failed of (\d+) configurations')


def make_search_smoke_checker(configs):
    """search_smoke's summary line with 0 failed of exactly the `configs` configurations asked for
    ("0 failed of 0" is not a pass), and exit 0."""
    def checker(output, rc):
        ms = SMOKE_RE.findall(output)
        if not ms:
            return False, 'no "search smoke: ... failed of ... configurations" summary line', {}
        bad, total = (int(x) for x in ms[-1])
        extra = {'configurations': total, 'failed': bad}
        if total != configs:
            return False, f'{total} configuration(s) ran, expected {configs}', extra
        if bad:
            return False, f'{bad}/{total} configurations failed', extra
        if rc != 0:
            return False, f'exit {rc}', extra
        return True, f'0/{total} configurations failed', extra
    return checker


def json_lines(output, *keys):
    """Every line of `output` that is a JSON object with all of `keys` (other lines skipped)."""
    out = []
    for line in output.splitlines():
        if line.startswith('{'):
            try:
                obj = json.loads(line)
            except ValueError:
                continue
            if isinstance(obj, dict) and all(k in obj for k in keys):
                out.append(obj)
    return out


def make_h2h_checker(games):
    """encoding's checkpoint-load: h2h's final result line (a JSON object with "games" and
    "unfinished" - not a "# batch" line) with exactly `games` games, 0 unfinished, exit 0."""
    def checker(output, rc):
        res = json_lines(output, 'games', 'unfinished')
        if not res:
            return False, 'no h2h result line ({"games": ..., "unfinished": ...})', {}
        r = res[-1]
        if r['games'] != games:
            return False, f'{r["games"]} games played, expected {games}', {}
        if r['unfinished']:
            return False, f'{r["unfinished"]} of {games} games unfinished', {}
        if rc != 0:
            return False, f'exit {rc}', {}
        return True, f'checkpoint loads and plays ({games} games, 0 unfinished)', {}
    return checker


COMPAT_SIDE_RE = re.compile(r'^(old|new): (.+) \(([\d.]+) s; engine (.+)\)\s*$')
COMPAT_TARGET_RE = re.compile(r'^  (\w+): (\d+) samples, records ([0-9a-f]+), results ([0-9a-f]+)\s*$')


def make_compat_checker(old_root, new_root):
    """check_checkpoint_compat's positive evidence: its result line {"identical": true, "differences":
    []}; an "old:" line for the baseline checkout and a "new:" line for THIS checkout, each with that
    checkout's own mtgz/_engine and at least one policy target with samples > 0 (identical digests
    of nothing prove nothing); exit 0."""
    def checker(output, rc):
        sides, cur = {}, None
        for line in output.splitlines():
            m = COMPAT_SIDE_RE.match(line)
            if m:
                cur = m.group(1)
                sides[cur] = {'root': m.group(2), 'engine': m.group(4), 'targets': {}}
                continue
            m = COMPAT_TARGET_RE.match(line)
            if m and cur:
                sides[cur]['targets'][m.group(1)] = int(m.group(2))
        res = json_lines(output, 'identical')
        if not res:
            return False, f'no {{"identical": ...}} result line (exit {rc}): {last_line(output)}', {}
        r = res[-1]
        if r['identical'] is not True or r.get('differences'):
            return False, 'NOT identical: ' + '; '.join(r.get('differences') or ['?'])[:300], {}
        for side, root in (('old', old_root), ('new', new_root)):
            s = sides.get(side)
            if not s:
                return False, f'no "{side}: <checkout> (... s; engine ...)" line', {}
            if norm(s['root']) != norm(root):
                return False, f'the {side} side ran {s["root"]}, not {root}', {}
            if norm(os.path.dirname(s['engine'])) != norm(os.path.join(root, 'mtgz')):
                return False, f'the {side} side imported {s["engine"]}, not {root}\'s mtgz/_engine', {}
            if not s['targets'] or any(n <= 0 for n in s['targets'].values()):
                return False, f'the {side} side recorded no samples to compare ({s["targets"] or "no targets"})', {}
        if rc != 0:
            return False, f'exit {rc}', {}
        new = sides['new']['targets']
        return True, ('identical across both checkouts (' + ', '.join(f'{t}: {n} samples' for t, n in new.items())
                      + ')'), {'compat_samples': new}
    return checker


def make_build_checker(module, pkg_dir):
    """A build_py.sh step's positive evidence: build_engine.sh's last line, "<module> OK: <file> |
    games: [...]", printed only after it imported the module it just installed - from `pkg_dir`
    (this checkout's package, not another checkout's), with at least one game - and exit 0. A
    build that recompiled nothing still prints it; a failed compile, copy or import does not."""
    pat = re.compile(rf'^{re.escape(module)} OK: (.+?) \| games: \[(.*)\]\s*$', re.M)

    def checker(output, rc):
        ms = pat.findall(output)
        if not ms:
            return False, f'no "{module} OK: ..." line (the build, the copy or the import failed)', {}
        path, games = ms[-1]
        if norm(os.path.dirname(path)) != norm(pkg_dir):
            return False, f'{module} was imported from {path}, not from {pkg_dir}', {}
        if not games.strip():
            return False, f'{module} lists no games', {}
        if rc != 0:
            return False, f'exit {rc} after "{module} OK"', {}
        return True, f'{module} built and imported from {path} (games: {games})', {'module': path}
    return checker


BUILD_OK_RE = re.compile(r'^\s*ok\s+(\S+): build \S+ \(\d+ s\)\s*$', re.M)
WITH_CORE_RE = re.compile(r'^=== (\S+): worktree .* with zero-core ([0-9a-f]+)\s*$', re.M)
GAMES_CHECKED_RE = re.compile(r'^\s*games: (\d+) of (\d+) checked', re.M)


def make_build_only_checker(names, core_pin):
    """smoke_all.sh --with-this-core --build-only's positive evidence: an "ok <game>: build ..." summary
    line for EVERY game the step names, each one's "worktree ... with zero-core <sha>" line matching
    this checkout's pin, the summary's "games: N of N checked" for exactly those N games, and exit 0
    - not just GREEN, which smoke_all printed with no game at all when it found none (ENGINE.md 90)."""
    def checker(output, rc):
        built = set(BUILD_OK_RE.findall(output)) - {'core'}       # ("ok core: build zero._engine" is the core's)
        extra = {'built': sorted(built)}
        missing = [n for n in names if n not in built]
        if missing:
            return False, (f'no "ok <game>: build" line for {", ".join(missing)} (built: '
                           f'{", ".join(sorted(built)) or "none"}) - a game never built is not a pass'), extra
        cores = dict(WITH_CORE_RE.findall(output))
        wrong = [n for n in names if not core_pin.startswith(cores.get(n) or 'x')]
        if wrong:
            return False, (f'{", ".join(wrong)}: not built in a worktree with zero-core {core_pin[:12]} '
                           f'(seen: {cores or "none"})'), extra
        m = GAMES_CHECKED_RE.search(output)
        if not m or int(m.group(1)) != len(names) or int(m.group(2)) != len(names):
            return False, (f'no "games: {len(names)} of {len(names)} checked" summary line (seen: '
                           f'{m.group(0).strip() if m else "none"})'), extra
        if rc != 0:
            return False, f'exit {rc}: ' + last_line(output), extra
        return True, f'{len(names)} game(s) built against this core: {", ".join(names)}', extra
    return checker


def make_metrics_checker(run_dir, want_games=None, want_iters=1):
    """A run-loop smoke's real positive evidence: metrics.jsonl itself (not log text) - every
    iteration present, the last one's self-play counts present, no unfinished games, more than 0
    games (exactly `want_games` for a known preset)."""
    def checker(output, rc):
        if rc != 0:
            return False, f'exit {rc}', {}
        mfile = os.path.join(run_dir, 'metrics.jsonl')
        if not os.path.exists(mfile):
            return False, 'no metrics.jsonl written', {}
        with open(mfile, encoding='utf-8') as f:
            rows = [json.loads(line) for line in f if line.strip()]
        iters = [r.get('iter') for r in rows]
        if iters != list(range(want_iters + 1)):
            return False, f'iterations {iters} != {list(range(want_iters + 1))}', {}
        sp = rows[-1].get('selfplay') or {}
        if 'games' not in sp or 'unfinished' not in sp:
            return False, f'the last metrics row has no self-play games / unfinished counts ({sorted(sp)})', {}
        if sp['unfinished'] != 0:
            return False, f'{sp["unfinished"]} unfinished games', {}
        if not sp['games'] or (want_games is not None and sp['games'] != want_games):
            return False, f'{sp["games"]} games != expected {want_games or "> 0"}', {}
        return True, f'{sp["games"]} games, 0 unfinished', {'games': sp['games']}
    return checker


MAC_LINE_RE = re.compile(r'^\[mac\] (\S+) (\S+) @ ([0-9a-f]{7,40}): ', re.M)


def mac_commit_problem(output, sha):
    """None when mac.sh's "[mac] <host> <branch> @ <sha9>: <cmd>" line is there for this commit, else
    why not: the positive evidence that a Mac step reached the Mac and ran the gated commit."""
    m = MAC_LINE_RE.search(output)
    if not m:
        return 'no "[mac] <host> <branch> @ <sha>:" line (mac.sh never reached the Mac?)'
    if not sha.startswith(m.group(3)):
        return f'the Mac ran {m.group(3)}, not {sha[:9]}'
    return None


def make_mac_checker(sha, inner):
    def checker(output, rc):
        why = mac_commit_problem(output, sha)
        return (False, why, {}) if why else inner(output, rc)
    return checker


def make_eval_checkout_checker(sha):
    """repin's `mac.sh --eval-checkout`: the "[mac] ... @ <sha9>" line, the Mac's build line
    ("mtgz._engine OK: .../run-<sha12>/mtgz/..."), `mtgz.az.remote check`'s "mtgz remote: <dir> is
    <sha12>, unchanged, its engine built there", "eval checkout ready: .../run-<sha12>", exit 0."""
    run = f'run-{sha[:12]}'

    def checker(output, rc):
        why = mac_commit_problem(output, sha)
        if why:
            return False, why, {}
        m = re.search(r'^mtgz\._engine OK: (.+?) \| games: \[(.+)\]\s*$', output, re.M)
        if not m or f'/{run}/' not in m.group(1).replace('\\', '/'):
            return False, f'no "mtgz._engine OK: .../{run}/mtgz/..." build line from the Mac', {}
        if not re.search(rf'^mtgz remote: \S.* is {sha[:12]}, unchanged, its engine built there\s*$', output, re.M):
            return False, f'no "mtgz remote: ... is {sha[:12]}, unchanged, its engine built there" line', {}
        m = re.search(r'^eval checkout ready: (.+?)\s*$', output, re.M)
        if not m or not m.group(1).rstrip('/').endswith('/' + run):
            return False, f'no "eval checkout ready: .../{run}" line', {}
        if rc != 0:
            return False, f'exit {rc}', {}
        return True, f'eval checkout ready: {m.group(1)}', {}
    return checker


# ---------------------------------------------------------------------------- steps and execution

@dataclasses.dataclass
class FlakyRetry:
    """A pytest step's retry policy (ENGINE.md 91): when the only failures are tests in `file`, rerun
    each alone once (`argv(node_id, k)`); the step passes only if every one passes alone, recorded as
    "flaky, passed on retry" in the report and the final line."""
    file: str
    argv: object


@dataclasses.dataclass
class Step:
    name: str
    argv: list
    cwd: str = None
    env: dict = None
    checker: object = check_cargo_test        # callable(output, rc) -> (passed, reason, extra)
    mac: bool = False
    group: str = None                          # steps sharing a group run concurrently
    skip_reason: str = None                    # set ONLY by an explicit opt-out: SKIPPED, named in the final line
    cleanup: object = None                     # callable() after the step passed (e.g. rmtree a tmp dir)
    id: str = None                             # short name: --allow-skip, the report, the final line
    evidence: str = ''                         # what the checker requires (printed by --dry-run)
    record_only: bool = False                  # a digest recorded, not compared: RECORDED, never PASSED
    not_needed: str = None                     # a conditional step whose condition is false: N/A, with why
    flaky: FlakyRetry = None
    prepare: object = None                     # callable() right before the step runs (never at --dry-run)

    def __post_init__(self):
        self.id = self.id or slug(self.name)


@dataclasses.dataclass
class StepResult:
    name: str
    id: str
    argv: list
    cwd: str
    seconds: float
    rc: object
    log: str
    log_sha256: str
    status: str            # passed | failed | skipped | recorded | not needed
    reason: str
    extra: dict
    mac: bool = False

    @property
    def passed(self):
        return self.status == 'passed'

    @property
    def skipped(self):
        return self.status == 'skipped'


def child_priority():
    """The Windows priority class for a step: below normal - or idle when gate.py itself runs at idle
    (an idle wrapper beside a live run): never above its own."""
    try:
        import ctypes
        k32 = ctypes.windll.kernel32
        k32.GetCurrentProcess.restype = ctypes.c_void_p
        k32.GetPriorityClass.argtypes = [ctypes.c_void_p]
        if k32.GetPriorityClass(k32.GetCurrentProcess()) == subprocess.IDLE_PRIORITY_CLASS:
            return subprocess.IDLE_PRIORITY_CLASS
    except (OSError, AttributeError):
        pass
    return subprocess.BELOW_NORMAL_PRIORITY_CLASS


def execute(argv, cwd, env, timeout):
    """Run one command -> (its stdout + stderr as text, its exit code or 'timeout'). stdin is closed (a
    command that reads it gets EOF instead of hanging or reading ours). Module-level so tests stub it."""
    kw = {'creationflags': child_priority()} if WIN else {}
    try:
        p = subprocess.run(argv, cwd=cwd, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                           stderr=subprocess.STDOUT, text=True, encoding='utf-8', errors='replace',
                           timeout=timeout, **kw)
        return p.stdout, p.returncode
    except subprocess.TimeoutExpired as e:
        out = e.stdout or ''
        if isinstance(out, bytes):          # TimeoutExpired.output is bytes even with text=True
            out = out.decode('utf-8', 'replace')
        return out + f'\n[gate.py: killed after {timeout}s timeout]\n', 'timeout'


SCRUBBED_ENV = ('PYTHONPATH', 'PYTEST_ADDOPTS', 'ZERO_CORE_UNPINNED')    # and every DIGEST_*


def step_env(ctx, step):
    """The step's environment: the caller's, the gate's base (CPU only, the target dir, ...) and the
    step's own (None = unset). Never the caller's DIGEST_* (DIGEST_WRITE re-records digests.txt
    instead of comparing; DIGEST_RANDOM / _HEURISTIC / _ENCODING / _IDS change the digest),
    PYTEST_ADDOPTS (can deselect tests), PYTHONPATH (can import another checkout) or
    ZERO_CORE_UNPINNED (builds a core that is not the pin): each step states what it wants."""
    env = dict(os.environ)
    env.update(ctx.base_env)
    for k in list(env):
        if k.upper().startswith('DIGEST_') or k.upper() in SCRUBBED_ENV:
            del env[k]
    for k, v in (step.env or {}).items():
        if v is None:
            env.pop(k, None)
        else:
            env[k] = v
    return env


def retry_flaky(ctx, step, env, out, reason, extra, log_parts):
    """The flaky policy (FlakyRetry): -> (passed, reason, extra) after rerunning each failed test of
    `step.flaky.file` alone once - or the failure unchanged when any failure is elsewhere."""
    ids, why = flaky_retry_candidates(out, step.flaky.file)
    if not ids:
        return False, (reason if why == 'no failures' else f'{reason} (not retried: {why})'), extra
    retries = []
    for k, nid in enumerate(ids, 1):
        argv = step.flaky.argv(nid, k)
        rout, rrc = execute(argv, step.cwd or ctx.root, env, ctx.step_timeout)
        log_parts.append(f'\n\n$ {show(argv)}\n[gate.py: flaky retry {k}/{len(ids)}: {nid} alone]\n\n{rout}')
        rok, rreason, _ = check_pytest(rout, rrc) if rrc != 'timeout' else (False, 'timed out', {})
        retries.append({'test': nid, 'passed': rok, 'reason': rreason})
    extra = dict(extra, retries=retries)
    again = [r['test'] for r in retries if not r['passed']]
    if again:
        return False, f'{reason}; rerun alone, failed again: {", ".join(again)}', extra
    extra['flaky'] = ids
    return True, f'{reason} in the full run; flaky, passed on retry alone: {", ".join(ids)}', extra


def run_step(ctx, step):
    cwd = step.cwd or ctx.root
    if step.not_needed:
        return StepResult(step.name, step.id, step.argv, cwd, 0.0, None, None, None, 'not needed',
                          f'not needed: {step.not_needed}', {}, mac=step.mac)
    if step.skip_reason:
        return StepResult(step.name, step.id, step.argv, cwd, 0.0, None, None, None, 'skipped',
                          f'skipped: {step.skip_reason}', {}, mac=step.mac)
    log_path = os.path.join(ctx.logs_dir, f'{ctx.sha_short}-{ctx.tier}-{slug(step.name)}.log')
    env = step_env(ctx, step)
    if step.prepare:
        step.prepare()
    t0 = time.time()
    out, rc = execute(step.argv, cwd, env, ctx.step_timeout)
    log_parts = [f'$ {show(step.argv)}\ncwd: {cwd}\nenv overrides: {step.env or {}}\n\n', out]
    if rc == 'timeout':
        passed, reason, extra = False, f'timed out after {ctx.step_timeout}s', {}
    else:
        passed, reason, extra = step.checker(out, rc)
        if not passed and step.flaky:
            passed, reason, extra = retry_flaky(ctx, step, env, out, reason, extra, log_parts)
    seconds = time.time() - t0
    with open(log_path, 'w', encoding='utf-8', errors='replace') as f:
        f.write(''.join(log_parts))
    # the checker may need files the step wrote (e.g. a run directory's metrics.jsonl): clean up
    # only AFTER checking, and only on success (a failure's run dir stays for debugging).
    if step.cleanup and passed:
        try:
            step.cleanup()
        except OSError:
            pass
    status = ('recorded' if step.record_only else 'passed') if passed else 'failed'
    return StepResult(step.name, step.id, step.argv, cwd, seconds, rc, log_path, sha256_file(log_path), status,
                      reason, extra, mac=step.mac)


def batches(steps):
    """The steps in run order, a group of consecutive same-`group` steps as one concurrent batch."""
    i = 0
    while i < len(steps):
        j = i + 1
        while steps[i].group is not None and j < len(steps) and steps[j].group == steps[i].group:
            j += 1
        yield steps[i:j], steps[j:]
        i = j


def run_steps(ctx, steps):
    """-> (results, ok, the steps not run after a failure)."""
    results = []
    for batch, rest in batches(steps):
        if len(batch) == 1:
            out = [run_step(ctx, batch[0])]
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
        if any(r.status == 'failed' for r in out):
            return results, False, rest
    return results, True, []


TAGS = {'passed': 'PASS', 'failed': 'FAIL', 'skipped': 'SKIP', 'recorded': 'RECORDED', 'not needed': 'N/A'}


def report_step(r):
    mac = ' [mac]' if r.mac else ''
    print(f'{TAGS[r.status]:8s}{mac} {r.name} ({r.seconds:.1f}s): {r.reason}', flush=True)


# ---------------------------------------------------------------------------- the final line

def caveats(results, dirty):
    """-> (skipped, recorded, flaky) descriptions: everything a pass did NOT fully check."""
    skipped = [f'{r.id} ({r.reason[len("skipped: "):]})' for r in results if r.status == 'skipped']
    if dirty:
        skipped.insert(0, f'clean-tree (--allow-dirty: {len(dirty)} uncommitted change(s) gated, not the commit)')
    recorded = [f'{r.id} = {r.extra.get("digest")} (--record-digest)' for r in results if r.status == 'recorded']
    flaky = [f'{r.id}: {", ".join(r.extra["flaky"])}' for r in results if r.extra.get('flaky')]
    return skipped, recorded, flaky


def pass_head(skipped, recorded, flaky):
    head = 'GATE PASSED'
    if skipped or recorded:
        head += ' WITH SKIPS'
    if flaky:
        head += ' AND FLAKY RETRIES' if (skipped or recorded) else ' WITH FLAKY RETRIES'
    return head


def caveat_text(skipped, recorded, flaky):
    parts = []
    if skipped:
        parts.append('skipped: ' + ', '.join(skipped))
    if recorded:
        parts.append('recorded, not compared: ' + ', '.join(recorded))
    if flaky:
        parts.append('flaky, passed on retry: ' + ', '.join(flaky))
    return '; '.join(parts)


def final_line(tier, sha_short, ok, results, not_run, seconds, path, dirty=()):
    """The one line a caller reads: "GATE PASSED" only when every step passed on its own positive
    evidence; "GATE PASSED WITH SKIPS ..." / "... FLAKY RETRIES ..." naming every opted-out step,
    record-only digest and flaky retry; "GATE FAILED ..." with the failing step(s) and what never ran."""
    if not ok:
        why = '; '.join(f'{r.id}: {r.reason[:300]}' for r in results if r.status == 'failed')
        nr = f'; not run: {", ".join(s.id for s in not_run)}' if not_run else ''
        return f'GATE FAILED {tier} {sha_short} in {seconds:.0f}s - {why}{nr}; report {path}'
    sk, rec, fl = caveats(results, dirty)
    text = caveat_text(sk, rec, fl)
    return f'{pass_head(sk, rec, fl)} {tier} {sha_short} in {seconds:.0f}s' + (f' - {text}' if text else '') + \
        f'; report {path}'


def print_plan(ctx, steps):
    print(f'plan for tier {ctx.tier!r} (sha {ctx.sha_short}, zero-core pin {(ctx.core_pin or "none")[:12]}, '
          f'target dir {ctx.target_dir}):')
    for batch, _ in batches(steps):
        prefix = '  (parallel) ' if len(batch) > 1 else '  '
        pad = ' ' * len(prefix)
        for s in batch:
            mac = ' [mac, via tools/mac.sh]' if s.mac else ''
            if s.not_needed:
                print(f'{prefix}- [{s.id}] {s.name}{mac}: NOT NEEDED ({s.not_needed})')
            elif s.skip_reason:
                print(f'{prefix}- [{s.id}] {s.name}{mac}: SKIPPED ({s.skip_reason})')
            else:
                env = f', env {s.env}' if s.env else ''
                rec = ' [RECORD ONLY: never PASSED]' if s.record_only else ''
                print(f'{prefix}- [{s.id}] {s.name}{mac}{rec}: {show(s.argv)}  (cwd={s.cwd or ctx.root}{env})')
                if s.evidence:
                    print(f'{pad}    passes on: {s.evidence}')
                if s.flaky:
                    print(f'{pad}    flaky policy: failures ONLY in {s.flaky.file} -> each rerun alone once; passes only '
                          f'if all pass alone ("flaky, passed on retry"); any other failure fails the step')
    sk = [f'{s.id} ({s.skip_reason})' for s in steps if s.skip_reason and not s.not_needed]
    if ctx.dirty:
        sk.insert(0, f'clean-tree (--allow-dirty: {len(ctx.dirty)} uncommitted change(s))')
    rec = [f'{s.id} (--record-digest)' for s in steps if s.record_only and not s.skip_reason]
    text = caveat_text(sk, rec, [])
    print(f'on success: {pass_head(sk, rec, [])}' + (f' - {text}' if text else ' (every step on positive evidence)'))


# ---------------------------------------------------------------------------- target guard, tree

def target_guard(target_dir, checkout_root):
    """Refuse a CARGO_TARGET_DIR already marked for a different checkout (one checkout's relative
    dep-info silently satisfying another's build is the bug reference_cargo_shared_target_dir.md
    describes); mark a fresh one for this checkout."""
    target_dir = os.path.abspath(target_dir)
    checkout_root = os.path.abspath(checkout_root)
    os.makedirs(target_dir, exist_ok=True)
    marker = os.path.join(target_dir, '.gate_checkout.json')
    if os.path.exists(marker):
        with open(marker, encoding='utf-8') as f:
            info = json.load(f)
        if norm(info.get('checkout', '') or '.') != norm(checkout_root):
            raise GateError(
                f'CARGO_TARGET_DIR {target_dir} is marked for checkout {info.get("checkout")!r}, not '
                f'{checkout_root!r}: refusing (a shared target dir between checkouts can silently reuse the '
                f'other one\'s relative dep-info paths - reference_cargo_shared_target_dir.md). Use a separate '
                f'CARGO_TARGET_DIR for this checkout, or --target-dir to point at one already marked for it.')
    else:
        with open(marker, 'w', encoding='utf-8') as f:
            json.dump({'checkout': checkout_root, 'created': iso_now()}, f, indent=1)
    return target_dir


def tracked_changes():
    """`git status --porcelain --untracked-files=no`: the tracked files (and the zero-core submodule)
    that differ from HEAD - none for a committed checkout at its pin."""
    p = subprocess.run(['git', 'status', '--porcelain', '--untracked-files=no'], cwd=ROOT, capture_output=True,
                       text=True)
    if p.returncode:
        raise GateError(f'git status ({ROOT}): {p.stderr.strip()}')
    return [line for line in p.stdout.splitlines() if line.strip()]


def check_tree(args):
    dirty = tracked_changes()
    if dirty and not args.allow_dirty:
        shown = ', '.join(line.strip() for line in dirty[:8]) + (' ...' if len(dirty) > 8 else '')
        raise GateError(f'uncommitted changes to tracked files ({shown}): the gate would check them under the '
                        f'commit\'s name. Commit them (a WIP commit is fine; a submodule not at the pin: `git '
                        f'submodule update third_party/zero-core`), or --allow-dirty (named in the final line).')
    return dirty


# ---------------------------------------------------------------------------- context

def env_jobs():
    try:
        return max(1, int(os.environ.get('CARGO_BUILD_JOBS', '')))
    except ValueError:
        return 4


class Ctx:
    def __init__(self, args):
        self.tier = args.tier
        self.root = ROOT
        self.jobs = args.jobs
        self.step_timeout = args.step_timeout
        head = git(['rev-parse', 'HEAD'])
        if args.sha and (len(args.sha) < 7 or not head.startswith(args.sha.lower())):
            raise GateError(f"--sha {args.sha} does not match this checkout's HEAD {head}: gate.py runs IN the "
                            f"checkout to gate (a pinned worktree the caller creates first), it does not create "
                            f"one. `git -C {ROOT} worktree add --detach <dir> {args.sha}` and run gate.py there.")
        self.sha = head
        self.sha_short = head[:12]
        # the commit this checkout PINS (HEAD's gitlink): what the core checks compare against, known even
        # before tools/build_py.sh initialises the submodule; the clean-tree check keeps the checkout at it
        self.core_pin = git(['rev-parse', 'HEAD:third_party/zero-core'], check=False) or None
        self.core_head = (git(['rev-parse', 'HEAD'], cwd=CORE, check=False) or None) \
            if os.path.exists(os.path.join(CORE, '.git')) else None
        default_target = os.environ.get('CARGO_TARGET_DIR') or os.path.join(
            'E:/cargo-target/mtg-zero' if WIN else os.path.join(ROOT, 'target'),
            os.path.basename(ROOT) if os.path.basename(ROOT) != 'mtg-zero' else 'gate')
        self.target_dir = os.path.abspath(args.target_dir or default_target)
        self.logs_dir = os.path.join(GATES_DIR, 'logs')
        self.base_env = {
            'CUDA_VISIBLE_DEVICES': '-1',              # an empty string does NOT hide the GPU on Windows
            'CARGO_TARGET_DIR': self.target_dir,
            'CARGO_BUILD_JOBS': str(self.jobs),
            'PYTHONIOENCODING': 'utf-8',
            'RUST_BACKTRACE': '1',
        }
        self.no_mac = args.no_mac
        self.dirty = []
        self.baseline = None


def python_exe():
    return sys.executable


def mac_step(ctx, name, remote_cmd, inner, step_id, evidence):
    """A Mac part of a tier: `bash tools/mac.sh '<remote_cmd>'`, which pushes HEAD and runs it on the
    checkout's exact commit there (ENGINE.md 44: bit-identical results). Its checker wants mac.sh's
    "[mac] ... @ <this sha>" line before `inner`'s own positive evidence."""
    return Step(name, ['bash', 'tools/mac.sh', remote_cmd], id=step_id, mac=True,
                checker=make_mac_checker(ctx.sha, inner),
                evidence='mac.sh\'s "[mac] <host> <branch> @ <this sha>" line, then ' + evidence)


def need_core():
    if not os.path.exists(os.path.join(CORE, '.git')):
        raise GateError(f'{CORE} is not initialised: `git submodule update --init third_party/zero-core` (or bash '
                        f'tools/build_py.sh) first')


def opted_out(args, step_id):
    return step_id in (args.allow_skip or [])


# ---------------------------------------------------------------------------- shared steps

def build_step(ctx, name='build'):
    pkg = os.path.join(ROOT, 'mtgz')
    return Step(name, ['bash', 'tools/build_py.sh'], id='build', checker=make_build_checker('mtgz._engine', pkg),
                evidence=f'"mtgz._engine OK: {pkg}{os.sep}_engine.* | games: [...]" (the fresh module, imported from '
                         f'this checkout) + exit 0')


def pytest_step(name='pytest', step_id='pytest'):
    return Step(name, [python_exe(), '-m', 'pytest', 'tests', '-q', '-p', 'no:cacheprovider'], id=step_id,
                checker=check_pytest, evidence='pytest\'s final summary line: 0 failed, 0 errors, N > 0 passed + exit 0')


def cargo_step(name, test, step_id=None):
    return Step(name, ['cargo', 'test', '--release', '-p', 'engine', '--test', test], id=step_id or f'cargo-test-{test}',
                checker=make_cargo_checker(single=True),
                evidence='exactly one "test result: ok. N passed; 0 failed" line, N > 0 + exit 0')


def digest_steps(ctx, args):
    """The two digest steps (encoding-free, full), each comparing with --expect-digest0 / --expect-digest
    - REQUIRED - or, with --record-digest encoding-free / full, recording only (RECORDED, never PASSED,
    named in the final line). The full digest test also compares with the checkout's own
    tests/data/digests.txt either way (it fails on a mismatch)."""
    record = set(args.record_digest or [])
    steps = []
    for which, flag, expect, env, name in (
            ('encoding-free', '--expect-digest0', args.expect_digest0, {'DIGEST_ENCODING': '0'},
             'digest encoding-free (DIGEST_ENCODING=0)'),
            ('full', '--expect-digest', args.expect_digest, None, 'digest full')):
        if expect and which in record:
            raise GateError(f'both {flag} and --record-digest {which}: compare or record, not both')
        if not expect and which not in record:
            raise GateError(f'the {ctx.tier} tier compares the {which} digest: it needs {flag} <16 hex> (the value '
                            f'it must equal); --record-digest {which} records it without comparing (RECORDED, never '
                            f'PASSED, and named in the final line)')
        games = f'{DIGEST_GAMES[0]} + {DIGEST_GAMES[1]}'
        steps.append(Step(name, list(DIGEST_ARGV), env=env, id=f'digest-{which}', record_only=not expect,
                          checker=make_digest_checker(expect, record_only=not expect),
                          evidence=(f'one "combined digest" line over the full {games} games == {expect}' if expect
                                    else f'one "combined digest" line over the full {games} games (recorded, NOT '
                                         f'compared)') + ', no re-record, "test result: ok. 1 passed" + exit 0'))
    return steps


def core_pytest_base(ctx):
    """pytest's --basetemp for the core suite (it writes runs and checkpoints): off C: on the desktop, as
    smoke_all.sh does; unique per gate run; NOT created here (--dry-run touches no disk)."""
    name = f'core-{ctx.sha_short}-{uuid.uuid4().hex[:8]}'
    if WIN:
        return f'E:/zero-smoke/mtgz-gate-{slug(os.path.basename(ROOT))}-pytest/{name}'
    return os.path.join(GATES_DIR, 'tmp', f'pytest-{name}')


def core_steps(ctx):
    """zero-core's own suite as its own steps (ENGINE.md 91): its engine module (python/zero/_engine, the
    toy games its tests import) built by its own tools/build_py.sh, then `python -m pytest tests -q` from
    the core checkout - how zero-core's AGENTS.md and smoke_all.sh run it - CPU only and below normal
    priority like every step, with the FLAKY_FILE retry policy (FlakyRetry)."""
    base = core_pytest_base(ctx)

    def argv(target, tmp):
        return [python_exe(), '-m', 'pytest', target, '-q', '-p', 'no:cacheprovider', '-rfE',
                '-W', 'ignore::pytest.PytestWarning', f'--basetemp={tmp}']
    zero_pkg = os.path.join(CORE, 'python', 'zero')
    return [
        Step('core build (zero._engine)', ['bash', 'tools/build_py.sh'], cwd=CORE, id='core-build',
             checker=make_build_checker('zero._engine', zero_pkg),
             evidence=f'"zero._engine OK: {zero_pkg}{os.sep}_engine.* | games: [...]" + exit 0'),
        Step('core pytest', argv('tests', base), cwd=CORE, id='core-pytest', checker=check_pytest,
             flaky=FlakyRetry(FLAKY_FILE, lambda nid, k: argv(nid, f'{base}-retry{k}')),
             prepare=lambda: os.makedirs(os.path.dirname(base), exist_ok=True),
             cleanup=lambda: [shutil.rmtree(p, ignore_errors=True) for p in glob.glob(base + '*')],
             evidence='pytest\'s final summary line: 0 failed, 0 errors, N > 0 passed + exit 0'),
    ]


def check_baseline(ctx, path):
    """--baseline-root: an existing checkout, not this one, not the same commit and core (a vacuous
    comparison), with its mtgz engine module built (check_checkpoint_compat imports it as is)."""
    b = os.path.abspath(path)
    if not os.path.isdir(b):
        raise GateError(f'--baseline-root {path}: no such directory ({b})')
    if norm(b) == norm(ROOT):
        raise GateError(f'--baseline-root {path} is this checkout: the comparison would be vacuous')
    engines = [p for p in (os.path.join(b, 'mtgz', '_engine.pyd'), os.path.join(b, 'mtgz', '_engine.so'))
               if os.path.exists(p)]
    if not engines:
        raise GateError(f'--baseline-root {b} has no built mtgz/_engine.pyd (check_checkpoint_compat imports each '
                        f'checkout\'s module as is): `bash tools/build_py.sh` there first')
    head = git(['rev-parse', 'HEAD'], cwd=b)
    pin = git(['rev-parse', 'HEAD:third_party/zero-core'], cwd=b, check=False) or None
    if head == ctx.sha and pin == ctx.core_pin:
        raise GateError(f'--baseline-root {b} is at this commit ({head[:12]}, core {str(pin)[:12]}): the comparison '
                        f'would be vacuous - point it at the checkout before the change')
    ctx.baseline = {'root': b, 'head': head, 'zero_core_pin': pin, 'engine': engines[0],
                    'engine_sha256': sha256_file(engines[0]),
                    'engine_mtime': datetime.fromtimestamp(os.path.getmtime(engines[0]), timezone.utc).isoformat()}
    return b


def compat_step(ctx, args, name):
    sid = 'checkpoint-compat'
    if not args.baseline_root:
        if not opted_out(args, sid):
            raise GateError(f'the {ctx.tier} tier\'s point is check_checkpoint_compat against the checkout before the '
                            f'change: it needs --baseline-root <that checkout> (e.g. ../merge-main); --allow-skip {sid} '
                            f'runs without it (SKIPPED, named in the final line)')
        return Step(name, [], id=sid, skip_reason=f'--allow-skip {sid}: no --baseline-root, the byte-identity check '
                                                  f'did not run')
    base = check_baseline(ctx, args.baseline_root)
    return Step(name, [python_exe(), os.path.join(CORE, 'tools', 'check_checkpoint_compat.py'), '--old', base,
                       '--new', ROOT, '--package', 'mtgz', '--net', 'random', '--game', 'mtg', '--spec',
                       args.checkpoint_spec or '{"max_turns": 60}'], id=sid,
                checker=make_compat_checker(base, ROOT),
                evidence='{"identical": true, "differences": []}, the old side = the baseline\'s and the new side = '
                         'this checkout\'s mtgz/_engine, samples > 0 on both + exit 0')


def smoke_step(name, specs, extra_args=(), group=None):
    n = len(specs)              # one seed (0) per spec
    return Step(name, [python_exe(), 'tools/search_smoke.py', '--specs', ','.join(specs), '--seeds', '0',
                       *extra_args], id='search-smoke', checker=make_search_smoke_checker(n), group=group,
                evidence=f'"search smoke: 0 failed of {n} configurations" + exit 0')


def run_loop_step(ctx, preset, games):
    run_dir = lazy_run_dir(ctx, 'smoke')
    return Step(f'run-loop smoke ({preset})',
                [python_exe(), '-m', 'mtgz.az.run', '--run', run_dir, '--preset', preset, '--iters', '1'],
                id='run-loop-smoke', checker=make_metrics_checker(run_dir, want_games=games, want_iters=1),
                cleanup=lambda: shutil.rmtree(run_dir, ignore_errors=True),
                evidence=f'its metrics.jsonl: iterations [0, 1], {games or "> 0"} games, 0 unfinished + exit 0')


# ---------------------------------------------------------------------------- tiers

def tier_docs(ctx, args):
    files = args.files or sorted(glob.glob(os.path.join(ROOT, 'tools', '*.py')))
    missing = [f for f in files if not os.path.isfile(f)]
    if missing:
        raise GateError(f'--files: no such file: {", ".join(missing)}')
    if not files:
        raise GateError('no files to compile (a step that compiles nothing is not a pass)')
    rel = [os.path.relpath(f, ROOT) for f in files]
    return [Step('py_compile touched tools', [python_exe(), '-c', PYCOMPILE_CODE, *rel], id='py-compile',
                 checker=make_pycompile_checker(len(rel)),
                 evidence=f'"py_compile: {len(rel)} file(s) compiled" + exit 0')]


def tier_python(ctx, args):
    preset = args.smoke_preset or 'mtg-smoke'
    games = {'mtg-smoke': 16, 'delver-smoke': 16}.get(preset)
    if opted_out(args, 'build'):
        build = Step('build', [], id='build', skip_reason='--allow-skip build: pytest and the smoke used the existing '
                                                          'mtgz/_engine, not one built from this commit')
    else:
        build = build_step(ctx)
    return [build, pytest_step(), run_loop_step(ctx, preset, games)]


def tier_encoding(ctx, args):
    sid = 'checkpoint-load'
    if args.checkpoint:
        if not os.path.isfile(args.checkpoint):
            raise GateError(f'--checkpoint {args.checkpoint}: no such file')
        load = Step('checkpoint-load (mtgz.az.h2h)',
                    [python_exe(), '-m', 'mtgz.az.h2h', args.checkpoint, args.checkpoint, '--games', '8', '--sims', '16',
                     '--spec', args.checkpoint_spec or '{"max_turns": 60}', '--device', 'cpu', '--threads', '4',
                     '--select', 'visits'], id=sid, checker=make_h2h_checker(8),
                    evidence='h2h\'s result line: 8 games, 0 unfinished + exit 0')
    elif opted_out(args, sid):
        load = Step('checkpoint-load (mtgz.az.h2h)', [], id=sid,
                    skip_reason=f'--allow-skip {sid}: no --checkpoint, no checkpoint was loaded on the new encoding')
    else:
        raise GateError(f'the encoding tier loads a run\'s checkpoint on the new encoding: it needs --checkpoint <a .pt> '
                        f'(--checkpoint-spec for its spec); --allow-skip {sid} runs without it (SKIPPED, named in the '
                        f'final line)')
    steps = [build_step(ctx), *digest_steps(ctx, args), cargo_step('cargo test az', 'az'), load, pytest_step()]
    # NO search smoke (the uniform evaluator cannot see an encoding change).
    return steps


def tier_engine(ctx, args):
    steps = [build_step(ctx), *digest_steps(ctx, args),
             cargo_step('cargo test az', 'az'), cargo_step('cargo test limits', 'limits'),
             cargo_step('cargo test hidden', 'hidden'), pytest_step(),
             smoke_step('smoke: 1 desktop config (random, seed 0)', ['random'])]
    sid = 'mac-recheck'
    name = 'full suite again on the Mac (merged sha differs, overlapping files)'
    if args.tested_sha:
        tested = git(['rev-parse', '--verify', '--quiet', f'{args.tested_sha}^{{commit}}'], check=False)
        if not tested:
            raise GateError(f'--tested-sha {args.tested_sha}: not a commit in this repository')
        if tested == ctx.sha:
            steps.append(Step(name, [], id=sid, mac=True, not_needed='--tested-sha is this commit'))
        else:
            touched = git(['diff', '--name-only', tested, ctx.sha]).splitlines()
            overlap = [f for f in touched if f.startswith(('engine/', 'card_ir/'))]
            if overlap:
                steps.append(mac_step(ctx, name, 'cargo test --release --no-fail-fast', make_cargo_checker(), sid,
                                      '"test result:" lines all ok, 0 failed, N > 0 passed + exit 0'))
            else:
                steps.append(Step(name, [], id=sid, mac=True,
                                  not_needed=f'--tested-sha {tested[:12]} differs from this commit in {len(touched)} '
                                             f'file(s), none under engine/ or card_ir/'))
    elif args.no_mac or opted_out(args, sid):
        why = f'--allow-skip {sid}' if opted_out(args, sid) else '--no-mac'
        steps.append(Step(name, [], id=sid, mac=True,
                          skip_reason=f'{why}: no --tested-sha, so whether the recheck was needed was not decided'))
    else:
        raise GateError(f'the engine tier decides its Mac recheck from --tested-sha <the sha the agent fuzzed/tested> '
                        f'(this commit\'s own sha when it is gated directly); --no-mac or --allow-skip {sid} runs '
                        f'without it (SKIPPED, named in the final line)')
    return steps


def split_list(value, default, flag):
    items = [x for x in (value or default).split(',') if x]
    if not items:
        raise GateError(f'{flag}: an empty list (a step over nothing is not a pass)')
    return items


def tier_search_core(ctx, args):
    need_core()
    configs = split_list(args.configs, 'random,eval,match,nomull', '--configs')
    return [
        build_step(ctx),
        Step('zero-core cargo test', ['cargo', 'test', '--release'], cwd=CORE, id='core-cargo-test',
             evidence='"test result:" lines all ok, 0 failed, N > 0 passed + exit 0'),
        pytest_step('this repo: pytest'),
        compat_step(ctx, args, 'check_checkpoint_compat goldens'),
        smoke_step(f'smoke: {len(configs)}-config of the pinning game ({",".join(configs)})', configs),
        run_loop_step(ctx, 'mtg-smoke', 16),
        # smoke_all for the OTHER zero-core games is zero-core-side (its own tools/smoke_all.sh
        # --build-only), run by the orchestrator against the moving core commit - not this repo's gate.
        *core_steps(ctx),
    ]


def tier_optin_core(ctx, args):
    need_core()
    feature_tests = split_list(args.feature_tests, 'az,hidden', '--feature-tests')
    steps = [build_step(ctx),
             Step('core tests', ['cargo', 'test', '--release'], cwd=CORE, id='core-cargo-test',
                  evidence='"test result:" lines all ok, 0 failed, N > 0 passed + exit 0'),
             compat_step(ctx, args, 'checkpoint_compat byte identity')]
    for t in feature_tests:
        steps.append(cargo_step(f'feature tests: cargo test --test {t}', t, step_id=f'feature-{t}'))
    steps.append(build_only_step(ctx, args))
    steps.extend(core_steps(ctx))
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
    and its pytest are this tier's own steps ('core tests', 'core pytest' - ENGINE.md 91). Own
    SMOKE_TARGET / SMOKE_DIR per checkout, and the gate's CARGO_TARGET_DIR unset so no game ever builds
    into this repo's target."""
    if not ctx.core_pin:
        raise GateError('no zero-core pin in HEAD (third_party/zero-core): nothing to build the games against')
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
                cwd=CORE, id='build-only', env={'SKIP_CORE': '1', 'SMOKE_TARGET': smoke_target, 'SMOKE_DIR': smoke_dir,
                                                 'CARGO_TARGET_DIR': None},
                checker=make_build_only_checker(names, ctx.core_pin),
                evidence=f'an "ok <game>: build" line + "with zero-core {ctx.core_pin[:12]}" for each of {len(names)} '
                         f'game(s), "games: {len(names)} of {len(names)} checked" + exit 0')


def tier_repin(ctx, args):
    missing = [flag for flag, v in (('--expect-digest', args.expect_digest), ('--spec', args.spec),
                                    ('--preset', args.preset)) if not v]
    if missing:
        raise GateError(f'the repin tier needs {", ".join(missing)}: --expect-digest is main\'s recorded value (the '
                        f'whole point of this tier is that a re-pin changes nothing behaviourally, which needs a '
                        f'known-good value to compare against, not just "it ran"), --spec / --preset are the run\'s '
                        f'own (the smoke must exercise the run being re-pinned, not a default)')
    specs = split_list(args.spec, '', '--spec')
    # watch_run.sh is deliberately NOT run here: it watches a LIVE run directory after the re-pin
    # takes effect (no natural pass/fail endpoint for a one-shot gate) - an operator step after
    # this gate passes, documented in --help and ENGINE.md 70, not a scripted check.
    return [
        build_step(ctx, 'build (this checkout\'s own target)'),
        Step('digest equals main\'s recorded value', list(DIGEST_ARGV), id='digest-full',
             checker=make_digest_checker(args.expect_digest),
             evidence=f'one "combined digest" line over the full {DIGEST_GAMES[0]} + {DIGEST_GAMES[1]} games == '
                      f'{args.expect_digest}, "test result: ok. 1 passed" + exit 0'),
        smoke_step(f'1-config smoke on the run\'s spec ({args.spec}, preset {args.preset})', specs,
                   ['--preset', args.preset], group='parallel-mac'),
        Step('mac.sh --eval-checkout', ['bash', 'tools/mac.sh', '--eval-checkout'], id='mac-eval-checkout', mac=True,
             group='parallel-mac', checker=make_eval_checkout_checker(ctx.sha),
             evidence=f'"[mac] ... @ {ctx.sha[:9]}", the Mac\'s "mtgz._engine OK: .../run-{ctx.sha_short}/...", "mtgz '
                      f'remote: ... is {ctx.sha_short}, unchanged", "eval checkout ready: .../run-{ctx.sha_short}" + '
                      f'exit 0'),
    ]


TIERS = {
    'docs': (tier_docs, 'docs / tools not on the run path: py_compile of touched tools (~0 min).'),
    'python': (tier_python, 'Python run-loop/settings: build + pytest + a tiny run-loop smoke (~3 min + the build).'),
    'encoding': (tier_encoding, 'encoding-only (layout unchanged): build + DIGEST_ENCODING=0 digest == --expect-digest0 '
                 '+ full digest == --expect-digest + --test az + checkpoint-load (--checkpoint) + pytest. NO search '
                 'smoke (~7 min).'),
    'engine': (tier_engine, 'engine rules/cards (orchestrator half): fresh build, digests == the agent\'s report, '
               'az/limits/hidden, pytest, 1 desktop smoke config; a Mac full-suite recheck only if the merged sha '
               'differs from --tested-sha with overlapping engine/card_ir files (~10 min).'),
    'search-core': (tier_search_core, 'search/core behaviour: build + core cargo test + pytest + check_checkpoint_compat '
                    'goldens (--baseline-root) + 4-config smoke of the pinning game + run-loop smoke + core build + '
                    'core pytest (~35 min).'),
    'optin-core': (tier_optin_core, 'opt-in core feature with byte-identical defaults: build + core tests + '
                   'checkpoint_compat byte identity (--baseline-root) + feature tests + build-only of the other games '
                   '+ core build + core pytest (~25 min).'),
    'repin': (tier_repin, 're-pin onto an already-verified commit: own-target build + digest == main\'s recorded value '
              '+ 1-config smoke on the run\'s --spec/--preset + mac.sh --eval-checkout, in parallel (~5 min).'),
}

# The options each tier uses: any other tier-specific option given is refused (it would be ignored, and
# the check it names would not run).
TIER_OPTIONS = {
    'docs': {'files'},
    'python': {'smoke_preset', 'allow_skip'},
    'encoding': {'expect_digest', 'expect_digest0', 'record_digest', 'checkpoint', 'checkpoint_spec', 'allow_skip'},
    'engine': {'expect_digest', 'expect_digest0', 'record_digest', 'tested_sha', 'allow_skip'},
    'search-core': {'baseline_root', 'checkpoint_spec', 'configs', 'allow_skip'},
    'optin-core': {'baseline_root', 'checkpoint_spec', 'feature_tests', 'other_games', 'allow_skip'},
    'repin': {'expect_digest', 'spec', 'preset'},
}
TIER_SPECIFIC = set().union(*TIER_OPTIONS.values())
# The steps a caller may opt out of by name (--allow-skip <id>); Mac steps also go with --no-mac.
SKIPPABLE = {'python': ('build',), 'encoding': ('checkpoint-load',), 'engine': ('mac-recheck',),
             'search-core': ('checkpoint-compat',), 'optin-core': ('checkpoint-compat',)}


def check_options(args):
    for dest in sorted(TIER_SPECIFIC - TIER_OPTIONS[args.tier]):
        if getattr(args, dest) not in (None, [], False, ''):
            raise GateError(f'--{dest.replace("_", "-")} is not used by the {args.tier} tier (it would be ignored, and '
                            f'the check it names would not run)')
    for s in args.allow_skip or []:
        if s not in SKIPPABLE.get(args.tier, ()):
            raise GateError(f'--allow-skip {s}: the {args.tier} tier has no skippable step of that name (skippable: '
                            f'{", ".join(SKIPPABLE.get(args.tier, ())) or "none"})')


def finish_plan(steps, args):
    """--no-mac marks every Mac step that would run as skipped; every --allow-skip must have skipped
    something (given the step's input as well, it is a contradiction, not a no-op)."""
    if args.no_mac:
        for s in steps:
            if s.mac and not s.skip_reason and not s.not_needed:
                s.skip_reason = '--no-mac'
    for sid in args.allow_skip or []:
        if not any(s.id == sid and s.skip_reason and '--allow-skip' in s.skip_reason for s in steps):
            raise GateError(f'--allow-skip {sid}: nothing to skip (its input was given - drop one of the two)')


# ---------------------------------------------------------------------------- report

def write_report(ctx, args, results, ok, not_run, started, seconds, path, line):
    sk, rec, fl = caveats(results, ctx.dirty)
    report = {
        'sha': ctx.sha, 'sha_short': ctx.sha_short, 'zero_core_pin': ctx.core_pin, 'zero_core_head': ctx.core_head,
        'tier': ctx.tier, 'started': started, 'seconds': round(seconds, 1), 'cargo_target_dir': ctx.target_dir,
        'passed': ok, 'clean': bool(ok and not (sk or rec or fl)), 'final': line,
        'skipped': sk, 'recorded': rec, 'flaky': fl, 'not_run': [s.id for s in not_run],
        'tree': {'uncommitted': ctx.dirty, 'allow_dirty': bool(args.allow_dirty)},
        'baseline': ctx.baseline,
        'options': {k: v for k, v in sorted(vars(args).items()) if v not in (None, [], False) and k != 'dry_run'},
        'digests': {}, 'test_counts': {}, 'steps': [],
    }
    for r in results:
        report['steps'].append({'id': r.id, 'name': r.name, 'status': r.status, 'passed': r.passed,
                                'skipped': r.skipped, 'reason': r.reason, 'cmd': r.argv, 'cwd': r.cwd,
                                'seconds': round(r.seconds, 2), 'rc': r.rc, 'log': r.log, 'log_sha256': r.log_sha256,
                                'mac': r.mac, **({'retries': r.extra['retries']} if 'retries' in r.extra else {})})
        if r.extra.get('digest'):
            key = 'encoding_free' if 'encoding-free' in r.id else 'full'
            report['digests'][key] = {'value': r.extra['digest'], 'status': r.status}
        if 'passed' in r.extra and 'failed' in r.extra:
            report['test_counts'][r.name] = {k: r.extra[k] for k in ('passed', 'failed', 'errors', 'skipped', 'binaries')
                                             if k in r.extra}
    os.makedirs(GATES_DIR, exist_ok=True)
    with open(path, 'w', encoding='utf-8') as f:
        json.dump(report, f, indent=1)
    return path


# ---------------------------------------------------------------------------- main

def hex16(value):
    v = value.strip().lower()
    if not re.fullmatch(r'[0-9a-f]{16}', v):
        raise argparse.ArgumentTypeError(f'{value!r} is not a 16-hex-digit combined digest')
    return v


def build_parser():
    ap = argparse.ArgumentParser(
        prog='gate.py', formatter_class=argparse.RawDescriptionHelpFormatter,
        description=__doc__.split('\n\n')[0],
        epilog='Tiers:\n' + '\n'.join(f'  {name:12s} {desc}' for name, (_, desc) in TIERS.items()) +
               '\n\nSkippable steps (--allow-skip): ' + '; '.join(f'{t}: {", ".join(s)}' for t, s in SKIPPABLE.items()))
    ap.add_argument('tier', choices=sorted(TIERS))
    ap.add_argument('--sha', default=None, help="the commit being gated (default: this checkout's HEAD); must "
                    "match the checkout (gate.py never creates a worktree)")
    ap.add_argument('--tested-sha', default=None, help='engine tier (required): the sha the agent fuzzed/tested - '
                    'this commit\'s own when gated directly; the Mac recheck runs when a different one touched '
                    'engine/ or card_ir/')
    ap.add_argument('--expect-digest', type=hex16, default=None, help='encoding/engine/repin (required): the full '
                    'combined digest the digest step must equal')
    ap.add_argument('--expect-digest0', type=hex16, default=None, help='encoding/engine (required): the '
                    'DIGEST_ENCODING=0 combined digest the digest step must equal')
    ap.add_argument('--record-digest', action='append', choices=('full', 'encoding-free'), default=None,
                    help='encoding/engine: record that digest WITHOUT comparing, in place of its --expect-digest[0] '
                    '(the step is RECORDED, never PASSED, and the final line says so); may repeat')
    ap.add_argument('--checkpoint', default=None, help='encoding tier (required): a .pt checkpoint for the h2h '
                    'load/play check')
    ap.add_argument('--checkpoint-spec', default=None, help='the mtg spec JSON for --checkpoint / '
                    'check_checkpoint_compat (default the smoke spec, max_turns 60)')
    ap.add_argument('--baseline-root', default=None, help='search-core/optin-core (required): the checkout before the '
                    'change, to compare against with check_checkpoint_compat (its mtgz engine built)')
    ap.add_argument('--spec', default=None, help='repin tier (required): the search_smoke spec of the run being '
                    're-pinned (e.g. delver)')
    ap.add_argument('--preset', default=None, help='repin tier (required): the run preset (e.g. delver-mirror)')
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
    ap.add_argument('--allow-skip', action='append', default=None, metavar='STEP', help='run without a skippable '
                    'step\'s essential input (see "Skippable steps" below); the step is SKIPPED and the final line '
                    'says "GATE PASSED WITH SKIPS ... skipped: STEP (...)"; may repeat')
    ap.add_argument('--allow-dirty', action='store_true', help='gate uncommitted changes to tracked files (refused '
                    'otherwise; named in the final line)')
    ap.add_argument('--jobs', type=int, default=env_jobs(), help='CARGO_BUILD_JOBS (default: the environment\'s '
                    'CARGO_BUILD_JOBS, else 4)')
    ap.add_argument('--target-dir', default=None, help='override CARGO_TARGET_DIR (default: the env var if set, '
                    'else E:/cargo-target/mtg-zero/<this checkout\'s dir name>)')
    ap.add_argument('--step-timeout', type=float, default=3600.0, help='seconds before a single step is killed as '
                    'hung (default 3600)')
    ap.add_argument('--no-mac', action='store_true', help='skip Mac steps (SKIPPED, not failed, and named in the '
                    'final line); also stands in for the engine tier\'s --tested-sha')
    ap.add_argument('--dry-run', action='store_true', help='print the plan, or the refusal; run nothing, write '
                    'nothing')
    return ap


def main(argv=None):
    args = build_parser().parse_args(argv)
    try:
        check_options(args)
        ctx = Ctx(args)
        ctx.dirty = check_tree(args)
        steps = TIERS[args.tier][0](ctx, args)
        finish_plan(steps, args)
    except GateError as e:
        print(f'gate.py: {e}', file=sys.stderr)
        return 2
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
    os.makedirs(ctx.logs_dir, exist_ok=True)
    os.makedirs(os.path.join(GATES_DIR, 'tmp'), exist_ok=True)
    print(f'gate.py {args.tier}: sha {ctx.sha_short}, core pin {(ctx.core_pin or "none")[:12]}, target '
          f'{ctx.target_dir}', flush=True)
    started = iso_now()
    t0 = time.time()
    before = tracked_changes()
    results, ok, not_run = run_steps(ctx, steps)
    if ok:
        after = tracked_changes()
        changed = sorted(set(after) - set(before))
        tree = StepResult('tracked files unchanged by the gate', 'tree-unchanged', [], ctx.root, 0.0, None, None, None,
                          'failed' if changed else 'passed',
                          ('tracked files changed during the gate: ' + ', '.join(c.strip() for c in changed[:8]))
                          if changed else 'git status unchanged', {})
        results.append(tree)
        report_step(tree)
        ok = not changed
    seconds = time.time() - t0
    path = os.path.join(GATES_DIR, f'{ctx.sha_short}.json')
    line = final_line(args.tier, ctx.sha_short, ok, results, not_run, seconds, path, ctx.dirty)
    write_report(ctx, args, results, ok, not_run, started, seconds, path, line)
    print(line, flush=True)
    return 0 if ok else 1


if __name__ == '__main__':
    sys.exit(main())
