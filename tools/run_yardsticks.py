"""The run's standing yardsticks (docs/ENGINE.md 164, mtgz.az.yardsticks): best-of-one head-to-heads against fixed
opponents and the canary library, on every evaluated checkpoint, once each, in order.

  python tools/run_yardsticks.py watch RUN [--supervised] [--once] [--yardsticks '<json>']
  python tools/run_yardsticks.py status RUN [--json]
  python tools/run_yardsticks.py one RUN --kind bo1|blunders --iter N [--opp LABEL]     (one measurement; the
                                                                                        watcher's child process)

`watch`: follows the run (RUN: its directory) and measures what its setting `yardsticks` asks for (config.json's,
else its preset's; `--yardsticks` replaces it for a hand start) in two lanes - the head-to-heads (the GPU) and the
canary library (the CPU) - each in order, oldest checkpoint first, nothing skipped; each measurement is a child
process (`one`) at below normal priority, its console <run>/yardsticks/work/<key>.log. A result goes to
<run>/yardsticks.jsonl; a failure is said loudly (YARDSTICK FAILED), written as a failure row and tried again after
the setting's wait (retry_s doubling to retry_max_s), the lane going on with the next checkpoint meanwhile. One
watcher per run (<run>/yardsticks/lock.json: pid and process creation time).

  * --supervised (the run's companion, mtgz.az.companions; zero-core docs/az.md 49): ends with the run - finished:
    everything still due is measured (the last checkpoint without the settling wait), then exit; stopped / refused /
    gave up: the measurement under way is killed (it is done again by the next watcher) and the watcher exits.
    Another watcher holding the lock (a hand-started one): this one waits for it.
  * by hand (no --supervised): runs until <run>/yardsticks/STOP exists, or the run's own companion takes the
    yardsticks over (its supervisor alive with a companion that runs them: the measurement under way is finished,
    then this one exits), or the run has finished and nothing is due. Another watcher holding the lock: refused.
  * --once: measure everything due now (no settling wait), then exit (the smoke).

`status`: what is measured, missing (an evaluated checkpoint without a result) and failing.
"""
import argparse
import json
import os
import subprocess
import sys
import threading
import time
import traceback

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
from mtgz.az import yardsticks as Y  # noqa: E402

LANES = ('gpu', 'cpu', 'cpu_bo1')


def say(msg):
    print(f'[{time.strftime("%Y-%m-%d %H:%M:%S")}] yardsticks: {msg}', flush=True)


# ------------------------------------------------------------------------------------------------ the lock
def lock_path(run):
    return os.path.join(run, Y.DIR, 'lock.json')


def _started(pid):
    import psutil
    try:
        return psutil.Process(pid).create_time()
    except (psutil.NoSuchProcess, psutil.AccessDenied):
        return None


def lock_owner(run):
    """The live watcher holding the run's lock -> its pid, or None (no lock, or its owner is gone: a reused pid
    does not count - the creation time must match)."""
    try:
        with open(lock_path(run), encoding='utf-8') as f:
            d = json.load(f)
    except (OSError, ValueError):
        return None
    pid, st = d.get('pid'), d.get('started')
    if not pid or pid == os.getpid():
        return None
    now = _started(int(pid))
    if now is None or st is None or abs(now - float(st)) > 1.0:
        return None
    return int(pid)


def take_lock(run):
    """-> True if this process now holds the run's lock, False when another live watcher does."""
    os.makedirs(os.path.join(run, Y.DIR), exist_ok=True)
    for _ in range(3):
        if lock_owner(run):
            return False
        try:
            fd = os.open(lock_path(run), os.O_CREAT | os.O_EXCL | os.O_WRONLY)
        except FileExistsError:
            try:
                os.remove(lock_path(run))       # (its owner is gone: lock_owner said so)
            except OSError:
                pass
            continue
        with os.fdopen(fd, 'w', encoding='utf-8') as f:
            json.dump({'pid': os.getpid(), 'started': _started(os.getpid())}, f)
        return True
    return False


def drop_lock(run):
    try:
        with open(lock_path(run), encoding='utf-8') as f:
            mine = json.load(f).get('pid') == os.getpid()
        if mine:                                # (closed first: Windows refuses to remove an open file)
            os.remove(lock_path(run))
    except (OSError, ValueError):
        pass


# ------------------------------------------------------------------------------------------------ the watcher
def kill_tree(p):
    import psutil
    try:
        q = psutil.Process(p.pid)
        for k in q.children(recursive=True):
            try:
                k.kill()
            except psutil.NoSuchProcess:
                pass
        q.kill()
    except psutil.NoSuchProcess:
        pass
    p.wait()


