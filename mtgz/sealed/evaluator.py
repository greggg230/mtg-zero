"""S4a, the deck evaluator (docs/ENGINE.md 108; DESIGN.md 9.1 step 2): a fast learned rating of a 40-card Edge of
Eternities deck - its probability of winning a best-of-one game against the field - trained on a run's own logged
games (`mtgz.sealed.games`), with the play network's pre-game value as a low-noise auxiliary target.

* **The dataset** (`dataset`): one row per (game, seat) of the run's finished games: the seat's deck as card counts
  over the EoE table (`vocab`: the engine's 320 names, sorted), the opponent's deck (its row), both builder modes, who
  piloted (a network seat searched; a bot did not), the iteration (the first metrics line after the game) and the seat's
  result (win 1, draw 0.5, loss 0). Held out **by game**: a game's two rows are always on one side (`split_of`: by
  iteration - the test the iterations from 40 on, which no checkpoint up to 39 trained on, the validation the 4 before
  them - or by a hash of the game seed; evaluation games, whose 128 fixed decks repeat every iteration, never held out).
* **The pre-game value** (`pregame_values`): each logged game's start position rebuilt from its spec line and seed (no
  action precedes it: version-1 lines rebuild too), the first decision of the game - CR 103.1's choice of who plays
  first, before any hand is drawn - with the chooser forced to each seat (the spec's `chooser`; the decks do not depend
  on it), and the checkpoint's value of each seat's own view (`encode(p)`: P(p wins), a draw 0.5). Hidden decklists:
  the view holds the seat's own deck and pool, never the opponent's; the encoding is the same in every world the
  viewer's `determinize` deals (checked), so one evaluation per (seat, chooser) is the average over dealt worlds.
  `v[game, seat, chooser]`; the target `pregame_target` is the mean over the two choosers (the value choice's own
  average, docs/ENGINE.md 107.2). By one checkpoint for every game, or (`'played'`) by the checkpoint that played each
  game: a network values the games it trained on with their results half-memorised (docs/ENGINE.md 108.2), so only
  the playing checkpoint's value is a leak-free target.
* **The models** (`DeckNet`, `fit`): a constant; a logistic regression on deck features (`FEATURES`: lands, curve,
  creatures, colours, splash; with or without the builder's mode); a linear card-value model (L2 logistic regression on
  card counts plus the deck features: its coefficients are card ranking v0); a set network (DeepSets over card
  embeddings, a per-card linear term and a pairwise term, card vectors interacting by dot product). Every model sees only
  its own deck (the deck against the field: decklists are hidden in sealed). The training target is the distillation
  mix y = (1 - lam) result + lam value under log loss.
* **The evaluator** (`Evaluator`): a trained ensemble, saved with its card table and feature scaling; `score(decks)`;
  `rank(pool, candidates)` checks the candidates are builds of the pool and scores them (the picker of
  `python -m mtgz.sealed.compare evalpick`).
* **Card ranking v0** (`card_values`): per card, the evaluator's change in win probability when one copy replaces a
  card of a typical deck of its colours, over the ensemble (its spread the uncertainty).
* **The yardstick** (`seventeen_lands_rates`): 17lands' EoE Premier Sealed per-card game-in-hand win rates. **A
  yardstick only** (DESIGN.md 0): a Spearman correlation reported, never an input, target or prior.

    python -m mtgz.sealed.evaluator values RUN --checkpoint CKPT --out VALUES.npz [--threads 4]
    python -m mtgz.sealed.evaluator values RUN --checkpoint played --models RUN/models --out VALUES.npz
    python -m mtgz.sealed.evaluator train RUN --values VALUES.npz --out DIR [--split time|hash] --judge CKPT
    python -m mtgz.sealed.evaluator rank --evaluator DIR/evaluator_set.pt --pool-seed 2026 [--k 32]

CPU only by default (`--device cpu`, at most `--threads` torch threads); deterministic from its seeds. Pure self-play:
the run's own games and networks train it.
"""
import argparse
import functools
import json
import math
import os
import sys
import time

import numpy as np

from . import cards as C
from . import games as GL

MASK64 = (1 << 64) - 1
SPLIT_SALT = 0x5EA1_E7A1_0000_0108
COLOURS = 'WUBRG'
PAIRS = tuple(a + b for i, a in enumerate(COLOURS) for b in COLOURS[i + 1:])
BASIC_COLOUR = {'Plains': 'W', 'Island': 'U', 'Swamp': 'B', 'Mountain': 'R', 'Forest': 'G'}
MODES = ('best', 'explore', 'chosen', 'given')


def split64(x):
    """splitmix64's output function (the engine's `rng::split64`)."""
    z = (x + 0x9E3779B97F4A7C15) & MASK64
    z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK64
    z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK64
    return z ^ (z >> 31)


def mix(*xs):
    h = 0
    for x in xs:
        h = split64(h ^ (int(x) & MASK64))
    return h


# ----------------------------------------------------------------------------- the card table and deck features

@functools.lru_cache(None)
def vocab():
    """The card table's names (the engine's EoE table, `cards.ir_names()`: the set but Paradox Haze, and the basics),
    sorted: a deck is a count vector over these."""
    return tuple(sorted(C.ir_names()))


@functools.lru_cache(None)
def index():
    return {n: i for i, n in enumerate(vocab())}


@functools.lru_cache(None)
def card_table():
    """Per-card attribute arrays over `vocab()` (from data/sealed/eoe_set.json): land, basic, creature, mana value,
    colours [V, 5], a basic's colour [V, 5]."""
    info = C.cards()
    V = len(vocab())
    t = {k: np.zeros(V, np.float32) for k in ('land', 'basic', 'creature', 'mv')}
    t['colour'] = np.zeros((V, 5), np.float32)
    t['basic_colour'] = np.zeros((V, 5), np.float32)
    for i, n in enumerate(vocab()):
        c = info.get(n)
        if c is None:
            raise ValueError(f'{n}: in the engine table but not in eoe_set.json')
        tl = c['type_line']
        t['land'][i] = 'Land' in tl
        t['basic'][i] = n in BASIC_COLOUR
        t['creature'][i] = 'Creature' in tl
        t['mv'][i] = c['mana_value']
        for ch in c['colors']:
            t['colour'][i, COLOURS.index(ch)] = 1.0
        if n in BASIC_COLOUR:
            t['basic_colour'][i, COLOURS.index(BASIC_COLOUR[n])] = 1.0
    return t


CURVE = ('mv<=1', 'mv2', 'mv3', 'mv4', 'mv5', 'mv6+')
FEATURES = (('lands', 'nonbasic_lands', 'creatures', 'noncreature_spells') + CURVE + ('mean_mv', 'colours', 'splash_spells',
            'colourless_spells') + tuple(f'spells_{c}' for c in COLOURS) + tuple(f'basics_{c}' for c in COLOURS)
            + ('mono',) + tuple(f'pair_{p}' for p in PAIRS))


def counts_matrix(decks):
    """Decks (lists of names or {name: count}) -> float32 counts [n, V]. An unknown name is an error."""
    ix = index()
    X = np.zeros((len(decks), len(ix)), np.float32)
    for r, d in enumerate(decks):
        items = d.items() if isinstance(d, dict) else ((n, 1) for n in d)
        for n, k in items:
            if n not in ix:
                raise ValueError(f'{n!r}: not a card of the EoE table')
            X[r, ix[n]] += k
    return X


