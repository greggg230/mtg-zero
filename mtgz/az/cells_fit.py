"""The metagame cells' joint rating fit and reference pool (docs/ENGINE.md 186, over mtgz.az.yardsticks' cells: 177).

Every cells game ever recorded in a run - a checkpoint against a reference ('cells'), against the previous checkpoint
('cells_gen'), a reference against itself (the cached 'cells_base' games) - is one observation of one model, fitted
per unordered deck pair {X, Y} (X before Y in cells_decks), over ALL of the pair's games at once:

    logit P(X's side wins) = m_XY + a[g, X|Y] - a[h, Y|X] + c_XY s

for generation g piloting X against generation h piloting Y; s = +1 when X's side chose play or draw (the chooser spec
0: X is seat 0), -1 when Y's did. a[g, X|Y] is g's rating in cell X|Y, m_XY the matchup's offset, c_XY the chooser's
edge (every measurement plays each seed from both sides, so the design balances it; fitting it keeps the ratings the
average over the choice and takes its variance out). The anchor - the run's first cells reference, generation 0 - has
a[anchor, .] = 0, so m_XY is identified by the anchor against itself (its baseline games) and every rating is Elo
over generation 0 in that cell, against any opponent (the model is additive in logits). A mirror {X, X}: m = 0 and one
rating a[g, X|X]. A draw counts half a win (the binomial log-likelihood with a fractional outcome). Elo = a x 400/ln 10.

The estimate: maximum likelihood with a weak Gaussian prior N(0, PRIOR_SD^2) on every parameter (PRIOR_SD = 4 logits,
~695 Elo: it moves an identified rating by well under 1 Elo and only keeps a cell at 100% finite), by Newton's method.

The standard errors: a bootstrap over seed units (CLUSTERS) - the default and the reported `se`. A cluster is one deal:
(the deck pair, the match's seed, the game id) - every game ever played on that deal, by any generations, from either
side of the choice, is in it. The design reuses the same seeds for every generation and reference (common random
numbers), so games on one deal are correlated: within a unit (the two choices), across the checkpoint's and the
reference's games (the pairing that makes a cell's gain precise), and across generations measured on the same deals.
The inverse Hessian (`se_model`, also reported) treats every game as independent: it overstates the se of a contrast
that the pairing makes precise and can understate one the shared deals make noisy. Resampling whole deals with
replacement and refitting keeps every correlation; pairs are fitted independently (disjoint seeds), so replicate b of
every pair together is a replicate of the whole fit (means over cells of different pairs, the 9-cell overall).

Read by `run_yardsticks.py cells-fit RUN`, `cells RUN --fit`, and in the pool mode (`cells_mode` 'pool') by the watcher:
the fit's point estimates choose each checkpoint's most informative reference (`plan`), and a 'cells_fit' row per
measured checkpoint records the fit (`fit_row`). numpy only inside the functions (yardsticks is light at import).
"""
import math
import re
import time

from . import yardsticks as Y

FIT_V = 1
PRIOR_SD = 4.0                 # logits: the weak prior on every parameter (stated in each record's method)
BOOT_SEED = 186                # the bootstrap's seed (with the pair's): a refit of the same rows gives the same se
FLAG_Z = 2.5                   # the transitivity flag: |observed - predicted| > 2.5 se
STALL_SE = 2.0                 # the reading rule: a step's gain within 2 se is no improvement
K = Y.ELO_K


def _np():
    import numpy as np
    return np


