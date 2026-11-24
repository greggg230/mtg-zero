"""The engine fuzz of one pool, sharded across cores (docs/ENGINE.md 167): the same games, seeds and checks as one
`cargo test --release -p engine --test fuzz` run of them, in a fraction of the time.

    python tools/fuzz_shards.py --pool default --games 2000 --shards 8
    python tools/fuzz_shards.py --pool default --hidden --games 2000        # FUZZ_HIDDEN_DECKS=1
    python tools/fuzz_shards.py --pool delver_dealt --games 2000 --jobs 4 --out E:/tmp/fz
    python tools/fuzz_shards.py --pool lands --games 600 --belief 2 --jobs 4          # FUZZ_BELIEF=2 in every shard
    python tools/fuzz_shards.py --pool lands --games 600 --chance 10 --jobs 4         # FUZZ_CHANCE=10 in every shard

The fuzz's own settings come from the flags only (each shard states its own): a caller's environment holding a FUZZ_*
variable is refused (exit 2), never silently dropped (ENGINE.md 270.4: a sharded FUZZ_BELIEF=2 run once played without
it). `--belief T` / `--chance N` put FUZZ_BELIEF / FUZZ_CHANCE into every games and matches shard and the merged
checks (echoed in the first line and each shard's log), and the run fails unless the merged totals show them at work
(`belief_worlds` / `chance_walks` > 0). The rest of the fuzz binary runs in the pool's settings alone (its
`the_murktide_trigger_is_asserted_over_enough_games` fails under any FUZZ_CHANCE: its synthetic totals hold no chance
walks, ENGINE.md 220.3.8).

What it runs, all from the fuzz test binary of THIS checkout (built first: `cargo test --release -p engine --test fuzz
--no-run`, into $CARGO_TARGET_DIR):

  * `fuzz_random_games` over disjoint seed ranges [first + k*G/S, ...) covering [first, first + games) (FUZZ_FIRST /
    FUZZ_GAMES per shard), and `fuzz_random_matches` the same way over [first, first + matches) (matches default
    max(20, games / 20), as the test's own default), each shard its own process (one test, one thread) with
    FUZZ_SHARD=1: every check of every game and match runs in its shard; the end-of-run checks over the whole run
    (what the mode must have exercised, the information-set walks' counters IS_WALKS / IS_STEPS / MATCH_IS_* /
    HD_STEPS, ... each over its least number of games) are left to the merged totals - a small shard must neither
    dodge a check that needs 200 games nor pass one alone;
  * the merge: every shard exited 0 with exactly one "test result: ok. 1 passed" line, its "FUZZ_TOTALS" line names
    exactly the range it was given (kind, first, n), the ranges tile [first, first + count) with no gap or overlap,
    the counts add up, and the shards' counters summed (a `max_` counter: the max) go through the same end-of-run
    checks as one run of all the seeds (`fuzz_shard_totals` with FUZZ_TOTALS_FILE: its "fuzz totals: <kind> checked
    over N seeds" lines);
  * the rest of the fuzz binary's tests once (the replay tests and the regression seeds), in the pool's settings.

The last lines: "fuzz shards: PASSED - ..." or "fuzz shards: FAILED - ..." (a failing shard's seed and its rerun
command come from the test's own panic message, in its log), then one JSON summary line {"fuzz_shards": {...}}.
Exit 0 passed, 1 failed, 2 setup error. Standard library only; below-normal priority on Windows.
"""
import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
import threading
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ENGINE_DIR = os.path.join(ROOT, 'engine')
WIN = sys.platform == 'win32'
TEST_RESULT_RE = re.compile(r'^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; \d+ ignored; \d+ measured; '
                            r'\d+ filtered out', re.M)
