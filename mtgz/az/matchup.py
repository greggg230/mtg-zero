"""The in-run matchup model of a constructed best-of-three pairing and sideboarding by its choice tables (docs/ENGINE.md
168: the Delver best-of-three run pb3's mirror; docs/ENGINE.md 240: two different 75s, a stew specialist's pairing).

**Two different 75s** (docs/ENGINE.md 240.3; everything below the mirror's, pb3's, which a mirror pairing still is
exactly - the same code). For a pairing {C, D} (C the deck of the lower metagame index, a fixed spec's seat 0) every
game is one row oriented C first, and the model is not antisymmetric - the pairing has its own base rate:

    z(A, B) = m + w_C . dA - w_D . dB + (dA U_C) W (dB U_D)^T + c_pd pd + c_pdg3 pd g3 + c_g3 g3 + c_lead lead
              + c_dC dealt_C + c_dD dealt_D,   P(C wins) = sigmoid(z)

dA (dB): C's (D's) 60 as copies minus its own registered main per entry of its own 75; pd +1 when C starts; lead
sign(C's wins - D's) before the game; dealt_C, dealt_D each deck's dealt flag. With C = D and the constraints w_C =
w_D, U_C = U_D, W skew, m = c_g3 = 0, c_dC = -c_dD it is the mirror's model below; for a mirror the code fits the
mirror's model itself. The same fit (the time split, the grid, the selection rule, the bootstrap ensembles and their
seeds); the guard's baseline is the pairing's base rate + play / draw + game 3 (no deck term), the mirror's play / draw
with the base rate a non-mirror needs. One choice table PER DECK (`pair_choice_tables`), each deck's 24 contexts
against the OTHER deck's constructed prior (restricted to the dealt range for a dealt opponent), the decider's win
probability sigmoid(z) for C, sigmoid(-z) for D; the tables go where the engine reads them (`with_tables`: a metagame
deck's entry, a fixed spec's seat), so each follows its deck to whichever seat plays it. One pairing per model (a
metagame of several pairings is refused: `the_pairing`).

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

**Its data** (docs/ENGINE.md 273, `load_games`). Every finished self-play game of the window, as the best-of-three game
log's lines (docs/ENGINE.md 168.2: one line a game with both 60s, the starter, the score, the dealt flags and the
result). Dealt seats (p 0.85 per seat in pb3) give the unconfounded signal: their 60s are drawn at random. The source
(`data_source`): the run's own game log (`<run>/games/games-<pid>.jsonl`: its local self-play only) when it covers the
window - no records there, or records of local self-play only -, else the run's game records (zero-core
`game_records`: every game the quota accepted, local, a remote host's and the farm's), each match replayed on this
checkout's engine into exactly the lines the log would hold (mtgz.az.bo3_records); records another engine played (an
earlier pin) are left out, counted and said. A version records its data (`data`: the source, the games per source, what
was left out) and whether it is STARVED (fewer games than `matchup_min_games_per_iter` times its window's iterations);
the run log says both loudly, and the stew's sideboarding health alerts on them (mtgz.stew.sbhealth).

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

**Dealt around the table** (docs/ENGINE.md 261, the spec's `dealt_decks.around` `"table"`): a dealt seat's 60 is then
dealt with its swaps counted from the 60 ITS OWN table picks for the deal's context (the main for a deck without a
table), so the belief about a dealt opponent (`opp_dealt` 1) is exactly that: base = the opponent deck's choice in the
version's own tables for its context - the same game, the other sideboarding position (it sideboards first exactly
when the decider does not), the opposite lead, and its own `opp_dealt` 0 (the decider, choosing, is not dealt) -, then
uniform over the swap counts of the range from it, then uniform over the 60s with that count (`sample_around`). Those
contexts depend only on the tables' `opp_dealt` 0 contexts, so the job solves every deck's `opp_dealt` 0 contexts
first, then its `opp_dealt` 1 contexts against the worlds around the other deck's (`choice_table` and
`pair_choice_tables` with `around` "table"). `"around": "main"` (the default) is unchanged.

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
    'matchup_min_games_per_iter': 500,  # a version fitted on fewer games than this times its window's iterations is
                                        # STARVED: published, and said loudly (the run log, its record, sb-health;
                                        # docs/ENGINE.md 273)
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


def game_line(x, specs, where, selfplay=True):
    """One best-of-three game log line (the run's log, or a record's replay: mtgz.az.bo3_records) -> the game to keep,
    or None: a spec line (taken into `specs`), another kind, an aborted game, a game of a spec that does not deal (with
    `selfplay`: the run's evaluation spec never deals)."""
    if x.get('kind') == 'bo3-spec':
        specs[x['spec']] = x
        return None
    if x.get('kind') != 'bo3':
        return None
    sp = specs.get(x['spec'])
    if sp is None:
        raise ValueError(f'{where}: a game of spec {x["spec"]} before its spec line')
    if x.get('result') is None or (selfplay and not sp.get('dealt_decks')):
        return None
    return x


def log_games(run, lo=None, hi=None, selfplay=True):
    """The best-of-three game log's finished games of `run` (a run directory, or a directory of .jsonl files) ->
    (games, spec lines by key). Each game gets 'iter' (the first metrics line at or after its time; None for an
    iteration in progress, or no metrics). `selfplay`: only games of a self-play spec (one with dealt decks; the run's
    evaluation spec never deals). Aborted games are left out. Local self-play only (the run's own process logs)."""
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
                x = game_line(json.loads(line), specs, f'{path}:{n}', selfplay)
                if x is None:
                    continue
                i = bisect.bisect_left(tt, x['time'])
                x['iter'] = times[i][1] if i < len(times) else None
                if lo is not None and (x['iter'] is None or x['iter'] < lo):
                    continue
                if hi is not None and (x['iter'] is None or x['iter'] > hi):
                    continue
                games.append(x)
    return games, specs


def record_iterations(run, lo=None, hi=None):
    """The iterations lo..hi with complete game records (zero-core `game_records`: records/iter_NNNN/DONE.json)."""
    from zero import gamerec as GR
    if not os.path.isdir(os.path.join(run, GR.DIR)):
        return []
    return [it for it in GR.done_iterations(run) if (lo is None or it >= lo) and (hi is None or it <= hi)]


def data_source(run, lo=None, hi=None):
    """The source rule (docs/ENGINE.md 273) -> ('log' | 'records', why). The game log when it covers the window - no
    iteration of it has game records, or every one that has them accepted local self-play only (DONE.json's sources:
    'local'; the log holds those games: pb3 and any local-only run publish the versions they always did) -; the game
    records otherwise (an iteration with a remote host's or the farm's games: the log misses them)."""
    from zero import gamerec as GR
    its = record_iterations(run, lo, hi)
    if not its:
        return 'log', 'no game records in the window'
    off = [it for it in its if set((GR.read_done(run, it) or {}).get('by_source') or {}) - {'local'}]
    if not off:
        return 'log', f'the game log covers the window: its {len(its)} iterations of records are local self-play only'
    return 'records', (f'{len(off)} of the window\'s {len(its)} iterations of records hold games played off this '
                       f'machine (remote or farm self-play: not in the game log)')


def _ranges(its):
    out = []
    for i in sorted(its):
        if out and out[-1][1] == i - 1:
            out[-1][1] = i
        else:
            out.append([i, i])
    return ', '.join(f'{a}' if a == b else f'{a}-{b}' for a, b in out) or 'none'


def load_games(run, lo=None, hi=None, selfplay=True, source='auto', jobs=1, cache=None, info=None, log=None):
    """The run's finished self-play games of iterations lo..hi -> (games, spec lines by key), from the source
    `data_source` picks ('auto'), or 'log' / 'records' (docs/ENGINE.md 273). The game log: `log_games`. The records:
    every iteration of the window with records replayed (mtgz.az.bo3_records: each game the dict the log holds, 'iter'
    its record's accepting iteration, 'time' the record's), an iteration whose records another engine played EXCLUDED
    and counted (never replayed on this one), an iteration of the window without records from the game log (its local
    games). `info` (a dict): receives what was read - the source and why, the games per source, the excluded
    iterations with their matches and games and why, the accepted games without a record, the replay's seconds."""
    if info is None:
        info = {}
    if source == 'auto':
        source, why = data_source(run, lo, hi)
    else:
        why = f'source {source} asked'
    info.update(source=source, why=why)
    if source == 'log':
        games, specs = log_games(run, lo, hi, selfplay)
        info.update(games=len(games), games_log=len(games), games_records=0, excluded=[], excluded_games=0)
        return games, specs
    if source != 'records':
        raise ValueError(f'source {source!r}: auto, log or records')
    from mtgz.az import bo3_records as BR
    its = record_iterations(run, lo, hi)
    t0 = time.time()
    got = BR.games_of(run, its, jobs=jobs, cache=cache, log=log)
    specs, games = {}, []
    excluded, missing, used = [], {}, []
    for it in its:
        o = got[it]
        if o.get('missing'):
            missing[str(it)] = o['missing']
        if 'excluded' in o:
            excluded.append(dict(o['excluded'], iteration=it, matches=o['matches'], games=o['games']))
            continue
        used.append(it)
        for m in o['lines']:
            for k, x in enumerate(m['lines']):
                g = game_line(x, specs, f'iteration {it} source {m["src"]} match {m["gid"]} line {k + 1}', selfplay)
                if g is not None:
                    games.append(dict(g, iter=it, time=m['t']))
    n_rec = len(games)
    # (the window's iterations without records: their logged - local - games)
    rest = []
    if lo is not None and hi is not None:
        have = set(its)
        if any(i not in have for i in range(lo, hi + 1)):
            lg, ls = log_games(run, lo, hi, selfplay)
            rest = [g for g in lg if g['iter'] not in have]
            for k in {g['spec'] for g in rest}:
                if k in specs and specs[k] != ls[k]:
                    raise ValueError(f'spec {k}: the game log\'s spec line differs from the records\' replay')
                specs[k] = ls[k]
    games += rest
    groups = {}
    for e in excluded:
        k = (e['why'], e['engine'], e['commit'])
        gr = groups.setdefault(k, {'why': e['why'], 'engine': e['engine'], 'commit': e['commit'], 'hint': e.get('hint'),
                                   'iterations': [], 'matches': 0, 'games': 0})
        gr['iterations'].append(e['iteration'])
        gr['matches'] += e['matches']
        gr['games'] += e['games']
    info.update(games=len(games), games_records=n_rec, games_log=len(rest),
                records_iterations=_ranges(used), log_iterations=_ranges({g['iter'] for g in rest}),
                matches=sum(len(got[i]['lines']) for i in used),
                cached_iterations=sum(1 for i in used if got[i].get('cached')),
                excluded=[dict(g, iterations=_ranges(g['iterations'])) for g in groups.values()],
                excluded_games=sum(g['games'] for g in groups.values()),
                missing=missing, seconds=round(time.time() - t0, 1))
    return games, specs


def data_note(info):
    """`load_games`'s info -> one line for the job's log and the version's report."""
    s = f'{info.get("games")} games from the {info.get("source")} ({info.get("why")})'
    if info.get('source') == 'records':
        s += (f': {info["games_records"]} replayed from the records of iterations {info["records_iterations"]} '
              f'({info["matches"]} matches, {info["cached_iterations"]} iterations cached, {info["seconds"]} s)')
        if info.get('games_log'):
            s += f', {info["games_log"]} from the game log of iterations {info["log_iterations"]} (no records)'
    for e in info.get('excluded') or []:
        s += (f'; LEFT OUT {e["games"]} games ({e["matches"]} matches) of iterations {e["iterations"]}: their records '
              f'were played by engine {e["engine"]} (mtg-zero {(e["commit"] or "?")[:12]}; {e["why"]}), which this '
              f'checkout cannot replay')
    if info.get('missing'):
        s += f'; accepted games without a record: {info["missing"]}'
    return s


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


def dealt_around(spec):
    """The spec's `dealt_decks.around` (docs/ENGINE.md 261): 'main' (also without the key or without dealt decks) or
    'table'; any other value raises (the engine refuses it too)."""
    sp = json.loads(spec) if isinstance(spec, str) else spec
    a = (sp.get('dealt_decks') or {}).get('around', 'main')
    if a not in ('main', 'table'):
        raise ValueError(f'dealt_decks.around {a!r}: "main" or "table" (docs/ENGINE.md 261)')
    return a


def opponent_context(game, first, lead):
    """The context of a dealt opponent's deal when the decider is at (game, first, lead) with the opponent dealt
    (docs/ENGINE.md 261): the same game, the other sideboarding position, the opposite lead, its own opp_dealt 0 (the
    decider chooses: it is not dealt)."""
    return (game, 1 - first, -lead, 0)


def sample_around(copies, base, main, lo, hi, n, rng):
    """n worlds of a dealt opponent's 60 around `base` (docs/ENGINE.md 261: the engine's `ConstructedPrior::dealt` with
    `base` as its main - uniform over the swap counts lo..hi counted from `base`, then uniform over the 60s with that
    count; exact, `sample_prior`) as copies minus the registered `main` -> (n x E) floats."""
    import numpy as np
    return (sample_prior(copies, list(base), lo, hi, n, rng) - np.asarray(main, dtype=np.float64)).astype(np.float64)


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


def choice_table(members, names, copies, main, dealt_swaps, version, worlds=8000, seed=0, log=print, z=2.0,
                 around='main'):
    """The 24 contexts' best 60s (module docs) -> (the spec's "sideboard_table", per-context detail). `around` 'table'
    (docs/ENGINE.md 261): a dealt opponent's worlds around this table's own choice for its context (`sample_around`),
    the opp_dealt 0 contexts solved first."""
    import numpy as np
    rng = np.random.default_rng(seed)
    S = sum(copies) - sum(main)
    m = np.asarray(main, dtype=np.float64)
    if around == 'table':
        beliefs = {0: sample_prior(copies, main, 0, S, worlds, rng) - m}
        order = [c for c in contexts() if c[3] == 0] + [c for c in contexts() if c[3] == 1]
    else:
        beliefs = {0: sample_prior(copies, main, 0, S, worlds, rng) - m,
                   1: sample_prior(copies, main, dealt_swaps[0], dealt_swaps[1], worlds, rng) - m}
        order = contexts()
    solved, by_base = {}, {}
    strong = strength_optimum(members, copies, main)        # (the strength term's exact argmax, unguarded: reported)
    for (g, f, l, o) in order:
        if around == 'table' and o == 1:
            base = tuple(solved[opponent_context(g, f, l)][0])
            if base not in by_base:
                by_base[base] = sample_around(copies, base, main, dealt_swaps[0], dealt_swaps[1], worlds, rng)
            B = by_base[base]
        else:
            B = beliefs[o].astype(np.float64)
        F = Objective(members, B, g, f, l, o)
        c, fv, sw = confident_climb(F, copies, main, z)
        solved[(g, f, l, o)] = (c, fv, sw, F)
    choices, detail = [], []
    for (g, f, l, o) in contexts():
        c, fv, sw, F = solved[(g, f, l, o)]
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
    check_tables(spec, [table])


def check_tables(spec, tables):
    """`check_table` for the tables of a version, placed as self-play places them (`with_tables`, docs/ENGINE.md 240)."""
    from mtgz import _engine
    _engine.mtg_spec_check(with_tables(spec, tables))


# ----------------------------------------------------------------------------- the pairing (docs/ENGINE.md 240)

# the non-mirror model's context columns (docs/ENGINE.md 240.3): the pairing's base rate, then as the mirror's terms but
# with no antisymmetry - a game 3 main effect and each deck's dealt flag on its own
PAIR_CTX = ('base', 'pd', 'pd_g3', 'g3', 'lead', 'dealt_C', 'dealt_D')
PAIR_BASELINE = 4                 # the first 4 columns: the baseline the deck model must beat (no deck term)


class Pairing:
    """The one pairing a matchup model is fit for (docs/ENGINE.md 240): deck C and deck D, each {'name', 'list'
    ([[card, copies], ...] in the engine's list order), 'main' (the registered main's copies per entry), 'index' (its
    metagame index; a fixed spec's seat)}. `mirror`: one 75 both seats - pb3's model and table, unchanged (C is D).
    C is the deck of the lower metagame index (a fixed two-75 spec: seat 0's)."""

    def __init__(self, decks, mirror):
        self.decks = decks
        self.mirror = mirror

    def json(self):
        return {'mirror': self.mirror, 'decks': [{k: d[k] for k in ('name', 'list', 'main')} for d in self.decks]}

    def seat_of_c(self, sp):
        """The seat that plays deck C in the games of spec line `sp` (a mirror: seat 0, pb3's A)."""
        if self.mirror:
            return 0
        mg = sp.get('metagame')
        if mg is None:
            return 0
        return 0 if mg['decks'][0] == self.decks[0]['index'] else 1


def _seat_decks(sp):
    mg = sp.get('metagame')
    return [{'name': mg['names'][s] if mg else f'p{s}', 'list': sp['lists'][s], 'main': sp['mains'][s],
             'index': mg['decks'][s] if mg else s} for s in (0, 1)]


def the_pairing(specs, games):
    """The one pairing of the games' spec lines (a Pairing): a mirror (both seats one list and main - pb3's case: the
    same in every spec, as `the_75`), or two different 75s - the same two decks in every spec line, in either seat order
    (a metagame restricted to the pairing plays both). More than one pairing is refused: a model is one pairing's
    (docs/ENGINE.md 240: a stew specialist's)."""
    keys = sorted({g['spec'] for g in games})
    if not keys:
        raise ValueError('no game')
    found = {}
    for k in keys:
        a, b = _seat_decks(specs[k])
        if a['list'] == b['list'] and a['main'] == b['main']:
            if specs[k].get('metagame') and a['name'] != b['name']:
                raise ValueError(f'metagame decks {a["name"]} and {b["name"]} are one 75: a deck\'s table is keyed by its '
                                 f'name, so two names of one 75 cannot share a model (docs/ENGINE.md 240)')
            key = ('mirror', json.dumps(a['list']), json.dumps(a['main']))
            found.setdefault(key, Pairing([a, a], True))
        else:
            c, d = (a, b) if a['index'] <= b['index'] else (b, a)
            key = ('pair', c['name'], json.dumps(c['list']), json.dumps(c['main']), d['name'], json.dumps(d['list']),
                   json.dumps(d['main']))
            found.setdefault(key, Pairing([c, d], False))
    if len(found) != 1:
        raise ValueError(f'the games play {len(found)} pairings: a matchup model is one pairing\'s (docs/ENGINE.md 240); '
                         f'{sorted(" vs ".join(x[1:2] + x[4:5]) if x[0] == "pair" else "a mirror" for x in found)}')
    return next(iter(found.values()))


def pair_arrays(games, specs, P):
    """The non-mirror model's inputs (docs/ENGINE.md 240.3), every game oriented deck C first: dA (C's 60, count deltas
    against C's main, games x E_C), dB (D's, games x E_D), ctx (games x 7: PAIR_CTX from C's side), y (C's result), it."""
    import numpy as np
    mC = np.asarray(P.decks[0]['main'], dtype=np.float64)
    mD = np.asarray(P.decks[1]['main'], dtype=np.float64)
    a = [P.seat_of_c(specs[g['spec']]) for g in games]
    dA = np.asarray([g['decks'][s] for g, s in zip(games, a)], dtype=np.float64) - mC
    dB = np.asarray([g['decks'][1 - s] for g, s in zip(games, a)], dtype=np.float64) - mD
    pd = np.asarray([1.0 if g['starter'] == s else -1.0 for g, s in zip(games, a)])
    g3 = np.asarray([1.0 if g['game'] == 3 else 0.0 for g in games])
    lead = np.asarray([float(np.sign(g['wins'][s] - g['wins'][1 - s])) for g, s in zip(games, a)])
    dC = np.asarray([float(g['dealt'][s]) for g, s in zip(games, a)])
    dD = np.asarray([float(g['dealt'][1 - s]) for g, s in zip(games, a)])
    ctx = np.stack([np.ones(len(games)), pd, pd * g3, g3, lead, dC, dD], axis=1)
    y = np.asarray([g['result'][s] for g, s in zip(games, a)], dtype=np.float64)
    it = np.asarray([g['iter'] if g['iter'] is not None else -1 for g in games])
    return dA, dB, ctx, y, it


def z_pair(p, dA, dB, ctx):
    """The non-mirror logit (numpy, float64) of member `p` ({'wC', 'wD', 'UC', 'UD', 'W', 'c'}): C's side."""
    import numpy as np
    z = dA @ p['wC'] - dB @ p['wD'] + ctx @ p['c']
    UC = np.asarray(p['UC'])
    if UC.size:
        z = z + np.einsum('nd,de,ne->n', dA @ UC, np.asarray(p['W']), dB @ np.asarray(p['UD']))
    return z


def fit_pair(dA, dB, ctx, y, wt, dim, l2_w, l2_inter, seed=0, base_only=False, threads=4):
    """One non-mirror fit (docs/ENGINE.md 240.3), the mirror's procedure (`fit`): full-batch L-BFGS (torch, float64) of the
    weighted mean log loss + L2 (l2_w on both decks' w, l2_inter on U_C, U_D and W) -> member dict (numpy lists).
    `base_only`: the baseline (the pairing's base rate, play / draw, game 3; no deck term, no lead, no dealt flags)."""
    import numpy as np
    import torch
    torch.set_num_threads(max(1, threads))
    g = torch.Generator().manual_seed(seed)
    EC, ED = dA.shape[1], dB.shape[1]
    K = ctx.shape[1]
    tA, tB, tc = (torch.as_tensor(a, dtype=torch.float64) for a in (dA, dB, ctx))
    ty, tw = torch.as_tensor(y, dtype=torch.float64), torch.as_tensor(wt, dtype=torch.float64)
    tw = tw / tw.sum()
    wC = torch.zeros(EC, dtype=torch.float64, requires_grad=not base_only)
    wD = torch.zeros(ED, dtype=torch.float64, requires_grad=not base_only)
    c = torch.zeros(K, dtype=torch.float64, requires_grad=True)
    UC = (0.1 * torch.randn(EC, dim, generator=g, dtype=torch.float64)).requires_grad_(dim > 0)
    UD = (0.1 * torch.randn(ED, dim, generator=g, dtype=torch.float64)).requires_grad_(dim > 0)
    W = (0.1 * torch.randn(dim, dim, generator=g, dtype=torch.float64)).requires_grad_(dim > 0)
    mask = torch.tensor([1.0] * PAIR_BASELINE + [0.0] * (K - PAIR_BASELINE), dtype=torch.float64) if base_only else \
        torch.ones(K, dtype=torch.float64)
    params = [c] + ([] if base_only else [wC, wD]) + ([UC, UD, W] if dim > 0 else [])
    opt = torch.optim.LBFGS(params, lr=1.0, max_iter=500, tolerance_grad=1e-10, tolerance_change=1e-12,
                            history_size=50, line_search_fn='strong_wolfe')

    def logit():
        z = tc @ (c * mask)
        if not base_only:
            z = z + tA @ wC - tB @ wD
        if dim > 0:
            z = z + torch.einsum('nd,de,ne->n', tA @ UC, W, tB @ UD)
        return z

    def closure():
        opt.zero_grad()
        z = logit()
        loss = (tw * torch.nn.functional.binary_cross_entropy_with_logits(z, ty, reduction='none')).sum()
        if not base_only:
            loss = loss + l2_w * ((wC * wC).sum() + (wD * wD).sum())
        if dim > 0:
            loss = loss + l2_inter * ((UC * UC).sum() + (UD * UD).sum() + (W * W).sum())
        loss = loss + 1e-6 * (c * c).sum()
        loss.backward()
        return loss

    opt.step(closure)
    z0 = np.zeros
    return {'wC': (wC.detach().numpy() if not base_only else z0(EC)).tolist(),
            'wD': (wD.detach().numpy() if not base_only else z0(ED)).tolist(),
            'c': (c * mask).detach().numpy().tolist(),
            'UC': UC.detach().numpy().tolist() if dim > 0 else [], 'UD': UD.detach().numpy().tolist() if dim > 0 else [],
            'W': W.detach().numpy().tolist() if dim > 0 else [], 'dim': dim, 'l2_w': l2_w, 'l2_inter': l2_inter}


def _npp(m):
    import numpy as np
    return {k: (np.asarray(v, dtype=np.float64) if k in ('wC', 'wD', 'UC', 'UD', 'W', 'c') else v) for k, v in m.items()}


def logloss_pair(members, dA, dB, ctx, y):
    """The per-game mean log loss of the members' mean probability (unweighted; `logloss`'s for the non-mirror model)."""
    import numpy as np
    p = np.mean([1.0 / (1.0 + np.exp(-np.asarray(z_pair(_npp(m), dA, dB, ctx)))) for m in members], axis=0)
    p = np.clip(p, 1e-12, 1 - 1e-12)
    return float(-np.mean(y * np.log(p) + (1 - y) * np.log(1 - p)))


def select_and_fit_pair(games, specs, P, val_iters=2, half_life=16.0, threads=4, seed=0, log=print, members_n=MEMBERS):
    """`select_and_fit` for two different 75s (docs/ENGINE.md 240.3): the same grid on the same time split (the newest
    `val_iters` iterations validate), the same selection rule, the same held-out ensemble (MEMBERS bootstrap fits on the
    training iterations: the yardstick) and the published ensemble (`members_n` bootstrap fits on the whole window, the
    same seeds) -> (members, report)."""
    import numpy as np
    dA, dB, ctx, y, it = pair_arrays(games, specs, P)
    its = sorted(set(int(x) for x in it if x >= 0))
    if len(its) < val_iters + 1:
        raise ValueError(f'games of {len(its)} iterations: too few for {val_iters} validation iterations')
    vcut = its[-val_iters]
    tr, va = it < vcut, it >= vcut
    ref = its[-1]
    wtr = weights(it[tr], its[-val_iters - 1], half_life)
    base = fit_pair(dA[tr], dB[tr], ctx[tr], y[tr], wtr, 0, 0.0, 0.0, seed=seed, base_only=True, threads=threads)
    base_val = logloss_pair([base], dA[va], dB[va], ctx[va], y[va])
    grid = []
    for dim in DIMS:
        for l2w in L2_W:
            for l2i in (L2_INTER if dim > 0 else (0.0,)):
                m = fit_pair(dA[tr], dB[tr], ctx[tr], y[tr], wtr, dim, l2w, l2i, seed=seed, threads=threads)
                v = logloss_pair([m], dA[va], dB[va], ctx[va], y[va])
                grid.append({'dim': dim, 'l2_w': l2w, 'l2_inter': l2i, 'val_logloss': v})
                log(f'# dim {dim} l2_w {l2w} l2_inter {l2i}: validation log loss {v:.5f} (base rate + play / draw only '
                    f'{base_val:.5f})')
    lo_v = min(g['val_logloss'] for g in grid)
    best = min((g for g in grid if g['val_logloss'] <= lo_v + 1e-5), key=lambda g: (g['dim'], -g['l2_w'], -g['l2_inter']))
    held = []
    rng = np.random.default_rng(seed)
    for k in range(MEMBERS):
        ix = np.flatnonzero(tr)
        b = rng.choice(ix, size=len(ix), replace=True)
        held.append(fit_pair(dA[b], dB[b], ctx[b], y[b], weights(it[b], its[-val_iters - 1], half_life), best['dim'],
                             best['l2_w'], best['l2_inter'], seed=seed + 1 + k, threads=threads))
    held_val = logloss_pair(held, dA[va], dB[va], ctx[va], y[va])
    dva = np.asarray([games[i]['dealt'][0] or games[i]['dealt'][1] for i in np.flatnonzero(va)])
    held_val_dealt = logloss_pair(held, dA[va][dva], dB[va][dva], ctx[va][dva], y[va][dva]) if dva.any() else None
    base_val_dealt = logloss_pair([base], dA[va][dva], dB[va][dva], ctx[va][dva], y[va][dva]) if dva.any() else None
    members = []
    wall = weights(it, ref, half_life)
    for k in range(members_n):
        b = rng.choice(len(y), size=len(y), replace=True)
        members.append(fit_pair(dA[b], dB[b], ctx[b], y[b], wall[b], best['dim'], best['l2_w'], best['l2_inter'],
                                seed=seed + 101 + k, threads=threads))

    def centred(key, main):
        mn = np.asarray(main, dtype=np.float64)
        W = np.asarray([m[key] for m in members])
        W = W - (W @ mn / mn.sum())[:, None]
        return W.mean(0).tolist(), W.std(0, ddof=1).tolist()

    (pcC, seC), (pcD, seD) = centred('wC', P.decks[0]['main']), centred('wD', P.decks[1]['main'])
    rep = {'games': int(len(y)), 'train_games': int(tr.sum()), 'val_games': int(va.sum()), 'iterations': its,
           'val_iterations': [i for i in its if i >= vcut], 'half_life': half_life, 'grid': grid, 'selected': best,
           'val_logloss': held_val, 'val_logloss_pd': base_val, 'gain_nats': base_val - held_val,
           'val_logloss_dealt': held_val_dealt, 'val_logloss_pd_dealt': base_val_dealt,
           'pd_baseline': base['c'][:PAIR_BASELINE], 'per_copy': {P.decks[0]['name']: pcC, P.decks[1]['name']: pcD},
           'per_copy_se': {P.decks[0]['name']: seC, P.decks[1]['name']: seD}, 'ctx_names': list(PAIR_CTX),
           'ctx': np.asarray([m['c'] for m in members]).mean(0).tolist(),
           'c_win_rate': float(np.mean(y)), 'baseline': 'base rate + play / draw + game 3 (no deck term)'}
    return members, rep


class PairObjective:
    """F of one deck's 60 in one context for two different 75s (docs/ENGINE.md 240.3; `Objective`'s for a mirror): the
    members' mean over the worlds of the OTHER deck's 60 (its constructed prior, restricted to the dealt range when it
    is dealt) of the decider's win probability - sigmoid(z) when the decider plays C (`role` 0), sigmoid(-z) when it
    plays D (`role` 1), z always C's side -, its better side of play and draw when it sideboards first, its worse
    second."""

    def __init__(self, members, worlds_delta, role, game, first, lead, opp_dealt):
        import numpy as np
        self.np = np
        self.role = role
        self.first = first
        g3 = 1.0 if game == 3 else 0.0
        self.parts = []
        for m in (_npp(x) for x in members):
            if role == 0:
                s_o = worlds_delta @ m['wD']
                g_o = (worlds_delta @ m['UD']) @ m['W'].T if m['UC'].size else None
            else:
                s_o = worlds_delta @ m['wC']
                g_o = (worlds_delta @ m['UC']) @ m['W'] if m['UC'].size else None
            ctx = {}
            for pd in (1.0, -1.0):
                pdc, lc = (pd, lead) if role == 0 else (-pd, -lead)
                dc, dd = (0.0, opp_dealt) if role == 0 else (opp_dealt, 0.0)
                ctx[pd] = float(np.asarray([1.0, pdc, pdc * g3, g3, lc, dc, dd]) @ m['c'])
            self.parts.append((m, s_o, g_o, ctx))

    def per_member(self, d):
        """Each member's F of candidates `d` (k x E of the decider's deck) -> (members, k)."""
        np = self.np
        out = []
        for m, s_o, g_o, ctx in self.parts:
            sides = []
            for pd in (1.0, -1.0):
                if self.role == 0:
                    z = (d @ m['wC'])[:, None] - s_o[None, :] + ctx[pd]
                    if g_o is not None:
                        z = z + (d @ m['UC']) @ g_o.T
                    p = 1.0 / (1.0 + np.exp(-z))
                else:
                    z = s_o[None, :] - (d @ m['wD'])[:, None] + ctx[pd]
                    if g_o is not None:
                        z = z + (d @ m['UD']) @ g_o.T
                    p = 1.0 / (1.0 + np.exp(z))
                sides.append(p.mean(axis=1))
            out.append(np.maximum(sides[0], sides[1]) if self.first else np.minimum(sides[0], sides[1]))
        return np.asarray(out)

    def __call__(self, d):
        return self.per_member(d).mean(axis=0)


def pair_choice_tables(members, P, dealt_swaps, version, worlds=8000, seed=0, log=print, z=2.0, around='main'):
    """Two different 75s (docs/ENGINE.md 240.3): one choice table PER DECK - each deck's 24 contexts' 60s by the same
    guarded climb (`confident_climb`, `z`) against the other deck's constructed prior (its full swap range; the spec's
    dealt range for a dealt opponent) -> ([C's table, D's table] - each the spec's {"version", "list", "choices"} of
    its own 75 -, {deck name: per-context detail}). `around` 'table' (docs/ENGINE.md 261): a dealt opponent's worlds
    around the other deck's own choice in these tables for its context (`sample_around`), every deck's opp_dealt 0
    contexts solved first."""
    import numpy as np
    rng = np.random.default_rng(seed)
    decks = P.decks
    info = []
    for d in decks:
        copies = [int(x[1]) for x in d['list']]
        main = list(d['main'])
        info.append((d, [x[0] for x in d['list']], copies, main, sum(copies) - sum(main)))
    # the worlds of each deck as the other's belief: D's first (C's table), then C's (D's table)
    beliefs = {}
    for k in (1, 0):
        d, names, copies, main, S = info[k]
        m = np.asarray(main, dtype=np.float64)
        if around == 'table':
            beliefs[k] = {0: sample_prior(copies, main, 0, S, worlds, rng) - m}
        else:
            beliefs[k] = {0: sample_prior(copies, main, 0, S, worlds, rng) - m,
                          1: sample_prior(copies, main, dealt_swaps[0], dealt_swaps[1], worlds, rng) - m}
    # (around the table, docs/ENGINE.md 261: both decks' opp_dealt 0 contexts first - a dealt opponent's base is the
    # other deck's choice for an opp_dealt 0 context)
    if around == 'table':
        order = [(r, c) for r in (0, 1) for c in contexts() if c[3] == 0] + [(r, c) for r in (0, 1) for c in contexts() if c[3] == 1]
    else:
        order = [(r, c) for r in (0, 1) for c in contexts()]
    solved, by_base = {}, {}
    for role, (g, f, l, o) in order:
        d, names, copies, main, S = info[role]
        if around == 'table' and o == 1:
            od, _, ocopies, omain, _ = info[1 - role]
            base = tuple(solved[(1 - role, opponent_context(g, f, l))][0])
            if (1 - role, base) not in by_base:
                by_base[(1 - role, base)] = sample_around(ocopies, base, omain, dealt_swaps[0], dealt_swaps[1], worlds, rng)
            B = by_base[(1 - role, base)]
        else:
            B = beliefs[1 - role][o].astype(np.float64)
        F = PairObjective(members, B, role, g, f, l, o)
        c, fv, sw = confident_climb(F, copies, main, z)
        solved[(role, (g, f, l, o))] = (c, fv, sw, F)
    tables, details = [], {}
    for role in (0, 1):
        d, names, copies, main, S = info[role]
        m = np.asarray(main, dtype=np.float64)
        key = 'wC' if role == 0 else 'wD'
        strong = strength_optimum([{'w': x[key]} for x in members], copies, main)
        choices, detail = [], []
        for (g, f, l, o) in contexts():
            c, fv, sw, F = solved[(role, (g, f, l, o))]
            choices.append([g, f, l, o, c])
            detail.append({'context': [g, f, l, o], 'deck': c, 'F': fv, 'F_main': float(F((np.asarray(main) - m)[None, :])[0]),
                           'swaps': [{'out': names[a], 'in': names[b], 'gain': gm, 'se': gs} for a, b, gm, gs in sw],
                           'z': z, 'argmax_in': {names[i]: strong[i] - main[i] for i in range(len(c)) if strong[i] > main[i]},
                           'in': {names[i]: c[i] - main[i] for i in range(len(c)) if c[i] > main[i]},
                           'out': {names[i]: main[i] - c[i] for i in range(len(c)) if c[i] < main[i]}})
            log(f'# {d["name"]} context game {g} first {f} lead {l} opp_dealt {o}: F {fv:.4f} (main '
                f'{detail[-1]["F_main"]:.4f}) in {detail[-1]["in"]} out {detail[-1]["out"]}')
        tables.append({'version': version, 'list': [[n, k] for n, k in zip(names, copies)], 'choices': choices})
        details[d['name']] = detail
    return tables, details


# ----------------------------------------------------------------------------- the self-play hook

def _counts75(main, side):
    from collections import Counter
    return dict(Counter(list(main) + list(side or [])))


def with_tables(spec, tables):
    """`spec` (JSON) with the choice tables `tables` (each {"version", "list", "choices"}) where the engine reads them -
    each on the deck whose 75 is its list: a metagame deck's entry (`"table"`, docs/ENGINE.md 240), a fixed spec's seat
    (`decks.pN_table`), or for a fixed mirror (one 75 both seats) pb3's one `"sideboard_table"` (docs/ENGINE.md 168.3,
    unchanged). A table that fits no deck, or two tables of one 75, are refused."""
    sp = json.loads(spec)
    tabs = [t for t in tables if t is not None]
    of = [{n: int(k) for n, k in t['list']} for t in tabs]
    placed = [0] * len(tabs)

    def hit(c75, where):
        h = [i for i, x in enumerate(of) if x == c75]
        if len(h) > 1:
            raise ValueError(f'{len(h)} choice tables of the 75 of {where} (one table per deck, docs/ENGINE.md 240)')
        for i in h:
            placed[i] += 1
        return tabs[h[0]] if h else None

    if 'metagame' in sp:
        for e in sp['metagame']['decks']:
            t = hit(_counts75(e['main'], e.get('sideboard')), f'metagame deck {e["name"]}')
            if t is not None:
                e['table'] = t
    else:
        d = sp['decks']
        c0, c1 = _counts75(d['p0'], d.get('p0_sideboard')), _counts75(d['p1'], d.get('p1_sideboard'))
        if c0 == c1:
            t = hit(c0, 'both seats')
            if t is not None:
                sp['sideboard_table'] = t
        else:
            for s_, c in ((0, c0), (1, c1)):
                t = hit(c, f'p{s_}')
                if t is not None:
                    d[f'p{s_}_table'] = t
    if not all(placed):
        raise ValueError(f'a choice table ({[t["version"] for t, n in zip(tabs, placed) if not n]}) fits no deck of the '
                         f'spec (its list is no deck\'s 75; docs/ENGINE.md 240)')
    return json.dumps(sp, separators=(',', ':'))


def with_table(spec, table):
    """pb3's one table (docs/ENGINE.md 168.3): `with_tables` of [table]."""
    return with_tables(spec, [table])


def has_tables(spec):
    """Does `spec` (a dict) carry a choice table of any form (docs/ENGINE.md 168.3, 223)?"""
    if 'sideboard_table' in spec:
        return True
    d = spec.get('decks')
    if isinstance(d, dict) and ('p0_table' in d or 'p1_table' in d):
        return True
    return any('table' in e for e in (spec.get('metagame') or {}).get('decks', []))


def version_tables(v):
    """A published version's choice tables (a list; None: no table): a mirror's one `table` (pb3's), two different 75s'
    `tables` (one per deck, docs/ENGINE.md 240)."""
    if v.get('tables'):
        return list(v['tables'])
    return [v['table']] if v.get('table') is not None else None


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
    if has_tables(sp):
        raise ValueError('matchup_every: the run\'s spec has a choice table of its own (the retrain writes them)')
    run = R.run
    used = [r for r in _rows(os.path.join(mdir(run), 'used.jsonl')) if r['iter'] == it]
    if used:
        v = used[-1]['version']
        t = None if v is None else version_tables(_load(os.path.join(mdir(run), v)))
        return None if t is None else with_tables(c['spec'], t)
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
    table = version_tables(_load(vs[-1][1]))
    _append(os.path.join(mdir(run), 'used.jsonl'), {'iter': it, 'version': name, 'table': table is not None,
                                                    'time': time.time()})
    if prev != name:
        R.log(f'iter {it} self-play: the chosen-60 seats sideboard by the matchup model\'s choice table'
              f'{"s" if table is not None and len(table) > 1 else ""} {name}' if table
              is not None else f'iter {it} self-play: the matchup model {name} has no table (it did not beat play / draw '
              f'on its held-out games by matchup_table_min_gain): the chosen-60 seats sideboard by search')
    return None if table is None else with_tables(c['spec'], table)


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
                  f'(gain {s.get("gain_nats"):+.5f} nats){"" if s.get("table", True) else ": no table"}, {s.get("seconds")} s'
                  + (f' - data: {s["data_note"]}' if s.get('data_note') else ''))
            if s.get('starved'):
                R.log(f'MATCHUP MODEL STARVED {name}: fitted on {s.get("games")} games (< {s.get("min_games")}: '
                      f'{s.get("min_games_per_iter")} a game per iteration of its window) - {s.get("data_note")}')
            if (s.get('data') or {}).get('excluded_games'):
                R.log(f'MATCHUP MODEL {name}: {s["data"]["excluded_games"]} games of its window LEFT OUT (records of '
                      f'another engine) - {s.get("data_note")}')
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
           '--members', str(int(c.get('matchup_members', 20))),
           '--min-games-per-iter', str(int(c.get('matchup_min_games_per_iter', 500)))]
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

