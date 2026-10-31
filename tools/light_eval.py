"""CPU evidence for light decisions (ENGINE.md 77.5): self-play games of a checkpoint at a run's self-play
settings, with light decisions off and on (zero-core's selfplay_sims_light ...), each game rebuilt from
its move log and every searched decision classified by the engine (`Game.light_class`, the timing
windows of az/light.rs). Read-only on the run: pass copies of its checkpoint and config.json; nothing is
written under runs/. CPU only (CUDA_VISIBLE_DEVICES=-1), below-normal priority.

  python tools/light_eval.py play --model CKPT --config RUN/config.json --games 96 --seed 1 --out off.json
  python tools/light_eval.py play ... --sims-light 4 --out light4.json            # unrecorded windows at 4
  python tools/light_eval.py play ... --sims-light 16 --sims-light-fast 4 --light-record 1 --out rec16.json
      [--light-classes all] [--sims 64 --sims-fast 16] [--conc 48] [--threads 2] [--torch-threads 2]
  python tools/light_eval.py report off.json[,off2.json ...] light4.json[,...] rec16.json[,...] \
      [--json report.json]

`play` writes every game's result row (with its move log and `light`), the samples (decision rows and
value-only rows), the driver's leaves, simulations and light counters, and the wall time. `report`
takes one argument per variant, its play files comma-separated (the first variant the control: several
seeds of a variant are pooled) and gives per variant: wall time and leaves per game, simulations per
game, recorded samples per game, the searched decisions and the timing windows among them (the
engine's classification of the rebuilt games: the same for every variant, light or not), the light
decisions (the rows: their share, recorded / unrecorded, simulations saved), the act rate in the
windows (the chosen move not a pass) overall, by class and by action, and the behaviour fingerprints
(tools/delver_fingerprints.py), each against the control with its standard error."""
import argparse
import collections
import json
import math
import os
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
sys.path.insert(0, os.path.join(ROOT, 'tools'))

M64 = (1 << 64) - 1
PASS = 0                                           # engine act::PASS: a priority decision's pass


def split64(x):
    z = (x + 0x9E3779B97F4A7C15) & M64
    z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & M64
    z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & M64
    return z ^ (z >> 31)


def first_draw(seed):
    """zero-core's `Rng::new(seed).next_u64()` (xoshiro256++ seeded by splitmix64)."""
    s, x = [], seed
    for _ in range(4):
        x = (x + 0x9E3779B97F4A7C15) & M64
        s.append(split64(x))
    if not any(s):
        s[0] = 1
    r = (s[0] + s[3]) & M64
    return ((((r << 23) | (r >> 41)) & M64) + s[0]) & M64


def game_seed(driver_seed, gid):
    """The game seed zero-core's `init_slot` gives game `gid` of a driver seeded `driver_seed` (not paired)."""
    return first_draw(split64(driver_seed ^ split64(gid + 1)))


# ------------------------------------------------------------------------------------------- play