TOTALS_RE = re.compile(r'^FUZZ_TOTALS (\{.*\})\s*$', re.M)
CHECKED_RE = re.compile(r'^fuzz totals: (\w+) checked over (\d+) seeds', re.M)
SEED_FAIL_RE = re.compile(r"(?:seed|match) (\d+) failed \(rerun: ([^)]*)\)")
GAMES_RE = re.compile(r'^(\d+) games: wins \[', re.M)
# one test thread with --nocapture: the harness's "test <name> ... " precedes the test's first printed line
HARNESS_RE = re.compile(r'^test \S+ \.\.\. (?=\S)', re.M)


def lines_of(out):
    """A test process's output with the harness's "test <name> ... " prefix taken off the line it shares."""
    return HARNESS_RE.sub('', out)
# the settings a shard's process takes from the caller's environment: none of the fuzz's own (each shard states its
# own), so a stray FUZZ_SKIP or FUZZ_GAMES in the caller's shell never changes what a shard plays; a FUZZ_* one in the
# caller's environment is refused (`stray_settings`), not dropped: the flags set what a shard takes
SCRUB = ('FUZZ_', 'DIGEST_')


class SetupError(Exception):
    pass


def stray_settings(environ=None):
    """The caller's FUZZ_* variables (sorted): settings a shard would not take - main() refuses to run with any."""
    env = os.environ if environ is None else environ
    return sorted(k for k in env if k.upper().startswith('FUZZ_'))


def flag_settings(belief=None, chance=None):
    """The fuzz settings the flags give every games / matches shard and the merged checks."""
    out = {}
    if belief is not None:
        if not belief > 0:
            raise SetupError(f'--belief {belief}: a tilt > 0')
        out['FUZZ_BELIEF'] = str(int(belief)) if belief == int(belief) else repr(float(belief))
    if chance is not None:
        if chance < 1:
            raise SetupError(f'--chance {chance}: one decision in n, n >= 1')
        out['FUZZ_CHANCE'] = str(int(chance))
    return out


def settings_at_work(settings, games_totals):
    """Problems when a flag's setting left no trace in the merged games totals (a silent no-op is a failure)."""
    t = games_totals or {}
    probs = []
    for var, counter in (('FUZZ_BELIEF', 'belief_worlds'), ('FUZZ_CHANCE', 'chance_walks')):
        if var in settings and not t.get(counter):
            probs.append(f'{var}={settings[var]} was given but the merged totals hold {t.get(counter, 0)} {counter}: '
                         f'the setting did not reach the shards')
    return probs


def split_ranges(first, count, shards):
    """[first, first + count) as `shards` contiguous ranges (the first ones one longer), empty ones dropped."""
    shards = max(1, min(shards, count)) if count else 0
    out, at = [], first
    for k in range(shards):
        n = count // shards + (1 if k < count % shards else 0)
        if n:
            out.append((at, n))
            at += n
    return out


def child_env(extra):
    env = {k: v for k, v in os.environ.items() if not k.upper().startswith(SCRUB)}
    env.update({'RUST_BACKTRACE': '1', 'CUDA_VISIBLE_DEVICES': '-1'})
    env.update(extra)
    return env


def creationflags():
    return subprocess.BELOW_NORMAL_PRIORITY_CLASS if WIN else 0


def find_exe(target, cargo_env):
    """The test binary `target` of engine, built if needed (`cargo test --release -p engine --test <target> --no-run`):
    its path from cargo's JSON messages."""
    argv = ['cargo', 'test', '--release', '-p', 'engine', '--test', target, '--no-run', '--message-format=json-render-diagnostics']
    p = subprocess.run(argv, cwd=ROOT, capture_output=True, text=True, env=cargo_env, creationflags=creationflags())
    exes = []
    for line in p.stdout.splitlines():
        if not line.startswith('{'):
            continue
        try:
            m = json.loads(line)
        except ValueError:
            continue
        t = m.get('target') or {}
        if m.get('reason') == 'compiler-artifact' and t.get('name') == target and 'test' in (t.get('kind') or []) \
                and m.get('executable'):
            exes.append(m['executable'])
    if p.returncode or len(exes) != 1:
        raise SetupError(f'`{" ".join(argv)}` (exit {p.returncode}) gave {len(exes)} executable(s) for test target '
                         f'{target!r}:\n{p.stderr[-3000:]}')
    return exes[0]


