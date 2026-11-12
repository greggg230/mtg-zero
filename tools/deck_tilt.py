"""The deck tilt's CPU evidence (docs/ENGINE.md 106.5): the search's deal of the opponent's unseen deck against
the true one, by tilt and by the prediction's source, on Edge of Eternities sealed positions.

  python tools/deck_tilt.py deals --model runs/<run>/models/iter_0004.pt [--positions 120] [--deals 64]

Positions: random play of `mtg_eoe` games on the spec's pools (turn 3 or later, one every ~20 decisions; the decider
the viewer). Per position: the network's prediction of the `deck` target from the decider's features - the auxiliary
head's (`NetEvaluator.aux`) and the deck reader's (`NetEvaluator.reader`) - and `--deals` worlds per arm (tilt 0: the
untilted K-sample deal, `determinize`; else `determinize_with` at `belief_tilts.deck` = the tilt, clip 20; the 'flat'
source is an untrained network's prediction, uniform over the 1,024 identities). Columns: the
cross-entropy per true unseen card of the worlds' mean unseen composition (1% of the untilted deal's mixed in), the
cards misplaced by it (half the L1 distance to the true composition), the distinct opponent decks among the worlds,
and the prediction's own cross-entropy per card. CPU only, below normal priority; reads only the checkpoint.
"""
import argparse
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
os.environ.setdefault('CUDA_VISIBLE_DEVICES', '-1')

import mtgz.az                         # noqa: E402,F401  (puts the pinned zero-core on sys.path)
import numpy as np                     # noqa: E402

D = 1024    # identities of each auxiliary target (docs/ENGINE.md 102.4)
SET_CARDS = 321    # the EoE space's card identities (docs/ENGINE.md 105.1)


def low_priority(threads):
    import torch
    torch.set_num_threads(threads)
    try:
        import psutil
        psutil.Process().nice(psutil.BELOW_NORMAL_PRIORITY_CLASS if os.name == 'nt' else 10)
    except Exception:                              # noqa: BLE001
        pass


def positions(spec, n, seed):
    """`n` (game, viewer) positions of random play: turn 3 or later, one every ~20 decisions, at most 6 a game."""
    from zero.games import engine
    E = engine()
    rng = np.random.default_rng(seed)
    out, s = [], 0
    while len(out) < n:
        g = E.Game('mtg_eoe', spec, 2, seed * 1000 + s)
        s += 1
        taken = 0
        while not g.over and len(out) < n and taken < 6:
            st = json.loads(g.export_state())
            if st.get('turn', 0) >= 3 and rng.integers(20) == 0:
                out.append(g.clone())
                taken += 1
            legal = list(g.legal())
            g.step(int(legal[rng.integers(len(legal))]))
    return out


def ce(mean, true):
    """Cross-entropy per true card of the composition `mean` (counts; 1% of a uniform over the set's cards mixed in, so
    a true card absent from every dealt deck costs ln(321 / 0.01) = 10.4 nats, not infinity) for the true counts `true`."""
    floor = np.zeros(D)
    floor[:SET_CARDS] = 1.0 / SET_CARDS
    q = 0.99 * mean / mean.sum() + 0.01 * floor
    k = true > 0
    return float(-(true[k] * np.log(np.maximum(q[k], 1e-300))).sum() / true.sum())


