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
  embeddings, a per-card linear term and a pairwise term, card vectors interacting by dot product). These see only
  their own deck (the deck against the field the games had: decklists are hidden in sealed). The training target is the
  distillation mix y = (1 - lam) result + lam value under log loss.
* **The pairwise model** (`kind 'pair'`, `PairNet`; docs/ENGINE.md 111, opt-in: `--kinds ...,pair`): P(deck A beats
  deck B) from both decks, trained on the logged games' actual pairings (one row per game, results only):
  z(A, B) = s(A) - s(B) + e(A)^T W e(B), P = sigmoid(z) - a deck strength s (the linear or set model's terms without
  the bias, `base`) and a matchup term between the decks' embeddings e(x) = x U / 40 + f(x) P (`dim` dimensions; card
  counts x, scaled features f) through a skew-symmetric W = M - M^T, so z(B, A) = -z(A, B) exactly (computed as
  (s(A) - s(B)) + (t(A, B) - t(B, A)), t(A, B) = e(A)^T M e(B)) and P(A beats B) + P(B beats A) = 1. `dim` 0 is
  Bradley-Terry: the strength alone.
* **Scoring against a field** (`Evaluator.score(decks, field=None)`): a pair evaluator scores a deck against any named
  field (a list of decks) as the mean over the field of P(deck beats it) - the exact mean, at the cost of one embedding
  per candidate plus N (dim + 1) multiply-adds and N sigmoids against the field's precomputed table (`FieldTable`: per
  member and field deck j, s(B_j) and g_j = W e(B_j); z = s(A) + e(A) . g_j - s(B_j)). Its default field is a stored
  sample of its training data's most recent self-play decks. The linear and set kinds have no opponent input: they
  score against the field their training games had, and refuse a named field.
* **Recency weighting** (`train(half_life=, from_iter=)`, opt-in; default off, today's fits exactly): a training game
  of iteration i weighs 0.5 ** ((ref - i) / half_life) (ref: the newest training iteration; the iteration in progress
  weighs 1), and `from_iter` drops the training games of iterations before it. Validation and test are untouched.
* **The evaluator** (`Evaluator`): a trained ensemble, saved with its card table and feature scaling; `score(decks)`;
  `rank(pool, candidates)` checks the candidates are builds of the pool and scores them (the picker of
  `python -m mtgz.sealed.compare evalpick`).
* **The export** (`Evaluator.export(path)`, `python -m mtgz.sealed.evaluator export`): a JSON file, no torch, holding
  everything a scorer needs (EXPORT FORMAT below); `ReferenceScorer` is a pure-numpy scorer of it (the reference for a
  port: it matches the torch model to 1e-6, tests/test_evaluator.py).
* **Card ranking v0** (`card_values`): per card, the evaluator's change in win probability when one copy replaces a
  card of a typical deck of its colours, over the ensemble (its spread the uncertainty).
* **The yardstick** (`seventeen_lands_rates`): 17lands' EoE Premier Sealed per-card game-in-hand win rates. **A
  yardstick only** (DESIGN.md 0): a Spearman correlation reported, never an input, target or prior.

    python -m mtgz.sealed.evaluator values RUN --checkpoint CKPT --out VALUES.npz [--threads 4]
    python -m mtgz.sealed.evaluator values RUN --checkpoint played --models RUN/models --out VALUES.npz
    python -m mtgz.sealed.evaluator train RUN --values VALUES.npz --out DIR [--split time|hash] --judge CKPT
        [--kinds const,feat,feat+mode,linear,set,pair] [--half-life H] [--from-iter I] [--export]
    python -m mtgz.sealed.evaluator rank --evaluator DIR/evaluator_set.pt --pool-seed 2026 [--k 32]
    python -m mtgz.sealed.evaluator export --evaluator DIR/evaluator_pair.pt --out DIR/evaluator_pair.json

CPU only by default (`--device cpu`, at most `--threads` torch threads); deterministic from its seeds. Pure self-play:
the run's own games and networks train it.

EXPORT FORMAT (`"format": "mtgz-deck-evaluator-export"`, `"version": 1`; JSON, every number a float32 value written
exactly; arrays row-major as nested lists):

* `kind`: "linear", "set" or "pair". `hyper`: the architecture ({} linear; set {d, k, hidden}; pair {base, dim} and,
  for base "set", d, k, hidden). `meta`: how it was trained (informational).
* `cards`: the card table in the order of `engine/src/az/eoe_cards.txt` (the EoE identities: card id c = its line,
  0-based; C = 321 lines): `names` [C]; `in_table` [C] (1: a card of the engine's EoE table; 0: Paradox Haze, never in a
  deck - a scorer refuses it); per card `land`, `basic`, `creature` (0/1: "Land" / "Creature" in the type line; a basic
  land: Plains, Island, Swamp, Mountain, Forest), `mv` (mana value) [C]; `colours` [C][5] (0/1 in W U B R G order);
  `basic_colour` [C][5] (a basic's colour). A deck is x [C]: its count of each card.
* `features`: `names` [35] (FEATURES), `mean` [35], `std` [35]. The raw features F(x), with spell_c = 1 - land_c:
    lands = sum x land; nonbasic_lands = sum x land (1 - basic); creatures = sum x creature spell;
    noncreature_spells = sum x spell - creatures; mv<=1, mv2, mv3, mv4, mv5, mv6+ = sum x spell [mv bin];
    mean_mv = sum x spell mv / max(sum x spell, 1); per[k] = sum_c x_c spell_c colours[c][k] (spells holding colour k);
    main = the two colours of largest per (ties to the earlier colour in WUBRG), only those with per > 0;
    colours = #{k: per[k] > 0}; splash_spells = sum x spell [the card holds a colour outside main];
    colourless_spells = sum x spell [the card has no colour]; spells_W..G = per; basics_W..G = sum x basic_colour;
    mono = 1 if main has exactly one colour; pair_WU, WB, WR, WG, UB, UR, UG, BR, BG, RG = 1 if main is that pair.
  Scaled: f = (F - mean) / std.
* `members` [M] (the ensemble; a score is the mean of the members' probabilities). Each member:
    linear: `bias`, `wc` [C], `wf` [35]: z = bias + wc . x + wf . f.
    set: the linear's, `emb` [C][d], `u` [C][k], `w1` [hidden][d + 35], `b1` [hidden], `w2` [hidden], `b2`:
      h = (x emb) / 40; a = relu(w1 [h, f] + b1); z = bias + wc . x + wf . f + w2 . a + b2
      + 0.5 (|x u|^2 - sum_c x_c |u_c|^2) / 40.
    pair: `strength` (a linear or set member as above, `base`, without `bias`: s(x) = its z with bias 0), `U` [C][dim],
      `P` [35][dim], `W` [dim][dim] (skew-symmetric; absent terms when dim = 0): e(x) = (x U) / 40 + f P;
      z(A, B) = s(A) - s(B) + e(A) . (W e(B)).
  P(win) = sigmoid(z) = 1 / (1 + exp(-z)): linear and set, P(the deck wins against the field its training games had);
  pair, P(A beats B), and against a field of decks B_1..B_N the mean over members of (1/N) sum_j sigmoid(z(A, B_j))
  (with g_j = W e(B_j) precomputed once: z = s(A) + e(A) . g_j - s(B_j)).
* `field` (pair only): the default field, `decks` [N] each a list of [card id, count], and `meta`.
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
def registry():
    """The EoE card registry's names in identity order (engine/src/az/eoe_cards.txt: the set's list in set order, one
    name per line; Paradox Haze has its line and is not in the table): the export's card order."""
    path = os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))), 'engine', 'src',
                        'az', 'eoe_cards.txt')
    with open(path, encoding='utf-8') as f:
        names = tuple(line.rstrip('\r\n') for line in f if line.strip())
    if not set(vocab()) <= set(names):
        raise ValueError(f'cards of the table without a line in eoe_cards.txt: {sorted(set(vocab()) - set(names))}')
    return names


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


