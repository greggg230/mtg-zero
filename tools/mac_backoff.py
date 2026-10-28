"""Low priority on the Mac (docs/ENGINE.md 50): a long job runs at `nice 19` in a process group of
its own and is **stopped (SIGSTOP) while a remote evaluation holds its lock**, then continued
(SIGCONT): a run's evaluations on the Mac (`~/mtgz-remote`, zero-core's `zero/remote.py`: one
evaluation at a time per workdir, the lock directory `<workdir>/.eval.lock` with `owner` = "<pid>
<time>") always take precedence.

    python tools/mac_backoff.py [--poll 5] [--log FILE] -- COMMAND [ARGS...]

runs COMMAND (nice 19, its own session), stops its whole process group within `--poll` seconds of
a lock appearing and continues it once the lock is gone; exits with COMMAND's status. SIGTERM /
SIGINT to this wrapper continue the group and pass the signal on. `tools/fuzz_farm.py` and
`tools/match_noise.py` use the same functions in-process.

The locks watched: `$MTGZ_BACKOFF_LOCKS` (colon-separated lock directories; glob patterns allowed)
or by default the mtgz remote's, `~/mtgz-remote/.eval.lock`, the other zero-core games' remotes on
the Mac (`~/kbz-remote/.eval.lock`, `~/domz-remote/.eval.lock`: their evaluations share the cores
too) and **remote self-play's busy markers** (`~/mtgz-selfplay/.busy-*`: a run's self-play worker
holds one while its phase runs, zero-core's zero/remoteplay.py, docs/ENGINE.md 52: the farm and the
studies yield to a run's self-play as to its evaluations). A lock whose owner process is gone is
ignored (zero-core removes such a stale lock at the next evaluation); a lock without its owner file
yet counts as held (being created).

A stopped job's wall clock keeps running: a job that times itself must count only the time it was
running (`Governor.paused_s`), or its watchdog would fire after a long evaluation.
"""
import argparse
import glob
import json
import os
import signal
import subprocess
import sys
import threading
import time

HOME = os.path.expanduser('~')
DEFAULT_LOCKS = [os.path.join(HOME, d, '.eval.lock') for d in ('mtgz-remote', 'kbz-remote', 'domz-remote')]
# remote self-play's busy markers (zero.remoteplay: <selfplay workdir>/.busy-<mirror>-w<K>, mtgz's workdir
# ~/mtgz-selfplay): held while a run's self-play phase runs on the Mac
SELFPLAY_BUSY = os.path.join(HOME, 'mtgz-selfplay', '.busy-*')


def lock_dirs():
    """The lock directories (or glob patterns of them) to back off for."""
    env = os.environ.get('MTGZ_BACKOFF_LOCKS')
    if env is not None:
        return [os.path.expanduser(p) for p in env.split(':') if p]
    return list(DEFAULT_LOCKS) + [SELFPLAY_BUSY]


def _expand(locks):
    for p in locks:
        if glob.has_magic(p):
            yield from sorted(glob.glob(p))
        else:
            yield p


def _alive(pid):
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False
    except OSError:
        return True                                # another user's process: alive


def held(locks=None):
    """The first lock directory held by a live owner (or being created), else None."""
    for d in _expand(lock_dirs() if locks is None else locks):
        if not os.path.isdir(d):
            continue
        try:
            with open(os.path.join(d, 'owner')) as f:
                pid = int(f.read().split()[0])
        except (OSError, ValueError, IndexError):
            return d                               # being created: held
        if _alive(pid):
            return d
    return None


def niceness():
    """This process at nice 19 (a no-op where it already is)."""
    try:
        os.nice(19 - os.nice(0))
    except (AttributeError, OSError):
        pass


def _signal_group(pgid, sig):
    try:
        os.killpg(pgid, sig)
        return True
    except (ProcessLookupError, PermissionError):
        return False


