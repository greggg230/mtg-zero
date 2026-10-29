"""Belief-weighted determinization at the Delver mirror (docs/ENGINE.md 69, zero-core docs/search.md 3.8):
how the worlds the search deals from the belief head's prediction compare with the true hand, what that
does to decisions, and what the extra row costs. CPU only; every model is only read (pass COPIES).

  python tools/belief_search.py positions --model M --games 60 --out P.json.gz [--sims 32 --seed 1]
  python tools/belief_search.py deals --model M --positions P.json.gz [--tilts 0,0.5,1,2 --deals 200 --every 3]
  python tools/belief_search.py decisions --model M --positions P.json.gz [--tilts 0,1 --sims 64 --reps 8]
  python tools/belief_search.py speed --model M [--games 64 --seconds 90]

* `positions`: games of the network against itself (zero.h2h's match: the line's evaluation rule, uniform
  deals), each game's moves logged; `deals` and `decisions` rebuild every decision from them (the Rust
  driver's game seeds, its transitions).
* `deals`: at every `--every`-th decision of a player whose opponent holds cards: the head's prediction
  from the decider's features (`NetEvaluator.aux`), then `--deals` worlds per tilt (tilt 0: today's
  `determinize`; else `determinize_with`): the worlds' opponent hands against the true one - the cards
  misplaced by the deal's mean (count error), the cross-entropy per true card of the deal's mean
  composition (1% of the uniform deal's exact composition mixed in: `mtgz.az.belief.uniform_prior`), Force of Will / Daze recall (the true hand holds one:
  the share of worlds that deal one) and false alarm (it holds none), the distinct hands dealt (the
  search's diversity) - overall and by stratum (whose turn, the opponent's untapped lands).
* `decisions`: the main-phase decisions where the decider may cast a threat (a creature, Tamiyo) into an
  opponent with cards in hand and an untapped land: Rust trees at `--sims` per tilt and seed with the
  line's rule (cq, guard 4, scale 0.3) - the threat's and passing's visits and Q, the moves.
* `speed`: leaves and network rows per second of evaluation matches (the network against itself, 32
  simulations) and of the line's self-play seat (64 / 16, p_full 0.25, minor 4), uniform vs belief-weighted:
  the extra row's cost.
"""
import argparse
import gzip
import json
import math
import os
import random
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)

import mtgz.az as A                    # noqa: E402  (puts the pinned zero-core on sys.path)
import numpy as np                     # noqa: E402

RULE = ('cq', 4, 50.0, 0.3)            # the Delver line's evaluation rule (cq-n4-v50-s0.3)
SEAT = {'select': 'cq', 'select_min_visits': 4, 'cq_c_visit': 50.0, 'cq_c_scale': 0.3}
M64 = (1 << 64) - 1
WATCH = ('Force of Will', 'Daze', 'Lightning Bolt', 'Brainstorm', 'Murktide Regent')
THREATS = ('Murktide Regent', "Dragon's Rage Channeler", 'Tamiyo', 'Cori-Steel Cutter')


def low_priority(threads):
    os.environ.setdefault('CUDA_VISIBLE_DEVICES', '-1')
    import torch
    torch.set_num_threads(threads)
    try:
        import psutil
        psutil.Process().nice(psutil.BELOW_NORMAL_PRIORITY_CLASS if os.name == 'nt' else 10)
    except Exception:                              # noqa: BLE001
        pass


def evaluator(path):
    from zero.infer import NetEvaluator
    from zero.net import load
    m, _ = load(path, 'cpu', game='mtg')
    return NetEvaluator(m, 'cpu')


# ------------------------------------------------------------------ the Rust driver's game seeds

def split64(x):
    z = (x + 0x9E3779B97F4A7C15) & M64
    z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & M64
    z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & M64
    return z ^ (z >> 31)


