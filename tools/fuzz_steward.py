"""The fuzz steward (docs/ENGINE.md 171): continuous, farm-first fuzz of main's newest build and of every live run's
pinned build, at a daily quota, with a failure queue and an alert for each new confirmed failure site.

    python tools/fuzz_steward.py run [--once] [--dry-run] [--status-file F]   # the steward (the logon task runs this)
    python tools/fuzz_steward.py status                                       # the state, today's numbers, the queue
    python tools/fuzz_steward.py stop                                         # STOP: it ends (its local jobs finish)
    python tools/fuzz_steward.py install-task [--status-file F] | uninstall-task
    python tools/fuzz_steward.py confirm ID                                   # replay a queue entry here now

The owner's decision (2026-11-15, D1): the gate fuzzes ~2,000 games per pool; the 20k-style depth is continuous farm
fuzz of main and of each live run's pinned build - QUOTA (50,000) seeds per build per UTC day at a uniform pace, the
farm first; when the farm is behind, the Mac and then the desktop fill in; the farm's side-work cap (~25% while a run is
live) may be exceeded while a build is behind its pace.

**Targets.** `main` (the newest commit of the local `main` branch) and every live run (a `runs/*/supervisor_state.json`
whose supervisor is running: its pid alive with the recorded start time, no STOP file), whose pinned commit is the
supervisor's working directory's HEAD. Each target gets its own pinned worktree `<wt>/steward-<target>` and cargo
target dir `$ZERO_BIG_DISK/cargo-target/mtg-zero/steward-<target>` (one per checkout, never shared), and a signed fuzz
build (`tools/farm_build.py --profile fuzz --publish` there, through the slot throttle; an existing signed fuzz build
of the same commit is reused). Main is rebuilt only when its **engine hash** changes (the git tree ids of every Rust
input: tools/zcache.py's paths and the zero-core gitlink) and at most once per `main_rebuild_min_h` (24 h).

**Quota and pace** (pure functions below; tests/test_fuzz_steward.py). A build's day: `quota` x (the part of the UTC
day since the build started, or all of it); the pace at time t is `quota` x (seconds since the later of midnight and
the build's start) / 86,400. Progress = seeds done today (farm results by their upload time, local jobs by their end)
+ seeds in flight (farm leases, running local jobs). The deficit in hours of quota, h = (pace - progress) / (quota /
24), decides, every `check_s` (15 min):

| h | farm (the campaign of the build's most-behind mode) | Mac | desktop |
|---|---|---|---|
| <= 0 (on pace) | no share: the backfill (idle PCs only) | stop starting chunks | stop |
| 0 - `boost_h` | share `side_share` (0.25, the side-work share) | - | - |
| > `boost_h` (1 h) | share `boost_share` (0.5: past the side-work share, allowed while behind) | - | - |
| > `mac_h` (2 h) after `local_after_s` (30 min) at boost, or no farm PC online | boost | start chunks (nice 19) | - |
| > `desktop_h` (3 h) with the Mac already at work for `local_after_s` | boost | chunks | start one job: the build's own signed fuzz worker, idle priority, through tools/slotd.sh, only while every live run's iteration time stays within `starve_ratio` (1.10) of its recent median (else stop, and wait `desktop_cooldown_s`) |

Each build's quota is split over its modes by weight (`modes`; the run's or main's list), and the farm boost and the
local chunks go to the mode furthest behind its own share. One fuzz campaign per (build, mode) on the coordinator,
named `fz-<target>-<sha7>-<alias>`, seeds from 100,000,000 (the coordinator's cursor never rewinds); local chunks use
their own seed spaces (Mac from 3e9, desktop from 4e9) so no seed is played twice. The Mac and the desktop run the same
fuzz loop as the farm through `tools/fuzz_local.py` (the worker's own `serve` with one job line): the desktop with the
build's signed executable, the Mac with the same commit's `mtgz-worker` built there (`cargo build --profile fuzz -p
mtgz_worker` through tools/mac.sh). The desktop runs the live runs' GPU self-play and training: the steward never
takes the GPU and starts at most one desktop job (`desktop_threads` processes), and the throttle refuses it when RAM is
short.

**Failures.** The farm's (`GET /v1/admin/campaign/{id}/failures` of the steward's campaigns), the local jobs' and the
live runs' self-play panics (`<run>/game_failures.jsonl`: a game dropped for a panic, docs/ENGINE.md 167.6) go into one
queue `<state>/queue.jsonl`: one JSON line per failure site (tools/fuzz_farm.py's key: the panic site and its
message's first line, digits masked - the id tools/farm_fuzz_pull.py gives the same panic), with kind, site, message,
mode, the first seeds, repro command, build, engine commit, target, first / last seen, count, sources and whether it
was confirmed. A new site is replayed here (a fuzz seed: the build's worker over that one seed, which must fail at the
same site; a self-play failure: its record's `repro` command in the run's own checkout - docs/ENGINE.md 172's replay, which exits
101 when it reproduces the panic) and,
once confirmed, the owner gets ONE short message through the game-alerts relay on the Mac (`relay.py --notify-b64`;
no machine details). A self-play panic that has no `repro` (records before docs/ENGINE.md 171's) counts as confirmed
when it happened on the owner's own machines.

**Files** (`<state>` = $ZERO_BIG_DISK/mtg-zero-state/fuzz-steward, else ~/.mtgz-state/fuzz-steward): config.json
(overrides of CONFIG, re-read every check), state.json, queue.jsonl, steward.log, daily.log (one summary line per build
per UTC day: seeds by source, failures), builds/ and jobs/ (logs), STOP. `--status-file`: a small JSON status (today's
numbers, the targets, the queue's head) written every check, for the hub. No port is bound.
The admin token and the data key are read by zero.farm_def / tools/farm_campaign.py from ~/.config/zero-farm (never
printed); the Mac is MTGZ_MAC in ~/.config/zero/local.env.
"""
import argparse
import datetime as dt
import glob
import hashlib
import json
import math
import os
import re
import shlex
import signal
import statistics
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(ROOT, 'third_party', 'zero-core', 'python'))
import local_config  # noqa: E402  (the machine's own settings: ZERO_BIG_DISK, MTGZ_MAC)

WIN = sys.platform == 'win32'
NO_WINDOW = subprocess.CREATE_NO_WINDOW if WIN else 0
DAY = 86400
RUST_INPUTS = ('engine', 'card_ir', 'py', 'worker', 'data', 'Cargo.toml', 'Cargo.lock', '.cargo', 'rust-toolchain',
               'rust-toolchain.toml', 'third_party/zero-core')
FARM_SEED_BASE = 100_000_000
LOCAL_SEED_BASE = {'mac': 3_000_000_000, 'desktop': 4_000_000_000}
TASK_NAME = 'MTG Zero fuzz steward'

# A mode: the worker's fuzz mode name, its env (in this order), its weight in the build's quota, a short alias for
# the campaign's name, and a starting rate (seeds per thread-second) until one is measured.
MAIN_MODES = [
    {'alias': 'g', 'mode': 'games', 'env': {}, 'w': 0.35, 'rate': 4.0},
    {'alias': 'hg', 'mode': 'hidden-games', 'env': {'FUZZ_HIDDEN_DECKS': '1'}, 'w': 0.15, 'rate': 3.0},
    {'alias': 'dd2g', 'mode': 'delver_dealt2-games', 'env': {'FUZZ_POOL': 'delver_dealt2'}, 'w': 0.3, 'rate': 2.5},
    {'alias': 'dd2m', 'mode': 'delver_dealt2-matches', 'env': {'FUZZ_POOL': 'delver_dealt2'}, 'w': 0.2, 'rate': 0.8},
]
DELVER_RUN_MODES = [
    {'alias': 'dd2g', 'mode': 'delver_dealt2-games', 'env': {'FUZZ_POOL': 'delver_dealt2'}, 'w': 0.5, 'rate': 2.5},
    {'alias': 'dd2m', 'mode': 'delver_dealt2-matches', 'env': {'FUZZ_POOL': 'delver_dealt2'}, 'w': 0.25, 'rate': 0.8},
    {'alias': 'g', 'mode': 'games', 'env': {}, 'w': 0.15, 'rate': 4.0},
    {'alias': 'hg', 'mode': 'hidden-games', 'env': {'FUZZ_HIDDEN_DECKS': '1'}, 'w': 0.1, 'rate': 3.0},
]

