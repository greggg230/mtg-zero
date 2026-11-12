"""The Gumbel root search at the Delver mirror (docs/ENGINE.md 68, zero-core docs/search.md 3.7): what
Gumbel AlphaZero's root (Gumbel noise, sequential halving over the top options, the completed-Q sigma)
does where PUCT at 64 simulations is locked in by the prior - the mulligan, the replay's upkeep
Brainstorm - and in games against PUCT with the same network. CPU only; every model is only read (pass
COPIES).

  python tools/gumbel_eval.py mull --model M [--zero 50 --one 100 --control 50 --sims 64 --reps 2 --noise-reps 8]
  python tools/gumbel_eval.py position --model M --replay R.json --frame 20 [--reps 16 --sims 64,256]
  python tools/gumbel_eval.py h2h --model M --a gumbel --b puct --games 300 --sims 32 [--view mover|root]
  python tools/gumbel_eval.py speed --model M [--games 64 --seconds 120]
  python tools/gumbel_eval.py selfplay --model M [--games 48]               (recorded self-play, both modes)

Modes (the search of one decision, the move it plays):
* `puct`: PUCT at the root, the Delver line's evaluation rule (cq, guard 4, sigma (50, 0.3)) - today's;
  `visits` its argmax-visits move is reported too.
* `gumbel`: the Gumbel root (m 16, no noise: an evaluation's), sigma (50, 0.3) - the target's and the
  rule's transform of the line; its move `gumbel_choose`.
* `gumbel-s1`: the same with sigma (50, 1.0), the paper's scale.
* `gumbel-noise`: Gumbel noise of scale 1 (self-play's: its move a sample of the improved policy),
  `--noise-reps` noise draws per position: the rate at which self-play would keep.
Each with the line's mover-view leaves and with root-view leaves (zero-core docs/search.md 3.6).

* `mull`: tools/mull_floor.py's seven-card hands (0 lands, 1 land, 2-4 as a control) at 64 simulations;
  per land group and declaration order: the prior on Mulligan, Q Keep / Q Mulligan, Mulligan's visits,
  the 'cq' target's mass on Mulligan (scale 0.3), the keep rate of each mode.
* `position`: a replay's frame (zero-replay/1: rebuilt from its seed and actions, tools/root_view.py)
  searched `--reps` times per mode and view: visits and Q per option, the moves.
* `h2h`: the same checkpoint, side A in one mode against side B in another (seats alternating, zero.h2h's
  match: the Delver spec, chance_k 2 as the A/B driver's h2h), both sides' leaves in `--view`.
* `speed`: self-play leaves per second of the line's seat (64 / 16, p_full 0.25, minor 4, cq 0.3),
  PUCT vs Gumbel (same games while the moves agree: per leaf).
* `selfplay`: recorded self-play games of the line's seat (as `speed`; Gumbel: noise 1 within
  temp_moves 60) per mode: the records' 'cq' targets against the network's prior on the same rows
  (entropy, mass off the prior's favourite, a target whose argmax is not the prior's), their root value
  q against the outcome z, and the moves played (not the prior's favourite).
"""
import argparse
import json
import os
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
sys.path.insert(0, os.path.join(ROOT, 'tools'))

import mtgz.az as A                    # noqa: E402  (puts the pinned zero-core on sys.path)
import numpy as np                     # noqa: E402

from root_view import KEEP, MULL, RULE, evaluator, low_priority, rebuild  # noqa: E402

SIGMA = (50.0, 0.3)                    # the Delver line's cq_c_visit / cq_c_scale (target and rule)
MODES = {'puct': None, 'gumbel': (16, 0.0, 0.3), 'gumbel-s1': (16, 0.0, 1.0), 'gumbel-noise': (16, 1.0, 0.3)}


