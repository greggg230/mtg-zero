"""The in-run matchup model of a constructed best-of-three mirror and sideboarding by its choice table (docs/ENGINE.md
168; the Delver best-of-three run pb3).

**What it predicts.** P(seat A wins a game) from the two 60s the game is played with - each as its copies minus the
registered main's for every entry of the (shared) 75 -, who is on the play, game 2 or 3, the match score and whether
each seat was dealt (docs/ENGINE.md 154, 168.1). The model is EoE run 2's pairwise evaluator (docs/ENGINE.md 111.1,
`mtgz.sealed.evaluator.PairNet`) on these inputs:

    z(A, B) = w . (dA - dB) + (dA U) W (dB U)^T + c_pd pd + c_g3 pd g3 + c_lead lead + c_dealt (dealt_A - dealt_B)
    W = M - M^T (skew-symmetric),  P(A wins) = sigmoid(z)

w is a per-copy strength (a Bradley-Terry deck strength over count deltas; identified up to a constant - the deltas
of a 60 sum to 0 - so only its differences mean anything: the report centres it at the main's average copy), U W U^T the matchup term (dim 0: none),
pd = +1 when A starts, lead = sign(A's game wins - B's) before the game, g3 = the game is a game 3. Antisymmetric by
construction: z(B, A) = -z(A, B) with the context mirrored, so P(A beats B) + P(B beats A) = 1 and one row per game
(A = seat 0) is the whole likelihood. Results only - no distillation from the network's value (the value head judges a
60 before the game barely better than play / draw: hub note value_pregame_check/report.md).

**Its data.** Every finished self-play game of the run's own processes (the best-of-three game log, docs/ENGINE.md
168.2: `<run>/games/games-<pid>.jsonl`, one line a game with both 60s, the starter, the score, the dealt flags and the
result). Dealt seats (p 0.85 per seat in pb3) give the unconfounded signal: their 60s are drawn at random. Remote
self-play (the Mac, the farm) does not log: their games are not in the data (a share of the games, the same spec).

**The rolling retrain** (run setting `matchup_every` N > 0; `mtgz.sealed.rolling`'s design): at the start of iteration
it, when b = it - 1 is a multiple of N, at least `matchup_min_window` iterations of the run's logged games exist and no
job runs, a background job (`python -m mtgz.az.matchup retrain RUN --iter b ...`: CPU only, below-normal priority)
fits the model on the window's games (the newest `matchup_window` iterations; the newest `matchup_val_iters` validate
the model's dimension and L2 against the play / draw baseline; recency half-life `matchup_half_life`), refits the
selected setting as an ensemble of 5 bootstrap fits on the whole window, computes the **choice table** and publishes
`<run>/matchup/v<b>.json` atomically (`index.jsonl` per version: the held-out numbers; `used.jsonl` per iteration).

**The choice table** (docs/ENGINE.md 168.3). At a sideboarding decision the decider's belief about the other seat's next
60 is the same in every state of one context - the other seat's 60 for the next game is dealt independently of what was
played (a dealt seat: its restricted constructed prior; a seat that chooses: the constructed prior, with game 3
independent of game 2 by the user's decision H3 = A) and nothing of the next game is seen. So the best 60 is a function
of the public context: the next game (2, 3), whether the decider sideboards first (it then chooses play or draw: its
score is the better side's; second, the worse side's - the chooser's choice), its lead (-1, 0, 1) and whether the
other seat is dealt. For each of the 24 contexts the job maximises F(A) = the ensemble's mean over the belief's worlds
of sigmoid(z(A, B)) over A's legal 60s: N worlds (`matchup_worlds`) drawn exactly from the constructed prior
(uniform over the swap count in the range, then uniform over the 60s with that count: `sample_prior`). The 60 moves
from the main one card at a time, only by swaps whose gain is clearly positive: the mean over the published ensemble's
bootstrap members (`matchup_members`) exceeds `matchup_swap_z` of their standard deviations - the members' spread is
the estimate's sampling spread -, the largest mean gain first, until none passes (`confident_climb`: the shrinkage
toward the main; `matchup_swap_z` 0 is the plain climb by the mean). A version publishes a table only when its model
beats play / draw on its held-out games by `matchup_table_min_gain` nats. (These guards and their defaults are
proposed, pending the user's approval: docs/ENGINE.md 168.6.) The table goes into the self-play spec (`"sideboard_table"`); every seat of the 75 that
is not dealt sideboards to its context's 60 by teacher decisions, recorded as policy targets (zero-core docs/search.md
3.19): the network's prior learns the table's choice.

Commands:

    python -m mtgz.az.matchup retrain RUN --iter B --from-iter LO [--val-iters 2] [--half-life 16] [--worlds 8000]
    python -m mtgz.az.matchup fit RUN|GAMEDIR [--iters LO-HI] [--val-iters 2] [--out DIR]   (a fit + report, no table)
    python -m mtgz.az.matchup report RUN                 the versions' held-out numbers and per-copy tables
    python -m mtgz.az.matchup census RUN [--iters LO-HI]  what the chosen-60 seats board (the game log)
"""
import argparse
import glob
import json
import math
import os
import re
import shutil
import subprocess
import sys
import time
import traceback

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
FORMAT = 'mtgz-matchup/1'
# the package's run settings (zero-core GamePackage.settings; registered by mtgz.az); all off by default
SETTINGS = {
    'matchup_every': 0,           # N > 0: retrain the matchup model every N iterations and sideboard by its table; 0: off
    'matchup_window': 32,         # a retrain reads at most the newest W iterations of the run's logged games
    'matchup_min_window': 4,      # ... and waits for this many
    'matchup_val_iters': 2,       # of them, the newest V validate (the model's selection)
    'matchup_half_life': 16.0,    # > 0: recency weighting (iterations); 0: none
    'matchup_threads': 4,         # the job's CPU threads
    'matchup_worlds': 8000,       # the belief's worlds per context when the table is computed
    # the guards (docs/ENGINE.md 168.3; proposed defaults, pending the user's approval):
    'matchup_table_min_gain': 0.005,  # a version publishes a table only when its held-out gain over play / draw (nats a
                                      # game) is at least this (the value head's pre-game margin)
    'matchup_swap_z': 2.0,        # the table moves from the main one card at a time, only by swaps whose bootstrap gain
                                  # exceeds this many standard errors (0: the model's plain argmax climb)
    'matchup_members': 20,        # the published ensemble's bootstrap fits (the swaps' standard errors come from them)
}
DIMS = (0, 2)
L2_W = (1e-3, 3e-3, 1e-2, 3e-2, 0.1, 0.3, 1.0)   # (strong L2 shrinks every copy's w to 0: the main's 60)
L2_INTER = (1e-3, 3e-3)
MEMBERS = 5                       # (the selection's held-out ensemble; the published one: matchup_members)
VERSION_RE = re.compile(r'^v(\d{4,})\.json$')
_JOBS = {}