class Xoshiro:
    """zero-core's `rng::Rng` (xoshiro256++ seeded by splitmix64)."""

    def __init__(self, seed):
        x, s = seed & M64, []
        for _ in range(4):
            x = (x + 0x9E3779B97F4A7C15) & M64
            s.append(split64(x))
        self.s = s if any(s) else [1, 0, 0, 0]

    def next_u64(self):
        s = self.s
        rotl = lambda v, k: ((v << k) | (v >> (64 - k))) & M64     # noqa: E731
        r = (rotl((s[0] + s[3]) & M64, 23) + s[0]) & M64
        t = (s[1] << 17) & M64
        s[2] ^= s[0]
        s[3] ^= s[1]
        s[1] ^= s[2]
        s[0] ^= s[3]
        s[2] ^= t
        s[3] = rotl(s[3], 45)
        return r


def game_seed(driver_seed, gid):
    """The seed `Driver` gives game `gid` (driver/worker.rs `init_slot`)."""
    return Xoshiro(split64((driver_seed & M64) ^ split64(gid + 1))).next_u64()


def rebuilt(pos_file):
    """Yield (game index, move index, game) at every decision of the logged games (each a clone)."""
    from zero import mcts
    from zero.games import engine
    E = engine()
    d = json.load(gzip.open(pos_file, 'rt'))
    for row in d['rows']:
        g = E.Game('mtg', d['spec'], 2, game_seed(d['seed'], row['gid']))
        for k, (seat, a, legal) in enumerate(row['moves']):
            if g.over:
                break
            if list(g.legal()) != list(legal) or g.to_move != seat:
                raise SystemExit(f'game {row["gid"]} move {k}: the rebuilt game does not match the log')
            if len(legal) >= 2:
                yield row['gid'], k, g.clone()
            mcts.transition(g, a)


def cmd_positions(a):
    from zero import h2h
    low_priority(a.torch_threads)
    t0 = time.time()
    r = h2h.match(a.model, a.model, games=a.games, sims=a.sims, chance_k=0, seed=a.seed, threads=a.threads,
                  spec=A.SPEC_DELVER, conc=a.conc, seat_a=dict(SEAT), seat_b=dict(SEAT), max_steps=20000,
                  move_log=100_000, rows=True)
    rows = [{'gid': x['gid'], 'moves': x['moves'], 'result': x['result']} for x in r['rows']]
    json.dump({'spec': A.SPEC_DELVER, 'seed': a.seed, 'model': a.model, 'sims': a.sims, 'rows': rows},
              gzip.open(a.out, 'wt'))
    print(json.dumps({'games': len(rows), 'moves': sum(len(x['moves']) for x in rows), 'seconds': round(time.time() - t0, 1),
                      'a_score': r['a_score']}))


# ------------------------------------------------------------------ deals

def card_columns(cards=WATCH):
    """{card name: its identity columns} (the spectator view's names, `mtgz.az.belief.names`)."""
    from mtgz.az import belief as B
    nm = B.names()
    return {c: [k for k, m in enumerate(nm) if m == c] for c in cards}


def state_facts(g, v):
    """(the decider's turn, the opponent's untapped lands) from the spectator view."""
    s = json.loads(g.export_state())
    lands = sum(1 for c in s['players'][1 - v]['battlefield'] if 'Land' in str(c.get('types')) and not c.get('tapped'))
    return s.get('active') == v, lands