def _mirror_version(cfg, games, specs, P, b, lo, name, val_iters, half_life, threads, worlds, log, min_gain, z,
                    members_n):
    """A mirror's version (pb3's, docs/ENGINE.md 168.3-168.4, unchanged): one model on the shared 75's count deltas, one
    table for every seat -> (the version's dict, report, tables or None, table changes, per-copy strengths)."""
    lst, main = P.decks[0]['list'], P.decks[0]['main']
    names = [x[0] for x in lst]
    copies = [int(x[1]) for x in lst]
    members, rep = select_and_fit(games, main, val_iters, half_life, threads, seed=b, log=log, members_n=members_n)
    dd = json.loads(cfg['spec']).get('dealt_decks') or {}
    sw = tuple(dd.get('swaps') or (0, sum(copies) - sum(main)))
    around = dealt_around(cfg['spec'])
    # (a model that does not beat play / draw on the held-out games by `min_gain` nats has not shown matchup
    # knowledge: its version publishes no table, and the chosen-60 seats sideboard by search until one does -
    # docs/ENGINE.md 168.4)
    if rep['gain_nats'] >= min_gain:
        table, detail = choice_table(members, names, copies, main, sw, name, worlds=worlds, seed=b, log=log, z=z,
                                     around=around)
        check_table(cfg['spec'], table)
    else:
        table, detail = None, []
        log(f'# held-out gain {rep["gain_nats"]:+.5f} nats < {min_gain}: no table in {name}')
    out = {'format': FORMAT, 'version': name, 'iter': b, 'window': [lo, b], 'list': lst, 'main': main,
           'members': members, 'report': rep, 'table': table, 'table_detail': detail, 'worlds': worlds,
           'min_gain': min_gain, 'swap_z': z,
           'dealt_swaps': list(sw), 'time': time.time()}
    if around != 'main':
        out['dealt_around'] = around
    changes = [{'context': x['context'], 'in': x['in'], 'out': x['out']} for x in detail]
    return out, rep, (None if table is None else [table]), changes, dict(zip(names, rep['per_copy']))