def mdir(run):
    return os.path.join(run, 'matchup')


def _append(path, row):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, 'a', encoding='utf-8') as f:
        f.write(json.dumps(row) + '\n')


def _rows(path):
    if not os.path.exists(path):
        return []
    with open(path, encoding='utf-8') as f:
        return [json.loads(x) for x in f if x.strip()]


def versions(run):
    out = []
    for p in glob.glob(os.path.join(mdir(run), 'v*.json')):
        m = VERSION_RE.match(os.path.basename(p))
        if m:
            out.append((int(m.group(1)), p.replace(os.sep, '/')))
    return sorted(out)


# ----------------------------------------------------------------------------- the game log

def iteration_times(run):
    p = os.path.join(run, 'metrics.jsonl')
    out = []
    if os.path.exists(p):
        with open(p, encoding='utf-8') as f:
            for line in f:
                if line.strip():
                    m = json.loads(line)
                    if 'time' in m and 'iter' in m:
                        out.append((m['time'], m['iter']))
    return sorted(out)


def load_games(run, lo=None, hi=None, selfplay=True):
    """The best-of-three game log's finished games of `run` (a run directory, or a directory of .jsonl files) ->
    (games, spec lines by key). Each game gets 'iter' (the first metrics line at or after its time; None for an
    iteration in progress, or no metrics). `selfplay`: only games of a self-play spec (one with dealt decks; the run's
    evaluation spec never deals). Aborted games are left out."""
    import bisect
    d = os.path.join(run, 'games')
    files = sorted(glob.glob(os.path.join(d if os.path.isdir(d) else run, '*.jsonl')))
    times = iteration_times(run)
    tt = [t for t, _ in times]
    specs, games = {}, []
    for path in files:
        with open(path, encoding='utf-8') as f:
            for n, line in enumerate(f, 1):
                if not line.endswith('\n') or not line.strip():
                    continue                       # (a line being written)
                x = json.loads(line)
                if x.get('kind') == 'bo3-spec':
                    specs[x['spec']] = x
                    continue
                if x.get('kind') != 'bo3':
                    continue
                sp = specs.get(x['spec'])
                if sp is None:
                    raise ValueError(f'{path}:{n}: a game of spec {x["spec"]} before its spec line')
                if x.get('result') is None or (selfplay and not sp.get('dealt_decks')):
                    continue
                i = bisect.bisect_left(tt, x['time'])
                x['iter'] = times[i][1] if i < len(times) else None
                if lo is not None and (x['iter'] is None or x['iter'] < lo):
                    continue
                if hi is not None and (x['iter'] is None or x['iter'] > hi):
                    continue
                games.append(x)
    return games, specs


def the_75(specs, games):
    """The shared 75 ([[name, copies], ...] in list order) and the registered main's copies of the games' specs: one
    mirror (both seats the same list and main), the same in every spec; anything else refused."""
    keys = sorted({g['spec'] for g in games})
    if not keys:
        raise ValueError('no game')
    lists = {json.dumps(specs[k]['lists']) for k in keys}
    mains = {json.dumps(specs[k]['mains']) for k in keys}
    if len(lists) != 1 or len(mains) != 1:
        raise ValueError(f'the games play {len(lists)} lists / {len(mains)} mains: one mirror 75 only')
    sp = specs[keys[0]]
    if sp['lists'][0] != sp['lists'][1] or sp['mains'][0] != sp['mains'][1]:
        raise ValueError('not a mirror: the matchup model reads one 75 for both seats (docs/ENGINE.md 168)')
    return sp['lists'][0], sp['mains'][0]


def arrays(games, main):
    """The model's inputs: dA, dB (count deltas, games x E), ctx (games x 4: pd, pd g3, lead, dealt A - B), y (seat 0's
    result: 1, 0.5, 0), it (the iteration)."""
    import numpy as np
    m = np.asarray(main, dtype=np.float64)
    dA = np.asarray([g['decks'][0] for g in games], dtype=np.float64) - m
    dB = np.asarray([g['decks'][1] for g in games], dtype=np.float64) - m
    pd = np.asarray([1.0 if g['starter'] == 0 else -1.0 for g in games])
    g3 = np.asarray([1.0 if g['game'] == 3 else 0.0 for g in games])
    lead = np.asarray([float(np.sign(g['wins'][0] - g['wins'][1])) for g in games])
    dl = np.asarray([float(g['dealt'][0]) - float(g['dealt'][1]) for g in games])
    ctx = np.stack([pd, pd * g3, lead, dl], axis=1)
    y = np.asarray([g['result'][0] for g in games], dtype=np.float64)
    it = np.asarray([g['iter'] if g['iter'] is not None else -1 for g in games])
    return dA, dB, ctx, y, it


# ----------------------------------------------------------------------------- the model

