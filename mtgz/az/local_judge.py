"""A local cells measurement's two chooser matches at once: the yardsticks' cells_local_split (docs/ENGINE.md 199;
mtgz.az.yardsticks.play_pair_local, docs/ENGINE.md 177 / 186; the Mac's way of doing it: mtgz.az.mac_judge, 198).

A cells measurement (a checkpoint against a reference in one deck pair, or a reference's baseline) is two matches, one
per chooser (`yardsticks.pair_jobs`), which `yardsticks.play_pair_local` plays one after the other in the watcher's
child (`run_yardsticks.py one`). With cells_local_split that child plays both at once, each in a process of its own on
this machine (`python -m mtgz.az.local_judge play`: this checkout, the child's interpreter and environment), each the
`zero.h2h.match` call play_pair_local makes - the same checkpoint files, spec, seed, games, sims, seat (unpaired: A in
seat gid % 2), bo1_threads, bo1_groups, cells_device and sparse rows - so the rows, results, statistics, baselines and
the baselines' cache keys (`base_key`) are play_pair_local's. Each process holds its own CUDA context (no CUDA-graph
capture shared between threads of a process) and its own two networks.

  * the protocol is the Mac's (`mac_judge.job_process`): the job one JSON line on the process's stdin, which stays open
    while the match plays (the process ends when it closes: the watcher killed the measurement; on Windows the pipe
    is peeked, never read, while the match plays - `_die_with_stdin`), the result one JSON line on stdout ({rows:
    [{gid, result, over}], unfinished, select_a, a_score, seconds, records}), its stderr in the measurement's console
    (`local c<chooser>: ...`); one match failing stops the other (its process tree);
  * the game records (docs/gamedb.md): with the run's game_records on, each process writes its match as one records
    file straight into the measurement's records directory with the measurement's sink's kind, iteration, labels,
    src, meta (and the pairing and the chooser, as play_pair_local's `with_meta`) and build line - the file
    play_pair_local's match writes there (`local-<pid>-<ms>-<n>.jsonl.gz`); the measurement's sink counts it.

  python -m mtgz.az.local_judge play      (the job JSON line on stdin -> the result JSON line on stdout)
"""
import json
import os
import sys
import threading
import time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
JOB_V = 1


# ------------------------------------------------------------------------------------------------ the measurement's side
def pair_jobs(a, b, pair, games, ys, seat, sink=None):
    """The pair's two chooser matches as jobs: play_pair_local's zero.h2h.match arguments (the checkpoints' absolute
    paths), and with `sink` (the measurement's MatchSink) where and how each match's games are recorded -> [job].
    ValueError for a seat that does not travel as JSON exactly (the process would play another seat)."""
    from zero import gamerec as GR
    from . import yardsticks as Y
    if json.loads(json.dumps(seat)) != seat:
        raise ValueError(f'cells_local_split: the seat {seat!r} does not survive JSON exactly')
    build = (sink.build or GR.build_info()) if sink is not None else None
    out = []
    for j in Y.pair_jobs(pair, games, ys):
        rec = None
        if sink is not None:                    # (play_pair_local's sink.with_meta(pair=, chooser=))
            rec = {'records': os.path.abspath(sink.records), 'kind': sink.kind, 'it': sink.it, 'src': sink.src,
                   'labels': dict(sink.labels), 'meta': dict(sink.meta, pair=list(pair), chooser=j['chooser']),
                   'build': build}
        out.append({'v': JOB_V, 'chooser': j['chooser'], 'a': os.path.abspath(a), 'b': os.path.abspath(b),
                    'games': j['games'], 'sims': ys['cells_sims'], 'seed': j['seed'], 'spec': j['spec'],
                    'seat': dict(seat), 'threads': ys['bo1_threads'], 'groups': ys['bo1_groups'],
                    'device': ys['cells_device'], 'sparse': bool(ys['sparse']), 'records': rec})
    return out


def run_job(job, log=print, procs=None):
    """One chooser match in a process of its own here (`python -m mtgz.az.local_judge play`) -> its result dict.
    `procs`: a list the process is added to (the other match's failure stops it). RuntimeError on a failure."""
    from . import mac_judge as MJ
    return MJ.job_process([sys.executable, '-m', 'mtgz.az.local_judge', 'play'], job, log, procs, tag='local',
                          fail=f"cells_local_split: chooser {job['chooser']}'s match failed", cwd=ROOT)


def _count_records(sink, m, job):
    """The match's records file (written by its process into the sink's directory) counted in the sink."""
    if sink is None:
        return
    r = m.get('records') or {}
    paths = r.get('paths') or []
    d = os.path.normcase(os.path.abspath(sink.dir()))
    if len(paths) != 1 or os.path.normcase(os.path.dirname(os.path.abspath(paths[0]))) != d \
            or not os.path.exists(paths[0]):
        raise RuntimeError(f"cells_local_split: chooser {job['chooser']}: records files {paths} (one expected in "
                           f"{sink.dir()})")
    for k in ('files', 'games', 'bytes'):
        sink.stats[k] += int(r.get(k) or 0)
    sink.stats['seconds'] += float(r.get('seconds') or 0.0)
    sink.paths.extend(paths)