def make_net(kind, V, F, d=32, k=8, hidden=64, base='linear', dim=8):
    """A deck model: 'const' (a bias), 'feat' (logistic on the deck features), 'linear' (logistic on card counts and
    the features: card values), 'set' (the linear terms, DeepSets over card embeddings - their mean over the 40 cards
    with the features through an MLP - and a pairwise term: 0.5 (|sum n_c u_c|^2 - sum n_c |u_c|^2) / 40, every pair of
    the deck's cards interacting by the dot product of their vectors u); 'pair' (`PairNet`: P(A beats B) from both
    decks; `base` its strength model, 'linear' or 'set', `dim` its matchup embedding's dimensions)."""
    torch = _nn()
    if kind == 'pair':
        if base not in ('linear', 'set'):
            raise ValueError(f'pair base {base!r}: linear or set')
        return _pair_net(make_net(base, V, F, d, k, hidden), V, F, int(dim))

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


def _pair_net(base, V, F, dim):
    """The pairwise model (module docs): `base` a 'linear' or 'set' DeckNet giving the strength s(x) (its bias left
    out: it would cancel in s(A) - s(B)), the matchup embedding e(x) = x U / 40 + f P in R^dim and M [dim, dim], W =
    M - M^T. forward(XA, FA, XB, FB) = z(A, B), exactly antisymmetric: (s(A) - s(B)) + (t(A, B) - t(B, A))."""
    torch = _nn()

    class PairNet(torch.nn.Module):
        def __init__(self):
            super().__init__()
            self.kind = 'pair'
            self.base = base
            self.dim = dim
            if dim:
                self.U = torch.nn.Parameter(torch.randn(V, dim) * 0.05)
                self.P = torch.nn.Parameter(torch.randn(F, dim) * 0.05)
                self.M = torch.nn.Parameter(torch.randn(dim, dim) * 0.05)

        def strength(self, X, Fz):
            return self.base(X, Fz) - self.base.bias

        def embed(self, X, Fz):
            return X @ self.U / 40.0 + Fz @ self.P

        def W(self):
            return self.M - self.M.t()

        def forward(self, XA, FA, XB, FB):
            z = self.strength(XA, FA) - self.strength(XB, FB)
            if self.dim:
                ea, eb = self.embed(XA, FA), self.embed(XB, FB)
                z = z + (((ea @ self.M) * eb).sum(1) - ((eb @ self.M) * ea).sum(1))
            return z

        def field(self, XF, FF):
            """The field's table: s(B_j) [N] and g_j = W e(B_j) [N, dim]."""
            s = self.strength(XF, FF)
            g = self.embed(XF, FF) @ self.W().t() if self.dim else None
            return s, g

        def against(self, X, Fz, table):
            """z(A, B_j) [n, N] of candidates against a field's table."""
            s, g = table
            z = self.strength(X, Fz)[:, None] - s[None, :]
            if self.dim:
                z = z + self.embed(X, Fz) @ g.t()
            return z

        def penalty(self, l2_cards, l2_other, l2_inter=0.0):
            p = self.base.penalty(l2_cards, l2_other)
            if self.dim:
                p = p + l2_inter * ((self.U ** 2).sum() + (self.P ** 2).sum() + (self.M ** 2).sum())
            return p

    return PairNet()


class Scaler:
    """Standardises the deck features with the training rows' mean and sd (sd floored)."""

    def __init__(self, F=None, mean=None, std=None):
        if F is not None:
            mean, std = F.mean(0), np.maximum(F.std(0), 1e-3)
        self.mean, self.std = np.asarray(mean, np.float32), np.asarray(std, np.float32)

    def __call__(self, F):
        return ((F - self.mean) / self.std).astype(np.float32)