def tail(path, n=15):
    try:
        with open(path, encoding='utf-8', errors='replace') as f:
            return ''.join(f.readlines()[-n:]).strip()
    except OSError:
        return ''


def last_json(path):
    """The last JSON object line of a child's console with a `kind` (its result), or None."""
    out = None
    try:
        with open(path, encoding='utf-8', errors='replace') as f:
            for line in f:
                line = line.strip()
                if line.startswith('{'):
                    try:
                        d = json.loads(line)
                    except ValueError:
                        continue
                    if isinstance(d, dict) and d.get('kind') in ('bo1', 'blunders') and 'iter' in d:
                        out = d
    except OSError:
        pass
    return out


class Watcher:
    def __init__(self, run, supervised=False, once=False, override=None, log=say, python=None, poll=None,
                 child_cmd=None):
        self.run = os.path.abspath(run)
        self.supervised, self.once, self.override = supervised, once, override
        self.log = log
        self.python = python or sys.executable
        self.poll_override = poll
        self.child_cmd = child_cmd or self.one_cmd     # item -> the measurement's command (tests: a fake one)
        self.finishing = once                  # measure what is due, then end (the run finished, or --once)
        self.draining = False                  # finish the measurement under way, start none (handed over)
        self.stop = threading.Event()          # kill the measurement under way and end
        self.write = threading.Lock()
        self.procs = {}

    # the run's setting, read at every look (a restart may change it)
    def settings(self):
        cfg = Y.cfg_of(self.run)
        ys = Y.normalize(self.override) if self.override is not None else Y.of_run(self.run, cfg)
        return cfg, ys

    def lib(self, ys):
        if not ys['blunders']:
            return None
        try:
            lib = Y.library_id(ys)
        except Exception as e:                 # noqa: BLE001
            self.log(f'YARDSTICK FAILED: the canary library {Y.library_path(ys)} unreadable: {e!r} (blunders not '
                     f'measured until it is)')
            return None
        if lib is None:
            self.log(f'canary library {Y.library_path(ys)}: no kept spot - nothing to score (blunders missing)')
        return lib

    def one_cmd(self, item):
        cmd = [self.python, os.path.abspath(__file__), 'one', self.run, '--kind', item['kind'], '--iter',
               str(item['iter'])] + (['--opp', item['opp']] if item.get('opp') else [])
        if self.override is not None:
            cmd += ['--yardsticks', json.dumps(self.override)]
        return cmd

    def measure(self, item, ys):
        """One measurement as a child process -> its row (a result or a failure), appended to the results."""
        key = f"{item['kind']}_{item['iter']:04d}" + (f"_{item['opp']}" if item.get('opp') else '')
        work = os.path.join(self.run, Y.DIR, 'work')
        os.makedirs(work, exist_ok=True)
        out = os.path.join(work, key + '.log')
        cmd = self.child_cmd(item)
        kw = {}
        if os.name == 'nt':
            kw['creationflags'] = 0x08000000 | 0x00004000      # CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS
        else:
            kw['preexec_fn'] = lambda: os.nice(10)
        env = dict(os.environ, PYTHONIOENCODING='utf-8')
        env.setdefault('ZERO_TOKEN_GRAPHS', '1')
        what = f"{item['kind']} of iteration {item['iter']}" + (f" vs {item['opp']}" if item.get('opp') else '')
        self.log(f'measuring {what} (attempt {item["attempt"]}; console {os.path.relpath(out, self.run)})')
        t0 = time.time()
        with open(out, 'w', encoding='utf-8') as f:
            p = subprocess.Popen(cmd, cwd=ROOT, stdout=f, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL, env=env,
                                 **kw)
            self.procs[item['lane']] = p
            err = None
            try:
                while True:
                    try:
                        code = p.wait(timeout=5)
                        break
                    except subprocess.TimeoutExpired:
                        pass
                    if self.stop.is_set():
                        kill_tree(p)
                        self.log(f'{what}: stopped with the watcher (measured again by the next one)')
                        return None
                    if time.time() - t0 > ys['timeout_s']:
                        kill_tree(p)
                        code, err = None, f"timed out after {ys['timeout_s']:.0f} s"
                        break
            finally:
                self.procs.pop(item['lane'], None)
        row = last_json(out) if code == 0 else None
        if row is not None and (row.get('kind'), row.get('iter'), row.get('opp')) == (item['kind'], item['iter'],
                                                                                      item.get('opp')):
            with self.write:
                Y.append_row(self.run, row)
            self.log(f"{what}: " + (f"{row['score']:.4f} +- {row['se']:.4f} (Elo {row.get('elo_diff')}), "
                                    f"{row['seconds']:.0f} s" if row['kind'] == 'bo1' else
                                    f"{row['passed']} / {row['total']} passed, {row['seconds']:.0f} s"))
            return row
        err = err or (f'exit {code}' if code else 'exit 0 without a result line') + ': ' + tail(out)
        wait = Y.backoff(ys, item['attempt'])
        row = {'kind': item['kind'], 'iter': item['iter'], **({'opp': item['opp']} if item.get('opp') else {}),
               **({'library': item.get('library')} if item['kind'] == 'blunders' else {}), 'error': err[-2000:],
               'attempt': item['attempt'], 'retry_at': round(time.time() + wait), 'time': round(time.time())}
        with self.write:
            Y.append_row(self.run, row)
        self.log(f'YARDSTICK FAILED: {what} (attempt {item["attempt"]}): {err[-600:]} - tried again in {wait:.0f} s; '
                 f'the lane goes on meanwhile (console {out})')
        return row

    def lane(self, name):
        """One lane's loop: its oldest due measurement, in order, until the watcher ends."""
        while not self.stop.is_set() and not self.draining:
            try:
                cfg, ys = self.settings()
                if ys is None:
                    self.log('the run\'s yardsticks setting is off: nothing to measure')
                    return
                lib = self.lib(ys) if name == 'cpu' else None
                settle = 0 if self.finishing else None
                item = Y.pending(self.run, cfg, ys, name, lib=lib, settle=settle)
            except Exception:                  # noqa: BLE001 - said, and looked at again
                self.log('YARDSTICK FAILED: the lane\'s look at the run failed:\n' + traceback.format_exc())
                item, ys = None, None
            if item is not None:
                self.measure(item, ys)
                continue
            if self.finishing:
                return                         # (nothing due: failures still waiting stay failed / missing)
            self.stop.wait(self.poll_override or (ys or Y.DEFAULTS)['poll'])

    def handed_over(self):
        """By hand: the run's own companion now runs the yardsticks (its supervisor alive, a companion command
        naming run_yardsticks.py)."""
        from zero import runctl
        st = runctl.read_state(self.run, 'supervisor') or {}
        c = st.get('companion') or {}
        return bool(runctl.alive(st) and c.get('pid') and runctl.alive(c)
                    and any('run_yardsticks.py' in x for x in c.get('cmd') or []))

    def run_finished(self):
        from zero import runctl
        st = runctl.read_state(self.run, 'supervisor') or {}
        return st.get('state') in runctl.SUPERVISOR_DONE and st.get('ended') == 'finished'

    def end_watch(self, poll=5.0):
        from zero import runctl
        while not self.stop.is_set():
            if self.supervised:
                try:
                    e = runctl.supervisor_end()
                except Exception as ex:        # noqa: BLE001
                    self.log(f'end watch: {ex!r}')
                    e = None
                if e == 'finished' and not self.finishing:
                    self.log('the run finished: measuring what is still due, then exiting')
                    self.finishing = True
                elif e and e != 'finished':
                    self.log(f'the run ended ({e}): exiting (a measurement under way is dropped)')
                    self.stop.set()
                    return
            else:
                if os.path.exists(os.path.join(self.run, Y.DIR, 'STOP')):
                    self.log(f'{Y.DIR}/STOP: exiting (a measurement under way is dropped)')
                    self.stop.set()
                    return
                try:
                    if self.handed_over():
                        self.log('the run\'s own companion runs the yardsticks now: finishing the measurement under '
                                 'way, then exiting')
                        self.draining = True
                        return
                    if self.run_finished() and not self.finishing:
                        self.log('the run has finished: measuring what is still due, then exiting')
                        self.finishing = True
                except Exception as ex:        # noqa: BLE001
                    self.log(f'end watch: {ex!r}')
            self.stop.wait(poll)

    def main(self):
        if not take_lock(self.run):
            if not self.supervised:
                self.log(f'another watcher (pid {lock_owner(self.run)}) measures this run ({lock_path(self.run)}): '
                         f'refused')
                return 2
            self.log(f'another watcher (pid {lock_owner(self.run)}) holds {lock_path(self.run)}: waiting for it')
            while not take_lock(self.run):
                if self.supervised:
                    from zero import runctl
                    if runctl.supervisor_end():
                        self.log('the run ended while waiting for the lock: exiting')
                        return 0
                time.sleep(30)
        try:
            cfg, ys = self.settings()
            if ys is None:
                self.log('the run\'s yardsticks setting is off: nothing to measure')
                return 0
            self.log(f"watching {self.run}: {json.dumps(ys)}" + ('' if not ys['bo1_games'] else
                     f"; best-of-one opponents {[o['name'] for o in Y.opponents(self.run, cfg, ys)]}"))
            lanes = (['gpu' if ys['bo1_device'] == 'cuda' else 'cpu_bo1'] if ys['bo1_games'] else []) +                 (['cpu'] if ys['blunders'] else [])
            ew = None
            if not self.once:
                ew = threading.Thread(target=self.end_watch, daemon=True)
                ew.start()
            ts = [threading.Thread(target=self.lane, args=(n,), daemon=True) for n in lanes]
            for t in ts:
                t.start()
            for t in ts:
                while t.is_alive():
                    t.join(5)
            self.stop.set()
            st = Y.status(self.run, cfg, ys)
            self.log(f"exiting: {len(st['bo1'])} best-of-one and {len(st['blunders'])} blunder results; missing "
                     f"{len(st['missing'])}, failing {len(st['failed'])}")
            return 0
        finally:
            drop_lock(self.run)