# ------------------------------------------------------------------------------------------------ the games
def _clusters(pair, seed, batch, n):
    """The deals of a chooser spec's game ids 0..n-1: (the match's seed, the game id in it). Sequential cells
    (docs/ENGINE.md 182): batch k is its own match, seeded seed + 7919 k, its game ids from 0 (`batch`: games a cell
    per batch; a mirror spec plays half of them)."""
    if batch:
        per = int(batch) // 2 if pair[0] == pair[1] else int(batch)
        return [(int(seed) + Y.SEQ_SEED * (i // per), i % per) for i in range(n)]
    return [(int(seed), i) for i in range(n)]


def match_games(pair, a, b, results, seed, batch, src):
    """A pair's two chooser matches of A (`a`) against B (`b`; the same for a baseline) -> [(gx, gy, s, y, deal,
    src)]: who piloted X (seat 0) and Y, s (+1: X's side chose), X's score. A in seat gid % 2 (177.1)."""
    r = {int(c): Y.res_decode(v) for c, v in results.items()}
    n = min(len(r.get(0, [])), len(r.get(1, [])))
    deals = _clusters(pair, seed, batch, n)
    out = []
    for c in (0, 1):
        s = 1 if c == 0 else -1
        for g in range(n):
            y = float(r[c][g])
            if g % 2 == 0:
                out.append((a, b, s, y, deals[g], src))
            else:
                out.append((b, a, s, 1.0 - y, deals[g], src))
    return out


_BASES = {}


def _load_base(cdir, key):
    import os
    p = Y.base_file(cdir, key)
    try:
        st = os.stat(p)
    except OSError:
        return None
    k = (p, st.st_mtime, st.st_size)
    if k not in _BASES:
        _BASES[k] = Y.load_base(cdir, key)
    return _BASES[k]


def collect(run, ys, rows):
    """Every recorded game of the run's cells -> {'games': {pair: [game]}, 'sources': [{kind, iter, ref, sha,
    ref_sha, pair, role?}], 'names': {sha: {'iter', 'labels'}}, 'measured': {sha}, 'missing_bases': [keys]}. The
    result rows' own games (the newest row of each measurement, as tally keeps) and the baselines' from the cache
    files their rows (or the cells rows) name - ladder-mode rows are as good as pool-mode ones."""
    res, _ = Y.tally(rows)
    games, sources, names, measured, bases = {}, [], {}, set(), []

    def name(sha, it=None, label=None):
        e = names.setdefault(sha, {'iter': None, 'labels': []})
        if it is not None and e['iter'] is None:
            e['iter'] = int(it)
        if label and label not in e['labels']:
            e['labels'].append(label)

    for k, r in sorted(res.items(), key=lambda kv: (kv[1].get('time') or 0, str(kv[0]))):
        if r['kind'] in ('cells', 'cells_gen') and r.get('results') and r.get('sha') and r.get('ref_sha'):
            pair = tuple(r['pair'])
            src = len(sources)
            sources.append({'kind': r['kind'], 'iter': r.get('iter'), 'ref': r['ref'], 'sha': r['sha'],
                            'ref_sha': r['ref_sha'], 'pair': pair, 'role': r.get('role'),
                            'games': {c: s.get('games') for c, s in (r.get('cells') or {}).items()},
                            'cells': r.get('cells') or {}})
            games.setdefault(pair, []).extend(match_games(pair, r['sha'], r['ref_sha'], r['results'], r['seed'],
                                                          r.get('batch'), src))
            name(r['sha'], r.get('iter'))
            measured.add(r['sha'])
            m = re.fullmatch(r'iter_(\d+)', str(r['ref']))
            name(r['ref_sha'], int(m.group(1)) if m else None, None if m else r['ref'])
            if r.get('base'):
                bases.append(r['base'])
        elif r['kind'] == 'cells_base' and r.get('base'):
            bases.append(r['base'])
            if r.get('ref_sha'):
                m = re.fullmatch(r'iter_(\d+)', str(r.get('ref')))
                name(r['ref_sha'], int(m.group(1)) if m else None, None if m else r.get('ref'))
    cdir = Y.cache_dir(run, ys)
    missing = []
    for key in dict.fromkeys(bases):
        d = _load_base(cdir, key)
        if d is None or not d.get('results') or not d.get('ref') or 'seed' not in d:
            missing.append(key)
            continue
        pair = tuple(d['pair'])
        src = len(sources)
        sources.append({'kind': 'cells_base', 'iter': None, 'ref': None, 'sha': d['ref'], 'ref_sha': d['ref'],
                        'pair': pair, 'base': key})
        games.setdefault(pair, []).extend(match_games(pair, d['ref'], d['ref'], d['results'], d['seed'],
                                                      d.get('batch'), src))
        name(d['ref'])
    return {'games': games, 'sources': sources, 'names': names, 'measured': measured, 'missing_bases': missing}


# ------------------------------------------------------------------------------------------------ the fit
def _newton(X, n, sy, th, prec, tol=1e-9, iters=100):
    """The penalized log-likelihood's maximum from `th` (Newton with step halving; the objective is concave) ->
    (theta, the negative Hessian at it)."""
    np = _np()
    eye = np.eye(X.shape[1])

    def obj(t):
        eta = X @ t
        return float(sy @ eta - n @ np.logaddexp(0.0, eta) - 0.5 * prec * (t @ t))

    def curv(t):
        p = 0.5 * (1.0 + np.tanh(0.5 * (X @ t)))
        return p, (X.T * (n * p * (1.0 - p))) @ X + prec * eye

    f = obj(th)
    for _ in range(iters):
        p, H = curv(th)
        g = X.T @ (sy - n * p) - prec * th
        step = np.linalg.solve(H, g)
        t = 1.0
        while True:
            nt = th + t * step
            nf = obj(nt)
            if nf >= f - 1e-12 or t < 1e-8:
                break
            t *= 0.5
        th, f = nt, nf
        if float(np.max(np.abs(t * step), initial=0.0)) < tol:
            break
    return th, curv(th)[1]


def fit_pair(games, pair, anchor, boot=0, seed=0, prior_sd=PRIOR_SD, exclude=None, start=None):
    """One deck pair's fit over `games` (match_games' tuples; `exclude`: sources left out) -> {'pair', 'mirror',
    'anchor', 'idx' {'m' | 'c' | (role, sha): column}, 'theta', 'cov' (the inverse Hessian), 'reps' (boot x params,
    or None), 'count' {(role, sha): games against another generation}, 'games', 'clusters', 'players'}. Roles: 'X' (a
    rating in cell pair[0]|pair[1]; a mirror's only one), 'Y' (pair[1]|pair[0])."""
    np = _np()
    mirror = pair[0] == pair[1]
    gs = [g for g in games if not exclude or g[5] not in exclude]
    players = sorted({g[0] for g in gs} | {g[1] for g in gs})
    others = [p for p in players if p != anchor]
    names = ([] if mirror else ['m']) + ['c']
    ry = 'X' if mirror else 'Y'
    names += [(role, p) for role in (('X',) if mirror else ('X', 'Y')) for p in others]
    idx = {k: i for i, k in enumerate(names)}
    combos, clusters = {}, {}
    ci = np.empty(len(gs), dtype=np.int64)
    cl = np.empty(len(gs), dtype=np.int64)
    y = np.empty(len(gs))
    for i, (gx, gy, s, yy, deal, _src) in enumerate(gs):
        ci[i] = combos.setdefault((gx, gy, s), len(combos))
        cl[i] = clusters.setdefault(deal, len(clusters))
        y[i] = yy
    X = np.zeros((len(combos), len(names)))
    for (gx, gy, s), k in combos.items():
        if not mirror:
            X[k, idx['m']] = 1.0
        X[k, idx['c']] = float(s)
        if gx != anchor:
            X[k, idx[('X', gx)]] += 1.0
        if gy != anchor:
            X[k, idx[(ry, gy)]] -= 1.0
    n = np.bincount(ci, minlength=len(combos)).astype(float)
    sy = np.bincount(ci, weights=y, minlength=len(combos))
    count = {}
    for (gx, gy, s), k in combos.items():
        if gx != gy:
            count[('X', gx)] = count.get(('X', gx), 0) + int(n[k])
            count[(ry, gy)] = count.get((ry, gy), 0) + int(n[k])
    prec = 1.0 / prior_sd ** 2
    th0 = np.zeros(len(names))
    if start is not None:
        for kk, i in idx.items():
            j = start['idx'].get(kk)
            if j is not None:
                th0[i] = start['theta'][j]
    th, H = _newton(X, n, sy, th0, prec)
    out = {'pair': tuple(pair), 'mirror': mirror, 'anchor': anchor, 'idx': idx, 'theta': th,
           'cov': np.linalg.inv(H), 'reps': None, 'count': count, 'games': len(gs), 'clusters': len(clusters),
           'players': players}
    if boot and len(clusters) > 1:
        rng = np.random.default_rng(seed)
        nc = len(clusters)
        reps = np.empty((int(boot), len(names)))
        for b in range(int(boot)):
            w = np.bincount(rng.integers(0, nc, nc), minlength=nc).astype(float)[cl]
            reps[b] = _newton(X, np.bincount(ci, weights=w, minlength=len(combos)),
                              np.bincount(ci, weights=w * y, minlength=len(combos)), th, prec, tol=1e-7)[0]
        out['reps'] = reps
    return out


def _pair_seed(pair):
    import hashlib
    return BOOT_SEED + int.from_bytes(hashlib.sha256('|'.join(pair).encode()).digest()[:4], 'little')


def cell_term(fits, x, y, sha):
    """Where generation `sha`'s rating in cell x|y is -> (pair, column) - column None: the anchor (0) - or None:
    unrated (no game in that role against another generation)."""
    pair = (x, y) if (x, y) in fits else (y, x)
    f = fits.get(pair)
    if f is None:
        return None
    if sha == f['anchor']:
        return (pair, None)
    role = 'X' if pair[0] == x else 'Y'
    if not f['count'].get((role, sha)):
        return None
    return (pair, f['idx'][(role, sha)])


def lin(fits, terms):
    """A linear combination sum(w x rating) of terms [(pair, column | None, w)] -> (value, se, se_model) in logits: se
    the bootstrap's (replicate b of every pair together: pairs are independent) or, without one, se_model; se_model
    the inverse Hessian's (the pair's covariance within a pair, independent across pairs)."""
    np = _np()
    val, var_m, rep, have_reps = 0.0, 0.0, None, True
    by = {}
    for pair, col, w in terms:
        if col is None:
            continue
        by.setdefault(pair, []).append((col, w))
    for pair, cw in by.items():
        f = fits[pair]
        v = np.zeros(len(f['theta']))
        for col, w in cw:
            v[col] += w
        val += float(v @ f['theta'])
        var_m += float(v @ f['cov'] @ v)
        if f['reps'] is None:
            have_reps = False
        else:
            r = f['reps'] @ v
            rep = r if rep is None else rep + r
    se_m = math.sqrt(max(var_m, 0.0))
    if by and have_reps and rep is not None and len(rep) > 1:
        se = float(np.std(rep, ddof=1))
    else:
        se = se_m
    return val, se, se_m


def anchor_sha(run, cfg, ys, rows):
    """The anchor: the run's first cells reference (generation 0) -> its sha12 (its file's, else its rows'), or None."""
    refs = []
    try:
        refs = Y.cell_refs(run, cfg, ys) if ys.get('cells_refs') else []
    except ValueError:
        refs = []
    path = refs[0]['path'] if refs else Y.model_path(run, 0)
    try:
        return Y.sha12(path)
    except OSError:
        pass
    label = refs[0]['label'] if refs else 'iter_0000'
    for r in rows:
        if r.get('ref') == label and r.get('ref_sha'):
            return r['ref_sha']
    return None


def point_fits(run, cfg, ys, rows, col=None):
    """The point estimates of every pair (no bootstrap; the pool's choice of references) -> (fits, collected)."""
    col = col or collect(run, ys, rows)
    anc = anchor_sha(run, cfg, ys, rows)
    fits = {}
    for pair in Y.cell_pairs(ys):
        gs = col['games'].get(pair)
        if gs and anc is not None:
            fits[pair] = fit_pair(gs, pair, anc)
    return fits, col


# ------------------------------------------------------------------------------------------------ the report
def _r(x, nd=1):
    return None if x is None else round(float(x), nd)


def _gen_summary(fits, decks, sha, base_elo):
    """One generation's ratings: cells {X|Y: {elo, se, se_model, games}}, per-deck means, the overall mean (only with
    all of their cells rated) and the anchored Elo (base_elo + overall)."""
    cells, terms = {}, {}
    for x in decks:
        for y in decks:
            t = cell_term(fits, x, y, sha)
            if t is None:
                continue
            cid = Y.cell_id(x, y)
            terms[cid] = t
            v, se, sem = lin(fits, [(t[0], t[1], 1.0)])
            pair = t[0]
            role = 'X' if pair[0] == x else 'Y'
            cells[cid] = {'elo': _r(K * v), 'se': _r(K * se), 'se_model': _r(K * sem),
                          'games': int(fits[pair]['count'].get((role, sha), 0))}

    def mean(cids):
        if not all(c in terms for c in cids):
            return None
        v, se, _ = lin(fits, [(terms[c][0], terms[c][1], 1.0 / len(cids)) for c in cids])
        return {'elo': _r(K * v), 'se': _r(K * se)}
    out = {'cells': cells, 'decks': {}, 'overall': None, 'anchored': None}
    for x in decks:
        d = mean([Y.cell_id(x, y) for y in decks])
        if d is not None:
            out['decks'][x] = d
    allc = [Y.cell_id(x, y) for x in decks for y in decks]
    out['complete'] = all(c in terms for c in allc)
    if out['complete']:
        out['overall'] = mean(allc)
        out['anchored'] = {'elo': _r(base_elo + out['overall']['elo']), 'se': out['overall']['se']}
    return out, terms


def fit_run(run, cfg, ys, rows=None, boot=None, flags=True):
    """The joint fit of every recorded cells game of the run -> {'fit_v', 'anchor', 'base_elo', 'decks', 'gens':
    [{sha, iter, labels, measured, cells, decks, overall, anchored, complete, flags}], 'pairs', 'transitivity',
    'method', 'games', 'missing_bases'} (JSON-ready). `boot`: bootstrap resamples (default cells_fit_boot)."""
    rows = Y.read_rows(run) if rows is None else rows
    boot = ys.get('cells_fit_boot', 200) if boot is None else int(boot)
    col = collect(run, ys, rows)
    anc = anchor_sha(run, cfg, ys, rows)
    decks = ys['cells_decks']
    base_elo = float(ys.get('cells_base_elo', 1000.0))
    fits = {}
    for pair in Y.cell_pairs(ys):
        gs = col['games'].get(pair)
        if gs and anc is not None:
            fits[pair] = fit_pair(gs, pair, anc, boot=boot, seed=_pair_seed(pair))
    shas = set()
    for f in fits.values():
        shas |= set(f['players'])
    if anc is not None:
        shas.add(anc)
    names = col['names']
    labels = {}
    try:
        for r in (Y.cell_refs(run, cfg, ys) if ys.get('cells_refs') else []):
            try:
                labels.setdefault(Y.sha12(r['path']), []).append(r['label'])
            except OSError:
                pass
    except ValueError:
        pass
    gens, terms = [], {}
    for sha in shas:
        e = names.get(sha, {'iter': None, 'labels': []})
        it = 0 if sha == anc and e['iter'] is None else e['iter']
        s, t = _gen_summary(fits, decks, sha, base_elo)
        terms[sha] = t
        gens.append(dict(s, sha=sha, iter=it, labels=sorted(set(e['labels']) | set(labels.get(sha, []))),
                         measured=sha in col['measured'] or sha == anc, flags={}))
    gens.sort(key=lambda g: (g['iter'] is None, g['iter'] if g['iter'] is not None else 0, g['sha']))
    out = {'fit_v': FIT_V, 'anchor': {'sha': anc, 'labels': next((g['labels'] for g in gens if g['sha'] == anc), [])},
           'base_elo': base_elo, 'decks': list(decks), 'gens': gens, 'pairs': {}, 'transitivity': [],
           'games': sum(f['games'] for f in fits.values()), 'missing_bases': col['missing_bases'],
           'method': {'model': 'logit P(X wins) = m_XY + a[g, X|Y] - a[h, Y|X] + c_XY s; a[anchor] = 0; mirrors m = 0',
                      'se': 'bootstrap over seed units (deals)' if boot else 'inverse Hessian', 'boot': boot,
                      'prior_sd_elo': round(K * PRIOR_SD, 1), 'flag_z': FLAG_Z, 'stall_se': STALL_SE}}
    for pair, f in fits.items():
        e = {'games': f['games'], 'clusters': f['clusters'], 'players': len(f['players'])}
        for k in ('m', 'c'):
            if k in f['idx']:
                v, se, _ = lin(fits, [(pair, f['idx'][k], 1.0)])
                e[k] = [_r(K * v), _r(K * se)]
        out['pairs'][','.join(pair)] = e
    if flags:
        out['transitivity'] = transitivity(fits, col, terms)
        for t in out['transitivity']:
            g = next((g for g in gens if g['sha'] == t['sha']), None)
            if g is not None and t['flag']:
                g['flags'].setdefault('transitivity', []).append(t['cell'])
        reading_rule(fits, gens, terms, decks)
    return out


def transitivity(fits, col, terms):
    """Each gen-over-gen measurement (N against P) per cell: its observed gain (the row's Elo, se) against the fit's
    prediction from N's and P's ratings with the measurement itself left out (a refit of its pair without its games)
    -> [{iter, prev, sha, cell, obs, obs_se, pred, pred_se, z, flag}]; flag: |z| > FLAG_Z ("exploitation of a
    reference?": the ratings, which come from the games against the references, do not predict the direct match).
    pred_se is the full fit's se of the difference (the left-out fit's is a little larger); obs_se the row's delta se,
    at least 2K / units (one unit's change near 50%)."""
    out = []
    for src, s in enumerate(col['sources']):
        if s['kind'] != 'cells_gen':
            continue
        pair = s['pair']
        f = fits.get(pair)
        if f is None:
            continue
        lo = fit_pair(col['games'][pair], pair, f['anchor'], exclude={src}, start=f)
        lofits = dict(fits)
        lofits[pair] = lo
        for cid, st in sorted(s['cells'].items()):
            x, y = cid.split('|')
            tn, tp = cell_term(lofits, x, y, s['sha']), cell_term(lofits, x, y, s['ref_sha'])
            if tn is None or tp is None or st.get('elo') is None:
                continue
            pred = lin(lofits, [(tn[0], tn[1], 1.0), (tp[0], tp[1], -1.0)])[0]
            fn, fp = cell_term(fits, x, y, s['sha']), cell_term(fits, x, y, s['ref_sha'])
            pse = lin(fits, [(fn[0], fn[1], 1.0), (fp[0], fp[1], -1.0)])[1] if fn and fp else 0.0
            # (the observed se at least one unit's worth, 2K / units: a cell whose every unit came out the same for
            # both checkpoints has a delta se of 0, which says no deal told them apart, not that the gain is exact)
            ose = max(float(st.get('se') or 0.0), 2.0 * K / max(int(st.get('units') or 1), 1))
            se = math.sqrt(ose ** 2 + (K * pse) ** 2)
            z = (float(st['elo']) - K * pred) / se if se > 0 else 0.0
            m = re.fullmatch(r'iter_(\d+)', str(s['ref']))
            out.append({'iter': s['iter'], 'prev': int(m.group(1)) if m else s['ref'], 'sha': s['sha'], 'cell': cid,
                        'obs': _r(st['elo']), 'obs_se': _r(ose), 'pred': _r(K * pred),
                        'pred_se': _r(K * pse), 'z': _r(z, 2), 'flag': abs(z) > FLAG_Z})
    return out


def reading_rule(fits, gens, terms, decks):
    """The reading rule, in place on the measured generations (oldest first): a cell whose rating rose by no more
    than STALL_SE se over each of the two steps up to a generation (three consecutive measured checkpoints) is
    'stalled' there; one whose rating fell by more than STALL_SE se in the last step is 'regressed'. The se of a step
    is the se of the difference (the fit's own: the two ratings share games and deals)."""
    ms = [g for g in gens if g['measured'] and g['iter'] is not None]
    cids = [Y.cell_id(x, y) for x in decks for y in decks]

    def step(a, b, cid):
        ta, tb = terms[a['sha']].get(cid), terms[b['sha']].get(cid)
        if ta is None or tb is None:
            return None
        v, se, _ = lin(fits, [(tb[0], tb[1], 1.0), (ta[0], ta[1], -1.0)])
        return v, se
    for i, g in enumerate(ms):
        if i < 1:
            continue
        for cid in cids:
            d2 = step(ms[i - 1], g, cid)
            if d2 is None:
                continue
            if d2[0] < -STALL_SE * d2[1]:
                g['flags'].setdefault('regressed', []).append(cid)
            if i >= 2:
                d1 = step(ms[i - 2], ms[i - 1], cid)
                if d1 is not None and d1[0] <= STALL_SE * d1[1] and d2[0] <= STALL_SE * d2[1]:
                    g['flags'].setdefault('stalled', []).append(cid)


def gen_of(fit, sha):
    return next((g for g in fit['gens'] if g['sha'] == sha), None)


# ------------------------------------------------------------------------------------------------ the pool
def active_refs(refs, it, k):
    """The pool for checkpoint `it`: the anchor (the first reference: generation 0, a permanent member) and the
    newest `k` promoted references whose `since` it reached, oldest first."""
    if not refs:
        return []
    later = [r for r in refs[1:] if r['since'] <= it]
    return [refs[0]] + later[-int(k):] if k else [refs[0]]


def _sig(x):
    return 1.0 / (1.0 + math.exp(-x))


def _val(f, role, sha):
    if f is None or sha is None or sha == f['anchor']:
        return 0.0
    i = f['idx'].get((role, sha))
    return 0.0 if i is None else float(f['theta'][i])


def predict(fits, pair, c_sha, r_sha):
    """The point fit's expected scores of `c_sha` against `r_sha` in the pair's cells (averaged over the choice) and
    the match's information (the sum over its cells of mean p(1 - p): highest at 0.5) -> ({cell: c's score}, info)."""
    f = fits.get(tuple(pair))
    m = 0.0 if f is None or 'm' not in f['idx'] else float(f['theta'][f['idx']['m']])
    c = 0.0 if f is None else float(f['theta'][f['idx']['c']])
    x, y = pair
    out, info = {}, 0.0
    if x == y:
        d = _val(f, 'X', c_sha) - _val(f, 'X', r_sha)
        ps = [_sig(d + c * s) for s in (1, -1)]
        out[Y.cell_id(x, x)] = round(sum(ps) / 2, 4)
        return out, sum(p * (1 - p) for p in ps) / 2
    p1 = [_sig(m + _val(f, 'X', c_sha) - _val(f, 'Y', r_sha) + c * s) for s in (1, -1)]       # c pilots X
    p2 = [1.0 - _sig(m + _val(f, 'X', r_sha) - _val(f, 'Y', c_sha) + c * s) for s in (1, -1)]  # c pilots Y
    out[Y.cell_id(x, y)] = round(sum(p1) / 2, 4)
    out[Y.cell_id(y, x)] = round(sum(p2) / 2, 4)
    info = sum(p * (1 - p) for p in p1) / 2 + sum(p * (1 - p) for p in p2) / 2
    return out, info


def split_games(ys, two):
    """A checkpoint's games per cell of one pair split between the main reference (cells_main_share) and the
    continuity one -> (main, continuity): whole batches (sequential) or even numbers, at least one unit each."""
    g = int(ys['cells_games'])
    if not two:
        return g, 0
    gran = int(ys['cells_batch']) if Y.cells_sequential(ys) else 2
    main = int(round(float(ys['cells_main_share']) * g / gran)) * gran
    main = min(max(main, gran), g)
    return (g, 0) if main >= g else (main, g - main)


def choose(fits, pair, c_sha, pool):
    """The most informative reference of `pool` for a checkpoint (its ratings, or its proxy's: `c_sha`) in the pair:
    the one whose predicted scores are nearest 0.5 (the most information; ties: the newer) -> (label, {label: (pred,
    info)})."""
    best, seen = None, {}
    for r in pool:
        pred, info = predict(fits, pair, c_sha, r.get('sha'))
        seen[r['label']] = (pred, round(info, 4))
        if best is None or info >= best[1] - 1e-12:
            best = (r['label'], info)
    return best[0], seen


def continuity(pool, main):
    """The continuity reference of a main one: the next-older member of the pool (the anchor at the end of the
    chain), None when the main one is the anchor."""
    labels = [r['label'] for r in pool]
    i = labels.index(main) if main in labels else 0
    return labels[i - 1] if i > 0 else None


def promote_rule(fit, refs, it, sha, ys):
    """The pool's promotion rule for measured checkpoint `it` (`sha`) -> {'iter', 'why': 'cadence' | 'threshold',
    'cells', 'vs'} or None: every cells_promote_every iterations after the newest reference's `since`, or sooner when
    a cell's expected score against the newest reference (the logistic of the fit's rating difference) passes
    cells_promote or falls below 1 - cells_promote. Only a checkpoint rated in all its cells, newer than the newest
    reference."""
    if not refs:
        return None
    new = refs[-1]
    if new['since'] >= it:
        return None
    g = gen_of(fit, sha)
    if g is None or not g.get('complete'):
        return None
    try:
        rsha = Y.sha12(new['path'])
    except OSError:
        rsha = new.get('sha')
    if rsha == sha:
        return None
    rg = gen_of(fit, rsha) if rsha else None
    thr = float(ys['cells_promote'])
    cells = []
    if rg is not None:
        for cid, c in g['cells'].items():
            rc = rg['cells'].get(cid)
            if rc is None or c['elo'] is None or rc['elo'] is None:
                continue
            p = Y.expected_score(c['elo'] - rc['elo'])
            if p > thr or p < 1.0 - thr:
                cells.append(cid)
    every = ys.get('cells_promote_every') or 0
    if every and it - new['since'] >= every:
        return {'iter': it, 'why': 'cadence', 'cells': cells, 'vs': new['label']}
    if cells:
        return {'iter': it, 'why': 'threshold', 'cells': cells, 'vs': new['label']}
    return None


# ------------------------------------------------------------------------------------------------ the record
def _pair2(d):
    return None if d is None else [d['elo'], d['se']]


def fit_row(fit, it, sha, ys, rows, refs=None, pool=None, promote=None, seconds=None):
    """The 'cells_fit' record of measured checkpoint `it` (docs/ENGINE.md 186.6) from a fit_run result."""
    g = gen_of(fit, sha) or {'cells': {}, 'decks': {}, 'overall': None, 'anchored': None, 'complete': False,
                             'flags': {}}
    res, _ = Y.tally(rows)
    used = {}                                    # the references the checkpoint played, per cell
    plan = {}
    for r in res.values():
        if r['kind'] == 'cells' and r.get('iter') == it and r.get('sha') == sha:
            for cid, st in (r.get('cells') or {}).items():
                used.setdefault(cid, {})[r['ref']] = st.get('games')
            p = plan.setdefault(','.join(r['pair']), {'main': None, 'cont': None, 'games': {}})
            p['games'][r['ref']] = next(iter((r.get('cells') or {}).values()), {}).get('games')   # (a cell's)
            p['cont' if r.get('role') == 'cont' else 'main'] = r['ref']
    cells = {cid: dict(c, refs=used.get(cid, {})) for cid, c in g['cells'].items()}
    trans = [{k: t[k] for k in ('cell', 'iter', 'prev', 'obs', 'obs_se', 'pred', 'pred_se', 'z')}
             for t in fit['transitivity'] if t['sha'] == sha and t['flag']]
    gens = []
    for x in fit['gens']:
        if not x['measured']:
            continue
        e = {'iter': x['iter'], 'sha': x['sha'], 'cells': {c: [v['elo'], v['se']] for c, v in x['cells'].items()},
             'decks': {d: _pair2(v) for d, v in x['decks'].items()}, 'overall': _pair2(x['overall']),
             'anchored': _pair2(x['anchored'])}
        if x['labels']:
            e['labels'] = x['labels']
        if x['flags']:
            e['flags'] = x['flags']
        gens.append(e)
    row = {'kind': 'cells_fit', 'iter': int(it), 'sha': sha, 'fit_v': FIT_V, 'mode': ys.get('cells_mode', 'ladder'),
           'anchor': fit['anchor'], 'base_elo': fit['base_elo'], 'complete': bool(g.get('complete')),
           'cells': cells, 'decks': g['decks'], 'overall': g['overall'], 'anchored': g['anchored'],
           'flags': {'transitivity': trans, 'stalled': g['flags'].get('stalled', []),
                     'regressed': g['flags'].get('regressed', [])},
           'promote': promote, 'plan': plan, 'pairs': fit['pairs'], 'gens': gens, 'method': fit['method'],
           'games': fit['games'], 'time': round(time.time())}
    if pool is not None:
        row['pool'] = [r['label'] for r in pool]
    if fit['missing_bases']:
        row['missing_bases'] = fit['missing_bases']
    if seconds is not None:
        row['seconds'] = round(seconds, 1)
    return row
