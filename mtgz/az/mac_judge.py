"""The metagame cells' games on the MacBook: the yardsticks' cells_backend 'mac' (docs/ENGINE.md 198;
mtgz.az.yardsticks, docs/ENGINE.md 177 / 186).

A cells measurement (a checkpoint against a reference in one deck pair, or a reference's baseline) is two matches, one
per chooser (`yardsticks.pair_jobs`). With cells_backend 'mac' the watcher's child (`run_yardsticks.py one`) plays both
at once on the Mac, each `zero.h2h.match` exactly as `yardsticks.play_pair_local` calls it - the same checkpoints (their
bytes), spec, seed, games, sims and seat (unpaired: A in seat gid % 2) - so the results, the baselines and their cache
keys are the local backend's, number for number up to the CPU's float rounding (net against net). The watcher's lane
'mac' runs cells_mac_jobs // 2 measurements at once (`yardsticks.lane_parallel`): cells_mac_jobs processes on the Mac.

  * the code: the watcher's own commit (this checkout's HEAD; a checkout with uncommitted changes is refused), played
    by the Mac's checkout of it, ~/mtg-zero-wt/run-<sha12> (`bash tools/mac.sh --eval-checkout` in the watcher's
    checkout: its engine built and stamped there); every process there checks it first (`mac.verify_checkout`);
  * the files: each checkpoint and its run's config.json (zero.h2h reads a side's seat keys - margin_w, redet,
    eval_cache - from <run>/config.json beside <run>/models/) copied to ~/mtgz-judge/<h8>/<run>/models/<file> on the
    Mac (<h8>: a hash of the desktop's run directory; the run's name and models/ kept, so the game records name the
    checkpoint as zero.gamerec.ckpt_id does here); a file already there with the same sha256 is not sent again, a
    copy lands under a temporary name and is renamed once its sha256 is checked;
  * the processes: `ssh <MTGZ_MAC> 'cd <checkout> && OMP_NUM_THREADS=t MTGZ_REMOTE_COMMIT=<sha> exec nice -n N python -m
    mtgz.az.mac_judge play'`, the job as one JSON line on stdin; stdin stays open while the match runs and the Mac's
    process exits when it closes (the watcher killed the child, the connection dropped): no orphan games on the Mac;
    nice cells_mac_nice (10: below the run's self-play workers' 5), cells_mac_threads search threads,
    cells_mac_groups driver groups, cells_mac_torch_threads torch / OMP threads;
  * the game records (docs/gamedb.md): with the run's game_records on, the Mac's process writes the match's games as
    one records file (zero.gamerec.MatchSink: the measurement's kind, iteration, labels and meta - the pairing, the
    chooser, backend 'mac' -, src 'mac', its own build line: the commit, zero-core, the engine id) and sends it back;
    here it is checked (every game id once, each record's result the row's) and written whole into the measurement's
    records directory <run>/records/<kind>/iter_NNNN/ (misc/ for a baseline), where gamedb ingests it.

The Mac's side (`python -m mtgz.az.mac_judge`, run in its checkout):

  play          the job JSON line on stdin -> one result JSON line on stdout ({rows: [{gid, result, over}],
                unfinished, select_a, a_score, seconds, records: base64 of the gzip'd records file or null, load})
  have          {path: sha256 hex | null} on stdin -> the paths missing or different (null: the file must not be there -
                removed if it is)
  put DEST SHA  the file's bytes on stdin -> DEST (a temporary name renamed once its sha256 is SHA)
"""
import base64
import gzip
import hashlib
import itertools
import json
import os
import shlex
import subprocess
import sys
import threading
import time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
JOB_V = 1
WORK = 'mtgz-judge'                            # under the Mac's home: the checkpoints and their runs' configs
SSH_OPTS = ['-o', 'BatchMode=yes', '-o', 'ConnectTimeout=30', '-o', 'ServerAliveInterval=30',
            '-o', 'ServerAliveCountMax=6']
_SEQ = itertools.count(1)
_SHA = {}


