"""How much of the opponent's hand the Delver network can read (docs/ENGINE.md 66.6): the belief head on
held-out positions against the baselines.

  python tools/belief_eval.py --ckpt sp/models/iter_0000.pt --train sp/data/iter_0001 \
      --heldout ho/data/iter_0001 [--joint sp/models/iter_0001.pt] [--epochs 12] \
      [--out report.json] [--head-out belief_head.pt] [--examples 8]

The shards must hold the belief target (`aux`: self-play with `aux_belief_w` > 0, the true hand of the
decider's opponent by identity). Measured on the held-out rows whose opponent holds a card:

* head-only: the checkpoint's trunk frozen (its pooled state z, the value head's input, computed once
  per row), an auxiliary head of zero-core's shape (2d -> 128 -> 1024, net.add_aux) fitted on the train
  rows with the multinomial loss (Adam, `--epochs` passes, `--lr`); written with the trunk as a
  checkpoint (`--head-out`: the network of --ckpt plus the head, every other weight exactly --ckpt's);
* joint: `--joint`, a checkpoint whose head was trained with the trunk (the run loop's training with
  aux_belief_w on, e.g. a fork's next iteration), evaluated as is;
* uniform: what the search's determinize deals today - the opponent's known hand cards plus its other
  hand cards uniform over the cards the decider has not seen (`mtgz.az.belief.uniform_prior`, from the
  features: the hidden-pool bag and the hand size);
* count-matching: the uniform prior with one weight per card identity, fitted on the train rows so that
  each identity's mean predicted count matches its mean count in the hands (iterative proportional
  fitting): what a static per-card bias - players keep Force of Will, play their lands - explains
  without reading the position.

Metrics: cross-entropy per card in hand (nats; -sum_k t_k / n log p_k), the expected-count error
(0.5 sum_k |n p_k - t_k|: cards misplaced), top-1 (the most likely card is in the hand), and per key card
the AUC of "holds at least one" and the mean predicted vs actual count. Examples: the held-out
positions where the head's Force of Will + Daze most exceeds the uniform prior, decoded from the
features (turn, whose turn and step, the opponent's lands and untapped lands, hand size, stack).
"""
import argparse
import glob
import json
import os
import sys
import time

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import mtgz.az  # noqa: E402,F401  (registers the game package)
import torch  # noqa: E402

from mtgz.az import belief as B  # noqa: E402
from zero.data import read_shard, shard_paths  # noqa: E402
from zero.net import add_aux, load, save  # noqa: E402

KEY = ['Force of Will', 'Daze', 'Lightning Bolt', 'Brainstorm', 'Ponder', 'Unholy Heat', 'Murktide Regent',
       'Volcanic Island', 'Wasteland', "Mishra's Bauble"]
STEPS = ['setup', 'untap', 'upkeep', 'draw', 'main 1', 'begin combat', 'attackers', 'blockers', 'first strike',
         'damage', 'end combat', 'main 2', 'end', 'cleanup']


def load_rows(dirs, max_rows=None):
    """x float16 [N, F], aux float32 [N, D], shard index [N] of the shards in `dirs` (only shards with
    aux; a game's rows are consecutive in its shard, so a split by shard is a split by game but for
    the games a shard boundary cuts)."""
    xs, ts, ks = [], [], []
    for d in dirs:
        paths = shard_paths(d) if os.path.isdir(d) else sorted(glob.glob(d))
        for p in paths:
            s = read_shard(p)
            if 'aux' not in s:
                continue
            xs.append(s['x'])
            ts.append(s['aux'].astype(np.float32))
            ks.append(np.full(len(s['x']), len(ks)))
    x, t, k = np.concatenate(xs), np.concatenate(ts), np.concatenate(ks)
    if max_rows and len(x) > max_rows:
        x, t, k = x[:max_rows], t[:max_rows], k[:max_rows]
    return x, t, k


