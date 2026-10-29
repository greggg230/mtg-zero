"""The root minimum visits at the Delver mirror's mulligan (docs/ENGINE.md 64, zero-core docs/search.md
3.5): opening seven-card hands, each searched by the Rust tree with a checkpoint's network at 64
simulations with the floor k = 0 (off: today's search) and k = 4 / 8, and what each move rule and
target makes of it.

  python tools/mull_floor.py --model COPY_OF.pt [--zero 50 --one 100 --control 50 --sims 64 --ks 0,4,8
      --reps 2 --threads 4 --out mull_floor.json]

Positions: games of the mirror's spec from fixed seeds; the PlayDraw choice at random, then the starting
player's mulligan declaration and (after it keeps) the other player's - both seven-card hands. `--zero`
hands without a land, `--one` with one, `--control` with 2-4. Per position and k, `--reps` searches (seeds): no root noise
(an evaluation's search; self-play adds Dirichlet noise to its full searches). Reported per land group:
the prior and the network's value of the hand, Mulligan's visits (forced and PUCT's own) and Q, the
completed-Q target's mass on Mulligan (cq_c_scale 0.3, the Delver line's target; 1.0), and the keep rate
of the 'visits' rule (argmax of the counts without the forced walks), of temperature sampling (by those
counts), and of the 'cq' rule with guard 4 at scale 0.3 (the Delver line's evaluation rule) and 1.0.
`--small N`: also N decisions of 2-4 options (mana payments left out) from games played by the
network's sampled prior, and how often the floor changes anything there (an option without a visit
at k 0; the cq target moved by > 0.1; the cq4 s0.3 move changed; a forced option's Q above the most
visited one's). CPU only; the model is only read.
"""
import argparse
import json
import os
import random
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)

import mtgz.az as A                    # noqa: E402  (puts the pinned zero-core on sys.path)
import numpy as np                     # noqa: E402

LANDS = {'Flooded Strand', 'Island', 'Misty Rainforest', 'Polluted Delta', 'Scalding Tarn', 'Thundering Falls',
         'Volcanic Island', 'Wasteland'}
KEEP, MULL = 0, 1


def hands(spec, zero, one, control, seed=0):
    """-> [(game, lands, order)] seven-card mulligan declarations (order 0: the starting player's, 1: the
    other's): `zero` without a land, `one` with one, `control` with 2-4."""
    from zero.games import engine
    E = engine()
    rng = random.Random(seed)
    want = {0: zero, 1: one, 2: control}
    got = {0: [], 1: [], 2: []}
    s = 0
    while any(len(got[b]) < want[b] for b in got):
        g = E.Game('mtg', spec, 2, 1_000 + s)
        s += 1
        g.step(rng.choice(g.legal()))                     # PlayDraw
        for order in range(2):
            st = json.loads(g.export_state())
            d = st['decision']
            if d['kind'] != 'Mulligan' or g.legal() != [KEEP, MULL]:
                break
            me = st['players'][g.to_move]
            if me['mulligans'] != 0 or len(me['hand']) != 7:
                break
            n = sum(c['name'] in LANDS for c in me['hand'])
            b = min(n, 2)
            if n <= 4 and len(got[b]) < want[b]:
                got[b].append((g.clone(), n, order))
            g.step(KEEP)
    return got[0] + got[1] + got[2]


def search_all(items, ev, sims, ks, reps):
    """Every (position, k, rep) searched in lockstep (one network batch per simulation step)."""
    trees = []
    for i, (g, _, _) in enumerate(items):
        for k in ks:
            for r in range(reps):
                t = g.search_tree(True, 7919 * i + 104729 * r + 1)
                t.set_root_min(k)
                trees.append((i, k, r, t))
    for _ in range(sims):
        pend = []
        for j, (i, _, _, t) in enumerate(trees):
            leaf = t.simulate(items[i][0], 1.0)
            if leaf is not None:
                pend.append((j, leaf))
        if not pend:
            continue
        x = np.stack([lf[1] for _, lf in pend]).astype(np.float32)
        acts = [np.asarray(lf[2], np.int32) for _, lf in pend]
        off = np.zeros(len(pend) + 1, np.int32)
        off[1:] = np.cumsum([len(a) for a in acts])
        pri, val, mar = ev.evaluate(x, np.full(len(pend), 2, np.int32), off,
                                    np.concatenate(acts) if acts else np.zeros(0, np.int32))
        for b, (j, _) in enumerate(pend):
            trees[j][3].expand([float(p) for p in pri[off[b]:off[b + 1]]], [float(v) for v in val[b]],
                               [float(m) for m in mar[b]])
    return trees


