"""CPU evidence for heavy decisions (ENGINE.md 79.5): self-play of a checkpoint at a run's self-play settings
with light / heavy decisions off or on (the compute per game, the heavy share, the probe's cost, the
canaries), and the heavy search against the normal one at the measured Bauble spots and at turn-1 canary
hands. Read-only on the run: pass copies of its checkpoint and config.json. CPU only
(CUDA_VISIBLE_DEVICES=-1), below-normal priority.

  python tools/heavy_eval.py play --model CKPT --config RUN/config.json --games 96 --seed 1 --out off.json \\
      [--sims-light 16 --sims-light-fast 4 --light-record 1] \\
      [--sims-heavy 128 --sims-heavy-fast 28 --chance-k-heavy 4 --root-min-visits-heavy 8 --heavy-prior-max 0.05]
  python tools/heavy_eval.py report off.json light.json heavy.json [--json report.json]
  python tools/heavy_eval.py spots --model CKPT --config RUN/config.json --replays RUN/replays [--spots 64]
      [--t1-seeds 4000 --t1-want 24] [--reps 4] [--out spots.json]

`spots` searches each spot with the run's self-play search (no root noise, no temperature: the search's
own view) under every setting of SETTINGS - the normal search (64 simulations, chance_k 0, no floor) and
heavy ones - and reports per setting: the visit share PUCT gives the Bauble-first options (pruned of the
floor's forced walks), how often they are the visit rule's / the 'cq' rule's move, their Q against the best
other option's (crack / land), over spots x reps.
"""
import argparse
import collections
import glob
import gzip
import json
import math
import os
import re
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
sys.path.insert(0, os.path.join(ROOT, 'tools'))

import heavy_canary as HC  # noqa: E402

# (name, sims, chance_k, root floor per option, stratified own-library tops: zero-core chance_mode_heavy)
SETTINGS = [('normal 64', 64, 0, 0, False), ('heavy 128 k4 f8', 128, 4, 8, False), ('heavy 256 k8 f8', 256, 8, 8, False),
            ('heavy 256 k0 f8', 256, 0, 8, False), ('heavy 128 k4 f0', 128, 4, 0, False), ('normal 64 f8', 64, 0, 8, False),
            ('heavy 128 strat f8', 128, 0, 8, True), ('heavy 256 strat f8', 256, 0, 8, True),
            ('heavy 256 k0 f32', 256, 0, 32, False), ('heavy 256 strat f32', 256, 0, 32, True)]
RULE = ('cq', 4, 50.0, 0.3)


def low_priority(threads):
    os.environ['CUDA_VISIBLE_DEVICES'] = '-1'
    import torch
    torch.set_num_threads(threads)
    try:
        import psutil
        psutil.Process().nice(psutil.BELOW_NORMAL_PRIORITY_CLASS if os.name == 'nt' else 10)
    except Exception:                              # noqa: BLE001
        pass


# ------------------------------------------------------------------------------------------- play

