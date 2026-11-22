"""The run's standing yardsticks (docs/ENGINE.md 164, mtgz.az.yardsticks): best-of-one head-to-heads against fixed
opponents and the canary library, on every evaluated checkpoint, once each, in order.

  python tools/run_yardsticks.py watch RUN [--supervised] [--once] [--yardsticks '<json>']
  python tools/run_yardsticks.py status RUN [--json]
  python tools/run_yardsticks.py one RUN --kind bo1|blunders --iter N [--opp LABEL]     (one measurement; the
                                                                                        watcher's child process)
  python tools/run_yardsticks.py one RUN --kind cells|cells_gen --iter N --ref LABEL --pair X,Y
                                 [--no-publish] [--title T --note N --series-label L --ref-run R --ref-gen G ...]
                                 (against an OUTSIDE reference: the run's refmatch.<run> record on delverai, below)
  python tools/run_yardsticks.py refmatch RUN [--ref LABEL --ref-ckpt PATH] [--rows FILE ...] [--dry-run] [fields]
                                 (rebuild and post the run's reference-match record: ENGINE.md 225)
  python tools/run_yardsticks.py one RUN --kind cells_base --ref LABEL --pair X,Y --games G
  python tools/run_yardsticks.py one RUN --kind cells_fit --iter N        (the pool mode's joint fit: ENGINE.md 186)
  python tools/run_yardsticks.py cells RUN [--json] [--fit] [--boot B]   (the metagame cells' report: ENGINE.md 177;
                                 the joint fit's table in the pool mode, or with --fit: ENGINE.md 186)
  python tools/run_yardsticks.py cells-fit RUN [--json] [--boot B]       (the joint fit from the records only: no games)
  python tools/run_yardsticks.py cells-freeze RUN --iter N [--label L] [--to PATH]    (the next reference)
  python tools/run_yardsticks.py cells-match CKPT --ref PATH [--games G] [--sims S] [--decks a,b,c] [--cache DIR]
                                 [--out FILE.jsonl] [--device cpu|cuda] [--json] [--records DIR]   (standalone;
                                 --records: every game a game record, docs/gamedb.md)
                                 [--backend auto|farm|local] [--reuse-control] [--farm-share X] [--farm-deadline-h H]
                                 (auto, the default: the farm when it can play exactly these games and the
                                 comparison's earlier games were not played here, docs/ENGINE.md 217)
                                 [--publish EXPERIMENT --title T [--a-label ..]]     (and to delverai's h2h tab:
                                 the overall match and one per cell, mtgz.az.site_h2h, ENGINE.md 193)

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
  * --take-over (by hand, docs/ENGINE.md 186.7): the run's own companion watcher (an older checkout's) holds the
    lock - it is stopped (its process tree; the measurement under way is done again) and this one takes the lock and
    keeps it (no hand-back to the companion: the supervisor restarts its companion, which waits for the lock) until
    <run>/yardsticks/STOP or the run's end. With --yardsticks '{..., "cells_mode": "pool"}' it switches a live run's
    cells to the pool without stopping training. A lock held by anything but the run's companion: refused.

A lane runs one measurement at a time, except the Mac's ('mac': cells_backend 'mac', docs/ENGINE.md 198): there
cells_mac_jobs // 2 at once (mtgz.az.yardsticks.lane_parallel; the oldest due first, a cell still waiting for its
baseline under way skipped), each a child process of its own; and the cells' local lane ('gpu' or 'cpu_bo1') with
cells_local_jobs > 1 (docs/ENGINE.md 199): that many at once, the same way.

`status`: what is measured, missing (an evaluated checkpoint without a result) and failing.

Reference matches (docs/ENGINE.md 225; mtgz.az.site_refmatch): a mirror cells result (pair X,X, kind 'cells') against
a reference from OUTSIDE the run - a cells_refs 'LABEL=PATH' that is no file of the run, e.g. pb1@262 - posts the
run's record refmatch.<run> on delverai (its page's reference-match chart) BY DEFAULT, rebuilt whole from every such
row on disk (<run>/yardsticks.jsonl, <run>/yardsticks/refmatch_rows.jsonl: `one`'s by hand, and `--rows` files):

  * `one` by hand: checks before the games that the post can work (an upload key; a mirror pair - the record shows
    mirrors only), registers the reference's network under its label (<run>/yardsticks/refmatch.json, with the
    fields the flags give: --series-label, --title, --note, --deck-label, --ref-run, --ref-gen, --mark ITER=LABEL,
    --row-note ITER=TEXT; derived ones otherwise), measures, prints the row as always, keeps it in
    refmatch_rows.jsonl and posts. `--no-publish`: a loud PUBLISH OFF line, the row kept (a later `refmatch` posts
    it). A failed post: PUBLISH FAILED with the retry command, exit 3 after the row was printed.
  * `watch`: after each such result it appended (the yardsticks setting refmatch_publish, default true; false: PUBLISH
    OFF at each result); a failed post is said (PUBLISH FAILED) and the lane goes on - the next result posts again.
  * `refmatch RUN`: the record rebuilt and posted (or `--dry-run` printed) now, e.g. after `--no-publish`, or to
    adopt a sidecar's rows (`--rows FILE --ref LABEL` with `--ref-ckpt PATH` or the rows' own ref_sha).
A record with no row is never posted: that is a failure (exit 3), not a quiet no-op.
"""
import argparse
import json
import os
import re
import subprocess
import sys
import threading
import time
import traceback

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
from mtgz.az import site_h2h as S, site_refmatch as RM, yardsticks as Y  # noqa: E402

LANES = ('gpu', 'cpu', 'cpu_bo1')
CHILD_ENV = 'MTGZ_YARDSTICK_CHILD'        # set in a watcher's measurement: the watcher posts its reference matches


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


