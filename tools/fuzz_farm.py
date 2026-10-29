"""The fuzz farm (docs/ENGINE.md 50.2): the Mac fuzzes the latest mtg-zero main for ever, at nice 19,
backing off while a remote evaluation runs there.

Each **round**: fetch the Mac's bare repo (`~/mtg-zero.git`; the desktop pushes main there with `git
push mac main`, or `bash tools/mac.sh true` from main's checkout, which also pushes zero-core's pin),
and if main moved, check it out in the farm's own clone (`~/mtgz-fuzzfarm/src`) and build it (its own
target dir `~/cargo-target/mtg-zero/fuzzfarm`: the PyO3 module and the fuzz test binaries); then run
one chunk of every mode, `jobs` at a time, each with **new seeds** (a cursor per mode, never rewound):

| mode | what | seeds |
|---|---|---|
| games, matches | engine/tests/fuzz.rs `fuzz_random_games` / `fuzz_random_matches` (random decks; best-of-three) | `FUZZ_FIRST`, `FUZZ_GAMES` / `FUZZ_MATCHES` |
| interactions-games, interactions-matches | the same with `FUZZ_POOL=interactions` | same |
| drafted-matches | `FUZZ_POOL=drafted` (drafted events' matches; its games are the default mode's) | same |
| exponential-games | `FUZZ_POOL=exponential` (its matches are the default mode's) | same |
| draft-events | engine/tests/draft.rs `fuzz_random_events` (`DRAFT_FUZZ`) | `DRAFT_FUZZ_FIRST` (skipped while main lacks it) |
| smoke-team_draft, smoke-team_draft_2v2, smoke-match, smoke-random | tools/search_smoke.py (uniform evaluator, the runs' self-play seats) | `--seeds` (driver seeds) |

A chunk aims at `chunk_s` seconds of running time (each mode's measured rate). A failure is recorded
in `failures/<id>.json` (+ the job's output, the first 3 times), deduplicated by its panic site and
message (digits masked): the mode, the seeds, main's commit, the panic and its **reproduction**; a
fuzz chunk that fails at a seed goes on from the next seed; a crash without a seed (an abort) is
bisected down to its seed. `status.json` holds the rounds, what each mode fuzzed (per hour of its
running time and per hour of the farm), the back-offs and the failures; `python fuzz_farm.py status`
prints it (`tools/fuzz_farm_status.sh` from the desktop).

Control (`~/mtgz-fuzzfarm/`): `config.json` (read every round: `ref`, `jobs`, `chunk_s`,
`smoke_threads`, `modes`, `expected`, `guaranteed_jobs`, `guaranteed_window`), `PAUSE` (stop the
jobs, like a back-off), `STOP` (end: the LaunchAgent does not restart a clean exit).
`tools/fuzz_farm.sh` installs and controls the user LaunchAgent `com.mtgzero.fuzzfarm` (KeepAlive:
restarted after a crash; started at login).

**The guaranteed slice** (tiered gates, docs/ENGINE.md 70): by default the farm backs ALL its jobs
off (SIGSTOP) while a run's remote self-play holds a busy marker on the Mac
(`mac_backoff.SELFPLAY_BUSY`) - zero farm throughput for however long that phase runs. Setting
`guaranteed_jobs` (an int) keeps that many of the round's jobs running at nice 19 through a plain
self-play busy marker (they still back off for a real evaluation lock, or PAUSE); nice 19 already
tells the OS scheduler to prefer self-play's normal-priority process. `guaranteed_window`
("HH:MM-HH:MM", local time, wraps past midnight) makes EVERY job guaranteed during that window
instead (a nightly slice when self-play is less likely running) - set one or the other, or
neither (0 / null: today's behaviour, unchanged). Either way the guaranteed job(s) still draw from
each mode's normal advancing cursor (fresh seeds every round, never a fixed range) - only whether
they back off differs. Both default to off; this was NOT started on the Mac (A/B round 3 is live
there) - `python tools/fuzz_farm.py simulate-guaranteed` checks the decision logic locally instead.

    python fuzz_farm.py run          # the farm (the LaunchAgent runs this)
    python fuzz_farm.py status [--since UNIX_TIME] [--json]
    python fuzz_farm.py once MODE N  # one chunk of a mode now, at the cursor (a check)
    python fuzz_farm.py simulate-guaranteed   # the guaranteed slice's decision logic, locally (no Mac, no build)
"""
import argparse
import hashlib
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import mac_backoff as mb  # noqa: E402

HOME = os.path.expanduser('~')
FARM = os.environ.get('MTGZ_FARM', os.path.join(HOME, 'mtgz-fuzzfarm'))
BARE = os.path.join(HOME, 'mtg-zero.git')
CORE = os.path.join(HOME, 'mtg-zero-core.git')
VENV = os.path.join(HOME, '.venvs', 'dominion-zero')
TARGET = os.environ.get('MTGZ_FARM_TARGET', os.path.join(HOME, 'cargo-target', 'mtg-zero', 'fuzzfarm'))
SRC = os.path.join(FARM, 'src')

CONFIG = {
    'ref': 'main',                 # the branch of ~/mtg-zero.git the farm fuzzes
    'jobs': 4,                     # jobs at a time (a fuzz job is one thread, a smoke job smoke_threads)
    'chunk_s': 600,                # running seconds a chunk aims at
    'smoke_threads': 4,
    'build_jobs': 6,
    'poll': 5,                     # seconds between back-off checks
    'modes': None,                 # None: all of MODES
    'timeout_factor': 5.0,         # a job running this many times its expected time (at least an hour) is a hang
    # failures whose site or message matches one of these are marked expected (known, being fixed)
    'expected': ['PayMana', 'Griselbrand', "Green Sun's Zenith"],
    # the guaranteed slice (tiered gates, docs/ENGINE.md 70): this many of `jobs` back off only for
    # a hard lock or PAUSE, never a plain self-play busy marker (mac_backoff.Governor's docs) - 0
    # (off, today's behaviour) unless set in config.json. `guaranteed_window`, e.g.
    # "02:00-06:00" (local time, HH:MM-HH:MM, wraps past midnight): during it every job is
    # guaranteed, overriding `guaranteed_jobs` for that round; None (off) unless set.
    'guaranteed_jobs': 0,
    'guaranteed_window': None,
}