def z_of(p, dA, dB, ctx):
    """The logit (numpy, float64) of member `p` ({'w', 'U', 'M', 'c'}) for rows dA, dB (n x E), ctx (n x 4)."""
    import numpy as np
    z = (dA - dB) @ p['w'] + ctx @ p['c']
    U = np.asarray(p['U'])
    if U.size:
        W = np.asarray(p['M']) - np.asarray(p['M']).T
        z = z + np.einsum('nd,de,ne->n', dA @ U, W, dB @ U)
    return z


def fit(dA, dB, ctx, y, wt, dim, l2_w, l2_inter, seed=0, pd_only=False, threads=4):
    """One fit by full-batch L-BFGS (torch, float64) of the weighted mean log loss + L2 -> member dict (numpy lists).
    `pd_only`: the play / draw baseline (c_pd, c_g3 only; no deck term)."""
    import numpy as np
    import torch
    torch.set_num_threads(max(1, threads))
    g = torch.Generator().manual_seed(seed)
    E = dA.shape[1]
    tA, tB, tc = (torch.as_tensor(a, dtype=torch.float64) for a in (dA, dB, ctx))
    ty, tw = torch.as_tensor(y, dtype=torch.float64), torch.as_tensor(wt, dtype=torch.float64)
    tw = tw / tw.sum()
    w = torch.zeros(E, dtype=torch.float64, requires_grad=not pd_only)
    c = torch.zeros(4, dtype=torch.float64, requires_grad=True)
    U = (0.1 * torch.randn(E, dim, generator=g, dtype=torch.float64)).requires_grad_(dim > 0)
    M = (0.1 * torch.randn(dim, dim, generator=g, dtype=torch.float64)).requires_grad_(dim > 0)
    mask = torch.tensor([1.0, 1.0, 0.0, 0.0], dtype=torch.float64) if pd_only else torch.ones(4, dtype=torch.float64)
    params = [c] + ([] if pd_only else [w]) + ([U, M] if dim > 0 else [])
    opt = torch.optim.LBFGS(params, lr=1.0, max_iter=500, tolerance_grad=1e-10, tolerance_change=1e-12,
                            history_size=50, line_search_fn='strong_wolfe')

    def logit():
        z = tc @ (c * mask)
        if not pd_only:
            z = z + (tA - tB) @ w
        if dim > 0:
            W = M - M.T
            z = z + torch.einsum('nd,de,ne->n', tA @ U, W, tB @ U)
        return z

    def closure():
        opt.zero_grad()
        z = logit()
        loss = (tw * torch.nn.functional.binary_cross_entropy_with_logits(z, ty, reduction='none')).sum()
        if not pd_only:
            loss = loss + l2_w * (w * w).sum()
        if dim > 0:
            loss = loss + l2_inter * ((U * U).sum() + (M * M).sum())
        loss = loss + 1e-6 * (c * c).sum()
        loss.backward()
        return loss

    opt.step(closure)
    return {'w': (w.detach().numpy() if not pd_only else np.zeros(E)).tolist(), 'c': (c * mask).detach().numpy().tolist(),
            'U': U.detach().numpy().tolist() if dim > 0 else [], 'M': M.detach().numpy().tolist() if dim > 0 else [],
            'dim': dim, 'l2_w': l2_w, 'l2_inter': l2_inter}


def logloss(members, dA, dB, ctx, y):
    """The per-game mean log loss of the members' mean probability (unweighted)."""
    import numpy as np
    p = np.mean([1.0 / (1.0 + np.exp(-np.asarray(z_of(_np(m), dA, dB, ctx)))) for m in members], axis=0)
    p = np.clip(p, 1e-12, 1 - 1e-12)
    return float(-np.mean(y * np.log(p) + (1 - y) * np.log(1 - p)))


def _np(m):
    import numpy as np
    return {k: (np.asarray(v, dtype=np.float64) if k in ('w', 'U', 'M', 'c') else v) for k, v in m.items()}


def weights(it, ref, half_life):
    import numpy as np
    if not half_life or half_life <= 0:
        return np.ones(len(it))
    return 0.5 ** ((ref - it) / half_life)


