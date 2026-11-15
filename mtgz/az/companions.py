"""The run's companion processes (docs/ENGINE.md 164.4; zero-core docs/az.md 49): the game package's companion hook,
composing the run's delverai publisher (mtgz.az.publish, ENGINE.md 163) and its yardsticks' watcher
(mtgz.az.yardsticks, 164) into the ONE companion the supervisor keeps per run.

  * one of them on: its own spec, unchanged (a run with only the publisher has exactly 163's command);
  * both: `python -m mtgz.az.companions --child publisher <log> <cmd JSON> --child yardsticks <log> <cmd JSON>`, a
    small keeper (console companions_console.txt) that starts each child from this checkout with the supervisor's
    environment (ZERO_SUPERVISOR_PID / _STARTED / ZERO_COMPANION_RUN, so each child reads the run's end itself,
    zero.runctl.supervisor_end), its console its own file in the run directory, and:
      - a child that exits while the run goes on (or is paused) is started again, at most 3 times within an hour,
        then given up loudly (the other child goes on);
      - once the run has ended (supervisor_end() says so) no child is started again; the keeper waits for its
        children to finish their last work and exits when none is left (0; 1 when a child was given up);
      - the supervisor's stop kills the keeper with its children (zero-core kill_tree).
    A child that cannot be started at all (its hook raised, e.g. the publisher's missing upload key) is said loudly
    by the supervisor's COMPANION NOT STARTED for that child, and the other one runs alone.

Light (no torch, no numpy): the supervisor imports it.
"""
import argparse
import json
import os
import subprocess
import sys
import time

from . import publish, yardsticks

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
LOG = 'companions_console.txt'
RESTARTS = 3


def companion(run, cfg):
    """zero-core's companion hook (GamePackage.companion): the publisher, the yardsticks' watcher, both (the keeper)
    or None. One that refuses (raises) while the other is on: the other alone, the refusal said on stderr (the
    supervisor's console) and in the keeper's log; both refusing / the only one refusing: raises (the supervisor's
    COMPANION NOT STARTED)."""
    specs, errors = [], []
    for name, hook in (('publisher', publish.companion), ('yardsticks', yardsticks.companion)):
        try:
            s = hook(run, cfg)
        except (ValueError, OSError) as e:
            errors.append(f'{name}: {e}')
            continue
        if s:
            specs.append(s)
    if errors and not specs:
        raise ValueError('; '.join(errors))
    if not specs:
        return None
    if errors:
        print(f'companions: COMPANION NOT STARTED - {"; ".join(errors)} (the others run)', file=sys.stderr, flush=True)
    if len(specs) == 1:
        return specs[0]
    cmd = [sys.executable, '-m', 'mtgz.az.companions']
    for s in specs:
        cmd += ['--child', s['name'], s.get('log') or f"{s['name']}_console.txt", json.dumps(s['cmd'])]
    return {'name': 'companions', 'cmd': cmd, 'log': LOG}


def say(msg):
    print(f'[{time.strftime("%Y-%m-%d %H:%M:%S")}] companions: {msg}', flush=True)


def _end():
    """The run's end as the supervisor wrote it (zero.runctl.supervisor_end), or None while it goes on. Without a
    supervisor's environment (a test, a hand start): never."""
    if not os.environ.get('ZERO_SUPERVISOR_PID'):
        return None
    from zero import runctl
    try:
        return runctl.supervisor_end()
    except Exception as e:                     # noqa: BLE001 - a state file being written: next look
        say(f'end watch: {e!r}')
        return None


class Child:
    def __init__(self, name, log, cmd, run):
        self.name, self.log, self.cmd, self.run = name, log, cmd, run
        self.p = None
        self.out = None
        self.deaths = []
        self.gave_up = False

    def start(self):
        kw = {}
        if os.name == 'nt':
            kw['creationflags'] = 0x08000000 | 0x00004000      # CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS
        self.out = open(os.path.join(self.run, self.log), 'a', encoding='utf-8')
        try:
            self.p = subprocess.Popen(self.cmd, cwd=ROOT, stdout=self.out, stderr=subprocess.STDOUT,
                                      stdin=subprocess.DEVNULL, env=dict(os.environ, PYTHONIOENCODING='utf-8'), **kw)
        except OSError as e:
            self.out.close()
            self.p, self.gave_up = None, True
            say(f'CHILD NOT STARTED ({self.name}): {e!r}')
            return
        say(f'{self.name} started: pid {self.p.pid}, console {self.log}: {" ".join(self.cmd)}')

    def exited(self):
        """-> its exit code once it has exited (once), else None."""
        if self.p is None or self.p.poll() is None:
            return None
        code = self.p.returncode
        self.out.close()
        self.p = None
        return code


def keep(children, run, poll=5.0, restarts=RESTARTS, end=_end, clock=time.time, sleep=time.sleep):
    """Start the children, keep them up while the run goes on, end when the run has ended and none is left ->
    exit status (1 if a child was given up)."""
    kids = [Child(n, lg, cmd, run) for n, lg, cmd in children]
    for k in kids:
        k.start()
    ended = None
    while True:
        if ended is None:
            ended = end()
            if ended:
                say(f'the run ended ({ended}): waiting for the children to finish')
        for k in kids:
            code = k.exited()
            if code is None:
                continue
            say(f'{k.name} exited with code {code}')
            if ended:
                continue
            now = clock()
            k.deaths = [t for t in k.deaths if now - t < 3600] + [now]
            if len(k.deaths) > restarts:
                k.gave_up = True
                say(f'CHILD GIVEN UP ({k.name}): it exited {len(k.deaths)} times within an hour (see {k.log}); the '
                    f'others go on')
                continue
            say(f'{k.name}: starting it again')
            k.start()
        alive = [k for k in kids if k.p is not None]
        if not alive and (ended or all(k.gave_up for k in kids)):
            code = 1 if any(k.gave_up for k in kids) else 0
            say(f'no child left: exiting ({code})')
            return code
        sleep(poll)


def main(argv=None):
    ap = argparse.ArgumentParser(description='the run\'s companion processes (docs/ENGINE.md 164.4)')
    ap.add_argument('--child', nargs=3, action='append', metavar=('NAME', 'LOG', 'CMD_JSON'), required=True)
    ap.add_argument('--run', default=None, help='the run directory (default: $ZERO_COMPANION_RUN)')
    ap.add_argument('--poll', type=float, default=5.0)
    a = ap.parse_args(argv)
    run = a.run or os.environ.get('ZERO_COMPANION_RUN')
    if not run or not os.path.isdir(run):
        ap.error('no run directory (--run, or the supervisor\'s ZERO_COMPANION_RUN)')
    children = [(n, lg, json.loads(c)) for n, lg, c in a.child]
    return keep(children, run, a.poll)


if __name__ == '__main__':
    sys.exit(main())