# ------------------------------------------------------------------------------------------------ the desktop's side
def remote():
    """The Mac as the cells' player -> {'host', 'home', 'python', 'repo', 'work', 'commit'}: the machine's own settings
    (MTGZ_MAC / MTGZ_MAC_HOME: mtgz.az.MAC_HOST / MAC_HOME, never committed), the Mac's checkout of this checkout's
    commit. RuntimeError when this checkout is not exactly its commit (the Mac plays a commit, never local changes)."""
    from . import MAC_HOME, MAC_HOST, MAC_PYTHON
    from . import mac as M
    head, dirty = M.checkout_state(ROOT)
    if dirty:
        raise RuntimeError(f'cells_backend mac: {ROOT} has uncommitted changes or untracked files, so the Mac cannot '
                           f'play its code: {"; ".join(d.strip() for d in dirty[:5])}')
    moved = M.moved_problem(ROOT, head)
    if moved:
        raise RuntimeError(f'cells_backend mac: {moved}')
    return {'host': MAC_HOST, 'home': MAC_HOME, 'python': MAC_PYTHON, 'repo': M.eval_checkout(head, MAC_HOME),
            'work': f'{MAC_HOME}/{WORK}', 'commit': head}


def remote_path(work, path):
    """A checkpoint's place on the Mac -> (its path, its run directory there, the run directory here): <work>/<h8>/
    <run>/<models>/<file>, <h8> the sha256 of the run directory's path here (8 hex): the run's name and the models/
    step kept (zero.gamerec.ckpt_id, zero.h2h.run_config: <run>/config.json beside <run>/models/)."""
    p = os.path.abspath(path)
    rd = os.path.dirname(os.path.dirname(p))
    h = hashlib.sha256(os.path.normcase(rd).replace('\\', '/').encode('utf-8')).hexdigest()[:8]
    base = f"{work}/{h}/{os.path.basename(rd) or 'root'}"
    return f'{base}/{os.path.basename(os.path.dirname(p))}/{os.path.basename(p)}', base, rd


def file_sha256(path):
    """sha256 (hex) of a file, memoized by path, size and modification time."""
    st = os.stat(path)
    k = (os.path.normcase(os.path.abspath(path)), st.st_size, st.st_mtime_ns)
    if k not in _SHA:
        h = hashlib.sha256()
        with open(path, 'rb') as f:
            for b in iter(lambda: f.read(1 << 20), b''):
                h.update(b)
        _SHA[k] = h.hexdigest()
    return _SHA[k]


def mirror(rt, paths):
    """The checkpoints `paths` and their runs' config.json files -> ({path here: path on the Mac}, [(here or None,
    on the Mac, sha256 or None)] the files the Mac must hold - None: a run without a config.json, whose copy there must
    not exist either)."""
    where, files, seen = {}, [], set()
    for p in paths:
        rp, base, rd = remote_path(rt['work'], p)
        where[p] = rp
        for here, there in ((p, rp), (os.path.join(rd, 'config.json'), f'{base}/config.json')):
            if there in seen:
                continue
            seen.add(there)
            files.append((here, there, file_sha256(here)) if os.path.exists(here) else (None, there, None))
    return where, files


def _ssh(rt, sh):
    return ['ssh', *SSH_OPTS, rt['host'], sh]


def _in_repo(rt, cmd, env=None, nice=None):
    """The shell line running `python -m mtgz.az.mac_judge <cmd>` in the Mac's checkout of the commit."""
    exports = ''.join(f'{k}={shlex.quote(str(v))} ' for k, v in (env or {}).items())
    nn = f'nice -n {int(nice)} ' if nice is not None else ''
    return (f'cd {shlex.quote(rt["repo"])} 2>/dev/null || {{ echo "mac judge: no checkout {rt["repo"]} on the Mac '
            f'(in the watcher checkout at {rt["commit"][:12]}: bash tools/mac.sh --eval-checkout)" >&2; exit 3; }}; '
            f'{exports}exec {nn}{shlex.quote(rt["python"])} -m mtgz.az.mac_judge {cmd}')


def _run(cmd, **kw):
    from mtgz import checkout
    return subprocess.run(cmd, capture_output=True, creationflags=checkout.NO_WINDOW, **kw)


def sync_files(rt, files, log=print):
    """The Mac holds `files` ([(here, there, sha256 | None)]) once this returns: only the missing or different ones
    are sent (each through ssh's stdin to a temporary name, renamed there once its sha256 matches). RuntimeError on a
    failure."""
    want = {there: sha for _, there, sha in files}
    r = _run(_ssh(rt, _in_repo(rt, 'have')), input=json.dumps(want).encode('utf-8'), timeout=300)
    if r.returncode:
        raise RuntimeError(f'cells_backend mac: the Mac\'s files could not be checked (exit {r.returncode}): '
                           f'{r.stderr.decode("utf-8", "replace").strip()[-600:]}')
    missing = set(json.loads(r.stdout.decode('utf-8').strip().splitlines()[-1]))
    for here, there, sha in files:
        if there not in missing or here is None:
            continue
        t0 = time.time()
        with open(here, 'rb') as f:
            r = _run(_ssh(rt, _in_repo(rt, f'put {shlex.quote(there)} {sha}')), stdin=f, timeout=1800)
        if r.returncode:
            raise RuntimeError(f'cells_backend mac: {here} could not be copied to the Mac (exit {r.returncode}): '
                               f'{r.stderr.decode("utf-8", "replace").strip()[-600:]}')
        log(f'mac: copied {os.path.basename(here)} ({os.path.getsize(here) / 1e6:.1f} MB) to the Mac in '
            f'{time.time() - t0:.0f} s')