# mode -> kind, test binary / smoke spec, test name, env, unit, initial rate (units per running second), minimum chunk
MODES = {
    'games': dict(kind='fuzz', bin='fuzz', test='fuzz_random_games', env={}, unit='games', rate=60.0, min=1000),
    'matches': dict(kind='fuzz', bin='fuzz', test='fuzz_random_matches', env={}, unit='matches', rate=6.0, min=50),
    'interactions-games': dict(kind='fuzz', bin='fuzz', test='fuzz_random_games', env={'FUZZ_POOL': 'interactions'},
                               unit='games', rate=40.0, min=1000),
    'interactions-matches': dict(kind='fuzz', bin='fuzz', test='fuzz_random_matches', env={'FUZZ_POOL': 'interactions'},
                                 unit='matches', rate=5.0, min=50),
    'drafted-matches': dict(kind='fuzz', bin='fuzz', test='fuzz_random_matches', env={'FUZZ_POOL': 'drafted'},
                            unit='matches', rate=20.0, min=50),
    'exponential-games': dict(kind='fuzz', bin='fuzz', test='fuzz_random_games', env={'FUZZ_POOL': 'exponential'},
                              unit='games', rate=2.0, min=100),
    'draft-events': dict(kind='draft', bin='draft', test='fuzz_random_events', env={}, unit='events', rate=60.0, min=50),
    'smoke-team_draft': dict(kind='smoke', spec='team_draft', unit='seeds', rate=1 / 60, min=1),
    'smoke-team_draft_2v2': dict(kind='smoke', spec='team_draft_2v2', unit='seeds', rate=1 / 30, min=1),
    'smoke-match': dict(kind='smoke', spec='match', unit='seeds', rate=1 / 120, min=1),
    'smoke-random': dict(kind='smoke', spec='random', unit='seeds', rate=1 / 60, min=1),
}
SEED_BASE = {'fuzz': 10_000_000, 'draft': 10_000_000, 'smoke': 1000}
COUNT_VAR = {'fuzz_random_games': 'FUZZ_GAMES', 'fuzz_random_matches': 'FUZZ_MATCHES', 'fuzz_random_events': 'DRAFT_FUZZ'}
FIRST_VAR = {'fuzz': 'FUZZ_FIRST', 'draft': 'DRAFT_FUZZ_FIRST'}
MAX_CONTINUATIONS = 200            # per mode and round (a failing chunk's restarts after its failing seeds)

STOP = False


def path(*p):
    return os.path.join(FARM, *p)


def now_iso(t=None):
    return time.strftime('%Y-%m-%d %H:%M:%S', time.localtime(t or time.time()))


def log(msg):
    os.makedirs(path('logs'), exist_ok=True)
    with open(path('logs', 'farm.log'), 'a', encoding='utf-8') as f:
        f.write(f'{now_iso()} {msg}\n')


def read_json(p, default):
    try:
        with open(p, encoding='utf-8') as f:
            return json.load(f)
    except (OSError, ValueError):
        return default


def write_json(p, obj):
    with open(p + '.tmp', 'w', encoding='utf-8') as f:
        json.dump(obj, f, indent=1)
    os.replace(p + '.tmp', p)


def config():
    c = dict(CONFIG)
    c.update(read_json(path('config.json'), {}))
    return c


def in_guaranteed_window(cfg, now=None):
    """Whether `now` (local time, default this moment) falls in `guaranteed_window`
    ("HH:MM-HH:MM"; wraps past midnight, e.g. "22:00-06:00"); False if unset or malformed (a bad
    config value backs off as today, it never silently runs the whole farm unthrottled)."""
    w = cfg.get('guaranteed_window')
    if not w:
        return False
    try:
        a, b = w.split('-')
        start = time.strptime(a.strip(), '%H:%M')
        end = time.strptime(b.strip(), '%H:%M')
    except ValueError:
        log(f'config: bad guaranteed_window {w!r} (want "HH:MM-HH:MM"): ignored')
        return False
    t = time.localtime(now) if now is not None else time.localtime()
    cur = (t.tm_hour, t.tm_min)
    lo, hi = (start.tm_hour, start.tm_min), (end.tm_hour, end.tm_min)
    return (lo <= cur < hi) if lo <= hi else (cur >= lo or cur < hi)


def base_env(cfg):
    env = dict(os.environ)
    env['PATH'] = ':'.join([os.path.join(VENV, 'bin'), os.path.join(HOME, '.cargo', 'bin'), '/opt/homebrew/bin',
                            '/usr/local/bin', '/usr/bin', '/bin', '/usr/sbin', '/sbin'])
    env.update(CARGO_TARGET_DIR=TARGET, CARGO_BUILD_JOBS=str(cfg['build_jobs']), PYTHON='python', PYO3_PYTHON='python',
               RUSTFLAGS='-C link-arg=-undefined -C link-arg=dynamic_lookup', RUST_BACKTRACE='0',
               VIRTUAL_ENV=VENV)
    for k in ('FUZZ_POOL', 'FUZZ_FIRST', 'FUZZ_GAMES', 'FUZZ_MATCHES', 'FUZZ_LOG', 'DRAFT_FUZZ', 'DRAFT_FUZZ_FIRST',
              'DRAFT_SCALE'):
        env.pop(k, None)
    return env


def git(*args, cwd=SRC, check=True):
    p = subprocess.run(['git', *args], cwd=cwd, capture_output=True, text=True)
    if check and p.returncode:
        raise RuntimeError(f'git {" ".join(args)}: {p.stderr.strip()[-500:]}')
    return p.stdout.strip()


# ---------------------------------------------------------------------------- the farm's state

class State:
    def __init__(self):
        self.d = read_json(path('state.json'), {})
        self.d.setdefault('cursors', {})
        self.d.setdefault('rates', {})
        self.d.setdefault('modes', {})
        self.d.setdefault('round', 0)
        self.d.setdefault('built', None)       # {sha, subject, time, executables, draft_first}
        self.d.setdefault('started', time.time())
        self.d.setdefault('wall_s', 0.0)       # the farm's running wall time over its lives
        self.d.setdefault('backoff', {'episodes': 0, 'paused_s': 0.0})
        self.d.setdefault('guaranteed_backoff', {'episodes': 0, 'paused_s': 0.0})

    def save(self):
        write_json(path('state.json'), self.d)

    def cursor(self, mode):
        m = MODES[mode]
        return self.d['cursors'].get(mode, SEED_BASE[m['kind']])

    def rate(self, mode):
        return self.d['rates'].get(mode, MODES[mode]['rate'])

    def mstats(self, mode):
        return self.d['modes'].setdefault(mode, {'runs': 0, 'units': 0, 'active_s': 0.0, 'failures': 0, 'extra': {},
                                                  'last': None})


# ---------------------------------------------------------------------------- failures

KEY_CHARS = 120                    # a failure's key: its panic site and its message's first characters


def norm(s):
    return re.sub(r'\d+', '#', s.strip())[:300]