def _pair_version(cfg, games, specs, P, b, lo, name, val_iters, half_life, threads, worlds, log, min_gain, z, members_n):
    """Two different 75s' version (docs/ENGINE.md 240.3): the non-mirror model, then one table per deck, published only
    when the model beats its baseline (the pairing's base rate, play / draw, game 3) on the held-out games by
    `min_gain` nats -> as `_mirror_version`."""
    members, rep = select_and_fit_pair(games, specs, P, val_iters, half_life, threads, seed=b, log=log,
                                       members_n=members_n)
    dd = json.loads(cfg['spec']).get('dealt_decks') or {}
    S = [sum(int(x[1]) for x in d['list']) - sum(d['main']) for d in P.decks]
    sw = tuple(dd.get('swaps') or (0, min(S)))
    around = dealt_around(cfg['spec'])
    if rep['gain_nats'] >= min_gain:
        tables, detail = pair_choice_tables(members, P, sw, name, worlds=worlds, seed=b, log=log, z=z, around=around)
        check_tables(cfg['spec'], tables)
    else:
        tables, detail = None, {}
        log(f'# held-out gain {rep["gain_nats"]:+.5f} nats < {min_gain}: no table in {name}')
    out = {'format': FORMAT, 'version': name, 'iter': b, 'window': [lo, b], 'pairing': P.json(), 'model': 'pair',
           'members': members, 'report': rep, 'table': None, 'tables': tables, 'table_detail': detail,
           'worlds': worlds, 'min_gain': min_gain, 'swap_z': z, 'dealt_swaps': list(sw), 'time': time.time()}
    if around != 'main':
        out['dealt_around'] = around
    changes = [{'deck': k, 'context': x['context'], 'in': x['in'], 'out': x['out']} for k, xs in detail.items() for x in xs]
    return out, rep, tables, changes, rep['per_copy']