def overdue(item, t0, last, timeout_s, now=None):
    """Why the measurement under way (started at `t0`; its console last grew at `last`) is killed now, or None. On
    the GPU or CPU lane: `timeout_s` after it started. A farm best of one (lane 'farm', docs/ENGINE.md 165.6, 175.6):
    `timeout_s` without progress - its console unchanged that long (the campaign says every pull that brought games and
    every 64-game piece of its local fallback) - so one that is delivering games is never killed part way: a kill lost
    the local piece under way and the next attempt played it again, and with the farm's wait taking 3/4 of the budget
    the fallback's first attempt had ~30 min for hours of games (pb3's best of one against gen0, 2026-11-16)."""
    now = time.time() if now is None else now
    if item.get('lane') != 'farm':
        return f'timed out after {timeout_s:.0f} s' if now - t0 > timeout_s else None
    if now - max(t0, last) > timeout_s:
        return f'no progress for {timeout_s:.0f} s (its console unchanged; {now - t0:.0f} s in all)'
    return None


def kill_pid_tree(pid):
    import psutil
    try:
        q = psutil.Process(pid)
        for k in q.children(recursive=True):
            try:
                k.kill()
            except psutil.NoSuchProcess:
                pass
        q.kill()
        q.wait(30)
    except (psutil.NoSuchProcess, psutil.TimeoutExpired):
        pass


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
                    if isinstance(d, dict) and d.get('kind') in Y.KINDS and ('iter' in d or 'base' in d):
                        out = d
    except OSError:
        pass
    return out


def same_measurement(row, item):
    """Whether a child's result line is the measurement asked for (bo1 / blunders: kind, iteration, opponent - the
    library is the child's own reading; the cells: their whole key)."""
    if item['kind'] in Y.CELL_KINDS:
        return row.get('kind') == item['kind'] and Y.key_of(row) == Y.key_of(item)
    return (row.get('kind'), row.get('iter'), row.get('opp')) == (item['kind'], item['iter'], item.get('opp'))


def _deck(name):
    return re.sub(r'[^A-Za-z0-9]+', '', name.split('_')[0])[:12] or 'deck'


def item_key(item):
    """A measurement's console name and its words in the log -> (key, what)."""
    k = item['kind']
    if k in Y.CELL_KINDS:
        pair = '-'.join(_deck(x) for x in item['pair'])
        ref = re.sub(r'[^A-Za-z0-9_.-]+', '_', item['ref'])
        if k == 'cells_base':
            return (f"cells_base_{ref}_{pair}_{item['base'] or 'noref'}",
                    f"cells baseline of {item['ref']} in {' / '.join(item['pair'])}")
        return (f"{k}_{item['iter']:04d}_{ref}_{pair}",
                f"{'cells' if k == 'cells' else 'gen-over-gen cells'} of iteration {item['iter']} vs {item['ref']} in "
                f"{' / '.join(item['pair'])}")
    key = f"{k}_{item['iter']:04d}" + (f"_{item['opp']}" if item.get('opp') else '')
    return key, f"{k} of iteration {item['iter']}" + (f" vs {item['opp']}" if item.get('opp') else '')


def result_line(row):
    k = row['kind']
    if k == Y.FIT_KIND:
        o = row.get('overall')
        return (f"fit of {row.get('games')} games: overall " + (f"{o['elo']:+.1f} +- {o['se']:.1f}, anchored "
                f"{row['anchored']['elo']:.1f}" if o else 'incomplete') +
                (f"; promote ({row['promote']['why']}) {row['promote'].get('label') or 'suggested'}"
                 if row.get('promote') else '') + f", {row.get('seconds', 0):.0f} s")
    if k == 'bo1':
        return f"{row['score']:.4f} +- {row['se']:.4f} (Elo {row.get('elo_diff')}), {row['seconds']:.0f} s"
    if k == 'blunders':
        return f"{row['passed']} / {row['total']} passed, {row['seconds']:.0f} s"
    if k == 'cells_base':
        return (f"baseline {row['base']} " + ', '.join(f'{c} {s:.3f}' for c, s in sorted(row['scores'].items()))
                + f" ({'cached' if row.get('cached') else 'played'}), {row['seconds']:.0f} s")
    return ', '.join(f"{c} {s['elo']:+.0f} +- {s['se']:.0f}" for c, s in sorted(row['cells'].items())) + \
        f", {row['seconds']:.0f} s"


def _is_or_child_of(pid, parent):
    """pid is `parent` or one of its descendants (a venv launcher's interpreter) - False when either is unknown."""
    if not pid or not parent:
        return False
    if pid == parent:
        return True
    try:
        import psutil
        return any(p.pid == parent for p in psutil.Process(pid).parents())
    except Exception:                          # noqa: BLE001 (gone, no psutil, no access)
        return False