def play(a):
    os.environ['CUDA_VISIBLE_DEVICES'] = '-1'      # (an empty string does not hide the GPU on Windows)
    import numpy as np
    import torch
    import mtgz.az  # noqa: F401
    from zero import games as G, light as LT
    from zero.infer import NetEvaluator, drive
    from zero.net import load
    from zero.run import Run, below_normal
    below_normal()
    torch.set_num_threads(a.torch_threads)
    with open(a.config, encoding='utf-8') as f:
        cfg = json.load(f)
    for k in ('selfplay_remote', 'eval_remote', 'heldout_shards', 'elo_anchors'):
        cfg[k] = ''
    cfg['infer_device'] = cfg['train_device'] = 'cpu'
    if a.sims:
        cfg['sims'] = a.sims
    if a.sims_fast:
        cfg['sims_fast'] = a.sims_fast
    cfg['selfplay_sims_light'] = a.sims_light
    cfg['selfplay_sims_light_fast'] = a.sims_light_fast
    cfg['selfplay_light_record'] = a.light_record
    cfg['selfplay_light_classes'] = a.light_classes
    cfg['threads'] = a.threads
    scratch = os.path.join(os.path.dirname(os.path.abspath(a.out)), 'run_stub')
    R = Run(scratch, cfg)                          # (attributes only: nothing written)
    seat = R.nn_seat('m', cfg['sims'], selfplay=True)
    m, _ = load(a.model, 'cpu', game=cfg['game'])
    ev = NetEvaluator(m, 'cpu')
    kw = {'aux': True} if R.aux else {}
    drv = G.Driver(cfg['game'], [[seat, seat]] * a.games, spec=cfg['spec'], concurrency=a.conc, seed=a.seed, record=True,
                   threads=a.threads, groups=2, max_steps=cfg['max_steps'], move_log=1_000_000, **kw)
    rows, cnt = [], {'samples': 0, 'decision_samples': 0, 'view_samples': 0}
    t0 = time.time()

    def take(_lb=None):
        rows.extend(drv.take_results())
        s = drv.take_samples()
        if s is not None:
            nopt = np.diff(np.asarray(s['opt_off']))
            cnt['samples'] += len(s['player'])
            cnt['decision_samples'] += int((nopt > 0).sum())
            cnt['view_samples'] += int((nopt == 0).sum())
    drive(drv, {'m': ev}, on_batch=take)
    take()
    secs = time.time() - t0
    st = drv.stats()
    out = {'model': a.model, 'config': a.config, 'seed': a.seed, 'games': a.games, 'sims': cfg['sims'],
           'sims_fast': cfg['sims_fast'], 'p_full': cfg['p_full'], 'sims_minor': cfg.get('sims_minor'),
           'light': R.light, 'seat': seat, 'threads': a.threads, 'torch_threads': a.torch_threads,
           'seconds': round(secs, 1), 'leaves': st['leaves'], 'sims_total': st['sims'],
           'leaves_per_s': round(st['leaves'] / max(secs, 1e-9), 1),
           'stats_light': {k: st[k] for k in ('light', 'light_recorded', 'light_saved', 'minor', 'recorded')},
           **cnt, 'spec': cfg['spec'], 'rows': rows}
    if R.light:
        out['tally'] = LT.summary(LT.tally(rows))
    with open(a.out, 'w', encoding='utf-8') as f:
        json.dump(out, f)
    print(json.dumps({k: v for k, v in out.items() if k not in ('rows', 'spec', 'seat')}), flush=True)


# ------------------------------------------------------------------------------------------- rebuild

def rebuild(spec, driver_seed, row):
    """The game of a result row rebuilt from its move log: a zero-replay/1-like dict (one spectator state
    per step, before it, and the final one; frames {s, p, k, a, c}: c = the searched decision's light
    class by the engine, None if no window) the fingerprint tool reads."""
    import mtgz.az  # noqa: F401
    from zero import games as G
    sp = json.loads(spec)
    sp['log'] = True
    g = G.engine().Game('mtg', json.dumps(sp), 2, game_seed(driver_seed, int(row['gid'])))
    states, frames = [], []

    def step(a, kind):
        c = g.light_class() if kind == 'nn' else None
        states.append(json.loads(g.export_state()))
        frames.append({'s': len(states) - 1, 'p': g.to_move, 'k': kind, 'a': int(a), 'c': c})
        g.step(int(a))

    def auto():
        while g.to_move is not None and len(g.legal()) == 1:
            step(g.legal()[0], 'forced')
    for seat, a, legal in row['moves']:
        auto()
        if g.to_move != seat or list(g.legal()) != list(legal):
            raise SystemExit(f'game {row["gid"]}: the rebuilt game left its move log at frame {len(frames)}')
        step(a, 'nn')
    auto()
    if g.to_move is not None:
        raise SystemExit(f'game {row["gid"]}: the move log ended before the game')
    states.append(json.loads(g.export_state()))
    return {'format': 'zero-replay/1', 'states': states, 'frames': frames, 'result': row['result']}