def select_and_fit(games, main, val_iters=2, half_life=16.0, threads=4, seed=0, log=print, members_n=MEMBERS):
    """The grid on the time split (the newest `val_iters` iterations validate), then the selected setting refit as an
    ensemble of MEMBERS bootstrap fits on the whole window -> (members, report)."""
    import numpy as np
    dA, dB, ctx, y, it = arrays(games, main)
    its = sorted(set(int(x) for x in it if x >= 0))
    if len(its) < val_iters + 1:
        raise ValueError(f'games of {len(its)} iterations: too few for {val_iters} validation iterations')
    vcut = its[-val_iters]
    tr, va = it < vcut, it >= vcut
    ref = its[-1]
    wtr = weights(it[tr], its[-val_iters - 1], half_life)
    base = fit(dA[tr], dB[tr], ctx[tr], y[tr], wtr, 0, 0.0, 0.0, seed=seed, pd_only=True, threads=threads)
    base_val = logloss([base], dA[va], dB[va], ctx[va], y[va])
    grid = []
    for dim in DIMS:
        for l2w in L2_W:
            for l2i in (L2_INTER if dim > 0 else (0.0,)):
                m = fit(dA[tr], dB[tr], ctx[tr], y[tr], wtr, dim, l2w, l2i, seed=seed, threads=threads)
                v = logloss([m], dA[va], dB[va], ctx[va], y[va])
                grid.append({'dim': dim, 'l2_w': l2w, 'l2_inter': l2i, 'val_logloss': v})
                log(f'# dim {dim} l2_w {l2w} l2_inter {l2i}: validation log loss {v:.5f} (play / draw only {base_val:.5f})')
    # (the lowest validation loss; within 1e-5 of it the simpler model - the smaller dimension, then the larger L2: a
    # matchup term held at its zero saddle scores as the strength alone, and must not be picked for a tie)
    lo_v = min(g['val_logloss'] for g in grid)
    best = min((g for g in grid if g['val_logloss'] <= lo_v + 1e-5), key=lambda g: (g['dim'], -g['l2_w'], -g['l2_inter']))
    # the selection's held-out ensemble (fit on the training iterations only): the yardstick's number
    held = []
    rng = np.random.default_rng(seed)
    for k in range(MEMBERS):
        ix = np.flatnonzero(tr)
        b = rng.choice(ix, size=len(ix), replace=True)
        held.append(fit(dA[b], dB[b], ctx[b], y[b], weights(it[b], its[-val_iters - 1], half_life), best['dim'],
                        best['l2_w'], best['l2_inter'], seed=seed + 1 + k, threads=threads))
    held_val = logloss(held, dA[va], dB[va], ctx[va], y[va])
    dva = np.asarray([games[i]['dealt'][0] or games[i]['dealt'][1] for i in np.flatnonzero(va)])
    held_val_dealt = logloss(held, dA[va][dva], dB[va][dva], ctx[va][dva], y[va][dva]) if dva.any() else None
    base_val_dealt = logloss([base], dA[va][dva], dB[va][dva], ctx[va][dva], y[va][dva]) if dva.any() else None
    # the published ensemble: the selected setting on the whole window
    members = []
    wall = weights(it, ref, half_life)
    for k in range(members_n):
        b = rng.choice(len(y), size=len(y), replace=True)
        members.append(fit(dA[b], dB[b], ctx[b], y[b], wall[b], best['dim'], best['l2_w'], best['l2_inter'],
                           seed=seed + 101 + k, threads=threads))
    # (w is identified up to a constant - every 60 has 60 cards, so the count deltas sum to 0: only differences of w
    # mean anything; the report centres each member's w at the main's average copy: a copy's worth against it)
    mn = np.asarray(main, dtype=np.float64)
    W = np.asarray([m['w'] for m in members])
    W = W - (W @ mn / mn.sum())[:, None]
    rep = {'games': int(len(y)), 'train_games': int(tr.sum()), 'val_games': int(va.sum()), 'iterations': its,
           'val_iterations': [i for i in its if i >= vcut], 'half_life': half_life, 'grid': grid, 'selected': best,
           'val_logloss': held_val, 'val_logloss_pd': base_val, 'gain_nats': base_val - held_val,
           'val_logloss_dealt': held_val_dealt, 'val_logloss_pd_dealt': base_val_dealt,
           'pd_baseline': base['c'][:2], 'per_copy': W.mean(0).tolist(), 'per_copy_se': W.std(0, ddof=1).tolist(),
           'ctx_names': ['pd', 'pd_g3', 'lead', 'dealt'], 'ctx': np.asarray([m['c'] for m in members]).mean(0).tolist()}
    return members, rep


# ----------------------------------------------------------------------------- the constructed prior

def prior_table(copies, main, lo, hi):
    """counts[i][a][b]: the number of count vectors of entries i.. with a cards in and b cards out (beyond / below the
    main), a, b <= hi (Python integers: exact)."""
    E = len(copies)
    S = hi
    t = [[[0] * (S + 1) for _ in range(S + 1)] for _ in range(E + 1)]
    t[E][0][0] = 1
    for i in range(E - 1, -1, -1):
        for a in range(S + 1):
            for b in range(S + 1):
                n = 0
                for d in range(copies[i] + 1):
                    ai, bi = max(0, d - main[i]), max(0, main[i] - d)
                    if ai <= a and bi <= b:
                        n += t[i + 1][a - ai][b - bi]
                t[i][a][b] = n
    return t


def _below(rng, n):
    """A uniform integer in [0, n) for a Python int n of any size (the prior's counts pass 2^63): 62-bit chunks, then
    rejection of the draws past n."""
    bits = n.bit_length()
    k = (bits + 61) // 62
    while True:
        r = 0
        for _ in range(k):
            r = (r << 62) | int(rng.integers(0, 1 << 62))
        r >>= k * 62 - bits
        if r < n:
            return r


def sample_prior(copies, main, lo, hi, n, rng):
    """n 60s of the constructed prior restricted to swaps lo..hi (docs/ENGINE.md 120.2, 154: uniform over the swap
    count, then uniform over the 60s of the 75 with that count) - exact: big-integer draws by the table's counts (they
    pass 2^63) -> (n x E) counts."""
    import numpy as np
    t = prior_table(copies, main, lo, hi)
    ss = [s for s in range(lo, hi + 1) if t[0][s][s] > 0]
    out = np.zeros((n, len(copies)), dtype=np.int64)
    for k in range(n):
        s = ss[int(rng.integers(len(ss)))]
        a = b = s
        for i in range(len(copies)):
            r = _below(rng, t[i][a][b])
            for d in range(copies[i] + 1):
                ai, bi = max(0, d - main[i]), max(0, main[i] - d)
                if ai <= a and bi <= b:
                    w = t[i + 1][a - ai][b - bi]
                    if r < w:
                        out[k, i] = d
                        a, b = a - ai, b - bi
                        break
                    r -= w
    return out


# ----------------------------------------------------------------------------- the choice table

def contexts():
    """The 24 contexts (game, first, lead, opp_dealt) in the engine's index order (sbtable::context_index)."""
    return [(g, f, l, o) for g in (2, 3) for f in (0, 1) for l in (-1, 0, 1) for o in (0, 1)]