class Governor:
    """A thread that stops every registered process group while a lock is held (or `pause_file`
    exists) and continues them after. `paused_s`: seconds stopped so far (a job's running time is
    its wall time less what it spent stopped while registered: `active(t0)`)."""

    def __init__(self, poll=5.0, locks=None, pause_file=None, log=None):
        self.poll, self.locks, self.pause_file, self.logf = poll, locks, pause_file, log
        self.groups = {}                           # pgid -> registration time
        self.mu = threading.Lock()
        self.stopped = False
        self.why = None
        self.paused_s = 0.0
        self._since = None
        self.events = 0                            # back-off episodes
        self._quit = threading.Event()
        self.th = threading.Thread(target=self._loop, name='backoff', daemon=True)
        self.th.start()

    def log(self, msg):
        if self.logf:
            with open(self.logf, 'a') as f:
                f.write(time.strftime('%Y-%m-%d %H:%M:%S ') + msg + '\n')

    def register(self, pgid):
        with self.mu:
            self.groups[pgid] = time.time()
            if self.stopped:
                _signal_group(pgid, signal.SIGSTOP)

    def unregister(self, pgid):
        with self.mu:
            self.groups.pop(pgid, None)

    def paused_total(self):
        with self.mu:
            return self.paused_s + (time.time() - self._since if self._since else 0.0)

    def reason(self):
        if self.pause_file and os.path.exists(self.pause_file):
            return f'pause file {self.pause_file}'
        h = held(self.locks)
        return f'remote evaluation ({h})' if h else None

    def _loop(self):
        while not self._quit.is_set():
            why = self.reason()
            with self.mu:
                if why and not self.stopped:
                    for g in list(self.groups):
                        _signal_group(g, signal.SIGSTOP)
                    self.stopped, self.why, self._since = True, why, time.time()
                    self.events += 1
                    self.log(f'backing off: {why}; stopped {sorted(self.groups)}')
                elif not why and self.stopped:
                    for g in list(self.groups):
                        _signal_group(g, signal.SIGCONT)
                    took = time.time() - self._since
                    self.paused_s += took
                    self.stopped, self.why, self._since = False, None, None
                    self.log(f'continuing after {took:.0f} s; continued {sorted(self.groups)}')
            self._quit.wait(self.poll)

    def wait_clear(self, check=None):
        """Block while backing off (for cooperative callers between work units)."""
        while self.stopped or self.reason():
            if check is not None:
                check()
            time.sleep(self.poll)

    def close(self):
        self._quit.set()
        with self.mu:
            if self.stopped:
                for g in list(self.groups):
                    _signal_group(g, signal.SIGCONT)
                self.paused_s += time.time() - self._since
                self.stopped, self._since = False, None


def spawn(cmd, gov=None, **kw):
    """Popen at nice 19 in a session of its own (its pgid = its pid), registered with `gov`."""
    def pre():
        os.setsid()
        try:
            os.nice(19 - os.nice(0))
        except OSError:
            pass
    p = subprocess.Popen(cmd, preexec_fn=pre, **kw)
    if gov is not None:
        gov.register(p.pid)
    return p


def kill_group(pgid, grace=5.0):
    """Continue the group (a stopped process ignores SIGTERM until then), SIGTERM, then SIGKILL."""
    _signal_group(pgid, signal.SIGCONT)
    if not _signal_group(pgid, signal.SIGTERM):
        return
    t = time.time() + grace
    while time.time() < t:
        if not _signal_group(pgid, 0):
            return
        time.sleep(0.2)
    _signal_group(pgid, signal.SIGKILL)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('--poll', type=float, default=5.0)
    ap.add_argument('--log', default=None, help='append back-off events here')
    ap.add_argument('--status', default=None, help='a JSON file: state, paused seconds, back-offs (rewritten each minute)')
    ap.add_argument('cmd', nargs=argparse.REMAINDER)
    a = ap.parse_args()
    cmd = a.cmd[1:] if a.cmd[:1] == ['--'] else a.cmd
    if not cmd:
        ap.error('no command')
    niceness()
    gov = Governor(a.poll, log=a.log)
    p = spawn(cmd, gov)
    gov.log(f'started pid {p.pid}: {" ".join(cmd)}')

    def forward(sig, _frame):
        if sig == signal.SIGTERM:
            kill_group(p.pid)
        else:
            _signal_group(p.pid, signal.SIGCONT)
            _signal_group(p.pid, sig)
    signal.signal(signal.SIGTERM, forward)
    signal.signal(signal.SIGINT, forward)
    t0 = time.time()
    while True:
        try:
            code = p.wait(timeout=60)
            break
        except subprocess.TimeoutExpired:
            if a.status:
                with open(a.status + '.tmp', 'w') as f:
                    json.dump({'pid': p.pid, 'cmd': cmd, 'state': 'backing off' if gov.stopped else 'running',
                               'why': gov.why, 'wall_s': round(time.time() - t0), 'paused_s': round(gov.paused_total()),
                               'backoffs': gov.events, 'time': time.time()}, f)
                os.replace(a.status + '.tmp', a.status)
    gov.close()
    gov.log(f'pid {p.pid} exited {code} after {time.time() - t0:.0f} s ({gov.paused_s:.0f} s stopped, {gov.events} back-offs)')
    return code


if __name__ == '__main__':
    sys.exit(main())
