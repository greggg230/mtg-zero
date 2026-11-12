"""Root-view leaf values at the Delver mirror (docs/ENGINE.md 67, zero-core docs/search.md 3.6): what a
search leaf of the opponent's is worth to the root player from the root player's own view (its true
hand, the opponent's unknown) against the mover's view (the opponent's sampled hand, blind to ours),
and what that does to the moves. CPU only; every model is only read (pass COPIES).

  python tools/root_view.py mull --model M [--zero 50 --one 100 --control 50 --sims 64 --reps 2]
  python tools/root_view.py position --model M --replay R.json --frame 20 [--reps 16 --sims 64,256]
  python tools/root_view.py selfplay --model M --games 64 --out S.npz      (CPU self-play, value-only samples)
  python tools/root_view.py calib --model M --samples S.npz [--split 0.25]
  python tools/root_view.py finetune --model M --samples S.npz --out F.pt [--steps 300]
  python tools/root_view.py play --model M --games 24 [--view mover,root]  (instants by step, both views)
  python tools/root_view.py speed --model M                                  (leaves/s, both views)

* `mull`: tools/mull_floor.py's seven-card hands (0 lands, 1 land, 2-4 as a control) searched by the
  Rust tree at 64 simulations with the leaf values of each view; per land group and declaration
  order: the prior, Q Keep / Q Mulligan, the keep rates of the 'visits' rule and of the line's
  evaluation rule (cq, guard 4, scale 0.3), and at the searches' leaves of the opponent the root
  player's value from both views (the same positions).
* `position`: a replay's frame (zero-replay/1: rebuilt from its seed and actions) searched with both
  views, `--reps` seeds: Q and visits per option, the move of the line's rule.
* `selfplay`: games of the run's self-play seat (`--sims`/`--fast`, mover-view leaves as the main
  line plays) recording every decision's value-only sample (zero-core `view_samples` 1): the
  decider's view and the other player's view of the same state, z, q.
* `calib`: the network's value on those samples against the outcomes, decision rows (the decider's
  view: what it was trained on) vs value-only rows (the other view: what root-view leaves ask for):
  log loss, Brier, calibration bins; the pairs' value difference.
* `finetune`: a short fine-tune on the samples (decision rows: policy + value as in training;
  value-only rows: value only), held-out games' calibration before / after.
* `play`: games of the network against itself under each view (`--view`), the line's evaluation
  rule; how often each player casts an instant in its own upkeep, draw step and main phases.
"""
import argparse
import json
import math
import os
import random
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
sys.path.insert(0, os.path.join(ROOT, 'tools'))

import mtgz.az as A                    # noqa: E402  (puts the pinned zero-core on sys.path)
import numpy as np                     # noqa: E402

KEEP, MULL = 0, 1
RULE = ('cq', 4, 50.0, 0.3)            # the Delver line's evaluation rule (cq-n4-v50-s0.3)
INSTANTS = {'Brainstorm', 'Lightning Bolt', 'Daze', 'Force of Will', 'Unholy Heat', 'Prismari Charm'}


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


def search_all(games, ev, sims, view, seeds, root_min=0, pairs=None):
    """One Rust tree per (game, seed), `sims` simulations each in lockstep (one network batch per
    step, the value-only rows of root-view leaves in the same batch). `view` 'mover' | 'root'.
    `pairs` (a list): for every leaf of another seat than the tree's viewer (root view only), the
    viewer's value of it from the mover's row and from its own row: (mover-view, root-view). Rows carry their tails
    and overrides on a layout v7 engine (docs/ENGINE.md 125, tools/rowkit.py)."""
    import rowkit
    rk = rowkit.kind('mtg')
    trees = []
    for g, s in zip(games, seeds):
        t = g.search_tree(True, int(s))
        t.set_root_min(root_min)
        trees.append(t)
    rv = view == 'root'
    for _ in range(sims):
        rows, acts, rrs, owners = [], [], [], []
        for j, (g, t) in enumerate(zip(games, trees)):
            r = rowkit.simulate(t, g, 1.0, 0, rv, rk)
            if r is None:
                continue
            p, x, a, vx, rr, vrr = r
            rows.append(x)
            acts.append(list(a))
            rrs.append(rr)
            owners.append((j, len(rows) - 1, None if vx is None else len(rows), p))
            if vx is not None:
                rows.append(vx)
                acts.append([])
                rrs.append(vrr)
        if not rows:
            continue
        off = np.zeros(len(rows) + 1, np.int32)
        off[1:] = np.cumsum([len(a) for a in acts])
        pri, val, mar = rowkit.evaluate(ev, rows, acts, rk, rrs)[:3]
        for j, b, vb, p in owners:
            v = b if vb is None else vb
            if vb is not None and pairs is not None:
                # the viewer's P(win): column 1 of the mover's row (relative to the mover), column 0 of its own
                pairs.append((float(val[b][1]), float(val[vb][0])))
            trees[j].expand([float(x) for x in pri[off[b]:off[b + 1]]], [float(x) for x in val[v]],
                            [float(x) for x in mar[v]])
    return trees