# ------------------------------------------------------------------------------------------------ the commands
def cmd_one(a):
    try:
        import psutil
        psutil.Process().nice(psutil.BELOW_NORMAL_PRIORITY_CLASS if os.name == 'nt' else 10)
    except Exception:                          # noqa: BLE001
        pass
    run = os.path.abspath(a.run)
    cfg = Y.cfg_of(run)
    ys = Y.normalize(json.loads(a.yardsticks)) if a.yardsticks else Y.of_run(run, cfg)
    if ys is None:
        raise SystemExit('the run\'s yardsticks setting is off')
    if a.kind == 'bo1':
        if not a.opp:
            raise SystemExit('one --kind bo1 needs --opp')
        row = Y.measure_bo1(run, a.iter, a.opp, cfg, ys)
    else:
        row = Y.measure_blunders(run, a.iter, cfg, ys)
    print(json.dumps(row), flush=True)
    return 0


def cmd_status(a):
    run = os.path.abspath(a.run)
    cfg = Y.cfg_of(run)
    ys = Y.normalize(json.loads(a.yardsticks)) if a.yardsticks else Y.of_run(run, cfg)
    st = Y.status(run, cfg, ys)
    if st is None:
        print('yardsticks: off for this run')
        return 0
    if a.json:
        print(json.dumps(st, indent=1))
        return 0
    print(f"evaluated checkpoints: {st['expected']}")
    for r in st['bo1']:
        print(f"  bo1 {r['iter']:5d} vs {r['opp']:<10} {r['score']:.4f} +- {r['se']:.4f}  Elo {r.get('elo_diff')} "
              f"+- {r.get('elo_se')}  ({r['games']} games, {r['seconds']:.0f} s)")
    for r in st['blunders']:
        print(f"  blunders {r['iter']:5d}: {r['passed']} / {r['total']} passed  ({r['seconds']:.0f} s)")
    for m in st['missing']:
        print(f"  MISSING {m['kind']} {m['iter']}" + (f" vs {m['opp']}" if m.get('opp') else ''))
    for f in st['failed']:
        print(f"  FAILING {f['kind']} {f['iter']}" + (f" vs {f['opp']}" if f.get('opp') else '')
              + f" ({f['attempts']} attempts, next at {time.strftime('%H:%M', time.localtime(f['retry_at'] or 0))}): "
              f"{f['error'][-200:]}")
    return 0


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    sub = ap.add_subparsers(dest='cmd', required=True)
    w = sub.add_parser('watch')
    w.add_argument('run')
    w.add_argument('--supervised', action='store_true')
    w.add_argument('--once', action='store_true')
    w.add_argument('--yardsticks', default=None, help='the setting as JSON (default: the run\'s)')
    w.add_argument('--poll', type=float, default=None)
    s = sub.add_parser('status')
    s.add_argument('run')
    s.add_argument('--json', action='store_true')
    s.add_argument('--yardsticks', default=None)
    o = sub.add_parser('one')
    o.add_argument('run')
    o.add_argument('--kind', choices=['bo1', 'blunders'], required=True)
    o.add_argument('--iter', type=int, required=True)
    o.add_argument('--opp', default=None)
    o.add_argument('--yardsticks', default=None)
    a = ap.parse_args(argv)
    if a.cmd == 'one':
        return cmd_one(a)
    if a.cmd == 'status':
        return cmd_status(a)
    if not os.path.exists(os.path.join(a.run, 'config.json')):
        ap.error(f'{a.run}: no config.json (not a run directory)')
    return Watcher(a.run, a.supervised, a.once, json.loads(a.yardsticks) if a.yardsticks else None,
                   poll=a.poll).main()


if __name__ == '__main__':
    sys.exit(main())
