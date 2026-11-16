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
    digest line over the full 2,000 + 500 games, digest_formats' three lines (constructed, eoe, driver:
    ENGINE.md 148.1, 157) and dealt_digests' line (154, 157), check_checkpoint_compat's {"identical": true}
    with samples on both sides, a Mac step's "[mac] ... @ <this sha>" and "[mac] done @ <this sha>"
    lines (the Mac checkout's HEAD before and after the command), "0 failed of N
    configurations" with N the configurations asked for, ... A missing marker fails loudly.
  * **No silent skips.** A tier's essential inputs are required (encoding: --expect-digest,
    --expect-digest0, --checkpoint; engine: --expect-digest, --expect-digest0, --tested-sha and the
    tested sha's suite report; search-core and optin-core: --baseline-root; repin: --expect-digest,
    --spec, --preset): without
    one, gate.py refuses before running anything (exit 2) unless the caller opts out BY NAME -
    `--allow-skip <step>`, `--record-digest <which>` (record without comparing), `--no-mac`,
    `--allow-dirty`. An opted-out step is SKIPPED / RECORDED (never PASSED) in the report, and the
    final line names it: "GATE PASSED WITH SKIPS <tier> <sha> ... - skipped: <step> (<why>)". An
    option the tier does not use is refused too (it would be ignored, and the check it names
    would not run).
  * **The commit, unchanged.** The checkout's tracked files must match HEAD (the zero-core
    submodule at the pin), and no untracked file may sit where a build, a test or a run picks it
    up (mtgz/checkout.py: engine/, mtgz/, tests/, tools/, data/, ...) - else the gate would check
    code that is in no commit under the commit's name - and both must still hold afterwards, with
    HEAD unmoved (a step that rewrites a tracked file, e.g. re-recorded digests, fails the gate).
    A caller's DIGEST_*, FUZZ_*, PYTEST_ADDOPTS, PYTHONPATH and ZERO_CORE_UNPINNED never reach a
    step.
  * **Evidence, not assertions** (ENGINE.md 143). The engine tier no longer takes the full engine
    suite and the fuzz on the agent's word (--tested-sha): the `suite` tier runs them on a
    commit and its report gates/<sha12>-suite.json is the evidence the engine tier checks (tier,
    sha, pin, a clean pass, a fuzz step of at least FUZZ_GAMES games - 2,000 a pool since ENGINE.md 167 -, every log unchanged); without it the engine
    tier refuses unless --allow-skip suite-evidence, named in the final line.
  * **A timeout bounds the step** - the command and every process it started (tools/proctree.py).
  * **The core's own suite** (search-core, optin-core): zero-core's engine module built with its
    tools/build_py.sh, then its pytest as its own step ("core pytest"). If the ONLY failures are in
    tests/test_remoteplay.py (it times real threads: zero-core AGENTS.md), each failed test is
    rerun alone once; the step passes only if every one passes alone, recorded as "flaky, passed
    on retry" in the report and the final line. Any other failure fails the step.

  * **Farm compatibility is required** (AGENTS.md; ENGINE.md 155): the engine, encoding, search-core, optin-core
    and repin tiers build the release `mtgz-worker` (zero-farm's executable) from the commit into the gate's target
    dir before pytest (whose worker tests then run that build, never a stale one: MTGZ_WORKER) and run
    tools/farm_compat.py with it: gate 2 (the worker's self-play vs zero-core's Python remote worker) and gate 3 (its
    records replayed by the trainer's path), byte-identical, on the Delver best-of-three dealt preset. Its positive
    evidence: exactly one "farm compat: PASSED - ..." line and its JSON summary "passed": true. Not skippable.

  * **Off the desktop beside a live run** (ENGINE.md 166): with a run live on this machine (zero.runctl.live_supervisors)
    the steps with a Mac command - cargo tests, digests, the engine suite, the fuzz, pytest, the run-loop smoke, the
    search smoke (not zero-core's own tests: its search goldens are x86's) - run on the Mac (tools/mac.sh: this commit's checkout, `nice`, a
    Mac-side tools/slotd.sh slot) when it answers with memory to spare, else on the desktop; the desktop's own heavy
    steps (builds, the Windows worker, farm compat, checkpoint checks, what needs private data: PYTEST_DESKTOP_ONLY as
    pytest's desktop part, beside the Mac's) each hold a tools/slotd.sh slot. Same checkers, same evidence (the Mac's
    engine is bit-identical: ENGINE.md 44.2, 166.2); a Mac step that never reached the Mac runs on the desktop.
    --offload auto|mac|local, --throttle auto|on|off, --no-mac (nothing on the Mac). Tests the live-run fence skipped
    (zero-core docs/az.md 51) are named in the final line as skips.

Run from WITHIN the checkout to gate (a pinned worktree the caller creates first, e.g. `git
worktree add --detach ../mtg-zero-wt/run-<sha> <sha>`, exactly as the old scripts did) - this tool
never creates or manages a worktree itself, the same contract as tools/build_py.sh and
tools/search_smoke.py. `--sha` (optional) must match the checkout's HEAD or the gate refuses
(never silently gates the wrong commit).

    python tools/gate.py docs
    python tools/gate.py python
    python tools/gate.py encoding --expect-digest <full> --expect-digest0 <encoding-free> --checkpoint <a .pt>
    python tools/gate.py suite [--fuzz-pools default,delver]      # the agent's half of an engine change
    python tools/gate.py engine --expect-digest <the agent's full> --expect-digest0 <...> --tested-sha <the agent's sha>
        [--suite-report <its checkout>/gates/<sha12>-suite.json]
    python tools/gate.py search-core --baseline-root ../other-checkout
    python tools/gate.py optin-core --baseline-root ../other-checkout
    python tools/gate.py repin --expect-digest <main's recorded digest> --spec delver --preset delver-mirror
    python tools/gate.py <tier> --dry-run      # print the plan (or the refusal); run nothing

Exit status: 0 passed ("GATE PASSED ...", or "GATE PASSED WITH SKIPS / WITH FLAKY RETRIES ..."
naming every opted-out step and every flaky retry), 1 failed ("GATE FAILED ..."), 2 refused (a
setup problem or a missing essential input; nothing ran).

Writes `gates/<sha12>-<tier>.json` - one per commit AND tier, so a docs pass never overwrites an
engine report (ENGINE.md 143) - (sha, zero-core pin, tier, options, the final line, each step's status -
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
import shlex
import shutil
import subprocess
import sys
import threading
import time
import uuid
from datetime import datetime, timezone

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import proctree  # noqa: E402  (tools/proctree.py: a timeout that bounds the whole process tree)
import local_config  # noqa: E402  (tools/local_config.py: the machine's own settings, ~/.config/zero/local.env)


def _load_checkout():
    """mtgz/checkout.py by its path (standard library only): `import mtgz` needs the zero-core submodule, which the
    gate checks before anything initialises it."""
    import importlib.util
    spec = importlib.util.spec_from_file_location('_mtgz_checkout', os.path.join(ROOT, 'mtgz', 'checkout.py'))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


checkout = _load_checkout()
CORE = os.path.join(ROOT, 'third_party', 'zero-core')
GATES_DIR = os.path.join(ROOT, 'gates')
WIN = sys.platform == 'win32'
# a Windows machine's big scratch disk for build dirs and smoke runs (ZERO_BIG_DISK, e.g. E: on a desktop whose C: is
# small: the machine's own setting in ~/.config/zero/local.env); '' = this checkout's target/ and gates/tmp
BIG_DISK = (local_config.get('ZERO_BIG_DISK', '') if WIN else '').rstrip('/\\')
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


FORMATS = ('constructed', 'eoe', 'driver')     # engine/tests/digest_formats.rs's three digests (ENGINE.md 148.1)
FORMATS_RE = re.compile(r'^(constructed|eoe|driver): .*; combined digest ([0-9a-f]{16})\s*$', re.M)
FORMATS_ARGV = ['cargo', 'test', '--release', '-p', 'engine', '--test', 'digest_formats', '--', '--ignored',
                '--nocapture']


def check_digest_formats(output, rc):
    """digest_formats' positive evidence (ENGINE.md 148.1, 157): exactly three "<name>: ... combined digest <16 hex>"
    lines, one each of constructed, eoe and driver (each test compares its lines with the checkout's own
    tests/data/digests_<name>.txt and fails on a difference), no "wrote ...digests_" line (DIGEST_WRITE re-records
    instead of comparing), exactly one "test result: ok. 3 passed" line + exit 0."""
    ms = FORMATS_RE.findall(output)
    seen = {}
    for name, d in ms:
        seen.setdefault(name, []).append(d)
    extra = {'formats': {n: v[0] for n, v in seen.items() if len(v) == 1}}
    if len(ms) != len(FORMATS) or sorted(seen) != sorted(FORMATS):
        return False, (f'{len(ms)} "<name>: ... combined digest <16 hex>" line(s) ({", ".join(n for n, _ in ms) or "none"}): '
                       f'expected exactly one each of {", ".join(FORMATS)} (did it compile/run?)'), extra
    if re.search(r'^wrote .*digests_', output, re.M):
        return False, 'the digest_formats tests RE-RECORDED tests/data/digests_*.txt instead of comparing', extra
    ok, why, counts = make_cargo_checker(single=True)(output, rc)
    extra.update(counts)
    if not ok:
        return False, f'the digest_formats tests did not pass: {why}', extra
    if counts['passed'] != len(FORMATS):
        return False, f'{counts["passed"]} passed, not the {len(FORMATS)} digests', extra
    return True, (', '.join(f'{n} {extra["formats"][n]}' for n in FORMATS) + ' (each = the checkout\'s recorded '
                  'tests/data/digests_<name>.txt)'), extra


DEALT_RE = re.compile(r'^dealt_digests: (\d+) matches in [\d.]+ s; combined digest ([0-9a-f]{16})\s*$', re.M)
DEALT_ARGV = ['cargo', 'test', '--release', '-p', 'engine', '--test', 'dealt_decks', '--', '--ignored', '--nocapture',
              'dealt_digests']


def check_digest_dealt(output, rc):
    """dealt_decks.rs::dealt_digests' positive evidence (ENGINE.md 154, 157): exactly one "dealt_digests: N matches in
    ... s; combined digest <16 hex>" line (the test compares its lines with the checkout's own
    tests/data/digests_dealt.txt and fails on a difference), no "wrote ...digests_dealt" line (DIGEST_WRITE re-records
    instead of comparing), exactly one "test result: ok. 1 passed" line + exit 0."""
    ms = DEALT_RE.findall(output)
    extra = {'formats': {'dealt': ms[0][1]}} if len(ms) == 1 else {}
    if len(ms) != 1:
        return False, (f'{len(ms)} "dealt_digests: N matches in ... s; combined digest <16 hex>" line(s): expected exactly '
                       f'one (did it compile/run?)'), extra
    if re.search(r'^wrote .*digests_dealt', output, re.M):
        return False, 'dealt_digests RE-RECORDED tests/data/digests_dealt.txt instead of comparing', extra
    ok, why, counts = make_cargo_checker(single=True)(output, rc)
    extra.update(counts)
    if not ok:
        return False, f'dealt_digests did not pass: {why}', extra
    if counts['passed'] != 1:
        return False, f'{counts["passed"]} passed, not the one dealt digest', extra
    return True, f'dealt {ms[0][1]} over {ms[0][0]} matches (= the checkout\'s recorded tests/data/digests_dealt.txt)', extra


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


METRICS_PREFIX = '[metrics] '          # a run-loop smoke on the Mac prints its metrics.jsonl with this prefix


def make_metrics_checker(run_dir, want_games=None, want_iters=1):
    """A run-loop smoke's real positive evidence: metrics.jsonl itself (not log text) - every
    iteration present, the last one's self-play counts present, no unfinished games, more than 0
    games (exactly `want_games` for a known preset). `run_dir` None: the file's lines as the Mac printed them after
    the run, each prefixed METRICS_PREFIX (run_loop_step's Mac command; ENGINE.md 166)."""
    def checker(output, rc):
        if rc != 0:
            return False, f'exit {rc}', {}
        if run_dir is None:
            lines = [ln[len(METRICS_PREFIX):] for ln in output.splitlines() if ln.startswith(METRICS_PREFIX)]
            if not lines:
                return False, f'no "{METRICS_PREFIX}..." lines (the Mac printed no metrics.jsonl)', {}
            rows = [json.loads(line) for line in lines if line.strip()]
        else:
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


WORKER_EXE = 'mtgz-worker.exe' if WIN else 'mtgz-worker'
FARM_COMPAT_RE = re.compile(r'^farm compat: (PASSED|FAILED) - (.*)$', re.M)
CARGO_FINISHED_RE = re.compile(r'^\s*Finished `release` profile', re.M)


CACHE_HIT_RE = re.compile(r'^artifact cache: hit (\S+) ([0-9a-f]{16}) -> (.+?) \(sha256 ([0-9a-f]{16})', re.M)


def make_worker_build_checker(exe):
    """The release worker (`tools/zcache.py worker`: fetched from the artifact cache by this commit's Rust input hash,
    else `cargo build --release -p mtgz_worker` and stored - ENGINE.md 167): cargo's "Finished `release` profile" line or
    the cache's "artifact cache: hit worker <key> -> <exe> (sha256 ...)" line naming `exe` with the sha256 it has, the
    executable at `exe` + exit 0 (its sha256 recorded, and whether it was built or fetched)."""
    def checker(output, rc):
        if rc != 0:
            return False, f'exit {rc}', {}
        hit = [m for m in CACHE_HIT_RE.findall(output) if m[0] == 'worker']
        if not hit and not CARGO_FINISHED_RE.search(output):
            return False, 'no "Finished `release` profile" line', {}
        if not os.path.isfile(exe):
            return False, f'no {exe} after the build', {}
        digest = sha256_file(exe)
        if hit:
            _, key, dest, want = hit[-1]
            if norm(dest) != norm(exe) or not digest.startswith(want):
                return False, f'the cache placed {dest} (sha256 {want}), but {exe} has sha256 {digest[:16]}', {}
            return True, f'{exe} from the artifact cache (key {key})', {'worker': exe, 'worker_sha256': digest,
                                                                        'worker_cache': f'hit {key}'}
        return True, f'{exe} built', {'worker': exe, 'worker_sha256': digest, 'worker_cache': 'built'}
    return checker


def check_test_build(output, rc):
    """The test binaries' build (`cargo test --release -p engine --no-run --test ...`): cargo's "Finished `release`
    profile" line + exit 0 (the steps after it run them without building)."""
    if rc != 0:
        return False, f'exit {rc}: ' + last_line(output), {}
    if not CARGO_FINISHED_RE.search(output):
        return False, 'no "Finished `release` profile" line', {}
    return True, 'test binaries built', {}


FUZZ_SHARDS_RE = re.compile(r'^fuzz shards: (PASSED|FAILED) - (.*)$', re.M)


def make_fuzz_shards_checker(games, pool, hidden):
    """tools/fuzz_shards.py's positive evidence (ENGINE.md 167): exactly one "fuzz shards: PASSED - ..." line and its
    JSON summary {"fuzz_shards": {"passed": true, ...}} for this pool with exactly `games` games, the merged end-of-run
    checks run ("checked" names the games, and the matches unless the pool plays none), + exit 0."""
    def checker(output, rc):
        ms = FUZZ_SHARDS_RE.findall(output)
        summ = [s['fuzz_shards'] for s in json_lines(output, 'fuzz_shards') if isinstance(s.get('fuzz_shards'), dict)]
        if len(ms) != 1 or len(summ) != 1:
            return False, f'{len(ms)} "fuzz shards:" line(s), {len(summ)} summary line(s) (expected exactly 1 each)', {}
        verdict, text = ms[0]
        s = summ[0]
        extra = {'fuzz_games': s.get('games'), 'fuzz_shards': {k: s.get(k) for k in (
            'pool', 'hidden', 'games', 'matches', 'post', 'shards', 'checked', 'rest_passed', 'seconds', 'slowest_shard_s',
            'matches_skipped', 'logs')}}
        if verdict != 'PASSED' or not s.get('passed'):
            return False, f'fuzz shards {verdict}: {text[:250]}', extra
        if s.get('games') != games or s.get('pool') != pool or bool(s.get('hidden')) != hidden:
            return False, (f'the shards fuzzed {s.get("games")} games of {s.get("pool")}{" (hidden)" if s.get("hidden") else ""}, '
                           f'not {games} of {pool}{" (hidden)" if hidden else ""}'), extra
        kinds = {c.split()[0] for c in s.get('checked') or []}
        if 'games' not in kinds or (not hidden and not s.get('matches_skipped') and 'matches' not in kinds):
            return False, f'the merged checks covered {sorted(kinds)}', extra
        if rc != 0:
            return False, f'"fuzz shards: PASSED" but exit {rc}', extra
        return True, text[:250], extra
    return checker


def check_farm_compat(output, rc):
    """tools/farm_compat.py's positive evidence (ENGINE.md 155): exactly one "farm compat: PASSED - ..." line (both
    gates identical with rows > 0, every game's decks compared: the tool's own checks), its JSON summary with
    "passed": true + exit 0."""
    ms = FARM_COMPAT_RE.findall(output)
    if len(ms) != 1:
        return False, f'{len(ms)} "farm compat:" lines (expected exactly 1)', {}
    verdict, text = ms[0]
    summary = next(iter(json_lines(output, 'gate2', 'gate3', 'passed')), None)
    extra = {'farm_compat': {k: (summary or {}).get(k) for k in ('preset', 'games')}}
    if summary:
        for g in ('gate2', 'gate3'):
            r = (summary.get(g) or {}).get('result') or {}
            extra['farm_compat'][g] = {k: r.get(k) for k in ('games', 'rows', 'batches_compared', 'setups', 'identical')
                                       if k in r}
    if verdict != 'PASSED' or not (summary and summary.get('passed')):
        return False, f'farm compat {verdict}: {text[:250]}', extra
    if rc != 0:
        return False, f'"farm compat: PASSED" but exit {rc}', extra
    return True, text[:250], extra


MAC_LINE_RE = re.compile(r'^\[mac\] (\S+) (\S+) @ ([0-9a-f]{40}): ', re.M)
MAC_DONE_RE = re.compile(r'^\[mac\] done @ ([0-9a-f]{40}) \(exit (\d+)\)\s*$', re.M)


def mac_commit_problem(output, sha):
    """None when mac.sh's "[mac] <host> <branch> @ <sha>: <cmd>" line - the Mac checkout's own HEAD, read there
    after the checkout - is this commit, and its "[mac] done @ <sha> (exit N)" line - HEAD read again after the
    command - is too; else why not: the positive evidence that a Mac step reached the Mac and ran the gated commit
    from start to end (the line used to echo the sha the desktop sent, whatever the Mac had checked out, and
    another mac.sh on the same branch could switch the checkout under a running command: ENGINE.md 143)."""
    m = MAC_LINE_RE.search(output)
    if not m:
        return 'no "[mac] <host> <branch> @ <the Mac\'s HEAD>:" line (mac.sh never reached the Mac?)'
    if m.group(3) != sha:
        return f'the Mac\'s checkout was at {m.group(3)[:12]}, not {sha[:12]}'
    d = MAC_DONE_RE.findall(output)
    if not d:
        return 'no "[mac] done @ <the Mac\'s HEAD> (exit N)" line: the Mac\'s checkout after the command is unknown'
    if d[-1][0] != sha:
        return f'the Mac\'s checkout moved to {d[-1][0][:12]} during the command (not {sha[:12]})'
    return None


def make_mac_checker(sha, inner):
    def checker(output, rc):
        why = mac_commit_problem(output, sha)
        return (False, why, {}) if why else inner(output, rc)
    return checker


def make_eval_checkout_checker(sha):
    """repin's `mac.sh --eval-checkout`: the "[mac] ... @ <sha>" / "[mac] done @ <sha>" lines, the Mac's build line
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
    verify: object = None                      # callable() -> (passed, reason, extra, log text): a check that runs
                                               # no command (another gate run's report as evidence)
    # placement (ENGINE.md 166): a heavy step takes a slot of tools/slotd.sh when the gate throttles; a step with
    # `mac_cmd` (a shell command for the Mac checkout of this commit) runs there through tools/mac.sh when the gate
    # offloads, `mac_checker` (default: `checker`) reading its output, `desktop_part` (a Step) running on the desktop
    # beside it for what only the desktop can check (the Windows worker, private data, platform-bound hashes)
    heavy: bool = False
    mac_cmd: object = None                     # True: the same argv / env / cwd; callable(ctx) -> (argv, env, cwd)
    mac_checker: object = None
    desktop_part: object = None
    mac_flaky: object = None                   # callable(node id, k) -> (argv, env, cwd): a flaky retry on the Mac
    placement: str = 'desktop'                 # desktop | desktop, throttled | mac | desktop (the Mac unreachable)
    local: object = None                       # an offloaded step's desktop version (the fallback)

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
    placement: str = 'desktop'

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
    command that reads it gets EOF instead of hanging or reading ours). The timeout bounds the command AND
    its descendants (tools/proctree.py: subprocess.run's killed only bash, then waited for the grandchild that
    held the pipe - ENGINE.md 143); a descendant still holding the output after the command exited is killed at
    the deadline. Module-level so tests stub it."""
    out, _, rc = proctree.run(argv, cwd=cwd, env=env, timeout=timeout, creationflags=child_priority() if WIN else 0)
    if rc == 'timeout':
        out += f'\n[gate.py: killed after {timeout}s timeout, with every process it started]\n'
    return out, rc


SCRUBBED_ENV = ('PYTHONPATH', 'PYTEST_ADDOPTS', 'ZERO_CORE_UNPINNED')    # and every DIGEST_* and FUZZ_*


def step_env(ctx, step):
    """The step's environment: the caller's, the gate's base (CPU only, the target dir, ...) and the
    step's own (None = unset). Never the caller's DIGEST_* (DIGEST_WRITE re-records digests.txt
    instead of comparing; DIGEST_RANDOM / _HEURISTIC / _ENCODING / _IDS change the digest), FUZZ_*
    (FUZZ_GAMES / _FIRST / _SKIP / _POOL change what the fuzz plays), PYTEST_ADDOPTS (can deselect
    tests), PYTHONPATH (can import another checkout) or ZERO_CORE_UNPINNED (builds a core that is not
    the pin): each step states what it wants."""
    env = dict(os.environ)
    env.update(ctx.base_env)
    for k in list(env):
        if k.upper().startswith(('DIGEST_', 'FUZZ_')) or k.upper() in SCRUBBED_ENV:
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
        rout, rrc = execute(argv, step.cwd or ctx.root, env, step_timeout(ctx, step))
        log_parts.append(f'\n\n$ {show(argv)}\n[gate.py: flaky retry {k}/{len(ids)}: {nid} alone]\n\n{rout}')
        rok, rreason, _ = check_pytest(rout, rrc) if rrc != 'timeout' else (False, 'timed out', {})
        retries.append({'test': nid, 'passed': rok, 'reason': rreason})
    extra = dict(extra, retries=retries)
    again = [r['test'] for r in retries if not r['passed']]
    if again:
        return False, f'{reason}; rerun alone, failed again: {", ".join(again)}', extra
    extra['flaky'] = ids
    return True, f'{reason} in the full run; flaky, passed on retry alone: {", ".join(ids)}', extra


def step_timeout(ctx, step):
    """A throttled step's timeout also covers its wait for a slot (tools/slotd.sh gives up after ctx.slot_wait)."""
    return ctx.step_timeout + (ctx.slot_wait + 60 if step.placement == THROTTLED else 0)


def run_step(ctx, step):
    cwd = step.cwd or ctx.root
    if step.not_needed:
        return StepResult(step.name, step.id, step.argv, cwd, 0.0, None, None, None, 'not needed',
                          f'not needed: {step.not_needed}', {}, mac=step.mac, placement=step.placement)
    if step.skip_reason:
        return StepResult(step.name, step.id, step.argv, cwd, 0.0, None, None, None, 'skipped',
                          f'skipped: {step.skip_reason}', {}, mac=step.mac, placement=step.placement)
    log_path = os.path.join(ctx.logs_dir, f'{ctx.sha_short}-{ctx.tier}-{slug(step.name)}.log')
    if step.verify:
        t0 = time.time()
        passed, reason, extra, text = step.verify()
        with open(log_path, 'w', encoding='utf-8', errors='replace') as f:
            f.write(text)
        return StepResult(step.name, step.id, step.argv, cwd, time.time() - t0, 0, log_path, sha256_file(log_path),
                          'passed' if passed else 'failed', reason, extra, mac=step.mac, placement=step.placement)
    env = step_env(ctx, step)
    if step.prepare:
        step.prepare()
    t0 = time.time()
    out, rc = execute(step.argv, cwd, env, step_timeout(ctx, step))
    log_parts = [f'$ {show(step.argv)}\ncwd: {cwd}\nenv overrides: {step.env or {}}\nplacement: {step.placement}\n\n',
                 out]
    if step.placement == 'mac' and step.local is not None and mac_never_reached(out):
        # the Mac went away between the plan and this step: the desktop runs it (throttled), the log keeps both
        local = step.local
        log_parts.append(f'\n\n[gate.py: the Mac was not reached ({mac_never_reached(out)}): running the step on the '
                         f'desktop instead]\n\n$ {show(local.argv)}\n\n')
        step = local
        env = step_env(ctx, step)
        out, rc = execute(step.argv, cwd, env, step_timeout(ctx, step))
        log_parts.append(out)
    if rc == 'timeout':
        passed, reason, extra = False, f'timed out after {step_timeout(ctx, step)}s', {}
    else:
        passed, reason, extra = step.checker(out, rc)
        if not passed and step.flaky:
            passed, reason, extra = retry_flaky(ctx, step, env, out, reason, extra, log_parts)
        if passed:
            fenced = fenced_tests(out)
            if fenced:
                extra = dict(extra, fenced=fenced)
                reason = f'{reason}; {fenced} live-run-hazard test(s) fenced off (a live run on this machine)'
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
                      reason, extra, mac=step.mac, placement=step.placement)


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
    mac = ' [mac]' if r.mac else (' [throttled]' if r.placement == THROTTLED else '')
    print(f'{TAGS[r.status]:8s}{mac} {r.name} ({r.seconds:.1f}s): {r.reason}', flush=True)


# ---------------------------------------------------------------------------- the privacy check

PRIVACY_RE = re.compile(r'^privacy check: (\d+) file\(s\), (\d+) finding\(s\)', re.M)


def privacy_scan(root):
    """tools/privacy_check.py on the checkout's tracked files (AGENTS.md "Private details": no farm machines, host
    names, tailnet addresses, the Mac's ssh target or home paths with a user name; the machine's own patterns from
    ~/.config/zero/privacy_patterns.txt) -> (ok, reason). Every tier runs it first, in-process and quick; a finding
    fails the gate before any step runs. Module-level: tests stub it."""
    try:
        p = subprocess.run([python_exe(), os.path.join(root, 'tools', 'privacy_check.py'), '--root', root],
                           capture_output=True, text=True, timeout=600, stdin=subprocess.DEVNULL)
    except (OSError, subprocess.TimeoutExpired) as e:
        return False, f'tools/privacy_check.py did not run: {type(e).__name__}: {e}'
    m = PRIVACY_RE.search(p.stdout)
    if p.returncode == 0 and m and m.group(2) == '0':
        return True, m.group(0)
    shown = [x for x in p.stdout.splitlines() if x.strip()][:12]
    return False, (f'exit {p.returncode}: ' + ' | '.join(shown) + (p.stderr.strip()[-300:] if not shown else '') +
                   ' (fix the file: AGENTS.md "Private details")')


def privacy_result(ctx):
    t0 = time.time()
    ok, why = privacy_scan(ctx.root)
    return StepResult('no private details in the tracked files (tools/privacy_check.py)', 'privacy', [], ctx.root,
                      time.time() - t0, 0 if ok else 1, None, None, 'passed' if ok else 'failed', why, {})


# ---------------------------------------------------------------------------- the final line

def caveats(results, dirty):
    """-> (skipped, recorded, flaky) descriptions: everything a pass did NOT fully check."""
    skipped = [f'{r.id} ({r.reason[len("skipped: "):]})' for r in results if r.status == 'skipped']
    if dirty:
        skipped.insert(0, f'clean-tree (--allow-dirty: {len(dirty)} uncommitted change(s) gated, not the commit)')
    recorded = [f'{r.id} = {r.extra.get("digest")} (--record-digest)' for r in results if r.status == 'recorded']
    flaky = [f'{r.id}: {", ".join(r.extra["flaky"])}' for r in results if r.extra.get('flaky')]
    # tests the live-run fence skipped ran nowhere (a pass on the Mac runs them there: ENGINE.md 166.3)
    skipped += [f'{r.id}: {r.extra["fenced"]} live-run-hazard test(s) (fenced: a live run on this machine)'
                for r in results if r.extra.get('fenced') and r.status != 'skipped']
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
    if ctx.placement:
        print(f'  placement: {ctx.placement["summary"]}')
    for batch, _ in batches(steps):
        prefix = '  (parallel) ' if len(batch) > 1 else '  '
        pad = ' ' * len(prefix)
        for s in batch:
            mac = ' [mac, via tools/mac.sh]' if s.mac else (' [throttled: tools/slotd.sh]' if s.placement == THROTTLED
                                                            else '')
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


def untracked_files():
    """The untracked, not ignored files where a build, a test or a run picks them up (mtgz/checkout.py: under
    engine/, mtgz/, tests/, tools/, data/, ..., the root's build / test configuration, and inside the zero-core
    submodule) as '?? <path>' lines: an untracked tests/test_*.py or engine/tests/*.rs is collected or compiled by
    the gate's steps and is in no commit (ENGINE.md 143)."""
    try:
        return [f'?? {p}' for p in checkout.untracked_code(ROOT)]
    except subprocess.CalledProcessError as e:
        raise GateError(f'git ls-files --others ({ROOT}): {(e.stderr or "").strip()}') from None


def tree_state():
    return tracked_changes() + untracked_files()


def check_tree(args):
    dirty = tree_state()
    if dirty and not args.allow_dirty:
        shown = ', '.join(line.strip() for line in dirty[:8]) + (' ...' if len(dirty) > 8 else '')
        raise GateError(f'the checkout is not exactly its commit ({shown}): the gate would check uncommitted changes '
                        f'or untracked files under the commit\'s name. Commit them (a WIP commit is fine; a submodule '
                        f'not at the pin: `git submodule update third_party/zero-core`), remove the untracked files, '
                        f'or --allow-dirty (named in the final line).')
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
            f'{BIG_DISK}/cargo-target/mtg-zero' if BIG_DISK else os.path.join(ROOT, 'target'),
            os.path.basename(ROOT) if os.path.basename(ROOT) != 'mtg-zero' else 'gate')
        self.target_dir = os.path.abspath(args.target_dir or default_target)
        self.logs_dir = os.path.join(GATES_DIR, 'logs')
        self.base_env = {
            'CUDA_VISIBLE_DEVICES': '-1',              # an empty string does NOT hide the GPU on Windows
            'CARGO_TARGET_DIR': self.target_dir,
            'CARGO_BUILD_JOBS': str(self.jobs),
            'PYTHONIOENCODING': 'utf-8',
            'RUST_BACKTRACE': '1',
            # (the worker tests and tools/farm_compat.py run the worker the gate builds: worker_build_step)
            'MTGZ_WORKER': os.path.join(self.target_dir, 'release', WORKER_EXE),
        }
        self.no_mac = args.no_mac
        self.dirty = []
        self.baseline = None
        self.slot_wait = args.slot_wait
        self.throttle = False
        self.placement = None
        self.pytest_shards = max(1, args.pytest_shards)          # (ENGINE.md 167)


def python_exe():
    return sys.executable


def mac_step(ctx, name, remote_cmd, inner, step_id, evidence):
    """A Mac part of a tier: `bash tools/mac.sh '<remote_cmd>'`, which pushes HEAD and runs it on the
    checkout's exact commit there (ENGINE.md 44: bit-identical results). Its checker wants mac.sh's
    "[mac] ... @ <this sha>" and "[mac] done @ <this sha>" lines (the Mac checkout's HEAD before and after the
    command: mac_commit_problem) before `inner`'s own positive evidence."""
    return Step(name, ['bash', 'tools/mac.sh', remote_cmd], id=step_id, mac=True,
                checker=make_mac_checker(ctx.sha, inner),
                evidence='mac.sh\'s "[mac] <host> <branch> @ <this sha>" and "[mac] done @ <this sha>" lines, then '
                         + evidence)


def need_core():
    if not os.path.exists(os.path.join(CORE, '.git')):
        raise GateError(f'{CORE} is not initialised: `git submodule update --init third_party/zero-core` (or bash '
                        f'tools/build_py.sh) first')


def opted_out(args, step_id):
    return step_id in (args.allow_skip or [])


# ---------------------------------------------------------------------------- placement (ENGINE.md 166)
# Beside a live run the gate keeps its heavy work off the desktop: the steps with a Mac command run on the Mac
# (tools/mac.sh, this commit's checkout there, `nice`, a Mac-side slot of tools/slotd.sh), the desktop's own heavy
# steps (its builds, the Windows worker, farm compat, what needs private data) take a desktop slot of tools/slotd.sh.
# A step's evidence is the same wherever it ran (the Mac's engine is bit-identical: ENGINE.md 44.2, 166.2). zero-core's
# own cargo tests and pytest stay on the desktop: its search goldens hash differently on the Mac (14 driver tests
# failed there at be9ebfa: platform floating point in the search), so they are valid only here (ENGINE.md 166.2).

THROTTLED = 'desktop, throttled'
# the Mac's ssh target: the machine's own setting, as tools/mac.sh reads it ('' = this machine has no Mac)
MAC_HOST = local_config.get('MTGZ_MAC', '')
MAC_NICE = 10                       # below a live run's own Mac work (its evaluation and self-play run at 0)
MAC_JOBS = 6                        # CARGO_BUILD_JOBS on the Mac (mac.sh's own 12 is for an idle Mac)
MAC_MIN_FREE_GB = 8                 # the Mac's available memory below this: the desktop runs the steps (throttled)
# the tests the Mac cannot check like the desktop, as pytest arguments (files or node ids), run on the desktop beside
# the Mac's pytest (ENGINE.md 166.2: the Mac ran the rest of 72e5a0df's suite with the desktop's outcomes)
PYTEST_DESKTOP_ONLY = (
    'tests/test_worker_farm.py', 'tests/test_worker_records.py', 'tests/test_worker_shards.py',
    'tests/test_farm_campaign.py',          # the Windows release worker this gate built (MTGZ_WORKER)
    'tests/test_deck_scorer.py',            # fixtures hashed bit for bit (platform floating point) + private data on E:
    'tests/test_position_lab.py',           # main-line replays on E:
    'tests/test_drafted.py::test_human_pick_agreement_on_a_few_drafts',     # private data on E:
    'tests/test_evaluator.py::test_the_export_and_its_reference_scorer',    # private data on E:
    'tests/test_sealedai_publish.py::test_dry_run_command_line',            # psutil's process access on macOS
    # a supervisor test that fails on macOS (the companion started 8 times: ENGINE.md 166.6), so it runs on the
    # desktop - fenced while a run is live here, and then named in the final line as a skip
    'tests/test_run_publisher.py::test_the_supervisor_starts_one_publisher_per_run_and_it_ends_finished',
)


def live_runs():
    """The live supervisors on this machine (zero.runctl.live_supervisors: any game's run) -> [{pid, run}].
    Module-level: tests stub it."""
    need_core()
    core_py = os.path.join(CORE, 'python')
    if core_py not in sys.path:
        sys.path.insert(0, core_py)
    from zero import runctl
    return [{'pid': s['pid'], 'run': s['run']} for s in runctl.live_supervisors()]


MAC_PROBE = ('sysctl -n vm.loadavg; vm_stat | awk \'/page size of/{ps=$8} /Pages (free|inactive|speculative)/'
             '{gsub("[.]","",$NF); n+=$NF} END{printf "%.1f\\n", n*ps/1073741824}\'')


def mac_probe(host=MAC_HOST, timeout=20):
    """-> (ok, what): the Mac answers over ssh, with its load and available memory; not ok when it does not answer
    or has less than MAC_MIN_FREE_GB available (the desktop then runs the steps, throttled). Module-level: tests
    stub it."""
    if not host:
        return False, 'no Mac configured (MTGZ_MAC in ~/.config/zero/local.env: tools/local.env.example)'
    try:
        p = subprocess.run(['ssh', '-o', 'BatchMode=yes', '-o', f'ConnectTimeout={min(timeout, 10)}', host, MAC_PROBE],
                           capture_output=True, text=True, timeout=timeout, stdin=subprocess.DEVNULL)
    except (OSError, subprocess.TimeoutExpired) as e:
        return False, f'the Mac ({host}) did not answer: {type(e).__name__}'
    lines = [x.strip() for x in p.stdout.splitlines() if x.strip()]
    if p.returncode != 0 or len(lines) < 2:
        return False, f'the Mac ({host}) did not answer (ssh exit {p.returncode}: {p.stderr.strip()[-200:]})'
    load, free = lines[0].strip('{} '), lines[-1]
    try:
        free_gb = float(free)
    except ValueError:
        return False, f'the Mac ({host}) answered without its memory ({free!r})'
    if free_gb < MAC_MIN_FREE_GB:
        return False, f'the Mac has {free_gb:.1f} GB available (< {MAC_MIN_FREE_GB}), load {load}'
    return True, f'the Mac {host}: load {load}, {free_gb:.1f} GB available'


def mac_never_reached(output):
    """Why an offloaded step's output says mac.sh never ran the command on the Mac (None: it did) - the desktop then
    runs the step itself (Step.local)."""
    if MAC_LINE_RE.search(output):
        return None
    tail = [x for x in output.strip().splitlines() if x.strip()][-1:] or ['(no output)']
    return f'no "[mac] ... @ <sha>:" line; {tail[0][:200]}'


FENCED_RE = re.compile(r'^SKIPPED \[(\d+)\] .*live-run fence:', re.M)


def fenced_tests(output):
    """How many tests pytest's -rs lines say the live-run fence skipped (zero.runctl.live_run_fence_reason)."""
    return sum(int(n) for n in FENCED_RE.findall(output))


# every DIGEST_* / FUZZ_* the Mac's login environment might carry is dropped first (step_env's rule)
MAC_SCRUB = 'for v in $(env | sed -n -e "s/^\\(DIGEST_[A-Za-z0-9_]*\\)=.*/\\1/p" -e "s/^\\(FUZZ_[A-Za-z0-9_]*\\)=.*/\\1/p"); ' \
            'do unset "$v"; done; '


def remote_command(argv, env=None, cwd=None):
    """The shell command the Mac runs for a step's `argv` / `env` (None = unset) / `cwd`, in the Mac checkout of this
    commit (mac.sh): the gate's environment rules (the caller's DIGEST_*, FUZZ_*, PYTEST_ADDOPTS, PYTHONPATH,
    ZERO_CORE_UNPINNED never reach it; CPU only), then a Mac-side slot (the checkout's own tools/slotd.sh, state
    ~/.mtgz-state) and `nice`, the checkout's `cwd`-relative directory."""
    argv = list(argv)
    sets = {'CUDA_VISIBLE_DEVICES': '-1', 'PYTHONIOENCODING': 'utf-8', 'RUST_BACKTRACE': '1',
            'CARGO_BUILD_JOBS': str(MAC_JOBS)}
    unset = list(SCRUBBED_ENV)
    for k, v in (env or {}).items():
        if v is None:
            unset.append(k)
        else:
            sets[k] = v
    if argv and argv[0] == python_exe():
        argv[0] = 'python'
    inner = argv[2] if len(argv) == 3 and argv[:2] == ['bash', '-c'] else shlex.join(argv)
    rel = os.path.relpath(cwd, ROOT).replace(os.sep, '/') if cwd else '.'
    if rel != '.':
        inner = f'cd {shlex.quote(rel)} && {inner}'
    envs = ' '.join([f'-u {k}' for k in unset] + [f'{k}={shlex.quote(v)}' for k, v in sets.items()])
    return f'{MAC_SCRUB}env {envs} bash tools/slotd.sh nice -n {MAC_NICE} bash -c {shlex.quote(inner)}'


def offload(ctx, step):
    """The Mac version of an offloadable step: its `mac_cmd` (a string, or a callable(ctx) -> (argv, env, cwd)
    that remote_command wraps) through tools/mac.sh, the checker behind mac.sh's "[mac] ... @ <this sha>" lines,
    the desktop version kept as the fallback (Step.local)."""
    local = throttled(ctx, step) if ctx.throttle else dataclasses.replace(step, placement='desktop')
    cmd = step.mac_cmd
    if cmd is True:
        cmd = remote_command(step.argv, step.env, step.cwd)
    elif callable(cmd):
        cmd = remote_command(*cmd(ctx))
    mac = dataclasses.replace(
        step, argv=['bash', 'tools/mac.sh', cmd], env=None, cwd=None, mac=True, placement='mac', local=local,
        checker=make_mac_checker(ctx.sha, step.mac_checker or step.checker),
        evidence='mac.sh\'s "[mac] <host> <branch> @ <this sha>" and "[mac] done @ <this sha>" lines, then ' +
                 step.evidence)
    if step.flaky:
        inner = step.mac_flaky or (lambda nid, k: (step.flaky.argv(nid, k), step.env, step.cwd))
        mac.flaky = FlakyRetry(step.flaky.file, lambda nid, k: ['bash', 'tools/mac.sh', remote_command(*inner(nid, k))])
    return mac


def throttled(ctx, s):
    """`s` holding a desktop slot of tools/slotd.sh (its timeout grows by the slot wait: step_timeout)."""
    s = dataclasses.replace(s, placement=THROTTLED)
    if s.argv[:2] != ['bash', 'tools/slotd.sh']:
        s.argv = ['bash', os.path.join(ROOT, 'tools', 'slotd.sh'), *s.argv] if s.cwd else ['bash', 'tools/slotd.sh',
                                                                                           *s.argv]
    s.env = dict(s.env or {}, MTGZ_SLOT_WAIT_MAX=str(int(ctx.slot_wait)))
    if s.flaky:
        inner, wrap = s.flaky.argv, s.argv[:2]
        s.flaky = FlakyRetry(s.flaky.file, lambda nid, k: [*wrap, *inner(nid, k)])
    return s


def place(ctx, args, steps):
    """Decide where each step runs (ENGINE.md 166) -> the steps to run. --offload auto (the default): the steps with
    a Mac command go to the Mac when a run is live on this machine, the Mac answers with memory to spare and --no-mac
    is not given, else they stay on the desktop; --throttle auto: the desktop's heavy steps take a tools/slotd.sh slot
    while a run is live."""
    live = None

    def is_live():
        nonlocal live
        if live is None:
            live = live_runs()
        return live
    throttle = {'on': True, 'off': False}.get(args.throttle)
    want_mac = {'mac': True, 'local': False}.get(args.offload)
    why = []
    if args.no_mac and want_mac is not False:
        want_mac = False
        why.append('--no-mac: nothing on the Mac')
    active = [s for s in steps if not s.skip_reason and not s.not_needed]
    candidates = [s for s in active if s.mac_cmd]
    heavy = [s for s in active if s.heavy or s.mac_cmd]
    if want_mac is None and candidates:
        want_mac = bool(is_live())
        why.append(f'{len(live)} live supervisor(s) here ({", ".join(sorted({x["run"] for x in live}))[:160]})' if live
                   else 'no live run here: the desktop runs everything')
    if throttle is None:
        throttle = bool(heavy) and bool(is_live())
        if live and not any('live supervisor' in w for w in why):
            why.append(f'{len(live)} live supervisor(s) here ({", ".join(sorted({x["run"] for x in live}))[:160]}): '
                       f'the heavy steps throttled')
    mac_ok, mac_what = False, ''
    if want_mac and candidates:
        mac_ok, mac_what = mac_probe()
        if not mac_ok and args.offload == 'mac':
            raise GateError(f'--offload mac: {mac_what}')
        why.append(mac_what + ('' if mac_ok else ': the desktop runs those steps'))
    ctx.throttle = throttle
    out = []
    for s in steps:
        if s.mac_cmd and mac_ok and not s.skip_reason and not s.not_needed:
            m = offload(ctx, s)
            out.append(m)
            if s.desktop_part is not None:          # (beside the Mac step: a group runs concurrently)
                d = throttled(ctx, s.desktop_part) if throttle else s.desktop_part
                m.group = d.group = m.group or f'beside-{m.id}'
                out.append(d)
        elif (s.heavy or s.mac_cmd) and throttle and not s.skip_reason and not s.not_needed:
            out.append(throttled(ctx, s))
        else:
            out.append(s)
    n_mac = sum(1 for s in out if s.placement == 'mac')
    n_thr = sum(1 for s in out if s.placement == THROTTLED)
    summary = (f'{n_mac} step(s) on the Mac, {n_thr} on the desktop throttled (tools/slotd.sh), '
               f'{len(out) - n_mac - n_thr} on the desktop' + (f' - {"; ".join(why)}' if why else ''))
    ctx.placement = {'offload': 'mac' if n_mac else 'desktop', 'throttle': throttle, 'live_runs': live or [],
                     'mac': mac_what, 'summary': summary, 'slot_wait': ctx.slot_wait}
    return out


# ---------------------------------------------------------------------------- shared steps

def build_step(ctx, name='build'):
    # (tools/build_py.sh takes the module from the artifact cache when this commit's Rust inputs were built before:
    # ENGINE.md 167)
    pkg = os.path.join(ROOT, 'mtgz')
    return Step(name, ['bash', 'tools/build_py.sh'], id='build', checker=make_build_checker('mtgz._engine', pkg), heavy=True,
                evidence=f'"mtgz._engine OK: {pkg}{os.sep}_engine.* | games: [...]" (the fresh module, imported from '
                         f'this checkout) + exit 0')


def mac_built(shell):
    """A Mac command that needs this checkout's engine module there: tools/build_py.sh first (incremental after the
    gate's first Mac step), then `shell`."""
    return (['bash', '-c', f'bash tools/build_py.sh && {shell}'], None, None)


PYTEST_ARGS = ['-q', '-p', 'no:cacheprovider', '-rs']      # (-rs: the skip reasons, the live-run fence's among them)


PYTEST_DURATIONS = 'pytest_durations.json'      # each test file's seconds, from the shards' junit files


def durations_dir():
    """Where the pytest durations live: the machine's state dir shared by every checkout (tools/slotd.sh's:
    $MTGZ_SLOTS_HOME, $ZERO_BIG_DISK/mtg-zero-state on Windows, ~/.mtgz-state - a warm worktree's gates/ is its own),
    else gates/."""
    for d in (os.environ.get('MTGZ_SLOTS_HOME'), f'{BIG_DISK}/mtg-zero-state' if BIG_DISK else None,
              os.path.expanduser('~/.mtgz-state')):
        if d and os.path.isdir(d):
            return d
    return GATES_DIR


def pytest_files():
    """Every test file `pytest tests` collects: tests/test_*.py and the card-IR pipeline's tests/ir/test_*.py
    (ENGINE.md 170)."""
    found = glob.glob(os.path.join(ROOT, 'tests', 'test_*.py')) + glob.glob(os.path.join(ROOT, 'tests', 'ir', 'test_*.py'))
    return sorted(os.path.relpath(p, ROOT).replace('\\', '/') for p in found)


def pytest_partition(files, n, weights):
    """`files` into `n` shards of about equal time -> [(entries, deselected)]: `entries` the pytest arguments (files, or
    node ids of a split file), `deselected` node ids passed as --deselect. `weights` = seconds from earlier runs
    (durations_dir()'s pytest_durations.json: per file, and per test as "<file>::<test id>"); a file without one is
    weighed by its size (1 s per 2 kB). A file longer than a shard's share with measured tests is split by its tests:
    its first part runs the file with the other parts' tests deselected (so a test added since, unmeasured, runs there),
    the others name their tests. Then longest first, each unit to the lightest shard."""
    def w(f):
        if f in weights:
            return float(weights[f])
        try:
            return os.path.getsize(os.path.join(ROOT, f)) / 2000.0
        except OSError:
            return 1.0
    target = sum(w(f) for f in files) / max(1, n)
    units = []                                  # (seconds, entries, deselected)
    for f in files:
        tests = {k: float(v) for k, v in weights.items() if k.startswith(f + '::')}
        k = min(len(tests), int(-(-w(f) // target)) if target > 0 else 1)
        if k < 2:
            units.append((w(f), [f], []))
            continue
        parts = [[0.0, []] for _ in range(k)]
        for t in sorted(tests, key=lambda t: (-tests[t], t)):
            p = min(parts, key=lambda p: p[0])
            p[0] += tests[t]
            p[1].append(t)
        rest = [t for p in parts[1:] for t in p[1]]
        units.append((parts[0][0] + max(0.0, w(f) - sum(tests.values())), [f], sorted(rest)))
        units += [(p[0], sorted(p[1]), []) for p in parts[1:] if p[1]]
    bins = [[0.0, [], []] for _ in range(n)]
    for sec, entries, desel in sorted(units, key=lambda u: (-u[0], u[1])):
        b = min(bins, key=lambda b: b[0])
        b[0] += sec
        b[1] += entries
        b[2] += desel
    return [(b[1], b[2]) for b in bins if b[1]]


def pytest_coverage(files, shards):
    """None when `shards` (pytest_partition's) run every test of `files` exactly once, else why not: every file is the
    entry of exactly one shard, and each node id named elsewhere is a test of a file deselected in that file's shard."""
    whole = [e for entries, _ in shards for e in entries if '::' not in e]
    if sorted(whole) != sorted(files):
        return f'files as entries: {sorted(set(files) ^ set(whole)) or "a file twice"}'
    owner = {e: k for k, (entries, _) in enumerate(shards) for e in entries if '::' not in e}
    named = [e for entries, _ in shards for e in entries if '::' in e]
    desel = {d: k for k, (_, ds) in enumerate(shards) for d in ds}
    if len(named) != len(set(named)) or sorted(named) != sorted(desel):
        return 'a split file\'s named tests and its deselected ones differ'
    for t in named:
        f = t.split('::')[0]
        if f not in owner or desel[t] != owner[f]:
            return f'{t}: not deselected in the shard that runs {f}'
    return None


def read_durations():
    try:
        with open(os.path.join(durations_dir(), PYTEST_DURATIONS), encoding='utf-8') as f:
            return json.load(f)
    except (OSError, ValueError):
        return {}


def junit_node(tc):
    """A junit testcase's pytest node id ("tests/test_x.py::Class::test[param]") and its file, from its classname
    ("tests.test_x" or "tests.test_x.Class") and name."""
    parts = (tc.get('classname') or '').split('.')
    k = next((i for i in range(len(parts)) if parts[i].startswith('test_')), len(parts) - 1)
    f = '/'.join(parts[:k + 1]) + '.py'
    if not f.startswith('tests/'):
        f = 'tests/' + f.split('tests/')[-1]
    return '::'.join([f, *parts[k + 1:], tc.get('name') or '']), f


def record_durations(xml_path):
    """A shard's junit file -> each test file's and each test's seconds into durations_dir()'s pytest_durations.json
    (under a lock file: the shards finish at about the same time). Best effort: a missing or unreadable file changes
    nothing. A split file's parts each add their tests; the file's own total is written by the part that ran it whole
    or, for a split file, summed from its tests' latest seconds."""
    import xml.etree.ElementTree as ET
    try:
        root = ET.parse(xml_path).getroot()
    except (OSError, ET.ParseError):
        return
    tests = {}
    for tc in root.iter('testcase'):
        node, _ = junit_node(tc)
        tests[node] = tests.get(node, 0.0) + float(tc.get('time') or 0)
    lock = os.path.join(durations_dir(), PYTEST_DURATIONS + '.lock')
    for _ in range(100):
        try:
            fd = os.open(lock, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
            break
        except FileExistsError:
            time.sleep(0.1)
    else:
        return
    try:
        d = read_durations()
        d.update({k: round(v, 2) for k, v in tests.items()})
        for f in {n.split('::')[0] for n in tests}:
            d[f] = round(sum(v for k, v in d.items() if k.startswith(f + '::')), 2)
        with open(os.path.join(durations_dir(), PYTEST_DURATIONS), 'w', encoding='utf-8') as fh:
            json.dump(d, fh, indent=1, sort_keys=True)
    finally:
        os.close(fd)
        os.remove(lock)
    try:
        os.remove(xml_path)
    except OSError:
        pass


def pytest_step(name='pytest', step_id='pytest', ctx=None):
    """This repo's pytest -> its steps. On the Mac (ENGINE.md 166) without PYTEST_DESKTOP_ONLY, which run on the desktop
    beside it (step `pytest-desktop`): together exactly the desktop's tests. With ctx.pytest_shards > 1 (ENGINE.md 167)
    the tests are split into that many shards (`pytest-K-of-N`: pytest_partition, every test exactly once -
    pytest_coverage, checked when the plan is made), each its own process and --basetemp, run at once by a parallel
    group: on the desktop each shard runs its part, on the Mac each runs its part but the desktop-only tests, and the
    desktop part beside the first shard runs those."""
    ev = "pytest's final summary line: 0 failed, 0 errors, N > 0 passed + exit 0"
    files = [a for a in PYTEST_DESKTOP_ONLY if '::' not in a]
    ids = [a for a in PYTEST_DESKTOP_ONLY if '::' in a]
    desk = Step(f'{name} (desktop part: the Windows worker, private data, platform-bound checks)',
                [python_exe(), '-m', 'pytest', *PYTEST_DESKTOP_ONLY, *PYTEST_ARGS], id=f'{step_id}-desktop',
                checker=check_pytest, heavy=True, evidence=ev)
    n = getattr(ctx, 'pytest_shards', 1) if ctx is not None else 1
    if n <= 1:
        mac = shlex.join(['python', '-m', 'pytest', 'tests', *PYTEST_ARGS, *[f'--ignore={f}' for f in files],
                          *[f'--deselect={i}' for i in ids]])
        return [Step(name, [python_exe(), '-m', 'pytest', 'tests', *PYTEST_ARGS], id=step_id, checker=check_pytest,
                     heavy=True, mac_cmd=lambda ctx: mac_built(mac), desktop_part=desk, evidence=ev)]
    allf = pytest_files()
    shards = pytest_partition(allf, n, read_durations())
    why = pytest_coverage(allf, shards)
    if why:
        raise GateError(f'internal: the pytest shards do not run every test exactly once ({why})')
    steps, first = [], True
    for k, (entries, desel) in enumerate(shards, 1):
        tag = f'{k}-of-{len(shards)}'
        base = os.path.join(GATES_DIR, 'tmp', f'pytest-{ctx.sha_short}-{tag}-{uuid.uuid4().hex[:8]}')
        xml = base + '.xml'
        on_mac = [e for e in entries if e.split('::')[0] not in files and e not in ids]
        mac_desel = [d for d in desel + ids if d.split('::')[0] in on_mac]
        mac = shlex.join(['python', '-m', 'pytest', *on_mac, *PYTEST_ARGS, *[f'--deselect={d}' for d in mac_desel]])
        nfiles = len({e.split('::')[0] for e in entries})
        s = Step(f'{name} {tag} ({nfiles} files)',
                 [python_exe(), '-m', 'pytest', *entries, *PYTEST_ARGS, *[f'--deselect={d}' for d in desel],
                  f'--basetemp={base}', f'--junitxml={xml}'],
                 id=f'{step_id}-{tag}', checker=check_pytest, heavy=True,
                 mac_cmd=(lambda ctx, mac=mac: mac_built(mac)) if on_mac else None,
                 cleanup=lambda base=base, xml=xml: (record_durations(xml), shutil.rmtree(base, ignore_errors=True)),
                 evidence=ev)
        if on_mac and first:
            s.desktop_part, first = desk, False
        steps.append(s)
    return steps


def in_parallel(*steps):
    """`steps` (Steps or lists of them) as one parallel group (ENGINE.md 167): run_steps runs them at once - each where
    `place` put it, a heavy desktop one through the machine's throttle when the gate throttles; the gate goes on when
    all ended, and stops after the group if any failed."""
    flat = []
    for s in steps:
        flat.extend(s if isinstance(s, list) else [s])
    for s in flat:
        s.group = 'parallel'
    return flat


def test_build_step(tests):
    """The engine test binaries `tests` (cargo --test targets; none: all) built before the steps that run them, so a
    parallel group's cargo steps only run (ENGINE.md 167); on the Mac too when the gate offloads (its steps then find
    them built there)."""
    argv = ['cargo', 'test', '--release', '-p', 'engine', '--no-run']
    for t in tests:
        argv += ['--test', t]
    return Step(f'build the test binaries ({", ".join(tests) if tests else "all"})', argv, id='test-build',
                checker=check_test_build, heavy=True, mac_cmd=True, evidence='"Finished `release` profile" + exit 0')


def cargo_step(name, test, step_id=None):
    return Step(name, ['cargo', 'test', '--release', '-p', 'engine', '--test', test], id=step_id or f'cargo-test-{test}',
                checker=make_cargo_checker(single=True), mac_cmd=True,
                evidence='exactly one "test result: ok. N passed; 0 failed" line, N > 0 + exit 0')


def worker_path(ctx):
    return os.path.join(ctx.target_dir, 'release', WORKER_EXE)


def worker_build_step(ctx):
    """The release `mtgz-worker` (zero-farm's executable) built from this commit into the gate's target dir, before
    pytest (its worker tests run it: they find it through CARGO_TARGET_DIR / MTGZ_WORKER, so they never test a stale
    build - ENGINE.md 114.9's first engine gate did) and the farm-compat step (ENGINE.md 155)."""
    exe = worker_path(ctx)
    # (tools/zcache.py: the artifact cache by this commit's Rust input hash, else cargo build + stored; ENGINE.md 167)
    return Step("worker build (release mtgz-worker, the farm's executable)",
                [python_exe(), 'tools/zcache.py', 'worker', '--root', ROOT, '--target-dir', ctx.target_dir],
                id='worker-build', checker=make_worker_build_checker(exe), heavy=True,
                evidence=f'"Finished `release` profile" or "artifact cache: hit worker ... -> {exe}", {exe} there + exit 0')


def farm_compat_step(ctx):
    """Farm compatibility is required (AGENTS.md, ENGINE.md 155): tools/farm_compat.py with the worker this gate built -
    gate 2 (the worker's self-play vs zero-core's Python remote worker, byte-identical shards and batches) and gate 3
    (its records replayed by the trainer's path == its shards) on the Delver best-of-three dealt preset, uniform
    evaluator, a few matches. The full farm gates on a run's real network are tools/farm_gates.py (ENGINE.md 114.8)."""
    out = lazy_run_dir(ctx, 'farm-compat')
    return Step('farm compat (worker vs Python self-play; records -> trainer rows)',
                [python_exe(), 'tools/farm_compat.py', '--worker', worker_path(ctx), '--out', out], id='farm-compat',
                checker=check_farm_compat, cleanup=lambda: shutil.rmtree(out, ignore_errors=True), heavy=True,
                evidence='exactly one "farm compat: PASSED - ..." line (gate 2 and gate 3 identical, rows > 0, every '
                         "game's decks compared), its JSON summary \"passed\": true + exit 0")


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
        steps.append(Step(name, list(DIGEST_ARGV), env=env, id=f'digest-{which}', record_only=not expect, mac_cmd=True,
                          checker=make_digest_checker(expect, record_only=not expect),
                          evidence=(f'one "combined digest" line over the full {games} games == {expect}' if expect
                                    else f'one "combined digest" line over the full {games} games (recorded, NOT '
                                         f'compared)') + ', no re-record, "test result: ok. 1 passed" + exit 0'))
    steps.extend(digest_format_steps())
    return steps


def digest_format_steps():
    """The digests beyond the cube table's, each compared with the checkout's own recorded file (a change meant to
    alter play re-records them in its commit, as digests.txt; the caller's DIGEST_* never reach them: step_env):
    engine/tests/digest_formats.rs (ENGINE.md 148.1: constructed best-of-three, the legacy decks, EoE, zero-core's
    driver) and dealt_decks.rs::dealt_digests (ENGINE.md 154: the dealt preset, pb2's self-play spec; 157)."""
    return [Step('digest formats (constructed, EoE, driver)', list(FORMATS_ARGV), id='digest-formats', mac_cmd=True,
                 checker=check_digest_formats,
                 evidence='exactly one "<name>: ... combined digest <16 hex>" line each of constructed, eoe and driver, '
                          'no "wrote ...digests_" line, exactly one "test result: ok. 3 passed" + exit 0'),
            Step('digest dealt (dealt decks: the dealt preset\'s self-play spec)', list(DEALT_ARGV), id='digest-dealt',
                 mac_cmd=True,
                 checker=check_digest_dealt,
                 evidence='exactly one "dealt_digests: N matches in ... s; combined digest <16 hex>" line, no '
                          '"wrote ...digests_dealt" line, exactly one "test result: ok. 1 passed" + exit 0')]


def core_pytest_base(ctx):
    """pytest's --basetemp for the core suite (it writes runs and checkpoints): off C: on the desktop, as
    smoke_all.sh does (the machine's big disk, ZERO_BIG_DISK); unique per gate run; NOT created here (--dry-run touches
    no disk)."""
    name = f'core-{ctx.sha_short}-{uuid.uuid4().hex[:8]}'
    if WIN and BIG_DISK:
        return f'{BIG_DISK}/zero-smoke/mtgz-gate-{slug(os.path.basename(ROOT))}-pytest/{name}'
    return os.path.join(GATES_DIR, 'tmp', f'pytest-{name}')


def core_steps(ctx):
    """zero-core's own suite as its own steps (ENGINE.md 91): its engine module (python/zero/_engine, the
    toy games its tests import) built by its own tools/build_py.sh, then `python -m pytest tests -q` from
    the core checkout - how zero-core's AGENTS.md and smoke_all.sh run it - CPU only and below normal
    priority like every step, with the FLAKY_FILE retry policy (FlakyRetry)."""
    base = core_pytest_base(ctx)

    def argv(target, tmp):
        return [python_exe(), '-m', 'pytest', target, '-q', '-p', 'no:cacheprovider', '-rfEs',
                '-W', 'ignore::pytest.PytestWarning', f'--basetemp={tmp}']
    zero_pkg = os.path.join(CORE, 'python', 'zero')
    return [
        # (tools/zcache.py core-pyd: the artifact cache by the pinned commit, else the core's own tools/build_py.sh,
        # stored; ENGINE.md 167)
        Step('core build (zero._engine)', [python_exe(), 'tools/zcache.py', 'core-pyd', '--root', ROOT], id='core-build',
             heavy=True,
             checker=make_build_checker('zero._engine', zero_pkg),
             evidence=f'"zero._engine OK: {zero_pkg}{os.sep}_engine.* | games: [...]" + exit 0'),
        Step('core pytest', argv('tests', base), cwd=CORE, id='core-pytest', checker=check_pytest, heavy=True,
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
                       args.checkpoint_spec or '{"max_turns": 60}'], id=sid, heavy=True,
                checker=make_compat_checker(base, ROOT),
                evidence='{"identical": true, "differences": []}, the old side = the baseline\'s and the new side = '
                         'this checkout\'s mtgz/_engine, samples > 0 on both + exit 0')


def smoke_step(name, specs, extra_args=(), group=None):
    n = len(specs)              # one seed (0) per spec
    return Step(name, [python_exe(), 'tools/search_smoke.py', '--specs', ','.join(specs), '--seeds', '0',
                       *extra_args], id='search-smoke', checker=make_search_smoke_checker(n), group=group, heavy=True,
                mac_cmd=lambda ctx: mac_built(shlex.join(['python', 'tools/search_smoke.py', '--specs', ','.join(specs),
                                                          '--seeds', '0', *extra_args])),
                evidence=f'"search smoke: 0 failed of {n} configurations" + exit 0')


def mac_run_loop(ctx, preset):
    """The run-loop smoke on the Mac (ENGINE.md 166): a throwaway run under the Mac checkout's gates/tmp, its
    metrics.jsonl printed line by line with METRICS_PREFIX (the checker reads those), the run removed after."""
    rel = f'gates/tmp/gate-{ctx.tier}-smoke-{ctx.sha_short}-{uuid.uuid4().hex[:8]}'
    run = shlex.join(['python', '-m', 'mtgz.az.run', '--run', rel, '--preset', preset, '--iters', '1'])
    return mac_built(f'{{ {run}; rc=$?; sed "s/^/{METRICS_PREFIX}/" {rel}/metrics.jsonl; rm -rf {rel}; exit $rc; }}')


def run_loop_step(ctx, preset, games):
    run_dir = lazy_run_dir(ctx, 'smoke')
    return Step(f'run-loop smoke ({preset})',
                [python_exe(), '-m', 'mtgz.az.run', '--run', run_dir, '--preset', preset, '--iters', '1'],
                id='run-loop-smoke', checker=make_metrics_checker(run_dir, want_games=games, want_iters=1),
                cleanup=lambda: shutil.rmtree(run_dir, ignore_errors=True), heavy=True,
                mac_cmd=lambda ctx: mac_run_loop(ctx, preset), mac_checker=make_metrics_checker(None, games, 1),
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
    # (pytest runs the worker tests: the worker built from this commit, never another checkout's - ENGINE.md 155); the
    # builds come from the artifact cache when no Rust input changed, and pytest's shards and the smoke run at once
    # (ENGINE.md 167)
    return [build, worker_build_step(ctx), *in_parallel(pytest_step(ctx=ctx), run_loop_step(ctx, preset, games))]


def tier_encoding(ctx, args):
    sid = 'checkpoint-load'
    if args.checkpoint:
        if not os.path.isfile(args.checkpoint):
            raise GateError(f'--checkpoint {args.checkpoint}: no such file')
        load = Step('checkpoint-load (mtgz.az.h2h)',
                    [python_exe(), '-m', 'mtgz.az.h2h', args.checkpoint, args.checkpoint, '--games', '8', '--sims', '16',
                     '--spec', args.checkpoint_spec or '{"max_turns": 60}', '--device', 'cpu', '--threads', '4',
                     '--select', 'visits'], id=sid, checker=make_h2h_checker(8), heavy=True,
                    evidence='h2h\'s result line: 8 games, 0 unfinished + exit 0')
    elif opted_out(args, sid):
        load = Step('checkpoint-load (mtgz.az.h2h)', [], id=sid,
                    skip_reason=f'--allow-skip {sid}: no --checkpoint, no checkpoint was loaded on the new encoding')
    else:
        raise GateError(f'the encoding tier loads a run\'s checkpoint on the new encoding: it needs --checkpoint <a .pt> '
                        f'(--checkpoint-spec for its spec); --allow-skip {sid} runs without it (SKIPPED, named in the '
                        f'final line)')
    steps = [build_step(ctx), worker_build_step(ctx), test_build_step(['digest', 'digest_formats', 'dealt_decks', 'az']),
             *in_parallel(*digest_steps(ctx, args), cargo_step('cargo test az', 'az'), load, pytest_step(ctx=ctx),
                          farm_compat_step(ctx))]
    # NO search smoke (the uniform evaluator cannot see an encoding change).
    return steps


# The gate's fuzz before an engine change merges (the user's decision D1, 2026-11-15; ENGINE.md 167): ~2,000 games per
# pool, sharded across cores (tools/fuzz_shards.py), the hidden-information checks included; the 20k depth runs on the
# farm continuously, on main and each live run's build, not in the gate.
FUZZ_GAMES = 2000
FUZZ_POOLS = 'default,default+hidden,delver_dealt'    # the suite tier's default --fuzz-pools (+hidden: FUZZ_HIDDEN_DECKS=1)
FUZZ_RE = re.compile(r'^(\d+) games: wins \[', re.M)
SUITE_STEPS = ('cargo-test-engine',)          # + one 'fuzz-<pool>' step per --fuzz-pools entry


def make_fuzz_checker(games):
    """The fuzz test binary's positive evidence: fuzz_random_games' "N games: wins [...]" line (printed with
    --nocapture, after every game ended) with N = `games` exactly, then the binary's one "test result: ok. N
    passed; 0 failed" line (its match fuzz and the rest passed too) + exit 0."""
    def checker(output, rc):
        ms = FUZZ_RE.findall(output)
        if not ms:
            return False, 'no "N games: wins [...]" line (fuzz_random_games did not finish, or ran without --nocapture)', {}
        if len(ms) != 1:
            return False, f'{len(ms)} "N games: wins" lines (expected exactly 1)', {}
        n = int(ms[0])
        if n != games:
            return False, f'{n} games fuzzed, not {games}', {'fuzz_games': n}
        ok, why, extra = make_cargo_checker(single=True)(output, rc)
        extra = dict(extra, fuzz_games=n)
        if not ok:
            return False, f'{n} games fuzzed, but the fuzz binary did not pass: {why}', extra
        return True, f'{n} games fuzzed; {why}', extra
    return checker


def tier_suite(ctx, args):
    """The full engine suite and the sharded fuzz (FUZZ_GAMES a pool: tools/fuzz_shards.py, ENGINE.md 167) on THIS
    commit (the engine change's own half: AGENTS.md); its report gates/<sha12>-suite.json is the evidence the engine
    tier wants for --tested-sha (ENGINE.md 143). The test binaries first, then the suite and every pool's shards at
    once (on the Mac beside a live run: ENGINE.md 166)."""
    pools = split_list(args.fuzz_pools, FUZZ_POOLS, '--fuzz-pools')
    shards = max(1, args.fuzz_shards or 8)
    steps = [Step('cargo test (the full engine suite)', ['cargo', 'test', '--release', '-p', 'engine', '--no-fail-fast'],
                  id='cargo-test-engine', checker=make_cargo_checker(), mac_cmd=True,
                  evidence='"test result:" lines all ok, 0 failed, N > 0 passed + exit 0')]
    for spec in pools:
        pool, _, mod = spec.partition('+')
        if mod not in ('', 'hidden'):
            raise GateError(f'--fuzz-pools {spec}: <pool> or <pool>+hidden (FUZZ_HIDDEN_DECKS=1)')
        hidden = mod == 'hidden'
        # (relative to the checkout: the same path on the Mac's checkout when the step runs there)
        out = f'gates/tmp/fuzz-{ctx.sha_short}-{slug(spec)}-{uuid.uuid4().hex[:8]}'
        argv = [python_exe(), 'tools/fuzz_shards.py', '--pool', pool, '--games', str(FUZZ_GAMES), '--shards', str(shards),
                '--out', out] + (['--hidden'] if hidden else [])
        what = ('the default pool' if pool == 'default' else f'FUZZ_POOL={pool}') + (', hidden decklists' if hidden else '')
        steps.append(Step(f'fuzz {FUZZ_GAMES} games ({what}), {shards} shards', argv, id=f'fuzz-{slug(spec)}',
                          checker=make_fuzz_shards_checker(FUZZ_GAMES, pool, hidden), mac_cmd=True,
                          cleanup=lambda out=out: shutil.rmtree(os.path.join(ROOT, out), ignore_errors=True),
                          evidence=f'exactly one "fuzz shards: PASSED - ..." line and its JSON summary: {FUZZ_GAMES} games of '
                                   f'{spec} over disjoint seed ranges, every range ran, the merged end-of-run checks passed '
                                   f'+ exit 0'))
    return [test_build_step([]), *in_parallel(*steps)]


def suite_report_problems(path, sha, pin):
    """Why the report at `path` is not the evidence that the full engine suite and the fuzz passed on commit
    `sha` with zero-core `pin` -> (problems, the report or None). The evidence: a `suite` tier report of that sha and
    pin, passed and clean (no skip, no --allow-dirty), its engine-suite step and at least one fuzz step of
    FUZZ_GAMES games passed, and every step's log still there with the sha256 the report recorded."""
    try:
        with open(path, encoding='utf-8') as f:
            r = json.load(f)
    except (OSError, ValueError) as e:
        return [f'cannot read it ({type(e).__name__}: {e})'], None
    out = []
    if r.get('tier') != 'suite':
        out.append(f'its tier is {r.get("tier")!r}, not \'suite\'')
    if r.get('sha') != sha:
        out.append(f'it gates {str(r.get("sha"))[:12]}, not {sha[:12]}')
    if pin and r.get('zero_core_pin') != pin:
        out.append(f'its zero-core pin is {str(r.get("zero_core_pin"))[:12]}, not {pin[:12]}')
    if not (r.get('passed') and r.get('clean')):
        out.append(f'it did not pass cleanly: {str(r.get("final"))[:200]}')
    steps = {s.get('id'): s for s in r.get('steps') or []}
    for sid in SUITE_STEPS:
        if (steps.get(sid) or {}).get('status') != 'passed':
            out.append(f'no passed {sid} step')
    fuzz = [s for k, s in steps.items() if str(k).startswith('fuzz-')]
    # (a report of the 20k gate before ENGINE.md 167 is evidence too: more games than the gate asks for now)
    if not any(s.get('status') == 'passed' and (s.get('fuzz_games') or 0) >= FUZZ_GAMES for s in fuzz):
        out.append(f'no passed fuzz step of {FUZZ_GAMES} games')
    for s in steps.values():
        log, want = s.get('log'), s.get('log_sha256')
        if not log:
            continue
        if not os.path.isfile(log):
            out.append(f'its {s.get("id")} log {log} is gone (the evidence cannot be checked)')
        elif sha256_file(log) != want:
            out.append(f'its {s.get("id")} log {log} changed since the report (sha256)')
    return out, r


def suite_evidence_step(ctx, args, sha, why):
    """The engine tier's evidence that the full engine suite and the fuzz passed on `sha` (the --tested-sha, or
    this commit): a `suite` tier report (--suite-report, default gates/<sha12>-suite.json of this checkout). Without
    one the tier refuses unless --allow-skip suite-evidence (SKIPPED: "asserted, not evidenced" in the final line)."""
    sid = 'suite-evidence'
    name = f'evidence: the full engine suite + the {FUZZ_GAMES // 1000}k fuzz passed on {sha[:12]} ({why})'
    path = os.path.abspath(args.suite_report or report_path(sha[:12], 'suite'))
    pin = git(['rev-parse', f'{sha}:third_party/zero-core'], check=False) or None
    if not args.suite_report and not os.path.exists(path):
        if opted_out(args, sid):
            return Step(name, [], id=sid, skip_reason=f'--allow-skip {sid}: the full engine suite and the '
                                                      f'{FUZZ_GAMES // 1000}k fuzz of {sha[:12]} are asserted, not evidenced')
        raise GateError(f'the engine tier wants evidence that the full engine suite and the {FUZZ_GAMES // 1000}k fuzz '
                        f'passed on {sha[:12]} ({why}): `python tools/gate.py suite` in a checkout of it writes '
                        f'gates/{sha[:12]}-suite.json (here: {path}); --suite-report <that file> when it is another '
                        f'checkout\'s; --allow-skip {sid} runs without it (SKIPPED, named in the final line)')
    if opted_out(args, sid):
        raise GateError(f'--allow-skip {sid} with a suite report ({path}): check it or skip it, not both')
    problems, _ = suite_report_problems(path, sha, pin)
    if problems:
        raise GateError(f'suite report {path} is not the evidence for {sha[:12]}: ' + '; '.join(problems))

    def verify():
        p, r = suite_report_problems(path, sha, pin)
        text = f'suite report {path}\nsha256 {sha256_file(path) if os.path.exists(path) else "-"}\n' + \
            json.dumps(r, indent=1) + '\n'
        if p:
            return False, 'the suite report is no longer the evidence: ' + '; '.join(p), {}, text
        fuzz = [s for s in r['steps'] if str(s.get('id')).startswith('fuzz-') and s.get('status') == 'passed']
        games = sorted({s.get('fuzz_games') for s in fuzz if s.get('fuzz_games')})
        return True, (f'{sha[:12]}: full engine suite and {len(fuzz)} fuzz run(s) of '
                      f'{"/".join(map(str, games)) or FUZZ_GAMES} games passed (report {path}, {r.get("started", "")[:19]})'), \
            {'suite_report': path}, text
    return Step(name, [], id=sid, verify=verify,
                evidence=f'{path}: a clean passed suite report of {sha[:12]} (engine suite + a {FUZZ_GAMES}-game fuzz), '
                         f'its logs unchanged')


def tier_engine(ctx, args):
    checks = [*digest_steps(ctx, args), cargo_step('cargo test az', 'az'), cargo_step('cargo test limits', 'limits'),
              cargo_step('cargo test hidden', 'hidden'), *pytest_step(ctx=ctx),
              smoke_step('smoke: 1 desktop config (random, seed 0)', ['random']), farm_compat_step(ctx)]
    # the builds (the artifact cache's when no Rust input changed), the test binaries, then every check at once, each
    # where `place` puts it (ENGINE.md 166, 167)
    steps = [build_step(ctx), worker_build_step(ctx),
             test_build_step(['digest', 'digest_formats', 'dealt_decks', 'az', 'limits', 'hidden']), *in_parallel(*checks)]
    sid = 'mac-recheck'
    name = 'full suite again on the Mac (merged sha differs, overlapping files)'
    if args.tested_sha:
        tested = git(['rev-parse', '--verify', '--quiet', f'{args.tested_sha}^{{commit}}'], check=False)
        if not tested:
            raise GateError(f'--tested-sha {args.tested_sha}: not a commit in this repository')
        steps.append(suite_evidence_step(ctx, args, tested, 'the --tested-sha' if tested != ctx.sha else 'this commit'))
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
        steps.append(suite_evidence_step(ctx, args, ctx.sha, 'this commit: no --tested-sha'))
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
    core_build, core_pytest = core_steps(ctx)
    return [
        build_step(ctx),
        worker_build_step(ctx),
        core_build,
        test_build_step(['digest_formats', 'dealt_decks']),
        # every check at once, each where `place` puts it (ENGINE.md 166, 167)
        *in_parallel(
            Step('zero-core cargo test', ['cargo', 'test', '--release'], cwd=CORE, id='core-cargo-test', heavy=True,
                 evidence='"test result:" lines all ok, 0 failed, N > 0 passed + exit 0'),
            pytest_step('this repo: pytest', ctx=ctx),
            farm_compat_step(ctx),
            compat_step(ctx, args, 'check_checkpoint_compat goldens'),
            *digest_format_steps(),         # (the driver digest: zero-core's Driver<MtgAz>, ENGINE.md 148.1)
            smoke_step(f'smoke: {len(configs)}-config of the pinning game ({",".join(configs)})', configs),
            run_loop_step(ctx, 'mtg-smoke', 16),
            # smoke_all for the OTHER zero-core games is zero-core-side (its own tools/smoke_all.sh
            # --build-only), run by the orchestrator against the moving core commit - not this repo's gate.
            core_pytest),
    ]


def tier_optin_core(ctx, args):
    need_core()
    feature_tests = split_list(args.feature_tests, 'az,hidden', '--feature-tests')
    core_build, core_pytest = core_steps(ctx)
    checks = [Step('core tests', ['cargo', 'test', '--release'], cwd=CORE, id='core-cargo-test', heavy=True,
                   evidence='"test result:" lines all ok, 0 failed, N > 0 passed + exit 0'),
              compat_step(ctx, args, 'checkpoint_compat byte identity'),
              *digest_format_steps(), farm_compat_step(ctx)]        # (the driver digest: zero-core's Driver<MtgAz>, ENGINE.md 148.1)
    for t in feature_tests:
        checks.append(cargo_step(f'feature tests: cargo test --test {t}', t, step_id=f'feature-{t}'))
    checks.append(build_only_step(ctx, args))
    checks.append(core_pytest)
    # the builds, then every check at once (ENGINE.md 167)
    return [build_step(ctx), worker_build_step(ctx), core_build,
            test_build_step(['digest_formats', 'dealt_decks', *feature_tests]), *in_parallel(*checks)]


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
        # (each --other-games value is one checkout path, never split: 'C:/Users/<first last>/...' has a space)
        games = [os.path.abspath(p) for p in args.other_games if p]
        bad = [p for p in games if not os.path.isdir(p)]
        if bad:
            raise GateError(f'--other-games: no such checkout: {", ".join(bad)}')
    else:
        games = default_other_games()
    names = [repo_name(p) for p in games]
    smoke_target = 'mtgz-gate-' + slug(os.path.basename(ROOT))
    smoke_dir = (f'{BIG_DISK}/zero-smoke/{smoke_target}' if BIG_DISK else os.path.join(GATES_DIR, 'tmp', smoke_target))
    return Step(f'build-only: the other games ({", ".join(names)})',
                ['bash', os.path.join(CORE, 'tools', 'smoke_all.sh'), '--with-this-core', '--build-only', *games],
                cwd=CORE, id='build-only', heavy=True, env={'SKIP_CORE': '1', 'SMOKE_TARGET': smoke_target, 'SMOKE_DIR': smoke_dir,
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
        Step('digest equals main\'s recorded value', list(DIGEST_ARGV), id='digest-full', mac_cmd=True,
             checker=make_digest_checker(args.expect_digest),
             evidence=f'one "combined digest" line over the full {DIGEST_GAMES[0]} + {DIGEST_GAMES[1]} games == '
                      f'{args.expect_digest}, "test result: ok. 1 passed" + exit 0'),
        *digest_format_steps(),
        worker_build_step(ctx),
        farm_compat_step(ctx),
        smoke_step(f'1-config smoke on the run\'s spec ({args.spec}, preset {args.preset})', specs,
                   ['--preset', args.preset], group='parallel-mac'),
        Step('mac.sh --eval-checkout', ['bash', 'tools/mac.sh', '--eval-checkout'], id='mac-eval-checkout', mac=True,
             group='parallel-mac', checker=make_eval_checkout_checker(ctx.sha),
             evidence=f'"[mac] ... @ {ctx.sha_short}..." and "[mac] done @ {ctx.sha_short}...", the Mac\'s "mtgz._engine OK: .../run-{ctx.sha_short}/...", "mtgz '
                      f'remote: ... is {ctx.sha_short}, unchanged", "eval checkout ready: .../run-{ctx.sha_short}" + '
                      f'exit 0'),
    ]


TIERS = {
    'docs': (tier_docs, 'docs / tools not on the run path: py_compile of touched tools (~0 min). Every tier first '
             'runs tools/privacy_check.py on the tracked files (step privacy, in-process).'),
    'python': (tier_python, 'Python run-loop/settings: build + the release worker (its pytest tests; both from the '
               'artifact cache when no Rust input changed) + pytest in --pytest-shards shards beside a tiny run-loop smoke '
               '(~5 min).'),
    'encoding': (tier_encoding, 'encoding-only (layout unchanged): build + DIGEST_ENCODING=0 digest == --expect-digest0 '
                 '+ full digest == --expect-digest + digest formats (constructed, EoE, driver, dealt) == the checkout\'s '
                 'recorded files + --test az + checkpoint-load (--checkpoint) + pytest + farm compat (the release worker built, '
                 'tools/farm_compat.py). NO search smoke (~11 min).'),
    'suite': (tier_suite, 'engine rules/cards (the agent\'s half): the full engine suite + a sharded 2,000-game fuzz per '
              f'pool (--fuzz-pools, default {FUZZ_POOLS}; tools/fuzz_shards.py, --fuzz-shards) at once on this commit; its '
              'report gates/<sha>-suite.json is the engine tier\'s evidence for --tested-sha.'),
    'engine': (tier_engine, 'engine rules/cards (orchestrator half): fresh build, digests == the agent\'s report, '
               'digest formats (constructed, EoE, driver, dealt) == the checkout\'s recorded files, az/limits/hidden, pytest, 1 desktop smoke config, the --tested-sha\'s suite report (the full suite + '
               'the sharded fuzz, evidenced); a Mac full-suite recheck only if the merged sha differs from --tested-sha '
               'with overlapping engine/card_ir files; the release worker built + farm compat (tools/farm_compat.py) (~13 min).'),
    'search-core': (tier_search_core, 'search/core behaviour: build + core cargo test + pytest + check_checkpoint_compat '
                    'goldens (--baseline-root) + digest formats (its driver digest: zero-core\'s driver) + 4-config '
                    'smoke of the pinning game + run-loop smoke + core build + core pytest + the release worker built + farm compat '
                    '(~39 min).'),
    'optin-core': (tier_optin_core, 'opt-in core feature with byte-identical defaults: build + core tests + '
                   'checkpoint_compat byte identity (--baseline-root) + digest formats (its driver digest: zero-core\'s '
                   'driver) + feature tests + build-only of the other games + core build + core pytest + the release worker built + '
                   'farm compat (~29 min).'),
    'repin': (tier_repin, 're-pin onto an already-verified commit: own-target build + digest == main\'s recorded value '
              '+ digest formats (constructed, EoE, driver, dealt) == the checkout\'s recorded files + the release worker built + farm compat + 1-config smoke on the run\'s --spec/--preset + mac.sh '
              '--eval-checkout, in parallel (~8 min).'),
}

# The options each tier uses: any other tier-specific option given is refused (it would be ignored, and
# the check it names would not run).
TIER_OPTIONS = {
    'docs': {'files'},
    'python': {'smoke_preset', 'allow_skip'},
    'encoding': {'expect_digest', 'expect_digest0', 'record_digest', 'checkpoint', 'checkpoint_spec', 'allow_skip'},
    'suite': {'fuzz_pools', 'fuzz_shards'},
    'engine': {'expect_digest', 'expect_digest0', 'record_digest', 'tested_sha', 'suite_report', 'allow_skip'},
    'search-core': {'baseline_root', 'checkpoint_spec', 'configs', 'allow_skip'},
    'optin-core': {'baseline_root', 'checkpoint_spec', 'feature_tests', 'other_games', 'allow_skip'},
    'repin': {'expect_digest', 'spec', 'preset'},
}
TIER_SPECIFIC = set().union(*TIER_OPTIONS.values())
# The steps a caller may opt out of by name (--allow-skip <id>); Mac steps also go with --no-mac.
SKIPPABLE = {'python': ('build',), 'encoding': ('checkpoint-load',), 'engine': ('mac-recheck', 'suite-evidence'),
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

def report_path(sha_short, tier):
    """gates/<sha12>-<tier>.json: one report per commit and tier."""
    return os.path.join(GATES_DIR, f'{sha_short}-{tier}.json')


def write_report(ctx, args, results, ok, not_run, started, seconds, path, line):
    sk, rec, fl = caveats(results, ctx.dirty)
    report = {
        'sha': ctx.sha, 'sha_short': ctx.sha_short, 'zero_core_pin': ctx.core_pin, 'zero_core_head': ctx.core_head,
        'tier': ctx.tier, 'started': started, 'seconds': round(seconds, 1), 'cargo_target_dir': ctx.target_dir,
        'passed': ok, 'clean': bool(ok and not (sk or rec or fl)), 'final': line,
        'skipped': sk, 'recorded': rec, 'flaky': fl, 'not_run': [s.id for s in not_run],
        'tree': {'uncommitted': ctx.dirty, 'allow_dirty': bool(args.allow_dirty)},
        'baseline': ctx.baseline,
        'placement': ctx.placement,
        'options': {k: v for k, v in sorted(vars(args).items()) if v not in (None, [], False) and k != 'dry_run'},
        'digests': {}, 'test_counts': {}, 'steps': [],
    }
    for r in results:
        report['steps'].append({'id': r.id, 'name': r.name, 'status': r.status, 'passed': r.passed,
                                'skipped': r.skipped, 'reason': r.reason, 'cmd': r.argv, 'cwd': r.cwd,
                                'seconds': round(r.seconds, 2), 'rc': r.rc, 'log': r.log, 'log_sha256': r.log_sha256,
                                'mac': r.mac, 'placement': r.placement,
                                **({'retries': r.extra['retries']} if 'retries' in r.extra else {}),
                                **({'fenced': r.extra['fenced']} if r.extra.get('fenced') else {}),
                                **({'fuzz_games': r.extra['fuzz_games']} if 'fuzz_games' in r.extra else {}),
                                **{k: r.extra[k] for k in ('farm_compat', 'worker', 'worker_sha256') if k in r.extra}})
        if r.extra.get('digest'):
            key = 'encoding_free' if 'encoding-free' in r.id else 'full'
            report['digests'][key] = {'value': r.extra['digest'], 'status': r.status}
        for name, d in (r.extra.get('formats') or {}).items():          # (constructed, eoe, driver; dealt)
            report['digests'][name] = {'value': d, 'status': r.status}
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
    ap.add_argument('--suite-report', default=None, help='engine tier: the suite tier\'s report of the --tested-sha (the '
                    'evidence that the full engine suite and the fuzz passed on it; default gates/<sha12>-suite.json '
                    'of this checkout)')
    ap.add_argument('--fuzz-pools', default=None, help='suite tier: comma-separated FUZZ_POOL values, a sharded '
                    f'{FUZZ_GAMES}-game fuzz each, <pool>+hidden for hidden decklists (default: {FUZZ_POOLS})')
    ap.add_argument('--fuzz-shards', type=int, default=None, help='suite tier: shards per fuzz loop (default 8)')
    ap.add_argument('--pytest-shards', type=int, default=4, help='pytest as this many parallel steps over disjoint test '
                    'files (default 4; 1: one step)')
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
                    'else $ZERO_BIG_DISK/cargo-target/mtg-zero/<this checkout\'s dir name> on Windows when the machine '
                    'names a big disk (~/.config/zero/local.env), else this checkout\'s target/)')
    ap.add_argument('--step-timeout', type=float, default=3600.0, help='seconds before a single step is killed as '
                    'hung (default 3600)')
    ap.add_argument('--no-mac', action='store_true', help='skip Mac steps (SKIPPED, not failed, and named in the '
                    'final line); also stands in for the engine tier\'s --tested-sha; nothing is offloaded to the Mac')
    ap.add_argument('--offload', choices=('auto', 'mac', 'local'), default='auto', help='where the steps with a Mac '
                    'command run (ENGINE.md 166): auto (default) = on the Mac (tools/mac.sh, nice, a Mac-side slot) when a '
                    'run is live on this machine and the Mac answers, else here; mac = on the Mac or refuse; local = here')
    ap.add_argument('--throttle', choices=('auto', 'on', 'off'), default='auto', help='the desktop\'s heavy steps '
                    'each hold a tools/slotd.sh slot (RAM / commit aware): auto (default) = while a run is live on this '
                    'machine')
    ap.add_argument('--slot-wait', type=float, default=7200.0, help='seconds a throttled step may wait for its slot '
                    '(on top of --step-timeout; default 7200)')
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
        steps = place(ctx, args, steps)
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
    print(f'placement: {ctx.placement["summary"]}', flush=True)
    started = iso_now()
    t0 = time.time()
    before = tree_state()
    privacy = privacy_result(ctx)                # before any step: quick, and a finding fails the gate
    report_step(privacy)
    if privacy.passed:
        results, ok, not_run = run_steps(ctx, steps)
        results.insert(0, privacy)
    else:
        results, ok, not_run = [privacy], False, [s for s in steps if not (s.skip_reason or s.not_needed)]
    if ok:
        # the checkout at the end: the same files (a step that rewrites a tracked file or leaves an untracked one where
        # a build or test picks it up fails the gate) and the same HEAD (a commit made in the checkout during the gate
        # would leave `git status` clean and the report naming the old sha: ENGINE.md 143)
        changed = sorted(set(tree_state()) - set(before))
        head = git(['rev-parse', 'HEAD'], check=False)
        why = []
        if changed:
            why.append('the checkout changed during the gate: ' + ', '.join(c.strip() for c in changed[:8]))
        if head != ctx.sha:
            why.append(f'HEAD moved during the gate: {ctx.sha_short} -> {head[:12] or "?"}')
        tree = StepResult('the checkout unchanged by the gate', 'tree-unchanged', [], ctx.root, 0.0, None, None, None,
                          'failed' if why else 'passed', '; '.join(why) or 'git status and HEAD unchanged', {})
        results.append(tree)
        report_step(tree)
        ok = not why
    seconds = time.time() - t0
    # one report per (commit, tier): a docs pass on a commit no longer overwrites its engine report (ENGINE.md 143)
    path = report_path(ctx.sha_short, args.tier)
    line = final_line(args.tier, ctx.sha_short, ok, results, not_run, seconds, path, ctx.dirty)
    write_report(ctx, args, results, ok, not_run, started, seconds, path, line)
    print(line, flush=True)
    return 0 if ok else 1


if __name__ == '__main__':
    sys.exit(main())