class Watcher:
    def __init__(self, run, supervised=False, once=False, override=None, log=say, python=None, poll=None,
                 child_cmd=None, take_over=False):
        self.run = os.path.abspath(run)
        self.take_over = take_over             # (by hand: the run's companion stopped, never handed back to)
        self.supervised, self.once, self.override = supervised, once, override
        self.log = log
        self.python = python or sys.executable
        self.poll_override = poll
        self.child_cmd = child_cmd or self.one_cmd     # item -> the measurement's command (tests: a fake one)
        self.finishing = once                  # measure what is due, then end (the run finished, or --once)
        self.draining = False                  # finish the measurement under way, start none (handed over)
        self.stop = threading.Event()          # kill the measurement under way and end
        self.write = threading.Lock()
        self.posting = threading.Lock()        # (one reference-match post at a time: each replaces the record)
        self.procs = {}
        self.alive = set()                     # (the lanes still running: the fit lane outlives them when finishing)
        self.tick = 5.0                        # seconds between looks at the measurement under way (tests: less)

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
        cmd = [self.python, os.path.abspath(__file__), 'one', self.run, '--kind', item['kind']]
        if item.get('iter') is not None:
            cmd += ['--iter', str(item['iter'])]
        cmd += ['--opp', item['opp']] if item.get('opp') else []
        if item['kind'] in Y.CELL_KINDS:
            cmd += ['--ref', item['ref'], '--pair', ','.join(item['pair'])]
            if item['kind'] == 'cells_base':
                cmd += ['--games', str(item['games'])]
            elif item.get('role'):             # (a pool leg, docs/ENGINE.md 186)
                cmd += ['--games', str(item['games']), '--role', item['role']]
        if self.override is not None:
            cmd += ['--yardsticks', json.dumps(self.override)]
        return cmd

    def measure(self, item, ys):
        """One measurement as a child process -> its row (a result or a failure), appended to the results."""
        key, what = item_key(item)
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
        env[CHILD_ENV] = '1'                   # (the child does not post a reference match: this watcher does)
        self.log(f'measuring {what} (attempt {item["attempt"]}; console {os.path.relpath(out, self.run)})')
        t0 = time.time()
        with open(out, 'w', encoding='utf-8') as f:
            p = subprocess.Popen(cmd, cwd=ROOT, stdout=f, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL, env=env,
                                 **kw)
            self.procs[key] = p                # (by measurement: a lane may run several at once)
            err = None
            seen = [0, t0]                     # the console's size and when it last grew
            try:
                while True:
                    try:
                        code = p.wait(timeout=self.tick)
                        break
                    except subprocess.TimeoutExpired:
                        pass
                    if self.stop.is_set():
                        kill_tree(p)
                        self.log(f'{what}: stopped with the watcher (measured again by the next one)')
                        return None
                    size = os.fstat(f.fileno()).st_size     # (the handle's: a directory entry's lags on Windows)
                    if size != seen[0]:
                        seen[:] = [size, time.time()]
                    late = overdue(item, t0, seen[1], ys['timeout_s'])
                    if late:
                        kill_tree(p)
                        code, err = None, late
                        break
            finally:
                self.procs.pop(key, None)
        row = last_json(out) if code == 0 else None
        if row is not None and row.get('error') is None and same_measurement(row, item):
            with self.write:
                Y.append_row(self.run, row)
            self.log(f"{what}: {result_line(row)}")
            self.refmatch(row, ys)
            return row
        err = err or (f'exit {code}' if code else 'exit 0 without a result line') + ': ' + tail(out)
        wait = Y.backoff(ys, item['attempt'])
        row = {'kind': item['kind'], **({'iter': item['iter']} if 'iter' in item else {}),
               **({'opp': item['opp']} if item.get('opp') else {}),
               **({'library': item.get('library')} if item['kind'] == 'blunders' else {}),
               **({k: item[k] for k in ('ref', 'pair', 'base', 'games') if k in item}
                  if item['kind'] in Y.CELL_KINDS else {}),
               'error': err[-2000:], 'attempt': item['attempt'], 'retry_at': round(time.time() + wait),
               'time': round(time.time())}
        with self.write:
            Y.append_row(self.run, row)
        self.log(f'YARDSTICK FAILED: {what} (attempt {item["attempt"]}): {err[-600:]} - tried again in {wait:.0f} s; '
                 f'the lane goes on meanwhile (console {out})')
        return row

    def refmatch(self, row, ys):
        """A mirror cells result against an outside reference -> the run's reference-match record posted (docs/ENGINE.md
        225), or PUBLISH OFF / PUBLISH FAILED said; never raises (publishing never stops the yardsticks)."""
        pair = row.get('pair') or []
        if row.get('kind') != 'cells' or len(pair) != 2 or pair[0] != pair[1]:
            return
        name = RM.record_name(self.run)
        try:
            cfg = Y.cfg_of(self.run)
            e = RM.ref_entry(self.run, cfg, ys, row['ref'])
            if e is None or not RM.outside_ref(self.run, cfg, ys, row['ref']):
                return
            if not ys.get('refmatch_publish', True):
                self.log(RM.published_off(self.run, 'the yardsticks setting refmatch_publish is false'))
                return
            with self.posting:
                meta = RM.read_meta(self.run)
                RM.register(meta, row['ref'], row.get('ref_sha'), e['path'])
                RM.save_meta(self.run, meta)
                rec = RM.publish(self.run, log=self.log, rows_log=False)
            self.log(f"{name} posted: {sum(len(x['rows']) for x in rec['series'])} rows in {len(rec['series'])} series")
        except Exception as ex:                # noqa: BLE001 - said; the next result posts again
            self.log(f"PUBLISH FAILED: {name}: {ex} - the row is in {Y.ROWS}; the next cells result against "
                     f"{row.get('ref')} posts again, or now: python tools/run_yardsticks.py refmatch {self.run}")

    def refmatch_plan(self, cfg, ys):
        """The watcher's start: which outside references its cells post as refmatch.<run> (or PUBLISH OFF), said."""
        if not (ys['cells_games'] or ys['cells_gen_games']):
            return
        try:
            outs = [r['label'] for r in Y.cell_refs(self.run, cfg, ys) if RM.outside_ref(self.run, cfg, ys, r['label'])]
        except Exception as ex:                # noqa: BLE001
            self.log(f'reference matches: the ladder unreadable ({ex!r})')
            return
        if not outs:
            return
        name = RM.record_name(self.run)
        if not ys['refmatch_publish']:
            self.log(f'PUBLISH OFF: {name} - the mirror cells against the outside reference(s) {outs} will not appear on '
                     f'the Delver AI site (the yardsticks setting refmatch_publish is false; docs/ENGINE.md 225)')
            return
        try:
            RM.check_key()
            self.log(f'reference matches: the mirror cells against {outs} post {name} (docs/ENGINE.md 225)')
        except RM.RefmatchError as ex:
            self.log(f'PUBLISH FAILED: {name} cannot be posted: {ex} - the yardsticks go on, the results stay on disk')

    def lane(self, name):
        """One lane's loop: its oldest due measurement, in order, until the watcher ends."""
        try:
            self._lane(name)
        finally:
            self.alive.discard(name)

    def _lane(self, name):
        running = {}                           # (a parallel lane's measurements under way: key -> thread)
        done = threading.Event()               # (one of them ended: look again at once)
        try:
            self._lane_loop(name, running, done)
        finally:
            for t in list(running.values()):   # (stopped: each kills its child; drained: each finishes)
                t.join()

    def _lane_loop(self, name, running, done):
        while not self.stop.is_set():
            for k, t in list(running.items()):
                if not t.is_alive():
                    running.pop(k)
            if self.draining:
                if not running:
                    return
                done.wait(min(self.tick, 5.0))
                done.clear()
                continue
            item, par = None, 1
            try:
                cfg, ys = self.settings()
                if ys is None:
                    self.log('the run\'s yardsticks setting is off: nothing to measure')
                    return                     # (_lane waits for the measurements under way)
                par = Y.lane_parallel(ys, name)
                if len(running) < par:
                    lib = self.lib(ys) if name == 'cpu' else None
                    settle = 0 if self.finishing else None
                    item = Y.pending(self.run, cfg, ys, name, lib=lib, settle=settle, busy=set(running))
            except Exception:                  # noqa: BLE001 - said, and looked at again
                self.log('YARDSTICK FAILED: the lane\'s look at the run failed:\n' + traceback.format_exc())
                item, ys = None, None
            if item is not None and par <= 1 and not running:
                self.measure(item, ys)         # (one at a time: in this thread, as always)
                continue
            if item is not None:
                key = Y.key_of(item, item.get('library'))

                def run(item=item, ys=ys):
                    try:
                        self.measure(item, ys)
                    except Exception:          # noqa: BLE001 - said; the item stays due
                        self.log('YARDSTICK FAILED: a measurement thread failed:\n' + traceback.format_exc())
                    finally:
                        done.set()
                t = threading.Thread(target=run, daemon=True)
                running[key] = t
                t.start()
                continue
            if running:                        # (full, or nothing more due yet: until one ends or the next look)
                done.wait(min(self.tick if len(running) >= par else (self.poll_override or (ys or Y.DEFAULTS)['poll']),
                              60.0))
                done.clear()
                continue
            if self.finishing:
                if name == 'fit' and self.alive - {'fit'}:
                    self.stop.wait(min(self.tick, 2.0))     # (the fits wait on the other lanes' measurements)
                    continue
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
                    if not self.take_over and self.handed_over():
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

    def seize(self, wait_s=10.0):
        """--take-over: the lock from the run's own companion watcher -> True when this process holds it. The
        companion (the pid in the supervisor's state, a run_yardsticks.py command, the lock's owner) is killed with its
        measurement and the lock taken at once (the supervisor restarts its companion, which then waits for the lock);
        anything else holding the lock is refused."""
        from zero import runctl
        for _ in range(3):
            if take_lock(self.run):
                return True
            pid = lock_owner(self.run)
            if pid is None:
                continue
            c = (runctl.read_state(self.run, 'supervisor') or {}).get('companion') or {}
            # A venv's python.exe is a launcher on Windows: the supervisor records the launcher's pid, the lock holds
            # its child's (the interpreter). Either is the companion.
            if not any('run_yardsticks.py' in x for x in c.get('cmd') or []) or not _is_or_child_of(pid, c.get('pid')):
                self.log(f'--take-over: the lock is held by pid {pid}, not the run\'s companion watcher: refused '
                         f'(stop it first: {Y.DIR}/STOP)')
                return False
            self.log(f"--take-over: stopping the run's companion watcher pid {pid} (its measurement under way is "
                     f"measured again) and taking its lock")
            kill_pid_tree(c.get('pid') or pid)
            t0 = time.time()
            while time.time() - t0 < wait_s:
                if take_lock(self.run):
                    return True
                time.sleep(0.05)
        return False

    def main(self):
        if self.take_over and not self.seize():
            self.log('--take-over: the lock was not taken')
            return 2
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
            self.refmatch_plan(cfg, ys)
            from mtgz.az import farm_campaign as C
            note = C.run_pool_note(cfg)        # (farm settings that do nothing: said, docs/ENGINE.md 218)
            if note:
                self.log(f'WARNING: {note}')
            lanes = Y.lanes(self.run, cfg, ys)
            ew = None
            if not self.once:
                ew = threading.Thread(target=self.end_watch, daemon=True)
                ew.start()
            self.alive = set(lanes)
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
def _ys_of(a, run, cfg):
    return Y.normalize(json.loads(a.yardsticks)) if a.yardsticks else Y.of_run(run, cfg)