def root_row(t):
    nv, w, p0, v0 = list(t.root_visits), list(t.root_values), list(t.root_priors0), t.root_value0
    q = [w[a] / nv[a] if nv[a] else None for a in range(len(nv))]
    return {'n': nv, 'q': q, 'p0': p0, 'v0': v0, 'acts': list(t.root_acts),
            'visits': int(max(range(len(nv)), key=lambda k: (nv[k], p0[k]))), 'rule': t.root_select(*RULE)}


# ------------------------------------------------------------------ mull

def cmd_mull(a):
    from mull_floor import hands
    ev = evaluator(a.model)
    items = hands(A.SPEC_DELVER, a.zero, a.one, a.control)
    games = [g for g, _, _ in items for _ in range(a.reps)]
    seeds = [7919 * i + 104729 * r + 1 for i in range(len(items)) for r in range(a.reps)]
    out = {'model': a.model, 'sims': a.sims, 'reps': a.reps, 'root_min': a.root_min, 'groups': {}, 'leaves': {}}
    rows = {}
    for view in ('mover', 'root'):
        t0 = time.time()
        pairs = []
        trees = search_all(games, ev, a.sims, view, seeds, a.root_min, pairs if view == 'root' else None)
        rows[view] = [dict(root_row(t), pos=k // a.reps, lands=items[k // a.reps][1], order=items[k // a.reps][2])
                      for k, t in enumerate(trees)]
        out[f'seconds_{view}'] = round(time.time() - t0, 1)
        if pairs:
            d = np.asarray([r - m for m, r in pairs])
            out['leaves'] = {'n': len(d), 'mean_root_minus_mover': float(d.mean()), 'mean_abs': float(np.abs(d).mean()),
                             'p90_abs': float(np.quantile(np.abs(d), 0.9)), 'share_over_0.05': float((np.abs(d) > 0.05).mean())}
    groups = [('0 lands', lambda L: L == 0), ('1 land', lambda L: L == 1), ('2-4 lands', lambda L: L >= 2)]
    for name, f in groups:
        for order in (None, 0, 1):
            key = name + ('' if order is None else (' first' if order == 0 else ' second'))
            g = {}
            for view in ('mover', 'root'):
                rs = [x for x in rows[view] if f(x['lands']) and (order is None or x['order'] == order)]
                if not rs:
                    continue
                qk = [x['q'][KEEP] for x in rs if x['q'][KEEP] is not None]
                qm = [x['q'][MULL] for x in rs if x['q'][MULL] is not None]
                g[view] = {'searches': len(rs), 'prior_mull': round(float(np.mean([x['p0'][MULL] for x in rs])), 3),
                           'value': round(float(np.mean([x['v0'] for x in rs])), 3),
                           'q_keep': round(float(np.mean(qk)), 3) if qk else None,
                           'q_mull': round(float(np.mean(qm)), 3) if qm else None,
                           'n_mull': round(float(np.mean([x['n'][MULL] for x in rs])), 2),
                           'keep_visits': round(1 - float(np.mean([x['visits'] == MULL for x in rs])), 3),
                           'keep_rule': round(1 - float(np.mean([x['rule'] == MULL for x in rs])), 3)}
            out['groups'][key] = g
            print(key, json.dumps(g), flush=True)
    print('leaves', json.dumps(out['leaves']), flush=True)
    if a.out:
        json.dump(dict(out, rows=rows), open(a.out, 'w'), indent=1)
    return out


# ------------------------------------------------------------------ position

def rebuild(rep, upto):
    """A zero-replay/1 game (zero.replay.RecordingDriver's seeding: the driver's Random(seed), the
    game's slot stream) after its first `upto` frames, each frame's action applied as the driver did
    (a transition: single-option decisions auto-stepped); checks the legal actions of every network
    frame on the way."""
    from zero import mcts
    from zero.games import engine
    E = engine()
    rng = random.Random(rep['seed'])
    for _ in range(rep['game_index']):
        rng.randrange(1 << 30)
    srng = random.Random(rng.randrange(1 << 30))
    srng.randrange(1 << 30)
    srng.randrange(1 << 30)
    g = E.Game(rep['game'], rep['spec'], 2, srng.getrandbits(63))
    for k, f in enumerate(rep['frames'][:upto]):
        if f['k'] == 'nn' and (g.legal() != f['acts'] or g.to_move != f['p']):
            raise SystemExit(f'frame {k}: the rebuilt game does not match the replay ({g.legal()} vs {f["acts"]})')
        mcts.transition(g, f['a'])
    return g


def cmd_position(a):
    import gzip
    op = gzip.open if a.replay.endswith('.gz') else open
    rep = json.load(op(a.replay, 'rt', encoding='utf-8'))
    g = rebuild(rep, a.frame)
    f = rep['frames'][a.frame]
    st = json.loads(g.export_state())
    names = {o['a']: o['t'] for o in st['decision']['options']}
    ev = evaluator(a.model)
    out = {'replay': os.path.basename(a.replay), 'frame': a.frame, 'recorded': f, 'options': names, 'runs': {}}
    for sims in [int(x) for x in a.sims.split(',')]:
        for view in ('mover', 'root'):
            pairs = []
            trees = search_all([g] * a.reps, ev, sims, view, [1000 + r for r in range(a.reps)], a.root_min, pairs)
            rr = [root_row(t) for t in trees]
            acts = rr[0]['acts']
            res = {'n': [round(float(np.mean([x['n'][i] for x in rr])), 1) for i in range(len(acts))],
                   'q': [round(float(np.mean([x['q'][i] for x in rr if x['q'][i] is not None])), 4)
                         if any(x['q'][i] is not None for x in rr) else None for i in range(len(acts))],
                   'v0': round(float(np.mean([x['v0'] for x in rr])), 4),
                   'rule_moves': {names.get(acts[i], acts[i]): sum(x['rule'] == i for x in rr) for i in range(len(acts))},
                   'visits_moves': {names.get(acts[i], acts[i]): sum(x['visits'] == i for x in rr) for i in range(len(acts))}}
            if pairs:
                d = np.asarray([r - m for m, r in pairs])
                res['leaves'] = {'n': len(d), 'root_minus_mover': round(float(d.mean()), 4), 'mean_abs': round(float(np.abs(d).mean()), 4)}
            out['runs'][f'{view} {sims}'] = res
            print(f'{view} {sims}', json.dumps(res), flush=True)
    # the pass's first leaf: the opponent's priority decision, both views (value-only rows, whole: tools/rowkit.py)
    import rowkit
    from zero import mcts
    rk = rowkit.kind('mtg')
    for i, act in enumerate(g.legal()):
        h = g.clone()
        mcts.transition(h, act)
        if h.over:
            continue
        me, mover = g.to_move, h.to_move
        (xm, rm), (xr, rr_) = rowkit.row(h, mover, rk), rowkit.row(h, me, rk)
        _, vm, _ = rowkit.evaluate(ev, [xm], [[]], rk, [rm])[:3]
        _, vr, _ = rowkit.evaluate(ev, [xr], [[]], rk, [rr_])[:3]
        mine = (lambda v, p: float(v[0][0] if p == me else v[0][1]))
        out.setdefault('after', {})[names.get(act, act)] = {'next_decider': mover, 'mover_view': round(mine(vm, mover), 4),
                                                            'own_view': round(float(vr[0][0]), 4)}
    print('after each option, the decider\'s P(win):', json.dumps(out.get('after')), flush=True)
    if a.out:
        json.dump(out, open(a.out, 'w'), indent=1)
    return out


# ------------------------------------------------------------------ self-play samples, calibration, fine-tune

def cmd_selfplay(a):
    from zero import games as G
    from zero.infer import drive
    from zero.protocol import concat_samples
    cfg = json.load(open(a.config)) if a.config else {}
    ev = evaluator(a.model)
    seat = G.nn_seat('m', a.sims, cfg.get('c_puct', 1.0), True, cfg.get('alpha', 0.3), cfg.get('eps', 0.25),
                     cfg.get('temp_moves', 60), a.fast or None, a.p_full, 0, 0.0, True,
                     cfg.get('policy_target', 'cq'), 50.0, cfg.get('cq_c_scale', 0.3), sims_minor=a.minor or None,
                     leaf_value_view=a.view, view_samples=1.0, view_sample_target='next_q')
    drv = G.Driver('mtg', [[seat, seat]] * a.games, spec=A.SPEC_DELVER, concurrency=min(a.games, a.conc), seed=a.seed,
                   record=True, threads=a.search_threads, groups=2, max_steps=20000)
    parts, order = [], []

    def take(lb=None):
        # (result rows and samples come out in the same order: a game's samples are consecutive)
        order.extend(drv.take_results())
        parts.append(drv.take_samples())
    t0 = time.time()
    drive(drv, {'m': ev}, on_batch=take)
    take()
    secs = time.time() - t0
    rows = sorted(order, key=lambda r: r['gid'])
    s = concat_samples([p for p in parts if p is not None and len(p['player'])])
    s['gid'] = np.repeat([r['gid'] for r in order], [r['samples'] for r in order]).astype(np.int64)
    assert len(s['gid']) == len(s['player'])
    st = drv.stats()
    np.savez_compressed(a.out, **{k: np.asarray(v) for k, v in s.items()})
    info = {'games': len(rows), 'samples': int(len(s['player'])), 'value_only': int((np.diff(s['opt_off']) == 0).sum()),
            'seconds': round(secs, 1), 'leaves': st['leaves'], 'view_rows': st['view_rows'],
            'leaves_per_s': round(st['leaves'] / secs, 1), 'rows_per_s': round((st['leaves'] + st['view_rows']) / secs, 1),
            'decisions_per_game': round(float(np.mean([r['decisions'] for r in rows])), 1), 'view': a.view,
            'p0_win': round(float(np.mean([r['result'][0] for r in rows])), 3), 'sims': [a.sims, a.fast, a.minor],
            'p_full': a.p_full}
    json.dump(info, open(a.out + '.json', 'w'), indent=1)
    print(json.dumps(info), flush=True)


def load_samples(paths):
    """Sample files of `selfplay` (comma-separated: concatenated, their games numbered apart) -> (samples,
    options per row)."""
    from zero.protocol import concat_samples
    parts, base = [], 0
    for p in paths.split(','):
        s = dict(np.load(p))
        s['gid'] = s['gid'] + base
        base = int(s['gid'].max()) + 1
        parts.append(s)
    gid = np.concatenate([p['gid'] for p in parts])
    s = concat_samples([{k: v for k, v in p.items() if k != 'gid'} for p in parts])
    s['gid'] = gid
    return s, np.diff(s['opt_off'])


def values(ev, s, rows):
    """The network's P(win) for each sample's own player (its row's view; the rows whole - their tails - on a layout
    v7 engine, as value-only rows: tools/rowkit.py)."""
    import rowkit
    rk = rowkit.kind('mtg')
    out = np.zeros(len(rows))
    B = 256
    for i in range(0, len(rows), B):
        r = rows[i:i + B]
        _, v, _ = ev.evaluate(np.ascontiguousarray(s['x'][r], np.float32), np.full(len(r), 2, np.int32),
                              np.zeros(len(r) + 1, np.int32), np.zeros(0, np.int32), None,
                              **rowkit.sample_kw(rk, s, r, options=False))
        out[i:i + len(r)] = v[:, 0]
    return out


def calib_report(p, z, q=None):
    p = np.clip(p, 1e-6, 1 - 1e-6)
    ll = float(-(z * np.log(p) + (1 - z) * np.log(1 - p)).mean())
    br = float(((p - z) ** 2).mean())
    bins = np.clip((p * 5).astype(int), 0, 4)
    rel = [{'bin': f'{k / 5:.1f}-{(k + 1) / 5:.1f}', 'n': int((bins == k).sum()),
            'pred': round(float(p[bins == k].mean()), 3) if (bins == k).any() else None,
            'won': round(float(z[bins == k].mean()), 3) if (bins == k).any() else None} for k in range(5)]
    ece = sum(abs(r['pred'] - r['won']) * r['n'] for r in rel if r['n']) / len(p)
    base = float(-(z * np.log(z.mean()) + (1 - z) * np.log(1 - z.mean())).mean()) if 0 < z.mean() < 1 else 0.0
    out = {'n': int(len(p)), 'log_loss': round(ll, 4), 'brier': round(br, 4), 'ece': round(float(ece), 4),
           'mean_pred': round(float(p.mean()), 3), 'mean_won': round(float(z.mean()), 3), 'log_loss_const': round(base, 4),
           'bins': rel}
    if q is not None:
        out['brier_vs_q'] = round(float(((p - q) ** 2).mean()), 4)
    return out


def split_rows(s, n, test, seed=0):
    """(train rows, test rows): a `test` share of the games held out, whole."""
    games = np.unique(s['gid'])
    rng = np.random.default_rng(seed)
    held = rng.choice(games, max(1, int(round(test * len(games)))), replace=False)
    te = np.isin(s['gid'], held)
    return np.flatnonzero(~te), np.flatnonzero(te)


def calib_all(ev, s, n, rows=None):
    rows = np.arange(len(n)) if rows is None else rows
    v = values(ev, s, rows)
    z = s['z'][rows, 0].astype(np.float64)
    q = s['q'][rows, 0].astype(np.float64)
    dec = n[rows] > 0
    out = {'decision_rows': calib_report(v[dec], z[dec], q[dec]), 'value_only_rows': calib_report(v[~dec], z[~dec], q[~dec])}
    # the pairs: the decider's P(win) from its view and from the other player's view (1 - its value)
    pos = {r: k for k, r in enumerate(rows)}
    d = []
    for k, r in enumerate(rows):
        if n[r] == 0 and (r - 1) in pos:
            d.append(v[pos[r - 1]] - (1 - v[k]))
    d = np.asarray(d)
    out['pairs'] = {'n': int(len(d)), 'decider_minus_other_view': round(float(d.mean()), 4) if len(d) else None,
                    'mean_abs': round(float(np.abs(d).mean()), 4) if len(d) else None,
                    'p90_abs': round(float(np.quantile(np.abs(d), 0.9)), 4) if len(d) else None}
    return out


def cmd_calib(a):
    s, n = load_samples(a.samples)
    ev = evaluator(a.model)
    out = calib_all(ev, s, n)
    print(json.dumps(out, indent=1))
    if a.out:
        json.dump(out, open(a.out, 'w'), indent=1)


def cmd_finetune(a):
    import torch
    import torch.nn.functional as F
    from zero.net import load, save
    s, n = load_samples(a.samples)
    tr, te = split_rows(s, n, a.split, a.seed)
    if a.decisions_only:
        tr = tr[n[tr] > 0]                      # the control: the decision rows alone (as training sees them now)
    m, extra = load(a.model, 'cpu', game='mtg')
    ev0 = evaluator(a.model)
    before = calib_all(ev0, s, n, te)
    m.train()
    opt = torch.optim.AdamW(m.parameters(), lr=a.lr, weight_decay=1e-4)
    A_ = m.config['actions']
    rng = np.random.default_rng(a.seed)
    off = s['opt_off']
    vq = a.value_q
    t0 = time.time()
    log = []
    for step in range(a.steps):
        idx = tr[rng.integers(len(tr), size=a.batch)]
        x = torch.from_numpy(s['x'][idx].astype(np.float32))
        npl = torch.full((len(idx),), 2, dtype=torch.int64)
        mask = np.zeros((len(idx), A_), bool)
        pi = np.zeros((len(idx), A_), np.float32)
        for b, r in enumerate(idx):
            acts = s['opt_action'][off[r]:off[r + 1]]
            mask[b, acts] = True
            pi[b, acts] = s['pi'][off[r]:off[r + 1]]
        pi /= np.maximum(pi.sum(1, keepdims=True), 1e-8)
        mask_t, pi_t = torch.from_numpy(mask), torch.from_numpy(pi)
        dec = mask_t.any(1).float()
        z = torch.from_numpy(s['z'][idx].astype(np.float32))
        q = torch.from_numpy(s['q'][idx].astype(np.float32))
        zt = ((1 - vq) * z + vq * q)[:, :m.seats]
        zt = zt / zt.sum(1, keepdim=True).clamp(min=1e-8)
        # (layout v7: the rows' tails and overrides, as a run's window gives them; tools/rowkit.py)
        import rowkit
        plog, vlog, _ = m(x, npl, **rowkit.train_kw(m, s, idx, x.numpy()))
        logp = F.log_softmax(plog.masked_fill(~mask_t, -1e9), 1)
        lp = (-(pi_t * logp).sum(1) * dec).sum() / dec.sum().clamp(min=1)
        lv = -(zt * F.log_softmax(vlog, 1).clamp(min=-1e4)).sum(1).mean()
        loss = lp + lv
        opt.zero_grad()
        loss.backward()
        torch.nn.utils.clip_grad_norm_(m.parameters(), 5.0)
        opt.step()
        if step % 25 == 0 or step == a.steps - 1:
            log.append({'step': step, 'policy': round(lp.item(), 4), 'value': round(lv.item(), 4)})
            print(json.dumps(log[-1]), flush=True)
    m.eval()
    save(m, a.out, extra)
    after = calib_all(evaluator(a.out), s, n, te)
    out = {'train_rows': int(len(tr)), 'test_rows': int(len(te)), 'decisions_only': a.decisions_only,
           'steps': a.steps, 'batch': a.batch, 'lr': a.lr,
           'value_q': vq, 'seconds': round(time.time() - t0, 1), 'log': log, 'before': before, 'after': after}
    for k in ('decision_rows', 'value_only_rows'):
        print(k, 'before', {kk: before[k][kk] for kk in ('log_loss', 'brier', 'ece', 'mean_pred', 'mean_won')},
              'after', {kk: after[k][kk] for kk in ('log_loss', 'brier', 'ece', 'mean_pred', 'mean_won')}, flush=True)
    print('pairs before', before['pairs'], 'after', after['pairs'])
    json.dump(out, open(a.out + '.json', 'w'), indent=1)


# ------------------------------------------------------------------ play: instants by step

def cmd_play(a):
    """Games of the network against itself (the line's evaluation rule, `--sims`), recorded by
    zero.replay's driver; per view the instants cast by the player whose turn it is, by step, and by
    the other player."""
    from mtgz.az.replays import _Recorder
    from zero.infer import drive
    from zero.play import NNSeat
    ev = evaluator(a.model)
    spec = json.loads(A.SPEC_DELVER)
    spec['log'] = True
    spec = json.dumps(spec, separators=(',', ':'))
    out = {}
    for view in a.view.split(','):
        seat = NNSeat('m', a.sims, impl='rs', select='cq', select_min_visits=4, cq_c_visit=50.0, cq_c_scale=0.3,
                      leaf_value_view=view)
        # (mtgz.az.replays' recorder: the minor decisions - mana payments - at sims_minor, as self-play)
        drv = type('Rec', (_Recorder,), {'sims_minor': a.minor})('mtg', a.games, lambda g: [seat, seat], spec=spec,
                                                                 concurrency=a.conc, seed=a.seed, max_steps=5000)
        t0 = time.time()
        drive(drv, {'m': ev})
        reps = drv.take_replays()
        own, other = {}, {}
        wins = []
        for r in reps:
            wins.append(r['result'][0] if r['result'] else 0.5)
            for f in r['frames']:
                if f['k'] != 'nn':
                    continue
                stt = r['states'][f['s']]
                opts = {o['a']: o['t'] for o in stt['decision']['options']}
                t = opts.get(f['a'], '')
                if not t.startswith('Cast '):
                    continue
                card = t[5:].split(' (')[0]
                if card not in INSTANTS:
                    continue
                step = stt.get('step', '?')
                d = own if f['p'] == stt.get('active') else other
                d.setdefault(card, {}).setdefault(step, 0)
                d[card][step] += 1
        out[view] = {'games': len(reps), 'seconds': round(time.time() - t0, 1), 'p0_score': round(float(np.mean(wins)), 3),
                     'own_turn': own, 'opponents_turn': other}
        print(view, json.dumps(out[view]), flush=True)
    if a.out:
        json.dump(out, open(a.out, 'w'), indent=1)


def cmd_speed(a):
    """Leaves per second of self-play's searches on this CPU, mover vs root view (the same games while
    the values do not decide the moves - here they do: so per leaf)."""
    from zero import games as G
    from zero.infer import drive
    ev = evaluator(a.model)
    out = {}
    for view in ('mover', 'root'):
        seat = G.nn_seat('m', 64, selfplay=True, sims_fast=16, p_full=0.25, sims_minor=4, policy_target='cq',
                         cq_c_scale=0.3, temp_moves=60, leaf_value_view=view)
        drv = G.Driver('mtg', [[seat, seat]] * a.games, spec=A.SPEC_DELVER, concurrency=a.games, seed=a.seed,
                       record=False, threads=a.search_threads, groups=2, max_steps=20000)
        t0 = time.time()
        ev.seconds, ev.calls, ev.leaves = 0.0, 0, 0
        n = [0]

        def stop(lb):
            n[0] += 1
            if time.time() - t0 > a.seconds:
                raise StopIteration
        try:
            drive(drv, {'m': ev}, on_batch=stop)
        except StopIteration:
            pass
        secs = time.time() - t0
        st = drv.stats()
        out[view] = {'seconds': round(secs, 1), 'leaves': st['leaves'], 'view_rows': st['view_rows'],
                     'leaves_per_s': round(st['leaves'] / secs, 1), 'rows_per_s': round((st['leaves'] + st['view_rows']) / secs, 1),
                     'infer_s': round(ev.seconds, 1), 'avg_batch_rows': round(ev.leaves / max(1, ev.calls), 1),
                     'view_rows_per_leaf': round(st['view_rows'] / max(1, st['leaves']), 3)}
        print(view, json.dumps(out[view]), flush=True)
    if a.out:
        json.dump(out, open(a.out, 'w'), indent=1)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    sub = ap.add_subparsers(dest='cmd', required=True)
    for name in ('mull', 'position', 'selfplay', 'calib', 'finetune', 'play', 'speed'):
        p = sub.add_parser(name)
        p.add_argument('--model', required=True, help='a COPY of the checkpoint (only read)')
        p.add_argument('--threads', type=int, default=4, help='torch threads')
        p.add_argument('--out', default='')
        p.add_argument('--seed', type=int, default=1)
        if name == 'mull':
            p.add_argument('--zero', type=int, default=50)
            p.add_argument('--one', type=int, default=100)
            p.add_argument('--control', type=int, default=50)
            p.add_argument('--sims', type=int, default=64)
            p.add_argument('--reps', type=int, default=2)
            p.add_argument('--root-min', type=int, default=0)
        if name == 'position':
            p.add_argument('--replay', required=True)
            p.add_argument('--frame', type=int, required=True)
            p.add_argument('--reps', type=int, default=16)
            p.add_argument('--sims', default='64')
            p.add_argument('--root-min', type=int, default=0)
        if name == 'selfplay':
            p.add_argument('--games', type=int, default=64)
            p.add_argument('--conc', type=int, default=64)
            p.add_argument('--sims', type=int, default=64)
            p.add_argument('--fast', type=int, default=16)
            p.add_argument('--minor', type=int, default=4)
            p.add_argument('--p-full', type=float, default=0.25)
            p.add_argument('--view', default='mover')
            p.add_argument('--config', default='')
            p.add_argument('--search-threads', type=int, default=4)
        if name in ('calib', 'finetune'):
            p.add_argument('--samples', required=True)
        if name == 'finetune':
            p.add_argument('--steps', type=int, default=300)
            p.add_argument('--batch', type=int, default=256)
            p.add_argument('--lr', type=float, default=1e-4)
            p.add_argument('--value-q', type=float, default=0.6)
            p.add_argument('--split', type=float, default=0.25)
            p.add_argument('--decisions-only', action='store_true', help='the control: no value-only rows in training')
        if name == 'play':
            p.add_argument('--games', type=int, default=24)
            p.add_argument('--conc', type=int, default=24)
            p.add_argument('--sims', type=int, default=64)
            p.add_argument('--view', default='mover,root')
            p.add_argument('--minor', type=int, default=4)
        if name == 'speed':
            p.add_argument('--games', type=int, default=64)
            p.add_argument('--seconds', type=float, default=120)
            p.add_argument('--search-threads', type=int, default=4)
    a = ap.parse_args(argv)
    low_priority(a.threads)
    {'mull': cmd_mull, 'position': cmd_position, 'selfplay': cmd_selfplay, 'calib': cmd_calib, 'finetune': cmd_finetune,
     'play': cmd_play, 'speed': cmd_speed}[a.cmd](a)


if __name__ == '__main__':
    main()