def fit(kind, X, Fz, y, *, l2_cards=1e-3, l2_other=1e-4, l2_inter=1e-3, weights=None, rw=None, opp=None, val=None,
        seed=0, hyper=None, epochs=60, batch=256, lr=3e-3, patience=6):
    """One model fitted to targets y in [0, 1] by log loss + L2 (the mean loss per row plus the penalty). Linear kinds
    (and 'pair' on a linear base): full-batch L-BFGS (weighted rows: a bootstrap's counts). 'set' (and 'pair' on a set
    base): Adam on minibatches (a bootstrap's rows drawn with replacement), early stopping on `val` = (X, Fz, y), for
    'pair' (X, Fz, y, (X_opp, F_opp)). 'pair': `opp` = (X_opp, F_opp), the opponent's deck of each row (y: the row's
    deck beat it); `l2_inter` the matchup terms' L2. `rw`: recency weights per row (None: every row 1, today's fit
    exactly): the weighted mean loss (L-BFGS: the bootstrap's counts times rw; Adam: each minibatch's loss weighted by
    rw over its mean on the bootstrap's rows) -> the trained net."""
    torch = _nn()
    torch.manual_seed(seed)
    V, F = X.shape[1], Fz.shape[1]
    net = make_net(kind, V, F, **(hyper or {}))
    Xt, Ft, yt = torch.from_numpy(X), torch.from_numpy(Fz), torch.from_numpy(np.asarray(y, np.float32))
    ins = (Xt, Ft)
    if kind == 'pair':
        if opp is None:
            raise ValueError("a pair model needs the opponents' decks (opp)")
        ins = (Xt, Ft, torch.from_numpy(opp[0]), torch.from_numpy(opp[1]))
        pen = lambda: net.penalty(l2_cards, l2_other, l2_inter)  # noqa: E731
    else:
        pen = lambda: net.penalty(l2_cards, l2_other)  # noqa: E731
    bce = torch.nn.functional.binary_cross_entropy_with_logits
    adam = kind == 'set' or (kind == 'pair' and net.base.kind == 'set')
    if not adam:
        if rw is not None:
            weights = np.asarray(rw, np.float64) * (1.0 if weights is None else np.asarray(weights, np.float64))
        w = None if weights is None else torch.from_numpy(np.asarray(weights, np.float32))
        opt = torch.optim.LBFGS(net.parameters(), lr=1.0, max_iter=300, history_size=20, line_search_fn='strong_wolfe',
                                tolerance_grad=1e-8, tolerance_change=1e-12)

        def closure():
            opt.zero_grad()
            z = net(*ins)
            loss = (bce(z, yt, weight=w, reduction='sum') / (w.sum() if w is not None else len(yt))
                    + pen())
            loss.backward()
            return loss
        opt.step(closure)
        return net.eval()
    rng = np.random.default_rng(seed)
    pick = np.arange(len(yt)) if weights is None else np.repeat(np.arange(len(yt)), np.asarray(weights, np.int64))
    rwt = None if rw is None else torch.from_numpy((np.asarray(rw, np.float64) / np.mean(np.asarray(rw)[pick]))
                                                   .astype(np.float32))
    opt = torch.optim.Adam(net.parameters(), lr=lr)
    best, best_state, bad = math.inf, None, 0
    if val is not None:
        val_in = tuple(torch.from_numpy(a) for a in (val[0], val[1]) + (tuple(val[3]) if len(val) > 3 else ()))
        val_y = torch.from_numpy(np.asarray(val[2], np.float32))
    for _ in range(epochs):
        net.train()
        order = rng.permutation(pick)
        for i in range(0, len(order), batch):
            j = torch.from_numpy(order[i:i + batch])
            opt.zero_grad()
            if rwt is None:
                loss = bce(net(*(t[j] for t in ins)), yt[j]) + pen()
            else:
                loss = (bce(net(*(t[j] for t in ins)), yt[j], reduction='none') * rwt[j]).mean() + pen()
            loss.backward()
            opt.step()
        if val is not None:
            net.eval()
            with torch.no_grad():
                vl = float(bce(net(*val_in), val_y))
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


def predict_pair(nets, XA, FA, XB, FB, batch=8192):
    """A pair ensemble's mean P(A_i beats B_i) of count rows XA, XB with scaled features FA, FB."""
    torch = _nn()
    out = np.zeros(len(XA), np.float64)
    with torch.no_grad():
        for net in nets:
            for i in range(0, len(XA), batch):
                s = slice(i, i + batch)
                z = net(*(torch.from_numpy(a[s]) for a in (XA, FA, XB, FB)))
                out[s] += torch.sigmoid(z).double().numpy()
    return out / max(1, len(nets))


class FieldTable:
    """A field's precomputed summary for a pair ensemble (module docs): per member, s(B_j) [N] and g_j = W e(B_j)
    [N, dim]; `n` the field's decks. Scoring a candidate against it costs one embedding plus N (dim + 1) multiply-adds
    and N sigmoids per member."""

    def __init__(self, nets, XF, FF):
        torch = _nn()
        if not len(XF):
            raise ValueError('an empty field')
        self.n = len(XF)
        with torch.no_grad():
            self.tables = [net.field(torch.from_numpy(XF), torch.from_numpy(FF)) for net in nets]

    @classmethod
    def of(cls, tables):
        """A table of given members' (s, g) (one member's, for its own score)."""
        t = cls.__new__(cls)
        t.tables = list(tables)
        t.n = len(tables[0][0])
        return t


def predict_field(nets, X, Fz, table, batch=2048):
    """A pair ensemble's mean over its members and the field's decks of P(each row's deck beats the field's deck)."""
    torch = _nn()
    out = np.zeros(len(X), np.float64)
    with torch.no_grad():
        for net, tab in zip(nets, table.tables):
            for i in range(0, len(X), batch):
                z = net.against(torch.from_numpy(X[i:i + batch]), torch.from_numpy(Fz[i:i + batch]), tab)
                out[i:i + batch] += torch.sigmoid(z).double().mean(1).numpy()
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


EXPORT_FORMAT = 'mtgz-deck-evaluator-export'


def sparse_decks(X, names):
    """Count rows -> [{name: count}] (the card names `names` of the columns)."""
    return [{names[i]: int(row[i]) for i in np.nonzero(row)[0]} for row in np.asarray(X)]


