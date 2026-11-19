"""The farm release's speed gate (docs/ENGINE.md 199): a new farm worker build (and the network export it will play)
against the previous published release, in interleaved A/B windows on each reference machine; the release fails loudly
when it got slower.

    python tools/farm_speed.py gate --new <build dir | build id> [--baseline <build id | dir>] [--machines cpu[,mac][,cuda]]
        [--model reference | run:<run dir> | <model.onnx>] [--model-new <model.onnx>] [--config <run config.json>]
        [--minutes 3] [--warmup 30] [--blocks 1] [--retry-blocks 1] [--tolerance 0.05]
        [--idle-min 180] [--idle-wait-max 14400] [--idle-redo 3] [--job MACHINE:key=JSON ...]
        [--cuda-new <dir | id>] [--cuda-baseline <id>] [--mac-new <dir | id>] [--mac-baseline <id>] [--out DIR]
    python tools/farm_gates.py --speed ...                 # the same (farm_gates.py hands it over)
    python tools/farm_speed.py judge <out dir> [--tolerance 0.05]    # the recorded windows judged again
    python tools/farm_speed.py windows --plan plan.json --out DIR --order new,old,old,new [--first 0]   # the runner

**What it compares.** Two sides, each a worker build with its runtime pack and a network export: `new` (the release's
build) and `old` (the baseline: `--baseline`, else the previous published release build in the farm builds index -
the newest published entry before the new build's first entry whose build differs, the way farm_release.py and
farm_release_watch.py track releases; the macOS / CUDA builds the same way from the entries' `mac` / `cuda` fields).
Both play one network: `--model` (default `reference`: the release reference export, farm_release.py `reference`;
`run:<run dir>`: the run's newest complete pool export) - the old side always, the new side too unless `--model-new`
names another export **of the same checkpoint** (farm_release.py passes the commit's own export of the reference
checkpoint, so an export change is measured with the build: what the farm gets after the release vs before). The
exports' sha256, checkpoint, zero-onnx version and graph digest (the model without its metadata) are recorded.

**How.** Per machine, blocks of windows in the order new, old, old, new (`--blocks`, default 1; every other block
mirrored, old, new, new, old, so that over two blocks each side takes each position as often); each window a rolling
self-play job (`<worker> serve`, the farm's job line: `rolling`, records output, the machine's layout) for `--minutes`
(3); its rate is the network leaves per second (the like-for-like metric; games and rows per game drift) after
`--warmup` seconds (30): the upper quartile of the 10-second rates between the worker's status lines (other load only
ever slows a window down: the upper quartile is its uncontended speed, blind to a burst over up to three quarters of
it; the median and the span's mean are recorded too). The machine's load (CPU %, on Windows also Task Manager's
utility, the worker's share, the others', available memory) is sampled every 10 s: context only, never the verdict.
Reference machines and their layouts (release.json `speed.machines` overrides; `--job cpu:conc=512`):

* `cpu` - the desktop with the Windows CPU pack (each build's `requires_runtime` under the builds directory); shared
  with live training, so the worker runs at below-normal priority and only the ratio means anything (the interleaving
  is what makes it fair);
* `mac` - the Mac with the macOS build and pack (`~/mtgz-macfarm/builds/<id>/`, `~/mtgz-macfarm/packs/<version>/`,
  copied there from the builds directory when missing; tools/mac.sh's `MTGZ_MAC`): this file, the plan and the exports go
  to `~/mtgz-macfarm/speed/<stamp>/` and its `windows` command runs there at `nice 15`, the agent's own layout (threads
  16, 2,304 games in 12 groups); each sweep books `~/mtgz-macfarm/bench.lock` (owner "<purpose> <unix time>"; waited for,
  never taken over) and holds `~/domz-remote/.eval.lock` with a live `sleep` as its owner so a run's Mac self-play backs
  off, and starts no window while `~/mtgz-remote/.eval.lock` is held (a run's evaluation);
* `cuda` - the desktop's GPU with the CUDA alternates and the CUDA pack: off by default (the GPU trains live runs);
  when on, tiny (1-minute windows, one block).

**Only on an idle machine** (the user's rule, 2026-11-18). Before each window the runner waits until the machine has
had no user input for `--idle-min` seconds (180; Windows: GetLastInputInfo, macOS: IOHIDSystem's HIDIdleTime) and is
on AC power (Windows: GetSystemPowerStatus, macOS: `pmset -g batt`), at most `--idle-wait-max` seconds (4 h). During
the window and at its end it checks again: input during the window (an idle time shorter than the window so far) or
battery power discards the window, which is run again after the next idle wait - at most `--idle-redo` times (3) per
block, then the gate FAILS as "machine in use" (never a pass). An idle time the machine cannot tell fails too. Every
check (idle seconds, AC, how long it waited) is recorded with its window.

**Pass rule, per machine:** mean(new windows' leaves/s) >= (1 - `--tolerance`) x mean(old windows'), tolerance 5%;
any window that errors (the worker exits, an `error` event, no leaves, the wrong build id in its `info`) fails the
machine, as does a machine in use (and a machine that cannot start: no baseline, a missing build). **Bursts of other
load** (`--busy-margin`, 20 points): a window's rate leaves out the 10-second intervals in which the others' share of
the CPU rose that many points above the machine's baseline (the median of all the machine's samples: a constant
background is the baseline); a window with fewer than half its intervals clean is contended, left out, and makes the
machine noisy (a side with none left: "machine busy"). A failing machine that is noisy - contended windows, the windows
of one side spread by more than the tolerance, or the two neighbouring pairs' ratios (new0/old1, new3/old2) differ by
more than it - gets `--retry-blocks` more blocks (1) and is judged on all its windows. The gate passes iff every
machine run passed.

**Files** (`--out`; farm_release.py: `<builds>/releases/<sha12>/speed/`): `result.json` (the verdict, the line, each
machine's ratio and rates, the baseline, the network), `hints.json` (hardware-class hints: each machine's hardware,
the layout it ran - threads, infer_workers x infer_threads, groups, games in flight - and its rates: seeds for the farm
agent's autotuner; recorded only), `<machine>/machine.json` (the plan, every window with its status series, load and
idle samples, the judge), `<machine>/w<i>-<side>/` (the worker's events and stderr). Two invocations into one
directory (farm_release.py: the desktop's machines under a slot, then the Mac) combine into one result.

The `windows` command is self-contained (the standard library; psutil when present) so that it runs on the Mac
without a checkout.
"""
import argparse
import datetime
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import threading
import time

TOOLS = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(TOOLS)
SCHEMA = 1
ORDER = ['new', 'old', 'old', 'new']
SPEED_DEFAULTS = {
    'tolerance': 0.05,
    'minutes': 3.0,
    'warmup_s': 30.0,
    'blocks': 1,
    'retry_blocks': 1,
    'idle_min_s': 180.0,
    'idle_wait_max_s': 4 * 3600.0,
    'idle_redo': 3,
    'busy_margin_pct': 20.0,       # (a 10 s interval with other load this far above the machine's baseline is left out)
    'model': 'reference',
    'export': 'commit',             # farm_release.py: the new side plays the commit's own export of the reference
    'machines': {
        'cpu': {'job': {'threads': 8, 'infer_threads': 4, 'conc': 256, 'groups': 4}, 'priority': 'below_normal'},
        # (the agent's own macOS layout: threads 16 - the build splits them into 5 sessions x 3 -, 2,304 games in 12 groups)
        'mac': {'job': {'threads': 16, 'conc': 2304, 'groups': 12}, 'nice': 15,
                'quiet': {'hold': '~/domz-remote/.eval.lock', 'avoid': ['~/mtgz-remote/.eval.lock'], 'settle_s': 60},
                'bench_lock': '~/mtgz-macfarm/bench.lock'},
        'cuda': {'job': {'threads': 4, 'conc': 128, 'groups': 2, 'providers': ['cuda']}, 'priority': 'below_normal',
                 'minutes': 1.0, 'warmup_s': 15.0, 'blocks': 1, 'retry_blocks': 0},
    },
}
JOB_BASE = {'rolling': True, 'flush_s': 60, 'output': 'records'}
LABELS = {'cpu': 'the desktop, CPU', 'mac': 'the Mac', 'cuda': 'the desktop, CUDA'}
MAC_HOME_DIR = 'mtgz-macfarm'
BELOW_NORMAL, NO_WINDOW = 0x00004000, 0x08000000


def now():
    return datetime.datetime.now(datetime.timezone.utc).astimezone().isoformat(timespec='seconds')


def say(m):
    print(f'[farm_speed {time.strftime("%H:%M:%S")}] {m}', flush=True)


# ================================================================================================ the runner (any machine)

def idle_state():
    """(seconds since the machine's last user input or None when it cannot tell, on AC power: True / False / None)."""
    if os.name == 'nt':
        return _idle_windows(), _ac_windows()
    if sys.platform == 'darwin':
        return _idle_mac(), _ac_mac()
    return None, None


def _idle_windows():
    try:
        import ctypes

        class LASTINPUTINFO(ctypes.Structure):
            _fields_ = [('cbSize', ctypes.c_uint), ('dwTime', ctypes.c_uint)]
        li = LASTINPUTINFO()
        li.cbSize = ctypes.sizeof(li)
        if not ctypes.windll.user32.GetLastInputInfo(ctypes.byref(li)):
            return None
        k = ctypes.windll.kernel32
        k.GetTickCount.restype = ctypes.c_uint
        return ((k.GetTickCount() - li.dwTime) & 0xFFFFFFFF) / 1000.0
    except Exception:                              # noqa: BLE001 - unknown
        return None


def _ac_windows():
    try:
        import ctypes

        class SYSTEM_POWER_STATUS(ctypes.Structure):
            _fields_ = [('ACLineStatus', ctypes.c_ubyte), ('BatteryFlag', ctypes.c_ubyte), ('BatteryLifePercent', ctypes.c_ubyte),
                        ('SystemStatusFlag', ctypes.c_ubyte), ('BatteryLifeTime', ctypes.c_ulong), ('BatteryFullLifeTime', ctypes.c_ulong)]
        s = SYSTEM_POWER_STATUS()
        if not ctypes.windll.kernel32.GetSystemPowerStatus(ctypes.byref(s)):
            return None
        return {0: False, 1: True}.get(s.ACLineStatus)
    except Exception:                              # noqa: BLE001
        return None