def deck_features(X):
    """Deck features of count rows X [n, V] -> [n, len(FEATURES)] (unscaled): land counts, creatures, the curve of the
    nonland spells, the colours by nonland spells (the main pair: the two colours with the most spells; splash spells:
    those with a colour outside it), basics per colour, the pair one-hot."""
    t = card_table()
    X = np.asarray(X, np.float32)
    spell = 1.0 - t['land']
    lands = X @ t['land']
    nonbasic = X @ (t['land'] * (1 - t['basic']))
    creatures = X @ (t['creature'] * spell)
    spells = X @ spell
    mv = t['mv']
    bins = [mv <= 1, mv == 2, mv == 3, mv == 4, mv == 5, mv >= 6]
    curve = np.stack([X @ (spell * b) for b in bins], 1)
    mean_mv = (X @ (spell * mv)) / np.maximum(spells, 1)
    per = X @ (t['colour'] * spell[:, None])                    # spells holding each colour [n, 5]
    top = np.argsort(-per, axis=1, kind='stable')[:, :2]
    main = np.zeros_like(per)
    np.put_along_axis(main, top, 1.0, axis=1)
    main *= per > 0
    outside = ((t['colour'][None, :, :] * (1 - main[:, None, :])).sum(-1) > 0).astype(np.float32)   # [n, V]
    splash = (X * outside * spell[None, :]).sum(1)
    colourless = X @ (spell * (t['colour'].sum(1) == 0))
    ncol = (per > 0).sum(1).astype(np.float32)
    basics = X @ t['basic_colour']
    mono = (main.sum(1) == 1).astype(np.float32)
    pair = np.zeros((X.shape[0], len(PAIRS)), np.float32)
    for k, p in enumerate(PAIRS):
        pair[:, k] = main[:, COLOURS.index(p[0])] * main[:, COLOURS.index(p[1])]
    F = np.concatenate([np.stack([lands, nonbasic, creatures, spells - creatures], 1), curve, mean_mv[:, None],
                        ncol[:, None], splash[:, None], colourless[:, None], per, basics, mono[:, None], pair], 1)
    assert F.shape[1] == len(FEATURES)
    return F.astype(np.float32)


def deck_colours(X):
    """The main colours of count rows (the two colours with the most nonland spells; one for a mono deck) as sets."""
    t = card_table()
    per = np.asarray(X, np.float32) @ (t['colour'] * (1 - t['land'])[:, None])
    out = []
    for row in per:
        top = [COLOURS[i] for i in np.argsort(-row, kind='stable')[:2] if row[i] > 0]
        out.append(frozenset(top))
    return out


# ----------------------------------------------------------------------------- the dataset

def game_key(g):
    """A logged game's identity (its file, seed and time): stable as the log grows."""
    return f"{g['file']}:{g['seed']}:{g['time']}"


def split_of(g, it=None, how='hash', test=0.15, val=0.10, time_from=40, val_iters=4):
    """'train', 'val' or 'test' for a game (both its rows). Evaluation games (the run's 128 fixed decks, replayed every
    iteration) always train: holding them out would test decks seen in training. `hash`: a self-play game is test with
    probability `test` by its seed's hash, else val with `val`; `time`: a self-play game of iteration >= `time_from`
    (or of the iteration in progress) is test, of the `val_iters` iterations before it val (the validation mirrors
    the test: the most recent games), the rest train."""
    if g['phase'] == 'eval':
        return 'train'
    h = mix(g['seed'], SPLIT_SALT) / 2.0 ** 64
    if how == 'time':
        if it is None or it >= time_from:
            return 'test'
        return 'val' if it >= time_from - val_iters else 'train'
    if how != 'hash':
        raise ValueError(f'split {how!r}: hash or time')
    return 'test' if h < test else ('val' if h < test + val else 'train')


def dataset(run, games=None, specs=None, how='hash', pilots='nn', **split_kw):
    """The rows of a run's finished games (module docs) -> dict of arrays: 'X' counts [n, V], 'opp' (the opponent's
    row), 'game' (the game's index in 'keys'), 'seat', 'result', 'mode' / 'opp_mode' (index in MODES), 'searched' /
    'opp_searched', 'iter' (-1: the iteration in progress), 'phase' (0 self-play, 1 evaluation), 'split' (0 train,
    1 val, 2 test), 'pool_seed', and 'keys' (game_key per game), 'counts' (the excluded games by pilots).
    `pilots`: 'nn' keeps the games both seats searched (self-play, Elo), 'all' every game."""
    if games is None:
        specs, games = GL.load(run)
    times = GL.iteration_times(run) if run else []
    keep, skipped = [], {'aborted': 0, 'bot_seat': 0}
    for g in games:
        if g['result'] is None or g['aborted']:
            skipped['aborted'] += 1
            continue
        if pilots == 'nn' and not all(g['searched']):
            skipped['bot_seat'] += 1
            continue
        keep.append(g)
    n = 2 * len(keep)
    X = counts_matrix([s['deck'] for g in keep for s in g['seats']])
    out = {'X': X, 'keys': [game_key(g) for g in keep], 'skipped': skipped}
    col = lambda f, dt: np.array([f(g, p) for g in keep for p in (0, 1)], dt)  # noqa: E731
    its = [GL.game_iteration(g, times) for g in keep]
    sp = {'train': 0, 'val': 1, 'test': 2}
    splits = [sp[split_of(g, it, how, **split_kw)] for g, it in zip(keep, its)]
    out['game'] = np.repeat(np.arange(len(keep)), 2)
    out['seat'] = np.tile(np.arange(2), len(keep))
    out['opp'] = np.arange(n) ^ 1
    out['result'] = col(lambda g, p: float(g['result'][p]), np.float32)
    out['mode'] = col(lambda g, p: MODES.index(g['seats'][p]['mode']), np.int8)
    out['opp_mode'] = col(lambda g, p: MODES.index(g['seats'][1 - p]['mode']), np.int8)
    out['searched'] = col(lambda g, p: bool(g['searched'][p]), bool)
    out['opp_searched'] = col(lambda g, p: bool(g['searched'][1 - p]), bool)
    out['phase'] = col(lambda g, p: int(g['phase'] == 'eval'), np.int8)
    out['pool_seed'] = col(lambda g, p: g['seats'][p]['pool_seed'], np.uint64)
    out['iter'] = np.repeat(np.array([-1 if it is None else it for it in its], np.int32), 2)
    out['split'] = np.repeat(np.array(splits, np.int8), 2)
    return out