class Evaluator:
    """A trained deck evaluator: an ensemble of one model kind over the card table `vocab` and the scaled deck
    features. `score(decks)` -> P(win against the field) per deck (the ensemble's mean). A 'pair' evaluator scores a
    deck against any field (`score(decks, field)`; default its stored field, `field` counts [N, V] with `field_meta`)
    and one deck against another (`matchup`)."""

    def __init__(self, kind, nets, scaler, hyper=None, meta=None, names=None, field=None, field_meta=None):
        self.kind, self.nets, self.scaler = kind, list(nets), scaler
        self.hyper, self.meta = dict(hyper or {}), dict(meta or {})
        self.names = tuple(names or vocab())
        if self.names != vocab():
            raise ValueError('the evaluator\'s card table is not this checkout\'s (vocab)')
        if field is not None and kind != 'pair':
            raise ValueError(f'a {kind} evaluator has no opponent input: no field')
        self.field = None if field is None else np.asarray(field, np.float32)
        self.field_meta = dict(field_meta or {})
        self._default_table = None

    def features(self, X):
        return self.scaler(deck_features(X))

    def field_table(self, field=None):
        """A pair evaluator's precomputed field (`FieldTable`): None the stored default field, a FieldTable as it is,
        a list of decks (names or {name: count}) or count rows [N, V]. Reuse it to score many candidates."""
        if self.kind != 'pair':
            raise ValueError(f'a {self.kind} evaluator has no opponent input: it scores against the field its training '
                             f'games had, never a named field (the pair kind does)')
        if isinstance(field, FieldTable):
            return field
        if field is None:
            if self.field is None:
                raise ValueError('this pair evaluator has no stored field: name one')
            if self._default_table is None:
                self._default_table = FieldTable(self.nets, self.field, self.features(self.field))
            return self._default_table
        XF = np.asarray(field, np.float32) if isinstance(field, np.ndarray) else counts_matrix(field)
        return FieldTable(self.nets, XF, self.features(XF))

    def score_counts(self, X, field=None):
        X = np.asarray(X, np.float32)
        if self.kind == 'pair':
            return predict_field(self.nets, X, self.features(X), self.field_table(field))
        if field is not None:
            self.field_table(field)     # refuses
        return predict(self.nets, X, self.features(X))

    def score(self, decks, field=None):
        """decks: lists of names or {name: count} -> P(win against the field) [n]. `field` (pair only; module docs):
        the mean P(the deck beats each of its decks); default the stored field."""
        return self.score_counts(counts_matrix(decks), field)

    def matchup(self, decks_a, decks_b):
        """P(decks_a[i] beats decks_b[i]) [n] (pair only)."""
        if self.kind != 'pair':
            raise ValueError(f'a {self.kind} evaluator has no opponent input: no matchup (the pair kind has)')
        XA, XB = counts_matrix(decks_a), counts_matrix(decks_b)
        return predict_pair(self.nets, XA, self.features(XA), XB, self.features(XB))

    def members(self, X, field=None):
        """Each member's win probability [members, n] (the ensemble's spread)."""
        X = np.asarray(X, np.float32)
        Fz = self.features(X)
        if self.kind == 'pair':
            tab = self.field_table(field)
            return np.stack([predict_field([n], X, Fz, FieldTable.of([t])) for n, t in zip(self.nets, tab.tables)])
        if field is not None:
            self.field_table(field)     # refuses
        return np.stack([predict([n], X, Fz) for n in self.nets])

    def save(self, path):
        torch = _nn()
        os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
        d = {'format': FORMAT, 'version': 1, 'kind': self.kind, 'vocab': list(self.names),
             'features': list(FEATURES), 'mean': self.scaler.mean.tolist(), 'std': self.scaler.std.tolist(),
             'hyper': self.hyper, 'meta': self.meta, 'states': [n.state_dict() for n in self.nets]}
        if self.kind == 'pair':
            d['version'] = 2
            d['field'] = None if self.field is None else sparse_decks(self.field, self.names)
            d['field_meta'] = self.field_meta
        torch.save(d, path)

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
        field = d.get('field')
        return cls(d['kind'], nets, Scaler(mean=d['mean'], std=d['std']), d['hyper'], d['meta'], d['vocab'],
                   field=None if field is None else counts_matrix(field), field_meta=d.get('field_meta'))

    def export(self, path=None):
        """The evaluator as the export format (module docs): a JSON-able dict, written to `path` if given."""
        out = export_dict(self)
        if path:
            os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
            with open(path, 'w', encoding='utf-8', newline='') as fh:
                json.dump(out, fh, separators=(',', ':'))
        return out


def _f32(a):
    """A float32 array (or scalar) as nested lists of Python floats (exact: a float32 value is a double)."""
    a = np.asarray(a, np.float32)
    return float(a) if a.ndim == 0 else a.astype(np.float64).tolist()


def export_dict(e):
    """`Evaluator.export`'s dict: the card table in registry (eoe_cards.txt) order, the feature scaler, each member's
    weights by name, the pair's default field."""
    if e.kind not in ('linear', 'set', 'pair'):
        raise ValueError(f'kind {e.kind!r}: the export supports linear, set and pair')
    reg = registry()
    n_ids = len(reg)
    ix = index()
    perm = np.array([ix.get(n, -1) for n in reg])        # registry id -> vocab column (-1: not in the table)
    t = card_table()

    def by_id(a):
        """[V, ...] over vocab -> [C, ...] over the registry, zero for a card not in the table."""
        a = np.asarray(a, np.float32)
        out = np.zeros((n_ids,) + a.shape[1:], np.float32)
        out[perm >= 0] = a[perm[perm >= 0]]
        return out
    cards = {'names': list(reg), 'in_table': [int(p >= 0) for p in perm]}
    for k in ('land', 'basic', 'creature', 'mv'):
        cards[k] = _f32(by_id(t[k]))
    cards['colours'] = _f32(by_id(t['colour']))
    cards['basic_colour'] = _f32(by_id(t['basic_colour']))

    def deck_member(net, bias=True):
        sd = {k: v.detach().cpu().numpy() for k, v in net.state_dict().items()}
        m = {}
        if bias:
            m['bias'] = _f32(sd['bias'][0])
        m['wc'] = _f32(by_id(sd['wc']))
        m['wf'] = _f32(sd['wf'])
        if net.kind == 'set':
            m['emb'] = _f32(by_id(sd['emb']))
            m['u'] = _f32(by_id(sd['u']))
            m['w1'] = _f32(sd['mlp.0.weight'])
            m['b1'] = _f32(sd['mlp.0.bias'])
            m['w2'] = _f32(sd['mlp.2.weight'][0])
            m['b2'] = _f32(sd['mlp.2.bias'][0])
        return m
    members = []
    for net in e.nets:
        if e.kind == 'pair':
            m = {'strength': deck_member(net.base, bias=False)}
            if net.dim:
                m['U'] = _f32(by_id(net.U.detach().numpy()))
                m['P'] = _f32(net.P.detach().numpy())
                m['W'] = _f32(net.W().detach().numpy())
            members.append(m)
        else:
            members.append(deck_member(net))
    out = {'format': EXPORT_FORMAT, 'version': 1, 'kind': e.kind, 'hyper': e.hyper, 'meta': e.meta, 'cards': cards,
           'features': {'names': list(FEATURES), 'mean': _f32(e.scaler.mean), 'std': _f32(e.scaler.std)},
           'members': members}
    if e.kind == 'pair':
        rid = {n: c for c, n in enumerate(reg)}
        out['hyper'] = dict(e.hyper, dim=int(e.nets[0].dim), base=e.nets[0].base.kind)
        out['field'] = None if e.field is None else {
            'decks': [[[rid[n], k] for n, k in sorted(d.items(), key=lambda x: rid[x[0]])]
                      for d in sparse_decks(e.field, e.names)],
            'meta': e.field_meta}
    return out