def summarize(items, trees):
    from zero import mcts
    rows = []
    for i, k, r, t in trees:
        nv, w, p0, v0 = t.root_visits, t.root_values, t.root_priors0, t.root_value0
        pr = t.root_visits_pruned
        forced = t.root_forced or [0, 0]
        q = [w[a] / nv[a] if nv[a] else None for a in (KEEP, MULL)]
        rows.append({
            'pos': i, 'lands': items[i][1], 'order': items[i][2], 'k': k, 'rep': r, 'p_mull': p0[MULL], 'v0': v0,
            'n': list(nv), 'forced': list(forced), 'pruned': list(pr), 'q_keep': q[0], 'q_mull': q[1],
            'cq03_mull': t.root_cq_target(50.0, 0.3)[MULL], 'cq10_mull': t.root_cq_target(50.0, 1.0)[MULL],
            # the moves: 'visits' = argmax (N, P0) of the counts without the forced walks (the driver's)
            'visits': mcts.select_index(p0, pr, w, v0, 'visits'),
            'sample_mull': pr[MULL] / sum(pr) if sum(pr) else 0.0,
            'cq4_03': t.root_select('cq', 4, 50.0, 0.3), 'cq4_10': t.root_select('cq', 4, 50.0, 1.0),
        })
    return rows


def table(rows, ks):
    groups = [('0 lands', lambda L: L == 0), ('1 land', lambda L: L == 1), ('2-4 lands', lambda L: 2 <= L <= 4)]
    out = {}
    for name, f in groups:
        for k in ks:
            rs = [x for x in rows if f(x['lands']) and x['k'] == k]
            if not rs:
                continue
            qd = [x['q_mull'] - x['q_keep'] for x in rs if x['q_mull'] is not None and x['q_keep'] is not None]
            keep = lambda key: 1 - float(np.mean([x[key] == MULL for x in rs]))        # noqa: E731
            out[f'{name} k{k}'] = {
                'searches': len(rs), 'positions': len({x['pos'] for x in rs}),
                'prior_mull': float(np.mean([x['p_mull'] for x in rs])), 'value': float(np.mean([x['v0'] for x in rs])),
                'n_mull': float(np.mean([x['n'][MULL] for x in rs])), 'forced_mull': float(np.mean([x['forced'][MULL] for x in rs])),
                'puct_mull': float(np.mean([x['pruned'][MULL] for x in rs])),
                'q_keep': float(np.mean([x['q_keep'] for x in rs if x['q_keep'] is not None])),
                'q_mull': float(np.mean([x['q_mull'] for x in rs if x['q_mull'] is not None])) if qd else None,
                'q_gap': float(np.mean(qd)) if qd else None,
                'q_mull_better': float(np.mean([d > 0 for d in qd])) if qd else None,
                'cq03_mull': float(np.mean([x['cq03_mull'] for x in rs])),
                'cq10_mull': float(np.mean([x['cq10_mull'] for x in rs])),
                'keep_visits': keep('visits'), 'keep_sampled': 1 - float(np.mean([x['sample_mull'] for x in rs])),
                'keep_cq4_03': keep('cq4_03'), 'keep_cq4_10': keep('cq4_10'),
            }
    return out


def small_positions(spec, ev, n, games=48, every=6, seed=1):
    """`n` decisions of 2-4 options (mana payments - minor decisions, never recorded - left out) from
    games played by the network's prior (sampled), one in `every` of them, with their kind."""
    from zero.games import engine
    E = engine()
    rng = np.random.default_rng(seed)
    gs = [E.Game('mtg', spec, 2, 50_000 + i) for i in range(games)]
    out = []
    seen = 0
    small_positions.counts = counts = {'decisions': 0, 'minor': 0, 'small': 0}
    while len(out) < n and any(not g.over for g in gs):
        batch = []
        for g in gs:
            while not g.over and len(g.legal()) == 1:
                g.step(g.legal()[0])
            if not g.over:
                batch.append(g)
        if not batch:
            break
        x = np.stack([np.asarray(g.encode(g.to_move), np.float32) for g in batch])
        acts = [np.asarray(g.legal(), np.int32) for g in batch]
        off = np.zeros(len(batch) + 1, np.int32)
        off[1:] = np.cumsum([len(a) for a in acts])
        pri, _, _ = ev.evaluate(x, np.full(len(batch), 2, np.int32), off, np.concatenate(acts))
        for b, g in enumerate(batch):
            m = len(acts[b])
            counts['decisions'] += 1
            kind = json.loads(g.export_state())['decision']['kind']
            counts['minor'] += kind in ('PayMana', 'PayPool')
            if 2 <= m <= 4:
                if kind not in ('PayMana', 'PayPool'):
                    counts['small'] += 1
                    seen += 1
                    if seen % every == 0 and len(out) < n:
                        out.append((g.clone(), kind, m))
            p = np.asarray(pri[off[b]:off[b + 1]], np.float64)
            g.step(int(acts[b][rng.choice(m, p=p / p.sum())]))
    return out