def search_all(games, ev, sims, view, seeds, mode, noise_seed=0):
    """One Rust tree per (game, seed), `sims` simulations each in lockstep (one network batch per step,
    root-view leaves' value-only rows in the same batch); `mode` a MODES key (a Gumbel mode: a fresh
    root's sims - 1 root walks, noise from `noise_seed` + the tree's index). Rows carry their tails and overrides on
    a layout v7 engine (docs/ENGINE.md 125, tools/rowkit.py)."""
    import rowkit
    rk = rowkit.kind('mtg')
    trees = []
    g_cfg = MODES[mode]
    for j, (g, s) in enumerate(zip(games, seeds)):
        t = g.search_tree(True, int(s))
        if g_cfg is not None:
            m, scale, cs = g_cfg
            k = len(g.legal())
            noise = list(scale * np.random.default_rng(noise_seed + j).gumbel(size=k)) if scale else []
            t.set_gumbel(m, sims - 1, noise, SIGMA[0], cs)
        trees.append(t)
    rv = view == 'root'
    for _ in range(sims):
        rows, acts, rrs, owners = [], [], [], []
        for j, (g, t) in enumerate(zip(games, trees)):
            r = rowkit.simulate(t, g, 1.0, 0, rv, rk)
            if r is None:
                continue
            _, x, a, vx, rr, vrr = r
            rows.append(x)
            acts.append(list(a))
            rrs.append(rr)
            owners.append((j, len(rows) - 1, None if vx is None else len(rows)))
            if vx is not None:
                rows.append(vx)
                acts.append([])
                rrs.append(vrr)
        if not rows:
            continue
        off = np.zeros(len(rows) + 1, np.int32)
        off[1:] = np.cumsum([len(a) for a in acts])
        pri, val, mar = rowkit.evaluate(ev, rows, acts, rk, rrs)[:3]
        for j, b, vb in owners:
            v = b if vb is None else vb
            trees[j].expand([float(x) for x in pri[off[b]:off[b + 1]]], [float(x) for x in val[v]],
                            [float(x) for x in mar[v]])
    return trees


def root_row(t, mode):
    nv, w, p0, v0 = list(t.root_visits), list(t.root_values), list(t.root_priors0), t.root_value0
    q = [w[a] / nv[a] if nv[a] else None for a in range(len(nv))]
    cs = MODES[mode][2] if MODES[mode] else SIGMA[1]
    move = t.gumbel_choose() if MODES[mode] else t.root_select(*RULE)
    qw = t.gumbel_q() if MODES[mode] else t.root_win()
    return {'n': nv, 'q': q, 'p0': p0, 'v0': v0, 'acts': list(t.root_acts), 'move': int(move),
            'visits_move': int(max(range(len(nv)), key=lambda k: (nv[k], p0[k]))),
            'cq': list(t.root_cq_target(SIGMA[0], cs)), 'root_q': None if qw is None else qw[t.root_player],
            'root_mean': None if t.root_win() is None else t.root_win()[t.root_player]}


# ------------------------------------------------------------------ mull