@torch.no_grad()
def trunk_z(model, x, batch=256):
    """The pooled trunk state z (the value head's input) of rows `x`."""
    out = []
    grab = {}
    h = model.val.register_forward_hook(lambda m, i, o: grab.__setitem__('z', i[0]))
    try:
        for s in range(0, len(x), batch):
            xb = torch.from_numpy(x[s:s + batch].astype(np.float32))
            model(xb, torch.full((len(xb),), 2))
            out.append(grab['z'].float().clone())
    finally:
        h.remove()
    return torch.cat(out)


def logits(head, z, base=None):
    o = head(z)
    return o if base is None else o + base


def ce(head, z, t, base=None):
    with torch.no_grad():
        logp = torch.log_softmax(logits(head, z, base), 1)
        return float(-(t / t.sum(1, keepdim=True) * logp).sum(1).mean())


def fit_head(head, z, t, val, epochs, lr, wd, patience, batch=512, seed=0, log=print, base=None, vbase=None):
    """AdamW on the multinomial loss of rows with a non-empty hand; early stopping on `val` (z, t: other
    games' rows): the epoch with the lowest validation cross-entropy is kept. `base` / `vbase`: fixed
    logits the head's output is added to (the residual head over a baseline). -> (head, log rows)."""
    keep = t.sum(1) > 0
    z, t = z[keep], torch.from_numpy(t[keep])
    base = None if base is None else base[torch.from_numpy(keep)]
    vz, vt = val
    vk = vt.sum(1) > 0
    vz, vt = vz[vk], torch.from_numpy(vt[vk])
    vbase = None if vbase is None else vbase[torch.from_numpy(vk)]
    opt = torch.optim.AdamW(head.parameters(), lr=lr, weight_decay=wd)
    g = torch.Generator().manual_seed(seed)
    n = len(z)
    best, best_state, bad, hist = float('inf'), None, 0, []
    for ep in range(epochs):
        perm = torch.randperm(n, generator=g)
        tot = 0.0
        for s in range(0, n, batch):
            i = perm[s:s + batch]
            logp = torch.log_softmax(logits(head, z[i], None if base is None else base[i]), 1)
            tt = t[i]
            loss = -(tt / tt.sum(1, keepdim=True) * logp).sum(1).mean()
            opt.zero_grad()
            loss.backward()
            opt.step()
            tot += loss.item() * len(i)
        v = ce(head, vz, vt, vbase)
        hist.append([ep + 1, round(tot / n, 4), round(v, 4)])
        log(f'  head epoch {ep + 1}/{epochs}: train cross-entropy {tot / n:.4f}, validation {v:.4f}')
        if v < best - 1e-4:
            best, bad = v, 0
            best_state = {k: x.clone() for k, x in head.state_dict().items()}
        else:
            bad += 1
            if bad >= patience:
                break
    head.load_state_dict(best_state)
    return head, hist


def count_matching(prior, t, iters=60):
    """Per-identity weights w so that sum_rows n p_k = sum_rows t_k, p_k proportional to w_k prior_k (train rows)."""
    n = t.sum(1, keepdims=True)
    keep = n[:, 0] > 0
    prior, t, n = prior[keep], t[keep], n[keep]
    q = prior / np.maximum(prior.sum(1, keepdims=True), 1e-12)
    w = np.ones(prior.shape[1])
    want = t.sum(0)
    for _ in range(iters):
        p = q * w
        p /= np.maximum(p.sum(1, keepdims=True), 1e-12)
        got = (n * p).sum(0)
        w = np.where(got > 0, w * np.where(want > 0, want, 1e-6) / np.maximum(got, 1e-12), w)
    return w


def apply_weights(prior, w):
    p = prior * w
    return p / np.maximum(p.sum(1, keepdims=True), 1e-12)