def failure_key(site, message):
    """The dedup key: the panic site and the first KEY_CHARS characters of the message's first line with
    its digits masked (a panic that dumps its state, as PayMana's does, differs by board after that)."""
    first = next((x for x in message if x), '') if message else ''
    return f'{site}\n{norm(first)[:KEY_CHARS]}'


def rekey_failures():
    """Failures recorded under another key function (an older farm): renamed to their current id, merged
    with a failure already there."""
    d = path('failures')
    if not os.path.isdir(d):
        return
    for n in sorted(os.listdir(d)):
        if not n.endswith('.json'):
            continue
        f = read_json(os.path.join(d, n), None)
        if not f:
            continue
        key = failure_key(f['site'], f.get('message') or [])
        fid = hashlib.sha1(key.encode()).hexdigest()[:12]
        if fid == f['id']:
            continue
        g = read_json(os.path.join(d, f'{fid}.json'), None)
        if g:
            g['count'] += f['count']
            g['first_seen'] = min(g['first_seen'], f['first_seen'])
            g['last_seen'] = max(g['last_seen'], f['last_seen'])
            g['modes'] = sorted(set(g['modes']) | set(f['modes']))
            g['shas'] = sorted(set(g['shas']) | set(f['shas']))
            g['seeds'] = (g['seeds'] + f['seeds'])[:50]
        else:
            g = dict(f, id=fid, key=key)
            for k in range(1, 4):
                old_log = os.path.join(d, f'{f["id"]}-{k}.log')
                if os.path.exists(old_log):
                    os.replace(old_log, os.path.join(d, f'{fid}-{k}.log'))
        write_json(os.path.join(d, f'{fid}.json'), g)
        os.remove(os.path.join(d, n))
        log(f'failure {f["id"]} re-keyed as {fid}')


PANIC = re.compile(r"thread '([^']*)'(?: \(\d+\))? panicked at ([^\s]+?):(\d+):(\d+):?\s*$")
RERUN = re.compile(r'\b(?:seed|match|event) (\d+) failed \(rerun: ([^)]*)\)')
MATCH_PANIC = re.compile(r'MTG ENGINE PANIC in a heuristic-played match: event (\d+) match (\d+) \(seed \d+\): ([^\n]*)')


def panics(text):
    """[(site 'file:line', message lines)] of every panic in `text` (Rust's hook format; also the
    search smoke's indented copies)."""
    lines = [x.strip() for x in text.splitlines()]
    out = []
    i = 0
    while i < len(lines):
        m = PANIC.search(lines[i])
        if not m:
            i += 1
            continue
        site = f'{m.group(2)}:{m.group(3)}'
        msg = []
        i += 1
        while i < len(lines) and len(msg) < 40:
            x = lines[i]
            if PANIC.search(x) or x.startswith(('note: run with', 'stack backtrace:', 'test result:', 'failures:',
                                                'error: test failed', 'ok    seed', 'FAIL (', 'HANG', 'BAD  ',
                                                'search smoke:')) or re.match(r'^test \S+ \.\.\. ', x):
                break
            msg.append(x)
            i += 1
        while msg and not msg[-1]:
            msg.pop()
        out.append((site, msg))
    return out


def record_failure(cfg, mode, sha, site, message, repro, seed, output, kind='panic'):
    os.makedirs(path('failures'), exist_ok=True)
    first = next((x for x in message if x), '') if message else ''
    key = failure_key(site, message)
    fid = hashlib.sha1(key.encode()).hexdigest()[:12]
    p = path('failures', f'{fid}.json')
    f = read_json(p, None)
    t = time.time()
    if f is None:
        expected = any(e in site or any(e in x for x in message) for e in cfg.get('expected') or [])
        f = {'id': fid, 'kind': kind, 'site': site, 'message': message[:40], 'key': key, 'modes': [], 'count': 0,
             'first_seen': t, 'last_seen': t, 'repro': repro, 'sha': sha, 'seeds': [], 'shas': [],
             'expected': expected}
        log(f'NEW failure {fid} ({mode}, {"expected" if expected else "unexpected"}): {site}: {first[:200]}')
    f['count'] += 1
    f['last_seen'] = t
    if mode not in f['modes']:
        f['modes'].append(mode)
    if sha[:12] not in f['shas']:
        f['shas'].append(sha[:12])
    if seed is not None and len(f['seeds']) < 50:
        f['seeds'].append({'mode': mode, 'seed': seed, 'sha': sha[:12]})
    if f['count'] <= 3:
        with open(path('failures', f'{fid}-{f["count"]}.log'), 'w', encoding='utf-8') as g:
            g.write(f'mode {mode}, {sha}, seed {seed}\nrepro: {repro}\n\n')
            g.write('\n'.join(output.splitlines()[-400:]))
    write_json(p, f)
    return fid


def failures():
    d = path('failures')
    if not os.path.isdir(d):
        return []
    out = [read_json(os.path.join(d, n), None) for n in os.listdir(d) if n.endswith('.json')]
    return sorted((f for f in out if f), key=lambda f: -f['last_seen'])


# ---------------------------------------------------------------------------- source and build

class Job:
    def __init__(self, mode, first, n, cont=0, bisect=False, hang=False, guaranteed=False):
        self.mode, self.first, self.n, self.cont, self.bisect = mode, first, n, cont, bisect
        self.hang = hang                            # a half of a hung chunk: a tight time limit
        self.guaranteed = guaranteed                # the guaranteed slice: backs off for less (mac_backoff.Governor)
        self.p = None
        self.t0 = None
        self.paused0 = 0.0
        self.logpath = None
        self.expected_s = None

    def describe(self):
        return f'{self.mode} [{self.first}+{self.n}]' + (' (bisect)' if self.bisect else '') \
            + (' (guaranteed)' if self.guaranteed else '')


def run_governed(cmd, cwd, env, gov, logpath, timeout_active=None):
    """Run `cmd` to completion under the governor; -> (returncode, output)."""
    with open(logpath, 'w', encoding='utf-8') as out:
        p = mb.spawn(cmd, gov, cwd=cwd, env=env, stdout=out, stderr=subprocess.STDOUT)
        jobs_file_add(p.pid, cmd)
        t0, paused0 = time.time(), gov.paused_total()
        try:
            while p.poll() is None:
                if STOP:
                    mb.kill_group(p.pid)
                elif timeout_active and time.time() - t0 - (gov.paused_total() - paused0) > timeout_active:
                    mb.kill_group(p.pid)
                time.sleep(1)
        finally:
            gov.unregister(p.pid)
            jobs_file_remove(p.pid)
    with open(logpath, encoding='utf-8', errors='replace') as f:
        return p.returncode, f.read()