def cmd_one(a):
    try:
        import psutil
        psutil.Process().nice(psutil.BELOW_NORMAL_PRIORITY_CLASS if os.name == 'nt' else 10)
    except Exception:                          # noqa: BLE001
        pass
    run = os.path.abspath(a.run)
    cfg = Y.cfg_of(run)
    ys = _ys_of(a, run, cfg)
    if ys is None:
        raise SystemExit('the run\'s yardsticks setting is off')
    if a.kind in Y.CELL_KINDS:
        if not a.ref or not a.pair or len(a.pair.split(',')) != 2:
            raise SystemExit(f'one --kind {a.kind} needs --ref and --pair X,Y')
        pair = tuple(a.pair.split(','))
        if a.kind == 'cells_base':
            if not a.games:
                raise SystemExit('one --kind cells_base needs --games')
            row = Y.measure_cells_base(run, a.ref, pair, a.games, cfg, ys)
        else:
            if a.iter is None:
                raise SystemExit(f'one --kind {a.kind} needs --iter')
            plan = refmatch_plan(a, run, cfg, ys, pair)
            row = Y.measure_cells(run, a.iter, a.ref, pair, cfg, ys, kind=a.kind, games=a.games, role=a.role)
            if plan is not None:
                print(json.dumps(row), flush=True)          # (the result line first, as always)
                return refmatch_after(a, run, row, plan)
    elif a.iter is None:
        raise SystemExit(f'one --kind {a.kind} needs --iter')
    elif a.kind == Y.FIT_KIND:
        row = Y.measure_cells_fit(run, a.iter, cfg, ys)
    elif a.kind == 'bo1':
        if not a.opp:
            raise SystemExit('one --kind bo1 needs --opp')
        row = Y.measure_bo1(run, a.iter, a.opp, cfg, ys)
    else:
        row = Y.measure_blunders(run, a.iter, cfg, ys)
    print(json.dumps(row), flush=True)
    return 0