def play(a):
    import numpy as np
    low_priority(a.torch_threads)
    import mtgz.az  # noqa: F401
    from zero import games as G, heavy as HV, light as LT
    from zero.infer import NetEvaluator, drive
    from zero.net import load
    from zero.run import Run
    with open(a.config, encoding='utf-8') as f:
        cfg = json.load(f)
    for k in ('selfplay_remote', 'eval_remote', 'heldout_shards', 'elo_anchors'):
        cfg[k] = ''
    cfg['infer_device'] = cfg['train_device'] = 'cpu'
    cfg.update(selfplay_sims_light=a.sims_light, selfplay_sims_light_fast=a.sims_light_fast,
               selfplay_light_record=a.light_record, selfplay_light_classes=a.light_classes,
               selfplay_sims_heavy=a.sims_heavy, selfplay_sims_heavy_fast=a.sims_heavy_fast,
               selfplay_chance_k_heavy=a.chance_k_heavy, selfplay_root_min_visits_heavy=a.root_min_visits_heavy,
               selfplay_heavy_prior_max=a.heavy_prior_max, selfplay_heavy_depth=a.heavy_depth,
               selfplay_heavy_horizon=a.heavy_horizon, selfplay_heavy_lines=a.heavy_lines, threads=a.threads)
    R = Run(os.path.join(os.path.dirname(os.path.abspath(a.out)), 'run_stub'), cfg)   # (attributes only)
    seat = R.nn_seat('m', cfg['sims'], selfplay=True)
    m, _ = load(a.model, 'cpu', game=cfg['game'])
    ev = NetEvaluator(m, 'cpu')
    kw = {'aux': True} if R.aux else {}
    drv = G.Driver(cfg['game'], [[seat, seat]] * a.games, spec=cfg['spec'], concurrency=a.conc, seed=a.seed, record=True,
                   threads=a.threads, groups=2, max_steps=cfg['max_steps'], move_log=1_000_000, **kw)
    rows, cnt = [], {'samples': 0, 'decision_samples': 0}
    t0 = time.time()

    def take(_lb=None):
        rows.extend(drv.take_results())
        s = drv.take_samples()
        if s is not None:
            nopt = np.diff(np.asarray(s['opt_off']))
            cnt['samples'] += len(s['player'])
            cnt['decision_samples'] += int((nopt > 0).sum())
    drive(drv, {'m': ev}, on_batch=take)
    take()
    st = drv.stats()
    out = {'model': a.model, 'config': a.config, 'seed': a.seed, 'games': a.games, 'seat': seat, 'light': R.light,
           'heavy': R.heavy, 'seconds': round(time.time() - t0, 1), 'leaves': st['leaves'], 'sims_total': st['sims'],
           'busy_s': st['busy_s'], 'stats': {k: st[k] for k in st if k.startswith(('heavy', 'light')) or k in ('minor', 'recorded')},
           **cnt, 'spec': cfg['spec'], 'rows': rows}
    if R.light:
        out['light_tally'] = LT.summary(LT.tally(rows))
    if R.heavy:
        out['heavy_tally'] = HV.summary(HV.tally(rows))
    with open(a.out, 'w', encoding='utf-8') as f:
        json.dump(out, f)
    print(json.dumps({k: v for k, v in out.items() if k not in ('rows', 'spec', 'seat')}), flush=True)


def report(a):
    out = {}
    for path in a.files:
        d = json.load(open(path, encoding='utf-8'))
        g = max(1, len(d['rows']))
        dec = sum(r['decisions'] for r in d['rows'])
        recs = [HC.game_records(*s) for s in HC.sources_from_play(path)]
        st = d['stats']
        out[os.path.basename(path)] = {
            'games': len(d['rows']), 'seconds_per_game': round(d['seconds'] / g, 2),
            'sims_per_game': round(d['sims_total'] / g, 1), 'leaves_per_game': round(d['leaves'] / g, 1),
            'searched_per_game': round(dec / g, 1), 'samples_per_game': round(d['samples'] / g, 1),
            'light': d.get('light_tally'), 'heavy': d.get('heavy_tally'),
            'probe_share_of_busy': round(st.get('heavy_probe_s', 0) / max(1e-9, d.get('busy_s', 0)), 4),
            'canary': HC.canary(recs)}
    print(json.dumps(out, indent=1))
    if a.json:
        with open(a.json, 'w', encoding='utf-8') as f:
            json.dump(out, f, indent=1)


# ------------------------------------------------------------------------------------------- spots

def evaluate(ev, rows, acts, belief=False):
    import numpy as np
    rows = np.stack([np.asarray(r, np.float32) for r in rows])
    n = len(acts)
    off = np.zeros(n + 1 + (1 if belief else 0), np.int32)
    off[1:n + 1] = np.cumsum([len(x) for x in acts])
    if belief:
        off[-1] = off[n]
        rows = np.concatenate([rows, rows[-1:]], 0)
    extra = np.zeros(len(rows), np.uint8)
    if belief:
        extra[-1] = 2
    flat = np.concatenate([np.asarray(x, np.int32) for x in acts]) if n else np.zeros(0, np.int32)
    return ev.evaluate(rows, np.full(len(rows), 2, np.int32), off, flat, extra if belief else None)