def describe(ds):
    s = ds['split']
    g = lambda m: int(m.sum() // 2)  # noqa: E731
    return {'rows': int(len(s)), 'games': g(np.ones_like(s, bool)),
            'train_games': g(s == 0), 'val_games': g(s == 1), 'test_games': g(s == 2),
            'selfplay_games': g(ds['phase'] == 0), 'eval_games': g(ds['phase'] == 1),
            'iters': [int(ds['iter'][ds['iter'] >= 0].min()) if (ds['iter'] >= 0).any() else None,
                      int(ds['iter'].max())], 'in_progress_games': g(ds['iter'] < 0), 'skipped': ds['skipped']}


# ----------------------------------------------------------------------------- the play network's pre-game value

class NetValue:
    """A checkpoint's value of value-only rows (P(the row's viewer wins)), batched on CPU."""

    def __init__(self, checkpoint, device='cpu', batch=256):
        from zero.infer import NetEvaluator
        from zero.net import load
        import mtgz.az  # noqa: F401
        m, _ = load(checkpoint, device, game='mtg_eoe')
        self.ev = NetEvaluator(m, device)
        self.batch = int(batch)

    def __call__(self, rows):
        out = np.zeros(len(rows), np.float64)
        for i in range(0, len(rows), self.batch):
            x = np.stack(rows[i:i + self.batch]).astype(np.float32)
            b = x.shape[0]
            _, val, _ = self.ev.evaluate(x, np.full(b, 2, np.int32), np.zeros(b + 1, np.int32), np.zeros(0, np.int32))
            out[i:i + b] = np.asarray(val)[:, 0]
        return out


def _engine():
    import mtgz.az  # noqa: F401
    from zero import games as G
    return G.engine()


def start_position(spec_line, seed, chooser):
    """A logged game's first decision (CR 103.1's play-or-draw choice, before any hand) with `chooser` forced: the
    game from its spec line (`cardcredit.replay_spec`) and seed. Checks it is the chooser's turn-0 decision."""
    from .cardcredit import replay_spec
    sp = json.loads(replay_spec(spec_line))
    sp['chooser'] = int(chooser)
    g = _engine().Game('mtg_eoe', json.dumps(sp), 2, int(seed))
    if g.over or g.rounds != 0 or g.to_move != chooser:
        raise ValueError(f'game {seed}: the start position is not chooser {chooser}\'s first decision')
    return g


def played_checkpoint(models, it, last):
    """The checkpoint that played a game of iteration `it` (its self-play and evaluation use the newest network,
    trained at the end of the iteration before): models/iter_{it - 1}.pt; a game of the iteration in progress (None),
    the newest metrics line's iteration `last`."""
    k = (last if it is None else it - 1)
    return os.path.join(models, f'iter_{max(0, k):04d}.pt')


def pregame_values(run, checkpoint, out=None, device='cpu', batch=256, threads=4, worlds_check=20, models=None,
                   log=None):
    """The play network's pre-game value of every finished logged game (module docs): v [n, seat, chooser] with the
    games' keys. `checkpoint`: one checkpoint for every game, or 'played' with `models` (a run's models directory): each
    game valued by the network that played it (`played_checkpoint`), which never trained on its result. `worlds_check`:
    for this many games, every seat's encoding is compared with its encoding in 3 worlds its `determinize` deals (they
    must be equal: then the value is the average over worlds). Writes `out` (.npz)."""
    import torch
    torch.set_num_threads(max(1, int(threads)))
    specs, games = GL.load(run)
    games = [g for g in games if g['result'] is not None and not g['aborted']]
    if not games:
        raise SystemExit(f'no finished games in {run}')
    if checkpoint == 'played':
        if not models:
            raise ValueError("checkpoint 'played' needs the run's models directory")
        times = GL.iteration_times(run)
        if not times:
            raise ValueError(f'{run}: no metrics lines (the games\' iterations)')
        cks = [played_checkpoint(models, GL.game_iteration(g, times), times[-1][1]) for g in games]
    else:
        cks = [checkpoint] * len(games)
    missing = sorted({c for c in cks if not os.path.isfile(c)})
    if missing:
        raise FileNotFoundError(f'checkpoints missing: {missing[:5]}')
    v = np.zeros((len(games), 2, 2), np.float64)
    t0 = time.time()
    worlds_differ = worlds_checked = done = 0
    for ck in sorted(set(cks)):
        net = NetValue(ck, device, batch)
        mine = [k for k, c in enumerate(cks) if c == ck]
        rows, where = [], []
        for n, k in enumerate(mine):
            g = games[k]
            for c in (0, 1):
                gm = start_position(specs[g['spec']], g['seed'], c)
                for p in (0, 1):
                    e = np.asarray(gm.encode(p), np.float32)
                    rows.append(e)
                    where.append((k, p, c))
                    if k < worlds_check:
                        for s in range(3):
                            w = gm.clone()
                            w.determinize(p, mix(g['seed'], p, c, s))
                            worlds_checked += 1
                            worlds_differ += not np.array_equal(e, np.asarray(w.encode(p), np.float32))
            if len(rows) >= 4 * batch or n == len(mine) - 1:
                for (kk, p, c), x in zip(where, net(rows)):
                    v[kk, p, c] = x
                rows, where = [], []
            done += 1
            if log and done % 2000 == 0:
                log(f'# {done} of {len(games)} games valued ({time.time() - t0:.0f} s)')
    if worlds_differ:
        raise RuntimeError(f'{worlds_differ} of {worlds_checked} dealt worlds changed a viewer\'s pre-game encoding')
    meta = {'run': run, 'checkpoint': checkpoint, 'models': models, 'checkpoints': len(set(cks)), 'games': len(games),
            'seconds': round(time.time() - t0, 1), 'worlds_checked': worlds_checked,
            'v1_games': sum(g.get('v', 1) < 2 for g in games), 'last_time': max(g['time'] for g in games)}
    if out:
        os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
        np.savez_compressed(out, v=v, keys=np.array([game_key(g) for g in games]), meta=json.dumps(meta))
    return {'v': v, 'keys': [game_key(g) for g in games], 'meta': meta}


def load_values(path):
    z = np.load(path, allow_pickle=False)
    return {'v': z['v'], 'keys': [str(k) for k in z['keys']], 'meta': json.loads(str(z['meta']))}


def attach_values(ds, vals):
    """Put the pre-game values on the dataset's rows ('v' [n, 2]: chooser me, chooser the opponent) and keep only
    the games the values file holds (its snapshot of the log) -> a new dataset."""
    at = {k: i for i, k in enumerate(vals['keys'])}
    have = np.array([k in at for k in ds['keys']])
    rows = have[ds['game']]
    keep_games = np.nonzero(have)[0]
    remap = -np.ones(len(ds['keys']), np.int64)
    remap[keep_games] = np.arange(len(keep_games))
    out = {k: (x[rows] if isinstance(x, np.ndarray) and len(x) == len(rows) else x) for k, x in ds.items()}
    out['keys'] = [ds['keys'][i] for i in keep_games]
    out['game'] = remap[ds['game'][rows]]
    out['opp'] = np.arange(int(rows.sum())) ^ 1
    vv = vals['v']
    gi = np.array([at[out['keys'][g]] for g in out['game']])
    seat = out['seat']
    out['v'] = np.stack([vv[gi, seat, seat], vv[gi, seat, 1 - seat]], 1).astype(np.float32)
    return out


def pregame_target(ds):
    """The auxiliary target: the pre-game value averaged over who chooses play or draw."""
    return ds['v'].mean(1)


class NetJudge:
    """The play network's pre-game value of any given deck (the value choice's rating, docs/ENGINE.md 107.2): the deck
    and its sideboard (the rest of its pool) in seat 0 of a hidden-decklist sealed game, the opponent seat a placeholder
    (hidden: no encoding of seat 0 shows it), seat 0's view at the play-or-draw decision with either chooser ->
    [n, 2] (seat 0 chooses, the opponent chooses). The given-deck game encodes seat 0 exactly as the logged spec's game
    with the same deck and pool (tests/test_evaluator.py)."""

    def __init__(self, checkpoint, device='cpu', batch=256, sealed=None):
        self.net = NetValue(checkpoint, device, batch)
        self.sealed = dict(sealed or {'set': 'eoe', 'collation': 'arena'})

    def spec(self, deck, sideboard, chooser):
        s = dict(self.sealed)
        s['p0'] = {'deck': list(deck), 'sideboard': list(sideboard)}
        s['p1'] = {'deck': list(deck), 'sideboard': []}
        for k in ('best_share', 'deck_choice'):
            s.pop(k, None)
        return {'sealed': s, 'max_turns': 60, 'chooser': int(chooser)}

    def values(self, decks):
        """decks: [(deck names, sideboard names)] -> [n, 2]."""
        eng = _engine()
        rows = []
        for deck, side in decks:
            for c in (0, 1):
                g = eng.Game('mtg_eoe', json.dumps(self.spec(deck, side, c)), 2, 0)
                rows.append(np.asarray(g.encode(0), np.float32))
        return self.net(rows).reshape(-1, 2) if rows else np.zeros((0, 2))


# ----------------------------------------------------------------------------- the models

def _nn():
    import torch
    return torch


def make_net(kind, V, F, d=32, k=8, hidden=64):
    """A deck model: 'const' (a bias), 'feat' (logistic on the deck features), 'linear' (logistic on card counts and
    the features: card values), 'set' (the linear terms, DeepSets over card embeddings - their mean over the 40 cards
    with the features through an MLP - and a pairwise term: 0.5 (|sum n_c u_c|^2 - sum n_c |u_c|^2) / 40, every pair of
    the deck's cards interacting by the dot product of their vectors u)."""
    torch = _nn()

    class DeckNet(torch.nn.Module):
        def __init__(self):
            super().__init__()
            self.kind = kind
            self.bias = torch.nn.Parameter(torch.zeros(1))
            if kind in ('feat', 'linear', 'set'):
                self.wf = torch.nn.Parameter(torch.zeros(F))
            if kind in ('linear', 'set'):
                self.wc = torch.nn.Parameter(torch.zeros(V))
            if kind == 'set':
                self.emb = torch.nn.Parameter(torch.randn(V, d) * 0.1)
                self.u = torch.nn.Parameter(torch.randn(V, k) * 0.05)
                self.mlp = torch.nn.Sequential(torch.nn.Linear(d + F, hidden), torch.nn.ReLU(), torch.nn.Linear(hidden, 1))
                torch.nn.init.zeros_(self.mlp[2].weight)
                torch.nn.init.zeros_(self.mlp[2].bias)
            elif kind not in ('const', 'feat', 'linear'):
                raise ValueError(f'model kind {kind!r}')

        def forward(self, X, Fz):
            z = self.bias.expand(X.shape[0])
            if self.kind != 'const':
                z = z + Fz @ self.wf
            if self.kind in ('linear', 'set'):
                z = z + X @ self.wc
            if self.kind == 'set':
                h = X @ self.emb / 40.0
                z = z + self.mlp(torch.cat([h, Fz], 1)).squeeze(1)
                s = X @ self.u
                z = z + 0.5 * ((s * s).sum(1) - X @ (self.u * self.u).sum(1)) / 40.0
            return z

        def penalty(self, l2_cards, l2_other):
            p = 0.0
            if self.kind != 'const':
                p = p + l2_other * (self.wf ** 2).sum()
            if self.kind in ('linear', 'set'):
                p = p + l2_cards * (self.wc ** 2).sum()
            if self.kind == 'set':
                p = p + l2_other * ((self.emb ** 2).sum() + (self.u ** 2).sum()
                                    + sum((m.weight ** 2).sum() for m in self.mlp if hasattr(m, 'weight')))
            return p

    return DeckNet()


class Scaler:
    """Standardises the deck features with the training rows' mean and sd (sd floored)."""

    def __init__(self, F=None, mean=None, std=None):
        if F is not None:
            mean, std = F.mean(0), np.maximum(F.std(0), 1e-3)
        self.mean, self.std = np.asarray(mean, np.float32), np.asarray(std, np.float32)

    def __call__(self, F):
        return ((F - self.mean) / self.std).astype(np.float32)


def fit(kind, X, Fz, y, *, l2_cards=1e-3, l2_other=1e-4, weights=None, val=None, seed=0, hyper=None, epochs=60,
        batch=256, lr=3e-3, patience=6):
    """One model fitted to targets y in [0, 1] by log loss + L2 (the mean loss per row plus the penalty). Linear kinds:
    full-batch L-BFGS (weighted rows: a bootstrap's counts). 'set': Adam on minibatches (a bootstrap's rows drawn with
    replacement), early stopping on `val` = (X, Fz, y) -> the trained net."""
    torch = _nn()
    torch.manual_seed(seed)
    V, F = X.shape[1], Fz.shape[1]
    net = make_net(kind, V, F, **(hyper or {}))
    Xt, Ft, yt = torch.from_numpy(X), torch.from_numpy(Fz), torch.from_numpy(np.asarray(y, np.float32))
    bce = torch.nn.functional.binary_cross_entropy_with_logits
    if kind != 'set':
        w = None if weights is None else torch.from_numpy(np.asarray(weights, np.float32))
        opt = torch.optim.LBFGS(net.parameters(), lr=1.0, max_iter=300, history_size=20, line_search_fn='strong_wolfe',
                                tolerance_grad=1e-8, tolerance_change=1e-12)

        def closure():
            opt.zero_grad()
            z = net(Xt, Ft)
            loss = (bce(z, yt, weight=w, reduction='sum') / (w.sum() if w is not None else len(yt))
                    + net.penalty(l2_cards, l2_other))
            loss.backward()
            return loss
        opt.step(closure)
        return net.eval()
    rng = np.random.default_rng(seed)
    pick = np.arange(len(yt)) if weights is None else np.repeat(np.arange(len(yt)), np.asarray(weights, np.int64))
    opt = torch.optim.Adam(net.parameters(), lr=lr)
    best, best_state, bad = math.inf, None, 0
    for _ in range(epochs):
        net.train()
        order = rng.permutation(pick)
        for i in range(0, len(order), batch):
            j = torch.from_numpy(order[i:i + batch])
            opt.zero_grad()
            loss = bce(net(Xt[j], Ft[j]), yt[j]) + net.penalty(l2_cards, l2_other)
            loss.backward()
            opt.step()
        if val is not None:
            net.eval()
            with torch.no_grad():
                vl = float(bce(net(torch.from_numpy(val[0]), torch.from_numpy(val[1])),
                               torch.from_numpy(np.asarray(val[2], np.float32))))
            if vl < best - 1e-5:
                best, bad = vl, 0
                best_state = {k: v.clone() for k, v in net.state_dict().items()}
            else:
                bad += 1
                if bad >= patience:
                    break
    if best_state is not None:
        net.load_state_dict(best_state)
    return net.eval()


def predict(nets, X, Fz, batch=8192):
    """The ensemble's mean win probability of count rows X with scaled features Fz."""
    torch = _nn()
    out = np.zeros(len(X), np.float64)
    with torch.no_grad():
        for net in nets:
            for i in range(0, len(X), batch):
                z = net(torch.from_numpy(X[i:i + batch]), torch.from_numpy(Fz[i:i + batch]))
                out[i:i + batch] += torch.sigmoid(z).double().numpy()
    return out / max(1, len(nets))


# ----------------------------------------------------------------------------- the metrics

def rankdata(x):
    """Ranks (1-based) with ties averaged."""
    x = np.asarray(x, np.float64)
    order = np.argsort(x, kind='stable')
    r = np.empty(len(x))
    r[order] = np.arange(1, len(x) + 1)
    _, inv, cnt = np.unique(x, return_inverse=True, return_counts=True)
    sums = np.bincount(inv, weights=r)
    return (sums / cnt)[inv]


def pearson(a, b):
    a, b = np.asarray(a, np.float64), np.asarray(b, np.float64)
    if len(a) < 2 or a.std() == 0 or b.std() == 0:
        return None
    return float(np.corrcoef(a, b)[0, 1])


def spearman(a, b):
    return pearson(rankdata(a), rankdata(b))


def logloss(p, r):
    p = np.clip(np.asarray(p, np.float64), 1e-6, 1 - 1e-6)
    r = np.asarray(r, np.float64)
    return float(-np.mean(r * np.log(p) + (1 - r) * np.log(1 - p)))


def game_ranking(p, ds, rows):
    """Over the decisive games whose two rows are both in `rows`: the share where the winner's deck has the higher
    score (a tie counts a half)."""
    seat0 = np.nonzero(rows & (ds['seat'] == 0) & rows[ds['opp']])[0]
    o = ds['opp'][seat0]
    r0 = ds['result'][seat0]
    dec = r0 != 0.5
    s = (np.sign(p[seat0] - p[o]) * np.sign(r0 - 0.5))[dec]
    return {'games': int(dec.sum()), 'accuracy': round(float(np.mean((s > 0) + 0.5 * (s == 0))), 4) if len(s) else None}


def metrics(p, ds, rows, v=None):
    """Held-out numbers of predictions p (per row of ds) on `rows`: log loss and Brier against the results, the
    correlation with the pre-game value, the winner's-deck ranking accuracy."""
    r = ds['result'][rows]
    v = pregame_target(ds) if v is None else v
    out = {'rows': int(rows.sum()), 'logloss': round(logloss(p[rows], r), 5),
           'brier': round(float(np.mean((p[rows] - r) ** 2)), 5)}
    cv, sv = pearson(p[rows], v[rows]), spearman(p[rows], v[rows])
    out['corr_value'] = None if cv is None else round(cv, 4)
    out['spearman_value'] = None if sv is None else round(sv, 4)
    out['game_ranking'] = game_ranking(p, ds, rows)
    return out


# ----------------------------------------------------------------------------- the evaluator

FORMAT = 'mtgz-deck-evaluator'


class Evaluator:
    """A trained deck evaluator: an ensemble of one model kind over the card table `vocab` and the scaled deck
    features. `score(decks)` -> P(win against the field) per deck (the ensemble's mean)."""

    def __init__(self, kind, nets, scaler, hyper=None, meta=None, names=None):
        self.kind, self.nets, self.scaler = kind, list(nets), scaler
        self.hyper, self.meta = dict(hyper or {}), dict(meta or {})
        self.names = tuple(names or vocab())
        if self.names != vocab():
            raise ValueError('the evaluator\'s card table is not this checkout\'s (vocab)')

    def features(self, X):
        return self.scaler(deck_features(X))

    def score_counts(self, X):
        X = np.asarray(X, np.float32)
        return predict(self.nets, X, self.features(X))

    def score(self, decks):
        """decks: lists of names or {name: count} -> P(win against the field) [n]."""
        return self.score_counts(counts_matrix(decks))

    def members(self, X):
        """Each member's win probability [members, n] (the ensemble's spread)."""
        X = np.asarray(X, np.float32)
        Fz = self.features(X)
        return np.stack([predict([n], X, Fz) for n in self.nets])

    def save(self, path):
        torch = _nn()
        os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
        torch.save({'format': FORMAT, 'version': 1, 'kind': self.kind, 'vocab': list(self.names),
                    'features': list(FEATURES), 'mean': self.scaler.mean.tolist(), 'std': self.scaler.std.tolist(),
                    'hyper': self.hyper, 'meta': self.meta, 'states': [n.state_dict() for n in self.nets]}, path)

    @classmethod
    def load(cls, path):
        torch = _nn()
        d = torch.load(path, map_location='cpu', weights_only=False)
        if d.get('format') != FORMAT:
            raise ValueError(f'{path}: not a {FORMAT} file')
        if tuple(d['features']) != FEATURES:
            raise ValueError(f'{path}: its deck features are not this checkout\'s')
        nets = []
        for st in d['states']:
            n = make_net(d['kind'], len(d['vocab']), len(d['features']), **d['hyper'])
            n.load_state_dict(st)
            nets.append(n.eval())
        return cls(d['kind'], nets, Scaler(mean=d['mean'], std=d['std']), d['hyper'], d['meta'], d['vocab'])


def check_build(pool_cards, deck):
    """A deck is a build of the pool: at least 40 cards, every nonbasic card within the pool's count (basics free)."""
    have = {}
    for n, k in (pool_cards.items() if isinstance(pool_cards, dict) else ((n, 1) for n in pool_cards)):
        have[n] = have.get(n, 0) + k
    need = {}
    for n in deck:
        if n not in BASIC_COLOUR:
            need[n] = need.get(n, 0) + 1
    over = {n: k for n, k in need.items() if k > have.get(n, 0)}
    if len(deck) < 40 or over:
        raise ValueError(f'not a build of the pool: {len(deck)} cards, beyond the pool {over}')


def rank(pool_cards, candidates, evaluator):
    """The picker (DESIGN.md 9.1): the evaluator's scores of candidate builds of a pool (each a list of names, or a
    dict with 'deck'), checked to be builds of it -> np.array of P(win against the field)."""
    decks = [c['deck'] if isinstance(c, dict) else c for c in candidates]
    for d in decks:
        check_build(pool_cards, d)
    return evaluator.score(decks)


# ----------------------------------------------------------------------------- card ranking v0

def card_values(evaluator, X_typical, n_decks=150, seed=0, cards=None):
    """Per card: the evaluator's change in win probability (points) when one copy replaces a card of a typical deck of
    its colours (decks from `X_typical` whose main colours hold the card's; any deck for a colourless card): in a deck
    without it, the card replaces each of the deck's nonland cards in turn (a nonbasic land replaces each basic), in a
    deck holding it each of them replaces one copy - the mean change (by copies), over up to `n_decks` decks. Computed
    per ensemble member: the card's value is the members' mean and its uncertainty their sd. -> rows sorted by value."""
    t = card_table()
    X_typical = np.asarray(X_typical, np.float32)
    cols = deck_colours(X_typical)
    rng = np.random.default_rng(seed)
    out = []
    for c in (cards or [n for n in vocab() if n not in BASIC_COLOUR]):
        ci = index()[c]
        need = {COLOURS[i] for i in range(5) if t['colour'][ci, i]}
        pick = np.nonzero(np.array([need <= cs for cs in cols]))[0]
        if len(pick) > n_decks:
            pick = np.sort(rng.choice(pick, n_decks, replace=False))
        swap_ok = (t['basic'] > 0) if t['land'][ci] else (t['land'] == 0)
        rows, base_of, sign, weight = [], [], [], []
        for b, d in enumerate(pick):
            D = X_typical[d]
            holds = D[ci] > 0
            for x in np.nonzero((D > 0) & swap_ok)[0]:
                if x == ci:
                    continue
                R = D.copy()
                R[ci] += -1 if holds else 1
                R[x] += 1 if holds else -1
                rows.append(R)
                base_of.append(b)
                sign.append(-1.0 if holds else 1.0)
                weight.append(D[x])
        if not rows:
            out.append({'name': c, 'decks': 0, 'holding': 0, 'value': None, 'sd': None})
            continue
        base_of, sign, weight = np.array(base_of), np.array(sign), np.array(weight, np.float64)
        B = X_typical[pick]
        pr, pb = evaluator.members(np.stack(rows)), evaluator.members(B)     # [m, rows], [m, decks]
        delta = sign[None, :] * (pr - pb[:, base_of])
        den = np.bincount(base_of, weights=weight, minlength=len(pick))
        num = np.stack([np.bincount(base_of, weights=delta[m] * weight, minlength=len(pick)) for m in range(len(delta))])
        has = den > 0
        per_member = (num[:, has] / den[has]).mean(1) * 100.0
        out.append({'name': c, 'decks': int(has.sum()), 'holding': int((B[has][:, ci] > 0).sum()),
                    'value': round(float(per_member.mean()), 3),
                    'sd': round(float(per_member.std(ddof=1)) if len(per_member) > 1 else 0.0, 3)})
    out.sort(key=lambda r: (r['value'] is None, -(r['value'] or 0.0)))
    return out


# ----------------------------------------------------------------------------- the 17lands yardstick (never training)

def seventeen_lands_rates(event='Sealed'):
    """17lands' EoE sealed per-card win rates - A YARDSTICK ONLY (DESIGN.md 0): 'gih' game in hand (the card in the
    opening hand or drawn: the data has draws) and 'gp' game played (in the deck): {name: {'gih', 'gih_n', 'gp',
    'gp_n'}} over nonbasic cards."""
    from . import seventeen_lands as L
    acc = {}
    for p in L.load(event):
        for b in p.builds:
            for g in b.games:
                seen = set(g.opening_hand) | set(g.drawn)
                for n in b.deck:
                    if n in BASIC_COLOUR:
                        continue
                    e = acc.setdefault(n, [0, 0, 0, 0])
                    e[2] += g.won
                    e[3] += 1
                    if n in seen:
                        e[0] += g.won
                        e[1] += 1
    return {n: {'gih': e[0] / e[1] if e[1] else None, 'gih_n': e[1], 'gp': e[2] / e[3] if e[3] else None, 'gp_n': e[3]}
            for n, e in acc.items()}


def yardstick(ranking, rates, min_games=200):
    """Spearman of a card ranking's values with 17lands' GIH and GP win rates over the cards both have (at least
    `min_games` in-hand games). A yardstick only."""
    both = [r for r in ranking if r['value'] is not None and r['name'] in rates
            and (rates[r['name']]['gih_n'] or 0) >= min_games]
    if len(both) < 3:
        return {'cards': len(both)}
    v = [r['value'] for r in both]
    return {'cards': len(both), 'min_gih_games': min_games,
            'spearman_gih': round(spearman(v, [rates[r['name']]['gih'] for r in both]), 4),
            'spearman_gp': round(spearman(v, [rates[r['name']]['gp'] for r in both]), 4)}


# ----------------------------------------------------------------------------- same-pool ranking against the network

def pool_candidates(pool_seed, k=16, collation='arena'):
    """A generated pool's cards and the deck choice's K candidate builds of it (docs/ENGINE.md 107.2: candidate 0 the
    builder's best guess, the rest randomised builds; the game seed = the pool seed)."""
    from . import engine as R
    p = R.pool(int(pool_seed), collation=collation)
    spec = {'sealed': {'set': 'eoe', 'collation': collation, 'p0': {'pool': list(p['cards'])},
                       'deck_choice': {'k': int(k), 'explore': 0.0}}, 'max_turns': 60}
    return list(p['cards']), R.candidates(spec, int(pool_seed))[0]['candidates']


def pool_ranking(evaluators, judge, pool_seeds, k=16, collation='arena', log=None):
    """Same-pool ranking against the play network's pre-game value (the judge: `NetJudge`, the mean over who chooses):
    for each pool, K candidate builds; per evaluator (and the builder's best guess as a picker), the share of candidate
    pairs ordered as the judge orders them (distinct decks; a tie in score counts a half), how often its top pick is
    the judge's top, and its pick's judge value above the candidates' mean ('gain') and below their best ('regret'),
    in points."""
    acc = {name: {'conc': [], 'top1': [], 'gain': [], 'regret': []} for name in list(evaluators) + ['best_guess']}
    spread = []
    for n, ps in enumerate(pool_seeds):
        cards, cands = pool_candidates(ps, k, collation)
        v = judge.values([(c['deck'], c['sideboard']) for c in cands]).mean(1)
        keys = [tuple(sorted(c['deck'])) for c in cands]
        pairs = [(a, b) for a in range(len(cands)) for b in range(a + 1, len(cands)) if keys[a] != keys[b]]
        spread.append((v.max() - v.mean()) * 100)
        scores = {name: rank(cards, cands, e) for name, e in evaluators.items()}
        scores['best_guess'] = np.array([1.0] + [0.0] * (len(cands) - 1))
        for name, s in scores.items():
            if pairs:
                agree = [0.5 if s[a] == s[b] else float((s[a] - s[b]) * (v[a] - v[b]) > 0) for a, b in pairs]
                acc[name]['conc'].append(np.mean(agree))
            j = int(np.argmax(s))
            acc[name]['top1'].append(float(v[j] == v.max()))
            acc[name]['gain'].append((v[j] - v.mean()) * 100)
            acc[name]['regret'].append((v.max() - v[j]) * 100)
        if log and (n + 1) % 50 == 0:
            log(f'# same-pool ranking: {n + 1} of {len(pool_seeds)} pools')
    out = {'pools': len(pool_seeds), 'k': k, 'judge_best_above_mean': round(float(np.mean(spread)), 3)}
    for name, a in acc.items():
        out[name] = {'concordance': round(float(np.mean(a['conc'])), 4) if a['conc'] else None,
                     'top1': round(float(np.mean(a['top1'])), 4), 'gain': round(float(np.mean(a['gain'])), 3),
                     'gain_se': round(float(np.std(a['gain'], ddof=1) / math.sqrt(len(a['gain']))), 3)
                     if len(a['gain']) > 1 else None,
                     'regret': round(float(np.mean(a['regret'])), 3)}
    return out


# ----------------------------------------------------------------------------- training and the report

LAMS = (0.0, 0.25, 0.5, 0.75, 1.0)
GRID = {
    'const': [{}],
    'feat': [{'l2_other': 1e-4}],
    'feat+mode': [{'l2_other': 1e-4}],
    'linear': [{'l2_cards': c, 'l2_other': 1e-4} for c in (1e-4, 3e-4, 1e-3, 3e-3, 1e-2, 3e-2)],
    'set': [{'l2_cards': 1e-3, 'l2_other': o} for o in (1e-4, 1e-3)],
}
SET_HYPER = {'d': 32, 'k': 8, 'hidden': 64}


def _bootstrap(ds, rows, seed):
    """Per-row weights of a bootstrap of the games of `rows` (a game drawn m times weighs m on both its rows)."""
    games = np.unique(ds['game'][rows])
    rng = np.random.default_rng(seed)
    cnt = np.bincount(rng.integers(0, len(games), len(games)), minlength=len(games))
    w = np.zeros(int(ds['game'].max()) + 1)
    w[games] = cnt
    return w[ds['game'][rows]]


def train(run, values, out, how='time', seed=0, ensemble=5, judge=None, pools=200, k=16, threads=4, kinds=None,
          lams=LAMS, split_kw=None, set_epochs=60, n_card_decks=150, log=None):
    """The whole S4a pipeline (module docs): the dataset with its pre-game values, every model kind fitted to each
    distillation mix and selected on the validation games' results, ensembles of the selected (and of the results-only)
    fits on bootstraps of the training games, their held-out numbers, the same-pool ranking against `judge` (a
    checkpoint; default the values'), card ranking v0 and the 17lands yardstick. Writes `out`/report.json,
    `out`/cards_<kind>.json and `out`/evaluator_<kind>.pt (linear and set) -> the report."""
    import torch
    torch.set_num_threads(max(1, int(threads)))
    say = log or (lambda s: None)
    kinds = list(kinds or GRID)
    vals = load_values(values)
    ds = attach_values(dataset(run, how=how, **(split_kw or {})), vals)
    rep = {'run': run, 'values': values, 'values_meta': vals['meta'], 'split': how, 'seed': seed, 'ensemble': ensemble,
           'dataset': describe(ds)}
    say('# dataset ' + json.dumps(rep['dataset']))
    X = ds['X']
    Fraw = deck_features(X)
    tr, va, te = ds['split'] == 0, ds['split'] == 1, ds['split'] == 2
    scaler = Scaler(Fraw[tr])
    Fz = scaler(Fraw)
    M = np.stack([ds['mode'] == 0, ds['mode'] == 1], 1).astype(np.float32)
    Fm = np.concatenate([Fz, Scaler(M[tr])(M)], 1)
    r, v = ds['result'], pregame_target(ds)
    unseen = te & ((ds['iter'] < 0) | (ds['iter'] >= 40))
    rep['test_unseen_rows'] = int(unseen.sum())

    def feats(kind):
        return Fm if kind == 'feat+mode' else Fz

    def ytarget(lam):
        return ((1 - lam) * r + lam * v).astype(np.float32)

    def model_kind(kind):
        return 'feat' if kind == 'feat+mode' else kind

    # the play network's own pre-game value, as a predictor (it trained on games up to its iteration)
    rep['network_value'] = {
        'mean_over_choosers': metrics(v.astype(np.float64), ds, te),
        'chooser_view': metrics(ds['v'][:, 0].astype(np.float64), ds, te),
        'waiting_view': metrics(ds['v'][:, 1].astype(np.float64), ds, te),
        'mean_over_choosers_unseen': metrics(v.astype(np.float64), ds, unseen) if unseen.any() else None}
    say('# network value ' + json.dumps(rep['network_value']['mean_over_choosers']))
    rep['models'] = {}
    evaluators = {}
    for kind in kinds:
        F_k = feats(kind)
        sel = []
        for h in GRID[kind]:
            for lam in lams:
                t0 = time.time()
                net = fit(model_kind(kind), X[tr], F_k[tr], ytarget(lam)[tr], seed=seed, epochs=set_epochs,
                          val=(X[va], F_k[va], ytarget(lam)[va]), hyper=SET_HYPER if kind == 'set' else None, **h)
                p = predict([net], X, F_k)
                ll = logloss(p[va], r[va])
                sel.append({'hyper': h, 'lam': lam, 'val_logloss': round(ll, 5),
                            'val_corr_value': round(pearson(p[va], v[va]) or 0.0, 4), 'seconds': round(time.time() - t0, 1)})
                say(f'# {kind} {json.dumps(sel[-1])}')
        best = min(sel, key=lambda s: (s['val_logloss'], -s['lam']))
        res0 = min((s for s in sel if s['lam'] == 0.0), key=lambda s: s['val_logloss'])
        entry = {'grid': sel, 'selected': {'hyper': best['hyper'], 'lam': best['lam']},
                 'results_only': {'hyper': res0['hyper']}}
        for tag, ch in (('selected', best), ('results_only', res0)):
            if tag == 'results_only' and ch is best:
                entry['results_only']['same_as_selected'] = True
                entry['results_only']['test'] = entry['selected']['test']
                continue
            nets = []
            for m in range(ensemble):
                w = _bootstrap(ds, tr, mix(seed, m, 1))
                nets.append(fit(model_kind(kind), X[tr], F_k[tr], ytarget(ch['lam'])[tr], weights=w, seed=seed + m,
                                epochs=set_epochs,
                                val=(X[va], F_k[va], ytarget(ch['lam'])[va]),
                                hyper=SET_HYPER if kind == 'set' else None, **ch['hyper']))
            p = predict(nets, X, F_k)
            entry[tag]['test'] = metrics(p, ds, te)
            if unseen.any():
                entry[tag]['test_unseen'] = metrics(p, ds, unseen)
            say(f'# {kind} {tag} lam {ch["lam"]} test ' + json.dumps(entry[tag]['test']))
            if tag == 'selected' and kind in ('linear', 'set'):
                e = Evaluator(kind, nets, scaler, SET_HYPER if kind == 'set' else {},
                              {'run': run, 'values': values, 'checkpoint': vals['meta']['checkpoint'], 'lam': ch['lam'],
                               'l2': ch['hyper'], 'split': how, 'seed': seed, 'test': entry[tag]['test']})
                e.save(os.path.join(out, f'evaluator_{kind}.pt'))
                evaluators[kind] = e
        rep['models'][kind] = entry
    # the Bradley-Terry extra: the same linear card model on the difference of the two decks (a matchup model)
    if 'linear' in kinds:
        s0 = np.nonzero(ds['seat'] == 0)[0]
        o = ds['opp'][s0]
        Xd, Fd = X[s0] - X[o], Fz[s0] - Fz[o]
        gtr, gva, gte = tr[s0], va[s0], te[s0]
        h = rep['models']['linear']['results_only']['hyper']
        bt = fit('linear', Xd[gtr], Fd[gtr], r[s0][gtr], **h)
        pg = predict([bt], Xd, Fd)
        torch_ = _nn()
        with torch_.no_grad():
            f = (torch_.from_numpy(X) @ bt.wc + torch_.from_numpy(Fz) @ bt.wf).numpy().astype(np.float64)
        rep['bradley_terry'] = {'hyper': h, 'test_games': int(gte.sum()), 'game_logloss': round(logloss(pg[gte], r[s0][gte]), 5),
                                'game_ranking': game_ranking(f, ds, te),
                                'corr_value': round(pearson(f[te], v[te]) or 0.0, 4)}
        say('# bradley-terry ' + json.dumps(rep['bradley_terry']))
    # same-pool ranking against the play network's value
    if pools and evaluators:
        judge = judge or vals['meta']['checkpoint']
        if not os.path.isfile(judge):
            raise ValueError(f'the same-pool ranking\'s judge {judge!r} is not a checkpoint (give --judge)')
        rng = np.random.default_rng(seed)
        ps = np.unique(ds['pool_seed'][te & (ds['phase'] == 0)])
        ps = [int(x) for x in rng.choice(ps, min(int(pools), len(ps)), replace=False)]
        rep['same_pool'] = pool_ranking(evaluators, NetJudge(judge), ps, k=k, log=say)
        rep['same_pool']['judge'] = judge
        say('# same-pool ' + json.dumps(rep['same_pool']))
    # card ranking v0 and the 17lands yardstick (never training)
    typical = np.unique(X[tr & (ds['phase'] == 0)], axis=0)
    rates = seventeen_lands_rates('Sealed')
    rep['cards'] = {}
    for kind, e in evaluators.items():
        cv = card_values(e, typical, n_decks=n_card_decks, seed=seed)
        with open(os.path.join(out, f'cards_{kind}.json'), 'w', encoding='utf-8') as fh:
            json.dump(cv, fh, indent=1)
        valued = [c for c in cv if c['value'] is not None]
        rep['cards'][kind] = {'cards': len(valued), 'top': valued[:15], 'bottom': valued[-15:][::-1],
                              'yardstick_17lands': yardstick(cv, rates)}
        say(f'# cards {kind} ' + json.dumps(rep['cards'][kind]['yardstick_17lands']))
    os.makedirs(out, exist_ok=True)
    with open(os.path.join(out, 'report.json'), 'w', encoding='utf-8') as fh:
        json.dump(rep, fh, indent=1)
    return rep


def summary_table(rep):
    """The held-out table of a report."""
    lines = [f"split {rep['split']}: " + json.dumps(rep['dataset']),
             f"{'model':24s} {'lam':>5s} {'logloss':>8s} {'brier':>7s} {'corr_v':>7s} {'game_rank':>9s}"]

    def row(name, lam, t):
        lines.append(f"{name:24s} {lam:>5} {t['logloss']:8.5f} {t['brier']:7.5f} {t['corr_value'] or 0:7.4f} "
                     f"{t['game_ranking']['accuracy'] or 0:9.4f}")
    for kind, e in rep['models'].items():
        if not e['results_only'].get('same_as_selected'):
            row(kind, '0', e['results_only']['test'])
        row(kind + ('' if e['results_only'].get('same_as_selected') else ' (selected)'), str(e['selected']['lam']),
            e['selected']['test'])
    nv = rep['network_value']
    row('network value (mean)', '-', nv['mean_over_choosers'])
    row('network value (chooser)', '-', nv['chooser_view'])
    row('network value (waiting)', '-', nv['waiting_view'])
    if 'same_pool' in rep:
        sp = rep['same_pool']
        lines.append(f"same pool ({sp['pools']} pools x {sp['k']} builds, judge {sp['judge']}; the judge's best is "
                     f"{sp['judge_best_above_mean']} points above the mean):")
        for name in [x for x in sp if isinstance(sp[x], dict)]:
            lines.append(f"  {name:12s} " + json.dumps(sp[name]))
    for kind, c in rep.get('cards', {}).items():
        lines.append(f"cards {kind}: 17lands yardstick {json.dumps(c['yardstick_17lands'])}")
        lines.append('  top: ' + ', '.join(f"{x['name']} {x['value']:+.2f}+-{x['sd']:.2f}" for x in c['top'][:10]))
        lines.append('  bottom: ' + ', '.join(f"{x['name']} {x['value']:+.2f}+-{x['sd']:.2f}" for x in c['bottom'][:10]))
    return '\n'.join(lines)


# ----------------------------------------------------------------------------- the command line

def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0], formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest='cmd', required=True)
    pv = sub.add_parser('values', help='the play network\'s pre-game value of every logged game')
    pv.add_argument('run')
    pv.add_argument('--checkpoint', required=True, help="a checkpoint for every game, or 'played' (with --models): "
                                                        "each game by the network that played it")
    pv.add_argument('--models', default=None, help="the run's models directory (for --checkpoint played)")
    pv.add_argument('--out', required=True, help='the values file (.npz)')
    pv.add_argument('--device', default='cpu')
    pv.add_argument('--batch', type=int, default=256)
    pv.add_argument('--threads', type=int, default=4)
    pt = sub.add_parser('train', help='fit and compare the models, save the evaluators, card ranking v0')
    pt.add_argument('run')
    pt.add_argument('--values', required=True)
    pt.add_argument('--out', required=True, help='a directory: report.json, cards_*.json, evaluator_*.pt')
    pt.add_argument('--split', default='time', choices=['hash', 'time'],
                    help='time (default): test the iterations from 40 on, validate on the 4 before; hash: by game seed')
    pt.add_argument('--seed', type=int, default=0)
    pt.add_argument('--ensemble', type=int, default=5)
    pt.add_argument('--kinds', default=','.join(GRID), help='model kinds, comma-separated')
    pt.add_argument('--lams', default=','.join(str(x) for x in LAMS), help='distillation mixes to select from')
    pt.add_argument('--judge', default=None, help='the same-pool ranking\'s checkpoint (default: the values\')')
    pt.add_argument('--pools', type=int, default=200, help='held-out pools of the same-pool ranking (0: none)')
    pt.add_argument('--k', type=int, default=16, help='candidate builds per pool')
    pt.add_argument('--threads', type=int, default=4)
    pr = sub.add_parser('rank', help='score a generated pool\'s candidate builds')
    pr.add_argument('--evaluator', required=True)
    pr.add_argument('--pool-seed', type=int, required=True)
    pr.add_argument('--collation', default='arena', choices=['arena', 'paper'])
    pr.add_argument('--k', type=int, default=32)
    a = ap.parse_args(argv)
    log = lambda s: print(s, file=sys.stderr, flush=True)  # noqa: E731
    if a.cmd == 'values':
        r = pregame_values(a.run, a.checkpoint, a.out, a.device, a.batch, a.threads, models=a.models, log=log)
        print(json.dumps(r['meta']))
        return r['meta']
    if a.cmd == 'train':
        rep = train(a.run, a.values, a.out, a.split, a.seed, a.ensemble, a.judge, a.pools, a.k, a.threads,
                    a.kinds.split(','), tuple(float(x) for x in a.lams.split(',')), log=log)
        print(summary_table(rep))
        return rep
    e = Evaluator.load(a.evaluator)
    cards, cands = pool_candidates(a.pool_seed, a.k, a.collation)
    s = rank(cards, cands, e)
    out = [{'candidate': j, 'score': round(float(s[j]), 4), 'colors': c['colors'] + ('+' + c['splash'] if c['splash'] else ''),
            'lands': c['lands']} for j, c in enumerate(cands)]
    out.sort(key=lambda x: -x['score'])
    print(json.dumps({'pool_seed': a.pool_seed, 'k': a.k, 'evaluator': a.evaluator, 'ranked': out}))
    return out


if __name__ == '__main__':
    main(sys.argv[1:])