def _err(msg):
    print(msg, file=sys.stderr, flush=True)


def refmatch_fields(a):
    """The flags' human fields of a reference match (mtgz.az.site_refmatch.apply_fields)."""
    return {'label': a.series_label, 'title': a.title, 'note': a.note, 'deck_label': a.deck_label,
            'ref_run': a.ref_run, 'ref_gen': a.ref_gen, 'marks': RM.parse_pairs(a.mark, '--mark'),
            'row_notes': RM.parse_pairs(a.row_note, '--row-note')}


def refmatch_plan(a, run, cfg, ys, pair):
    """Before `one`'s cells games (docs/ENGINE.md 225): against an outside reference by hand -> 'post' or 'off' (the
    row kept either way; the reference registered with the flags' fields), else None. SystemExit when the post could
    not work (no key, a pair that is no mirror, a label of another network) unless --no-publish."""
    if os.environ.get(CHILD_ENV) == '1' or a.kind != 'cells' or not RM.outside_ref(run, cfg, ys, a.ref):
        return None
    try:
        given = refmatch_fields(a)
        if not a.no_publish:
            if pair[0] != pair[1]:
                raise RM.RefmatchError(f'the reference-match record shows mirrors only, not {pair[0]},{pair[1]}: give '
                                       f'--no-publish to measure this pair against {a.ref} unpublished')
            RM.check_key(a.key_file, a.publish_dry_run)
        meta = RM.read_meta(run)
        path = RM.ref_entry(run, cfg, ys, a.ref)['path']
        RM.register(meta, a.ref, Y.sha12(path), path)
        RM.apply_fields(meta, a.ref, given)
        RM.save_meta(run, meta)
    except RM.RefmatchError as e:
        raise SystemExit(f'{RM.record_name(run)}: {e}')
    if a.no_publish:
        _err(RM.published_off(run, '--no-publish'))
        return 'off'
    return 'post'


def refmatch_after(a, run, row, plan):
    """After `one`'s cells games against an outside reference: the row kept (refmatch_rows.jsonl) and, unless
    --no-publish, the run's record posted -> exit code (3: PUBLISH FAILED, the row kept)."""
    RM.append_hand_row(run, row)
    if plan != 'post':
        return 0
    try:
        RM.publish(run, url=a.publish_url, key_file=a.key_file, dry_run=a.publish_dry_run, log=_err)
    except RM.RefmatchError as e:
        _err(f'PUBLISH FAILED: {RM.record_name(run)}: {e} - the row is kept in {RM.hand_rows_path(run)}; retry: '
             f'python tools/run_yardsticks.py refmatch {run}')
        return S.EXIT_PUBLISH_FAILED
    return 0


def cmd_refmatch(a):
    """The run's reference-match record rebuilt from every row on disk and posted now (docs/ENGINE.md 225)."""
    run = os.path.abspath(a.run)
    try:
        meta = RM.read_meta(run)
        if a.ref:
            if a.ref_ckpt:
                RM.register(meta, a.ref, Y.sha12(a.ref_ckpt), a.ref_ckpt)
            elif a.ref not in meta['refs'] or not meta['refs'][a.ref].get('sha'):
                shas = {str(r.get('ref_sha') or '')[:12] for r in RM.read_rows(RM.row_paths(run, a.rows))
                        if r.get('ref') == a.ref}
                if len(shas) != 1:
                    raise RM.RefmatchError(f'reference {a.ref!r} is not registered and its rows name '
                                           f'{len(shas)} networks ({sorted(shas)}): give --ref-ckpt PATH')
                RM.register(meta, a.ref, shas.pop())
            RM.apply_fields(meta, a.ref, refmatch_fields(a))
        elif any(v not in (None, []) for v in refmatch_fields(a).values()):
            raise RM.RefmatchError('the fields belong to a reference: give --ref LABEL')
        if not meta['refs']:
            raise RM.RefmatchError(f'no outside reference registered for this run ({RM.meta_path(run)}): give --ref LABEL')
        RM.save_meta(run, meta)
        RM.publish(run, extra=a.rows, url=a.publish_url, key_file=a.key_file, dry_run=a.dry_run, log=print)
    except RM.RefmatchError as e:
        _err(f'PUBLISH FAILED: {RM.record_name(run)}: {e}')
        return S.EXIT_PUBLISH_FAILED
    return 0


def _refmatch_args(p):
    """The reference-match record's flags (`one`, `refmatch`): its human fields and the post's options."""
    g = p.add_argument_group("the run's reference-match record on delverai (docs/ENGINE.md 225)")
    g.add_argument('--series-label', default=None, help="the reference's name on the site (default '<run>@<gen>')")
    g.add_argument('--title', default=None, help="the series' title")
    g.add_argument('--note', default=None, help="the series' note")
    g.add_argument('--deck-label', default=None, help="the deck's name (default data/decks/<deck>.json's)")
    g.add_argument('--ref-run', default=None, help="the reference's run (default from its checkpoint's path)")
    g.add_argument('--ref-gen', type=int, default=None, help="the reference's generation (likewise)")
    g.add_argument('--mark', action='append', default=[], metavar='ITER=LABEL', help='a mark on the chart')
    g.add_argument('--row-note', action='append', default=[], metavar='ITER=TEXT', help="a generation's note")
    g.add_argument('--key-file', default=None, help='the upload key (default $MTGZ_DELVERAI_KEY, ~/delverai/.upload_key)')
    g.add_argument('--publish-url', default=None, help=argparse.SUPPRESS)
    return g