def search(games, ev, sims, seeds, keys, c_puct, preds, chance_k, floor, strat=False):
    """The position lab's lockstep search (one Rust tree per game, the self-play seat's leaf view and root
    belief) with a chance_k and a root floor per setting (the heavy floor's clip: at most half the budget)."""
    import numpy as np
    from zero.games import engine
    E = engine()
    rv = keys.get('leaf_value_view', 'mover') == 'root'
    trees = []
    for g, s, pr in zip(games, seeds, preds):
        t = g.search_tree(True, int(s))
        k = E.heavy_floor(int(floor), len(g.legal()), int(sims)) if floor else 0
        if k:
            t.set_root_min(k)
        if strat:
            t.set_stratified(True)
        if keys.get('belief_tilt'):
            t.set_belief(np.asarray(pr, np.float32), g, float(keys['belief_tilt']), float(keys['belief_clip']),
                         float(keys['belief_mix']))
        trees.append(t)
    for _ in range(sims):
        rows, acts, owners = [], [], []
        for j, (g, t) in enumerate(zip(games, trees)):
            r = t.simulate(g, c_puct, chance_k=chance_k, root_view=rv)
            if r is None:
                continue
            rows.append(r[1])
            acts.append(list(r[2]))
            vx = r[3] if rv else None
            owners.append((j, len(rows) - 1, None if vx is None else len(rows)))
            if vx is not None:
                rows.append(vx)
                acts.append([])
        if not rows:
            continue
        pri, val, mar = evaluate(ev, rows, acts)[:3]
        off = np.zeros(len(acts) + 1, np.int64)
        off[1:] = np.cumsum([len(x) for x in acts])
        for j, b, vb in owners:
            v = b if vb is None else vb
            trees[j].expand([float(x) for x in pri[off[b]:off[b + 1]]], [float(x) for x in val[v]], [float(x) for x in mar[v]])
    out = []
    for t in trees:
        nv, w = list(t.root_visits), list(t.root_values)
        pruned = list(t.root_visits_pruned) if t.root_min else nv
        out.append({'acts': list(t.root_acts), 'n': nv, 'pruned': pruned, 'q': [w[i] / nv[i] if nv[i] else None for i in range(len(nv))],
                    'p0': list(t.root_priors0), 'rule': t.root_select(*RULE)})
    return out


def is_bauble(t):
    return t in ('Cast ' + HC.BAUBLE, 'Activate ' + HC.BAUBLE)


def bauble_spots(replays, limit):
    """measure-play's spots: a fetch cracked with an empty stack while a Mishra's Bauble was castable or
    activatable (the eval replays, rebuilt) -> [(game clone, decider, tag)]."""
    E = HC.engine()
    out = []
    for f in sorted(glob.glob(os.path.join(replays, '*.json.gz'))):
        with gzip.open(f, 'rt', encoding='utf-8') as fh:
            rep = json.load(fh)
        g = E.Game('mtg', rep['spec'], 2, HC.replay_seed(rep))
        for fm in rep['frames']:
            p, a = fm['p'], fm['a']
            while not g.over and len(g.legal()) == 1:
                g.step(g.legal()[0])
            if g.over or g.to_move != p:
                break
            s, lab = HC.state(g)
            if HC.is_crack(lab.get(a, '')) and not s.get('stack') and any(is_bauble(t) for t in lab.values()):
                out.append((g.clone(), p, f'{os.path.basename(f)}:{s["turn"]}'))
                if len(out) >= limit:
                    return out
            g.step(int(a))
    return out