def _idle_mac():
    try:
        out = subprocess.run(['ioreg', '-c', 'IOHIDSystem'], capture_output=True, text=True, timeout=20).stdout
        m = re.search(r'"HIDIdleTime"\s*=\s*(\d+)', out)
        return int(m.group(1)) / 1e9 if m else None
    except Exception:                              # noqa: BLE001
        return None


def _ac_mac():
    try:
        return parse_pmset(subprocess.run(['pmset', '-g', 'batt'], capture_output=True, text=True, timeout=20).stdout)
    except Exception:                              # noqa: BLE001
        return None


def parse_pmset(out):
    """`pmset -g batt`'s first line -> on AC power (True / False), None when it says neither."""
    if "'AC Power'" in out:
        return True
    if "'Battery Power'" in out:
        return False
    return None


def pid_alive(pid):
    if os.name == 'nt':
        try:
            import psutil
            return psutil.pid_exists(pid)
        except ImportError:
            return True
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False
    except OSError:
        return True                                # another user's process: alive


def lock_held(path):
    """An evaluation lock directory (zero-core's zero/remote.py: `owner` = "<pid> <time>") held by a live owner, or
    being created (no owner yet) - tools/mac_backoff.py's rule."""
    p = expand(path)
    if not os.path.isdir(p):
        return False
    try:
        with open(os.path.join(p, 'owner'), encoding='utf-8') as f:
            pid = int(f.read().split()[0])
    except (OSError, ValueError, IndexError):
        return True
    return pid_alive(pid)


class Quiet:
    """A quiet machine for a block of windows (plan `quiet`; the Mac): holds `hold` - another remote's evaluation lock,
    which a run's self-play workers on the machine back off for - with a live `sleep` as its owner (a holder that dies
    leaves a stale lock: the workers resume, so it fails safe), never while a `avoid` lock is held; released (holder
    killed, owner removed, directory removed) at the block's end and while waiting for the machine to be idle."""

    def __init__(self, cfg, hold_s, log=say):
        self.cfg, self.hold_s, self.log = cfg, min(float(hold_s), 3.5 * 3600), log
        self.holder = None
        self.records = []

    @property
    def held(self):
        return self.holder is not None and self.holder.poll() is None

    def acquire(self, wait_max=4 * 3600.0):
        lock = expand(self.cfg['hold'])
        t0, took_over = time.time(), False
        os.makedirs(os.path.dirname(lock), exist_ok=True)
        while True:
            try:
                os.mkdir(lock)
                break
            except FileExistsError:
                if lock_held(lock):
                    if time.time() - t0 > wait_max:
                        return {'ok': False, 'why': f'{self.cfg["hold"]} is held by another evaluation for {wait_max:.0f} s'}
                    time.sleep(15)
                    continue
                shutil.rmtree(lock, ignore_errors=True)        # stale (its owner is gone): zero-core's rule
                took_over = True
        self.holder = subprocess.Popen(['sleep', str(int(self.hold_s))], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                       stderr=subprocess.DEVNULL)
        with open(os.path.join(lock, 'owner'), 'w', encoding='utf-8') as f:
            f.write(f'{self.holder.pid} {int(time.time())}')
        settle = float(self.cfg.get('settle_s', 60))
        self.log(f'holding {self.cfg["hold"]} (holder pid {self.holder.pid}); {settle:.0f} s for the run\'s workers to back off')
        time.sleep(settle)
        rec = {'ok': True, 'lock': self.cfg['hold'], 'holder': self.holder.pid, 'acquired': now(), 'waited_s': round(time.time() - t0 - settle, 1),
               'settle_s': settle, 'took_over_stale': took_over}
        self.records.append(rec)
        return rec

    def release(self):
        if self.holder is None:
            return
        lock = expand(self.cfg['hold'])
        pid = self.holder.pid
        try:
            self.holder.kill()
            self.holder.wait(10)
        except Exception:                          # noqa: BLE001
            pass
        self.holder = None
        try:
            with open(os.path.join(lock, 'owner'), encoding='utf-8') as f:
                mine = f.read().split()[:1] == [str(pid)]
        except OSError:
            mine = False
        if mine:
            try:
                os.remove(os.path.join(lock, 'owner'))
                os.rmdir(lock)
            except OSError:
                pass
        if self.records:
            self.records[-1]['released'] = now()
        self.log(f'released {self.cfg["hold"]}')


class BenchLock:
    """The machine's benchmark booking (plan `bench_lock`; the Mac: `~/mtgz-macfarm/bench.lock`, the maintainer's rule
    for agents benchmarking there): a directory made atomically with an `owner` "<purpose> <unix time>", held for a whole
    sweep of windows (one block: keep sweeps short so others get turns), waited for while another holds it (retried
    every minute; never taken over: it has no pid to tell a stale one by), removed when the sweep ends."""

    def __init__(self, path, purpose, log=say):
        self.path, self.purpose, self.log = path, purpose, log
        self.mine = False
        self.record = None

    def acquire(self, wait_max=4 * 3600.0, poll=60.0):
        p = expand(self.path)
        os.makedirs(os.path.dirname(p), exist_ok=True)
        t0, said = time.time(), False
        while True:
            try:
                os.mkdir(p)
                break
            except FileExistsError:
                try:
                    with open(os.path.join(p, 'owner'), encoding='utf-8') as f:
                        holder = f.read().strip()
                except OSError:
                    holder = '?'
                if time.time() - t0 > wait_max:
                    return {'ok': False, 'why': f'{self.path} held by "{holder}" for {wait_max:.0f} s', 'lock': self.path}
                if not said:
                    self.log(f'waiting for {self.path} (held by "{holder}")')
                    said = True
                time.sleep(poll)
        with open(os.path.join(p, 'owner'), 'w', encoding='utf-8') as f:
            f.write(f'{self.purpose} {int(time.time())}')
        self.mine = True
        self.record = {'ok': True, 'lock': self.path, 'purpose': self.purpose, 'acquired': now(), 'waited_s': round(time.time() - t0, 1)}
        self.log(f'booked {self.path} ({self.purpose})')
        return self.record

    def release(self):
        if not self.mine:
            return
        p = expand(self.path)
        try:
            with open(os.path.join(p, 'owner'), encoding='utf-8') as f:
                ours = f.read().split()[:1] == [self.purpose]
        except OSError:
            ours = False
        if ours:
            try:
                os.remove(os.path.join(p, 'owner'))
                os.rmdir(p)
            except OSError:
                pass
        self.mine = False
        if self.record:
            self.record['released'] = now()
        self.log(f'released {self.path}')


def held_avoid(avoid):
    return next((x for x in avoid or [] if lock_held(x)), None)


def wait_idle(idle, log=say, clock=time.time, sleep=time.sleep, avoid=None, on_wait=None):
    """Wait until the machine has had no user input for idle['min_s'] seconds, is not on battery and no `avoid` lock is
    held (at most idle['wait_max_s']) -> {ok, idle_s, ac, waited_s, checks, why?, in_use?}. `on_wait` is called once
    when it has to wait."""
    t0 = clock()
    checks, said = 0, None
    min_s, wait_max = float(idle.get('min_s', 180)), float(idle.get('wait_max_s', 4 * 3600))
    while True:
        s, ac = idle_state()
        busy = held_avoid(avoid)
        checks += 1
        rec = {'idle_s': None if s is None else round(s, 1), 'ac': ac, 'waited_s': round(clock() - t0, 1), 'checks': checks,
               'at': now()}
        if busy:
            rec['busy'] = busy
        if s is None:
            return dict(rec, ok=False, why='the machine cannot tell its idle time (no user-input clock): not benchmarked')
        if s >= min_s and ac is not False and not busy:
            return dict(rec, ok=True)
        if clock() - t0 >= wait_max:
            if busy:
                return dict(rec, ok=False, why=f'machine busy: {busy} held for {wait_max:.0f} s')
            return dict(rec, ok=False, in_use=True, why=f'machine in use: no {min_s:.0f} s without user input on AC power '
                                                         f'within {wait_max:.0f} s (idle {s:.0f} s, AC {ac})')
        if checks == 1 and on_wait:
            on_wait()
        if said is None or clock() - said >= 600:
            log(f'waiting for the machine to be idle: no input for {s:.0f} s of {min_s:.0f}, AC {ac}'
                + (f', {busy} held' if busy else ''))
            said = clock()
        sleep(max(5.0, min(30.0, min_s - s if (ac is not False and not busy) else 30.0)))


WIN_UTILITY = r'\Processor Information(_Total)\% Processor Utility'


def utility_reader(every, latest):
    """Windows: Task Manager's CPU % (the counter `WIN_UTILITY`, typeperf every `every` s) into latest['utility_pct'];
    -> the typeperf process (None when it cannot start). The CPU-time counters psutil reads can under-report on a
    hybrid CPU (measured on the desktop: 8 spinning threads of 32 read as 12% "Processor Time", 66% utility)."""
    try:
        tp = subprocess.Popen(['typeperf', WIN_UTILITY, '-si', str(max(1, int(every)))], stdout=subprocess.PIPE,
                              stderr=subprocess.DEVNULL, stdin=subprocess.DEVNULL, text=True, creationflags=NO_WINDOW)
    except OSError:
        return None

    def read():
        for ln in tp.stdout:
            parts = [x.strip('"') for x in ln.strip().split('","')]
            if len(parts) == 2:
                try:
                    latest['utility_pct'] = round(float(parts[1].rstrip('"')), 1)
                except ValueError:
                    pass
    threading.Thread(target=read, daemon=True).start()
    return tp