def cmd_status(a):
    run = os.path.abspath(a.run)
    cfg = Y.cfg_of(run)
    ys = _ys_of(a, run, cfg)
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
    if st.get('cells'):
        c = st['cells']
        print(f"  cells: {len(c['report']['cells'])} measured against a reference, {len(c['report']['gen'])} "
              f"gen-over-gen (run_yardsticks.py cells RUN)")
        for m in c['missing']:
            print(f"  MISSING {m['kind']} {m.get('iter', '')}" + (f" vs {m['ref']} in {' / '.join(m['pair'])}"
                                                                  if m.get('pair') else ''))
        for f in c['failed']:
            print(f"  FAILING {f['kind']} {f.get('iter', '')}" + (f" vs {f['ref']} in {' / '.join(f['pair'])}"
                                                                  if f.get('pair') else '')
                  + f" ({f['attempts']} attempts): {f['error'][-200:]}")
    return 0


def _short(name):
    return name.split('_')[0][:8]


def print_summary(s, decks, title):
    """The N x N table of one summary: rows the measured side's deck, columns the opponent's; Elo gain +- se."""
    print(title)
    w = 16
    print('  ' + 'pilot / opp'.ljust(12) + ''.join(_short(y).rjust(w) for y in decks) + 'deck'.rjust(w))
    for x in decks:
        line = '  ' + _short(x).ljust(12)
        for y in decks:
            c = s['cells'].get(f'{x}|{y}')
            mark = '*' if c and f'{x}|{y}' in s['promote'] else ' '
            line += (f"{c['elo']:+.0f} +-{c['se']:.0f}{mark}" if c else '-').rjust(w)
        d = s['decks'].get(x)
        line += (f"{d['elo']:+.0f} +-{d['se']:.0f}" if d else '-').rjust(w)
        print(line)
    if s['overall']:
        print(f"  overall {s['overall']['elo']:+.1f} +- {s['overall']['se']:.1f}" +
              (f";  anchored {s['anchored']['elo']:.1f} +- {s['anchored']['se']:.1f}" if s.get('anchored') else ''))
    else:
        print(f"  incomplete: missing {', '.join(s['missing'])}")
    if s['promote']:
        print(f"  * vs_ref above the threshold in {', '.join(s['promote'])}: a new reference is suggested")


def print_fit(fit, decks, only=None):
    """The joint fit's table per measured generation (docs/ENGINE.md 186): each cell's rating over the anchor (Elo
    +- the bootstrap se), the per-deck and overall means, the anchored Elo, the flags."""
    m = fit['method']
    print(f"joint fit (v{fit['fit_v']}): {fit['games']} games, anchor {', '.join(fit['anchor'].get('labels') or [])} "
          f"{fit['anchor'].get('sha')} at {fit['base_elo']:.0f}; se {m['se']}"
          + (f" ({m['boot']} resamples)" if m['boot'] else '') + f"; prior sd {m['prior_sd_elo']} Elo")
    for k, p in sorted(fit['pairs'].items()):
        print(f"  pair {' / '.join(_short(x) for x in k.split(','))}: {p['games']} games on {p['clusters']} deals"
              + (f", matchup m {p['m'][0]:+.0f} +- {p['m'][1]:.0f}" if p.get('m') else '')
              + (f", chooser c {p['c'][0]:+.0f} +- {p['c'][1]:.0f}" if p.get('c') else ''))
    w = 16
    for g in fit['gens']:
        if not g['measured'] or (only is not None and g['iter'] not in only):
            continue
        print(f"generation {g['iter']}" + (f" ({', '.join(g['labels'])})" if g['labels'] else '') + f" {g['sha']}:")
        print('  ' + 'pilot / opp'.ljust(12) + ''.join(_short(y).rjust(w) for y in decks) + 'deck'.rjust(w))
        for x in decks:
            line = '  ' + _short(x).ljust(12)
            for y in decks:
                c = g['cells'].get(f'{x}|{y}')
                line += (f"{c['elo']:+.0f} +-{c['se']:.0f}" if c else '-').rjust(w)
            d = g['decks'].get(x)
            line += (f"{d['elo']:+.0f} +-{d['se']:.0f}" if d else '-').rjust(w)
            print(line)
        if g['overall']:
            print(f"  overall {g['overall']['elo']:+.1f} +- {g['overall']['se']:.1f};  anchored "
                  f"{g['anchored']['elo']:.1f} +- {g['anchored']['se']:.1f}")
        for k, v in sorted(g['flags'].items()):
            print(f"  FLAG {k}: {', '.join(v)}")
    for t in fit['transitivity']:
        if t['flag']:
            print(f"  TRANSITIVITY: iteration {t['iter']} vs {t['prev']} in {t['cell']}: observed {t['obs']:+.0f} +- "
                  f"{t['obs_se']:.0f}, the fit predicts {t['pred']:+.0f} +- {t['pred_se']:.0f} (z {t['z']:+.1f}): "
                  f"exploitation of a reference?")


def cmd_fit(a):
    """The joint fit from the records only (no games): docs/ENGINE.md 186."""
    from mtgz.az import cells_fit as CF
    run = os.path.abspath(a.run)
    cfg = Y.cfg_of(run)
    ys = _ys_of(a, run, cfg)
    if ys is None or not (ys['cells_games'] or ys['cells_gen_games']):
        print('yardsticks: no cells for this run')
        return 0
    fit = CF.fit_run(run, cfg, ys, boot=a.boot)
    if a.json:
        print(json.dumps(fit, indent=1))
    else:
        print_fit(fit, ys['cells_decks'])
    return 0