class Objective:
    """F(A) for one context: the members' mean over the worlds B of sigmoid(z(A, B)); the decider sideboarding first
    takes the better of play and draw, second the worse (module docs)."""

    def __init__(self, members, worlds_delta, game, first, lead, opp_dealt):
        import numpy as np
        self.ms = [_np(m) for m in members]
        self.first = first
        g3 = 1.0 if game == 3 else 0.0
        self.parts = []
        for m in self.ms:
            sB = worlds_delta @ m['w']                       # B's strength (n)
            gB = None
            if m['U'].size:
                W = m['M'] - m['M'].T
                gB = (worlds_delta @ m['U']) @ W.T           # so that e(A) W e(B)^T = e(A) . gB
            ctx = {pd: pd * m['c'][0] + pd * g3 * m['c'][1] + lead * m['c'][2] + (0.0 - opp_dealt) * m['c'][3]
                   for pd in (1.0, -1.0)}
            self.parts.append((m, sB, gB, ctx))
        self.np = np

    def per_member(self, dA):
        """Each member's F of candidates dA (k x E) -> (members, k): the member's mean over the worlds, its better
        side when the decider sideboards first, its worse second."""
        np = self.np
        out = []
        for m, sB, gB, ctx in self.parts:
            sides = []
            for pd in (1.0, -1.0):
                z = (dA @ m['w'])[:, None] - sB[None, :] + ctx[pd]
                if gB is not None:
                    z = z + (dA @ m['U']) @ gB.T
                sides.append((1.0 / (1.0 + np.exp(-z))).mean(axis=1))
            out.append(np.maximum(sides[0], sides[1]) if self.first else np.minimum(sides[0], sides[1]))
        return np.asarray(out)

    def __call__(self, dA):
        """F of candidates dA (k x E) -> (k,): the members' mean."""
        return self.per_member(dA).mean(axis=0)


def strength_optimum(members, copies, main):
    """The legal 60 maximising the members' mean w . dA exactly: the 60 copies of the highest per-copy w (a copy of an
    entry is worth its w; ties to the main's copies, then the list's order)."""
    import numpy as np
    w = np.mean([m['w'] for m in members], axis=0)
    cands = []
    for i, n in enumerate(copies):
        for k in range(n):
            cands.append((-w[i], 0 if k < main[i] else 1, i))
    cands.sort()
    c = [0] * len(copies)
    for _, _, i in cands[:sum(main)]:
        c[i] += 1
    return c


def confident_climb(F, copies, main, z=2.0):
    """From the main, one-card swaps (a copy of an entry out, a copy of another in) whose gain in F is clearly positive:
    the members' mean gain over the current 60 exceeds z times its standard error - the members' sd, the members being
    bootstrap fits of the window, so their spread is the estimate's sampling spread -, the swap with the largest mean
    gain first, until none passes (z 0: a plain climb by the mean). The model's shrinkage toward the main: a card
    leaves it only on evidence. -> (counts, F, the swaps [(out, in, mean gain, se)])."""
    import numpy as np
    m = np.asarray(main, dtype=np.float64)
    cop = np.asarray(copies)
    c = np.asarray(main, dtype=np.int64)
    cur = F.per_member((c - m)[None, :])[:, 0]
    swaps = []
    while True:
        outs = np.flatnonzero(c > 0)
        ins = np.flatnonzero(c < cop)
        pairs, nb = [], []
        for o in outs:
            for i in ins:
                if o != i:
                    x = c.copy()
                    x[o] -= 1
                    x[i] += 1
                    nb.append(x)
                    pairs.append((int(o), int(i)))
        nb = np.asarray(nb)
        g = F.per_member(nb - m) - cur[:, None]                  # (members, candidates)
        mean = g.mean(axis=0)
        se = g.std(axis=0, ddof=1) if g.shape[0] > 1 else np.zeros_like(mean)
        ok = mean - z * se > 0 if z > 0 else mean > 1e-12
        if not ok.any():
            break
        j = int(np.flatnonzero(ok)[np.argmax(mean[ok])])
        c = nb[j]
        cur = cur + g[:, j]
        swaps.append((pairs[j][0], pairs[j][1], float(mean[j]), float(se[j])))
    return [int(x) for x in c], float(cur.mean()), swaps


def choice_table(members, names, copies, main, dealt_swaps, version, worlds=8000, seed=0, log=print, z=2.0):
    """The 24 contexts' best 60s (module docs) -> (the spec's "sideboard_table", per-context detail)."""
    import numpy as np
    rng = np.random.default_rng(seed)
    S = sum(copies) - sum(main)
    m = np.asarray(main, dtype=np.float64)
    beliefs = {0: sample_prior(copies, main, 0, S, worlds, rng) - m,
               1: sample_prior(copies, main, dealt_swaps[0], dealt_swaps[1], worlds, rng) - m}
    choices, detail = [], []
    strong = strength_optimum(members, copies, main)        # (the strength term's exact argmax, unguarded: reported)
    for (g, f, l, o) in contexts():
        F = Objective(members, beliefs[o].astype(np.float64), g, f, l, o)
        c, fv, sw = confident_climb(F, copies, main, z)
        choices.append([g, f, l, o, c])
        detail.append({'context': [g, f, l, o], 'deck': c, 'F': fv, 'F_main': float(F((np.asarray(main) - m)[None, :])[0]),
                       'swaps': [{'out': names[a], 'in': names[b], 'gain': gm, 'se': gs} for a, b, gm, gs in sw],
                       'z': z, 'argmax_in': {names[i]: strong[i] - main[i] for i in range(len(c)) if strong[i] > main[i]},
                       'in': {names[i]: c[i] - main[i] for i in range(len(c)) if c[i] > main[i]},
                       'out': {names[i]: main[i] - c[i] for i in range(len(c)) if c[i] < main[i]}})
        log(f'# context game {g} first {f} lead {l} opp_dealt {o}: F {fv:.4f} (main {detail[-1]["F_main"]:.4f}) '
            f'in {detail[-1]["in"]} out {detail[-1]["out"]}')
    table = {'version': version, 'list': [[n, k] for n, k in zip(names, copies)], 'choices': choices}
    return table, detail


def check_table(spec, table):
    """The engine parses the spec with the table (`mtgz._engine.mtg_spec_check`): a malformed table fails here, in the
    job, never in self-play."""
    from mtgz import _engine
    sp = json.loads(spec)
    sp['sideboard_table'] = table
    _engine.mtg_spec_check(json.dumps(sp))


# ----------------------------------------------------------------------------- the self-play hook