def auc(score, label):
    """Area under the ROC curve (ties counted half)."""
    pos, neg = score[label], score[~label]
    if not len(pos) or not len(neg):
        return None
    order = np.argsort(np.r_[pos, neg], kind='stable')
    ranks = np.empty(len(order))
    allv = np.r_[pos, neg][order]
    i = 0
    while i < len(allv):
        j = i
        while j + 1 < len(allv) and allv[j + 1] == allv[i]:
            j += 1
        ranks[order[i:j + 1]] = (i + j) / 2 + 1
        i = j + 1
    return float((ranks[:len(pos)].sum() - len(pos) * (len(pos) + 1) / 2) / (len(pos) * len(neg)))


def metrics(p, t):
    """p: distributions [N, D], t: counts [N, D] (rows with n > 0)."""
    n = t.sum(1)
    ce = -(t / n[:, None] * np.log(np.maximum(p, 1e-9))).sum(1)
    err = 0.5 * np.abs(n[:, None] * p - t).sum(1)
    top = t[np.arange(len(t)), p.argmax(1)] > 0
    nm = B.names()
    cards = {}
    for c in KEY:
        k = nm.index(c)
        e = n * p[:, k]
        cards[c] = {'pred': round(float(e.mean()), 4), 'true': round(float(t[:, k].mean()), 4),
                    'auc': None if auc(e, t[:, k] > 0) is None else round(auc(e, t[:, k] > 0), 4)}
    return {'rows': int(len(t)), 'cross_entropy': round(float(ce.mean()), 4), 'cross_entropy_se': round(float(ce.std() / np.sqrt(len(ce))), 4),
            'count_error': round(float(err.mean()), 4), 'top1': round(float(top.mean()), 4), 'cards': cards}


def describe(x):
    """A readable line about a position from its features (layout v5)."""
    g = x.astype(np.float32)
    turn = int(round(50 * g[0]))
    step = STEPS[int(np.argmax(g[1:15]))] if g[1:15].max() > 0 else '?'
    mine = 'my' if g[15] > 0.5 else "their"
    lands, untapped = int(round(10 * g[24 + 40 + 20])), int(round(10 * g[24 + 40 + 21]))
    my_lands, my_untapped = int(round(10 * g[24 + 20])), int(round(10 * g[24 + 21]))
    n = int(round(10 * g[24 + 40 + 1]))
    stack = int(round(8 * g[20]))
    return (f'turn {turn}, {mine} {step}; they: {lands} lands ({untapped} untapped), {n} cards in hand; '
            f'me: {my_lands} lands ({my_untapped} untapped); stack {stack}')


def strata(x, T, preds, cards=('Force of Will', 'Daze', 'Lightning Bolt', 'Brainstorm')):
    """Mean true vs predicted counts of key cards by whose turn it is and the opponent's untapped lands."""
    g = x.astype(np.float32)
    mine = g[:, 15] > 0.5
    unt = np.rint(10 * g[:, 24 + 40 + 21]).astype(int)
    n = T.sum(1)
    nm = B.names()
    out = []
    for who, wm in (('my turn', mine), ('their turn', ~mine)):
        for lab, um in (('0 untapped', unt == 0), ('1 untapped', unt == 1), ('2+ untapped', unt >= 2)):
            m = wm & um
            if m.sum() < 50:
                continue
            row = {'stratum': f'{who}, they {lab}', 'rows': int(m.sum()), 'cards': {}}
            for c in cards:
                k = nm.index(c)
                row['cards'][c] = dict(true=round(float(T[m, k].mean()), 3),
                                       **{name: round(float((n[m] * p[m, k]).mean()), 3) for name, p in preds.items()})
            out.append(row)
    return out