def sampler(stop, samples, pid, every=10.0):
    """Every `every` s: the machine's CPU % (psutil's, and on Windows Task Manager's `utility_pct`), the worker's share
    of the machine, the others', available memory, python processes, the idle seconds and AC. Context for the windows:
    the verdict is the interleaved ratio of leaves/s (wall clock), never these."""
    try:
        import psutil
    except ImportError:
        psutil = None
    latest = {}
    tp = utility_reader(every, latest) if os.name == 'nt' else None
    n = os.cpu_count() or 1
    proc = None
    if psutil:
        psutil.cpu_percent(None)
        try:
            proc = psutil.Process(pid)
            proc.cpu_percent(None)
        except Exception:                          # noqa: BLE001
            proc = None
    t0 = time.time()
    while not stop.wait(every):
        s = {'t': round(time.time() - t0, 1)}
        try:
            if psutil:
                s['cpu_pct'] = psutil.cpu_percent(None)
                if proc is not None:
                    try:
                        s['worker_pct'] = round(proc.cpu_percent(None) / n, 1)
                        s['others_pct'] = round(max(0.0, s['cpu_pct'] - s['worker_pct']), 1)
                    except Exception:              # noqa: BLE001 - the worker is gone
                        pass
                s['avail_gb'] = round(psutil.virtual_memory().available / 2 ** 30, 1)
                if len(samples) % 6 == 0:            # (once a minute: listing the processes takes seconds on Windows)
                    s['py_procs'] = sum(1 for q in psutil.process_iter(['name'])
                                        if (q.info.get('name') or '').lower().startswith('python'))
            elif os.name == 'nt':
                out = subprocess.run(['wmic', 'cpu', 'get', 'loadpercentage'], capture_output=True, text=True, timeout=20).stdout
                vals = [int(x) for x in out.split() if x.isdigit()]
                if vals:
                    s['cpu_pct'] = sum(vals) / len(vals)
            if os.name != 'nt':
                s['load1_pct'] = round(100.0 * os.getloadavg()[0] / n, 1)
            if 'utility_pct' in latest:
                s['utility_pct'] = latest['utility_pct']
            idle, ac = idle_state()
            s['idle_s'], s['ac'] = (None if idle is None else round(idle, 1)), ac
        except Exception:                          # noqa: BLE001 - a missed sample
            pass
        samples.append(s)
    if tp is not None:
        tp.kill()


def load_summary(samples):
    def mean(k):
        v = [s[k] for s in samples if isinstance(s.get(k), (int, float))]
        return round(sum(v) / len(v), 1) if v else None
    others = [s['others_pct'] for s in samples if isinstance(s.get('others_pct'), (int, float))]
    return {'samples': len(samples), 'utility_pct': mean('utility_pct'), 'cpu_pct': mean('cpu_pct'), 'worker_pct': mean('worker_pct'),
            'others_pct': mean('others_pct'),
            'others_pct_max': max(others) if others else None, 'load1_pct': mean('load1_pct'), 'avail_gb_min':
            min((s['avail_gb'] for s in samples if isinstance(s.get('avail_gb'), (int, float))), default=None)}


def quantile(v, p):
    v = sorted(v)
    k = (len(v) - 1) * p
    lo = int(k)
    hi = min(lo + 1, len(v) - 1)
    return v[lo] + (v[hi] - v[lo]) * (k - lo)


def steady_rate(statuses, warmup_s, end=None):
    """A window's leaves/s after `warmup_s` -> {leaves_per_s: the UPPER QUARTILE of the rates between neighbouring status
    lines (other load on a shared machine only ever slows a window down, so the upper quartile estimates the window's
    uncontended speed and ignores a burst covering up to three quarters of it; the plain span rate when there are fewer
    than 3), median, mean (the span's rate: the first status after the warmup to the last), from_s, to_s, leaves,
    intervals, steady}; fewer than two statuses after the warmup: the whole job from the end's (else the last status's)
    counters, steady False."""
    pts = [(float(s['wall_s']), int(s['leaves'])) for s in statuses if isinstance(s, dict) and 'wall_s' in s and 'leaves' in s]
    after = [p for p in pts if p[0] >= warmup_s]
    if len(after) >= 2 and after[-1][0] > after[0][0]:
        (w0, l0), (w1, l1) = after[0], after[-1]
        mean = (l1 - l0) / (w1 - w0)
        rates = [(b[1] - a[1]) / (b[0] - a[0]) for a, b in zip(after, after[1:]) if b[0] > a[0]]
        n = len(rates)
        upper, med = (quantile(rates, 0.75), quantile(rates, 0.5)) if n >= 3 else (mean, mean)
        return {'leaves_per_s': round(upper, 1), 'median': round(med, 1), 'mean': round(mean, 1), 'from_s': w0, 'to_s': w1,
                'leaves': l1 - l0, 'intervals': n, 'steady': True}
    last = end if isinstance(end, dict) and end.get('wall_s') else (dict(zip(('wall_s', 'leaves'), pts[-1])) if pts else None)
    if not last or not last.get('wall_s'):
        return {'leaves_per_s': 0.0, 'from_s': None, 'to_s': None, 'leaves': 0, 'steady': False}
    return {'leaves_per_s': round(int(last['leaves']) / float(last['wall_s']), 1), 'from_s': 0.0, 'to_s': float(last['wall_s']),
            'leaves': int(last['leaves']), 'steady': False}


def expand(p):
    return os.path.abspath(os.path.expanduser(p)) if isinstance(p, str) and p else p


def _nicer(n):
    def f():
        try:
            if os.getpriority(os.PRIO_PROCESS, 0) < n:
                os.setpriority(os.PRIO_PROCESS, 0, n)
        except OSError:
            pass
    return f


def worker_info(side):
    """The side's `<worker> info --ort-dll none` -> its JSON (build_id, features, platform), or {'error': ...}."""
    cmd = side.get('info_cmd') or [expand(side['worker']), 'info', '--ort-dll', 'none']
    kw = {'creationflags': NO_WINDOW} if os.name == 'nt' else {}
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, timeout=120, stdin=subprocess.DEVNULL, **kw)
    except (OSError, subprocess.TimeoutExpired) as e:
        return {'error': f'{type(e).__name__}: {e}'}
    for ln in reversed(p.stdout.splitlines()):
        if ln.strip().startswith('{'):
            try:
                return json.loads(ln)
            except ValueError:
                pass
    return {'error': f'exit {p.returncode}: {(p.stdout + p.stderr).strip()[-400:]}'}


def run_window(plan, side_name, index, out_root, idle_ok=None, log=say, quiet=None):
    """One window of `side_name` (module docs) -> its record. `idle_ok`: the idle wait before it, `quiet`: the lock
    held for it (both recorded). A window with user input during it, on battery, or with a `quiet.avoid` lock taken
    during it comes back `discarded`."""
    side = plan['sides'][side_name]
    d = os.path.join(out_root, f'w{index}-{side_name}')
    shutil.rmtree(d, ignore_errors=True)
    os.makedirs(d)
    rec = {'index': index, 'side': side_name, 'build_id': side.get('build_id'), 'ok': False, 'started': now(),
           'idle': {'before': idle_ok}}
    if quiet:
        rec['quiet'] = quiet
    avoid = (plan.get('quiet') or {}).get('avoid')
    cmd = side.get('cmd') or [expand(side['worker']), 'serve']
    env = dict(os.environ)
    env.update(plan.get('env') or {})
    env.update(side.get('env') or {})
    env['ZERO_WORKER_BATCH_HIST'] = '1'
    for k in side.get('unset_env') or []:
        env.pop(k, None)
    for p in side.get('path_prepend') or []:
        env['PATH'] = expand(p) + os.pathsep + env.get('PATH', '')
    kw = {}
    if os.name == 'nt':
        kw['creationflags'] = NO_WINDOW | (BELOW_NORMAL if plan.get('priority') == 'below_normal' else 0)
    elif plan.get('nice'):
        kw['preexec_fn'] = _nicer(int(plan['nice']))
    events = []
    evf = open(os.path.join(d, 'events.jsonl'), 'w', encoding='utf-8')
    errf = open(os.path.join(d, 'stderr.log'), 'w', encoding='utf-8')
    t0 = time.time()
    p = subprocess.Popen(cmd, cwd=d, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=errf, text=True, encoding='utf-8',
                         errors='replace', bufsize=1, env=env, **kw)
    try:
        return _window(plan, side, d, rec, p, events, evf, errf, t0, avoid, index)
    except BaseException:
        if p.poll() is None:                       # (interrupted: the worker never outlives its runner)
            p.kill()
        raise