def with_table(spec, table):
    sp = json.loads(spec)
    sp['sideboard_table'] = table
    return json.dumps(sp, separators=(',', ':'))


def selfplay_spec(R, it):
    """zero-core's `selfplay_specs` hook for `mtg` (through `mtgz.az.decklists.selfplay_spec`; module docs): iteration
    `it`'s self-play spec - the run's with the choice table of the newest version published before the iteration first
    asked - or None (the run's own: `matchup_every` 0, or no version yet). Every call for one iteration answers the
    same (`used.jsonl`: the first answer is kept), so the local driver, the Mac's workers and the farm's definition of
    the iteration's checkpoint play one spec."""
    c = R.cfg
    every = int(c.get('matchup_every', 0) or 0)
    if every <= 0:
        return None
    sp = json.loads(c['spec'])
    if not (sp.get('constructed') and sp.get('format') == 'bo3' and sp.get('dealt_decks')):
        raise ValueError('matchup_every needs a constructed best-of-three self-play spec with dealt decks (docs/ENGINE.md 168)')
    if 'sideboard_table' in sp:
        raise ValueError('matchup_every: the run\'s spec has a sideboard_table of its own (the retrain writes it)')
    run = R.run
    used = [r for r in _rows(os.path.join(mdir(run), 'used.jsonl')) if r['iter'] == it]
    if used:
        v = used[-1]['version']
        t = None if v is None else _load(os.path.join(mdir(run), v)).get('table')
        return None if t is None else with_table(c['spec'], t)
    start = os.path.join(mdir(run), 'start.json')
    if not os.path.exists(start):
        os.makedirs(mdir(run), exist_ok=True)
        with open(start, 'w', encoding='utf-8') as f:
            json.dump({'first_iter': it, 'time': time.time()}, f)
    poll(R)
    launch_if_due(R, it)
    vs = versions(run)
    prev = (_rows(os.path.join(mdir(run), 'used.jsonl')) or [{}])[-1].get('version')
    if not vs:
        _append(os.path.join(mdir(run), 'used.jsonl'), {'iter': it, 'version': None, 'time': time.time()})
        return None
    name = os.path.basename(vs[-1][1])
    table = _load(vs[-1][1]).get('table')
    _append(os.path.join(mdir(run), 'used.jsonl'), {'iter': it, 'version': name, 'table': table is not None,
                                                    'time': time.time()})
    if prev != name:
        R.log(f'iter {it} self-play: the chosen-60 seats sideboard by the matchup model\'s choice table {name}' if table
              is not None else f'iter {it} self-play: the matchup model {name} has no table (it did not beat play / draw '
              f'on its held-out games by matchup_table_min_gain): the chosen-60 seats sideboard by search')
    return None if table is None else with_table(c['spec'], table)


def _load(path):
    with open(path, encoding='utf-8') as f:
        return json.load(f)


def job_dir(run, b):
    return os.path.join(mdir(run), 'work', f'v{b:04d}')


def _alive(pid, b):
    try:
        import psutil
        p = psutil.Process(int(pid))
        cmd = p.cmdline()
        return p.status() != 'zombie' and 'mtgz.az.matchup' in cmd and 'retrain' in cmd and str(b) in cmd
    except Exception:                              # noqa: BLE001 (no such process)
        return False


def running(R):
    j = _JOBS.get(R.run)
    if j is not None and j[1].poll() is None:
        return j[0]
    for d in glob.glob(os.path.join(mdir(R.run), 'work', 'v*')):
        b = int(os.path.basename(d)[1:])
        if os.path.exists(os.path.join(d, 'status.json')) or (j is not None and j[0] == b):
            continue
        pf = os.path.join(d, 'pid')
        if os.path.exists(pf):
            with open(pf) as f:
                pid = f.read().strip()
            if pid and _alive(pid, b):
                return b
    return None


def poll(R):
    """Report every finished job once (a job dir with neither a status nor a live process: failed, loudly)."""
    if running(R) is not None:
        return
    _JOBS.pop(R.run, None)
    for d in sorted(glob.glob(os.path.join(mdir(R.run), 'work', 'v*'))):
        if os.path.exists(os.path.join(d, 'reported')):
            continue
        name = os.path.basename(d)
        st = os.path.join(d, 'status.json')
        if os.path.exists(st):
            s = _load(st)
        else:
            s = {'ok': False, 'error': 'the job ended without a status (killed?); its log: ' + os.path.join(d, 'log.txt')}
        if s.get('ok'):
            R.log(f'matchup model {name} published: {s.get("games")} games of iterations {s.get("window")}, held-out '
                  f'log loss {s.get("val_logloss"):.5f} vs play / draw {s.get("val_logloss_pd"):.5f} '
                  f'(gain {s.get("gain_nats"):+.5f} nats){"" if s.get("table", True) else ": no table"}, {s.get("seconds")} s')
        else:
            keep = versions(R.run)
            kept = os.path.basename(keep[-1][1]) if keep else 'no table (the seats sideboard by search)'
            R.log(f'MATCHUP RETRAIN FAILED {name}: {str(s.get("error"))[-600:]} - keeping {kept}')
        with open(os.path.join(d, 'reported'), 'w') as f:
            f.write(str(time.time()))