def refresh(cfg, state, gov):
    """Fetch the farm's ref; check out and build a new commit. -> the built commit (or None)."""
    ref = cfg['ref']
    if not os.path.isdir(os.path.join(SRC, '.git')):
        os.makedirs(FARM, exist_ok=True)
        subprocess.run(['git', 'clone', '-q', '--no-checkout', BARE, SRC], check=True)
    git('config', 'submodule.third_party/zero-core.url', CORE)
    try:
        git('fetch', '-q', 'origin', f'+refs/heads/{ref}:refs/remotes/origin/{ref}')
    except RuntimeError as e:
        log(f'fetch failed: {e}')
    try:
        sha = git('rev-parse', f'refs/remotes/origin/{ref}')
    except RuntimeError as e:
        log(f'no {ref} in {BARE}: {e}')
        return (state.d['built'] or {}).get('sha')
    built = state.d['built']
    if built and built['sha'] == sha:
        return sha
    if state.d.get('failed_build') == sha:          # this commit failed to build: keep the last build
        return built['sha'] if built else None
    log(f'building {ref} {sha[:12]}')
    write_status(state, gov, [], f'building {sha[:12]}')
    prev = built['sha'] if built else None
    env = base_env(cfg)
    try:
        git('checkout', '-q', '-f', '--detach', sha)
        git('-c', 'protocol.file.allow=always', 'submodule', 'update', '-q', '--init', 'third_party/zero-core')
    except RuntimeError as e:
        return build_failed(cfg, state, sha, prev, f'checkout: {e}')
    os.makedirs(path('logs'), exist_ok=True)
    t0 = time.time()
    rc, out = run_governed(['bash', 'tools/build_py.sh'], SRC, env, gov, path('logs', 'build_py.log'))
    if rc:
        return build_failed(cfg, state, sha, prev, 'tools/build_py.sh failed:\n' + out[-3000:])
    rc, out = run_governed(['cargo', 'test', '--release', '-p', 'engine', '--test', 'fuzz', '--test', 'draft', '--no-run',
                            '--message-format=json'], SRC, env, gov, path('logs', 'build_tests.log'))
    exes = {}
    for line in out.splitlines():
        if line.startswith('{'):
            try:
                m = json.loads(line)
            except ValueError:
                continue
            if m.get('reason') == 'compiler-artifact' and m.get('executable') and 'test' in m['target']['kind']:
                exes[m['target']['name']] = m['executable']
    if rc or 'fuzz' not in exes or 'draft' not in exes:
        return build_failed(cfg, state, sha, prev, 'cargo test --no-run failed:\n' + out[-3000:])
    with open(os.path.join(SRC, 'engine', 'tests', 'draft.rs'), encoding='utf-8') as f:
        draft_first = 'DRAFT_FUZZ_FIRST' in f.read()
    subject = git('log', '-1', '--format=%s', sha)
    state.d['built'] = {'sha': sha, 'subject': subject[:200], 'time': time.time(), 'executables': exes,
                        'draft_first': draft_first, 'build_s': round(time.time() - t0)}
    state.d.pop('failed_build', None)
    state.save()
    log(f'built {sha[:12]} in {time.time() - t0:.0f} s ({subject[:100]}); draft fuzz seed offset: {draft_first}')
    return sha


def build_failed(cfg, state, sha, prev, why):
    log(f'build of {sha[:12]} failed: {why.splitlines()[0][:200]}')
    record_failure(cfg, 'build', sha, 'build', [f'build of {sha[:12]} failed'] + why.splitlines()[-30:],
                   f'git checkout {sha[:12]} && bash tools/build_py.sh && cargo test --release -p engine --test fuzz --no-run',
                   None, why, kind='build')
    state.d['failed_build'] = sha
    state.save()
    if prev:
        try:
            git('checkout', '-q', '-f', '--detach', prev)
            git('-c', 'protocol.file.allow=always', 'submodule', 'update', '-q', '--init', 'third_party/zero-core')
        except RuntimeError as e:
            log(f'could not go back to {prev[:12]}: {e}')
            return None
        return prev
    return None


# ---------------------------------------------------------------------------- jobs

def jobs_file_add(pid, cmd):
    d = read_json(path('jobs.json'), {})
    d[str(pid)] = {'cmd': cmd, 't': time.time()}
    write_json(path('jobs.json'), d)


def jobs_file_remove(pid):
    d = read_json(path('jobs.json'), {})
    d.pop(str(pid), None)
    write_json(path('jobs.json'), d)


def kill_orphans():
    """Process groups a previous life of the farm left (it was killed): continue and kill them."""
    d = read_json(path('jobs.json'), {})
    for pid, j in d.items():
        pid = int(pid)
        try:
            os.kill(pid, 0)
        except OSError:
            continue
        try:
            cmdline = subprocess.run(['ps', '-o', 'command=', '-p', str(pid)], capture_output=True, text=True).stdout
        except OSError:
            cmdline = ''
        exe = os.path.basename(j['cmd'][0]) if j.get('cmd') else '?'
        if exe and exe in cmdline:
            log(f'killing an orphaned job group {pid}: {" ".join(j["cmd"])[:200]}')
            mb.kill_group(pid)
    write_json(path('jobs.json'), {})


def plan(cfg, state):
    built = state.d['built']
    modes = cfg['modes'] or list(MODES)
    window = in_guaranteed_window(cfg)
    n_guaranteed = len(modes) if window else max(0, int(cfg.get('guaranteed_jobs', 0) or 0))
    jobs = []
    for mode in modes:
        if mode not in MODES:
            log(f'config: unknown mode {mode!r}')
            continue
        m = MODES[mode]
        if m['kind'] == 'draft' and not built.get('draft_first'):
            state.mstats(mode)['skipped'] = 'the built commit has no DRAFT_FUZZ_FIRST (ENGINE.md 50.2)'
            continue
        state.mstats(mode).pop('skipped', None)
        n = max(m['min'], int(state.rate(mode) * cfg['chunk_s']))
        if m['kind'] == 'smoke':
            n = min(n, 16)
        # the guaranteed slice always draws from the SAME advancing cursor as every other job
        # (fresh seeds, never a separate fixed range) - only whether it backs off differs.
        jobs.append(Job(mode, state.cursor(mode), n, guaranteed=len(jobs) < n_guaranteed))
    return jobs