class ReferenceScorer:
    """A pure-numpy scorer of an export (module docs' EXPORT FORMAT), from the JSON alone (its own card table, no
    torch, no `deck_features`): the reference a port checks against. Float64 throughout. `score(decks, field=None)`,
    `matchup(decks_a, decks_b)`; decks as lists of names or {name: count}."""

    def __init__(self, exp):
        if isinstance(exp, str):
            with open(exp, encoding='utf-8') as fh:
                exp = json.load(fh)
        if exp.get('format') != EXPORT_FORMAT or exp.get('version') != 1:
            raise ValueError('not a version-1 deck evaluator export')
        self.exp, self.kind = exp, exp['kind']
        c = exp['cards']
        self.names = c['names']
        self.id = {n: i for i, n in enumerate(self.names)}
        self.in_table = np.array(c['in_table'], bool)
        self.land, self.basic, self.creature, self.mv = (np.array(c[k], np.float64) for k in ('land', 'basic',
                                                                                              'creature', 'mv'))
        self.colours, self.basic_colour = np.array(c['colours'], np.float64), np.array(c['basic_colour'], np.float64)
        f = exp['features']
        self.mean, self.std = np.array(f['mean'], np.float64), np.array(f['std'], np.float64)
        self.members = [self._arrays(m) for m in exp['members']]
        fd = exp.get('field')
        self.field = None
        if fd:
            self.field = np.zeros((len(fd['decks']), len(self.names)))
            for r, d in enumerate(fd['decks']):
                for cid, k in d:
                    self.field[r, cid] += k

    @staticmethod
    def _arrays(m):
        return {k: (ReferenceScorer._arrays(v) if isinstance(v, dict) else np.array(v, np.float64)) for k, v in m.items()}

    def counts(self, decks):
        X = np.zeros((len(decks), len(self.names)))
        for r, d in enumerate(decks):
            for n, k in (d.items() if isinstance(d, dict) else ((n, 1) for n in d)):
                if n not in self.id or not self.in_table[self.id[n]]:
                    raise ValueError(f'{n!r}: not a card of the export\'s table')
                X[r, self.id[n]] += k
        return X

    def features(self, X):
        """The scaled features f(x) (EXPORT FORMAT's definitions, one deck at a time)."""
        out = []
        spell = 1.0 - self.land
        for x in X:
            n_spells = x @ spell
            creatures = x @ (self.creature * spell)
            mv = self.mv
            curve = [x @ (spell * b) for b in (mv <= 1, mv == 2, mv == 3, mv == 4, mv == 5, mv >= 6)]
            per = (x * spell) @ self.colours
            main = set()
            for k in sorted(range(5), key=lambda k: (-per[k], k))[:2]:
                if per[k] > 0:
                    main.add(k)
            outside = np.array([any(self.colours[c, k] and k not in main for k in range(5)) for c in range(len(x))])
            pairs = [1.0 if main == {COLOURS.index(p[0]), COLOURS.index(p[1])} else 0.0 for p in PAIRS]
            F = ([x @ self.land, x @ (self.land * (1 - self.basic)), creatures, n_spells - creatures] + curve
                 + [x @ (spell * mv) / max(n_spells, 1), float((per > 0).sum()), x @ (spell * outside),
                    x @ (spell * (self.colours.sum(1) == 0))] + list(per) + list(x @ self.basic_colour)
                 + [1.0 if len(main) == 1 else 0.0] + pairs)
            out.append(F)
        return (np.array(out) - self.mean) / self.std

    @staticmethod
    def _deck_z(m, X, f, bias=True):
        z = X @ m['wc'] + f @ m['wf']
        if bias:
            z = z + m['bias']
        if 'emb' in m:
            h = X @ m['emb'] / 40.0
            a = np.maximum(np.concatenate([h, f], 1) @ m['w1'].T + m['b1'], 0.0)
            z = z + a @ m['w2'] + m['b2']
            s = X @ m['u']
            z = z + 0.5 * ((s * s).sum(1) - X @ (m['u'] * m['u']).sum(1)) / 40.0
        return z

    def _strength_embed(self, m, X, f):
        s = self._deck_z(m['strength'], X, f, bias=False)
        e = X @ m['U'] / 40.0 + f @ m['P'] if 'U' in m else None
        return s, e

    @staticmethod
    def _sigmoid(z):
        return 1.0 / (1.0 + np.exp(-z))

    def matchup(self, decks_a, decks_b):
        if self.kind != 'pair':
            raise ValueError('matchup: a pair export only')
        XA, XB = self.counts(decks_a), self.counts(decks_b)
        fa, fb = self.features(XA), self.features(XB)
        out = np.zeros(len(XA))
        for m in self.members:
            sa, ea = self._strength_embed(m, XA, fa)
            sb, eb = self._strength_embed(m, XB, fb)
            z = sa - sb
            if ea is not None:
                z = z + ((ea @ m['W']) * eb).sum(1)
            out += self._sigmoid(z)
        return out / len(self.members)

    def score(self, decks, field=None):
        X = self.counts(decks)
        f = self.features(X)
        out = np.zeros(len(X))
        if self.kind != 'pair':
            if field is not None:
                raise ValueError(f'a {self.kind} export has no opponent input: no field')
            for m in self.members:
                out += self._sigmoid(self._deck_z(m, X, f))
            return out / len(self.members)
        XF = self.field if field is None else self.counts(field)
        if XF is None:
            raise ValueError('no stored field: name one')
        fF = self.features(XF)
        for m in self.members:
            sF, eF = self._strength_embed(m, XF, fF)
            s, e = self._strength_embed(m, X, f)
            z = s[:, None] - sF[None, :]
            if e is not None:
                z = z + e @ (eF @ m['W'].T).T          # g_j = W e(B_j)
            out += self._sigmoid(z).mean(1)
        return out / len(self.members)


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
    # the pairwise model (opt-in): `dim` 0 is the strength alone (Bradley-Terry), dim > 0 adds the matchup term (its
    # L2 at 1e-2 or more holds the bilinear term at its zero saddle: the strength alone, so the grid stops below)
    'pair': ([{'dim': 0, 'l2_cards': c, 'l2_other': 1e-4} for c in (1e-3, 3e-3, 1e-2, 3e-2)]
             + [{'dim': m, 'l2_cards': c, 'l2_other': 1e-4, 'l2_inter': i}
                for m in (2, 4, 8) for c in (3e-3, 1e-2) for i in (3e-4, 1e-3, 3e-3)]),
}
DEFAULT_KINDS = ('const', 'feat', 'feat+mode', 'linear', 'set')       # 'pair' is opt-in
PAIR_SET_GRID = ([{'dim': 0, 'l2_cards': 1e-3, 'l2_other': o} for o in (1e-4, 1e-3)]
                 + [{'dim': m, 'l2_cards': 1e-3, 'l2_other': 1e-4, 'l2_inter': i} for m in (4, 8) for i in (3e-4, 1e-3)])