def cmd_deals(a):
    low_priority(a.torch_threads)
    ev = evaluator(a.model)
    tilts = [float(x) for x in a.tilts.split(',')]
    idx = card_columns()
    assert all(idx.values()), idx
    pos = []
    n_seen = 0
    for gid, k, g in rebuilt(a.positions):
        v = g.to_move
        t = np.asarray(g.aux_target(v), np.float64)
        if t.sum() == 0:
            continue
        n_seen += 1
        if n_seen % a.every:
            continue
        pos.append((gid, k, g, v, t))
        if a.max and len(pos) >= a.max:
            break
    x = np.stack([np.asarray(g.encode(v), np.float32) for _, _, g, v, _ in pos])
    p = np.concatenate([ev.aux(x[i:i + 256], np.full(len(x[i:i + 256]), 2))['belief'] for i in range(0, len(x), 256)])
    t0 = time.time()
    res = {tt: [] for tt in tilts}
    for j, (gid, k, g, v, t) in enumerate(pos):
        n = t.sum()
        facts = state_facts(g, v)
        for tt in tilts:
            e = np.zeros(len(t))
            wc = {c: np.zeros(a.deals) for c in idx}
            hands = set()
            for s in range(a.deals):
                w = g.clone()
                seed = (gid * 1_000_003 + k * 7919 + s) & M64
                if tt == 0:
                    w.determinize(v, seed)
                else:
                    w.determinize_with(v, seed, p[j], tt, a.clip)
                d = np.asarray(w.aux_target(v), np.float64)
                e += d
                nz = np.flatnonzero(d)
                hands.add(tuple(nz) + tuple(d[nz]))
                for c, ks in idx.items():
                    wc[c][s] = d[ks].sum()
            res[tt].append(((e / a.deals, wc, len(hands)), t, n, facts, p[j]))
        if j % 200 == 0:
            print(f'{j}/{len(pos)} positions, {time.time() - t0:.0f} s', flush=True)
    out = {'positions': len(pos), 'deals': a.deals, 'clip': a.clip, 'every': a.every, 'by_tilt': {}}
    # the uniform deal's exact expected counts (mtgz.az.belief.uniform_prior, from the features): the
    # deals' compositions are smoothed with 1% of it (a true card a finite set of deals never dealt)
    from mtgz.az import belief as B
    uni = B.uniform_prior(x)
    base = {j: uni[j] for j in range(len(pos))}
    for tt in tilts:
        out['by_tilt'][str(tt)] = summarize(res[tt], idx, base, p_head=None)
    # the head's own prediction (what tilt 1 targets, to first order) and the uniform deal, exactly
    def point(e):
        return (e, {c: np.array([e[ks].sum()]) for c, ks in idx.items()}, 1)
    head = [(point(pr * n), t, n, f, pr) for (_, t, n, f, pr) in res[tilts[0]]]
    out['head'] = summarize(head, idx, base, p_head=True)
    exact = [(point(uni[j]), t, n, f, pr) for j, (_, t, n, f, pr) in enumerate(res[tilts[0]])]
    out['uniform_exact'] = summarize(exact, idx, base, p_head=True)
    strata = {}
    for tt in tilts:
        for key, sel in (('my turn, they 0 untapped', lambda f: f[0] and f[1] == 0),
                         ('my turn, they 1+ untapped', lambda f: f[0] and f[1] >= 1),
                         ('their turn', lambda f: not f[0])):
            rows = [r for r in res[tt] if sel(r[3])]
            if rows:
                strata.setdefault(key, {})[str(tt)] = summarize(rows, idx, None, p_head=None, short=True)
    out['strata'] = strata
    print(json.dumps(out, indent=1))
    if a.out:
        json.dump(out, open(a.out, 'w'), indent=1)


def summarize(rows, idx, base, p_head=None, short=False):
    """Deal quality of positions [((the deals' mean counts [D], per watched card its dealt counts per deal,
    distinct hands), true counts [D], hand size, facts, head p)]; `base`: per position the uniform deal's
    exact expected counts (smoothing)."""
    err, ce, distinct = [], [], []
    rec = {c: [] for c in idx}
    fa = {c: [] for c in idx}
    mean_true = {c: [] for c in idx}
    mean_dealt = {c: [] for c in idx}
    for j, ((e, wc, nd), t, n, _, pr) in enumerate(rows):
        err.append(0.5 * np.abs(e - t).sum())
        q = e / n
        if base is not None and not p_head:
            q = 0.99 * q + 0.01 * base[j] / n
        q = np.maximum(q, 1e-6)
        ce.append(float(-(t / n * np.log(q)).sum()))
        distinct.append(nd)
        for c, ks in idx.items():
            tc = t[ks].sum()
            dc = wc[c]
            mean_true[c].append(tc)
            mean_dealt[c].append(dc.mean())
            if p_head:
                continue
            (rec if tc > 0 else fa)[c].append(float((dc > 0).mean()))
    o = {'count_error': round(float(np.mean(err)), 4), 'cross_entropy': round(float(np.mean(ce)), 4),
         'ce_se': round(float(np.std(ce) / math.sqrt(len(ce))), 4)}
    if not p_head:
        o['distinct_hands'] = round(float(np.mean(distinct)), 2)
    for c in idx:
        if short and c not in ('Force of Will', 'Daze', 'Brainstorm'):
            continue
        d = {'true': round(float(np.mean(mean_true[c])), 3), 'dealt': round(float(np.mean(mean_dealt[c])), 3)}
        if not p_head:
            d['recall'] = round(float(np.mean(rec[c])), 3) if rec[c] else None
            d['false_alarm'] = round(float(np.mean(fa[c])), 3) if fa[c] else None
        o[c] = d
    o['n'] = len(rows)
    return o