def cmd_cells(a):
    run = os.path.abspath(a.run)
    cfg = Y.cfg_of(run)
    ys = _ys_of(a, run, cfg)
    if ys is None or not (ys['cells_games'] or ys['cells_gen_games']):
        print('yardsticks: no cells for this run')
        return 0
    rep = Y.cells_report(run, cfg, ys)
    fit = None
    if Y.cells_pool(ys) or a.fit:
        from mtgz.az import cells_fit as CF
        fit = CF.fit_run(run, cfg, ys, boot=a.boot)
    if a.json:
        print(json.dumps(dict(rep, joint=fit) if fit else rep, indent=1))
        return 0
    if Y.cells_pool(ys):
        print(f"reference pool (cells_mode pool; {ys['cells_pool_active']} active + the anchor, promotion every "
              f"{ys['cells_promote_every']} or past {ys['cells_promote']}, auto {ys['cells_auto_promote']}):")
        for r in rep['refs']:
            print(f"  {r.get('label')}: since {r.get('since')}, sha {r.get('sha')}"
                  + (f", Elo {r['elo']:.1f} +- {r['se']:.1f} ({r['how']})" if r.get('elo') is not None else ''))
        print_fit(fit, ys['cells_decks'])
        if rep.get('promote'):
            p = rep['promote']
            print(f"SUGGESTED: a new reference - iteration {p['iter']} ({p['why']}"
                  + (f": {', '.join(p['cells'])} past {ys['cells_promote']} against {p['ref']}" if p['cells'] else '')
                  + f"):  python tools/run_yardsticks.py cells-freeze {a.run} --iter {p['iter']}")
        return 0
    print('reference ladder:')
    for r in rep['refs']:
        print(f"  {r.get('label')}: since {r.get('since')}, sha {r.get('sha')}, Elo "
              + (f"{r['elo']:.1f} +- {r['se']:.1f} ({r['how']})" if r.get('elo') is not None else
                 f"unchained {r.get('error', '')}"))
    for g in rep['cells']:
        print_summary(g['summary'], ys['cells_decks'], f"iteration {g['iter']} against {g['ref']}:")
    for g in rep['gen']:
        print_summary(g['summary'], ys['cells_decks'], f"iteration {g['iter']} against iteration {g['prev']} "
                      f"(gen over gen):")
    if rep['promote']:
        p = rep['promote']
        print(f"SUGGESTED: a new reference - iteration {p['iter']} scores above {ys['cells_promote']} against "
              f"{p['ref']} in {', '.join(p['cells'])}:  python tools/run_yardsticks.py cells-freeze {a.run} "
              f"--iter {p['iter']}")
    if fit is not None:
        print_fit(fit, ys['cells_decks'])
    return 0


def cmd_freeze(a):
    run = os.path.abspath(a.run)
    cfg = Y.cfg_of(run)
    ys = _ys_of(a, run, cfg)
    try:
        e = Y.freeze_ref(run, a.iter, a.label, a.to, cfg, ys)
    except ValueError as ex:
        raise SystemExit(str(ex))
    print(json.dumps(e, indent=1))
    print(f"frozen: {e['label']} (iteration {e['iter']}), Elo {e['elo']} +- {e['elo_se']}; checkpoints from "
          f"{e['since']} on are measured against it ({Y.ladder_path(run)})")
    return 0


