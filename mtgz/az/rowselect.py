"""The row selector of a multi-source distillation (zero-core docs/az.md 76, `GamePackage.row_selectors`; docs/ENGINE.md
239.1): which rows of a metagame run's shard a source takes, by the deck its acting player plays and the pairing.

    {"actor": "izzet_delver_8016541"}                          rows whose acting player plays that 75
    {"pairing": ["izzet_delver_8016541", "doomsday_8027957"]}  rows of games of that pairing (unordered; [x, x]: x's mirror)
    {"not_pairing": [[a, b], ...]}                              rows of none of those pairings
    keys AND-combined; every name one of the run's metagame decks (its config.json's spec)

A row is a player's view (its `player`'s encoding; a value-only row too). What it holds, exactly (engine
`encode.rs`, checked against the engine in tests/test_rowselect.py on best-of-one and best-of-three metagame games):

* **The opponent's deck**: the opponent-pool bag (`bag::OPP_POOL`, 0.25 a copy) is the opponent's whole pool in every
  row of an open-decklist constructed game - a best-of-three's (`MatchPool::constructed`: the 75, basic lands too) in
  game 1, at sideboarding and in the post-board games whose 60s are hidden, a best-of-one's (`MatchPool::new`: the
  main and sideboard less the five basic lands) everywhere. The row's opponent deck = the one metagame deck whose
  pool equals the bag, count for count; none or several: a loud failure.
* **The acting player's own cards**: the bags of its hand, graveyard, exile and library (`MY_HAND`, `MY_GRAVEYARD`,
  `MY_EXILE`, `MY_LIBRARY`) hold only cards it owns (each zone its owner's: CR 400.3, the exile bag by owner; the
  battlefield bag, by controller, is left out), card identities below the token range (CUBE_CAP: a token is no deck
  card), so their sum U is a sub-multiset of its current deck, itself within its 75. Its deck is among the decks
  whose 75 holds U; and a metagame deck plays the row's opponent deck only in a cell of positive weight (either seat
  order). Those candidates always hold the true deck.
* **Decided**: a row's selection is decided when every candidate gives the same answer (one candidate: the deck is
  known; several - U empty, e.g. a sideboarding row, where no game bag is written - can still agree, e.g. a pairing
  the opponent's deck alone rules out). A row the selector cannot decide fails the whole selection, loudly, with the
  shard and the count: never a guess. In a two-deck run (a stew specialist: decks [C, D], weights [[0, 1], [1, 0]])
  or a mirror run every row is decided by the opponent's deck alone; in a best-of-one metagame every row's U
  decides it; in a best-of-three metagame of three or more decks a sideboarding row (U empty) of a cell whose
  opponent deck several decks play is undecidable for `actor` - such a run needs a per-row tag (not built).
"""
import functools
import json

import numpy as np

KEYS = ('actor', 'pairing', 'not_pairing')
# the encoding's bags (engine/src/az/encode.rs `bag`; mtgz.az.belief.O_BAGS / D: 0.25 a card)
MY_HAND, MY_GRAVEYARD, MY_EXILE, MY_LIBRARY, OPP_POOL = 0, 3, 5, 7, 15
OWN_BAGS = (MY_HAND, MY_GRAVEYARD, MY_EXILE, MY_LIBRARY)
BAGS = 23
BASICS = ('Plains', 'Island', 'Swamp', 'Mountain', 'Forest')     # engine decks::BASICS (MatchPool::new leaves them out)
CHUNK = 4096


@functools.lru_cache(maxsize=1)
def identities():
    """Card name -> identity (mtgz.az.belief.names: the cube's first, as the engine's identity table)."""
    from . import belief
    out = {}
    for i, n in enumerate(belief.names()):
        if n is not None and n not in out:
            out[n] = i
    return out


class Metagame:
    """A run's metagame decks as the selector reads them: `names`, `full` int64 [n, D] (each 75's counts by identity),
    `opp` [n, D] (the opponent-pool bag each deck gives: the 75 in a constructed best-of-three, less the basics
    otherwise), `plays` bool [n, n] (deck a can meet deck b: a cell of positive weight, either seat order)."""

    def __init__(self, spec):
        from . import belief
        meta = spec.get('metagame') if isinstance(spec, dict) else None
        if not isinstance(meta, dict) or not meta.get('decks'):
            raise ValueError('the MTG row selector reads metagame runs only (a spec with "metagame": docs/ENGINE.md 176); '
                             'this run\'s spec has none')
        ident = identities()
        bo3 = spec.get('format', 'bo1') == 'bo3' and bool(spec.get('constructed'))
        decks = meta['decks']
        self.names = [d['name'] for d in decks]
        n = len(decks)
        self.full = np.zeros((n, belief.D), np.int64)
        self.opp = np.zeros((n, belief.D), np.int64)
        for k, d in enumerate(decks):
            for c in list(d.get('main') or []) + list(d.get('sideboard') or []):
                if c not in ident:
                    raise ValueError(f'deck {d["name"]}: {c!r} has no card identity (engine/src/az/identity.rs)')
                self.full[k, ident[c]] += 1
                if bo3 or c not in BASICS:
                    self.opp[k, ident[c]] += 1
        for a in range(n):
            for b in range(a + 1, n):
                if np.array_equal(self.opp[a], self.opp[b]):
                    raise ValueError(f'decks {self.names[a]} and {self.names[b]} have the same pool: the opponent-pool '
                                     f'bag cannot tell them apart')
        w = np.asarray(meta.get('weights') or np.ones((n, n)), np.float64)
        if w.shape != (n, n):
            raise ValueError(f'metagame weights of shape {w.shape} for {n} decks')
        self.plays = (w > 0) | (w.T > 0)
        self.bo3 = bo3

    def index(self, name, what):
        if not isinstance(name, str) or name not in self.names:
            raise ValueError(f'{what} {name!r} is not one of the run\'s decks {self.names}')
        return self.names.index(name)