def start(cfg, state, job, gov):
    m = MODES[job.mode]
    env = base_env(cfg)
    exes = state.d['built']['executables']
    if m['kind'] in ('fuzz', 'draft'):
        env.update(m['env'])
        env[FIRST_VAR[m['kind']]] = str(job.first)
        env[COUNT_VAR[m['test']]] = str(job.n)
        cmd = [exes[m['bin']], m['test'], '--exact', '--nocapture', '--test-threads=1']
        cwd = os.path.join(SRC, 'engine')
    else:
        cmd = [os.path.join(VENV, 'bin', 'python'), 'tools/search_smoke.py', '--specs', m['spec'], '--seeds',
               f'{job.first}-{job.first + job.n - 1}', '--threads', str(cfg['smoke_threads']), '--stall', '1e7',
               '--timeout', '1e6']
        cwd = SRC
    os.makedirs(path('logs', 'jobs'), exist_ok=True)
    job.logpath = path('logs', 'jobs', f'{job.mode}.log')
    job.out = open(job.logpath, 'w', encoding='utf-8')
    job.out.write(f'# {job.describe()} at {state.d["built"]["sha"][:12]}: {" ".join(cmd)}\n# env: '
                  + ' '.join(f'{k}={env[k]}' for k in sorted(set(m.get('env', {})) | {FIRST_VAR.get(m['kind'], ''),
                             COUNT_VAR.get(m.get('test', ''), '')}) if k in env) + '\n')
    job.out.flush()
    job.p = mb.spawn(cmd, gov, guaranteed=job.guaranteed, cwd=cwd, env=env, stdout=job.out, stderr=subprocess.STDOUT)
    jobs_file_add(job.p.pid, cmd)
    job.t0, job.paused0 = time.time(), gov.paused_total(guaranteed=job.guaranteed)
    job.expected_s = job.n / max(state.rate(job.mode), 1e-9)
    job.env = {k: env[k] for k in list(m.get('env', {})) + [FIRST_VAR.get(m['kind'], ''), COUNT_VAR.get(m.get('test', ''), '')]
               if k in env}
    job.cmd = cmd


def hang_limit(cfg, job):
    """Running seconds after which a job counts as hung: generous for a chunk (timeout_factor x its
    expected time, at least an hour), tight for the halves of a hung chunk (4 x + 2 minutes)."""
    exp = job.expected_s or 0
    if job.hang:
        return 120.0 + 4 * exp
    return max(3600.0, cfg['timeout_factor'] * exp)


def active_s(job, gov):
    return time.time() - job.t0 - (gov.paused_total(guaranteed=job.guaranteed) - job.paused0)


def repro_of(job, seed, sha):
    m = MODES[job.mode]
    at = f'(at {sha[:12]}) '
    if m['kind'] in ('fuzz', 'draft'):
        env = ' '.join(f'{k}={v}' for k, v in m['env'].items())
        first, count = FIRST_VAR[m['kind']], COUNT_VAR[m['test']]
        extra = ' FUZZ_LOG=1' if m['test'] == 'fuzz_random_games' else ''
        test = 'fuzz' if m['kind'] == 'fuzz' else 'draft'
        return (at + f'{env + " " if env else ""}{first}={seed} {count}=1{extra} cargo test --release -p engine '
                     f'--test {test} {m["test"]} -- --exact --nocapture').strip()
    return at + f'python tools/search_smoke.py --specs {m["spec"]} --seeds {seed}'


def finish(cfg, state, job, gov, queue, killed=False):
    """A job ended: parse it, record failures, move the cursor, maybe queue its continuation."""
    job.out.close()
    gov.unregister(job.p.pid)
    jobs_file_remove(job.p.pid)
    act = active_s(job, gov)
    with open(job.logpath, encoding='utf-8', errors='replace') as f:
        out = f.read()
    m = MODES[job.mode]
    sha = state.d['built']['sha']
    st = state.mstats(job.mode)
    rc = job.p.returncode
    done = job.n
    nfail = 0
    note = ''
    if m['kind'] == 'smoke':
        done, nfail, extra = parse_smoke(cfg, job, out, sha, killed)
        for k, v in extra.items():
            st['extra'][k] = st['extra'].get(k, 0) + v
        note = ', '.join(f'{v} {k}' for k, v in extra.items())
    elif killed and job.n > 1 and job.cont < MAX_CONTINUATIONS:
        # a hung chunk (a game that never ends, or one that takes far too long): bisect it down to its
        # seed, each half with a tight limit (hang_limit)
        h = job.n // 2
        queue.appendleft(Job(job.mode, job.first + h, job.n - h, job.cont + 1, bisect=True, hang=True))
        queue.appendleft(Job(job.mode, job.first, h, job.cont + 1, bisect=True, hang=True))
        log(f'{job.describe()} hung ({act:.0f} s running): bisecting')
        done = 0
    elif killed:
        record_failure(cfg, job.mode, sha, f'HANG {job.mode}', [f'no exit after {act:.0f} s of running time '
                       f'(expected {job.expected_s:.0f} s): seeds {job.first}..{job.first + job.n - 1}'],
                       repro_of(job, job.first, sha) + (f' (seeds {job.first}-{job.first + job.n - 1})' if job.n > 1 else ''),
                       job.first, out, kind='hang')
        nfail = 1
    elif rc == 0:
        mo = re.search(r'(?m)(?:^|\.\.\. )(\d+ (?:games|matches|events)[:,].*)$', out)
        note = mo.group(1)[:200] if mo else ''
    else:
        ps = panics(out)
        rr = RERUN.search(out)
        listed = MATCH_PANIC.findall(out)
        if listed and not rr:
            # the draft fuzz's heuristic-played matches that panicked in the mtg engine (it goes on and
            # lists them at the end, then fails): each its own failure, its event alone the repro
            for e, i, text in listed:
                site, msg = next(((s, mm) for s, mm in ps if mm and mm[0][:60] == text[:60]), ('mtg engine', [text]))
                spec = re.search(rf'event {e} match {i} \(seed {e}\):[^\n]*\n\s*spec: (\{{[^\n]*\}})', out)
                record_failure(cfg, job.mode, sha, site, msg, repro_of(job, int(e), sha) + f' (its match {i}; spec: '
                               + (spec.group(1)[:20000] if spec else '?') + ')', int(e), out)
                nfail += 1
        elif rr:
            seed = int(rr.group(1))
            inner = [p for p in ps if not RERUN.search(' '.join(p[1]))]
            site, msg = inner[0] if inner else (ps[0] if ps else (f'exit {rc}', []))
            record_failure(cfg, job.mode, sha, site, msg, repro_of(job, seed, sha), seed, out)
            nfail = 1
            done = seed - job.first + 1
            rest = job.first + job.n - (seed + 1)
            if rest > 0 and job.cont < MAX_CONTINUATIONS:
                queue.appendleft(Job(job.mode, seed + 1, max(rest, m['min']), job.cont + 1))
        elif ps:
            # a panic outside a seed (an assertion over the whole chunk): the chunk is done
            site, msg = ps[0]
            record_failure(cfg, job.mode, sha, site, msg, repro_of(job, job.first, sha) +
                           f' (the chunk: {COUNT_VAR.get(m.get("test"), "n")}={job.n})', job.first, out)
            nfail = 1
        else:
            # a crash without unwinding (abort, segfault, stack overflow): bisect to its seed
            if job.n > 1:
                h = job.n // 2
                queue.appendleft(Job(job.mode, job.first + h, job.n - h, job.cont, bisect=True))
                queue.appendleft(Job(job.mode, job.first, h, job.cont, bisect=True))
                log(f'{job.describe()} crashed (exit {rc}) without a seed: bisecting')
                done = 0
            else:
                tail = [x for x in out.splitlines() if x.strip()][-6:]
                record_failure(cfg, job.mode, sha, f'crash exit {rc}', tail, repro_of(job, job.first, sha), job.first,
                               out, kind='crash')
                nfail = 1
    if job.bisect and nfail == 0 and done:
        done = job.n                                # a passing half of a bisection
    st['runs'] += 1
    st['units'] += done if m['kind'] != 'smoke' else done
    st['active_s'] += act
    st['failures'] += nfail
    st['last'] = {'first': job.first, 'n': job.n, 'done': done, 'seconds': round(act, 1), 'rc': rc, 'note': note,
                  'time': time.time(), 'sha': sha[:12]}
    if done and act > 1 and not job.bisect:
        r = done / act
        state.d['rates'][job.mode] = round(0.5 * state.rate(job.mode) + 0.5 * r, 6)
    end = job.first + done
    state.d['cursors'][job.mode] = max(state.cursor(job.mode), end)
    state.save()
    log(f'{job.describe()}: rc {rc}, {done} {m["unit"]} in {act:.0f} s'
        + (f', {nfail} failure(s)' if nfail else '') + (f'; {note}' if note else ''))