def cmd_match(a):
    """Standalone: a checkpoint's cells against a reference, outside any run's watcher - on the farm when it can play
    exactly these games (--backend auto, the default; docs/ENGINE.md 217), else here."""
    from mtgz.az import h2h_farm as HF
    ov = {'bo1_games': 0, 'blunders': False, 'cells_games': a.games, 'cells_refs': ['ref=' + a.ref]}
    ov.update(json.loads(a.yardsticks) if a.yardsticks else {})
    for k, v in (('cells_sims', a.sims), ('cells_device', a.device), ('cells_seed', a.seed)):
        if v is not None:
            ov[k] = v
    if a.decks:
        ov['cells_decks'] = a.decks.split(',')
    if a.reuse_control:
        ov['cells_reuse_base'] = True
    # (an auto campaign's share and deadline: zero.h2h's farm backend's, docs/ENGINE.md 217)
    ov.setdefault('bo1_farm_share', HF.AUTO_SHARE if a.farm_share is None else a.farm_share)
    ys0 = Y.normalize(ov)
    n = Y.pair_jobs(Y.cell_pairs(ys0)[-1], ys0['cells_games'], ys0)[0]['games']     # (a chooser match's games)
    ov.setdefault('bo1_farm_wait_s', a.farm_deadline_h * 3600 if a.farm_deadline_h is not None else
                  min(HF.DEADLINE_MAX_S, HF.DEADLINE_BASE_S + HF.DEADLINE_PER_WORK_S * n * ys0['cells_sims']))
    ys = Y.normalize(ov)
    S.check(a)
    cdir = a.cache or os.path.join(os.path.dirname(os.path.abspath(a.out or a.ckpt)), 'cells_base')
    ck, ref = os.path.abspath(a.ckpt), os.path.abspath(a.ref)
    backend, why, farm = Y.standalone_backend(ck, ref, ys, cdir, a.backend, a.out)
    print(f'cells-match: backend {backend} - {why}', file=sys.stderr, flush=True)
    rows, s = Y.measure_cells_standalone(ck, ref, ys, cdir,
                                         label=a.ref_label, rows_out=a.out,
                                         log=lambda m: print('# ' + m, file=sys.stderr, flush=True),
                                         records=os.path.abspath(a.records) if a.records else None,
                                         backend=backend, farm=farm)
    if a.json:
        print(json.dumps({'summary': s, 'rows': rows, 'backend': backend}, indent=1))
    else:
        print_summary(s, ys['cells_decks'], f'{a.ckpt} against {a.ref}:')
    if a.publish:
        # the sides by their run and generation (never a path); B the reference's label when its file says nothing
        a.a_label = a.a_label or S.checkpoint_label(a.ckpt)
        rl = S.checkpoint_label(a.ref)
        a.b_label = a.b_label or (rl if rl != a.a_label and S.checkpoint_side(a.ref)[1] is not None else a.ref_label)
        name = S.default_key(a.ckpt, a.ref)
        a.publish_key = a.publish_key or name[:44] + '.'     # (the keys' prefix: another measurement's never collide)
        if S.publish_cells({'summary': s, 'rows': rows}, a, name, overall_only=a.publish_overall_only) != 0:
            return S.EXIT_PUBLISH_FAILED
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
    w.add_argument('--take-over', action='store_true', help='stop the run\'s companion watcher and keep its lock '
                   '(by hand; docs/ENGINE.md 186.7)')
    s = sub.add_parser('status')
    s.add_argument('run')
    s.add_argument('--json', action='store_true')
    s.add_argument('--yardsticks', default=None)
    o = sub.add_parser('one')
    o.add_argument('run')
    o.add_argument('--kind', choices=list(Y.KINDS), required=True)
    o.add_argument('--iter', type=int, default=None)
    o.add_argument('--opp', default=None)
    o.add_argument('--ref', default=None)
    o.add_argument('--pair', default=None)
    o.add_argument('--games', type=int, default=None)
    o.add_argument('--role', choices=['main', 'cont', 'gen0'], default=None)
    o.add_argument('--yardsticks', default=None)
    o.add_argument('--no-publish', action='store_true', help="cells against an outside reference: do not post the "
                   "run's reference-match record (said loudly: PUBLISH OFF; the row is kept)")
    o.add_argument('--publish-dry-run', action='store_true', help='print the reference-match record, post nothing')
    _refmatch_args(o)
    rm = sub.add_parser('refmatch', help="rebuild and post the run's reference-match record (docs/ENGINE.md 225)")
    rm.add_argument('run')
    rm.add_argument('--ref', default=None, help='the reference label the fields / --ref-ckpt are for')
    rm.add_argument('--ref-ckpt', default=None, help="the reference's checkpoint (registers its network)")
    rm.add_argument('--rows', action='append', default=[], metavar='FILE', help='more cells rows (JSONL)')
    rm.add_argument('--dry-run', action='store_true', help='print the record, post nothing')
    _refmatch_args(rm)
    c = sub.add_parser('cells', help='the metagame cells\' report (docs/ENGINE.md 177)')
    c.add_argument('run')
    c.add_argument('--json', action='store_true')
    c.add_argument('--yardsticks', default=None)
    c.add_argument('--fit', action='store_true', help='the joint fit\'s table too (always in the pool mode)')
    c.add_argument('--boot', type=int, default=None, help='bootstrap resamples (default cells_fit_boot)')
    cf = sub.add_parser('cells-fit', help='the cells\' joint rating fit from the records only (docs/ENGINE.md 186)')
    cf.add_argument('run')
    cf.add_argument('--json', action='store_true')
    cf.add_argument('--boot', type=int, default=None)
    cf.add_argument('--yardsticks', default=None)
    f = sub.add_parser('cells-freeze', help='freeze a checkpoint as the next cells reference (explicit; never automatic)')
    f.add_argument('run')
    f.add_argument('--iter', type=int, required=True)
    f.add_argument('--label', default=None)
    f.add_argument('--to', default=None, help='where the copy goes (default <run>/yardsticks/cells_refs/<label>.pt)')
    f.add_argument('--yardsticks', default=None)
    m = sub.add_parser('cells-match', help='standalone: a checkpoint\'s cells against a reference, local games')
    m.add_argument('ckpt')
    m.add_argument('--ref', required=True)
    m.add_argument('--ref-label', default='ref')
    m.add_argument('--games', type=int, default=400, help='games per cell (even)')
    m.add_argument('--sims', type=int, default=None)
    m.add_argument('--seed', type=int, default=None)
    m.add_argument('--device', choices=['cpu', 'cuda'], default=None)
    m.add_argument('--decks', default=None, help='comma-separated data/decks names (default the metagame\'s three)')
    m.add_argument('--cache', default=None, help='the baselines\' cache directory (default <out or ckpt dir>/cells_base)')
    m.add_argument('--out', default=None, help='append every row to this JSONL file')
    m.add_argument('--records', default=None, metavar='DIR',
                   help='keep every game played as a game record under DIR/cells/misc/ and DIR/cells_base/misc/ '
                        '(docs/gamedb.md: tools/gamedb.py ingest --records DIR)')
    m.add_argument('--json', action='store_true')
    m.add_argument('--yardsticks', default=None, help='more cells_* options as JSON')
    m.add_argument('--backend', default='auto', choices=['auto', 'farm', 'local'],
                   help="where the games are played (docs/ENGINE.md 217): 'auto' (default) - the farm when it can play "
                        "exactly these games and the comparison's earlier games (its cache, --out) were not played here, "
                        "else here; 'farm' - there or refused; 'local' - here")
    m.add_argument('--reuse-control', action='store_true',
                   help="reuse a cached baseline (each pair's control) played on another backend (cells_reuse_base: "
                        "the opt-out of one backend per comparison; said in the log and the row)")
    m.add_argument('--farm-share', type=float, default=None, help='the campaigns\' share of the farm (default 0.5)')
    m.add_argument('--farm-deadline-h', type=float, default=None,
                   help='hours before the games the farm has not delivered are played here (default 1 h + 0.25 s per '
                        'game-simulation of a chooser match, at most 12 h)')
    S.add_args(m)
    m.add_argument('--publish-overall-only', action='store_true', help='with --publish: the overall match only, no '
                   'match per cell')
    a = ap.parse_args(argv)
    if a.cmd == 'one':
        return cmd_one(a)
    if a.cmd == 'status':
        return cmd_status(a)
    if a.cmd == 'cells':
        return cmd_cells(a)
    if a.cmd == 'cells-fit':
        return cmd_fit(a)
    if a.cmd == 'cells-freeze':
        return cmd_freeze(a)
    if a.cmd == 'cells-match':
        return cmd_match(a)
    if a.cmd == 'refmatch':
        return cmd_refmatch(a)
    if not os.path.exists(os.path.join(a.run, 'config.json')):
        ap.error(f'{a.run}: no config.json (not a run directory)')
    if a.take_over and a.supervised:
        ap.error('--take-over is for a watcher started by hand')
    return Watcher(a.run, a.supervised, a.once, json.loads(a.yardsticks) if a.yardsticks else None,
                   poll=a.poll, take_over=a.take_over).main()


if __name__ == '__main__':
    sys.exit(main())