def play_pair_split(a, b, pair, games, ys, seat, log=print):
    """The pair's two matches A (`a`) against B (`b`; the same file for a baseline) at once, each in a process here,
    each `zero.h2h.match` as play_pair_local calls it -> play_pair_local's dict ({'results': {'0', '1': A's result per
    game id}, 'unfinished', 'select'}: the same values). The measurement's game records (zero.gamerec.active(): its
    sink) written by the processes into its directory."""
    from zero import gamerec as GR
    from . import mac_judge as MJ
    if log is print:                           # (the watcher's child: its console is a file - every line at once)
        def log(m):
            print(m, flush=True)
    sink = GR.active()
    jobs = pair_jobs(a, b, pair, games, ys, seat, sink)
    t0 = time.time()
    outs = MJ.run_both(jobs, lambda job, procs: run_job(job, log, procs))
    out = {'results': {}, 'unfinished': 0, 'select': None}
    for job in jobs:                           # (chooser 0, then 1: play_pair_local's order, so 'select' is its)
        m = outs[job['chooser']]
        rows = sorted(m['rows'], key=lambda r: r['gid'])
        if [r['gid'] for r in rows] != list(range(job['games'])):
            raise RuntimeError(f"cells {pair} chooser {job['chooser']}: {len(rows)} games of {job['games']}")
        _count_records(sink, m, job)
        out['results'][str(job['chooser'])] = [float(r['result'][r['gid'] % 2]) for r in rows]
        out['unfinished'] += int(m.get('unfinished', 0))
        out['select'] = m.get('select_a')
        log(f"cells {pair[0]} / {pair[1]} chooser seat {job['chooser']}: {job['games']} games, A "
            f"{m.get('a_score', 0.0):.4f}, {m.get('seconds', 0.0):.0f} s")
    log(f"cells {pair[0]} / {pair[1]}: both chooser matches at once in {time.time() - t0:.0f} s")
    return out


# ------------------------------------------------------------------------------------------------ the match process
def play_job(job):
    """One chooser match of a job (the process's side of play_pair_split; tests call it here) -> the result dict."""
    from zero import gamerec as GR, h2h
    if job.get('v') != JOB_V:
        raise SystemExit(f"local judge: job version {job.get('v')!r}, this checkout reads {JOB_V}")
    r = job.get('records')
    sink = None
    if r:
        sink = GR.MatchSink(r['records'], r['kind'], it=r['it'], src=r['src'], meta=r['meta'], labels=r['labels'],
                            build=r.get('build'))
    with GR.recording(sink):
        m = h2h.match(job['a'], job['b'], int(job['games']), int(job['sims']), seed=int(job['seed']),
                      threads=int(job['threads']), groups=int(job['groups']), device=job['device'], spec=job['spec'],
                      seat_a=dict(job['seat']), seat_b=dict(job['seat']), paired=False, rows=True,
                      sparse=bool(job['sparse']))
    rec = None
    if sink is not None:
        rec = dict(sink.stats, paths=list(sink.paths))
    return {'v': JOB_V, 'rows': [{'gid': int(x['gid']), 'result': [float(v) for v in x['result']],
                                  'over': bool(x.get('over'))} for x in m['rows']],
            'unfinished': int(m.get('unfinished', 0)), 'select_a': m.get('select_a'), 'a_score': m.get('a_score'),
            'seconds': m.get('seconds'), 'records': rec}


def _die_with_stdin(poll=1.0):
    """The measurement keeps this process's stdin open while it waits: its end (killed by the watcher, or gone) ends
    this process, games and all (exit 75). POSIX: mac_judge._die_with_stdin (a raw read until EOF). Windows: a thread
    blocked in a synchronous read of the stdin pipe blocks every other call on that handle, and a DLL's C runtime asks
    the standard handles' type as it starts (GetFileType) - numpy's import, the first thing h2h loads, hung so behind
    it (2026-11-18) - so there the pipe is peeked every `poll` seconds instead (PeekNamedPipe: broken once the
    measurement's end is closed)."""
    from . import mac_judge as MJ
    if os.name != 'nt':
        return MJ._die_with_stdin()
    import _winapi
    import msvcrt
    h = msvcrt.get_osfhandle(0)
    while True:
        try:
            _winapi.PeekNamedPipe(h, 0)
        except OSError:
            os._exit(75)
        time.sleep(poll)


def cmd_play():
    """One job (the JSON line on stdin) -> its result line on stdout, exit 0; a failure: its reason or traceback on
    stderr, exit 1. The process ends (exit 75) when its stdin closes first (_die_with_stdin)."""
    import traceback

    from . import mac_judge as MJ
    line = MJ._read_line()
    if not line.strip():
        raise SystemExit('local judge: no job on stdin')
    job = json.loads(line)
    threading.Thread(target=_die_with_stdin, daemon=True).start()
    code = 1
    try:
        print(json.dumps(play_job(job), separators=(',', ':')), flush=True)
        code = 0
    except SystemExit as e:
        print(e.code if isinstance(e.code, str) else f'local judge: exit {e.code}', file=sys.stderr, flush=True)
    except BaseException:                      # noqa: BLE001 - said on stderr: the measurement raises it
        traceback.print_exc()
    finally:
        MJ._finish(code)


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    if argv == ['play']:
        return cmd_play()
    raise SystemExit('usage: python -m mtgz.az.local_judge play (docs/ENGINE.md 199)')


if __name__ == '__main__':
    sys.exit(main())