def run_job(rt, job, ys, log=print, procs=None):
    """One chooser match on the Mac (`python -m mtgz.az.mac_judge play` there) -> its result dict. `procs`: a list
    the ssh process is added to (the other match's failure stops it). RuntimeError on a failure."""
    from mtgz import checkout
    env = {'OMP_NUM_THREADS': int(ys['cells_mac_torch_threads']), 'MTGZ_REMOTE_COMMIT': rt['commit']}
    cmd = _ssh(rt, _in_repo(rt, 'play', env=env, nice=ys['cells_mac_nice']))
    p = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                         creationflags=checkout.NO_WINDOW)
    if procs is not None:
        procs.append(p)
    err = []

    def errors():
        for line in p.stderr:
            s = line.decode('utf-8', 'replace').rstrip()
            err.append(s)
            if s:
                log(f"mac c{job['chooser']}: {s}")
    t = threading.Thread(target=errors, daemon=True)
    t.start()
    try:
        # (stdin stays open while the match runs: the Mac's process ends when it closes - mac_judge play)
        p.stdin.write(json.dumps(job, separators=(',', ':')).encode('utf-8') + b'\n')
        p.stdin.flush()
        out = p.stdout.read()
        code = p.wait()
    finally:
        try:
            p.stdin.close()
        except OSError:
            pass
        t.join(10)
    lines = [x for x in out.decode('utf-8', 'replace').splitlines() if x.strip().startswith('{')]
    if code or not lines:
        raise RuntimeError(f"cells_backend mac: chooser {job['chooser']}'s match failed on the Mac (exit {code}): "
                           + ' | '.join(err[-8:])[-1500:])
    return json.loads(lines[-1])


def _install_records(sink, out, job, rows):
    """The Mac's records file of one match -> written whole into the measurement's records directory (the sink's),
    after checking it: a build line, every game id once, each record's kind the sink's and its result the row's."""
    if sink is None:
        return None
    if not out.get('records'):
        raise RuntimeError(f"cells_backend mac: chooser {job['chooser']}: the Mac sent no game records (game_records "
                           f"is on)")
    raw = base64.b64decode(out['records'])
    lines = [json.loads(x) for x in gzip.decompress(raw).decode('utf-8').splitlines() if x.strip()]
    if not lines or lines[0].get('kind') != 'build':
        raise RuntimeError(f"cells_backend mac: chooser {job['chooser']}: the records file has no build line")
    recs = [x for x in lines[1:] if x.get('kind') not in ('build', 'spec')]
    res = {int(r['gid']): [float(x) for x in r['result']] for r in rows}
    if sorted(int(r['gid']) for r in recs) != list(range(job['games'])) or any(r.get('kind') != sink.kind for r in recs):
        raise RuntimeError(f"cells_backend mac: chooser {job['chooser']}: {len(recs)} records for {job['games']} games "
                           f"(or of another kind than {sink.kind})")
    bad = [r['gid'] for r in recs if [float(x) for x in r['result']] != res[int(r['gid'])]]
    if bad:
        raise RuntimeError(f"cells_backend mac: chooser {job['chooser']}: records disagree with the results in games "
                           f"{bad[:5]}")
    d = sink.dir()
    os.makedirs(d, exist_ok=True)
    path = os.path.join(d, f"mac-{os.getpid()}-{int(time.time() * 1000)}-{next(_SEQ)}-c{job['chooser']}.jsonl.gz")
    tmp = path + f'.{os.getpid()}.tmp'
    with open(tmp, 'wb') as f:
        f.write(raw)
    os.replace(tmp, path)
    sink.stats['files'] += 1
    sink.stats['games'] += len(recs)
    sink.stats['bytes'] += len(raw)
    sink.paths.append(path)
    return path