def opening_spot(spec, seed):
    """The starting player's first main-phase priority of game `seed` (the chooser plays first, both keep,
    priority passes until then) -> (game, decider, hand) or None (the position lab's opening spot)."""
    E = HC.engine()
    g = E.Game('mtg', json.dumps(spec) if not isinstance(spec, str) else spec, 2, int(seed))
    for _ in range(40):
        if g.over:
            return None
        while not g.over and len(g.legal()) == 1:
            g.step(g.legal()[0])
        st, lab = HC.state(g)
        d = st['decision']
        if st['turn'] > 1:
            return None
        if d['kind'] == 'Priority' and st['step'] == 'main1' and st['active'] == g.to_move:
            return g, g.to_move, sorted(c['name'] for c in st['players'][g.to_move]['hand'])
        want = {'PlayDraw': 'Play first', 'Mulligan': 'Keep', 'Priority': 'Pass priority'}.get(d['kind'])
        act = next((x for x, t in lab.items() if t == want), None)
        if act is None:
            return None
        g.step(int(act))
    return None


def spots(a):
    low_priority(a.torch_threads)
    import numpy as np
    import mtgz.az  # noqa: F401  (puts the pinned zero-core on sys.path)
    from zero.infer import NetEvaluator
    from zero.net import load
    with open(a.config, encoding='utf-8') as f:
        cfg = json.load(f)
    keys = {'leaf_value_view': cfg.get('selfplay_leaf_value_view', 'mover'), 'belief_tilt': cfg.get('selfplay_belief_tilt', 0),
            'belief_clip': cfg.get('belief_clip', 20.0), 'belief_mix': cfg.get('belief_mix', 0.0)}
    c_puct = float(cfg.get('c_puct', 1.0))
    m, _ = load(a.model, 'cpu', game='mtg')
    ev = NetEvaluator(m, 'cpu')
    sp = [(g, p, tag, 'bauble') for g, p, tag in bauble_spots(a.replays, a.spots)]
    spec = json.loads(cfg['spec'])
    n_t1 = 0
    for seed in range(a.t1_seeds):
        if n_t1 >= a.t1_want:
            break
        r = opening_spot(spec, seed)
        if r and HC.t1_composition(r[2]) in (('clean',) if a.t1_clean else ('clean', 'any')):
            sp.append((r[0], r[1], f'seed{seed}:{HC.t1_composition(r[2])}', 't1'))
            n_t1 += 1
    print('spots', collections.Counter(s[3] for s in sp), flush=True)
    preds = []
    for g, p, _, _ in sp:
        if keys['belief_tilt']:
            preds.append(np.asarray(evaluate(ev, [g.encode(p)], [list(g.legal())], belief=True)[3][0], np.float32))
        else:
            preds.append(None)
    res = collections.defaultdict(list)
    t0 = time.time()
    todo = [x for x in SETTINGS if not a.settings or x[0] in a.settings.split(',')]
    for name, sims, ck, fl, strat in todo:
        for rep in range(a.reps):
            out = search([s[0] for s in sp], ev, sims, [1000 * rep + 17 + 31 * i for i in range(len(sp))], keys, c_puct, preds, ck,
                         fl, strat)
            for (g, p, tag, kind), o in zip(sp, out):
                _, lab = HC.state(g)
                names = [lab.get(x, '') for x in o['acts']]
                bi = [i for i, t in enumerate(names) if is_bauble(t)]
                oi = [i for i in range(len(names)) if i not in bi]
                tot = max(1, sum(o['pruned']))
                qb = [o['q'][i] for i in bi if o['q'][i] is not None]
                qo = [o['q'][i] for i in oi if o['q'][i] is not None]
                best_o = max(oi, key=lambda i: (o['pruned'][i], o['p0'][i])) if oi else None
                vis = max(range(len(names)), key=lambda i: (o['pruned'][i], o['p0'][i]))
                res[(name, kind)].append({'tag': tag, 'rep': rep, 'p_bauble': sum(o['p0'][i] for i in bi),
                                          'share': sum(o['pruned'][i] for i in bi) / tot, 'visit_move': vis in bi,
                                          'rule_move': o['rule'] in bi, 'q_bauble': max(qb) if qb else None,
                                          'q_other': max(qo) if qo else None, 'other': names[best_o] if best_o is not None else None,
                                          'visits_bauble': sum(o['n'][i] for i in bi)})
        print('setting', name, round(time.time() - t0, 1), 's', flush=True)

    def mean(xs):
        xs = [x for x in xs if x is not None]
        if not xs:
            return None
        mu = sum(xs) / len(xs)
        se = math.sqrt(sum((x - mu) ** 2 for x in xs) / max(1, len(xs) - 1) / len(xs)) if len(xs) > 1 else 0.0
        return {'mean': round(mu, 4), 'se': round(se, 4), 'n': len(xs)}
    table = {}
    for (name, kind), rows in res.items():
        dq = [r['q_bauble'] - r['q_other'] for r in rows if r['q_bauble'] is not None and r['q_other'] is not None]
        table[f'{kind} | {name}'] = {'spots_x_reps': len(rows), 'prior_bauble': mean([r['p_bauble'] for r in rows]),
                                     'bauble_visit_share': mean([r['share'] for r in rows]),
                                     'bauble_visits': mean([r['visits_bauble'] for r in rows]),
                                     'visit_move_bauble': mean([float(r['visit_move']) for r in rows]),
                                     'cq_move_bauble': mean([float(r['rule_move']) for r in rows]),
                                     'q_bauble_minus_best_other': mean(dq), 'q_bauble': mean([r['q_bauble'] for r in rows]),
                                     'q_other': mean([r['q_other'] for r in rows]),
                                     'others': dict(collections.Counter(r['other'] for r in rows).most_common(4))}
    out = {'model': a.model, 'settings': todo, 'spots': collections.Counter(s[3] for s in sp), 'table': table,
           'rows': {f'{k[1]} | {k[0]}': v for k, v in res.items()}}
    with open(a.out, 'w', encoding='utf-8') as f:
        json.dump(out, f, indent=1)
    for k, v in sorted(table.items()):
        print(k, json.dumps({x: v[x] for x in ('bauble_visit_share', 'visit_move_bauble', 'cq_move_bauble', 'q_bauble_minus_best_other')}))


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest='cmd', required=True)
    p = sub.add_parser('play')
    p.add_argument('--model', required=True)
    p.add_argument('--config', required=True)
    p.add_argument('--games', type=int, default=96)
    p.add_argument('--seed', type=int, default=1)
    p.add_argument('--conc', type=int, default=48)
    p.add_argument('--threads', type=int, default=2)
    p.add_argument('--torch-threads', type=int, default=2)
    p.add_argument('--sims-light', type=int, default=0)
    p.add_argument('--sims-light-fast', type=int, default=0)
    p.add_argument('--light-record', type=int, default=0)
    p.add_argument('--light-classes', default='all')
    p.add_argument('--sims-heavy', type=int, default=0)
    p.add_argument('--sims-heavy-fast', type=int, default=0)
    p.add_argument('--chance-k-heavy', type=int, default=-1)
    p.add_argument('--root-min-visits-heavy', type=int, default=0)
    p.add_argument('--heavy-prior-max', type=float, default=1.0)
    p.add_argument('--heavy-depth', type=int, default=2)
    p.add_argument('--heavy-horizon', type=int, default=12)
    p.add_argument('--heavy-lines', type=int, default=64)
    p.add_argument('--out', required=True)
    p = sub.add_parser('report')
    p.add_argument('files', nargs='+')
    p.add_argument('--json', default=None)
    p = sub.add_parser('spots')
    p.add_argument('--model', required=True)
    p.add_argument('--config', required=True)
    p.add_argument('--replays', required=True)
    p.add_argument('--spots', type=int, default=64)
    p.add_argument('--t1-seeds', type=int, default=4000)
    p.add_argument('--t1-want', type=int, default=24)
    p.add_argument('--t1-clean', type=int, default=1)
    p.add_argument('--reps', type=int, default=4)
    p.add_argument('--settings', default='', help='comma-separated SETTINGS names (default: all)')
    p.add_argument('--torch-threads', type=int, default=4)
    p.add_argument('--out', required=True)
    a = ap.parse_args(argv)
    {'play': play, 'report': report, 'spots': spots}[a.cmd](a)


if __name__ == '__main__':
    main()