def cmd_deals(a):
    from zero.infer import NetEvaluator
    from zero.net import load
    low_priority(a.torch_threads)
    m, _ = load(a.model, 'cpu', game='mtg_eoe')
    ev = NetEvaluator(m, 'cpu')
    spec = a.spec or json.loads(open(os.path.join(os.path.dirname(os.path.dirname(a.model)), 'config.json')).read())['spec']
    # (one set of positions per effective-sample-size floor: the same games, the spec's deck_tilt_ess differing)
    ess = [None] + list(a.ess)
    pos = {e: positions(spec if e is None else json.dumps(dict(json.loads(spec), deck_tilt_ess=e)), a.positions, a.seed)
           for e in ess}
    arms = [('untilted', None, 0.0, None)] + [(f'{src}, tilt {t:g}', src, t, None) for src in ('flat', 'head', 'reader')
                                              for t in a.tilts]
    arms += [(f'{src}, tilt {t:g}, ESS floor {e:g} x K', src, t, e) for e in a.ess for src in ('flat', 'head', 'reader') for t in a.tilts]
    rows = {name: {'ce': [], 'mis': [], 'distinct': []} for name, *_ in arms}
    own = {'head': [], 'reader': [], 'flat': []}
    t0 = time.time()
    # (the rows whole: a game with variable-length rows' tails too, docs/ENGINE.md 125, tools/rowkit.py; mtg_eoe has none)
    import rowkit
    rk = rowkit.kind(m.config['game'])
    for pi, g in enumerate(pos[None]):
        v = g.to_move
        xr, rr = rowkit.row(g, v, rk)
        x = xr[None]
        n2 = np.full(1, 2, np.int64)
        tk = {'tail': rowkit.kw(rk, [rr])['tail']} if rk is not None and rk.widths else {}
        pred = {'head': np.asarray(ev.aux(x, n2, **tk)['deck'][0], np.float32),
                'reader': np.asarray(ev.reader(x, n2, **tk)['deck'][0], np.float32),
                'flat': np.full(D, 1.0 / D, np.float32)}          # (an untrained network's: uniform over the identities)
        true = np.asarray(g.aux_target(v), np.float64)[D:2 * D]
        if true.sum() < 1:
            continue
        for src in own:
            own[src].append(ce(pred[src] * true.sum(), true))
        for name, src, t, e in arms:
            ge = pos[e][pi]
            dealt, decks = [], set()
            for s in range(a.deals):
                w = ge.clone()
                if src is None:
                    w.determinize(v, 1000 * pi + s)
                else:
                    full = np.concatenate([np.full(D, 1.0 / D, np.float32), pred[src]])
                    w.determinize_with(v, 1000 * pi + s, full, 0.0, a.clip, {'deck': t})
                d = np.asarray(w.aux_target(v), np.float64)[D:2 * D]
                dealt.append(d)
                decks.add(d.tobytes())
            mean = np.mean(dealt, 0)
            r = rows[name]
            r['ce'].append(ce(mean, true))
            r['mis'].append(0.5 * float(np.abs(mean - true).sum()))
            r['distinct'].append(len(decks))
    n = len(rows['untilted']['ce'])
    unseen = np.mean([float(np.asarray(g.aux_target(g.to_move))[D:].sum()) for g in pos[None]])
    print(f'{n} positions ({unseen:.1f} unseen opponent cards on average), {a.deals} worlds an arm, {time.time() - t0:.0f} s; '
          f'model {a.model}')
    print('| deal | cross-entropy per card +- se | paired vs untilted | cards misplaced | distinct decks of %d |' % a.deals)
    print('|---|---|---|---|---|')
    c0 = np.asarray(rows['untilted']['ce'])
    for name, *_ in arms:
        r = rows[name]
        c = np.asarray(r['ce'])
        dlt = c - c0
        print(f'| {name} | {c.mean():.3f} +- {c.std() / np.sqrt(n):.3f} | {dlt.mean():+.3f} +- {dlt.std() / np.sqrt(n):.3f} | '
              f'{np.mean(r["mis"]):.2f} | {np.mean(r["distinct"]):.1f} |')
    for src, c in own.items():
        c = np.asarray(c)
        print(f'| the {src} source\'s own prediction | {c.mean():.3f} +- {c.std() / np.sqrt(n):.3f} | | | |')


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest='cmd', required=True)
    p = sub.add_parser('deals')
    p.add_argument('--model', required=True)
    p.add_argument('--spec', default=None, help="the game spec (default: the model's run's)")
    p.add_argument('--positions', type=int, default=120)
    p.add_argument('--deals', type=int, default=64)
    p.add_argument('--tilts', type=float, nargs='+', default=[0.5, 1.0])
    p.add_argument('--clip', type=float, default=20.0)
    p.add_argument('--ess', type=float, nargs='*', default=[],
                   help='more effective-sample-size floors (deck_tilt_ess) to compare, every source at each')
    p.add_argument('--seed', type=int, default=106)
    p.add_argument('--torch-threads', type=int, default=4)
    a = ap.parse_args(argv)
    {'deals': cmd_deals}[a.cmd](a)


if __name__ == '__main__':
    main()