# ------------------------------------------------------------------ decisions

def cmd_decisions(a):
    low_priority(a.torch_threads)
    ev = evaluator(a.model)
    tilts = [float(x) for x in a.tilts.split(',')]
    found = []
    for gid, k, g in rebuilt(a.positions):
        v = g.to_move
        s = json.loads(g.export_state())
        dec = s.get('decision') or {}
        if dec.get('kind') != 'Priority' or s.get('active') != v or not str(s.get('step', '')).startswith('main'):
            continue
        casts = [o for o in dec.get('options', []) if o.get('t', '').startswith('Cast ') and
                 any(th in o['t'] for th in THREATS)]
        if not casts:
            continue
        _, lands = state_facts(g, v)
        if lands < 1 or sum(np.asarray(g.aux_target(v))) < 2:
            continue
        found.append((gid, k, g, v, casts, dec))
        if len(found) >= a.max:
            break
    print(f'{len(found)} positions', flush=True)
    out = []
    for gid, k, g, v, casts, dec in found:
        p = ev.aux(np.asarray(g.encode(v), np.float32)[None], np.full(1, 2))['belief'][0]
        acts = list(g.legal())
        cast_a = casts[0]['a']
        pass_a = next((o['a'] for o in dec['options'] if o.get('t', '').startswith('Pass')), None)
        row = {'game': gid, 'move': k, 'cast': casts[0]['t'], 'by_tilt': {}}
        for tt in tilts:
            trees = []
            for r in range(a.reps):
                t = g.search_tree(True, 1000 + r)
                if tt:
                    t.set_belief(p, g, tt, a.clip, a.mix)
                trees.append(t)
            search(trees, g, ev, a.sims)
            nv = np.mean([t.root_visits for t in trees], 0)
            w = np.mean([t.root_values for t in trees], 0)
            q = [float(w[i] / nv[i]) if nv[i] else None for i in range(len(nv))]
            moves = [acts[t.root_select(*RULE)] for t in trees]
            ic, ip = acts.index(cast_a), (acts.index(pass_a) if pass_a in acts else None)
            row['by_tilt'][str(tt)] = {'cast_visits': round(float(nv[ic]), 1), 'cast_q': q[ic] and round(q[ic], 3),
                                       'pass_visits': None if ip is None else round(float(nv[ip]), 1),
                                       'pass_q': None if ip is None or q[ip] is None else round(q[ip], 3),
                                       'cast_moves': sum(m == cast_a for m in moves), 'pass_moves': sum(m == pass_a for m in moves),
                                       'reps': a.reps}
        print(json.dumps(row), flush=True)
        out.append(row)
    if a.out:
        json.dump(out, open(a.out, 'w'), indent=1)


def search(trees, g, ev, sims):
    """`sims` simulations of every tree of the same position in lockstep (one batch a step)."""
    for _ in range(sims):
        rows, acts, owners = [], [], []
        for j, t in enumerate(trees):
            r = t.simulate(g, 1.0)
            if r is None:
                continue
            rows.append(r[1])
            acts.append(np.asarray(r[2], np.int32))
            owners.append(j)
        if not rows:
            continue
        off = np.zeros(len(rows) + 1, np.int32)
        off[1:] = np.cumsum([len(x) for x in acts])
        pri, val, mar = ev.evaluate(np.stack(rows).astype(np.float32), np.full(len(rows), 2, np.int32), off,
                                    np.concatenate(acts))
        for b, j in enumerate(owners):
            trees[j].expand([float(x) for x in pri[off[b]:off[b + 1]]], [float(x) for x in val[b]], [float(x) for x in mar[b]])