def replay_cache(run):
    """Where a run's retrains keep the records' replayed games (mtgz.az.bo3_records; one file per iteration)."""
    return os.path.join(mdir(run), 'replayed')


def retrain(run, b, lo, val_iters=2, half_life=16.0, threads=4, worlds=8000, log=print, min_gain=0.005, z=2.0,
            members_n=20, min_games_per_iter=500, source='auto', cache='run'):
    """The job (module docs): the run's self-play games of iterations lo..b (`load_games`: the game log, or every
    accepted game from the records, docs/ENGINE.md 273) -> status (status.json; a failure raises after writing it). The
    games' one pairing (`the_pairing`): a mirror's version is pb3's (one model, one table), two different 75s' the
    non-mirror model with one table per deck (docs/ENGINE.md 240). Fewer games than `min_games_per_iter` times the
    window's iterations: STARVED (published; `starved` in the status, the version's `data` and the index). `cache`:
    'run' (`replay_cache`), a directory, or None."""
    t0 = time.time()
    d = job_dir(run, b)
    os.makedirs(d, exist_ok=True)
    name = f'v{b:04d}'
    min_games = int(min_games_per_iter) * (b - lo + 1)
    status = {'ok': False, 'version': name, 'iter': b, 'window': [lo, b], 'half_life': half_life, 'started': t0,
              'min_games': min_games, 'min_games_per_iter': int(min_games_per_iter)}
    try:
        cfg = _load(os.path.join(run, 'config.json'))
        info = {}
        cdir = replay_cache(run) if cache == 'run' else cache
        try:
            games, specs = load_games(run, lo, b, source=source, jobs=max(1, int(threads)), cache=cdir, info=info,
                                      log=log)
        finally:
            status['data'] = info
            if info.get('source'):
                status['data_note'] = data_note(info) if 'games' in info else f'the {info["source"]}: failed'
        if cdir and info.get('source') == 'records':
            from mtgz.az import bo3_records as BR
            BR.prune_cache(cdir, lo)
        status['games'] = len(games)
        status['starved'] = len(games) < min_games
        log(f'# {len(games)} games of iterations {lo}-{b}: {status["data_note"]}')
        if status['starved']:
            log(f'# STARVED: {len(games)} games < {min_games} ({min_games_per_iter} a game per iteration of the window)')
        if not games:
            raise RuntimeError(f'no self-play game in iterations {lo}-{b}: {status["data_note"]} (the game log: '
                               f'{os.path.join(run, "games")}, the records: {os.path.join(run, "records")})')
        P = the_pairing(specs, games)
        make = _mirror_version if P.mirror else _pair_version
        out, rep, tables, changes, per_copy = make(cfg, games, specs, P, b, lo, name, val_iters, half_life, threads,
                                                   worlds, log, min_gain, z, members_n)
        out['data'] = dict(info, note=status['data_note'], starved=status['starved'], min_games=min_games)
        staged = os.path.join(d, f'{name}.json')
        with open(staged, 'w', encoding='utf-8') as f:
            json.dump(out, f)
        dst = os.path.join(mdir(run), f'{name}.json')
        tmp = dst + f'.{os.getpid()}.tmp'
        shutil.copyfile(staged, tmp)
        os.replace(tmp, dst)
        status.update(ok=True, val_logloss=rep['val_logloss'], val_logloss_pd=rep['val_logloss_pd'],
                      gain_nats=rep['gain_nats'], selected=rep['selected'], table=tables is not None,
                      seconds=round(time.time() - t0, 1))
        _append(os.path.join(mdir(run), 'index.jsonl'), {
            'version': name, 'iter': b, 'window': [lo, b], 'games': rep['games'], 'val_games': rep['val_games'],
            'selected': rep['selected'], 'val_logloss': rep['val_logloss'], 'val_logloss_pd': rep['val_logloss_pd'],
            'gain_nats': rep['gain_nats'], 'val_logloss_dealt': rep['val_logloss_dealt'],
            'val_logloss_pd_dealt': rep['val_logloss_pd_dealt'], 'per_copy': per_copy,
            'table': tables is not None, 'table_changes': changes, 'source': info.get('source'),
            'excluded_games': info.get('excluded_games', 0), 'starved': status['starved'], 'time': time.time()})
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
    against the main - each card's mean copies in / out, and the most common 60s. Two different 75s (docs/ENGINE.md
    223): per deck, its seats against its own main (keys '<deck> <mode> game <g>')."""
    from collections import Counter
    games, specs = load_games(run, lo, hi)
    if not games:
        return {'games': 0}
    P = the_pairing(specs, games)
    out = {}
    for g in games:
        a = P.seat_of_c(specs[g['spec']])
        for p in (0, 1):
            mode = g['mode'][p]
            if mode not in ('table', 'chosen'):
                continue
            deck = P.decks[0 if p == a else 1]
            names = [x[0] for x in deck['list']]
            main = deck['main']
            k = f'{mode} game {g["game"]}' if P.mirror else f'{deck["name"]} {mode} game {g["game"]}'
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
    r.add_argument('--min-games-per-iter', type=int, default=500)
    r.add_argument('--source', default='auto', choices=('auto', 'log', 'records'))
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
    f.add_argument('--source', default='auto', choices=('auto', 'log', 'records'))
    f.add_argument('--jobs', type=int, default=1, help='processes replaying the records')
    f.add_argument('--cache', default=None, help='a directory for the records\' replayed games (default: none)')
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
                    z=a.swap_z, members_n=a.members, min_games_per_iter=a.min_games_per_iter, source=a.source)
        print(json.dumps({k: s[k] for k in ('version', 'games', 'val_logloss', 'val_logloss_pd', 'gain_nats', 'seconds')}))
    elif a.cmd == 'fit':
        lo, hi = rng(a.iters)
        info = {}
        games, specs = load_games(a.run, lo, hi, source=a.source, jobs=a.jobs, cache=a.cache, info=info,
                                  log=lambda m: print(m, flush=True))
        print(f'data: {data_note(info)}')
        P = the_pairing(specs, games)
        if not P.mirror:
            members, rep = select_and_fit_pair(games, specs, P, a.val_iters, a.half_life, a.threads, members_n=a.members)
            print(f'{rep["games"]} games of {P.decks[0]["name"]} vs {P.decks[1]["name"]} ({rep["train_games"]} train, '
                  f'{rep["val_games"]} validation of iterations {rep["val_iterations"]}); selected {rep["selected"]}')
            print(f'held-out log loss {rep["val_logloss"]:.5f} vs the baseline {rep["val_logloss_pd"]:.5f}: gain '
                  f'{rep["gain_nats"]:+.5f} nats a game')
            for dk in P.decks:
                print(f'{dk["name"]} per copy (ensemble mean +- sd):')
                rp = {'per_copy': rep['per_copy'][dk['name']], 'per_copy_se': rep['per_copy_se'][dk['name']]}
                for line in per_copy_lines([x[0] for x in dk['list']], rp):
                    print(line)
            out = {'report': rep, 'members': members, 'pairing': P.json(), 'data': info}
            if a.table:
                S = min(sum(int(x[1]) for x in dk['list']) - sum(dk['main']) for dk in P.decks)
                out['tables'], out['table_detail'] = pair_choice_tables(members, P, (0, S), 'fit', worlds=a.worlds,
                                                                        z=a.swap_z)
            if a.out:
                os.makedirs(a.out, exist_ok=True)
                with open(os.path.join(a.out, 'matchup_fit.json'), 'w', encoding='utf-8') as fh:
                    json.dump(out, fh, indent=1)
            return
        lst, main_ = P.decks[0]['list'], P.decks[0]['main']
        names = [x[0] for x in lst]
        members, rep = select_and_fit(games, main_, a.val_iters, a.half_life, a.threads, members_n=a.members)
        print(f'{rep["games"]} games ({rep["train_games"]} train, {rep["val_games"]} validation of iterations '
              f'{rep["val_iterations"]}); selected {rep["selected"]}')
        print(f'held-out log loss {rep["val_logloss"]:.5f} vs play / draw only {rep["val_logloss_pd"]:.5f}: gain '
              f'{rep["gain_nats"]:+.5f} nats a game (the value head\'s bar: +0.005)')
        print('per copy (w: logit per copy beyond the main, ensemble mean +- sd):')
        print('\n'.join(per_copy_lines(names, rep)))
        out = {'report': rep, 'members': members, 'list': lst, 'main': main_, 'data': info}
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
                deck = f'{ch["deck"]} ' if 'deck' in ch else ''
                print(f'   {deck}context {ch["context"]}: in {ch["in"]} out {ch["out"]}')
    elif a.cmd == 'census':
        lo, hi = rng(a.iters)
        print(json.dumps(census(a.run, lo, hi), indent=1))


if __name__ == '__main__':
    main()