# ------------------------------------------------------------------------------------------- report

def binom(k, n):
    if not n:
        return None
    r = k / n
    return {'rate': round(r, 4), 'k': k, 'n': n, 'se': round(math.sqrt(r * (1 - r) / n), 4)}


def diff(a, b):
    """Two binom dicts -> their difference (a - b) with its standard error."""
    if not a or not b:
        return None
    return {'diff': round(a['rate'] - b['rate'], 4), 'se': round(math.sqrt(a['se'] ** 2 + b['se'] ** 2), 4)}


def fingerprints(reps):
    from delver_fingerprints import Band, METRICS, scan_game
    B = Band()
    for r in reps:
        scan_game(r, B)
    out = {'games': B.games}
    for label, num, den in METRICS:
        out[label] = binom(B.c[num], B.c[den])
    keeps = collections.defaultdict(lambda: [0, 0])
    for (lands, pos), v in B.mull.items():
        keeps['<=1' if lands <= 1 else '2+'][0] += v[0]
        keeps['<=1' if lands <= 1 else '2+'][1] += v[1]
    out['keep_one_lander'] = binom(*keeps['<=1'])
    out['keep_2plus_landers'] = binom(*keeps['2+'])
    return out


def mean_se(xs):
    n = len(xs)
    if not n:
        return None
    mu = sum(xs) / n
    sd = math.sqrt(sum((x - mu) ** 2 for x in xs) / max(1, n - 1)) if n > 1 else 0.0
    return {'mean': round(mu, 2), 'se': round(sd / math.sqrt(n), 2), 'n': n}


def variant(files):
    plays = [json.load(open(f, encoding='utf-8')) for f in files]
    p0 = plays[0]
    for p in plays[1:]:
        if (p['light'], p['sims'], p['sims_fast'], p['spec']) != (p0['light'], p0['sims'], p0['sims_fast'], p0['spec']):
            raise SystemExit(f'{files}: the play files of one variant differ in their settings')
    games = sum(len(p['rows']) for p in plays)
    g = max(1, games)
    win = collections.Counter()
    act = collections.Counter()
    act_by = collections.defaultdict(lambda: [0, 0])
    actions = collections.Counter()
    searched = 0
    reps = []
    per_game_leaves = []
    turns = []
    for p in plays:
        for r in sorted(p['rows'], key=lambda r: r['gid']):
            rep = rebuild(p['spec'], p['seed'], r)
            reps.append(rep)
            turns.append(r['rounds'])
            for fm in rep['frames']:
                if fm['k'] != 'nn':
                    continue
                searched += 1
                c = fm['c']
                if c is None:
                    continue
                win[c] += 1
                acted = fm['a'] != PASS
                act[c] += acted
                own = 'own' if c.startswith('own_') else 'opp'
                act_by[own][0] += acted
                act_by[own][1] += 1
                if acted:
                    opts = {o['a']: o['t'] for o in (rep['states'][fm['s']].get('decision') or {}).get('options', [])}
                    actions[opts.get(fm['a'], f'action {fm["a"]}')] += 1
    nwin = sum(win.values())
    lt = None
    if p0['light']:
        from zero import light as LT
        acc = None
        for p in plays:
            acc = LT.merge(acc, LT.tally(p['rows']))
        lt = LT.summary(acc)
        lt['decisions_vs_windows'] = f'{acc["decisions"]} light decisions, {nwin} windows of the selected classes or all'
    by_class = {c: {'windows': win[c], 'per_game': round(win[c] / g, 2), 'act': binom(act[c], win[c])}
                for c, _ in win.most_common()}
    return {
        'files': files, 'games': games, 'light': p0['light'], 'sims': p0['sims'], 'sims_fast': p0['sims_fast'],
        'seconds_per_game': round(sum(p['seconds'] for p in plays) / g, 2),
        'leaves_per_game': round(sum(p['leaves'] for p in plays) / g, 1),
        'sims_per_game': round(sum(p['sims_total'] for p in plays) / g, 1),
        'leaves_per_s': [p['leaves_per_s'] for p in plays],
        'samples_per_game': round(sum(p['samples'] for p in plays) / g, 2),
        'decision_samples_per_game': round(sum(p['decision_samples'] for p in plays) / g, 2),
        'view_samples_per_game': round(sum(p['view_samples'] for p in plays) / g, 2),
        'searched_per_game': round(searched / g, 2),
        'windows_per_game': round(nwin / g, 2), 'window_share': round(nwin / max(1, searched), 4),
        'turns': mean_se(turns),
        'light_rows': lt,
        'window_act': binom(sum(act.values()), nwin),
        'window_act_own_turn': binom(*act_by['own']), 'window_act_opp_turn': binom(*act_by['opp']),
        'window_by_class': by_class,
        'window_actions': dict(actions.most_common(14)),
        'fingerprints': fingerprints(reps),
    }