SMOKE_OK = re.compile(r'^ok\s+seed (\d+) (\S+): (.*)$')
SMOKE_BAD = re.compile(r'^(BAD|FAIL \(exit [^)]*\)|HANG \([^)]*\))\s+seed (\d+) (\S+)(.*)$')


def parse_smoke(cfg, job, out, sha, killed):
    """-> (seeds done, failures, extra counts {games, matches, events})."""
    lines = out.splitlines()
    done, nfail, extra = 0, 0, {}
    seen = set()
    for i, x in enumerate(lines):
        mo = SMOKE_OK.match(x.strip())
        if mo:
            seen.add(int(mo.group(1)))
            done += 1
            rest = mo.group(3)
            for k, pat in (('games', r'(\d+) games'), ('events', r'(\d+) events resolved'), ('matches', r'(\d+) matches \(')):
                mm = re.search(pat, rest)
                if mm:
                    if k == 'games' and MODES[job.mode]['spec'] == 'match':
                        k = 'matches'                       # the match spec's games are best-of-three matches
                    extra[k] = extra.get(k, 0) + int(mm.group(1))
            continue
        mb_ = SMOKE_BAD.match(x.strip())
        if mb_:
            seed = int(mb_.group(2))
            seen.add(seed)
            done += 1
            what = mb_.group(1)
            block = []
            for y in lines[i + 1:i + 60]:
                if SMOKE_OK.match(y.strip()) or SMOKE_BAD.match(y.strip()) or y.startswith('search smoke:'):
                    break
                block.append(y)
            ps = panics('\n'.join(block))
            if ps:
                site, msg = ps[0]
            else:
                site, msg = f'{what.split(" (")[0]} {MODES[job.mode]["spec"]}', [x.strip()] + [y.strip() for y in block[:10]]
            record_failure(cfg, job.mode, sha, site, msg, repro_of(job, seed, sha), seed, out)
            nfail += 1
    if killed:
        missing = [s for s in range(job.first, job.first + job.n) if s not in seen]
        if missing:
            record_failure(cfg, job.mode, sha, f'HANG {job.mode}', [f'no exit after the farm timeout; seeds without a '
                           f'result: {missing}'], repro_of(job, missing[0], sha), missing[0], out, kind='hang')
            nfail += 1
            done += 1                               # the hung seed is not retried
    return done, nfail, extra


# ---------------------------------------------------------------------------- status

def write_status(state, gov, running, phase=None):
    now = time.time()
    built = state.d.get('built') or {}
    life = now - state.d.get('life_started', now)
    wall_h = (state.d['wall_s'] + life) / 3600
    modes = {}
    for mode in MODES:
        st = state.d['modes'].get(mode)
        if not st:
            continue
        unit = MODES[mode]['unit']
        modes[mode] = {'unit': unit, 'fuzzed': st['units'], 'runs': st['runs'], 'active_h': round(st['active_s'] / 3600, 2),
                       'per_active_h': round(st['units'] / max(st['active_s'], 1e-9) * 3600, 1),
                       'per_wall_h': round(st['units'] / max(wall_h, 1e-9), 1), 'failures': st['failures'],
                       'cursor': state.cursor(mode), 'extra': st.get('extra', {}),
                       'extra_per_wall_h': {k: round(v / max(wall_h, 1e-9), 1) for k, v in st.get('extra', {}).items()},
                       'last': st.get('last'), 'skipped': st.get('skipped')}
    fs = failures()
    s = {'updated': now, 'updated_iso': now_iso(now), 'pid': os.getpid(),
         'state': phase or ('paused' if gov and gov.stopped and gov.why and 'pause file' in gov.why else
                            f'backing off: {gov.why}' if gov and gov.stopped else 'running'),
         'ref': config()['ref'], 'sha': built.get('sha'), 'subject': built.get('subject'),
         'draft_fuzz_offset': built.get('draft_first'), 'round': state.d['round'], 'wall_h': round(wall_h, 2),
         'started': state.d['started'],
         'backoff': {'episodes': state.d['backoff']['episodes'] + (gov.events if gov else 0),
                     'paused_h': round((state.d['backoff']['paused_s'] + (gov.paused_total() if gov else 0)) / 3600, 2)},
         'guaranteed_jobs': config().get('guaranteed_jobs', 0), 'guaranteed_window': config().get('guaranteed_window'),
         'guaranteed_backoff': {'episodes': state.d['guaranteed_backoff']['episodes'] + (gov.hard_events if gov else 0),
                                'paused_h': round((state.d['guaranteed_backoff']['paused_s'] +
                                                   (gov.paused_total(guaranteed=True) if gov else 0)) / 3600, 2)},
         'running': [{'mode': j.mode, 'first': j.first, 'n': j.n, 'running_s': round(active_s(j, gov)) if gov else None,
                      'expected_s': round(j.expected_s or 0)} for j in running],
         'modes': modes,
         'failures': [{k: f.get(k) for k in ('id', 'kind', 'site', 'count', 'modes', 'first_seen', 'last_seen', 'repro',
                                             'expected', 'shas')} | {'message': (f.get('message') or [''])[:3]}
                      for f in fs]}
    write_json(path('status.json'), s)
    return s