def launch_if_due(R, it):
    c = R.cfg
    every = int(c['matchup_every'])
    W, V = int(c.get('matchup_window', 32)), int(c.get('matchup_val_iters', 2))
    M = int(c.get('matchup_min_window', 4))
    if not 1 <= V < M <= W:
        raise ValueError(f'matchup_val_iters {V} < matchup_min_window {M} <= matchup_window {W}: not so')
    b = it - 1
    first = _load(os.path.join(mdir(R.run), 'start.json'))['first_iter']
    lo = max(first, b - W + 1)
    if b % every or b - first + 1 < M or any(v == b for v, _ in versions(R.run)) or os.path.isdir(job_dir(R.run, b)):
        return
    r = running(R)
    if r is not None:
        R.log(f'iter {it} matchup model: the retrain of v{r:04d} is still running; v{b:04d} skipped')
        return
    d = job_dir(R.run, b)
    os.makedirs(d, exist_ok=True)
    th = int(c.get('matchup_threads', 4))
    cmd = [sys.executable, '-m', 'mtgz.az.matchup', 'retrain', R.run, '--iter', str(b), '--from-iter', str(lo),
           '--val-iters', str(V), '--half-life', str(float(c.get('matchup_half_life', 0.0) or 0.0)),
           '--threads', str(th), '--worlds', str(int(c.get('matchup_worlds', 8000))),
           '--min-gain', str(float(c.get('matchup_table_min_gain', 0.005))), '--swap-z', str(float(c.get('matchup_swap_z', 2.0))),
           '--members', str(int(c.get('matchup_members', 20)))]
    from mtgz.sealed.rolling import background_flags, job_env
    log = open(os.path.join(d, 'log.txt'), 'w', encoding='utf-8')
    p = subprocess.Popen(cmd, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL,
                         env=job_env(th), **background_flags())
    log.close()
    with open(os.path.join(d, 'pid'), 'w') as f:
        f.write(str(p.pid))
    _JOBS[R.run] = (b, p)
    R.log(f'iter {it} matchup model: the retrain of v{b:04d} started (the games of iterations {lo}-{b}, pid {p.pid}, {d})')


# ----------------------------------------------------------------------------- the retrain job

def retrain(run, b, lo, val_iters=2, half_life=16.0, threads=4, worlds=8000, log=print, min_gain=0.005, z=2.0,
            members_n=20):
    """The job (module docs): the run's logged self-play games of iterations lo..b -> status (status.json; a failure
    raises after writing it)."""
    t0 = time.time()
    d = job_dir(run, b)
    os.makedirs(d, exist_ok=True)
    name = f'v{b:04d}'
    status = {'ok': False, 'version': name, 'iter': b, 'window': [lo, b], 'half_life': half_life, 'started': t0}
    try:
        cfg = _load(os.path.join(run, 'config.json'))
        games, specs = load_games(run, lo, b)
        status['games'] = len(games)
        log(f'# {len(games)} games of iterations {lo}-{b}')
        if not games:
            raise RuntimeError(f'no logged self-play game in iterations {lo}-{b} (the game log: {os.path.join(run, "games")})')
        lst, main = the_75(specs, games)
        names = [x[0] for x in lst]
        copies = [int(x[1]) for x in lst]
        members, rep = select_and_fit(games, main, val_iters, half_life, threads, seed=b, log=log, members_n=members_n)
        dd = json.loads(cfg['spec']).get('dealt_decks') or {}
        sw = tuple(dd.get('swaps') or (0, sum(copies) - sum(main)))
        # (a model that does not beat play / draw on the held-out games by `min_gain` nats has not shown matchup
        # knowledge: its version publishes no table, and the chosen-60 seats sideboard by search until one does -
        # docs/ENGINE.md 168.4)
        if rep['gain_nats'] >= min_gain:
            table, detail = choice_table(members, names, copies, main, sw, name, worlds=worlds, seed=b, log=log, z=z)
            check_table(cfg['spec'], table)
        else:
            table, detail = None, []
            log(f'# held-out gain {rep["gain_nats"]:+.5f} nats < {min_gain}: no table in {name}')
        out = {'format': FORMAT, 'version': name, 'iter': b, 'window': [lo, b], 'list': lst, 'main': main,
               'members': members, 'report': rep, 'table': table, 'table_detail': detail, 'worlds': worlds,
               'min_gain': min_gain, 'swap_z': z,
               'dealt_swaps': list(sw), 'time': time.time()}
        staged = os.path.join(d, f'{name}.json')
        with open(staged, 'w', encoding='utf-8') as f:
            json.dump(out, f)
        dst = os.path.join(mdir(run), f'{name}.json')
        tmp = dst + f'.{os.getpid()}.tmp'
        shutil.copyfile(staged, tmp)
        os.replace(tmp, dst)
        status.update(ok=True, val_logloss=rep['val_logloss'], val_logloss_pd=rep['val_logloss_pd'],
                      gain_nats=rep['gain_nats'], selected=rep['selected'], table=table is not None,
                      seconds=round(time.time() - t0, 1))
        _append(os.path.join(mdir(run), 'index.jsonl'), {
            'version': name, 'iter': b, 'window': [lo, b], 'games': rep['games'], 'val_games': rep['val_games'],
            'selected': rep['selected'], 'val_logloss': rep['val_logloss'], 'val_logloss_pd': rep['val_logloss_pd'],
            'gain_nats': rep['gain_nats'], 'val_logloss_dealt': rep['val_logloss_dealt'],
            'val_logloss_pd_dealt': rep['val_logloss_pd_dealt'], 'per_copy': dict(zip(names, rep['per_copy'])),
            'table': table is not None,
            'table_changes': [{'context': x['context'], 'in': x['in'], 'out': x['out']} for x in detail], 'time': time.time()})
    except Exception as e:                         # noqa: BLE001 (recorded, then raised: the run logs it loudly)
        status.update(ok=False, error=f'{type(e).__name__}: {e}', traceback=traceback.format_exc()[-3000:],
                      seconds=round(time.time() - t0, 1))
        with open(os.path.join(d, 'status.json'), 'w', encoding='utf-8') as f:
            json.dump(status, f, indent=1)
        raise
    with open(os.path.join(d, 'status.json'), 'w', encoding='utf-8') as f:
        json.dump(status, f, indent=1)
    return status


# ----------------------------------------------------------------------------- yardsticks

def per_copy_lines(names, rep):
    rows = sorted(zip(names, rep['per_copy'], rep['per_copy_se']), key=lambda r: -r[1])
    return [f'  {n:<55} {w:+.4f} +- {se:.4f}' for n, w, se in rows]