@functools.lru_cache(maxsize=16)
def _metagame(spec_text):
    return Metagame(json.loads(spec_text))


def metagame_of(config):
    """The run config's (config.json's) metagame (`Metagame`)."""
    if not isinstance(config, dict):
        raise ValueError('the MTG row selector needs the shard\'s run config (<run>/config.json beside <run>/data, or '
                         'the source\'s "config")')
    spec = config.get('spec')
    if isinstance(spec, dict):
        spec = json.dumps(spec, sort_keys=True)
    if not isinstance(spec, str) or not spec:
        raise ValueError('the run config has no spec')
    return _metagame(spec)


def parse(select, mg):
    """A selector checked against the metagame -> (actor index or None, [pairings as sorted index pairs], [not ...])."""
    bad = sorted(set(select) - set(KEYS))
    if bad:
        raise ValueError(f'unknown selector keys {bad} (the MTG row selector: {list(KEYS)})')

    def pair(p, what):
        if not isinstance(p, (list, tuple)) or len(p) != 2:
            raise ValueError(f'{what} {p!r}: two deck names')
        return tuple(sorted(mg.index(x, what) for x in p))
    actor = mg.index(select['actor'], 'actor') if 'actor' in select else None
    pairing = [pair(select['pairing'], 'pairing')] if 'pairing' in select else []
    nots = select.get('not_pairing', [])
    if 'not_pairing' in select and (not isinstance(nots, list) or not nots):
        raise ValueError(f'not_pairing {nots!r}: a non-empty list of pairs of deck names')
    return actor, pairing, [pair(p, 'not_pairing') for p in nots]