def play_pair_mac(a, b, pair, games, ys, seat, log=print):
    """The pair's two matches A (`a`) against B (`b`; the same file for a baseline) on the Mac, at once, each
    `zero.h2h.match` as play_pair_local calls it (unpaired, both sides at `seat`) -> play_pair_local's dict + 'mac'
    {chooser: {seconds, load}, 'commit'}. The measurement's game records (zero.gamerec.active(): its sink) written
    here from the Mac's files."""
    from zero import gamerec as GR
    from . import yardsticks as Y
    if log is print:                           # (the watcher's child: its console is a file - every line at once)
        def log(m):
            print(m, flush=True)
    rt = remote()
    where, files = mirror(rt, [a] if os.path.normcase(os.path.abspath(a)) == os.path.normcase(os.path.abspath(b))
                          else [a, b])
    sync_files(rt, files, log)
    sink = GR.active()
    jobs = []
    for j in Y.pair_jobs(pair, games, ys):
        rec = None
        if sink is not None:
            rec = {'kind': sink.kind, 'it': sink.it, 'labels': dict(sink.labels),
                   'meta': dict(sink.meta, pair=list(pair), chooser=j['chooser'], backend='mac')}
        jobs.append({'v': JOB_V, 'commit': rt['commit'], 'chooser': j['chooser'], 'a': where[a], 'b': where[b],
                     'games': j['games'], 'sims': ys['cells_sims'], 'seed': j['seed'], 'spec': j['spec'],
                     'seat': dict(seat), 'threads': ys['cells_mac_threads'], 'groups': ys['cells_mac_groups'],
                     'torch_threads': ys['cells_mac_torch_threads'], 'sparse': bool(ys['sparse']), 'records': rec})
    procs, outs, errs = [], {}, {}
    t0 = time.time()

    def one(job):
        try:
            outs[job['chooser']] = run_job(rt, job, ys, log, procs)
        except Exception as e:                 # noqa: BLE001 - raised below; the other match is stopped
            errs[job['chooser']] = e
            for p in procs:
                try:
                    p.kill()
                except OSError:
                    pass
    ts = [threading.Thread(target=one, args=(job,), daemon=True) for job in jobs]
    for t in ts:
        t.start()
    for t in ts:
        t.join()
    if errs:
        raise next(iter(errs.values()))
    out = {'results': {}, 'unfinished': 0, 'select': None, 'mac': {'commit': rt['commit'][:12]}}
    for job in jobs:
        m = outs[job['chooser']]
        rows = sorted(m['rows'], key=lambda r: r['gid'])
        if [r['gid'] for r in rows] != list(range(job['games'])):
            raise RuntimeError(f"cells {pair} chooser {job['chooser']} (mac): {len(rows)} games of {job['games']}")
        _install_records(sink, m, job, rows)
        out['results'][str(job['chooser'])] = [float(r['result'][r['gid'] % 2]) for r in rows]
        out['unfinished'] += int(m.get('unfinished', 0))
        out['select'] = m.get('select_a')
        out['mac'][str(job['chooser'])] = {'seconds': m.get('seconds'), 'load': m.get('load')}
        log(f"cells {pair[0]} / {pair[1]} chooser seat {job['chooser']} (mac): {job['games']} games, A "
            f"{m.get('a_score', 0.0):.4f}, {m.get('seconds', 0.0):.0f} s, Mac load {m.get('load')}")
    log(f"cells {pair[0]} / {pair[1]} (mac): both chooser matches in {time.time() - t0:.0f} s")
    return out


# ------------------------------------------------------------------------------------------------ the Mac's side
def _load():
    try:
        return [round(x, 1) for x in os.getloadavg()]
    except (OSError, AttributeError):
        return None


def play_job(job):
    """One chooser match of a job (the Mac's side of play_pair_mac; tests call it here) -> the result dict."""
    import shutil
    import tempfile

    import torch

    from zero import gamerec as GR, h2h
    if job.get('v') != JOB_V:
        raise SystemExit(f"mac judge: job version {job.get('v')!r}, this checkout reads {JOB_V}")
    torch.set_num_threads(int(job['torch_threads']))
    sink, tmp = None, None
    r = job.get('records')
    if r:
        tmp = tempfile.mkdtemp(prefix='mtgz-judge-rec-')
        sink = GR.MatchSink(tmp, r['kind'], it=r['it'], src='mac', meta=r['meta'], labels=r['labels'])
    load0 = _load()
    try:
        with GR.recording(sink):
            m = h2h.match(job['a'], job['b'], int(job['games']), int(job['sims']), seed=int(job['seed']),
                          threads=int(job['threads']), groups=int(job['groups']), device='cpu', spec=job['spec'],
                          seat_a=dict(job['seat']), seat_b=dict(job['seat']), paired=False, rows=True,
                          sparse=bool(job['sparse']))
        data = None
        if sink is not None:
            if len(sink.paths) != 1:
                raise RuntimeError(f'mac judge: {len(sink.paths)} records files for one match')
            with open(sink.paths[0], 'rb') as f:
                data = base64.b64encode(f.read()).decode('ascii')
    finally:
        if tmp:
            shutil.rmtree(tmp, ignore_errors=True)
    return {'v': JOB_V, 'rows': [{'gid': int(x['gid']), 'result': [float(v) for v in x['result']],
                                  'over': bool(x.get('over'))} for x in m['rows']],
            'unfinished': int(m.get('unfinished', 0)), 'select_a': m.get('select_a'), 'a_score': m.get('a_score'),
            'seconds': m.get('seconds'), 'records': data, 'load': [load0, _load()]}