def status_text(s, since=0.0):
    L = []
    upd = s.get('updated', 0)
    age = time.time() - upd
    L.append(f'fuzz farm: {s["state"]} (status {age:.0f} s old, pid {s.get("pid")}); {s["ref"]} '
             f'{(s.get("sha") or "?")[:12]} "{(s.get("subject") or "")[:70]}"')
    L.append(f'round {s["round"]}, {s["wall_h"]} h of farm time; backed off {s["backoff"]["episodes"]} times, '
             f'{s["backoff"]["paused_h"]} h; draft fuzz seed offset in this build: {s.get("draft_fuzz_offset")}')
    if s.get('running'):
        L.append('running: ' + '; '.join(f'{j["mode"]} [{j["first"]}+{j["n"]}] {j["running_s"]}/{j["expected_s"]} s'
                                        for j in s['running']))
    L.append(f'{"mode":22s} {"fuzzed":>14s} {"per running h":>14s} {"per farm h":>11s} {"fail":>5s}  cursor / last')
    for mode, m in s['modes'].items():
        extra = ', '.join(f'{v:,} {k} ({m["extra_per_wall_h"].get(k)}/farm h)' for k, v in m.get('extra', {}).items())
        last = m.get('last') or {}
        L.append(f'{mode:22s} {m["fuzzed"]:>10,} {m["unit"][:5]:5s}{m["per_active_h"]:>12,.0f} {m["per_wall_h"]:>11,.0f} '
                 f'{m["failures"]:>5d}  {m["cursor"]}' + (f' (last rc {last.get("rc")}, {last.get("seconds")} s)' if last else '')
                 + (f'  [{extra}]' if extra else '') + (f'  SKIPPED: {m["skipped"]}' if m.get('skipped') else ''))
    fs = s.get('failures') or []
    new = [f for f in fs if f['first_seen'] > since]
    L.append(f'failures: {len(fs)} distinct ({sum(1 for f in fs if f.get("expected"))} expected); '
             f'{len(new)} new since {now_iso(since) if since else "the start"}')
    for f in fs:
        flag = 'NEW ' if f['first_seen'] > since else ''
        L.append(f'  {flag}[{f["id"]}] x{f["count"]} {"(expected) " if f.get("expected") else ""}{f["kind"]} '
                 f'{",".join(f["modes"])}; first {now_iso(f["first_seen"])}, last {now_iso(f["last_seen"])}; at {f["site"]}')
        for x in (f.get('message') or [])[:2]:
            L.append(f'      {x[:220]}')
        L.append(f'      repro: {f["repro"]}')
    return '\n'.join(L)


# ---------------------------------------------------------------------------- the loop

def single_instance():
    p = path('farm.pid')
    try:
        with open(p) as f:
            pid = int(f.read().strip())
        if pid != os.getpid() and mb._alive(pid):
            cmd = subprocess.run(['ps', '-o', 'command=', '-p', str(pid)], capture_output=True, text=True).stdout
            if 'fuzz_farm' in cmd:
                raise SystemExit(f'fuzz farm already running (pid {pid})')
    except (OSError, ValueError):
        pass
    with open(p, 'w') as f:
        f.write(str(os.getpid()))


def on_signal(sig, _frame):
    global STOP
    STOP = True
    log(f'signal {sig}: stopping')


def run():
    os.makedirs(FARM, exist_ok=True)
    mb.niceness()
    single_instance()
    signal.signal(signal.SIGTERM, on_signal)
    signal.signal(signal.SIGINT, on_signal)
    if os.path.exists(path('STOP')):
        log('STOP file present: not starting (rm it, or tools/fuzz_farm.sh start)')
        return 0
    cfg = config()
    gov = mb.Governor(cfg['poll'], pause_file=path('PAUSE'), log=path('logs', 'farm.log'))
    state = State()
    state.d['life_started'] = time.time()
    kill_orphans()
    rekey_failures()
    log(f'fuzz farm started (pid {os.getpid()}), round {state.d["round"]}')
    from collections import deque
    try:
        while not STOP:
            cfg = config()
            gov.poll = cfg['poll']
            sha = refresh(cfg, state, gov)
            if STOP:
                break
            if sha is None:
                write_status(state, gov, [], 'waiting: no build (see failures, logs/build_*.log)')
                for _ in range(600):
                    if STOP:
                        break
                    time.sleep(1)
                continue
            queue = deque(plan(cfg, state))
            running = []
            t_round = time.time()
            log(f'round {state.d["round"]} at {sha[:12]}: ' + ', '.join(j.describe() for j in queue))
            last_status = 0
            while (queue or running) and not STOP:
                if os.path.exists(path('STOP')):
                    log('STOP file: stopping')
                    break
                while queue and len(running) < max(1, int(config()['jobs'])) and not gov.stopped:
                    j = queue.popleft()
                    start(cfg, state, j, gov)
                    running.append(j)
                for j in list(running):
                    killed = False
                    if j.p.poll() is None:
                        limit = hang_limit(cfg, j)
                        if active_s(j, gov) > limit:
                            log(f'{j.describe()}: no exit after {active_s(j, gov):.0f} s running: killing (a hang)')
                            mb.kill_group(j.p.pid)
                            j.p.wait()
                            killed = True
                        else:
                            continue
                    running.remove(j)
                    finish(cfg, state, j, gov, queue, killed=killed)
                if time.time() - last_status > 10:
                    last_status = time.time()
                    write_status(state, gov, running)
                time.sleep(1)
            for j in running:                       # stopping: the unfinished jobs' seeds stay at the cursor
                mb.kill_group(j.p.pid)
                j.p.wait()
                j.out.close()
                gov.unregister(j.p.pid)
                jobs_file_remove(j.p.pid)
            if STOP or os.path.exists(path('STOP')):
                break
            state.d['round'] += 1
            with open(path('logs', 'rounds.jsonl'), 'a', encoding='utf-8') as f:
                f.write(json.dumps({'round': state.d['round'] - 1, 'sha': sha[:12], 'seconds': round(time.time() - t_round),
                                    'time': time.time(), 'modes': {k: v.get('last') for k, v in state.d['modes'].items()}})
                        + '\n')
            state.save()
            write_status(state, gov, [])
    finally:
        gov.close()
        state.d['wall_s'] += time.time() - state.d.pop('life_started', time.time())
        state.d['guaranteed_backoff']['episodes'] += gov.hard_events
        state.d['guaranteed_backoff']['paused_s'] += gov.hard_paused_s
        state.d['backoff']['episodes'] += gov.events
        state.d['backoff']['paused_s'] += gov.paused_s
        state.save()
        write_status(state, None, [], 'stopped')
        log('fuzz farm stopped')
    return 0