def top_cards(v, k=6):
    return ', '.join(f'{nm} {c:.2f}' for nm, c in B.counts_by_name(v, k))


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('--ckpt', required=True, help='the network whose trunk is frozen (no head needed)')
    ap.add_argument('--train', nargs='+', required=True, help='shard dirs (or globs) with the belief target')
    ap.add_argument('--heldout', nargs='+', required=True)
    ap.add_argument('--joint', default=None, help='a checkpoint with a jointly trained head')
    ap.add_argument('--epochs', type=int, default=40)
    ap.add_argument('--lr', type=float, default=1e-3)
    ap.add_argument('--wd', type=float, default=0.01)
    ap.add_argument('--patience', type=int, default=3)
    ap.add_argument('--val-share', type=float, default=0.2, help='the last shards of --train: early stopping')
    ap.add_argument('--max-train', type=int, default=None)
    ap.add_argument('--examples', type=int, default=8)
    ap.add_argument('--out', default=None)
    ap.add_argument('--head-out', default=None, help='write --ckpt + the fitted head as a checkpoint')
    ap.add_argument('--threads', type=int, default=4)
    a = ap.parse_args(argv)
    torch.set_num_threads(a.threads)
    t0 = time.time()
    xtr, ttr, ktr = load_rows(a.train, a.max_train)
    xho, tho, _ = load_rows(a.heldout)
    print(f'{len(xtr)} train rows, {len(xho)} held-out rows ({time.time() - t0:.0f}s)')
    model, _ = load(a.ckpt)
    model.eval()
    lay = mtgz.az.PACKAGE.aux_of('mtg')
    t1 = time.time()
    ztr, zho = trunk_z(model, xtr), trunk_z(model, xho)
    print(f'trunk states {tuple(ztr.shape)} + {tuple(zho.shape)} ({time.time() - t1:.0f}s)')
    hm = add_aux(model, lay, seed=1)
    head = hm.aux_head
    head.train()
    t2 = time.time()
    cut = np.quantile(ktr, 1 - a.val_share)
    fit = ktr < cut
    val = (ztr[torch.from_numpy(~fit)], ttr[~fit])
    print(f'head fit on {int(fit.sum())} rows (shards < {cut:g}), early stopping on {int((~fit).sum())}')
    head, hist = fit_head(head, ztr[torch.from_numpy(fit)], ttr[fit], val, a.epochs, a.lr, a.wd, a.patience)
    head.eval()
    print(f'head fitted ({time.time() - t2:.0f}s)')
    if a.head_out:
        save(hm, a.head_out, {'iter': 0, 'belief_head_from': os.path.abspath(a.ckpt), 'epochs': a.epochs})
        print(f'wrote {a.head_out}')
    keep = tho.sum(1) > 0
    T = tho[keep]
    with torch.no_grad():
        p_head = torch.softmax(head(zho[torch.from_numpy(keep)]), 1).numpy().astype(np.float64)
    prior_tr = B.uniform_prior(xtr).astype(np.float64)
    prior = B.uniform_prior(xho[keep]).astype(np.float64)
    p_uni = prior / np.maximum(prior.sum(1, keepdims=True), 1e-12)
    w = count_matching(prior_tr[fit], ttr[fit].astype(np.float64))
    p_cm = apply_weights(prior, w)
    # the residual head: count-matching's logits + a fresh head on the frozen trunk (its last layer zero:
    # it starts exactly at count-matching), early-stopped as the head-only one: does z add anything a
    # static per-card bias does not?
    def cm_logits(pr):
        return torch.from_numpy(np.log(np.maximum(apply_weights(pr, w), 1e-12)).astype(np.float32))
    res = add_aux(model, lay, seed=2).aux_head
    res.train()
    btr = cm_logits(prior_tr)
    res, rhist = fit_head(res, ztr[torch.from_numpy(fit)], ttr[fit], val, a.epochs, a.lr, a.wd, a.patience,
                          base=btr[torch.from_numpy(fit)], vbase=btr[torch.from_numpy(~fit)])
    res.eval()
    with torch.no_grad():
        p_res = torch.softmax(logits(res, zho[torch.from_numpy(keep)], cm_logits(prior)), 1).numpy().astype(np.float64)
    rep = {'ckpt': os.path.abspath(a.ckpt), 'train_rows': int((ttr.sum(1) > 0).sum()), 'heldout_rows': int(keep.sum()),
           'heldout_known_share': round(float((B.public(xho[keep])[2].sum(1) > 0).mean()), 4),
           'heldout_cloud_share': round(float((B.public(xho[keep])[3].sum(1) > 0).mean()), 4),
           'uniform': metrics(p_uni, T), 'count_matching': metrics(p_cm, T), 'head_only': metrics(p_head, T),
           'residual': metrics(p_res, T),
           'head_fit': {'epochs': hist, 'residual_epochs': rhist, 'lr': a.lr, 'wd': a.wd, 'fit_rows': int(fit.sum()),
                        'val_rows': int((~fit).sum())}}
    nm = B.names()
    rep['count_matching_weights'] = {nm[k]: round(float(w[k]), 3) for k in np.argsort(-w) if ttr[:, k].sum() > 0}
    if a.joint:
        jm, _ = load(a.joint)
        from zero.infer import NetEvaluator
        ev = NetEvaluator(jm)
        pj = np.concatenate([ev.aux(xho[keep][s:s + 256].astype(np.float32), np.full(min(256, keep.sum() - s), 2))['belief']
                             for s in range(0, int(keep.sum()), 256)]).astype(np.float64)
        rep['joint'] = metrics(pj, T)
        rep['joint_ckpt'] = os.path.abspath(a.joint)
    preds = {'uniform': p_uni, 'count_matching': p_cm, 'head_only': p_head, 'residual': p_res}
    if a.joint:
        preds['joint'] = pj
    rep['strata'] = strata(xho[keep], T, preds)
    for row in rep['strata']:
        print(f"  {row['stratum']:28s} rows {row['rows']:6d}  " + '  '.join(
            f"{c.split()[0]} true {v['true']:.2f} " + ' '.join(f"{m[:3]} {v[m]:.2f}" for m in preds)
            for c, v in row['cards'].items()))
    for k in ('uniform', 'count_matching', 'head_only', 'residual', 'joint'):
        if k in rep:
            r = rep[k]
            print(f'{k:15s} CE {r["cross_entropy"]:.4f} +- {r["cross_entropy_se"]:.4f}  count error {r["count_error"]:.3f}  '
                  f'top-1 {r["top1"]:.3f}  ' + '  '.join(f'{c.split()[0]} {v["pred"]:.2f}/{v["true"]:.2f} auc {v["auc"]}'
                                                       for c, v in list(r['cards'].items())[:3]))
    # examples: hands of at most 7 cards where the head's Force of Will + Daze most exceed the uniform
    # prior (one per distinct position and hand: consecutive decisions of a game repeat them)
    n = T.sum(1)
    fd = [nm.index('Force of Will'), nm.index('Daze')]
    gain = np.where(n <= 7, (n[:, None] * (p_head[:, fd] - p_uni[:, fd])).sum(1), -np.inf)
    ex, seen = [], set()
    xk = xho[keep]
    for i in np.argsort(-gain):
        if len(ex) >= a.examples or not np.isfinite(gain[i]):
            break
        hand = (tuple(np.flatnonzero(T[i])), tuple(T[i][T[i] > 0]))
        if hand in seen:
            continue
        seen.add(hand)
        ex.append({'position': describe(xk[i]), 'head': top_cards(n[i] * p_head[i]),
                   'residual': top_cards(n[i] * p_res[i]), 'count_matching': top_cards(n[i] * p_cm[i]),
                   'uniform': top_cards(n[i] * p_uni[i]), 'true': top_cards(T[i], 10),
                   'fow_daze_gain': round(float(gain[i]), 3)})
    rep['examples'] = ex
    for e in ex:
        print(f'- {e["position"]}\n    head:     {e["head"]}\n    residual: {e["residual"]}\n'
              f'    count-m.: {e["count_matching"]}\n    uniform:  {e["uniform"]}\n    true:     {e["true"]}')
    if a.out:
        with open(a.out, 'w', encoding='utf-8') as f:
            json.dump(rep, f, indent=1)
        print(f'wrote {a.out}')


if __name__ == '__main__':
    main()