class ShardBags:
    """A shard's (dense or sparse) bags as the selector reads them: `counts(lo, hi)` -> (own [n, CUBE_CAP] int64: the
    own-zone bags summed, opp [n, D] int64: the opponent-pool bag) in card counts (a value that is not a whole number
    of 0.25s: ValueError). The shard's arrays are read once."""

    def __init__(self, path):
        from . import belief
        self.path, self.D, self.O = path, belief.D, belief.O_BAGS
        with np.load(path) as z:
            self.N = len(z['player'])
            if 'x_val' in z.files:
                self.ptr = z['x_ptr'].astype(np.int64)
                self.idx, self.val, self.x = z['x_idx'], z['x_val'], None
            else:
                self.x = z['x']
        self.slot = np.full(BAGS, -1, np.int64)
        for k, b in enumerate(OWN_BAGS + (OPP_POOL,)):
            self.slot[b] = k

    @classmethod
    def from_arrays(cls, path, ptr, idx, val):
        """Rows given as a sparse shard stores them (`x_ptr`, `x_idx`, `x_val`; `path` names them in messages)."""
        self = cls.__new__(cls)
        from . import belief
        self.path, self.D, self.O = path, belief.D, belief.O_BAGS
        self.ptr, self.idx, self.val, self.x = np.asarray(ptr, np.int64), idx, val, None
        self.N = len(self.ptr) - 1
        self.slot = np.full(BAGS, -1, np.int64)
        for k, b in enumerate(OWN_BAGS + (OPP_POOL,)):
            self.slot[b] = k
        return self

    def counts(self, lo, hi):
        from .belief import CUBE_CAP
        D, O, n = self.D, self.O, hi - lo
        nb = len(OWN_BAGS) + 1
        dense = np.zeros((n, nb, D), np.float32)
        if self.x is None:
            a, b = int(self.ptr[lo]), int(self.ptr[hi])
            idx = np.asarray(self.idx[a:b]).astype(np.int64)
            row = np.repeat(np.arange(n), np.diff(self.ptr[lo:hi + 1]))
            keep = (idx >= O) & (idx < O + BAGS * D)
            row, rel = row[keep], idx[keep] - O
            sl = self.slot[rel // D]
            m = sl >= 0
            dense[row[m], sl[m], (rel % D)[m]] = np.asarray(self.val[a:b])[keep][m].astype(np.float32)
        else:
            for b, k in enumerate(self.slot):
                if k >= 0:
                    dense[:, k, :] = self.x[lo:hi, O + b * D:O + (b + 1) * D]
        c = dense * 4.0
        r = np.rint(c)
        if not np.allclose(c, r, atol=1e-3):
            raise ValueError(f'shard {self.path}: bag values that are not whole cards (0.25 a card)')
        r = r.astype(np.int64)
        return r[:, :len(OWN_BAGS), :CUBE_CAP].sum(1), r[:, len(OWN_BAGS), :]


def identify(bags, mg, lo=0, hi=None):
    """Rows lo..hi-1 of a shard (`ShardBags`) -> (opp int64 [n]: the opponent's deck index, cand bool [n, decks]: the
    acting player's candidate decks - always holding its true deck). A row whose opponent-pool bag is no deck's pool,
    or one whose candidates are none: ValueError."""
    from .belief import CUBE_CAP
    hi = bags.N if hi is None else hi
    own, opp = bags.counts(lo, hi)
    eq = (opp[:, None, :] == mg.opp[None, :, :]).all(2)                 # [n, decks]
    hits = eq.sum(1)
    if (hits != 1).any():
        r = int(np.flatnonzero(hits != 1)[0])
        raise ValueError(f'shard {bags.path}: row {lo + r}: its opponent-pool bag ({int(opp[r].sum())} cards) is the '
                         f"pool of {int(hits[r])} of the run's decks {mg.names} (1 expected: a shard of another run or "
                         f'spec?)')
    o = eq.argmax(1)
    cand = (own[:, None, :] <= mg.full[None, :, :CUBE_CAP]).all(2) & mg.plays[o]
    if not cand.any(1).all():
        r = int(np.flatnonzero(~cand.any(1))[0])
        raise ValueError(f"shard {bags.path}: row {lo + r}: no deck of the run holds the acting player's own cards and "
                         f'plays {mg.names[o[r]]} (a shard of another run, or an encoding this selector misreads)')
    return o, cand


def evaluate(parsed, o, cand, n_decks):
    """Per row, (decided bool, selected bool) of the parsed selector given the opponents `o` and candidates `cand`."""
    actor, pairing, nots = parsed
    n = len(o)
    out = np.zeros((n, n_decks), bool)                  # the selector's answer per (row, candidate deck)
    for a in range(n_decks):
        ok = np.ones(n, bool)
        if actor is not None:
            ok &= a == actor
        lo_, hi_ = np.minimum(a, o), np.maximum(a, o)
        for p in pairing:
            ok &= (lo_ == p[0]) & (hi_ == p[1])
        for p in nots:
            ok &= ~((lo_ == p[0]) & (hi_ == p[1]))
        out[:, a] = ok
    yes = (out & cand).any(1)
    no = (~out & cand).any(1)
    return ~(yes & no), yes


def row_select(select, shard, ctx):
    """zero-core's row selector hook (`GamePackage.row_selectors['mtg']`, docs/az.md 76): the shard's rows the
    selector takes (module docstring). Raises (ValueError) on a key or deck it does not know, a shard of another
    spec, and any row it cannot decide."""
    mg = metagame_of(ctx.get('config'))
    parsed = parse(select, mg)
    meta = ctx.get('meta') or {}
    if meta.get('game', 'mtg') != 'mtg':
        raise ValueError(f'shard {shard}: game {meta.get("game")!r} (the MTG row selector reads game mtg)')
    bags = ShardBags(shard)
    N = bags.N
    out = np.zeros(N, bool)
    undecided, first = 0, None
    for lo in range(0, N, CHUNK):
        hi = min(N, lo + CHUNK)
        o, cand = identify(bags, mg, lo, hi)
        dec, sel = evaluate(parsed, o, cand, len(mg.names))
        out[lo:hi] = sel
        if not dec.all():
            undecided += int((~dec).sum())
            if first is None:
                r = int(np.flatnonzero(~dec)[0])
                first = (lo + r, [mg.names[k] for k in np.flatnonzero(cand[r])], mg.names[o[r]])
    if undecided:
        raise ValueError(f'shard {shard}: {undecided} of {N} rows the selector {json.dumps(select)} cannot decide (row '
                         f'{first[0]}: its acting player plays one of {first[1]} against {first[2]}, and the row holds '
                         f'none of its own cards - a sideboarding row; mtgz/az/rowselect.py, docs/ENGINE.md 239.1)')
    return out


def tally(shard, config, lo=0, hi=None):
    """A shard's rows by (acting deck, opponent deck) -> ({'a|b': rows}, undecided rows): a row with one candidate
    counts in its cell; with several, in 'undecided'."""
    mg = metagame_of(config)
    bags = ShardBags(shard)
    hi = bags.N if hi is None else hi
    cells, und = {}, 0
    for s in range(lo, hi, CHUNK):
        o, cand = identify(bags, mg, s, min(hi, s + CHUNK))
        one = cand.sum(1) == 1
        und += int((~one).sum())
        a = cand.argmax(1)
        for x, y in zip(a[one], o[one]):
            k = f'{mg.names[x]}|{mg.names[y]}'
            cells[k] = cells.get(k, 0) + 1
    return cells, und