def small_table(items, trees, ks):
    """Per k: the share of searches with an option left without a visit at k 0, where the floor moved
    the cq target (s0.3) by > 0.1 total variation, where it changed the cq4 s0.3 rule's move, and
    where a forced option's Q beat the most visited one's."""
    by = {}
    for i, k, r, t in trees:
        by[(i, k)] = t
    out = {'positions': len(items), 'kinds': {}}
    for _, kind, _ in items:
        out['kinds'][kind] = out['kinds'].get(kind, 0) + 1
    for k in ks:
        if k == 0:
            continue
        starved = tvs = moved = better = forced = 0
        for i in range(len(items)):
            t0, tk = by[(i, 0)], by[(i, k)]
            n0 = t0.root_visits
            starved += min(n0) == 0
            c0, ck = np.asarray(t0.root_cq_target(50.0, 0.3)), np.asarray(tk.root_cq_target(50.0, 0.3))
            tvs += 0.5 * float(np.abs(c0 - ck).sum()) > 0.1
            moved += t0.root_select('cq', 4, 50.0, 0.3) != tk.root_select('cq', 4, 50.0, 0.3)
            nv, w, f = tk.root_visits, tk.root_values, tk.root_forced or [0] * len(tk.root_visits)
            forced += sum(f)
            best = int(np.argmax(nv))
            qb = w[best] / nv[best]
            better += any(f[j] and nv[j] and w[j] / nv[j] > qb for j in range(len(nv)))
        out[f'k{k}'] = {'starved_k0': starved / len(items), 'cq_target_moved': tvs / len(items),
                        'cq4_03_move_changed': moved / len(items), 'forced_beats_best': better / len(items),
                        'forced_walks': forced / len(items)}
    out['decisions'] = dict(getattr(small_positions, 'counts', {}))
    return out


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('--model', required=True, help='a COPY of the checkpoint (only read)')
    ap.add_argument('--zero', type=int, default=50, help='hands without a land')
    ap.add_argument('--one', type=int, default=100, help='hands with one land')
    ap.add_argument('--control', type=int, default=50, help='hands with 2-4 lands')
    ap.add_argument('--sims', type=int, default=64)
    ap.add_argument('--ks', default='0,4,8')
    ap.add_argument('--reps', type=int, default=2)
    ap.add_argument('--threads', type=int, default=4)
    ap.add_argument('--small', type=int, default=0,
                    help='also N decisions of 2-4 options from prior-played games (attacks, Daze, yes / no ...)')
    ap.add_argument('--out', default='')
    a = ap.parse_args(argv)
    os.environ.setdefault('CUDA_VISIBLE_DEVICES', '-1')
    import torch
    torch.set_num_threads(a.threads)
    try:
        import psutil
        psutil.Process().nice(psutil.BELOW_NORMAL_PRIORITY_CLASS if os.name == 'nt' else 10)
    except Exception:                              # noqa: BLE001
        pass
    from zero.infer import NetEvaluator
    from zero.net import load
    ks = [int(x) for x in a.ks.split(',')]
    t0 = time.time()
    m, _ = load(a.model, 'cpu', game='mtg')
    ev = NetEvaluator(m, 'cpu')
    items = hands(A.SPEC_DELVER, a.zero, a.one, a.control)
    trees = search_all(items, ev, a.sims, ks, a.reps)
    rows = summarize(items, trees)
    res = {'model': a.model, 'sims': a.sims, 'ks': ks, 'reps': a.reps, 'groups': table(rows, ks)}
    if a.small:
        sp = small_positions(A.SPEC_DELVER, ev, a.small)
        res['small'] = small_table(sp, search_all([(g, kind, m) for g, kind, m in sp], ev, a.sims, ks, 1), ks)
        print('small decisions', json.dumps(res['small']), flush=True)
    res['seconds'] = round(time.time() - t0, 1)
    for name, g in res['groups'].items():
        print(name, json.dumps({kk: (round(v, 3) if isinstance(v, float) else v) for kk, v in g.items()}), flush=True)
    if a.out:
        with open(a.out, 'w') as f:
            json.dump(dict(res, rows=rows), f, indent=1)
    print(json.dumps({'seconds': res['seconds']}))


if __name__ == '__main__':
    main()