def _window(plan, side, d, rec, p, events, evf, errf, t0, avoid, index):

    def reader():
        for ln in p.stdout:
            ln = ln.strip()
            if not ln:
                continue
            try:
                e = json.loads(ln)
            except ValueError:
                e = {'ev': '?', 'line': ln[:500]}
            if not isinstance(e, dict):
                e = {'ev': '?', 'line': ln[:500]}
            e['_t'] = round(time.time() - t0, 2)
            events.append(e)
            evf.write(json.dumps(e) + '\n')
            evf.flush()
    th = threading.Thread(target=reader, daemon=True)
    th.start()
    stop, samples = threading.Event(), []
    sth = threading.Thread(target=sampler, args=(stop, samples, p.pid, float(plan.get('sample_s', 10))), daemon=True)
    sth.start()
    job = dict(JOB_BASE)
    job.update(plan.get('job') or {})
    job.update(op='job', lease_id=f'speed-{index}', kind='selfplay', settings=plan['settings'], games=1_000_000, gid_base=0,
               seed=int(plan['seed']), model=expand(side['model']), out_dir='out')
    if side.get('runtime_dir'):
        job['runtime_dir'] = expand(side['runtime_dir'])
    if plan.get('gen') is not None:
        job['gen'] = int(plan['gen'])
    error = discard = None
    t_start = time.time()
    try:
        p.stdin.write(json.dumps(job) + '\n')
        p.stdin.flush()
    except OSError as e:
        error = f'the job line: {e}'
    window_s = float(plan['minutes']) * 60
    idle_cfg = plan.get('idle') or {}
    t_check = 0.0
    while error is None and time.time() - t_start < window_s:
        bad = [e for e in events if e.get('ev') in ('error', 'end')]
        if not bad and p.poll() is not None:
            th.join(5)                             # (its last lines: an error event says more than the exit status)
            bad = [e for e in events if e.get('ev') in ('error', 'end')]
            if not bad:
                error = (f'the worker exited (status {p.returncode}) {time.time() - t_start:.0f} s into the window; stderr: '
                         f'{_tail(os.path.join(d, "stderr.log"))}')
                break
        if bad:
            error = f'the worker ended the job early: {json.dumps(bad[-1])[:600]}'
            break
        if idle_cfg.get('check', True) and time.time() - t_check >= float(idle_cfg.get('poll_s', 10)):
            t_check = time.time()
            s, ac = idle_state()
            if s is not None and s + 2 < t_check - t_start:
                discard = f'user input {t_check - t_start - s:.0f} s into the window (idle {s:.0f} s)'
                break
            if ac is False:
                discard = 'on battery power during the window'
                break
            busy = held_avoid(avoid)
            if busy:
                discard = f'{busy} taken {t_check - t_start:.0f} s into the window'
                break
        time.sleep(0.5)
    t_stop = time.time()
    if error is None and discard is None and idle_cfg.get('check', True):
        s, ac = idle_state()
        rec['idle']['end'] = {'idle_s': None if s is None else round(s, 1), 'ac': ac, 'window_s': round(t_stop - t_start, 1)}
        if s is None:
            error = 'the machine cannot tell its idle time at the window\'s end'
        elif s + 2 < t_stop - t_start:
            discard = f'user input during the window (idle {s:.0f} s at its end, the window {t_stop - t_start:.0f} s)'
        elif ac is False:
            discard = 'on battery power at the window\'s end'
    try:
        p.stdin.write(json.dumps({'op': 'quit'}) + '\n')
        p.stdin.flush()
    except OSError:
        pass
    deadline = time.time() + float(plan.get('quit_s', 300))
    while time.time() < deadline and p.poll() is None and not any(e.get('ev') in ('end', 'error') for e in events):
        time.sleep(0.2)
    try:
        p.wait(60)
    except subprocess.TimeoutExpired:
        p.kill()
        p.wait(30)
    try:
        p.stdin.close()
    except OSError:                                # (a worker that died early: the quit line stays unsent)
        pass
    th.join(10)
    stop.set()
    sth.join(30)
    evf.close()
    errf.close()
    if not plan.get('keep_output'):
        shutil.rmtree(os.path.join(d, 'out'), ignore_errors=True)
    ends = [e for e in events if e.get('ev') in ('end', 'error')]
    end = ends[-1] if ends else None
    if error is None and discard is None:
        if end is None:
            error = f'no end event after quit (status {p.returncode}); stderr: {_tail(os.path.join(d, "stderr.log"))}'
        elif end.get('ev') == 'error':
            error = f'the worker: {json.dumps(end)[:600]}'
    statuses = [e['stats'] for e in events if e.get('ev') == 'status' and isinstance(e.get('stats'), dict)]
    est = (end or {}).get('stats') if isinstance((end or {}).get('stats'), dict) else {}
    rate = steady_rate(statuses, float(plan.get('warmup_s', 30)), est)
    nb = est.get('infer_batches') or 0
    rec.update(
        finished=now(), seconds=round(t_stop - t_start, 1), leaves_per_s=rate['leaves_per_s'], rate=rate,
        warmup_s=float(plan.get('warmup_s', 30)),
        leaves=est.get('leaves'), games=est.get('games'), rows=est.get('rows'),
        leaves_per_s_job=est.get('leaves_per_s'), infer_batches=nb,
        batch_rows_mean=round(est['infer_rows'] / nb, 1) if nb and est.get('infer_rows') is not None else None,
        infer_ms_mean=round(est['infer_ms'] / nb, 2) if nb and est.get('infer_ms') is not None else None,
        infer_ms_p50=est.get('infer_ms_p50'), infer_ms_p95=est.get('infer_ms_p95'), search_ms=est.get('search_ms'),
        wait_ms=est.get('wait_ms'), batch_rows_hist=est.get('batch_rows_hist'), threads=est.get('threads'),
        concurrency=est.get('concurrency'), provider=est.get('provider'), peak_rss_mb=est.get('peak_rss_mb'),
        status=[{k: s.get(k) for k in ('wall_s', 'leaves', 'games', 'infer_batches', 'infer_rows', 'rss_mb')} for s in statuses],
        load=load_summary(samples), samples=samples)
    if discard:
        rec.update(discarded=True, why=discard)
    elif error:
        rec['error'] = error
    elif not rate['leaves_per_s'] or rate['leaves_per_s'] <= 0:
        rec['error'] = 'no leaves in the window'
    else:
        rec['ok'] = True
    return rec


def _tail(path, n=400):
    try:
        return open(path, encoding='utf-8', errors='replace').read()[-n:]
    except OSError:
        return ''


def host_info():
    """The machine's hardware (for the hints; recorded in the release directory, never committed)."""
    import platform
    h = {'platform': sys.platform, 'machine': platform.machine(), 'os': platform.platform(), 'logical': os.cpu_count()}
    try:
        import psutil
        h['physical'] = psutil.cpu_count(logical=False)
        h['ram_gb'] = round(psutil.virtual_memory().total / 2 ** 30)
    except Exception:                              # noqa: BLE001
        pass
    try:
        if os.name == 'nt':
            import winreg
            with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, r'HARDWARE\DESCRIPTION\System\CentralProcessor\0') as k:
                h['cpu'] = winreg.QueryValueEx(k, 'ProcessorNameString')[0].strip()
        elif sys.platform == 'darwin':
            q = lambda key: subprocess.run(['sysctl', '-n', key], capture_output=True, text=True, timeout=10).stdout.strip()  # noqa: E731
            h['cpu'] = q('machdep.cpu.brand_string')
            h['perf_cores'], h['efficiency_cores'] = q('hw.perflevel0.physicalcpu'), q('hw.perflevel1.physicalcpu')
        elif os.path.exists('/proc/cpuinfo'):
            m = re.search(r'model name\s*:\s*(.+)', open('/proc/cpuinfo').read())
            h['cpu'] = m.group(1).strip() if m else None
    except Exception:                              # noqa: BLE001
        pass
    return h


def run_windows(plan, out, order, first=0, log=say):
    """The runner: `order`'s windows (each after an idle wait; a discarded one again, at most plan idle.redo times)
    -> their records (discarded ones too); also written to `<out>/windows-<first>.json` with the host's hardware."""
    os.makedirs(out, exist_ok=True)
    idle_cfg = plan.get('idle') or {}
    infos, windows = {}, []
    for name in sorted(set(order)):
        infos[name] = worker_info(plan['sides'][name])
    host = host_info()

    def save():
        with open(os.path.join(out, f'windows-{first}.json'), 'w', encoding='utf-8', newline='\n') as f:
            json.dump({'schema': SCHEMA, 'machine': plan.get('machine'), 'host': host, 'infos': infos, 'windows': windows}, f, indent=1)
    bad = {n: i for n, i in infos.items() if 'error' in i or (plan['sides'][n].get('build_id')
                                                              and i.get('build_id') != plan['sides'][n]['build_id'])}
    if bad:
        for n, i in bad.items():
            windows.append({'index': first + len(windows), 'side': n, 'ok': False, 'build_id': plan['sides'][n].get('build_id'),
                            'error': f'info: {i.get("error") or "the worker says build " + str(i.get("build_id"))} '
                                     f'(want {plan["sides"][n].get("build_id")})'})
        save()
        return windows
    redo = int(idle_cfg.get('redo', 3))
    qcfg = plan.get('quiet')
    quiet = Quiet(qcfg, len(order) * (float(plan['minutes']) * 60 + 600) + 1800, log) if qcfg else None
    bench = None
    if plan.get('bench_lock'):
        sides = plan['sides']
        purpose = f'farm-speed-gate:{plan.get("machine")}:{str(sides.get("new", {}).get("build_id"))[:8]}-vs-' \
                  f'{str(sides.get("old", {}).get("build_id"))[:8]}'
        bench = BenchLock(plan['bench_lock'], purpose, log)
    try:
        if bench:
            b = bench.acquire(float(idle_cfg.get('wait_max_s', 4 * 3600)))
            if not b['ok']:
                windows.append({'index': first, 'side': order[0], 'ok': False, 'error': f'machine busy: {b["why"]}', 'bench_lock': b})
                save()
                return windows
        _windows(plan, out, order, first, log, idle_cfg, redo, quiet, windows, save)
        return windows
    finally:
        if quiet:
            quiet.release()
        if bench:
            bench.release()
            for w in windows:
                w['bench_lock'] = bench.record
            save()


def _windows(plan, out, order, first, log, idle_cfg, redo, quiet, windows, save):
    k = first
    avoid = (plan.get('quiet') or {}).get('avoid')
    for name in order:
        while True:
            before = None
            if idle_cfg.get('check', True):
                before = wait_idle(idle_cfg, log, avoid=avoid, on_wait=quiet.release if quiet else None)
            if before is not None and not before['ok']:
                windows.append({'index': k, 'side': name, 'ok': False, 'build_id': plan['sides'][name].get('build_id'),
                                'error': before['why'], 'in_use': bool(before.get('in_use')), 'idle': {'before': before}})
                save()
                return windows
            q = None
            if quiet:
                if not quiet.held:
                    q = quiet.acquire(float(idle_cfg.get('wait_max_s', 4 * 3600)))
                    if not q['ok']:
                        windows.append({'index': k, 'side': name, 'ok': False, 'build_id': plan['sides'][name].get('build_id'),
                                        'error': f'machine busy: {q["why"]}', 'quiet': q})
                        save()
                        return windows
                    s, ac = idle_state()             # (input or an evaluation during the settle: wait again)
                    if idle_cfg.get('check', True) and (s is None or s < float(idle_cfg.get('min_s', 180)) or ac is False
                                                        or held_avoid(avoid)):
                        quiet.records[-1]['after_settle'] = {'idle_s': s, 'ac': ac, 'busy': held_avoid(avoid)}
                        log(f'not idle after the settle (idle {s} s, AC {ac}): waiting again')
                        redo -= 1
                        if redo < 0:
                            windows.append({'index': k, 'side': name, 'ok': False, 'in_use': True,
                                            'build_id': plan['sides'][name].get('build_id'),
                                            'error': 'machine in use: not idle after taking the quiet lock, too often'})
                            save()
                            return windows
                        quiet.release()
                        continue
                q = dict(quiet.records[-1])
            log(f'window {k} ({name}, {plan["sides"][name].get("build_id")}): {plan["minutes"]} min')
            w = run_window(plan, name, k, out, before, log, q)
            windows.append(w)
            k += 1
            save()
            if w.get('discarded'):
                log(f'window {w["index"]} discarded: {w["why"]}')
                redo -= 1
                if redo < 0:
                    user = 'input' in w['why'] or 'battery' in w['why']
                    windows.append({'index': k, 'side': name, 'ok': False, 'in_use': user, 'build_id': w.get('build_id'),
                                    'error': f'machine {"in use" if user else "busy"}: windows discarded '
                                             f'{int(idle_cfg.get("redo", 3)) + 1} times (the last: {w["why"]})'})
                    save()
                    return windows
                continue
            log(f'window {w["index"]} ({name}): ' + (f'{w["leaves_per_s"]:.0f} leaves/s' if w['ok'] else f'ERROR {w.get("error")}'))
            if not w['ok']:
                return windows
            break
    return windows