SET_HYPER = {'d': 32, 'k': 8, 'hidden': 64}


def recency_weights(iters, half_life, ref):
    """Per row 0.5 ** ((ref - iteration) / half_life) (module docs): a game `half_life` iterations older than `ref`
    weighs half; the iteration in progress (-1) and anything at or after `ref` weigh 1."""
    it = np.asarray(iters, np.float64)
    age = np.where(it < 0, 0.0, np.maximum(float(ref) - it, 0.0))
    return 0.5 ** (age / float(half_life))


def default_field(ds, rows, size=256, iters=4, seed=0):
    """A pair evaluator's default field: the self-play decks (both seats) of the newest `iters` iterations among `rows`
    (the training games), at most `size` of them drawn without replacement (seeded) -> (counts [N, V], meta)."""
    sp = rows & (ds['phase'] == 0) & (ds['iter'] >= 0)
    its = np.unique(ds['iter'][sp])[-int(iters):]
    cand = np.nonzero(sp & np.isin(ds['iter'], its))[0]
    if not len(cand):
        raise ValueError('no self-play training decks for the default field')
    rng = np.random.default_rng(mix(seed, 0xF1E1D))
    pick = np.sort(rng.choice(cand, min(int(size), len(cand)), replace=False))
    return ds['X'][pick], {'iters': [int(i) for i in its], 'decks_there': int(len(cand)), 'size': int(len(pick)),
                           'seed': int(seed)}


def game_losses(p, ds, rows):
    """Per game (its seat-0 row in `rows`, both rows held out together): the log loss of P(seat 0's result)."""
    s0 = np.nonzero(rows & (ds['seat'] == 0))[0]
    q = np.clip(np.asarray(p, np.float64)[s0], 1e-6, 1 - 1e-6)
    y = ds['result'][s0].astype(np.float64)
    return -(y * np.log(q) + (1 - y) * np.log(1 - q))


def paired_diff(a, b):
    """mean(b - a) with its standard error."""
    d = np.asarray(b, np.float64) - np.asarray(a, np.float64)
    return {'games': int(len(d)), 'diff': round(float(d.mean()), 5),
            'se': round(float(d.std(ddof=1) / math.sqrt(len(d))), 5) if len(d) > 1 else None}


def train_pair(ds, X, Fz, scaler, tr, va, te, unseen, rw=None, seed=0, ensemble=5, base='linear', grid=None,
               epochs=60, field_size=256, field_iters=4, meta=None, log=None):
    """The pairwise kind (module docs) on the dataset's games (seat 0's row as A, its opponent as B; results only):
    the grid selected on the validation games, ensembles on bootstraps of the training games of the selected setting,
    the best strength-only (dim 0) and the best with the matchup term (dim > 0); their held-out numbers (per row
    P(the row's deck beats its opponent's)), the selected one's scores against its default field, and the matchup
    signal (the paired per-game log-loss difference, matchup term less strength only) -> (report entry, Evaluator)."""
    say = log or (lambda s: None)
    grid = list(grid or (GRID['pair'] if base == 'linear' else PAIR_SET_GRID))
    s0 = np.nonzero(ds['seat'] == 0)[0]
    o = ds['opp'][s0]
    gtr, gva = tr[s0], va[s0]
    XA, FA, XB, FB, y = X[s0], Fz[s0], X[o], Fz[o], ds['result'][s0]
    rwg = None if rw is None else np.asarray(rw)[s0][gtr]

    def arch(h):
        return dict(SET_HYPER if base == 'set' else {}, base=base, dim=int(h['dim']))

    def fit1(h, weights=None, s=seed):
        return fit('pair', XA[gtr], FA[gtr], y[gtr], opp=(XB[gtr], FB[gtr]), weights=weights, rw=rwg, seed=s,
                   epochs=epochs, val=(XA[gva], FA[gva], y[gva], (XB[gva], FB[gva])), hyper=arch(h),
                   **{k: x for k, x in h.items() if k != 'dim'})
    sel = []
    for h in grid:
        t0 = time.time()
        net = fit1(h)
        pv = predict_pair([net], XA[gva], FA[gva], XB[gva], FB[gva])
        sel.append({'hyper': h, 'val_logloss': round(logloss(pv, y[gva]), 5), 'seconds': round(time.time() - t0, 1)})
        say(f'# pair {json.dumps(sel[-1])}')
    best = min(sel, key=lambda s: (s['val_logloss'], s['hyper']['dim']))
    pick = {'selected': best}
    for tag, want in (('strength_only', lambda d: d == 0), ('interaction', lambda d: d > 0)):
        c = [s for s in sel if want(s['hyper']['dim'])]
        if c:
            pick[tag] = min(c, key=lambda s: s['val_logloss'])
    entry = {'base': base, 'grid': sel, 'recency': rw is not None}
    ens, preds = {}, {}
    for tag, ch in pick.items():
        key = json.dumps(ch['hyper'], sort_keys=True)
        if key not in ens:
            ens[key] = [fit1(ch['hyper'], weights=_bootstrap(ds, tr & (ds['seat'] == 0), mix(seed, m, 1)), s=seed + m)
                        for m in range(ensemble)]
            preds[key] = predict_pair(ens[key], X, Fz, X[ds['opp']], Fz[ds['opp']])
        p = preds[key]
        entry[tag] = {'hyper': ch['hyper'], 'test': metrics(p, ds, te), 'val': metrics(p, ds, va)}
        if unseen.any():
            entry[tag]['test_unseen'] = metrics(p, ds, unseen)
        say(f'# pair {tag} {json.dumps(ch["hyper"])} test ' + json.dumps(entry[tag]['test']))
    if 'strength_only' in pick and 'interaction' in pick:
        a = preds[json.dumps(pick['strength_only']['hyper'], sort_keys=True)]
        b = preds[json.dumps(pick['interaction']['hyper'], sort_keys=True)]
        entry['matchup_signal'] = {'test': paired_diff(game_losses(a, ds, te), game_losses(b, ds, te)),
                                   'val': paired_diff(game_losses(a, ds, va), game_losses(b, ds, va))}
        say('# pair matchup signal (log loss, matchup term less strength only) ' + json.dumps(entry['matchup_signal']))
    field, fmeta = default_field(ds, tr, field_size, field_iters, seed)
    nets = ens[json.dumps(best['hyper'], sort_keys=True)]
    e = Evaluator('pair', nets, scaler, arch(best['hyper']),
                  dict(meta or {}, l2={k: x for k, x in best['hyper'].items() if k != 'dim'},
                       test=entry['selected']['test']), field=field, field_meta=fmeta)
    pf = e.score_counts(X)
    entry['field'] = fmeta
    entry['selected']['test_field'] = metrics(pf, ds, te)
    say('# pair selected against its default field, test ' + json.dumps(entry['selected']['test_field']))
    return entry, e


