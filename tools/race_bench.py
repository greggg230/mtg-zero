"""Throughput of the paired races (docs/ENGINE.md 110.6): today's per-process race against the batched driver,
at 64 simulations on every decision, with self-play's simulation mix and at 16 simulations.

    python tools/race_bench.py --pilot E:/mtg-zero/runs/eoe1/models/iter_0078.pt \\
        --evaluator E:/mtg-zero/s4a/v1_time/evaluator_linear.pt,E:/mtg-zero/s4a/v1_time/evaluator_set.pt \\
        --pools 2026,2027,2028,2029,2030,2031,2032,2033 --device cuda --minutes 10 --out E:/mtg-zero-scratch/race-bench

Two phases, every run a `python -m mtgz.sealed.compare evalpick` of this checkout (logs and JSON in --out):

1. **Network evaluations per game** (`calib-<search>`): per pilot search, one pool's builds play `--calib-games`
   games each to the end in one batched run: rows (network evaluations) / games.
2. **Throughput** (`a`-`d`): each configuration runs the race of `--pools` for `--minutes`, then is stopped; its
   progress lines give the network evaluations per second after `--warmup` minutes (the batched driver's 'rows';
   the per-process runs' evaluator 'leaves', summed over the processes). Games per second = evaluations per
   second / evaluations per game (phase 1): a 64-simulation game of the batched driver lasts longer than the
   window. (a) today's layout: one process per pool (`--procs` at once), `--a-threads` driver threads (the desktop
   race's 3), the default evaluator, 64 simulations; (b) batched, 64 simulations; (c) batched, self-play's mix;
   (d) batched, 16 simulations.

`--configs a,b,c,d` picks some; `--calib 0` skips phase 1 (`--rows-per-game flat64=..,mix=..,flat16=..` instead).
The summary: `<out>/summary.json` and a table on stdout.
"""
import argparse
import json
import os
import statistics
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SEARCH = {'flat64': ['--sims', '64'], 'mix': ['--pilot-search', 'mix'], 'flat16': ['--sims', '16']}
CONFIGS = {'a': ('per-process', 'flat64'), 'b': ('batched', 'flat64'), 'c': ('batched', 'mix'), 'd': ('batched', 'flat16')}


def base(a, pools):
    return [sys.executable, '-m', 'mtgz.sealed.compare', 'evalpick', '--evaluator', a.evaluator,
            '--pool-seed', ','.join(str(p) for p in pools), '--collation', 'arena', '--k', '32', '--worlds', '8',
            '--vs', '3', '--pilot', a.pilot, '--device', a.device, '--progress', str(a.every)]


def batched_flags(a):
    out = ['--batched', '--driver-threads', str(a.driver_threads)]
    if a.cpus:
        out += ['--cpus', str(a.cpus)]
    return out + a.extra


def launch(cmd, log):
    env = dict(os.environ, PYTHONUNBUFFERED='1')
    with open(log, 'w', encoding='utf-8') as f:
        return subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.DEVNULL, stderr=f, env=env)


def lines(log, tag):
    out = []
    try:
        with open(log, encoding='utf-8') as f:
            for line in f:
                if line.startswith(tag + ' '):
                    out.append(json.loads(line[len(tag) + 1:]))
    except OSError:
        pass
    return out


def calibrate(a, search):
    """Rows per game of one pilot search: one pool's builds, --calib-games each, to the end."""
    name = f'calib-{search}'
    js = os.path.join(a.out, name + '.json')
    cmd = base(a, [a.calib_pool]) + SEARCH[search] + batched_flags(a) + [
        '--games', str(a.calib_games), '--block', str(a.calib_games), '--json', js]
    t0 = time.time()
    p = launch(cmd, os.path.join(a.out, name + '.log'))
    p.wait()
    if p.returncode:
        raise SystemExit(f'{name}: exit {p.returncode} (see {name}.log)')
    with open(js, encoding='utf-8') as f:
        st = json.load(f)['batched']
    return {'rows_per_game': round(st['rows'] / st['games'], 1), 'games': st['games'], 'rows': st['rows'],
            'seconds': round(time.time() - t0, 1), 'games_per_s': st['games_per_s']}


def rate(points, warm):
    """(seconds, cumulative count) points -> the count's rate after `warm` seconds (least squares slope)."""
    pts = [(t, c) for t, c in points if t >= warm]
    if len(pts) < 3:
        return None
    mt = statistics.fmean(t for t, _ in pts)
    mc = statistics.fmean(c for _, c in pts)
    num = sum((t - mt) * (c - mc) for t, c in pts)
    den = sum((t - mt) ** 2 for t, _ in pts)
    return num / den if den else None