# ================================================================================================ the judge

def interval_rates(status, warmup_s):
    """[(from_s, to_s, leaves/s)] between neighbouring status lines after the warmup."""
    pts = [(float(s['wall_s']), int(s['leaves'])) for s in status or [] if s.get('wall_s') is not None and s.get('leaves') is not None]
    after = [p for p in pts if p[0] >= warmup_s]
    return [(a[0], b[0], (b[1] - a[1]) / (b[0] - a[0])) for a, b in zip(after, after[1:]) if b[0] > a[0]]


def others_in(samples, a, b):
    """The largest 'others' share (the machine's CPU % less the worker's) sampled in (a, b + 2] s of a window."""
    v = [s['others_pct'] for s in samples or [] if isinstance(s.get('others_pct'), (int, float)) and a < s.get('t', -1) <= b + 2]
    return max(v) if v else None


def clean_rate(w, baseline, margin):
    """A window's rate without the intervals in which other load rose more than `margin` points above the machine's
    `baseline` -> (leaves/s or None when fewer than half - or than 3 - of its intervals are clean, contended intervals,
    intervals). No status series, samples or margin: the window's own rate."""
    iv = interval_rates(w.get('status'), float(w.get('warmup_s', 30)))
    if margin is None or baseline is None or len(iv) < 3 or not w.get('samples'):
        return float(w['leaves_per_s']), 0, len(iv)
    clean = []
    for a, b, r in iv:
        o = others_in(w['samples'], a, b)
        if o is None or o <= baseline + margin:
            clean.append(r)
    if len(clean) >= max(3, (len(iv) + 1) // 2):
        return round(quantile(clean, 0.75), 1), len(iv) - len(clean), len(iv)
    return None, len(iv) - len(clean), len(iv)


def judge(windows, tolerance, busy_margin=None):
    """A machine's windows -> its verdict (module docs): passed, ratio, the sides' mean leaves/s, their spreads, noisy,
    the neighbouring pairs' ratios; an erroring window or a machine in use fails it. `busy_margin` (points of CPU %):
    each window's rate leaves out the 10-second intervals in which the others' load rose that far above the machine's
    baseline (the median of every sample of these windows: a constant background is the baseline, a burst is not); a
    window with fewer than half its intervals clean is contended, left out, and makes the machine noisy."""
    used = [w for w in windows if not w.get('discarded')]
    out = {'tolerance': tolerance, 'windows': len(used), 'discarded': len(windows) - len(used)}
    errs = [w for w in used if not w.get('ok')]
    if errs:
        out.update(passed=False, error=True, in_use=any(w.get('in_use') for w in errs),
                   why='; '.join(f'window {w.get("index")} ({w.get("side")}): {w.get("error")}' for w in errs)[:1500])
        return out
    others = [s['others_pct'] for w in used for s in w.get('samples') or [] if isinstance(s.get('others_pct'), (int, float))]
    baseline = quantile(others, 0.5) if others and busy_margin is not None else None
    eff, contended, cut = {}, [], 0
    for w in used:
        r, k, n = clean_rate(w, baseline, busy_margin)
        cut += k
        if r is None:
            contended.append(w.get('index'))
        else:
            eff[w.get('index')] = r
    out.update(others_baseline=None if baseline is None else round(baseline, 1), busy_margin=busy_margin,
               contended_windows=contended, contended_intervals=cut)
    rates = {s: [eff[w.get('index')] for w in used if w['side'] == s and w.get('index') in eff] for s in ('new', 'old')}
    if not rates['new'] or not rates['old']:
        if contended:
            out.update(passed=False, error=False, noisy=True, busy=True,
                       why=f'machine busy: other load during windows {contended} (above {baseline:.0f} + {busy_margin} points)')
        else:
            out.update(passed=False, error=True, why='no windows of ' + ' and '.join(s for s in rates if not rates[s]))
        return out
    mean = {s: sum(v) / len(v) for s, v in rates.items()}
    spread = {s: round((max(v) - min(v)) / mean[s], 4) if mean[s] else 0.0 for s, v in rates.items()}
    ratio = mean['new'] / mean['old'] if mean['old'] else 0.0
    pairs = []
    for a, b in zip(used[0::2], used[1::2]):
        if {a['side'], b['side']} == {'new', 'old'} and a.get('index') in eff and b.get('index') in eff:
            n, o = (a, b) if a['side'] == 'new' else (b, a)
            pairs.append(round(eff[n.get('index')] / eff[o.get('index')], 4) if eff[o.get('index')] else None)
    ok_pairs = [p for p in pairs if p is not None]
    pair_spread = round(max(ok_pairs) - min(ok_pairs), 4) if len(ok_pairs) >= 2 else 0.0
    out.update(passed=ratio >= 1 - tolerance, error=False, ratio=round(ratio, 4), new_leaves_per_s=round(mean['new'], 1),
               old_leaves_per_s=round(mean['old'], 1), spread=spread, pairs=pairs, pair_spread=pair_spread,
               noisy=max(spread.values()) > tolerance or pair_spread > tolerance or bool(contended), rates=rates)
    return out


def run_blocks(runner, plan, tolerance, log=say):
    """`plan['blocks']` blocks of plan['order'] through `runner(order, first)`, then up to `plan['retry_blocks']` more
    while the machine fails, noisily and without errors -> (windows, judge, blocks run)."""
    windows, k = [], 0
    blocks, retry = int(plan.get('blocks', 1)), int(plan.get('retry_blocks', 0))
    while True:
        order = list(plan.get('order') or ORDER)
        if k % 2:                                  # (every other block mirrored - old, new, new, old -: each side takes each
            order = [{'new': 'old', 'old': 'new'}.get(s, s) for s in order]     # position as often, over two blocks)
        windows += runner(order, len(windows))
        k += 1
        j = judge(windows, tolerance, plan.get('busy_margin_pct'))
        if j.get('error'):
            break
        if k < blocks:
            continue
        if j['passed'] or not j.get('noisy') or k >= blocks + retry:
            break
        log(f'{plan.get("machine")}: failing and noisy ({j.get("ratio")}x, spreads {j.get("spread")}, pairs {j.get("pairs")}, '
            f'contended windows {j.get("contended_windows")}): one more block')
    return windows, j, k


# ================================================================================================ ONNX facts (no onnx needed)

def _varint(b, i):
    x = s = 0
    while True:
        c = b[i]
        i += 1
        x |= (c & 0x7F) << s
        if c < 0x80:
            return x, i
        s += 7


def onnx_info(path):
    """A zero-onnx export -> {sha256, graph_sha256 (the model without its metadata, producer and doc strings), meta:
    {checkpoint, checkpoint_iter, checkpoint_sha256, zero_onnx, protocol}} - a protobuf walk of ModelProto's top level."""
    data = open(path, 'rb').read()
    meta, g = {}, hashlib.sha256()
    i = 0
    while i < len(data):
        key, i = _varint(data, i)
        field, wt = key >> 3, key & 7
        start = i
        if wt == 0:
            _, i = _varint(data, i)
        elif wt == 2:
            n, i = _varint(data, i)
            i += n
        elif wt == 1:
            i += 8
        elif wt == 5:
            i += 4
        else:
            raise ValueError(f'{path}: not an ONNX model (wire type {wt})')
        body = data[start:i]
        if field == 14 and wt == 2:              # metadata_props: StringStringEntryProto {key = 1, value = 2}
            n, j = _varint(data, start)
            e, kv, p = data[j:j + n], {}, 0
            while p < len(e):
                k2, p = _varint(e, p)
                ln, p = _varint(e, p)
                kv[k2 >> 3] = e[p:p + ln].decode('utf-8', 'replace')
                p += ln
            meta[kv.get(1, '')] = kv.get(2, '')
        elif field in (1, 7, 8, 20, 25):         # ir_version, graph, opset_import, training_info, functions
            g.update(bytes([field]) + body)
    keep = ('checkpoint', 'checkpoint_iter', 'checkpoint_sha256', 'zero_onnx', 'protocol', 'game')
    return {'sha256': hashlib.sha256(data).hexdigest(), 'graph_sha256': g.hexdigest(), 'bytes': len(data),
            'meta': {k: meta.get(k) for k in keep}}


# ================================================================================================ the gate (the desktop)

def _fb():
    sys.path.insert(0, ROOT)
    sys.path.insert(0, os.path.join(ROOT, 'third_party', 'zero-core', 'python'))
    from mtgz.az import farm_builds
    return farm_builds


def portable(p):
    try:
        sys.path.insert(0, TOOLS)
        import local_config
        return local_config.portable_path(p)
    except Exception:                              # noqa: BLE001
        return p


def deep_merge(a, b):
    out = dict(a)
    for k, v in (b or {}).items():
        out[k] = deep_merge(out[k], v) if isinstance(v, dict) and isinstance(out.get(k), dict) else v
    return out


def release_settings(builds):
    """release.json's `reference` and `speed` (merged over SPEED_DEFAULTS; its `mac_runtime` too), the builds
    directory's."""
    p = os.path.join(builds, 'release.json')
    rj = {}
    if os.path.exists(p):
        with open(p, encoding='utf-8') as f:
            rj = json.load(f)
    ref = rj.get('reference') or {}
    rd = os.path.join(builds, 'reference')
    reference = {'config': ref.get('config') or os.path.join(rd, 'config.json'),
                 'export': ref.get('export') or os.path.join(rd, 'export')}
    sp = deep_merge(SPEED_DEFAULTS, rj.get('speed') or {})
    sp.setdefault('mac_runtime', rj.get('mac_runtime'))
    return reference, sp


def build_id_of(d):
    with open(os.path.join(d, 'manifest.json'), 'rb') as f:
        return hashlib.sha256(f.read()).hexdigest()[:24]


def resolve_build(x, builds):
    """A build directory (with its manifest) or a build id (`<builds>/<id>/`) -> {build_id, dir, manifest, worker}; an
    id without a local copy -> dir None (a Mac build that lives on the Mac only)."""
    if x and os.path.isdir(x) and os.path.exists(os.path.join(x, 'manifest.json')):
        d = os.path.abspath(x)
    elif x and re.fullmatch(r'[0-9a-f]{24}', x):
        d = os.path.join(builds, x)
        if not os.path.exists(os.path.join(d, 'manifest.json')):
            return {'build_id': x, 'dir': None, 'manifest': None, 'worker': None}
    else:
        raise SystemExit(f'farm_speed: {x!r} is neither a build directory (manifest.json) nor a build id')
    with open(os.path.join(d, 'manifest.json'), encoding='utf-8') as f:
        m = json.load(f)
    return {'build_id': build_id_of(d), 'dir': d, 'manifest': m, 'worker': os.path.join(d, m.get('entry') or 'mtgz-worker.exe')}


def machine_build(e, machine):
    """An index entry's build for `machine`: cpu the release build, cuda its CUDA alternate, mac its macOS release
    build."""
    if machine == 'cuda':
        return (e.get('cuda') or {}).get('build_id')
    if machine == 'mac':
        return ((e.get('mac') or {}).get('release') or {}).get('build_id')
    return e.get('build_id')


def find_baseline(ix, machine, new_id):
    """The previous published release build for `machine` (module docs) -> (build id, its entry) or (None, None): the
    newest published entry before the new build's first entry (all of them when it has none) whose build differs."""
    es = [e for e in ix.get('builds') or [] if e.get('published')]
    ids = [machine_build(e, machine) for e in es]
    cands = es[:ids.index(new_id)] if new_id in ids else es
    for e in reversed(cands):
        b = machine_build(e, machine)
        if b and b != new_id:
            return b, e
    return None, None


def index_entry_of(ix, bid, machine='cpu'):
    hit = [e for e in ix.get('builds') or [] if machine_build(e, machine) == bid]
    return hit[0] if hit else None


def resolve_model(spec, reference):
    """`reference` | `run:<run dir>` | a path -> (model path, default config path, a description)."""
    if spec in (None, '', 'reference'):
        return os.path.join(reference['export'], 'model.onnx'), reference['config'], 'the release reference export'
    if spec.startswith('run:'):
        run = spec[4:]
        pool = os.path.join(run, 'pool')
        best = None
        for rid in sorted(os.listdir(pool)) if os.path.isdir(pool) else []:
            ck = os.path.join(pool, rid, 'ckpt')
            for g in os.listdir(ck) if os.path.isdir(ck) else []:
                if g.isdigit() and all(os.path.exists(os.path.join(ck, g, f)) for f in ('model.onnx', 'reference.npz')):
                    if best is None or int(g) > best[0]:
                        best = (int(g), os.path.join(ck, g, 'model.onnx'))
        if best is None:
            raise SystemExit(f'farm_speed: --model {spec}: no complete pool export under {pool}')
        return best[1], os.path.join(run, 'config.json'), f'the run\'s newest pool export (gen {best[0]})'
    return spec, reference['config'], 'given'


def machine_settings(sp, machine, a):
    """The machine's settings: SPEED_DEFAULTS < release.json speed < its machines entry < the command line."""
    m = dict(sp.get('machines', {}).get(machine) or {})
    s = {k: sp[k] for k in ('tolerance', 'minutes', 'warmup_s', 'blocks', 'retry_blocks', 'idle_min_s', 'idle_wait_max_s', 'idle_redo',
                            'busy_margin_pct')}
    s.update({k: v for k, v in m.items() if k in s})
    for k, flag in (('tolerance', 'tolerance'), ('minutes', 'minutes'), ('warmup_s', 'warmup'), ('blocks', 'blocks'),
                    ('retry_blocks', 'retry_blocks'), ('idle_min_s', 'idle_min'), ('idle_wait_max_s', 'idle_wait_max'),
                    ('idle_redo', 'idle_redo'), ('busy_margin_pct', 'busy_margin')):
        v = getattr(a, flag, None)
        if v is not None:
            s[k] = v
    job = dict(m.get('job') or {})
    for kv in getattr(a, 'job', None) or []:
        mach, _, rest = kv.partition(':')
        if mach == machine:
            k, _, v = rest.partition('=')
            job[k] = json.loads(v)
    s['job'] = job
    s['priority'], s['nice'], s['quiet'] = m.get('priority'), m.get('nice'), m.get('quiet')
    s['bench_lock'] = m.get('bench_lock')
    return s


def layout_of(job):
    """The hint's layout: threads, infer_workers x infer_threads, groups, games in flight."""
    return {'threads': job.get('threads'), 'infer_workers': job.get('infer_workers', 'default'),
            'infer_threads': job.get('infer_threads', job.get('threads')), 'groups': job.get('groups'),
            'inflight': job.get('conc'), 'rolling': job.get('rolling', True), 'output': job.get('output', 'records'),
            'providers': job.get('providers') or ['cpu']}


def bench_settings(config):
    """The run's settings as a speed job plays them (tools/worker_bench.py's: the remote and resolver keys off)."""
    with open(config, encoding='utf-8') as f:
        cfg = json.load(f)
    cfg.update(eval_remote='', selfplay_remote='', resolver='', resolver_cfg={}, resolver_init='', infer_device='cpu')
    if 'shard_rows' in cfg:
        cfg['shard_rows'] = min(int(cfg['shard_rows']), 8192)
    return cfg


class Gate:
    def __init__(self, a):
        self.a = a
        FB = _fb()
        self.FB = FB
        self.builds = FB.builds_dir()
        if not self.builds:
            raise SystemExit('farm_speed: no builds directory ($MTGZ_FARM_BUILDS, or ZERO_BIG_DISK in ~/.config/zero/local.env)')
        self.reference, self.sp = release_settings(self.builds)
        self.ix = FB.load()
        self.out = os.path.abspath(a.out or os.path.join(os.getcwd(), f'farm-speed-{int(time.time())}'))
        os.makedirs(self.out, exist_ok=True)

    # ------------------------------------------------------------------ sides
    def sides_for(self, machine):
        """(new, old) for `machine`: {build_id, dir, worker, runtime, manifest} each, plus how the baseline was found."""
        a = self.a
        if machine == 'cpu':
            if not a.new:
                raise SystemExit('farm_speed: the cpu machine needs --new (the new build)')
            n, base_arg = resolve_build(a.new, self.builds), a.baseline
        else:
            given = getattr(a, f'{machine}_new')
            if given:
                n = resolve_build(given, self.builds)
            elif not a.new:
                raise SystemExit(f'farm_speed: give --{machine}-new (or --new, whose index entry names its {machine} build)')
            else:
                new = resolve_build(a.new, self.builds)
                e = index_entry_of(self.ix, new['build_id'])
                bid = machine_build(e or {}, machine)
                if not bid:
                    raise SystemExit(f'farm_speed: no {machine} build for {new["build_id"]}: give --{machine}-new')
                n = resolve_build(bid, self.builds)
            base_arg = getattr(a, f'{machine}_baseline')
            if not base_arg and a.baseline:
                e = index_entry_of(self.ix, resolve_build(a.baseline, self.builds)['build_id'])
                base_arg = machine_build(e or {}, machine)
        if base_arg:
            o, how = resolve_build(base_arg, self.builds), 'given'
        else:
            bid, e = find_baseline(self.ix, machine, n['build_id'])
            if not bid:
                raise SystemExit(f'farm_speed: no previous published {machine} build before {n["build_id"]} in {self.FB.index_path()}: '
                                 f'give --{"" if machine == "cpu" else machine + "-"}baseline')
            o = resolve_build(bid, self.builds)
            how = f'the previous published release (commit {e["commit"][:12]})'
        for s in (n, o):
            e = index_entry_of(self.ix, s['build_id'], machine)
            s['commit'] = (s.get('manifest') or {}).get('engine_commit') or (e or {}).get('commit')
            if s['dir'] is None and machine != 'mac':
                raise SystemExit(f'farm_speed: build {s["build_id"]} has no copy in {self.builds}')
        for k in ('profile', 'platform', 'kind'):
            if n.get('manifest') and o.get('manifest') and n['manifest'].get(k) != o['manifest'].get(k):
                raise SystemExit(f'farm_speed: the baseline {o["build_id"]} is a {k} {o["manifest"].get(k)!r} build, the new one '
                                 f'{n["manifest"].get(k)!r}')
        o['how'] = how
        return n, o

    def runtime_of(self, b, machine):
        rt = (b.get('manifest') or {}).get('requires_runtime')
        if not rt:
            raise SystemExit(f'farm_speed: build {b["build_id"]} names no runtime pack (requires_runtime)')
        d = os.path.join(self.builds, rt)
        if not os.path.exists(os.path.join(d, 'manifest.json')):
            raise SystemExit(f'farm_speed: runtime pack {rt} (of build {b["build_id"]}) is not in {self.builds}')
        return rt, d

    # ------------------------------------------------------------------ the gate
    def network(self):
        a = self.a
        model, config, how = resolve_model(a.model or self.sp.get('model'), self.reference)
        config = a.config or config
        old = onnx_info(model)
        new_model = a.model_new or model
        new = old if new_model == model else onnx_info(new_model)
        for k in ('checkpoint_iter', 'checkpoint_sha256'):
            if old['meta'].get(k) != new['meta'].get(k):
                raise SystemExit(f'farm_speed: --model-new is not the same network ({k} {new["meta"].get(k)} vs {old["meta"].get(k)})')
        return {'old': dict(old, path=portable(model), how=how), 'new': dict(new, path=portable(new_model)),
                'same_file': new['sha256'] == old['sha256'], 'same_graph': new['graph_sha256'] == old['graph_sha256'],
                'config': portable(config), 'checkpoint': old['meta'].get('checkpoint'),
                'checkpoint_iter': old['meta'].get('checkpoint_iter')}, model, new_model, config

    def run(self):
        a = self.a
        machines = [m for m in a.machines.split(',') if m]
        unknown = [m for m in machines if m not in SPEED_DEFAULTS['machines']]
        if unknown:
            raise SystemExit(f'farm_speed: unknown machines {unknown} (cpu, mac, cuda)')
        net, model, new_model, config = self.network()
        settings = bench_settings(config)
        gen = int(net['checkpoint_iter']) if str(net.get('checkpoint_iter') or '').isdigit() else None
        seed = int(settings.get('seed', 1)) * 7919 + (gen or 0) + 7_777_777
        for m in machines:
            try:
                self.machine(m, net, model, new_model, settings, gen, seed)
            except SystemExit as e:              # (a machine that cannot even start fails, recorded: never a silent pass)
                self.failed_machine(m, net, str(e))
        res = combine(self.out)
        print(res['line'], flush=True)
        return 0 if res['passed'] else 1

    def machine(self, m, net, model, new_model, settings, gen, seed):
        a = self.a
        ms = machine_settings(self.sp, m, a)
        new, old = self.sides_for(m)
        out_m = os.path.join(self.out, m)
        shutil.rmtree(out_m, ignore_errors=True)
        os.makedirs(out_m)
        plan = {'schema': SCHEMA, 'machine': m, 'minutes': ms['minutes'], 'warmup_s': ms['warmup_s'], 'blocks': ms['blocks'],
                'retry_blocks': ms['retry_blocks'], 'order': ORDER, 'priority': ms['priority'], 'nice': ms['nice'], 'quiet': ms['quiet'],
                'bench_lock': ms['bench_lock'], 'busy_margin_pct': ms['busy_margin_pct'],
                'seed': seed, 'gen': gen, 'settings': settings, 'job': dict(JOB_BASE, **ms['job']), 'sample_s': 10,
                'idle': {'check': True, 'min_s': ms['idle_min_s'], 'wait_max_s': ms['idle_wait_max_s'], 'redo': ms['idle_redo']},
                'sides': {}}
        record = {'schema': SCHEMA, 'machine': m, 'label': LABELS[m], 'started': now(), 'tolerance': ms['tolerance'],
                  'network': net, 'layout': layout_of(plan['job']), 'sides': {}}
        for name, b, mod in (('new', new, new_model), ('old', old, model)):
            side = {'build_id': b['build_id'], 'model': mod}
            if m == 'mac':
                rt = (b.get('manifest') or {}).get('requires_runtime') or self.sp.get('mac_runtime')
                side['runtime'] = rt
            else:
                rt, rd = self.runtime_of(b, m)
                side.update(worker=b['worker'], runtime_dir=rd, runtime=rt)
                if m == 'cuda':
                    side.update(path_prepend=[rd], unset_env=['CUDA_VISIBLE_DEVICES'])
            plan['sides'][name] = side
            record['sides'][name] = {'build_id': b['build_id'], 'commit': b.get('commit'), 'runtime': side.get('runtime'),
                                     'worker': portable(b.get('worker') or ''), 'how': b.get('how'),
                                     'model': portable(mod)}
        say(f'{m}: new {new["build_id"]} vs old {old["build_id"]} ({old.get("how")}), {ms["minutes"]} min windows '
            f'{"/".join(ORDER)} x {ms["blocks"]} (+{ms["retry_blocks"]} if noisy), layout {plan["job"]}')
        host = None
        try:
            runner, host = self.mac_runner(plan, new, old, out_m) if m == 'mac' else (local_runner(plan, out_m), None)
            windows, j, k = run_blocks(runner, plan, ms['tolerance'])
        except Exception as e:                     # noqa: BLE001 - a machine that cannot run fails, loudly
            windows, j, k = [], {'passed': False, 'error': True, 'why': f'{type(e).__name__}: {e}'}, 0
        host = host or _host_of(out_m)
        record.update(finished=now(), blocks_run=k, judge=j, passed=bool(j.get('passed')), windows=windows, host=host,
                      plan={k2: v for k2, v in plan.items() if k2 != 'settings'})
        record['hint'] = hint_of(record)
        with open(os.path.join(out_m, 'machine.json'), 'w', encoding='utf-8', newline='\n') as f:
            json.dump(record, f, indent=1)
        say(f'{m}: ' + machine_line(m, record))
        return record

    def failed_machine(self, m, net, why):
        say(f'{m}: FAILED before its windows: {why}')
        out_m = os.path.join(self.out, m)
        os.makedirs(out_m, exist_ok=True)
        rec = {'schema': SCHEMA, 'machine': m, 'label': LABELS.get(m, m), 'started': now(), 'finished': now(), 'passed': False,
               'network': net, 'windows': [], 'sides': {}, 'judge': {'passed': False, 'error': True, 'why': why}}
        rec['hint'] = hint_of(rec)
        with open(os.path.join(out_m, 'machine.json'), 'w', encoding='utf-8', newline='\n') as f:
            json.dump(rec, f, indent=1)

    # ------------------------------------------------------------------ the Mac
    def mac_runner(self, plan, new, old, out_m):
        sys.path.insert(0, TOOLS)
        import farm_gates as FG
        stamp = time.strftime('%Y%m%d-%H%M%S')
        remote = f'{MAC_HOME_DIR}/speed/{stamp}'
        rc, txt = FG.mac_shell(f'mkdir -p "$HOME/{remote}/out"\n')
        if rc:
            raise SystemExit(f'the Mac: {txt[-500:]}')
        for b in (new, old):                       # the builds on the Mac (from the builds directory when missing)
            rel = f'{MAC_HOME_DIR}/builds/{b["build_id"]}'
            rc, txt = FG.mac_shell(f'test -x "$HOME/{rel}/mtgz-worker" && echo present || echo missing\n')
            if 'present' not in txt:
                if not b.get('dir'):
                    raise SystemExit(f'the Mac has no {rel} and the builds directory no copy of {b["build_id"]}')
                FG.mac_shell(f'rm -rf "$HOME/{rel}" && mkdir -p "$HOME/{rel}"\n')
                for n in os.listdir(b['dir']):
                    FG.mac_scp(os.path.join(b['dir'], n), f'{rel}/{n}')
                FG.mac_shell(f'chmod 755 "$HOME/{rel}/mtgz-worker"\n')
        packs = {}
        for name in ('new', 'old'):
            rt = plan['sides'][name].pop('runtime', None)
            if not rt:
                raise SystemExit(f'the Mac: build {plan["sides"][name]["build_id"]} names no runtime pack (mac_runtime in release.json)')
            d = os.path.join(self.builds, rt)
            with open(os.path.join(d, 'manifest.json'), encoding='utf-8') as f:
                version = json.load(f)['version']
            rel = f'{MAC_HOME_DIR}/packs/{version}'
            if rel not in packs:
                rc, txt = FG.mac_shell(f'test -f "$HOME/{rel}/libonnxruntime.dylib" && echo present || echo missing\n')
                if 'present' not in txt:
                    FG.mac_shell(f'mkdir -p "$HOME/{rel}"\n')
                    for n in os.listdir(d):
                        FG.mac_scp(os.path.join(d, n), f'{rel}/{n}')
                packs[rel] = rt
            s = plan['sides'][name]
            s.update(worker=f'~/{MAC_HOME_DIR}/builds/{s["build_id"]}/mtgz-worker', runtime_dir=f'~/{rel}')
        models = {}
        for name in ('new', 'old'):
            src = plan['sides'][name]['model']
            if src not in models:
                models[src] = f'model-{len(models)}.onnx'
                FG.mac_scp(src, f'{remote}/{models[src]}')
            plan['sides'][name]['model'] = f'~/{remote}/{models[src]}'
        local_plan = os.path.join(out_m, 'plan.json')
        with open(local_plan, 'w', encoding='utf-8', newline='\n') as f:
            json.dump(plan, f, indent=1)
        FG.mac_scp(local_plan, f'{remote}/plan.json')
        FG.mac_scp(os.path.abspath(__file__), f'{remote}/farm_speed.py')
        nice = int(plan.get('nice') or 15)
        per_window = float(plan['minutes']) * 60 + 400 + float(plan['idle']['wait_max_s'])

        def run(order, first):
            say(f'mac: windows {first}..{first + len(order) - 1} on the Mac (~/{remote})')
            rc, txt = FG.mac_shell(f'cd "$HOME/{remote}"\nexport PATH="$HOME/.venvs/dominion-zero/bin:/opt/homebrew/bin:$PATH"\n'
                                   f'PY="$(command -v python || command -v python3)"\n'
                                   f'nice -n {nice} "$PY" farm_speed.py windows --plan plan.json --out out --order {",".join(order)} '
                                   f'--first {first} > windows-{first}.log 2>&1 || echo "runner exit $?"\n',
                                   timeout=per_window * (len(order) * (1 + int(plan['idle']['redo']))) + 600)
            local = os.path.join(out_m, 'remote')
            shutil.rmtree(local, ignore_errors=True)
            FG.mac_scp(f'{remote}/out', local, back=True)
            try:
                FG.mac_scp(f'{remote}/windows-{first}.log', os.path.join(out_m, f'windows-{first}.log'), back=True)
            except SystemExit:
                pass
            p = os.path.join(local, f'windows-{first}.json')
            if not os.path.exists(p):
                raise RuntimeError(f'the Mac\'s runner wrote no windows-{first}.json: {txt[-800:]}')
            with open(p, encoding='utf-8') as f:
                w = json.load(f)
            shutil.copyfile(p, os.path.join(out_m, f'windows-{first}.json'))
            return w['windows']
        return run, None


def local_runner(plan, out_m):
    def run(order, first):
        return run_windows(plan, out_m, order, first)
    return run


def _host_of(out_m):
    for root, _, files in os.walk(out_m):
        for n in sorted(files):
            if n.startswith('windows-') and n.endswith('.json'):
                try:
                    with open(os.path.join(root, n), encoding='utf-8') as f:
                        return json.load(f).get('host')
                except (OSError, ValueError):
                    pass
    return None


def hint_of(rec):
    """A machine's hardware-class hint (module docs): what the farm agent's autotuner can be seeded with."""
    j = rec.get('judge') or {}
    ok = [w for w in rec.get('windows') or [] if w.get('ok') and not w.get('discarded')]

    def mean(k, side=None):
        v = [w[k] for w in ok if isinstance(w.get(k), (int, float)) and (side is None or w['side'] == side)]
        return round(sum(v) / len(v), 2) if v else None
    return {'machine': rec['machine'], 'label': rec.get('label'), 'hardware': rec.get('host'), 'layout': rec.get('layout'),
            'leaves_per_s': {'new': j.get('new_leaves_per_s'), 'old': j.get('old_leaves_per_s')}, 'ratio': j.get('ratio'),
            'batch_rows_mean': mean('batch_rows_mean', 'new'), 'infer_ms_p50': mean('infer_ms_p50', 'new'),
            'peak_rss_mb': max((w.get('peak_rss_mb') or 0 for w in ok), default=None) or None,
            'others_pct_mean': (lambda v: round(sum(v) / len(v), 1) if v else None)(
                [w['load']['others_pct'] for w in ok if (w.get('load') or {}).get('others_pct') is not None]),
            'build_id': (rec.get('sides') or {}).get('new', {}).get('build_id'),
            'network': (rec.get('network') or {}).get('checkpoint'), 'measured': rec.get('finished')}


def machine_line(m, rec):
    j = rec.get('judge') or {}
    n = len([w for w in rec.get('windows') or [] if not w.get('discarded')])
    disc = j.get('discarded') or 0
    mins = (rec.get('plan') or {}).get('minutes')
    tail = f', {disc} discarded (input)' if disc else ''
    if j.get('error'):
        return f'{m} {"IN USE" if j.get("in_use") else "ERROR"}: {j.get("why")}'
    if j.get('busy') or 'ratio' not in j:
        return f'{m} BUSY: {j.get("why")}'
    cont = j.get('contended_windows') or []
    cut = f', {j.get("contended_intervals")} contended 10 s intervals left out' if j.get('contended_intervals') else ''
    cw = f', windows {cont} contended' if cont else ''
    return (f'{m} {j["ratio"]:.3f}x {"ok" if j["passed"] else "SLOWER"} (new {j["new_leaves_per_s"]:,.0f} / old '
            f'{j["old_leaves_per_s"]:,.0f} leaves/s, {n} x {mins:g} min{tail}{cut}{cw}{", noisy" if j.get("noisy") else ""})')


def combine(out):
    """Every `<out>/<machine>/machine.json` -> `<out>/result.json` (the verdict and the line) and `<out>/hints.json`."""
    machines = {}
    for name in sorted(os.listdir(out)):
        p = os.path.join(out, name, 'machine.json')
        if os.path.exists(p):
            with open(p, encoding='utf-8') as f:
                machines[name] = json.load(f)
    passed = bool(machines) and all(m.get('passed') for m in machines.values())
    any_m = next(iter(machines.values()), {})
    base = (any_m.get('sides') or {}).get('old', {})
    newb = (any_m.get('sides') or {}).get('new', {})
    net = any_m.get('network') or {}
    tol = any_m.get('tolerance')
    parts = [machine_line(k, v) for k, v in machines.items()]
    exp = '' if net.get('same_graph', True) else ', the new export\'s graph differs'
    line = (f'SPEED GATE {"PASSED" if passed else "FAILED"}: ' + ('; '.join(parts) or 'no machine ran')
            + f' - new {newb.get("build_id", "?")[:8]} vs {base.get("build_id", "?")[:8]}'
            + (f' ({(base.get("commit") or "")[:8]})' if base.get('commit') else '')
            + f', network {net.get("checkpoint")}{exp}, tolerance {tol if tol is None else f"{tol:.0%}"}')
    res = {'schema': SCHEMA, 'passed': passed, 'line': line, 'finished': now(), 'tolerance': tol,
           'new': {k: newb.get(k) for k in ('build_id', 'commit')}, 'baseline': {k: base.get(k) for k in ('build_id', 'commit', 'how')},
           'network': net,
           'machines': {k: {'passed': v.get('passed'), 'line': machine_line(k, v),
                            **{x: (v.get('judge') or {}).get(x) for x in ('ratio', 'new_leaves_per_s', 'old_leaves_per_s', 'spread',
                                                                          'noisy', 'windows', 'discarded', 'in_use', 'why')},
                            'new': (v.get('sides') or {}).get('new', {}).get('build_id'),
                            'old': (v.get('sides') or {}).get('old', {}).get('build_id'), 'layout': v.get('layout'),
                            'blocks_run': v.get('blocks_run')}
                        for k, v in machines.items()}}
    with open(os.path.join(out, 'result.json'), 'w', encoding='utf-8', newline='\n') as f:
        json.dump(res, f, indent=1)
    with open(os.path.join(out, 'hints.json'), 'w', encoding='utf-8', newline='\n') as f:
        json.dump({'schema': SCHEMA, 'hints': [v.get('hint') or hint_of(v) for v in machines.values()]}, f, indent=1)
    return res


def summary(res, path=None):
    """result.json -> the farm builds index's `speed` record."""
    return {'passed': res['passed'], 'line': res['line'], 'baseline': res.get('baseline'),
            'network': {k: (res.get('network') or {}).get(k) for k in ('checkpoint', 'same_graph')},
            'machines': {k: {x: v.get(x) for x in ('passed', 'ratio', 'new_leaves_per_s', 'old_leaves_per_s', 'layout')}
                         for k, v in (res.get('machines') or {}).items()},
            'result': path}


# ================================================================================================ the command line

def gate_args(ap):
    ap.add_argument('--new', help='the new build: its directory (manifest.json) or its id under the builds dir (the cpu machine '
                    'needs it; mac / cuda take --mac-new / --cuda-new, else those of the new build\'s index entry)')
    ap.add_argument('--baseline', help='the old build (default: the previous published release build, module docs)')
    ap.add_argument('--machines', default='cpu', help='cpu, mac, cuda (comma-separated)')
    ap.add_argument('--model', help='reference (default) | run:<run dir> | <model.onnx>: the network both sides play')
    ap.add_argument('--model-new', help='the new side\'s export of the same checkpoint (default: --model)')
    ap.add_argument('--config', help='the run settings (default: the reference\'s, or the run\'s for run:)')
    ap.add_argument('--minutes', type=float)
    ap.add_argument('--warmup', type=float)
    ap.add_argument('--blocks', type=int)
    ap.add_argument('--retry-blocks', type=int)
    ap.add_argument('--tolerance', type=float)
    ap.add_argument('--idle-min', type=float, help='seconds without user input before each window (default 180)')
    ap.add_argument('--idle-wait-max', type=float, help='the longest idle wait before a window fails as "machine in use"')
    ap.add_argument('--idle-redo', type=int, help='windows discarded for input that are run again, per block (default 3)')
    ap.add_argument('--busy-margin', type=float, help='points of CPU %% above the machine baseline that make a 10 s interval '
                    'contended (default 20)')
    ap.add_argument('--job', action='append', default=[], help='MACHINE:key=JSON: a layout field of that machine\'s job line')
    for m in ('cuda', 'mac'):
        ap.add_argument(f'--{m}-new', help=f'the new {m} build (directory or id; default: the new build\'s index entry\'s)')
        ap.add_argument(f'--{m}-baseline', help=f'the old {m} build (default: the baseline\'s entry, else the previous release\'s)')
    ap.add_argument('--out')


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0], formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest='cmd', required=True)
    gate_args(sub.add_parser('gate', help='the speed gate (module docs)'))
    jd = sub.add_parser('judge', help='judge recorded windows again')
    jd.add_argument('out')
    jd.add_argument('--tolerance', type=float)
    wn = sub.add_parser('windows', help='the runner: a plan\'s windows on this machine')
    wn.add_argument('--plan', required=True)
    wn.add_argument('--out', required=True)
    wn.add_argument('--order', default=','.join(ORDER))
    wn.add_argument('--first', type=int, default=0)
    a = ap.parse_args(argv)
    if a.cmd == 'windows':
        import signal
        for name in ('SIGHUP', 'SIGTERM'):     # (an ssh that drops: the worker killed and the lock released on the way out)
            if hasattr(signal, name):
                signal.signal(getattr(signal, name), lambda n, f: sys.exit(128 + n))
        with open(a.plan, encoding='utf-8') as f:
            plan = json.load(f)
        ws = run_windows(plan, a.out, [x for x in a.order.split(',') if x], a.first)
        return 0 if all(w.get('ok') or w.get('discarded') for w in ws) else 1
    if a.cmd == 'judge':
        for name in sorted(os.listdir(a.out)):
            p = os.path.join(a.out, name, 'machine.json')
            if os.path.exists(p):
                with open(p, encoding='utf-8') as f:
                    rec = json.load(f)
                tol = a.tolerance if a.tolerance is not None else rec.get('tolerance', SPEED_DEFAULTS['tolerance'])
                rec['judge'] = judge(rec.get('windows') or [], tol, (rec.get('plan') or {}).get('busy_margin_pct',
                                                                             SPEED_DEFAULTS['busy_margin_pct']))
                rec['passed'] = bool(rec['judge'].get('passed'))
                rec['tolerance'] = tol
                with open(p, 'w', encoding='utf-8', newline='\n') as f:
                    json.dump(rec, f, indent=1)
        res = combine(a.out)
        print(res['line'])
        return 0 if res['passed'] else 1
    return Gate(a).run()


if __name__ == '__main__':
    sys.exit(main())