def report(a):
    groups = [[f for f in arg.split(',') if f] for arg in a.files]
    if not groups or not all(groups):
        raise SystemExit('report: one argument per variant, its play files comma-separated')
    vs = [variant(g) for g in groups]
    ctl = vs[0]
    for v in vs[1:]:
        cmp = {'leaves_per_game': round(v['leaves_per_game'] / max(1e-9, ctl['leaves_per_game']) - 1, 4),
               'seconds_per_game': round(v['seconds_per_game'] / max(1e-9, ctl['seconds_per_game']) - 1, 4),
               'samples_per_game': round(v['samples_per_game'] / max(1e-9, ctl['samples_per_game']) - 1, 4),
               'decision_samples_per_game': round(v['decision_samples_per_game'] / max(1e-9, ctl['decision_samples_per_game']) - 1, 4),
               'window_act': diff(v['window_act'], ctl['window_act']),
               'window_act_own_turn': diff(v['window_act_own_turn'], ctl['window_act_own_turn']),
               'window_act_opp_turn': diff(v['window_act_opp_turn'], ctl['window_act_opp_turn']),
               'window_act_by_class': {c: diff(x['act'], ctl['window_by_class'].get(c, {}).get('act'))
                                       for c, x in v['window_by_class'].items() if x['windows'] >= 100},
               'fingerprints': {k: diff(x, ctl['fingerprints'].get(k)) for k, x in v['fingerprints'].items()
                                if isinstance(x, dict)}}
        v['vs_control'] = cmp
    out = {'variants': vs}
    s = json.dumps(out, indent=1)
    print(s)
    if a.json:
        with open(a.json, 'w', encoding='utf-8') as f:
            f.write(s)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    sub = ap.add_subparsers(dest='cmd', required=True)
    p = sub.add_parser('play')
    p.add_argument('--model', required=True)
    p.add_argument('--config', required=True)
    p.add_argument('--games', type=int, default=96)
    p.add_argument('--seed', type=int, default=1)
    p.add_argument('--out', required=True)
    p.add_argument('--sims', type=int, default=0, help='0 = the config\'s')
    p.add_argument('--sims-fast', type=int, default=0, help='0 = the config\'s')
    p.add_argument('--sims-light', type=int, default=0, help='selfplay_sims_light (0 = off)')
    p.add_argument('--sims-light-fast', type=int, default=0)
    p.add_argument('--light-record', type=int, default=0)
    p.add_argument('--light-classes', default='all')
    p.add_argument('--conc', type=int, default=48)
    p.add_argument('--threads', type=int, default=2)
    p.add_argument('--torch-threads', type=int, default=2)
    r = sub.add_parser('report')
    r.add_argument('files', nargs='+')
    r.add_argument('--json', default=None)
    a = ap.parse_args(argv)
    play(a) if a.cmd == 'play' else report(a)


if __name__ == '__main__':
    main()