def _bootstrap(ds, rows, seed):
    """Per-row weights of a bootstrap of the games of `rows` (a game drawn m times weighs m on both its rows)."""
    games = np.unique(ds['game'][rows])
    rng = np.random.default_rng(seed)
    cnt = np.bincount(rng.integers(0, len(games), len(games)), minlength=len(games))
    w = np.zeros(int(ds['game'].max()) + 1)
    w[games] = cnt
    return w[ds['game'][rows]]


def train(run, values, out, how='time', seed=0, ensemble=5, judge=None, pools=200, k=16, threads=4, kinds=None,
          lams=LAMS, split_kw=None, set_epochs=60, n_card_decks=150, log=None, half_life=None, from_iter=None,
          pair_base='linear', field_size=256, field_iters=4, export=False):
    """The whole S4a pipeline (module docs): the dataset with its pre-game values, every model kind fitted to each
    distillation mix and selected on the validation games' results, ensembles of the selected (and of the results-only)
    fits on bootstraps of the training games, their held-out numbers, the same-pool ranking against `judge` (a
    checkpoint; default the values'), card ranking v0 and the 17lands yardstick. Writes `out`/report.json,
    `out`/cards_<kind>.json and `out`/evaluator_<kind>.pt (linear, set and pair; `export`: also evaluator_<kind>.json,
    the export format) -> the report. Opt-in (defaults: today's pipeline exactly): `kinds` with 'pair' (`train_pair`,
    its strength model `pair_base`, its default field `field_size` decks of the newest `field_iters` training
    iterations); `half_life` (recency weights on every kind's training games) and `from_iter` (training games of
    earlier iterations dropped)."""
    import torch
    torch.set_num_threads(max(1, int(threads)))
    say = log or (lambda s: None)
    kinds = list(kinds or DEFAULT_KINDS)
    vals = load_values(values)
    ds = attach_values(dataset(run, how=how, **(split_kw or {})), vals)
    rep = {'run': run, 'values': values, 'values_meta': vals['meta'], 'split': how, 'split_kw': split_kw or {},
           'seed': seed, 'ensemble': ensemble, 'dataset': describe(ds)}
    say('# dataset ' + json.dumps(rep['dataset']))
    X = ds['X']
    Fraw = deck_features(X)
    tr, va, te = ds['split'] == 0, ds['split'] == 1, ds['split'] == 2
    extra_meta = {}
    if from_iter is not None:
        tr = tr & ~((ds['iter'] >= 0) & (ds['iter'] < int(from_iter)))
        extra_meta['from_iter'] = int(from_iter)
        rep['from_iter'] = {'from_iter': int(from_iter), 'train_games': int(tr.sum() // 2)}
        say('# from iteration ' + json.dumps(rep['from_iter']))
    rw = None
    if half_life is not None:
        ref = int(ds['iter'][tr & (ds['iter'] >= 0)].max())
        rw = recency_weights(ds['iter'], half_life, ref).astype(np.float64)
        w = rw[tr & (ds['seat'] == 0)]
        extra_meta['half_life'] = float(half_life)
        rep['recency'] = {'half_life': float(half_life), 'ref_iter': ref,
                          'effective_train_games': round(float(w.sum() ** 2 / (w * w).sum()), 1),
                          'train_games': int(len(w))}
        say('# recency ' + json.dumps(rep['recency']))
    rwt = None if rw is None else rw[tr]
    scaler = Scaler(Fraw[tr])
    Fz = scaler(Fraw)
    M = np.stack([ds['mode'] == 0, ds['mode'] == 1], 1).astype(np.float32)
    Fm = np.concatenate([Fz, Scaler(M[tr])(M)], 1)
    r, v = ds['result'], pregame_target(ds)
    time_from = (split_kw or {}).get('time_from', 40)
    unseen = te & ((ds['iter'] < 0) | (ds['iter'] >= time_from))
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
        if kind == 'pair':
            entry, e = train_pair(ds, X, Fz, scaler, tr, va, te, unseen, rw=rw, seed=seed, ensemble=ensemble,
                                  base=pair_base, epochs=set_epochs, field_size=field_size, field_iters=field_iters,
                                  meta=dict({'run': run, 'values': values, 'split': how, 'seed': seed}, **extra_meta),
                                  log=say)
            e.save(os.path.join(out, 'evaluator_pair.pt'))
            evaluators['pair'] = e
            rep['models']['pair'] = entry
            continue
        F_k = feats(kind)
        sel = []
        for h in GRID[kind]:
            for lam in lams:
                t0 = time.time()
                net = fit(model_kind(kind), X[tr], F_k[tr], ytarget(lam)[tr], seed=seed, epochs=set_epochs, rw=rwt,
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
                                epochs=set_epochs, rw=rwt,
                                val=(X[va], F_k[va], ytarget(ch['lam'])[va]),
                                hyper=SET_HYPER if kind == 'set' else None, **ch['hyper']))
            p = predict(nets, X, F_k)
            entry[tag]['test'] = metrics(p, ds, te)
            if unseen.any():
                entry[tag]['test_unseen'] = metrics(p, ds, unseen)
            if rw is not None or from_iter is not None:
                entry[tag]['val'] = metrics(p, ds, va)
            say(f'# {kind} {tag} lam {ch["lam"]} test ' + json.dumps(entry[tag]['test']))
            if tag == 'selected' and kind in ('linear', 'set'):
                e = Evaluator(kind, nets, scaler, SET_HYPER if kind == 'set' else {},
                              dict({'run': run, 'values': values, 'checkpoint': vals['meta']['checkpoint'],
                                    'lam': ch['lam'], 'l2': ch['hyper'], 'split': how, 'seed': seed,
                                    'test': entry[tag]['test']}, **extra_meta))
                e.save(os.path.join(out, f'evaluator_{kind}.pt'))
                evaluators[kind] = e
        rep['models'][kind] = entry
    if export:
        for kind, e in evaluators.items():
            e.export(os.path.join(out, f'evaluator_{kind}.json'))
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
        if kind == 'pair':
            for tag in ('strength_only', 'interaction'):
                if tag in e:
                    row(f"pair {tag} dim {e[tag]['hyper']['dim']}", '-', e[tag]['test'])
            row(f"pair selected dim {e['selected']['hyper']['dim']}", '-', e['selected']['test'])
            row('pair selected vs field', '-', e['selected']['test_field'])
            if 'matchup_signal' in e:
                lines.append('pair matchup term less strength only, per-game log loss: '
                             + json.dumps(e['matchup_signal']))
            continue
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
                    help='time (default): test the iterations from --time-from on, validate on the --val-iters before; '
                         'hash: by game seed')
    pt.add_argument('--time-from', type=int, default=40, help='--split time: the first test iteration')
    pt.add_argument('--val-iters', type=int, default=4, help='--split time: validation iterations before it')
    pt.add_argument('--seed', type=int, default=0)
    pt.add_argument('--ensemble', type=int, default=5)
    pt.add_argument('--kinds', default=','.join(DEFAULT_KINDS),
                    help=f'model kinds, comma-separated (of {",".join(GRID)}; pair is opt-in)')
    pt.add_argument('--lams', default=','.join(str(x) for x in LAMS), help='distillation mixes to select from')
    pt.add_argument('--judge', default=None, help='the same-pool ranking\'s checkpoint (default: the values\')')
    pt.add_argument('--pools', type=int, default=200, help='held-out pools of the same-pool ranking (0: none)')
    pt.add_argument('--k', type=int, default=16, help='candidate builds per pool')
    pt.add_argument('--threads', type=int, default=4)
    pt.add_argument('--half-life', type=float, default=None,
                    help='recency weighting: a training game this many iterations older than the newest weighs half '
                         '(default: off, every game 1)')
    pt.add_argument('--from-iter', type=int, default=None, help='drop the training games of iterations before this')
    pt.add_argument('--pair-base', default='linear', choices=['linear', 'set'], help='the pair kind\'s strength model')
    pt.add_argument('--field-size', type=int, default=256, help='the pair evaluator\'s stored field: at most this many decks')
    pt.add_argument('--field-iters', type=int, default=4, help='... of the newest this many training iterations')
    pt.add_argument('--export', action='store_true', help='also write each evaluator\'s export (evaluator_<kind>.json)')
    pr = sub.add_parser('rank', help='score a generated pool\'s candidate builds')
    pr.add_argument('--evaluator', required=True)
    pr.add_argument('--pool-seed', type=int, required=True)
    pr.add_argument('--collation', default='arena', choices=['arena', 'paper'])
    pr.add_argument('--k', type=int, default=32)
    px = sub.add_parser('export', help='write a saved evaluator as the export format (JSON, no torch)')
    px.add_argument('--evaluator', required=True)
    px.add_argument('--out', required=True)
    a = ap.parse_args(argv)
    log = lambda s: print(s, file=sys.stderr, flush=True)  # noqa: E731
    if a.cmd == 'values':
        r = pregame_values(a.run, a.checkpoint, a.out, a.device, a.batch, a.threads, models=a.models, log=log)
        print(json.dumps(r['meta']))
        return r['meta']
    if a.cmd == 'train':
        rep = train(a.run, a.values, a.out, a.split, a.seed, a.ensemble, a.judge, a.pools, a.k, a.threads,
                    a.kinds.split(','), tuple(float(x) for x in a.lams.split(',')), log=log,
                    split_kw={'time_from': a.time_from, 'val_iters': a.val_iters} if a.split == 'time' else None,
                    half_life=a.half_life, from_iter=a.from_iter, pair_base=a.pair_base, field_size=a.field_size,
                    field_iters=a.field_iters, export=a.export)
        print(summary_table(rep))
        return rep
    e = Evaluator.load(a.evaluator)
    if a.cmd == 'export':
        x = e.export(a.out)
        print(json.dumps({'evaluator': a.evaluator, 'out': a.out, 'kind': x['kind'], 'members': len(x['members']),
                          'field': len(x['field']['decks']) if x.get('field') else None}))
        return x
    cards, cands = pool_candidates(a.pool_seed, a.k, a.collation)
    s = rank(cards, cands, e)
    out = [{'candidate': j, 'score': round(float(s[j]), 4), 'colors': c['colors'] + ('+' + c['splash'] if c['splash'] else ''),
            'lands': c['lands']} for j, c in enumerate(cands)]
    out.sort(key=lambda x: -x['score'])
    print(json.dumps({'pool_seed': a.pool_seed, 'k': a.k, 'evaluator': a.evaluator, 'ranked': out}))
    return out


if __name__ == '__main__':
    main(sys.argv[1:])