# ------------------------------------------------------------------ speed

def cmd_speed(a):
    from zero import games as G
    from zero.infer import drive
    low_priority(a.torch_threads)
    ev = evaluator(a.model)
    out = {}
    for mode, tilt in (('eval', 0.0), ('eval', 1.0), ('selfplay', 0.0), ('selfplay', 1.0)):
        kw = dict(belief_tilt=tilt) if tilt else {}
        if mode == 'eval':
            seat = dict(G.nn_seat('m', 32, chance_k=0, **kw), **SEAT)
        else:
            # the line's self-play seat: 64 / 16 simulations, p_full 0.25, minor decisions at 4 (only the full
            # searches take the belief)
            seat = G.nn_seat('m', 64, selfplay=True, sims_fast=16, p_full=0.25, sims_minor=4, policy_target='cq',
                             cq_c_scale=0.3, temp_moves=60, chance_k=0, **kw)
        drv = G.Driver('mtg', [[seat, seat]] * a.games, spec=A.SPEC_DELVER, concurrency=a.games, seed=a.seed,
                       threads=a.threads, groups=2, max_steps=20000)
        t0 = time.time()
        ev.seconds, ev.calls, ev.leaves = 0.0, 0, 0

        def stop(lb):
            if time.time() - t0 > a.seconds:
                raise StopIteration
        try:
            drive(drv, {'m': ev}, on_batch=stop)
        except StopIteration:
            pass
        secs = time.time() - t0
        st = drv.stats()
        out[f'{mode} tilt {tilt:g}'] = {'seconds': round(secs, 1), 'leaves': st['leaves'], 'belief_rows': st['belief_rows'],
                          'leaves_per_s': round(st['leaves'] / secs, 1),
                          'rows_per_s': round((st['leaves'] + st['belief_rows']) / secs, 1),
                          'belief_rows_per_leaf': round(st['belief_rows'] / max(1, st['leaves']), 4),
                          'infer_s': round(ev.seconds, 1), 'avg_batch_rows': round(ev.leaves / max(1, ev.calls), 1)}
        print(mode, tilt, json.dumps(out[f'{mode} tilt {tilt:g}']), flush=True)
    if a.out:
        json.dump(out, open(a.out, 'w'), indent=1)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    sub = ap.add_subparsers(dest='cmd', required=True)
    for name in ('positions', 'deals', 'decisions', 'speed'):
        s = sub.add_parser(name)
        s.add_argument('--model', required=True)
        s.add_argument('--torch-threads', type=int, default=4)
        s.add_argument('--threads', type=int, default=4)
        s.add_argument('--seed', type=int, default=1)
        s.add_argument('--out', default='')
        s.add_argument('--clip', type=float, default=20.0)
        s.add_argument('--mix', type=float, default=0.0)
        if name == 'positions':
            s.add_argument('--games', type=int, default=60)
            s.add_argument('--sims', type=int, default=32)
            s.add_argument('--conc', type=int, default=60)
        if name in ('deals', 'decisions'):
            s.add_argument('--positions', required=True)
            s.add_argument('--tilts', default='0,0.5,1,2' if name == 'deals' else '0,1')
        if name == 'deals':
            s.add_argument('--deals', type=int, default=200)
            s.add_argument('--every', type=int, default=3)
            s.add_argument('--max', type=int, default=0)
        if name == 'decisions':
            s.add_argument('--sims', type=int, default=64)
            s.add_argument('--reps', type=int, default=8)
            s.add_argument('--max', type=int, default=40)
        if name == 'speed':
            s.add_argument('--games', type=int, default=64)
            s.add_argument('--seconds', type=float, default=90.0)
    a = ap.parse_args(argv)
    {'positions': cmd_positions, 'deals': cmd_deals, 'decisions': cmd_decisions, 'speed': cmd_speed}[a.cmd](a)


if __name__ == '__main__':
    main()