def census(run, lo=None, hi=None):
    """What the chosen-60 seats board (the game log): per mode ('table', 'chosen') and game, the seats' 60s as changes
    against the main - each card's mean copies in / out, and the most common 60s."""
    from collections import Counter
    games, specs = load_games(run, lo, hi)
    if not games:
        return {'games': 0}
    lst, main = the_75(specs, games)
    names = [x[0] for x in lst]
    out = {}
    for g in games:
        for p in (0, 1):
            mode = g['mode'][p]
            if mode not in ('table', 'chosen'):
                continue
            k = f'{mode} game {g["game"]}'
            o = out.setdefault(k, {'seats': 0, 'decks': Counter(), 'in': Counter(), 'out': Counter()})
            o['seats'] += 1
            c = g['decks'][p]
            ch = tuple(sorted((names[i], c[i] - main[i]) for i in range(len(c)) if c[i] != main[i]))
            o['decks'][ch] += 1
            for i in range(len(c)):
                if c[i] > main[i]:
                    o['in'][names[i]] += c[i] - main[i]
                elif c[i] < main[i]:
                    o['out'][names[i]] += main[i] - c[i]
    res = {}
    for k, o in sorted(out.items()):
        n = o['seats']
        res[k] = {'seats': n, 'in': {x: round(v / n, 3) for x, v in o['in'].most_common()},
                  'out': {x: round(v / n, 3) for x, v in o['out'].most_common()},
                  'top': [{'changes': dict(ch), 'share': round(v / n, 3)} for ch, v in o['decks'].most_common(5)]}
    return {'games': len(games), 'by': res}


def main(argv=None):
    ap = argparse.ArgumentParser(prog='python -m mtgz.az.matchup', description=__doc__.split('\n\n')[0])
    sub = ap.add_subparsers(dest='cmd', required=True)
    r = sub.add_parser('retrain')
    r.add_argument('run')
    r.add_argument('--iter', type=int, required=True)
    r.add_argument('--from-iter', type=int, required=True)
    r.add_argument('--val-iters', type=int, default=2)
    r.add_argument('--half-life', type=float, default=16.0)
    r.add_argument('--threads', type=int, default=4)
    r.add_argument('--worlds', type=int, default=8000)
    r.add_argument('--min-gain', type=float, default=0.005)
    r.add_argument('--swap-z', type=float, default=2.0)
    r.add_argument('--members', type=int, default=20)
    f = sub.add_parser('fit')
    f.add_argument('run')
    f.add_argument('--iters', default=None, help='LO-HI')
    f.add_argument('--val-iters', type=int, default=2)
    f.add_argument('--half-life', type=float, default=16.0)
    f.add_argument('--threads', type=int, default=4)
    f.add_argument('--table', action='store_true', help='also compute the choice table')
    f.add_argument('--worlds', type=int, default=4000)
    f.add_argument('--swap-z', type=float, default=2.0)
    f.add_argument('--members', type=int, default=20)
    f.add_argument('--out', default=None)
    p = sub.add_parser('report')
    p.add_argument('run')
    c = sub.add_parser('census')
    c.add_argument('run')
    c.add_argument('--iters', default=None, help='LO-HI')
    a = ap.parse_args(argv)
    rng = (lambda s: (None, None) if not s else tuple(int(x) for x in s.split('-')))
    if a.cmd == 'retrain':
        def log(m):
            print(m, flush=True)
        s = retrain(a.run, a.iter, a.from_iter, a.val_iters, a.half_life, a.threads, a.worlds, log=log, min_gain=a.min_gain,
                    z=a.swap_z, members_n=a.members)
        print(json.dumps({k: s[k] for k in ('version', 'games', 'val_logloss', 'val_logloss_pd', 'gain_nats', 'seconds')}))
    elif a.cmd == 'fit':
        lo, hi = rng(a.iters)
        games, specs = load_games(a.run, lo, hi)
        lst, main_ = the_75(specs, games)
        names = [x[0] for x in lst]
        members, rep = select_and_fit(games, main_, a.val_iters, a.half_life, a.threads, members_n=a.members)
        print(f'{rep["games"]} games ({rep["train_games"]} train, {rep["val_games"]} validation of iterations '
              f'{rep["val_iterations"]}); selected {rep["selected"]}')
        print(f'held-out log loss {rep["val_logloss"]:.5f} vs play / draw only {rep["val_logloss_pd"]:.5f}: gain '
              f'{rep["gain_nats"]:+.5f} nats a game (the value head\'s bar: +0.005)')
        print('per copy (w: logit per copy beyond the main, ensemble mean +- sd):')
        print('\n'.join(per_copy_lines(names, rep)))
        out = {'report': rep, 'members': members, 'list': lst, 'main': main_}
        if a.table:
            S = sum(int(x[1]) for x in lst) - sum(main_)
            out['table'], out['table_detail'] = choice_table(members, names, [int(x[1]) for x in lst], main_, (0, S),
                                                             'fit', worlds=a.worlds, z=a.swap_z)
        if a.out:
            os.makedirs(a.out, exist_ok=True)
            with open(os.path.join(a.out, 'matchup_fit.json'), 'w', encoding='utf-8') as fh:
                json.dump(out, fh, indent=1)
    elif a.cmd == 'report':
        for row in _rows(os.path.join(mdir(a.run), 'index.jsonl')):
            print(f'{row["version"]}: {row["games"]} games of {row["window"]}, held-out {row["val_logloss"]:.5f} vs '
                  f'play / draw {row["val_logloss_pd"]:.5f} (gain {row["gain_nats"]:+.5f} nats), selected {row["selected"]}')
            for ch in row['table_changes']:
                print(f'   context {ch["context"]}: in {ch["in"]} out {ch["out"]}')
    elif a.cmd == 'census':
        lo, hi = rng(a.iters)
        print(json.dumps(census(a.run, lo, hi), indent=1))


if __name__ == '__main__':
    main()