def once(mode, n):
    """One chunk of `mode` now (the farm's build; seeds at the cursor, which moves): a check."""
    cfg = config()
    state = State()
    gov = mb.Governor(cfg['poll'], pause_file=path('PAUSE'))
    from collections import deque
    q = deque([Job(mode, state.cursor(mode), n)])
    while q:
        j = q.popleft()
        start(cfg, state, j, gov)
        j.p.wait()
        finish(cfg, state, j, gov, q)
        print(json.dumps(state.mstats(mode)['last']))
    gov.close()


def simulate_guaranteed():
    """A LOCAL check of the guaranteed slice's DECISION logic (tiered gates, docs/ENGINE.md 70):
    no Mac, no build, no real farm, and no OS process signalling either (`Governor` SIGSTOPs real
    process groups, which does not exist on Windows - `mac_backoff._signal_group` and `.held` are
    swapped for controllable fakes so this runs anywhere; `Governor._loop`, `.register`,
    `.reason`, `.hard_reason` - the logic that decides who backs off for what - run unmodified).
    `python tools/fuzz_farm.py simulate-guaranteed`. Verifies: a normal job backs off for a
    self-play busy marker and a guaranteed one does not; both back off for a hard (evaluation)
    lock and for PAUSE (a real file: `os.path.exists` needs no fake); `in_guaranteed_window` reads
    a plain window and one that wraps past midnight."""
    import tempfile
    ok = True

    def check(name, cond):
        nonlocal ok
        print(('ok    ' if cond else 'FAIL  ') + name)
        ok = ok and cond

    state = {'busy': False, 'hard': False}
    NORMAL, HARD = ['NORMAL-LOCKS'], ['HARD-LOCKS']

    def fake_held(locks=None):
        if locks == HARD:
            return 'fake-hard-lock' if state['hard'] else None
        return 'fake-busy-marker' if (state['busy'] or state['hard']) else None
    import signal
    had_sigstop, had_sigcont = hasattr(signal, 'SIGSTOP'), hasattr(signal, 'SIGCONT')
    if not had_sigstop:
        signal.SIGSTOP = 23                             # Windows has neither: `_loop` only needs
    if not had_sigcont:                                 # the names to exist, `_signal_group` is faked below
        signal.SIGCONT = 25
    orig_signal_group, orig_held, orig_hard_locks = mb._signal_group, mb.held, mb.hard_locks
    mb._signal_group = lambda pgid, sig: True           # no real OS signal sent - fake pgids below
    mb.held = fake_held
    mb.hard_locks = lambda: HARD
    try:
        with tempfile.NamedTemporaryFile(delete=False) as f:
            pause_path = f.name
        os.remove(pause_path)                           # exists only once the test creates it
        gov = mb.Governor(poll=0.05, locks=NORMAL, pause_file=pause_path)
        try:
            gov.register(999901, guaranteed=False)
            gov.register(999902, guaranteed=True)
            time.sleep(0.2)
            check('neither backs off with nothing held', not gov.stopped and not gov.hard_stopped)
            state['busy'] = True
            time.sleep(0.2)
            check('a normal job backs off for a self-play busy marker', gov.stopped)
            check('a guaranteed job does NOT back off for a self-play busy marker alone', not gov.hard_stopped)
            state['busy'] = False
            time.sleep(0.2)
            check('the normal job resumes once the marker is gone', not gov.stopped)
            state['hard'] = True
            time.sleep(0.2)
            check('a normal job backs off for a hard (evaluation) lock', gov.stopped)
            check('a GUARANTEED job backs off for a hard lock too', gov.hard_stopped)
            state['hard'] = False
            time.sleep(0.2)
            check('both resume once the hard lock is gone', not gov.stopped and not gov.hard_stopped)
            with open(pause_path, 'w'):
                pass
            time.sleep(0.2)
            check('PAUSE stops the guaranteed job too', gov.hard_stopped and gov.stopped)
        finally:
            gov.close()
            os.remove(pause_path)
    finally:
        mb._signal_group, mb.held, mb.hard_locks = orig_signal_group, orig_held, orig_hard_locks
        if not had_sigstop:
            del signal.SIGSTOP
        if not had_sigcont:
            del signal.SIGCONT

    def ts(h, m):
        return time.mktime((2026, 1, 1, h, m, 0, 0, 1, -1))
    check('a plain window: 10:00 is outside 02:00-06:00', not in_guaranteed_window({'guaranteed_window': '02:00-06:00'}, ts(10, 0)))
    check('a plain window: 03:00 is inside 02:00-06:00', in_guaranteed_window({'guaranteed_window': '02:00-06:00'}, ts(3, 0)))
    check('a wrapping window: 23:30 is inside 22:00-06:00', in_guaranteed_window({'guaranteed_window': '22:00-06:00'}, ts(23, 30)))
    check('a wrapping window: 12:00 is outside 22:00-06:00', not in_guaranteed_window({'guaranteed_window': '22:00-06:00'}, ts(12, 0)))
    check('no window configured: never guaranteed', not in_guaranteed_window({'guaranteed_window': None}, ts(3, 0)))
    print('GUARANTEED-SLICE SIMULATION: ' + ('PASSED' if ok else 'FAILED'))
    return 0 if ok else 1


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('cmd', choices=('run', 'status', 'once', 'build', 'simulate-guaranteed'))
    ap.add_argument('args', nargs='*')
    ap.add_argument('--since', type=float, default=0.0, help='status: failures first seen after this time are NEW')
    ap.add_argument('--json', action='store_true')
    a = ap.parse_args()
    if a.cmd == 'run':
        return run()
    if a.cmd == 'status':
        s = read_json(path('status.json'), None)
        if s is None:
            print('fuzz farm: no status yet')
            return 1
        print(json.dumps(s, indent=1) if a.json else status_text(s, a.since))
        return 0
    if a.cmd == 'build':
        cfg = config()
        state = State()
        gov = mb.Governor(cfg['poll'])
        sha = refresh(cfg, state, gov)
        gov.close()
        print(sha)
        return 0 if sha else 1
    if a.cmd == 'simulate-guaranteed':
        return simulate_guaranteed()
    once(a.args[0], int(a.args[1]))
    return 0


if __name__ == '__main__':
    sys.exit(main())