def throughput(a, cfg):
    kind, search = CONFIGS[cfg]
    window = a.minutes * 60
    if kind == 'batched':
        log = os.path.join(a.out, f'{cfg}.log')
        p = launch(base(a, a.pool_list) + SEARCH[search] + batched_flags(a) + [
            '--race', '--games', '128', '--min-games', '256', '--max-games', '1024', '--delta', '0.03'], log)
        procs, logs = [p], [log]
    else:
        procs, logs, queue = [], [], list(a.pool_list)
        while queue and len(procs) < a.procs:
            ps = queue.pop(0)
            log = os.path.join(a.out, f'{cfg}-{ps}.log')
            extra = ['--cpus', str(a.cpus)] if a.cpus else []
            if a.a_torch_threads:
                extra += ['--torch-threads', str(a.a_torch_threads)]
            procs.append(launch(base(a, [ps]) + SEARCH[search] + extra + [
                '--threads', str(a.a_threads), '--race', '--games', '128', '--min-games', '256', '--max-games', '1024',
                '--delta', '0.03'], log))
            logs.append(log)
    t0 = time.time()
    while time.time() - t0 < window and all(p.poll() is None for p in procs):
        time.sleep(5)
    for p in procs:
        if p.poll() is None:
            p.terminate()
    for p in procs:
        try:
            p.wait(timeout=60)
        except subprocess.TimeoutExpired:
            p.kill()
    warm = a.warmup * 60
    if kind == 'batched':
        pr = lines(logs[0], '# progress')
        rows = rate([(x['seconds'], x['rows']) for x in pr], warm)
        games = rate([(x['seconds'], x['games']) for x in pr], warm)
        last = pr[-1] if pr else {}
        return {'rows_per_s': None if rows is None else round(rows, 1), 'games_finished': last.get('games'),
                'finished_per_s': None if games is None else round(games, 4), 'in_flight': last.get('in_flight'),
                'rows_per_call': last.get('rows_per_call'), 'progress_lines': len(pr)}
    total, n = 0.0, 0
    for log in logs:
        # a process's progress lines carry its evaluator's cumulative leaves and the seconds since its driver began:
        # rebase them on the file's time order (the first line's driver time as 0)
        pts, off, prev = [], 0.0, None
        for x in lines(log, '# progress'):
            if prev is not None and x['seconds'] < prev:
                off += prev
            prev = x['seconds']
            pts.append((off + x['seconds'], x['leaves']))
        r = rate(pts, warm)
        if r is not None:
            total += r
            n += 1
    return {'rows_per_s': round(total, 1) if n else None, 'processes': len(procs), 'measured': n}


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('--pilot', required=True)
    ap.add_argument('--evaluator', required=True)
    ap.add_argument('--pools', default=','.join(str(p) for p in range(2026, 2034)))
    ap.add_argument('--device', default='cuda')
    ap.add_argument('--minutes', type=float, default=10.0)
    ap.add_argument('--warmup', type=float, default=3.0, help='minutes left out of each rate')
    ap.add_argument('--every', type=float, default=20.0, help='seconds between the runs\' progress lines')
    ap.add_argument('--procs', type=int, default=8, help='(a): processes at once (today\'s desktop race: 8)')
    ap.add_argument('--a-threads', type=int, default=3, help='(a): each process\'s driver threads (the desktop race: 3)')
    ap.add_argument('--a-torch-threads', type=int, default=None, help='(a): each process\'s torch threads')
    ap.add_argument('--driver-threads', type=int, default=1)
    ap.add_argument('--cpus', type=int, default=None, help='each run on this many logical CPUs')
    ap.add_argument('--configs', default='a,b,c,d')
    ap.add_argument('--calib', type=int, default=1)
    ap.add_argument('--calib-pool', type=int, default=2026)
    ap.add_argument('--calib-games', type=int, default=16)
    ap.add_argument('--rows-per-game', default='', help='flat64=..,mix=..,flat16=.. (with --calib 0)')
    ap.add_argument('--extra', default='', help='more flags for the batched runs (space-separated)')
    ap.add_argument('--out', required=True)
    a = ap.parse_args(argv)
    a.pool_list = [int(x) for x in a.pools.split(',')]
    a.extra = a.extra.split()
    os.makedirs(a.out, exist_ok=True)
    out = {'settings': {k: v for k, v in vars(a).items() if k != 'extra'}, 'calib': {}, 'configs': {}}
    rpg = {}
    if a.calib:
        for s in SEARCH:
            out['calib'][s] = calibrate(a, s)
            rpg[s] = out['calib'][s]['rows_per_game']
            print(f'# calib {s} ' + json.dumps(out['calib'][s]), flush=True)
    else:
        rpg = {k: float(v) for k, v in (kv.split('=') for kv in a.rows_per_game.split(',') if kv)}
    for cfg in a.configs.split(','):
        r = throughput(a, cfg)
        search = CONFIGS[cfg][1]
        if r.get('rows_per_s') and rpg.get(search):
            r['games_per_s'] = round(r['rows_per_s'] / rpg[search], 4)
        out['configs'][cfg] = dict(r, kind=CONFIGS[cfg][0], search=search)
        print(f'# config {cfg} ' + json.dumps(out['configs'][cfg]), flush=True)
        with open(os.path.join(a.out, 'summary.json'), 'w', encoding='utf-8') as f:
            json.dump(out, f, indent=1)
    base_gps = out['configs'].get('a', {}).get('games_per_s')
    print(f'{"config":<8}{"kind":<13}{"search":<8}{"rows/s":>10}{"games/s":>10}{"x (a)":>8}')
    for cfg, r in out['configs'].items():
        x = r.get('games_per_s')
        print(f'{cfg:<8}{r["kind"]:<13}{r["search"]:<8}{r.get("rows_per_s") or 0:>10.0f}{x or 0:>10.4f}'
              f'{(x / base_gps) if x and base_gps else 0:>8.2f}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