def cmd_mull(a):
    from mull_floor import hands
    ev = evaluator(a.model)
    items = hands(A.SPEC_DELVER, a.zero, a.one, a.control)
    out = {'model': a.model, 'sims': a.sims, 'reps': a.reps, 'noise_reps': a.noise_reps, 'groups': {}, 'seconds': {}}
    rows = {}
    for mode in a.modes.split(','):
        reps = a.noise_reps if mode == 'gumbel-noise' else a.reps
        games = [g for g, _, _ in items for _ in range(reps)]
        seeds = [7919 * i + 104729 * r + 1 for i in range(len(items)) for r in range(reps)]
        for view in a.views.split(','):
            t0 = time.time()
            trees = search_all(games, ev, a.sims, view, seeds, mode, noise_seed=a.seed * 1_000_003)
            rows[(mode, view)] = [dict(root_row(t, mode), pos=k // reps, lands=items[k // reps][1],
                                       order=items[k // reps][2]) for k, t in enumerate(trees)]
            out['seconds'][f'{mode} {view}'] = round(time.time() - t0, 1)
            print(f'{mode} {view}: {len(trees)} searches, {out["seconds"][f"{mode} {view}"]} s', flush=True)
    groups = [('0 lands', lambda L: L == 0), ('1 land', lambda L: L == 1), ('2-4 lands', lambda L: L >= 2)]
    for name, f in groups:
        for order in (None, 0, 1):
            key = name + ('' if order is None else (' first' if order == 0 else ' second'))
            g = {}
            for (mode, view), rr in rows.items():
                rs = [x for x in rr if f(x['lands']) and (order is None or x['order'] == order)]
                if not rs:
                    continue
                qk = [x['q'][KEEP] for x in rs if x['q'][KEEP] is not None]
                qm = [x['q'][MULL] for x in rs if x['q'][MULL] is not None]
                g[f'{mode} {view}'] = {
                    'searches': len(rs), 'prior_mull': round(float(np.mean([x['p0'][MULL] for x in rs])), 3),
                    'q_keep': round(float(np.mean(qk)), 3) if qk else None,
                    'q_mull': round(float(np.mean(qm)), 3) if qm else None,
                    'n_mull': round(float(np.mean([x['n'][MULL] for x in rs])), 1),
                    'cq_mull': round(float(np.mean([x['cq'][MULL] for x in rs])), 3),
                    'keep': round(1 - float(np.mean([x['move'] == MULL for x in rs])), 3),
                    'keep_visits_rule': round(1 - float(np.mean([x['visits_move'] == MULL for x in rs])), 3)}
            out['groups'][key] = g
            print(key, json.dumps(g), flush=True)
    if a.out:
        json.dump(dict(out, rows={f'{m} {v}': r for (m, v), r in rows.items()}), open(a.out, 'w'), indent=1)
    return out


# ------------------------------------------------------------------ position

def cmd_position(a):
    import gzip
    op = gzip.open if a.replay.endswith('.gz') else open
    rep = json.load(op(a.replay, 'rt', encoding='utf-8'))
    g = rebuild(rep, a.frame)
    f = rep['frames'][a.frame]
    st = json.loads(g.export_state())
    names = {o['a']: o['t'] for o in st['decision']['options']}
    ev = evaluator(a.model)
    out = {'replay': os.path.basename(a.replay), 'frame': a.frame, 'model': a.model, 'recorded': f, 'options': names,
           'runs': {}}
    for sims in [int(x) for x in a.sims.split(',')]:
        for mode in a.modes.split(','):
            for view in a.views.split(','):
                trees = search_all([g] * a.reps, ev, sims, view, [1000 + r for r in range(a.reps)], mode,
                                   noise_seed=a.seed * 7919)
                rr = [root_row(t, mode) for t in trees]
                acts = rr[0]['acts']
                res = {'n': [round(float(np.mean([x['n'][i] for x in rr])), 1) for i in range(len(acts))],
                       'q': [round(float(np.mean([x['q'][i] for x in rr if x['q'][i] is not None])), 4)
                             if any(x['q'][i] is not None for x in rr) else None for i in range(len(acts))],
                       'cq': [round(float(np.mean([x['cq'][i] for x in rr])), 3) for i in range(len(acts))],
                       'v0': round(float(np.mean([x['v0'] for x in rr])), 4),
                       'root_q': round(float(np.mean([x['root_q'] for x in rr])), 4),
                       'moves': {names.get(acts[i], acts[i]): sum(x['move'] == i for x in rr) for i in range(len(acts))}}
                out['runs'][f'{mode} {view} {sims}'] = res
                print(f'{mode} {view} {sims}', json.dumps(res), flush=True)
    if a.out:
        json.dump(out, open(a.out, 'w'), indent=1)
    return out


# ------------------------------------------------------------------ h2h, speed

def side_seat(mode, view):
    """zero.h2h seat settings of a side: 'puct' = the line's evaluation rule cq4 s0.3; a Gumbel mode =
    its root search (no noise for 'gumbel' / 'gumbel-s1')."""
    keys = {'select': 'cq', 'select_min_visits': RULE[1], 'cq_c_visit': RULE[2], 'cq_c_scale': RULE[3]}
    if MODES[mode] is not None:
        m, scale, cs = MODES[mode]
        keys = dict(root_search='gumbel', gumbel_m=m, gumbel_scale=scale, gumbel_c_visit=SIGMA[0], gumbel_c_scale=cs)
    if view == 'root':
        keys['leaf_value_view'] = 'root'
    return keys


def cmd_h2h(a):
    from zero import h2h
    r = h2h.match(a.model, a.model, games=a.games, sims=a.sims, chance_k=a.chance_k, seed=a.seed, threads=a.search_threads,
                  device='cpu', spec=A.SPEC_DELVER, conc=a.conc, seat_a=side_seat(a.a, a.view), seat_b=side_seat(a.b, a.view))
    r.update(mode_a=a.a, mode_b=a.b, view=a.view)
    print(json.dumps(r), flush=True)
    if a.out:
        json.dump(r, open(a.out, 'w'), indent=1)
    return r


def cmd_speed(a):
    from zero import games as G
    from zero.infer import drive
    ev = evaluator(a.model)
    out = {}
    for mode in a.modes.split(','):
        kw = {} if mode == 'puct' else dict(root_search='gumbel', gumbel_m=16, gumbel_scale=1.0)
        seat = G.nn_seat('m', 64, selfplay=True, sims_fast=16, p_full=0.25, sims_minor=4, policy_target='cq',
                         cq_c_scale=0.3, temp_moves=60, **kw)
        drv = G.Driver('mtg', [[seat, seat]] * a.games, spec=A.SPEC_DELVER, concurrency=a.games, seed=a.seed,
                       record=False, threads=a.search_threads, groups=2, max_steps=20000)
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
        out[mode] = {'seconds': round(secs, 1), 'leaves': st['leaves'], 'sims': st['sims'],
                     'leaves_per_s': round(st['leaves'] / secs, 1), 'infer_s': round(ev.seconds, 1),
                     'search_busy_s': round(st['busy_s'], 1), 'avg_batch_rows': round(ev.leaves / max(1, ev.calls), 1),
                     'search_us_per_leaf': round(1e6 * st['busy_s'] / max(1, st['leaves']), 1)}
        print(mode, json.dumps(out[mode]), flush=True)
    if a.out:
        json.dump(out, open(a.out, 'w'), indent=1)


def cmd_selfplay(a):
    from zero import games as G
    from zero.infer import drive
    from zero.protocol import concat_samples
    ev = evaluator(a.model)
    out = {}
    for mode in a.modes.split(','):
        kw = {} if mode == 'puct' else dict(root_search='gumbel', gumbel_m=16, gumbel_scale=1.0)
        seat = G.nn_seat('m', 64, selfplay=True, sims_fast=16, p_full=0.25, sims_minor=4, policy_target='cq',
                         cq_c_scale=0.3, temp_moves=60, **kw)
        drv = G.Driver('mtg', [[seat, seat]] * a.games, spec=A.SPEC_DELVER, concurrency=a.games, seed=a.seed,
                       record=True, threads=a.search_threads, groups=2, max_steps=20000, move_log=400)
        parts = []
        t0 = time.time()
        drive(drv, {'m': ev}, on_batch=lambda lb: parts.append(drv.take_samples()))
        parts.append(drv.take_samples())
        secs = time.time() - t0
        rows = drv.take_results()
        s = concat_samples([p for p in parts if p is not None and len(p['player'])])
        import rowkit
        rk = rowkit.kind('mtg')
        off = s['opt_off']
        n = len(off) - 1
        ent, off_fav, flip, maxp = [], [], [], []
        for i in range(0, n, 256):
            r = np.arange(i, min(n, i + 256))
            lo, hi = off[r[0]], off[r[-1] + 1]
            sub = (off[r[0]:r[-1] + 2] - lo).astype(np.int32)
            pri, _, _ = ev.evaluate(np.ascontiguousarray(s['x'][r], np.float32), np.full(len(r), 2, np.int32), sub,
                                    np.ascontiguousarray(s['opt_action'][lo:hi], np.int32), None,
                                    **rowkit.sample_kw(rk, s, r))
            for b in range(len(r)):
                pp = pri[sub[b]:sub[b + 1]]
                pi = s['pi'][lo + sub[b]:lo + sub[b + 1]]
                f = int(np.argmax(pp))
                ent.append(float(-(pi[pi > 0] * np.log(pi[pi > 0])).sum()))
                off_fav.append(1.0 - float(pi[f]))
                flip.append(int(np.argmax(pi)) != f)
                maxp.append(float(pp.max()))
        z, q = s['z'][:, 0].astype(float), s['q'][:, 0].astype(float)
        st = drv.stats()
        out[mode] = {'games': len(rows), 'seconds': round(secs, 1), 'leaves_per_s': round(st['leaves'] / secs, 1),
                     'records': int(n), 'records_per_game': round(n / max(1, len(rows)), 1),
                     'p0_score': round(float(np.mean([r['result'][0] for r in rows])), 3),
                     'turns': round(float(np.mean([r['rounds'] for r in rows])), 1),
                     'prior_max': round(float(np.mean(maxp)), 3), 'target_entropy': round(float(np.mean(ent)), 4),
                     'target_off_prior_favourite': round(float(np.mean(off_fav)), 4),
                     'target_argmax_not_prior': round(float(np.mean(flip)), 4),
                     'q_mean': round(float(q.mean()), 4), 'z_mean': round(float(z.mean()), 4),
                     'q_brier_vs_z': round(float(((q - z) ** 2).mean()), 4),
                     'q_corr_z': round(float(np.corrcoef(q, z)[0, 1]), 4)}
        print(mode, json.dumps(out[mode]), flush=True)
    if a.out:
        json.dump(out, open(a.out, 'w'), indent=1)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    sub = ap.add_subparsers(dest='cmd', required=True)
    for name in ('mull', 'position', 'h2h', 'speed', 'selfplay'):
        p = sub.add_parser(name)
        p.add_argument('--model', required=True, help='a COPY of the checkpoint (only read)')
        p.add_argument('--threads', type=int, default=4, help='torch threads')
        p.add_argument('--out', default='')
        p.add_argument('--seed', type=int, default=1)
        if name in ('mull', 'position'):
            p.add_argument('--modes', default='puct,gumbel,gumbel-s1,gumbel-noise')
            p.add_argument('--views', default='mover,root')
        if name == 'mull':
            p.add_argument('--zero', type=int, default=50)
            p.add_argument('--one', type=int, default=100)
            p.add_argument('--control', type=int, default=50)
            p.add_argument('--sims', type=int, default=64)
            p.add_argument('--reps', type=int, default=2)
            p.add_argument('--noise-reps', type=int, default=8)
        if name == 'position':
            p.add_argument('--replay', required=True)
            p.add_argument('--frame', type=int, required=True)
            p.add_argument('--reps', type=int, default=16)
            p.add_argument('--sims', default='64')
        if name == 'h2h':
            p.add_argument('--a', default='gumbel', choices=sorted(MODES))
            p.add_argument('--b', default='puct', choices=sorted(MODES))
            p.add_argument('--view', default='mover', choices=['mover', 'root'])
            p.add_argument('--games', type=int, default=300)
            p.add_argument('--sims', type=int, default=32)
            p.add_argument('--chance-k', type=int, default=2)
            p.add_argument('--conc', type=int, default=None)
            p.add_argument('--search-threads', type=int, default=4)
        if name == 'selfplay':
            p.add_argument('--modes', default='puct,gumbel')
            p.add_argument('--games', type=int, default=48)
            p.add_argument('--search-threads', type=int, default=4)
        if name == 'speed':
            p.add_argument('--modes', default='puct,gumbel')
            p.add_argument('--games', type=int, default=64)
            p.add_argument('--seconds', type=float, default=120)
            p.add_argument('--search-threads', type=int, default=4)
    a = ap.parse_args(argv)
    low_priority(a.threads)
    {'mull': cmd_mull, 'position': cmd_position, 'h2h': cmd_h2h, 'speed': cmd_speed, 'selfplay': cmd_selfplay}[a.cmd](a)


if __name__ == '__main__':
    main()