CONFIG = {
    'quota': 50_000,              # seeds per build per UTC day (a match seed counts once: it holds 2-3 games)
    'check_s': 900,               # pace checks
    'tick_s': 60,                 # the loop: local jobs, builds, STOP
    'side_share': 0.25,           # the side-work share (D5)
    'boost_share': 0.5,           # past it while behind (the owner allowed it)
    'boost_h': 1.0,
    'mac_h': 2.0,
    'desktop_h': 3.0,
    'local_after_s': 1800,
    'chunk_s': 1200,              # a local job's length
    'mac': True,
    'mac_threads': 6,
    'desktop': True,
    'desktop_threads': 2,
    'desktop_cooldown_s': 7200,
    'starve_ratio': 1.10,
    'starve_window': 10,          # iterations in a run's recent median
    'main_rebuild_min_h': 24,
    'priority': 0.3,
    'main_modes': MAIN_MODES,
    'run_modes': {'delver': DELVER_RUN_MODES},   # by the run's preset prefix; else main_modes
    'alerts': True,
    'farm_url': None,             # default zero.farm_def.URL
}


# ======================================================================== pure functions (tests/test_fuzz_steward.py)

def day_start(t):
    """The start of t's UTC day (unix seconds)."""
    return int(t // DAY) * DAY


def day_name(t):
    return dt.datetime.fromtimestamp(day_start(t), dt.timezone.utc).strftime('%Y-%m-%d')


def day_quota(quota, t, build_start):
    """A build's quota for t's UTC day: the whole quota, or the part of it from the build's start on (its first day)."""
    d0 = day_start(t)
    s = max(d0, build_start)
    return quota * max(0, d0 + DAY - s) / DAY


def pace_target(quota, t, build_start):
    """Seeds a build should have by time t of its UTC day: a uniform pace from midnight (or its start) on."""
    s = max(day_start(t), build_start)
    return quota * max(0.0, t - s) / DAY


def deficit_hours(quota, t, build_start, done, in_flight=0):
    """How far behind its pace a build is, in hours of quota (negative: ahead)."""
    per_h = quota / 24.0
    return (pace_target(quota, t, build_start) - done - in_flight) / per_h if per_h > 0 else 0.0


def mode_quotas(modes, quota):
    """{alias: quota share}; weights normalised."""
    tot = sum(m['w'] for m in modes) or 1.0
    return {m['alias']: quota * m['w'] / tot for m in modes}


def most_behind(modes, quota, t, build_start, done_by_mode, flight_by_mode=None, current=None, margin_h=0.5):
    """The alias of the mode furthest behind its own pace (in hours of its quota), and that deficit. `current` (the
    mode boosted now) is kept unless another is more than `margin_h` further behind (no flapping between equals)."""
    hs = {alias: deficit_hours(q, t, build_start, done_by_mode.get(alias, 0), (flight_by_mode or {}).get(alias, 0))
          for alias, q in mode_quotas(modes, quota).items()}
    best = max(hs, key=lambda a: (hs[a], -list(hs).index(a)))
    if current in hs and hs[current] >= hs[best] - margin_h:
        best = current
    return best, hs[best]


def farm_share(h, current, cfg):
    """The share the build's boosted campaign gets: None (the backfill) on pace; the side-work share a little behind;
    the boost share more than `boost_h` behind - kept until back on pace (no flapping between the two)."""
    if h <= 0:
        return None
    if h > cfg['boost_h'] or (current is not None and current >= cfg['boost_share'] - 1e-9):
        return cfg['boost_share']
    return cfg['side_share']


def local_decision(h, now, boost_since, mac_since, farm_online, cfg, desktop_ok=True):
    """Which local sources should be working for a build: {'mac': bool, 'desktop': bool}.
    `boost_since`: when the farm boost began (None: not boosted); `mac_since`: when the Mac began (None: idle);
    `farm_online`: farm PCs seen in the last few minutes."""
    out = {'mac': False, 'desktop': False}
    if h <= 0:
        return out
    boosted_long = boost_since is not None and now - boost_since >= cfg['local_after_s']
    if cfg['mac'] and h > cfg['mac_h'] and (boosted_long or farm_online == 0):
        out['mac'] = True
    if (cfg['desktop'] and desktop_ok and out['mac'] and h > cfg['desktop_h'] and mac_since is not None
            and now - mac_since >= cfg['local_after_s']):
        out['desktop'] = True
    return out


def starved(seconds, ratio, window, current_elapsed=None, baseline=None):
    """Is a live run slowed beyond `ratio` x its recent median iteration time? `seconds`: its completed iterations'
    times, oldest first; `baseline`: the median before local work began (else the median of the last `window` but the
    newest); the newest completed one and the one in progress (`current_elapsed`) are compared with it."""
    if baseline is None:
        prev = seconds[-window - 1:-1] if len(seconds) > 1 else []
        if len(prev) < 3:
            return False
        baseline = statistics.median(prev)
    if not baseline:
        return False
    last = seconds[-1] if seconds else 0
    return max(last, current_elapsed or 0) > ratio * baseline


def add_range(ranges, lo, hi):
    """Seed ranges [lo, hi) merged, sorted."""
    out = sorted([list(r) for r in ranges] + [[lo, hi]])
    merged = []
    for a, b in out:
        if merged and a <= merged[-1][1]:
            merged[-1][1] = max(merged[-1][1], b)
        else:
            merged.append([a, b])
    return merged


def count_ranges(ranges):
    return sum(b - a for a, b in ranges)


def failure_id(site, message_lines):
    """tools/fuzz_farm.py's key (the site and its message's first line, digits masked) and its id (sha1[:12]): the
    same id tools/farm_fuzz_pull.py and the Mac's fuzz farm give the same panic."""
    from fuzz_farm import failure_key
    key = failure_key(site or 'unknown', list(message_lines or []))
    return hashlib.sha1(key.encode()).hexdigest()[:12], key


def campaign_name(target, commit, alias):
    t = re.sub(r'[^a-z0-9-]', '-', target.lower())[:36].strip('-')
    return f'fz-{t}-{commit[:7]}-{alias}'


def alert_text(e):
    """The one short message for a new confirmed failure site (no machine details)."""
    where = f'{e.get("mode")} seed {e.get("seed")}' if e.get('kind') == 'fuzz' else f'self-play ({e.get("target")})'
    site = (e.get('site') or 'unknown').replace('\\', '/').split('/')[-1]
    return (f'mtg-zero: new engine failure {e["id"]} on {e.get("target")}@{(e.get("engine_commit") or "")[:7]} - '
            f'{where} - {site}. Queue: fuzz-steward queue.jsonl')


# ======================================================================== the machine

def state_dir():
    return local_config.big_disk_path('mtg-zero-state', 'fuzz-steward') or os.path.join(
        os.path.expanduser('~'), '.mtgz-state', 'fuzz-steward')


def read_json(p, default):
    try:
        with open(p, encoding='utf-8') as f:
            return json.load(f)
    except (OSError, ValueError):
        return default


def write_json(p, obj):
    tmp = f'{p}.tmp{os.getpid()}'
    with open(tmp, 'w', encoding='utf-8') as f:
        json.dump(obj, f, indent=1, sort_keys=True)
    os.replace(tmp, p)


def bash_exe():
    """Git Bash on Windows (the repo's scripts are Git Bash scripts; a bare `bash` from a console-less process can be
    WSL's, which reads a CRLF checkout's scripts as broken), `bash` elsewhere. MTGZ_BASH overrides."""
    v = local_config.get('MTGZ_BASH')
    if v:
        return v
    if WIN:
        import shutil
        g = shutil.which('git')
        if g:
            base = os.path.dirname(os.path.dirname(os.path.realpath(g)))
            for c in (os.path.join(base, 'bin', 'bash.exe'), os.path.join(os.path.dirname(base), 'bin', 'bash.exe')):
                if os.path.exists(c):
                    return c
    return 'bash'


def git(*args, cwd=ROOT, check=True):
    p = subprocess.run(['git', *args], cwd=cwd, capture_output=True, text=True, creationflags=NO_WINDOW)
    if check and p.returncode:
        raise RuntimeError(f'git {" ".join(args)}: {p.stderr.strip()[:500]}')
    return p.stdout.strip()


_ENGINE_HASH = {}


def engine_hash(commit, cwd=ROOT):
    """The tree ids of every Rust input at `commit` (and the zero-core gitlink), hashed: equal hashes = the same build.
    A full commit id's hash never changes: kept for the process."""
    if commit in _ENGINE_HASH:
        return _ENGINE_HASH[commit]
    out = git('ls-tree', commit, '--', *RUST_INPUTS, cwd=cwd)
    h = hashlib.sha256(out.encode()).hexdigest()[:16]
    if re.fullmatch(r'[0-9a-f]{40}', commit):
        _ENGINE_HASH[commit] = h
    return h


def pid_alive(pid, started=None):
    try:
        import psutil
    except ImportError:
        psutil = None
    if psutil is not None:
        try:
            p = psutil.Process(int(pid))
            return started is None or abs(p.create_time() - float(started)) < 120
        except Exception:                          # noqa: BLE001 - gone, denied
            return False
    if WIN:
        out = subprocess.run(['tasklist', '/FI', f'PID eq {int(pid)}', '/NH'], capture_output=True, text=True,
                             creationflags=NO_WINDOW).stdout
        return str(int(pid)) in out
    try:
        os.kill(int(pid), 0)
        return True
    except OSError:
        return False


def live_runs(runs_glob):
    """[{name, dir, commit, wt, preset, metrics}] of the runs whose supervisor is live."""
    out = []
    for sp in sorted(glob.glob(runs_glob)):
        rd = os.path.dirname(sp)
        st = read_json(sp, {})
        if st.get('state') != 'running' or os.path.exists(os.path.join(rd, 'STOP')):
            continue
        if not st.get('pid') or not pid_alive(st['pid'], st.get('started')):
            continue
        wt = None
        try:
            import psutil
            wt = psutil.Process(int(st['pid'])).cwd()
        except Exception:                          # noqa: BLE001
            wt = None
        if not wt:
            m = re.search(r'([A-Za-z]:[\\/][^"]*?mtg-zero-wt[\\/][^\\/"]+)[\\/]tools', json.dumps(st).replace('\\\\', '\\'))
            wt = m.group(1) if m else None
        if not wt or not os.path.isdir(wt):
            continue
        try:
            commit = git('rev-parse', 'HEAD', cwd=wt)
        except RuntimeError:
            continue
        args = st.get('args') or []
        preset = args[args.index('--preset') + 1] if '--preset' in args else ''
        out.append({'name': os.path.basename(rd), 'dir': rd, 'commit': commit, 'wt': wt, 'preset': preset})
    return out


def run_iteration_times(rd):
    """A run's completed iterations' seconds (oldest first) and the seconds since the newest one ended."""
    secs, last_t = [], None
    try:
        with open(os.path.join(rd, 'metrics.jsonl'), 'rb') as f:
            f.seek(0, 2)
            f.seek(max(0, f.tell() - 200_000))
            for line in f.read().decode('utf-8', 'replace').splitlines()[1:]:
                try:
                    m = json.loads(line)
                except ValueError:
                    continue
                if isinstance(m.get('seconds'), (int, float)):
                    secs.append(float(m['seconds']))
                    last_t = m.get('time', last_t)
    except OSError:
        pass
    return secs, (time.time() - last_t) if last_t else None


class Steward:
    def __init__(self, a):
        self.a = a
        self.dir = a.state or state_dir()
        os.makedirs(os.path.join(self.dir, 'builds'), exist_ok=True)
        os.makedirs(os.path.join(self.dir, 'jobs'), exist_ok=True)
        self.cfg = dict(CONFIG)
        self.state = read_json(os.path.join(self.dir, 'state.json'), {})
        self.state.setdefault('targets', {})
        self.state.setdefault('days', {})
        self.state.setdefault('runs_seen', {})
        self.state.setdefault('rates', {})
        self.queue = self.read_queue()
        self.procs = {}              # job id -> Popen (local jobs, builds) started by this process
        self._client = None
        self.last_check = 0

    # -------------------------------------------------------------- plumbing
    def log(self, msg):
        line = f'{time.strftime("%Y-%m-%d %H:%M:%S")} {msg}'
        with open(os.path.join(self.dir, 'steward.log'), 'a', encoding='utf-8') as f:
            f.write(line + '\n')
        if not self.a.quiet:
            print(line, flush=True)

    def load_cfg(self):
        over = read_json(os.path.join(self.dir, 'config.json'), {})
        self.cfg = dict(CONFIG, **{k: v for k, v in over.items() if k in CONFIG})

    def save(self):
        write_json(os.path.join(self.dir, 'state.json'), self.state)

    def read_queue(self):
        q = {}
        try:
            with open(os.path.join(self.dir, 'queue.jsonl'), encoding='utf-8') as f:
                for line in f:
                    if line.strip():
                        e = json.loads(line)
                        q[e['id']] = e
        except OSError:
            pass
        return q

    def write_queue(self):
        p = os.path.join(self.dir, 'queue.jsonl')
        tmp = p + '.tmp'
        with open(tmp, 'w', encoding='utf-8') as f:
            for e in sorted(self.queue.values(), key=lambda e: e.get('first_seen') or 0):
                f.write(json.dumps(e, sort_keys=True) + '\n')
        os.replace(tmp, p)

    def client(self):
        if self._client is None:
            from zero import farm_def as FD
            self._client = FD.Client(self.cfg.get('farm_url') or FD.URL)
        return self._client

    def py(self):
        return sys.executable.replace('pythonw.exe', 'python.exe')

    def spawn(self, jid, argv, cwd, log_path, env=None):
        kw = {'creationflags': NO_WINDOW | subprocess.BELOW_NORMAL_PRIORITY_CLASS} if WIN else {}
        lf = open(log_path, 'ab')
        p = subprocess.Popen(argv, cwd=cwd, stdin=subprocess.DEVNULL, stdout=lf, stderr=subprocess.STDOUT,
                             env=dict(os.environ, **(env or {})), **kw)
        lf.close()
        self.procs[jid] = p
        return p

    def day(self, t, key):
        """The day record of a build: {source: {alias: seeds}} plus failures."""
        d = self.state['days'].setdefault(day_name(t), {})
        return d.setdefault(key, {'farm': {}, 'mac': {}, 'desktop': {}, 'failures': 0})

    def credit(self, t, bkey, source, alias, n):
        rec = self.day(t, bkey)
        rec[source][alias] = rec[source].get(alias, 0) + int(n)

    # -------------------------------------------------------------- targets and builds
    def discover(self):
        tg = {}
        main_commit = git('rev-parse', 'refs/heads/main')
        tg['main'] = {'commit': main_commit, 'kind': 'main', 'modes': self.cfg['main_modes']}
        for r in live_runs(self.a.runs_glob):
            modes = next((v for k, v in self.cfg['run_modes'].items() if r['preset'].startswith(k)), self.cfg['main_modes'])
            tg[r['name']] = {'commit': r['commit'], 'kind': 'run', 'modes': modes, 'run_dir': r['dir'], 'run_wt': r['wt']}
        return tg

    def wt_of(self, key):
        return os.path.join(os.path.dirname(ROOT) if os.path.basename(os.path.dirname(ROOT)) == 'mtg-zero-wt'
                            else os.path.join(os.path.dirname(ROOT), 'mtg-zero-wt'), f'steward-{key}')

    def target_dir_of(self, key):
        big = local_config.big_disk_path('cargo-target', 'mtg-zero', f'steward-{key}')
        return big or os.path.join(self.wt_of(key), 'target')

    def find_build(self, commit):
        """A signed fuzz build on this machine of `commit`, or of a commit with the same engine hash (the same Rust
        inputs: the same executable) -> (build_id, exe) or (None, None). The exact commit is preferred."""
        pat = local_config.big_disk_path('cargo-target', 'mtg-zero')
        dists = sorted(glob.glob(os.path.join(pat, '*', 'farm-build', 'dist', '*', 'manifest.json'))) if pat else []
        want, same = None, None
        for m in dists:
            with open(m, 'rb') as f:
                raw = f.read()
            try:
                man = json.loads(raw)
            except ValueError:
                continue
            exe = os.path.join(os.path.dirname(m), man.get('entry') or 'mtgz-worker.exe')
            if man.get('profile') != 'fuzz' or man.get('kind') != 'worker' or not os.path.exists(exe):
                continue
            bid = hashlib.sha256(raw).hexdigest()[:24]
            if man.get('engine_commit') == commit:
                return bid, exe
            if same is None and man.get('engine_commit'):
                try:
                    want = want or engine_hash(commit)
                    if engine_hash(man['engine_commit']) == want:
                        same = (bid, exe)
                except RuntimeError:
                    pass
        return same or (None, None)

    def ensure_targets(self, now):
        found = self.discover()
        for key, t in found.items():
            cur = self.state['targets'].get(key)
            h = engine_hash(t['commit'])
            if cur and cur.get('engine_hash') == h and cur.get('build_id'):
                if cur.get('retired'):
                    self.log(f'{key}: a target again (build {cur["build_id"]}): its campaigns resume')
                    for name in (cur.get('campaigns') or {}).values():
                        self.put_campaign(name, state='active', share=None)
                    cur['shares'] = {}
                cur.update(commit_seen=t['commit'], modes=t['modes'], retired=None, run_dir=t.get('run_dir'),
                           run_wt=t.get('run_wt'))
                continue
            if cur and cur.get('building'):
                continue
            if (key == 'main' and cur and cur.get('build_id') and
                    now - cur.get('built_at', 0) < self.cfg['main_rebuild_min_h'] * 3600):
                cur['pending'] = {'commit': t['commit'], 'engine_hash': h}
                continue
            self.start_build(key, t, h, now)
        for key, cur in self.state['targets'].items():
            if key not in found and not cur.get('retired'):
                self.retire(key, now)

    def start_build(self, key, t, h, now):
        bid, exe = self.find_build(t['commit'])
        cur = self.state['targets'].setdefault(key, {})
        if bid:
            self.log(f'{key}: signed fuzz build {bid} of {t["commit"][:9]} exists ({exe})')
            self.adopt_build(key, t, h, bid, exe, now)
            return
        if self.a.dry_run:
            self.log(f'dry run: {key} would build {t["commit"][:9]}')
            return
        wt = self.wt_of(key)
        if not os.path.isdir(wt):
            git('worktree', 'add', '--detach', wt, t['commit'])
        git('checkout', '-q', '--detach', '-f', t['commit'], cwd=wt)
        subprocess.run(['git', '-c', 'protocol.file.allow=always', 'submodule', 'update', '-q', '--init',
                        'third_party/zero-core'], cwd=wt, creationflags=NO_WINDOW, capture_output=True)
        logp = os.path.join(self.dir, 'builds', f'{key}-{t["commit"][:9]}.log')
        jid = f'build:{key}'
        self.spawn(jid, [bash_exe(), 'tools/slotd.sh', self.py(), 'tools/farm_build.py', '--profile', 'fuzz', '--publish'],
                   wt, logp, env={'CARGO_TARGET_DIR': self.target_dir_of(key)})
        cur.update(building={'commit': t['commit'], 'engine_hash': h, 'log': logp, 'since': now, 'modes': t['modes'],
                             'kind': t['kind'], 'run_dir': t.get('run_dir'), 'run_wt': t.get('run_wt')})
        self.log(f'{key}: building the fuzz worker of {t["commit"][:9]} in {wt} (log {logp})')

    def poll_builds(self, now):
        for key, cur in self.state['targets'].items():
            b = cur.get('building')
            if not b:
                continue
            p = self.procs.get(f'build:{key}')
            if p is not None and p.poll() is None:
                continue
            if p is None and now - b['since'] < 4 * 3600:
                bid, exe = self.find_build(b['commit'])      # (a steward restarted mid-build: look for its result)
                if not bid:
                    continue
            bid, exe = self.find_build(b['commit'])
            cur.pop('building', None)
            self.procs.pop(f'build:{key}', None)
            if not bid:
                self.log(f'{key}: the fuzz build of {b["commit"][:9]} FAILED (log {b["log"]}); retried next check')
                continue
            self.adopt_build(key, {'commit': b['commit'], 'modes': b['modes'], 'kind': b['kind'],
                                   'run_dir': b.get('run_dir'), 'run_wt': b.get('run_wt')}, b['engine_hash'], bid, exe, now)

    def adopt_build(self, key, t, h, bid, exe, now):
        cur = self.state['targets'].setdefault(key, {})
        old = cur.get('build_id')
        if old and old != bid:
            self.pause_campaigns(cur)
            cur.setdefault('history', []).append({k: cur.get(k) for k in ('build_id', 'commit', 'built_at', 'campaigns')})
        cur.update(kind=t['kind'], commit=t['commit'], commit_seen=t['commit'], engine_hash=h, build_id=bid, exe=exe,
                   built_at=now, modes=t['modes'], campaigns={}, shares={}, run_dir=t.get('run_dir'),
                   run_wt=t.get('run_wt'), retired=None, pending=None,
                   local={'mac': {}, 'desktop': {}}, boost_since=None)
        self.log(f'{key}: build {bid} ({t["commit"][:9]}, engine {h}) adopted')
        self.ensure_campaigns(key, cur)

    def ensure_campaigns(self, key, cur):
        for m in cur['modes']:
            name = campaign_name(key, cur['commit'], m['alias'])
            if cur['campaigns'].get(m['alias']) == name:
                continue
            argv = [self.py(), os.path.join(HERE, 'farm_campaign.py'), 'create', name, '--build', cur['build_id'],
                    '--mode', m['mode'], '--seed-base', str(FARM_SEED_BASE), '--priority', str(self.cfg['priority'])]
            for k, v in m['env'].items():
                argv += ['--env', f'{k}={v}']
            if self.a.dry_run:
                argv.append('--dry-run')
            try:
                ans = self.client().get_or_none(f'/v1/admin/campaign/{name}')
            except Exception as e:                   # noqa: BLE001
                self.log(f'{key}: campaign {name}: the coordinator: {e}')
                continue
            if ans is not None:
                cur['campaigns'][m['alias']] = name
                if ans.get('state') != 'active':
                    self.put_campaign(name, state='active')
                continue
            p = subprocess.run(argv, cwd=ROOT, capture_output=True, text=True, creationflags=NO_WINDOW, timeout=900)
            if p.returncode:
                self.log(f'{key}: campaign {name} NOT created: {(p.stderr or p.stdout).strip()[-600:]}')
                continue
            if not self.a.dry_run:
                cur['campaigns'][m['alias']] = name
            self.log(f'{key}: campaign {name} ({m["mode"]} {m["env"]}) created')

    def put_campaign(self, name, **change):
        """The campaign put again with `change` (share, state); nothing signed (neither is in its definition)."""
        if self.a.dry_run:
            self.log(f'dry run: campaign {name} would be put with {change}')
            return True
        try:
            import farm_campaign as FC
            ans = self.client().call('GET', f'/v1/admin/campaign/{name}')
            body = dict(FC.campaign_body(FC.current_fields(ans)), **change)
            self.client().call('PUT', f'/v1/admin/campaign/{name}', body=body)
            return True
        except Exception as e:                       # noqa: BLE001
            self.log(f'campaign {name}: put {change} failed: {e}')
            return False

    def pause_campaigns(self, cur):
        for name in (cur.get('campaigns') or {}).values():
            self.put_campaign(name, state='paused', share=None)

    def retire(self, key, now):
        cur = self.state['targets'][key]
        self.log(f'{key}: no longer a target (the run ended): its campaigns paused, its local jobs finish')
        self.pause_campaigns(cur)
        self.stop_local(key, 'retired')
        cur['retired'] = now

    # -------------------------------------------------------------- farm progress and failures
    def poll_farm(self, now):
        """Credit the steward's campaigns' results (by upload time) and collect their failures; -> {key: {alias:
        in-flight seeds}}, the farm PCs seen working recently."""
        flight = {}
        for key, cur in self.state['targets'].items():
            for alias, name in (cur.get('campaigns') or {}).items():
                cs = self.state.setdefault('cursors', {}).setdefault(name, {'results': 0, 'failures': 0})
                try:
                    while True:
                        r = self.client().call('GET', f'/v1/admin/campaign/{name}/results?after={cs["results"]}&limit=1000')
                        rows = r.get('results') or []
                        for row in rows:
                            n = sum(b - a for a, b in row.get('ids') or [])
                            if n:
                                self.credit(row.get('t') or now, cur['build_id'], 'farm', alias, n)
                                self.state.setdefault('farm_log', []).append([row.get('t') or now, key, alias, n])
                        if not rows:
                            break
                        cs['results'] = r.get('cursor') or rows[-1]['id']
                        if not self.a.dry_run:
                            self.client().call('POST', f'/v1/admin/campaign/{name}/ack', body={'cursor': cs['results']})
                    while True:
                        r = self.client().call('GET', f'/v1/admin/campaign/{name}/failures?after={cs["failures"]}&limit=500')
                        rows = r.get('failures') or []
                        for f in rows:
                            self.enqueue_fuzz(key, cur, f, 'farm', f.get('t') or now)
                        if not rows:
                            break
                        cs['failures'] = r.get('cursor') or rows[-1]['id']
                    info = self.client().call('GET', f'/v1/admin/campaign/{name}')
                    if not cur.get('retired'):
                        flight.setdefault(key, {})[alias] = info.get('seeds_leased') or 0
                except Exception as e:                   # noqa: BLE001
                    self.log(f'{key}: campaign {name}: {e}')
        self.state['farm_log'] = [x for x in self.state.get('farm_log', []) if now - x[0] < DAY]
        online = 0
        try:
            st = self.client().call('GET', '/v1/admin/stats')
            online = sum(1 for m in st.get('machines') or [] if now - (m.get('last_seen') or 0) < 600)
        except Exception as e:                           # noqa: BLE001
            self.log(f'stats: {e}')
        return flight, online

    def note_rate(self, source, alias, seeds, thread_s):
        r = self.state['rates'].setdefault(f'{source}:{alias}', {'seeds': 0, 'thread_s': 0.0})
        r['seeds'] += int(seeds)
        if thread_s:
            r['thread_s'] += float(thread_s)

    def rate(self, source, m):
        r = self.state['rates'].get(f'{source}:{m["alias"]}') or {}
        if r.get('thread_s', 0) > 300:
            return r['seeds'] / r['thread_s']
        return m['rate']

    # -------------------------------------------------------------- the queue
    def touch(self, fid, key, base, t, seed_info):
        e = self.queue.get(fid)
        new = e is None
        if new:
            e = dict(base, id=fid, first_seen=t, last_seen=t, count=0, seeds=[], sources=[], confirmed=None,
                     confirm_note=None, alerted_at=None, alert_tries=0)
            self.queue[fid] = e
        e['count'] += 1
        e['first_seen'] = min(e['first_seen'], t)
        e['last_seen'] = max(e['last_seen'], t)
        if seed_info.get('source') and seed_info['source'] not in e['sources']:
            e['sources'].append(seed_info['source'])
        if len(e['seeds']) < 20:
            e['seeds'].append(seed_info)
        return e, new

    def enqueue_fuzz(self, key, cur, f, source, t):
        from fuzz_farm import panics
        ps = panics(f.get('panic') or '')
        msg = ps[0][1] if ps else [x for x in (f.get('panic') or '').splitlines() if x.strip()][:5]
        fid, k = failure_id(f.get('site'), msg)
        mode = f.get('mode')
        repro = f.get('repro') or ''
        if repro.startswith('(at unknown)') and cur.get('commit'):   # (the Mac's own build of the commit is unsigned)
            repro = f'(at {cur["commit"][:12]})' + repro[len('(at unknown)'):]
        f = dict(f, repro=repro)
        e, new = self.touch(fid, key, {'kind': 'fuzz', 'site': f.get('site'), 'message': msg[:8], 'key': k, 'mode': mode,
                                        'seed': f.get('seed'), 'repro': f.get('repro'), 'build': cur.get('build_id'),
                                        'engine_commit': cur.get('commit'), 'target': key},
                            t, {'seed': f.get('seed'), 'mode': mode, 'source': source, 'build': cur.get('build_id')})
        self.day(t, cur['build_id'])['failures'] += 1
        if new:
            self.log(f'NEW failure {fid} ({source}, {key} {mode} seed {f.get("seed")}): {f.get("site")}')

    def poll_selfplay_failures(self, now):
        """Every run's `game_failures.jsonl` (a game dropped for a panic, docs/ENGINE.md 167.6, 171), read on."""
        offs = self.state.setdefault('gf_offsets', {})
        for p in glob.glob(os.path.join(os.path.dirname(self.a.runs_glob), 'game_failures.jsonl')):
            rd = os.path.dirname(p)
            name = os.path.basename(rd)
            off = offs.get(p, 0)
            try:
                size = os.path.getsize(p)
                if size < off:
                    off = 0
                with open(p, 'rb') as f:
                    f.seek(off)
                    data = f.read()
            except OSError:
                continue
            end = data.rfind(b'\n') + 1
            for line in data[:end].decode('utf-8', 'replace').splitlines():
                try:
                    r = json.loads(line)
                except ValueError:
                    continue
                msg = r.get('message') or r.get('panic') or ''
                lines = msg.splitlines() if isinstance(msg, str) else list(msg)
                site = r.get('site') or (re.search(r'panicked at ([^\s:]+:\d+:\d+)', msg).group(1)
                                         if isinstance(msg, str) and re.search(r'panicked at ([^\s:]+:\d+:\d+)', msg)
                                         else 'unknown')
                fid, k = failure_id(site, lines)
                tgt = self.state['targets'].get(name) or {}
                t = r.get('t') or r.get('time') or now          # (167.6's lines: time, gid, steps, message, iter)
                e, new = self.touch(fid, name, {'kind': 'selfplay', 'site': site, 'message': lines[:8], 'key': k,
                                                'mode': 'selfplay', 'seed': r.get('game_seed'), 'repro': r.get('repro'),
                                                'build': r.get('build') or tgt.get('build_id'),
                                                'engine_commit': r.get('engine_commit') or tgt.get('commit'),
                                                'target': name, 'run_dir': rd},
                                    t, {'seed': r.get('game_seed'), 'game_id': r.get('game_id', r.get('gid')), 'iter': r.get('iter'),
                                        'source': r.get('source') or 'local'})
                if new:
                    self.log(f'NEW self-play failure {fid} ({name} iter {r.get("iter")}, game seed {r.get("game_seed")}): {site}')
            offs[p] = off + end

    def confirm(self, e):
        """Replay a queue entry here -> (confirmed: bool | None, note)."""
        if e['kind'] == 'fuzz':
            tgt = self.state['targets'].get(e['target']) or {}
            exe = None
            for c in [tgt] + list(tgt.get('history') or []):
                if c.get('build_id') == e.get('build'):
                    exe = c.get('exe') or self.find_build(c.get('commit') or '')[1]
            if not exe:
                exe = self.find_build(e.get('engine_commit') or '')[1]
            m = next((m for m in tgt.get('modes') or [] if m['mode'] == e.get('mode')), None)
            if not exe or m is None or e.get('seed') is None:
                return None, 'no local build or mode to replay it with'
            work = os.path.join(self.dir, 'jobs', f'confirm-{e["id"]}')
            argv = [self.py(), os.path.join(HERE, 'fuzz_local.py'), '--worker', exe, '--mode', m['mode'],
                    '--first', str(e['seed']), '--count', '1', '--threads', '1', '--work', work, '--idle']
            for k, v in m['env'].items():
                argv += ['--env', f'{k}={v}']
            try:
                p = subprocess.run(argv, capture_output=True, text=True, timeout=1800, creationflags=NO_WINDOW)
            except subprocess.TimeoutExpired:
                return None, 'the replay timed out'
            fails = [json.loads(x.split(' ', 1)[1]) for x in p.stdout.splitlines() if x.startswith('FUZZ_LOCAL_FAILURE ')]
            if any(f.get('site') == e.get('site') for f in fails):
                return True, f'replayed here: seed {e["seed"]} fails at the same site'
            if fails:
                return True, f'replayed here: seed {e["seed"]} fails, at {fails[0].get("site")}'
            return False, f'replayed here: seed {e["seed"]} passes (exit {p.returncode})'
        repro = e.get('repro')
        if not repro:
            src = set(e.get('sources') or [])
            if src and src <= {'local', 'mac'}:
                return True, 'seen in a run on the owner\'s machines (no repro command in the record)'
            return None, 'no repro command in the record'
        wt = (self.state['targets'].get(e['target']) or {}).get('run_wt')
        if not wt or not os.path.isdir(wt):
            return None, 'no checkout of the run to replay it in'
        if repro.startswith('python '):                  # (the run's Python: the steward's own venv's python.exe)
            repro = subprocess.list2cmdline([self.py()]) + repro[len('python'):]
        try:
            p = subprocess.run(repro, shell=True, cwd=wt, capture_output=True, text=True, timeout=1800,
                               creationflags=NO_WINDOW | (subprocess.IDLE_PRIORITY_CLASS if WIN else 0))
        except subprocess.TimeoutExpired:
            return None, 'the replay timed out'
        # mtgz.az.game_failure replay (ENGINE.md 172): 101 reproduced, 0 not reproduced (a panic inside a search's own
        # state), 2 cannot replay; anything else: the replay itself broke
        if p.returncode == 101:
            return True, 'replayed here: the repro reproduces the panic (exit 101)'
        if p.returncode == 0:
            return False, 'replayed here: the moves replay without the panic (a search-state panic?)'
        return None, f'the repro could not replay it (exit {p.returncode}): {(p.stderr or p.stdout).strip()[-200:]}'

    def process_queue(self):
        changed = False
        for e in self.queue.values():
            if e.get('confirmed') is None and e.get('confirm_tries', 0) < 3:
                ok, note = self.confirm(e)
                e['confirm_tries'] = e.get('confirm_tries', 0) + 1
                e['confirmed'], e['confirm_note'] = ok, note
                self.log(f'failure {e["id"]}: confirmed={ok} ({note})')
                changed = True
            if e.get('confirmed') and not e.get('alerted_at') and e.get('alert_tries', 0) < 5 and self.cfg['alerts']:
                if self.alert(alert_text(e)):
                    e['alerted_at'] = time.time()
                    self.log(f'failure {e["id"]}: the owner was told')
                e['alert_tries'] = e.get('alert_tries', 0) + 1
                changed = True
        if changed:
            self.write_queue()

    def alert(self, text):
        """One message through the game-alerts relay on the Mac (memory: game-alerts; relay.py --notify-b64)."""
        if self.a.dry_run:
            self.log(f'dry run: alert {text!r}')
            return True
        host = local_config.get('MTGZ_MAC')
        if not host:
            self.log('alert: no Mac configured (MTGZ_MAC)')
            return False
        import base64
        b64 = base64.b64encode(text.encode()).decode()
        relay = local_config.get('MTGZ_ALERT_RELAY', '~/game-alerts/relay.py')
        py = local_config.get('MTGZ_ALERT_PYTHON', 'python3')
        cmd = f'{py} {relay} --notify-b64 {b64}'
        try:
            p = subprocess.run(['ssh', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=20', host, cmd], capture_output=True,
                               text=True, timeout=240, creationflags=NO_WINDOW)
        except subprocess.TimeoutExpired:
            return False
        if p.returncode:
            self.log(f'alert: the relay answered {p.returncode}: {(p.stderr or p.stdout).strip()[-300:]}')
        return p.returncode == 0

    # -------------------------------------------------------------- local jobs
    def local_running(self, key, source):
        return {j: v for j, v in (self.state['targets'][key].get('local') or {}).get(source, {}).items() if v.get('running')}

    def start_local(self, key, cur, source, alias, now):
        m = next(m for m in cur['modes'] if m['alias'] == alias)
        threads = self.cfg['mac_threads'] if source == 'mac' else self.cfg['desktop_threads']
        count = max(50, int(self.rate(source, m) * threads * self.cfg['chunk_s']))
        cursors = cur.setdefault('local_cursor', {}).setdefault(source, {})
        first = cursors.get(alias, LOCAL_SEED_BASE[source] + 10_000_000 * len(cursors))
        cursors[alias] = first + count
        jid = f'{source}-{key}-{alias}-{first}'
        logp = os.path.join(self.dir, 'jobs', f'{jid}.log')
        envs = []
        for k, v in m['env'].items():
            envs += ['--env', f'{k}={v}']
        if self.a.dry_run:
            self.log(f'dry run: {source} job {jid} ({count} seeds) would start')
            return
        if source == 'desktop':
            work = os.path.join(self.dir, 'jobs', jid)
            stop = os.path.join(self.dir, 'jobs', f'{jid}.STOP')
            argv = [bash_exe(), 'tools/slotd.sh', self.py(), os.path.join(HERE, 'fuzz_local.py'), '--worker', cur['exe'],
                    '--mode', m['mode'], *envs, '--first', str(first), '--count', str(count), '--threads', str(threads),
                    '--max-s', str(self.cfg['chunk_s'] * 2), '--stop-file', stop, '--work', work, '--idle']
            self.spawn(jid, argv, ROOT, logp, env={'MTGZ_SLOT_WAIT_MAX': '900'})
        else:
            with open(os.path.join(HERE, 'fuzz_local.py'), 'rb') as f:
                import base64
                b64 = base64.b64encode(f.read()).decode()
            mac_cmd = '\n'.join([
                'set -u',
                'CARGO_BUILD_JOBS=6 nice -n 19 cargo build -q --profile fuzz -p mtgz_worker || exit 3',
                f'F="${{TMPDIR:-/tmp}}/fuzz_local_{hashlib.sha1(b64.encode()).hexdigest()[:10]}.py"',
                f"printf '%s' '{b64}' | base64 --decode > \"$F\"",
                'rm -f "$HOME/.fuzz-steward-STOP"',
                f'W="$HOME/fuzz-steward-work/{jid}"',
                'nice -n 19 python "$F" --worker "$CARGO_TARGET_DIR/fuzz/mtgz-worker" ' + ' '.join(
                    shlex.quote(x) for x in ['--mode', m['mode'], *envs, '--first', str(first), '--count', str(count),
                                             '--threads', str(threads), '--max-s', str(self.cfg['chunk_s'] * 2)])
                + ' --stop-file "$HOME/.fuzz-steward-STOP" --work "$W" --idle; RC=$?',
                'rm -rf "$W"', 'exit $RC'])
            wt = self.wt_of(key)
            if not os.path.isdir(wt) or git('rev-parse', 'HEAD', cwd=wt) != cur['commit']:
                self.log(f'{key}: mac job not started: {wt} is not at {cur["commit"][:9]}')
                return
            self.spawn(jid, [bash_exe(), 'tools/mac.sh', mac_cmd], wt, logp)
        cur['local'][source][jid] = {'running': True, 'alias': alias, 'first': first, 'count': count, 'threads': threads,
                                     'started': now, 'log': logp}
        self.log(f'{key}: {source} job {jid}: {m["mode"]} seeds [{first}, {first + count}) x{threads} threads')

    def poll_local(self, now):
        for key, cur in self.state['targets'].items():
            for source in ('mac', 'desktop'):
                for jid, j in list((cur.get('local') or {}).get(source, {}).items()):
                    if not j.get('running'):
                        continue
                    p = self.procs.get(jid)
                    if p is not None and p.poll() is None:
                        continue
                    if p is None and now - j['started'] < 3 * self.cfg['chunk_s']:
                        continue                         # (started by an earlier steward: give it time, then read its log)
                    self.procs.pop(jid, None)
                    j['running'] = False
                    j['ended'] = now
                    summary, fails = None, []
                    try:
                        with open(j['log'], encoding='utf-8', errors='replace') as f:
                            for line in f:
                                if line.startswith('FUZZ_LOCAL_SUMMARY '):
                                    summary = json.loads(line.split(' ', 1)[1])
                                elif line.startswith('FUZZ_LOCAL_FAILURE '):
                                    fails.append(json.loads(line.split(' ', 1)[1]))
                    except (OSError, ValueError):
                        pass
                    done = int((summary or {}).get('done') or 0)
                    j['done'] = done
                    j['seconds'] = (summary or {}).get('seconds')
                    if done:
                        self.credit(now, cur['build_id'], source, j['alias'], done)
                        self.note_rate(source, j['alias'], done, (summary.get('seconds') or 0) * j['threads'])
                    for f in fails:
                        self.enqueue_fuzz(key, cur, f, source, now)
                    if summary and summary.get('error'):
                        self.log(f'{key}: {source} job {jid} broke: {summary["error"]}')
                    self.log(f'{key}: {source} job {jid} ended: {done} seeds, {len(fails)} failures, '
                             f'{(summary or {}).get("seconds")} s (rc {p.returncode if p else "?"})')
                    if source == 'desktop':
                        import shutil
                        shutil.rmtree(os.path.join(self.dir, 'jobs', jid), ignore_errors=True)
                    self.last_check = 0              # the next tick decides whether another chunk follows
                # keep the last 20 finished jobs per source
                jobs = (cur.get('local') or {}).get(source) or {}
                fin = sorted((k for k, v in jobs.items() if not v.get('running')), key=lambda k: jobs[k].get('ended') or 0)
                for k in fin[:-20]:
                    jobs.pop(k, None)

    def stop_local(self, key, why, sources=('mac', 'desktop')):
        cur = self.state['targets'].get(key) or {}
        for source in sources:
            for jid in self.local_running(key, source) if cur else {}:
                if source == 'desktop':
                    open(os.path.join(self.dir, 'jobs', f'{jid}.STOP'), 'w').close()
                else:
                    host = local_config.get('MTGZ_MAC')
                    if host:
                        subprocess.run(['ssh', '-o', 'BatchMode=yes', host, 'touch "$HOME/.fuzz-steward-STOP"'],
                                       capture_output=True, timeout=60, creationflags=NO_WINDOW)
                self.log(f'{key}: {source} job {jid} asked to finish ({why})')

    def desktop_ok(self, now):
        """No live run is slowed past `starve_ratio` of its median before the desktop's fuzz began; not in cooldown."""
        if now < self.state.get('desktop_cooldown_until', 0):
            return False
        base = self.state.setdefault('starve_baseline', {})
        any_desktop = any(self.local_running(k, 'desktop') for k in self.state['targets'])
        for key, cur in self.state['targets'].items():
            if cur.get('kind') != 'run' or cur.get('retired') or not cur.get('run_dir'):
                continue
            secs, elapsed = run_iteration_times(cur['run_dir'])
            if not any_desktop:
                prev = secs[-self.cfg['starve_window']:]
                base[key] = statistics.median(prev) if len(prev) >= 3 else None
                continue
            if starved(secs, self.cfg['starve_ratio'], self.cfg['starve_window'], elapsed, base.get(key)):
                self.state['desktop_cooldown_until'] = now + self.cfg['desktop_cooldown_s']
                self.log(f'{key}: iteration time past {self.cfg["starve_ratio"]}x its median ({base.get(key)}): '
                         f'the desktop backs off for {self.cfg["desktop_cooldown_s"] // 60} min')
                return False
        return True

    # -------------------------------------------------------------- the check
    def check(self, now):
        self.load_cfg()
        self.ensure_targets(now)
        self.poll_builds(now)
        flight, online = self.poll_farm(now)
        self.poll_selfplay_failures(now)
        q = self.cfg['quota']
        dok = self.desktop_ok(now)
        report = {}
        for key, cur in self.state['targets'].items():
            if cur.get('retired') or not cur.get('build_id'):
                continue
            ensure_missing = [m for m in cur['modes'] if m['alias'] not in cur.get('campaigns', {})]
            if ensure_missing:
                self.ensure_campaigns(key, cur)
            rec = self.day(now, cur['build_id'])
            done_by = {}
            for src in ('farm', 'mac', 'desktop'):
                for a_, n in rec[src].items():
                    done_by[a_] = done_by.get(a_, 0) + n
            fl = dict(flight.get(key) or {})
            for src in ('mac', 'desktop'):
                for j in self.local_running(key, src).values():
                    fl[j['alias']] = fl.get(j['alias'], 0) + j['count']
            done, inflight = sum(done_by.values()), sum(fl.values())
            h = deficit_hours(q, now, cur['built_at'], done, inflight)
            boosted = next((a_ for a_, x in (cur.get('shares') or {}).items() if x is not None), None)
            alias, ah = most_behind(cur['modes'], q, now, cur['built_at'], done_by, fl, current=boosted)
            # the farm: one campaign boosted (the most-behind mode's), the rest the backfill
            want = farm_share(h, max((x for x in (cur.get('shares') or {}).values() if x is not None), default=None),
                              self.cfg)
            shares = cur.setdefault('shares', {})
            for a_, name in cur.get('campaigns', {}).items():
                s = want if a_ == alias else None
                if shares.get(a_) != s:
                    if self.put_campaign(name, share=s):
                        shares[a_] = s
                        self.log(f'{key}: {name} share {s} (behind {h:+.2f} h; {a_} {ah:+.2f} h)')
            if want is not None and want >= self.cfg['boost_share']:
                cur['boost_since'] = cur.get('boost_since') or now
            else:
                cur['boost_since'] = None
            # local fill-in
            mac_jobs = self.local_running(key, 'mac')
            mac_since = cur.get('mac_since') if mac_jobs else None
            dec = local_decision(h, now, cur.get('boost_since'), mac_since, online, self.cfg, dok)
            for source in ('mac', 'desktop'):
                running = self.local_running(key, source)
                if dec[source] and not running:
                    if source == 'mac':
                        cur['mac_since'] = cur.get('mac_since') or now
                    self.start_local(key, cur, source, alias, now)
                elif not dec[source] and running and (source == 'desktop' and (h <= 1 or not dok)):
                    self.stop_local(key, 'on pace' if h <= 1 else 'a live run is slowed', sources=('desktop',))
            if not self.local_running(key, 'mac') and not dec['mac']:
                cur['mac_since'] = None
            report[key] = {'build': cur['build_id'], 'commit': cur['commit'][:9], 'today': done, 'in_flight': inflight,
                           'quota_today': round(day_quota(q, now, cur['built_at'])), 'pace': round(pace_target(q, now, cur['built_at'])),
                           'behind_h': round(h, 2), 'mode_behind': alias, 'farm_share': want, 'local': dec,
                           'by_source': {s: sum(rec[s].values()) for s in ('farm', 'mac', 'desktop')},
                           'failures_today': rec['failures']}
        self.process_queue()
        self.daily_summary(now)
        self.state['last_check'] = now
        self.state['report'] = report
        self.state['farm_online'] = online
        self.save()
        self.write_status(now, report, online)
        self.log('check: ' + '; '.join(f'{k} {v["today"]}+{v["in_flight"]}/{v["pace"]} pace '
                                       f'({v["behind_h"]:+.2f} h, share {v["farm_share"]}, mac {v["local"]["mac"]}, '
                                       f'desktop {v["local"]["desktop"]})' for k, v in report.items())
                 + f'; farm PCs online {online}')

    def daily_summary(self, now):
        """One line per build for every UTC day that has ended and has no summary yet."""
        done = set(self.state.setdefault('summarized', []))
        today = day_name(now)
        for d in sorted(self.state['days']):
            if d >= today or d in done:
                continue
            parts = []
            for bkey, rec in self.state['days'][d].items():
                tgt = next((k for k, c in self.state['targets'].items()
                            if c.get('build_id') == bkey or any(h.get('build_id') == bkey for h in c.get('history') or [])), '?')
                by = {s: sum(rec[s].values()) for s in ('farm', 'mac', 'desktop')}
                parts.append(f'{tgt} build {bkey[:8]}: {sum(by.values())} seeds (farm {by["farm"]}, mac {by["mac"]}, '
                             f'desktop {by["desktop"]}), failures {rec["failures"]}')
            line = f'{d} ' + ('; '.join(parts) or 'nothing') + f'; queue {len(self.queue)} sites'
            with open(os.path.join(self.dir, 'daily.log'), 'a', encoding='utf-8') as f:
                f.write(line + '\n')
            self.log('daily: ' + line)
            done.add(d)
        self.state['summarized'] = sorted(done)[-60:]
        for d in sorted(self.state['days'])[:-30]:
            self.state['days'].pop(d, None)

    def write_status(self, now, report, online):
        if not self.a.status_file:
            return
        rates = {k: round(v['seeds'] / v['thread_s'] * 3600) for k, v in self.state['rates'].items() if v.get('thread_s')}
        farm = {}
        for t, key, alias, n in self.state.get('farm_log', []):
            f = farm.setdefault(key, {'last_hour': 0, 'last_day': 0})
            f['last_day'] += n
            if now - t < 3600:
                f['last_hour'] += n
        st = {'updated': now, 'updated_iso': dt.datetime.fromtimestamp(now, dt.timezone.utc).isoformat(timespec='seconds'),
              'quota': self.cfg['quota'], 'targets': report, 'farm_pcs_online': online,
              'local_seeds_per_thread_hour': rates, 'farm_seeds': farm,
              'queue': [{k: e.get(k) for k in ('id', 'kind', 'target', 'mode', 'seed', 'site', 'count', 'confirmed',
                                                'first_seen', 'last_seen', 'repro')}
                        for e in sorted(self.queue.values(), key=lambda e: -(e.get('last_seen') or 0))[:20]]}
        try:
            write_json(self.a.status_file, st)
        except OSError as e:
            self.log(f'status file: {e}')

    # -------------------------------------------------------------- the loop
    def run(self):
        lock = os.path.join(self.dir, 'steward.pid')
        old = read_json(lock, {})
        if old.get('pid') and old['pid'] != os.getpid() and pid_alive(old['pid'], old.get('started')):
            raise SystemExit(f'fuzz_steward: already running (pid {old["pid"]})')
        try:
            import psutil
            started = psutil.Process().create_time()
        except Exception:                                # noqa: BLE001
            started = None
        write_json(lock, {'pid': os.getpid(), 'started': started, 'root': ROOT})
        stop = os.path.join(self.dir, 'STOP')
        self.log(f'steward up (pid {os.getpid()}, {ROOT}, state {self.dir})')
        try:
            while True:
                now = time.time()
                if os.path.exists(stop):
                    self.log('STOP: the steward ends; its local jobs are asked to finish')
                    for key in list(self.state['targets']):
                        self.stop_local(key, 'STOP')
                    os.replace(stop, stop + '.old')
                    break
                try:
                    self.poll_builds(now)
                    self.poll_local(now)
                except Exception as e:                   # noqa: BLE001 - logged; the steward goes on
                    import traceback
                    self.log(f'tick failed: {e}\n{traceback.format_exc()[-2000:]}')
                if now - self.last_check >= self.cfg['check_s'] or self.a.once:
                    self.last_check = now
                    try:
                        self.check(now)
                    except Exception as e:               # noqa: BLE001 - a bad check must not end the steward
                        import traceback
                        self.log(f'check failed: {e}\n{traceback.format_exc()[-2000:]}')
                self.save()
                if self.a.once:
                    break
                time.sleep(self.cfg['tick_s'])
        finally:
            self.save()


# ======================================================================== the logon task

def task_xml(argv, cwd):
    import getpass
    user = os.environ.get('USERDOMAIN', '') + '\\' + getpass.getuser()
    cmd, args = argv[0], subprocess.list2cmdline(argv[1:])
    esc = lambda s: s.replace('&', '&amp;').replace('<', '&lt;').replace('>', '&gt;')  # noqa: E731
    return f'''<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>MTG Zero fuzz steward (tools/fuzz_steward.py; docs/ENGINE.md 171)</Description></RegistrationInfo>
  <Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{esc(user)}</UserId><Delay>PT2M</Delay></LogonTrigger></Triggers>
  <Principals><Principal id="Author"><UserId>{esc(user)}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <RestartOnFailure><Interval>PT5M</Interval><Count>10</Count></RestartOnFailure>
    <Priority>7</Priority>
    <Enabled>true</Enabled>
  </Settings>
  <Actions Context="Author"><Exec><Command>{esc(cmd)}</Command><Arguments>{esc(args)}</Arguments><WorkingDirectory>{esc(cwd)}</WorkingDirectory></Exec></Actions>
</Task>
'''


def install_task(a):
    py = sys.executable
    pyw = os.path.join(os.path.dirname(py), 'pythonw.exe')
    argv = [pyw if os.path.exists(pyw) else py, os.path.join(HERE, 'fuzz_steward.py'), 'run', '--quiet']
    if a.status_file:
        argv += ['--status-file', os.path.abspath(a.status_file)]
    xml = task_xml(argv, ROOT)
    p = os.path.join(state_dir(), 'task.xml')
    os.makedirs(os.path.dirname(p), exist_ok=True)
    with open(p, 'w', encoding='utf-16') as f:
        f.write(xml)
    r = subprocess.run(['schtasks', '/Create', '/TN', TASK_NAME, '/XML', p, '/F'], capture_output=True, text=True,
                       creationflags=NO_WINDOW)
    print((r.stdout + r.stderr).strip())
    return r.returncode


# ======================================================================== the commands

def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('cmd', choices=('run', 'status', 'stop', 'install-task', 'uninstall-task', 'confirm'))
    ap.add_argument('id', nargs='?')
    ap.add_argument('--once', action='store_true')
    ap.add_argument('--dry-run', action='store_true')
    ap.add_argument('--quiet', action='store_true')
    ap.add_argument('--state', default=None)
    ap.add_argument('--status-file', default=None)
    ap.add_argument('--runs-glob', default=None)
    a = ap.parse_args(argv)
    if a.runs_glob is None:
        runs = local_config.get('MTGZ_RUNS_ROOT') or local_config.big_disk_path('mtg-zero', 'runs')
        a.runs_glob = os.path.join(runs or os.path.join(ROOT, 'runs'), '*', 'supervisor_state.json')
    if a.cmd == 'stop':
        open(os.path.join(a.state or state_dir(), 'STOP'), 'w').close()
        print('STOP written: the steward ends at its next tick')
        return 0
    if a.cmd == 'install-task':
        return install_task(a)
    if a.cmd == 'uninstall-task':
        return subprocess.run(['schtasks', '/Delete', '/TN', TASK_NAME, '/F'], creationflags=NO_WINDOW).returncode
    s = Steward(a)
    if a.cmd == 'status':
        st = s.state
        print(json.dumps({'last_check': st.get('last_check'), 'report': st.get('report'), 'farm_online': st.get('farm_online'),
                          'targets': {k: {x: v.get(x) for x in ('commit', 'build_id', 'built_at', 'campaigns', 'shares',
                                                               'retired', 'building', 'pending')}
                                      for k, v in st['targets'].items()},
                          'queue': list(s.queue.values())}, indent=1))
        return 0
    if a.cmd == 'confirm':
        e = s.queue.get(a.id)
        if not e:
            raise SystemExit(f'no queue entry {a.id}')
        ok, note = s.confirm(e)
        e['confirmed'], e['confirm_note'] = ok, note
        s.write_queue()
        print(json.dumps({'id': a.id, 'confirmed': ok, 'note': note}))
        return 0
    signal.signal(signal.SIGTERM, lambda *_: open(os.path.join(s.dir, 'STOP'), 'w').close())
    try:
        s.run()
    except SystemExit:
        raise
    except BaseException as e:                           # (pythonw has no console: the reason goes to the log)
        import traceback
        s.log(f'the steward ended on an error: {e}\n{traceback.format_exc()[-3000:]}')
        raise
    return 0


if __name__ == '__main__':
    sys.exit(main())