def _read_line(fd=0):
    """The job line from stdin's file descriptor itself (no buffered reader: _die_with_stdin reads the rest)."""
    buf = b''
    while not buf.endswith(b'\n'):
        b = os.read(fd, 65536)
        if not b:
            break
        buf += b
    return buf


def _die_with_stdin(fd=0):
    """The desktop keeps this process's stdin open while it waits: EOF (the watcher's child killed, the connection
    gone) ends the process, games and all. Raw reads of the descriptor: a daemon thread blocked in sys.stdin's
    buffered reader makes the interpreter's shutdown abort ('could not acquire lock ... at interpreter shutdown')."""
    try:
        while os.read(fd, 65536):
            pass
    except OSError:
        pass
    os._exit(75)


def _finish(code):
    """Ends the process at once with `code`, its output flushed: no interpreter shutdown while _die_with_stdin's
    thread waits on stdin."""
    for f in (sys.stdout, sys.stderr):
        try:
            f.flush()
        except Exception:                      # noqa: BLE001
            pass
    os._exit(code)


def cmd_play():
    """One job (the JSON line on stdin) -> its result line on stdout, exit 0; a failure: its reason or traceback on
    stderr, exit 1 (the niceness comes from the command line: `nice -n N`)."""
    import traceback
    line = _read_line()
    if not line.strip():
        raise SystemExit('mac judge: no job on stdin')
    job = json.loads(line)
    threading.Thread(target=_die_with_stdin, daemon=True).start()
    code = 1
    try:
        from mtgz.az.mac import verify_checkout
        verify_checkout(ROOT, job.get('commit'))
        print(json.dumps(play_job(job), separators=(',', ':')), flush=True)
        code = 0
    except SystemExit as e:
        print(e.code if isinstance(e.code, str) else f'mac judge: exit {e.code}', file=sys.stderr, flush=True)
    except BaseException:                      # noqa: BLE001 - said on stderr: the desktop raises it
        traceback.print_exc()
    finally:
        _finish(code)


def cmd_have():
    want = json.loads(sys.stdin.read() or '{}')
    out = []
    for path, sha in want.items():
        if sha is None:
            try:
                os.remove(path)
            except FileNotFoundError:
                pass
            continue
        try:
            h = hashlib.sha256()
            with open(path, 'rb') as f:
                for b in iter(lambda: f.read(1 << 20), b''):
                    h.update(b)
            ok = h.hexdigest() == sha
        except OSError:
            ok = False
        if not ok:
            out.append(path)
    print(json.dumps(out), flush=True)
    return 0


def cmd_put(dest, sha):
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    tmp = f'{dest}.{os.getpid()}.tmp'
    h = hashlib.sha256()
    with open(tmp, 'wb') as f:
        for b in iter(lambda: sys.stdin.buffer.read(1 << 20), b''):
            h.update(b)
            f.write(b)
    if h.hexdigest() != sha:
        os.remove(tmp)
        raise SystemExit(f'mac judge: {dest}: sha256 {h.hexdigest()[:16]} of what arrived, {sha[:16]} expected')
    os.replace(tmp, dest)
    return 0


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    if argv[:1] == ['play'] and len(argv) == 1:
        return cmd_play()
    if argv[:1] == ['have'] and len(argv) == 1:
        return cmd_have()
    if argv[:1] == ['put'] and len(argv) == 3:
        return cmd_put(argv[1], argv[2])
    raise SystemExit('usage: python -m mtgz.az.mac_judge play | have | put DEST SHA256 (docs/ENGINE.md 198)')


if __name__ == '__main__':
    sys.exit(main())