def list_tests(exe):
    p = subprocess.run([exe, '--list'], cwd=ENGINE_DIR, capture_output=True, text=True, env=child_env({}),
                       creationflags=creationflags())
    if p.returncode:
        raise SetupError(f'{exe} --list: exit {p.returncode}: {p.stderr[-500:]}')
    return [line[:-len(': test')] for line in p.stdout.splitlines() if line.endswith(': test')]


def test_named(names, short):
    """The one test whose name is `short` or ends with `::short` (a consolidated binary's tests are module paths)."""
    hits = [n for n in names if n == short or n.endswith('::' + short)]
    if len(hits) != 1:
        raise SetupError(f'{len(hits)} tests named {short!r} in the fuzz binary ({", ".join(hits) or "none"})')
    return hits[0]


class Job:
    def __init__(self, name, argv, env, log):
        self.name, self.argv, self.env, self.log = name, argv, env, log
        self.out, self.rc, self.seconds = '', None, 0.0


def run_jobs(jobs, parallel, timeout, stop_on_fail):
    """Run `jobs` with at most `parallel` at once (in order: the longest first is the caller's job); each one's output
    to its log. A failure stops new jobs from starting when `stop_on_fail` (those already running finish)."""
    lock = threading.Lock()
    queue = list(jobs)
    failed = threading.Event()

    def worker():
        while True:
            with lock:
                if not queue or (stop_on_fail and failed.is_set()):
                    return
                j = queue.pop(0)
            t0 = time.time()
            with open(j.log, 'w', encoding='utf-8', errors='replace') as lf:
                lf.write(f'$ {" ".join(j.argv)}\nenv: {json.dumps({k: v for k, v in j.env.items() if k.startswith("FUZZ_")})}\n\n')
                lf.flush()
                try:
                    p = subprocess.run(j.argv, cwd=ENGINE_DIR, env=j.env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                       stdin=subprocess.DEVNULL, text=True, encoding='utf-8', errors='replace',
                                       timeout=timeout, creationflags=creationflags())
                    j.out, j.rc = lines_of(p.stdout), p.returncode
                except subprocess.TimeoutExpired as e:
                    j.out = (e.stdout or '') if isinstance(e.stdout, str) else (e.stdout or b'').decode('utf-8', 'replace')
                    j.rc = 'timeout'
                lf.write(j.out)
            j.seconds = time.time() - t0
            if j.rc != 0:
                failed.set()
    threads = [threading.Thread(target=worker) for _ in range(max(1, parallel))]
    for t in threads:
        t.start()
    for t in threads:
        t.join()


def shard_problem(j, kind, first, n):
    """None when shard job `j` of `kind` over [first, first + n) is positive evidence, else why not; -> (problem,
    its FUZZ_TOTALS objects)."""
    if j.rc is None:
        return 'never ran (an earlier shard failed)', []
    m = SEED_FAIL_RE.search(j.out)
    if j.rc != 0:
        why = f'exit {j.rc}'
        if m:
            why += f': {m.group(0)}'
        return why, []
    results = TEST_RESULT_RE.findall(j.out)
    if len(results) != 1 or results[0][0] != 'ok' or results[0][1] != '1' or results[0][2] != '0':
        return f'{len(results)} "test result:" line(s) {results} (expected one "ok. 1 passed; 0 failed")', []
    tots = []
    for raw in TOTALS_RE.findall(j.out):
        try:
            tots.append(json.loads(raw))
        except ValueError:
            return f'an unreadable FUZZ_TOTALS line: {raw[:200]}', []
    own = [t for t in tots if t.get('kind') == kind]
    if len(own) != 1:
        return f'{len(own)} FUZZ_TOTALS lines of kind {kind!r} (expected exactly 1)', tots
    t = own[0]
    if (t.get('first'), t.get('n')) != (first, n):
        return f'its FUZZ_TOTALS names [{t.get("first")}, +{t.get("n")}), not [{first}, +{n})', tots
    if kind == 'games':
        g = GAMES_RE.findall(j.out)
        if g != [str(n)]:
            return f'"N games: wins" line(s) {g}, expected one of {n}', tots
    if 'skipped' not in t and 'FUZZ_SHARD=1:' not in j.out:
        return 'no "FUZZ_SHARD=1:" line: the shard ran its own end-of-run checks (FUZZ_SHARD did not reach it?)', tots
    return None, tots


def merge(kind, parts, first, count):
    """The shards' FUZZ_TOTALS of `kind` -> (merged {"n", "totals"} or {"skipped"}, problems): the ranges tile
    [first, first + count), the counts add up, every shard agrees on a skip."""
    probs = []
    ranges = sorted((p['first'], p['n']) for p in parts)
    at = first
    for a, n in ranges:
        if a != at:
            probs.append(f'{kind}: a {"gap" if a > at else "overlap"} at seed {at} (next range starts at {a})')
        at = a + n
    if at != first + count:
        probs.append(f'{kind}: the ranges end at {at}, not {first + count}')
    if sum(n for _, n in ranges) != count:
        probs.append(f'{kind}: the shards played {sum(n for _, n in ranges)}, not {count}')
    skips = {p.get('skipped') for p in parts}
    if len(skips) != 1:
        probs.append(f'{kind}: shards disagree on a skip: {sorted(map(str, skips))}')
        return None, probs
    skip = skips.pop()
    if skip is not None:
        return {'skipped': skip, 'n': count}, probs
    tot = {}
    for p in parts:
        for k, v in (p.get('totals') or {}).items():
            if not isinstance(v, int) or v < 0:
                probs.append(f'{kind}: counter {k} = {v!r} is not a count')
                continue
            tot[k] = max(tot.get(k, 0), v) if k.startswith('max_') else tot.get(k, 0) + v
    return {'n': count, 'totals': tot}, probs


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0], formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--pool', default='default', help='FUZZ_POOL (default: the default pool, FUZZ_POOL unset)')
    ap.add_argument('--hidden', action='store_true', help='FUZZ_HIDDEN_DECKS=1 (hidden decklists; games only)')
    ap.add_argument('--games', type=int, required=True, help='games in all (fuzz_random_games)')
    ap.add_argument('--matches', type=int, default=None, help='matches in all (default max(20, games / 20), the test\'s)')
    ap.add_argument('--first', type=int, default=0, help='the first seed (default 0)')
    ap.add_argument('--shards', type=int, default=8, help='shards per loop (default 8)')
    ap.add_argument('--jobs', type=int, default=None, help='processes at once (default --shards)')
    ap.add_argument('--target', default='fuzz', help='the test target holding the fuzz tests (default fuzz)')
    ap.add_argument('--exe', default=None, help='that test binary (default: built and found through cargo)')
    ap.add_argument('--out', default=None, help='directory for the logs and the merged totals (default: a temp dir)')
    ap.add_argument('--timeout', type=float, default=3600.0, help='seconds per shard (default 3600)')
    ap.add_argument('--belief', type=float, default=None, help="FUZZ_BELIEF=T in every shard (the belief-weighted deal's checks)")
    ap.add_argument('--chance', type=int, default=None, help='FUZZ_CHANCE=N in every shard (a chance walk one decision in N)')
    a = ap.parse_args(argv)
    t0 = time.time()
    stray = stray_settings()
    if stray:
        print(f"fuzz_shards: the environment holds {', '.join(stray)}: a shard takes none of the caller's FUZZ_* settings, "
              f'and dropping them silently would run something else than asked - unset them and pass the flags '
              f'(--pool, --hidden, --games, --matches, --first, --belief, --chance)', file=sys.stderr)
        return 2
    try:
        settings = flag_settings(a.belief, a.chance)
    except SetupError as e:
        print(f'fuzz_shards: {e}', file=sys.stderr)
        return 2
    if a.games < 1 or a.shards < 1:
        print('fuzz_shards: --games and --shards must be >= 1', file=sys.stderr)
        return 2
    matches = a.matches if a.matches is not None else max(20, a.games // 20)
    if a.hidden:
        matches = 0                     # (hidden decklists are a games mode: the matches loop does not read it)
    # (absolute: the test processes run in engine/, the caller may name a path relative to the checkout)
    out = os.path.abspath(a.out or tempfile.mkdtemp(prefix=f'fuzz-shards-{a.pool}-'))
    os.makedirs(out, exist_ok=True)
    pool_env = {} if a.pool == 'default' else {'FUZZ_POOL': a.pool}
    if a.hidden:
        pool_env['FUZZ_HIDDEN_DECKS'] = '1'
    label = a.pool + (' +hidden decklists' if a.hidden else '') + ''.join(f' {k}={v}' for k, v in settings.items())
    try:
        exe = a.exe or find_exe(a.target, {**os.environ})
        names = list_tests(exe)
        t_games, t_matches = test_named(names, 'fuzz_random_games'), test_named(names, 'fuzz_random_matches')
        t_totals = test_named(names, 'fuzz_shard_totals')
    except SetupError as e:
        print(f'fuzz_shards: {e}', file=sys.stderr)
        return 2
    print(f'fuzz shards: {label}, {a.games} games + {matches} matches from seed {a.first}, {a.shards} shards a loop, '
          f'{a.jobs or a.shards} at once; {exe}; logs {out}', flush=True)
    jobs = []
    plan = {'games': split_ranges(a.first, a.games, a.shards), 'matches': split_ranges(a.first, matches, a.shards)}
    for kind, test, count_var in (('games', t_games, 'FUZZ_GAMES'), ('matches', t_matches, 'FUZZ_MATCHES')):
        for k, (f, n) in enumerate(plan[kind]):
            env = child_env({**pool_env, **settings, 'FUZZ_SHARD': '1', 'FUZZ_FIRST': str(f), count_var: str(n)})
            j = Job(f'{kind}[{f},{f + n})', [exe, test, '--exact', '--nocapture', '--test-threads=1'], env,
                    os.path.join(out, f'{kind}-{k:02d}.log'))
            j.kind, j.first, j.n = kind, f, n
            jobs.append(j)
    run_jobs(jobs, a.jobs or a.shards, a.timeout, stop_on_fail=True)
    problems, merged = [], {}
    for kind in ('games', 'matches'):
        own = [j for j in jobs if j.kind == kind]
        parts, posts = [], []
        for j in own:
            why, tots = shard_problem(j, kind, j.first, j.n)
            if why:
                problems.append(f'{j.name}: {why} (log {j.log})')
                continue
            parts += [t for t in tots if t.get('kind') == kind]
            posts += [t for t in tots if t.get('kind') == 'post']
        if problems or not own:
            continue
        m, p = merge(kind, parts, a.first, sum(j.n for j in own))
        problems += p
        merged[kind] = m
        if kind == 'games' and m is not None and 'skipped' not in m:
            problems += settings_at_work(settings, m.get('totals'))
        if kind == 'matches' and posts:
            if len(posts) != len(own):
                problems.append(f'post: {len(posts)} of {len(own)} matches shards played post-sideboard matches')
            else:
                m, p = merge('post', posts, a.first, matches)
                problems += p
                merged['post'] = m
    secs_shards = max([j.seconds for j in jobs] or [0])
    checked = []
    if not problems:
        mpath = os.path.join(out, 'merged_totals.json')
        with open(mpath, 'w', encoding='utf-8') as f:
            json.dump({k: v for k, v in merged.items() if 'skipped' not in v}, f, indent=1, sort_keys=True)
        final = [Job('merged checks', [exe, t_totals, '--exact', '--nocapture', '--test-threads=1'],
                     child_env({**pool_env, **settings, 'FUZZ_TOTALS_FILE': mpath}), os.path.join(out, 'merged-checks.log')),
                 Job('the rest of the fuzz binary', [exe, '--skip', t_games, '--skip', t_matches, '--skip', t_totals,
                                                     '--nocapture'],
                     child_env(dict(pool_env)), os.path.join(out, 'rest.log'))]
        run_jobs(final, 2, a.timeout, stop_on_fail=False)
        mc, rest = final
        want = sorted(k for k, v in merged.items() if 'skipped' not in v)
        got = CHECKED_RE.findall(mc.out)
        r = TEST_RESULT_RE.findall(mc.out)
        if mc.rc != 0 or sorted(k for k, _ in got) != want or len(r) != 1 or r[0][0] != 'ok':
            problems.append(f'the merged end-of-run checks: exit {mc.rc}, checked {got} (want {want}): '
                            f'{(SEED_FAIL_RE.search(mc.out) or re.search(r"panicked at[^\n]*\n[^\n]*", mc.out) or [""])[0][:400]} '
                            f'(log {mc.log})')
        else:
            for k, n in got:
                if int(n) != merged[k]['n']:
                    problems.append(f'the merged checks of {k} ran over {n} seeds, not {merged[k]["n"]}')
            checked = [f'{k} over {n}' for k, n in got]
        rr = TEST_RESULT_RE.findall(rest.out)
        if rest.rc != 0 or len(rr) != 1 or rr[0][0] != 'ok' or int(rr[0][1]) == 0 or int(rr[0][2]):
            problems.append(f'the rest of the fuzz binary: exit {rest.rc}, {rr} (log {rest.log})')
        rest_passed = int(rr[0][1]) if rr else 0
    else:
        rest_passed = 0
    secs = time.time() - t0
    summary = {'pool': a.pool, 'hidden': a.hidden, 'first': a.first, 'games': a.games, 'matches': matches,
               'shards': a.shards, 'passed': not problems, 'seconds': round(secs, 1),
               'slowest_shard_s': round(secs_shards, 1), 'checked': checked, 'rest_passed': rest_passed,
               'post': merged.get('post', {}).get('n'), 'matches_skipped': (merged.get('matches') or {}).get('skipped'),
               'logs': out, 'problems': problems[:20], 'settings': settings,
               'belief_worlds': ((merged.get('games') or {}).get('totals') or {}).get('belief_worlds'),
               'chance_walks': ((merged.get('games') or {}).get('totals') or {}).get('chance_walks')}
    if problems:
        print(f'fuzz shards: FAILED - {label}: ' + '; '.join(problems[:5]) + (f' (+{len(problems) - 5} more)' if len(problems) > 5 else ''))
    else:
        mt = ('matches skipped: ' + summary['matches_skipped']) if summary['matches_skipped'] else f'{matches} matches'
        if summary['post']:
            mt += f' + {summary["post"]} post-sideboard'
        at_work = ''.join(f'; {c} {summary[c]}' for c, v in (('belief_worlds', 'FUZZ_BELIEF'), ('chance_walks', 'FUZZ_CHANCE'))
                          if v in settings)
        print(f'fuzz shards: PASSED - {label}: {a.games} games, {mt} in {len(jobs)} shards from seed {a.first}; merged '
              f'checks: {", ".join(checked)}{at_work}; {rest_passed} other fuzz tests; {secs:.0f}s (slowest shard {secs_shards:.0f}s)')
    print(json.dumps({'fuzz_shards': summary}), flush=True)
    return 0 if not problems else 1


if __name__ == '__main__':
    sys.exit(main())
