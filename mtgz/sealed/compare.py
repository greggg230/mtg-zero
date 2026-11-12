"""Paired deck comparison: DESIGN.md 9's "best few raced in paired games" (S4's verify step) and the
S5 yardstick (our build of a 17lands pool against the human build of the same pool), docs/ENGINE.md
104.

Two or more decks (40-card lists, usually builds of one pool) each play the same games against an
opponent field, piloted by the same player (`random`, `heuristic`, or a network checkpoint):

* **Game k** of a comparison has one game seed s_k, one opponent deck (a fixed list, or the k-th deck
  of a seeded stream of our builder's decks from generated pools: `Field`), one seat for the deck
  under test (k % 2: its place in its block's driver, which a removed game shifts) and one chooser
  of play or draw (CR 103.1; (k // 2) % 2, so four games cover seat x chooser): the same for every
  candidate.
* **The same shuffles.** A game's randomness is one stream from its seed (the engine's `Game::rng`):
  the chooser, then the setup shuffles (CR 103.3, the starting player's library first), then every
  later shuffle and random choice, consumed as the game asks. A Fisher-Yates shuffle of n cards
  draws the same numbers whatever the cards are, so with one seed two decks of one size get the
  same permutation of their *slots*. The candidates are therefore laid out slot by slot (`slots`:
  the cards they share in the same slots, the cards they differ in paired land with land, then by
  mana value) and given with `"order": "given"` (ENGINE.md 104: the library before the setup shuffle
  is the list in that order; the default sorts by definition, which shifts every card between two
  swapped definitions). Then a shared card lies at the same library position in every candidate's
  game, a swapped card where its counterpart lies, the opening hands agree slot for slot, every draw
  from an unshuffled library agrees, and the opponent's whole library is the same.
* **The pilot's randomness** (a bot's generator, a network search's determinizations and noise) comes
  from the driver's stream of the game (zero-core: (driver seed, game id)): one driver per candidate
  per block, the same driver seed and the same queue order, so game k has the same pilot streams in
  every candidate's run.

What stays common is everything up to the first divergence: a different decision (a mulligan kept by
one deck and not the other, a different play once the swapped card is drawn) or a different number of
random draws (a shuffle effect, a random choice) offsets the two games' streams, and their later
shuffles differ - the rules give no correspondence between two games' later shuffles. Decks of
different sizes (a 41-card human build) cannot share the setup permutation at all: such a pair shares
the seed, the opponent, seat and chooser, and the opponent's library only when the opponent shuffles
first (it takes the first turn). `pairing` makes the weaker designs available for the noise study:
`sorted` (no slot layout: the engine's sorted libraries), `opponent` (same opponent, seat and chooser,
independent seeds and pilot streams), `none` (independent games: the unpaired baseline).

**Statistics** (ENGINE.md 73's conventions): a candidate's score per game (1 / 0.5 / 0), paired
differences D_k = score_i,k - score_j,k over the games both played, their mean, the se of the mean
(sd(D) / sqrt(n)), a 95% normal interval, and the variance the pairing left, var(D) / (var(i) +
var(j)) (1 = an unpaired comparison's). **Racing** (`race`): every surviving candidate plays the same
rounds of games; after each round, for every ordered pair (i, j) of survivors, zero.h2h's GSPRT (the
normal approximation of the per-pair observations, Fishtest's) of H0: E[D_ij] = 0 against H1:
E[D_ij] = -delta, at error rates alpha / (N - 1) and beta; i is dropped once some survivor j's test
accepts H1 (i is worse than j by delta). Testing against every survivor, not a data-chosen leader,
keeps the guarantee simple: the true best build is dropped with probability at most alpha (N - 1
tests, each at most alpha / (N - 1) when its true difference is >= 0). The race ends when one build is
left, when every pair of survivors accepted H0 both ways (equivalent within delta), or at
`max_games`.

**A yardstick only, for the 17lands builds** (DESIGN.md 0): `yardstick` plays a human build as a
candidate; nothing here feeds training.

    python -m mtgz.sealed.compare pair A.json B.json --games 2000
    python -m mtgz.sealed.compare race B0.json B1.json B2.json --delta 0.03
    python -m mtgz.sealed.compare yardstick --pool 0 --ours 0,1,2,3
    python -m mtgz.sealed.compare builds --pool-seed 7 --seeds 0,1,2,3 --out decks/
    python -m mtgz.sealed.compare choice --pool-seed 2026 --k 32 --vs 3 --race
    python -m mtgz.sealed.compare evalpick --evaluator L.pt,S.pt --pool-seed 2026,2027 --k 32 --vs 3 --race
    (--pilot random | heuristic | runs/<run>/models/iter_NNNN.pt [--sims 64 --device cuda])

`choice` races a pool's value-picked build (ENGINE.md 107.2) against random builds of it; `evalpick` races the deck
evaluator's pick (ENGINE.md 108) against the value pick and random builds, per pool, with the paired differences
summarised over the pools.

Cheaper races (docs/ENGINE.md 110, each opt-in; without them every command runs as before): `--batched` (every build's
and, with `evalpick`, every pool's drivers in one process, their batches answered together by `batch.RowExact`, whose
answers depend on each row alone: the per-process race with `--eval rowexact`, bit for bit), `--pilot-search mix`
(self-play's simulation mix instead of `--sims` on every decision; or a lower `--sims`), `--screen-delta 0.05` (a
screening stage before the final tests, alpha split between them: `RaceState`).

    python -m mtgz.sealed.compare evalpick --evaluator L.pt,S.pt --pool-seed 2026,2027,2028 --race --batched \\
        --pilot runs/<run>/models/iter_NNNN.pt --pilot-search mix --device cuda --screen-delta 0.05

Needs `mtgz._engine` (tools/build_py.sh).
"""
import argparse
import collections
import json
import math
import os
import sys
import time

from . import cards as C
from . import engine as R

MASK64 = (1 << 64) - 1
BIG = 1 << 62
GAME_SALT = 0x9A1E_D0C0_0000_0001
FIELD_SALT = 0xF1E1_D000_0000_0002
BUILD_SALT = 0xB011_D000_0000_0003
DRIVER_SALT = 0xD21F_E200_0000_0004
INDEP_SALT = 0x1DE9_0000_0000_0005
PAIRINGS = ('full', 'sorted', 'opponent', 'none')
Z95 = 1.959964


def split64(x):
    """splitmix64's output function (the engine's `rng::split64`)."""
    z = (x + 0x9E3779B97F4A7C15) & MASK64
    z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK64
    z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK64
    return z ^ (z >> 31)


def mix(*xs):
    """A 64-bit seed from integers (chained split64)."""
    h = 0
    for x in xs:
        h = split64(h ^ (int(x) & MASK64))
    return h


# ----------------------------------------------------------------------------- decks and slots

def card_key(name):
    """Pairing order of differing cards: lands first, then by mana value, then by name."""
    c = C.cards().get(name)
    land = c is not None and 'Land' in c['type_line']
    return (0 if land else 1, c['mana_value'] if c else 99, name)


def slots(decks):
    """The decks (lists of names) laid out slot by slot: each returned list is its deck's cards in
    slot order. Every copy of a card is an item held by some of the decks; items held by more decks
    come first, then by `card_key`, and each takes the lowest slot free in all its decks. Two decks:
    their shared cards fill the first slots (in the same slots), and the cards they differ in follow,
    paired by `card_key` rank (land with land, then by mana value). A deck's items that land past its
    own size move into its free slots below (more than two decks can leave gaps)."""
    held = collections.defaultdict(set)
    for i, d in enumerate(decks):
        seen = collections.Counter()
        for n in d:
            held[(n, seen[n])].add(i)
            seen[n] += 1
    order = sorted(held, key=lambda it: (-len(held[it]), card_key(it[0]), it[1]))
    occupied = []
    slot_of = {}
    for it in order:
        ds = held[it]
        s = 0
        while s < len(occupied) and occupied[s] & ds:
            s += 1
        if s == len(occupied):
            occupied.append(set())
        occupied[s] |= ds
        slot_of[it] = s
    out = []
    for i, d in enumerate(decks):
        mine = sorted((slot_of[it], it) for it in order if i in held[it])
        n = len(d)
        lay = [None] * n
        late = []
        for s, it in mine:
            (late.append(it) if s >= n else lay.__setitem__(s, it))
        free = [s for s in range(n) if lay[s] is None]
        for s, it in zip(free, late):
            lay[s] = it
        out.append([it[0] for it in lay])
    return out


def shared_slots(a, b):
    """How many slots two laid-out decks share (the same card in the same slot)."""
    return sum(1 for x, y in zip(a, b) if x == y)


def read_deck(path):
    """A deck file: JSON {"name", "deck": [...], "sideboard": [...]} (or a bare list), or text, one
    card per line with an optional count ("17 Plains"; a line "Sideboard" starts the sideboard)."""
    with open(path, encoding='utf-8') as f:
        txt = f.read()
    name = os.path.splitext(os.path.basename(path))[0]
    s = txt.strip()
    if s.startswith('{') or s.startswith('['):
        d = json.loads(s)
        if isinstance(d, list):
            d = {'deck': d}
        out = {'name': d.get('name', name), 'deck': list(d['deck']), 'sideboard': list(d.get('sideboard', []))}
        out.update({k: d[k] for k in ('colors', 'lands') if k in d})
        return out
    deck, side, cur = [], [], None
    cur = deck
    for line in txt.splitlines():
        line = line.strip()
        if not line or line.startswith('#'):
            continue
        if line.lower().rstrip(':') == 'sideboard':
            cur = side
            continue
        head, _, rest = line.partition(' ')
        if head.isdigit() and rest:
            cur.extend([rest.strip()] * int(head))
        else:
            cur.append(line)
    return {'name': name, 'deck': deck, 'sideboard': side}


def check_decks(cands):
    """Every candidate's cards exist and are supported in the EoE table (loudly otherwise)."""
    names = sorted({n for c in cands for n in c['deck'] + c.get('sideboard', [])})
    table = R.lookup(names)
    bad = {n: v for n, v in table.items() if not isinstance(v, int)}
    if bad:
        raise ValueError(f'unsupported or unknown cards: {bad}')
    for c in cands:
        if not 40 <= len(c['deck']) <= 250:
            raise ValueError(f"{c['name']}: {len(c['deck'])} cards (a limited deck has at least 40)")
        if any(n in C.BASICS for n in c.get('sideboard', [])):
            raise ValueError(f"{c['name']}: a basic land in the sideboard (basics are not pool cards)")


# ----------------------------------------------------------------------------- the field

class Field:
    """The opponents of a comparison. `decks`: fixed decks ({'deck', 'sideboard'}), game k plays
    decks[k % len]; else a seeded stream of our builder's decks from generated pools (`collation`,
    `builder` settings): item j is the j-th pool of the stream whose every card the engine supports,
    pool seed mix(seed, FIELD_SALT, i) for i = 0, 1, ...; pools holding an unsupported card are skipped
    and listed in `skipped` (never a dropped card: the whole pool is passed over)."""

    def __init__(self, decks=None, seed=0, collation='arena', builder=None):
        self.decks = [dict(d) for d in decks] if decks else None
        self.seed, self.collation, self.builder = int(seed), collation, builder
        self._items, self._next, self.skipped = [], 0, []

    def describe(self):
        if self.decks:
            return {'kind': 'fixed', 'decks': len(self.decks)}
        return {'kind': 'generated', 'seed': self.seed, 'collation': self.collation, 'builder': self.builder,
                'pools_skipped': len(self.skipped)}

    def opponent(self, j):
        """Opponent j: {'deck', 'sideboard'} (+ 'pool_seed', 'colors' for a generated one)."""
        if self.decks:
            return self.decks[j % len(self.decks)]
        while len(self._items) <= j:
            ps = mix(self.seed, FIELD_SALT, self._next)
            self._next += 1
            p = R.pool(ps, collation=self.collation)
            bad = [n for n, v in R.lookup(p['cards']).items() if not isinstance(v, int)]
            if bad:
                self.skipped.append({'pool_seed': ps, 'unsupported': sorted(bad)})
                continue
            b = R.build(p['cards'], mix(ps, BUILD_SALT), self.builder)
            self._items.append({'deck': b['deck'], 'sideboard': b['sideboard'], 'pool_seed': ps,
                                'colors': b['colors'] + (('+' + b['splash']) if b['splash'] else '')})
        return self._items[j]


# ----------------------------------------------------------------------------- pilots

def _games():
    """zero.games with MTG Zero's game package registered (`mtgz.az`)."""
    import mtgz.az  # noqa: F401
    from zero import games as G
    return G


SEARCHES = ('flat', 'mix')
EVALS = ('default', 'rowexact')


def run_config(ckpt):
    """The run config of a checkpoint runs/<run>/models/<file>.pt (its config.json), or {}."""
    d = os.path.dirname(os.path.dirname(os.path.abspath(ckpt)))
    try:
        with open(os.path.join(d, 'config.json'), encoding='utf-8') as f:
            c = json.load(f)
    except (OSError, ValueError):
        return {}
    return c if isinstance(c, dict) else {}


def mix_settings(ckpt):
    """Self-play's simulation mix of a checkpoint's run (docs/ENGINE.md 110.3): `sims` on a share `p_full` of
    the searched decisions, `sims_fast` on the others, `sims_minor` on the game's minor decisions (MTG Zero's
    mana payments) - the run config's own values, drawn per decision by the driver's playout cap as self-play
    draws them. A run without a fast budget is refused (it has no mix)."""
    c = run_config(ckpt)
    if not c:
        raise ValueError(f'--pilot-search mix: no run config.json beside {ckpt} (the mix is the run\'s own)')
    if not c.get('sims_fast'):
        raise ValueError(f'--pilot-search mix: the run of {ckpt} has no sims_fast (no simulation mix)')
    return {'sims': int(c['sims']), 'sims_fast': int(c['sims_fast']), 'p_full': float(c['p_full']),
            'sims_minor': int(c['sims_minor']) if c.get('sims_minor') else None}


class Pilot:
    """Who plays both seats: 'random', 'heuristic' (the engine's bots) or a checkpoint path (a
    network seat as zero.h2h plays it: `sims` simulations, `chance_k`, its run's evaluation move rule
    from runs/<run>/config.json next to models/; evaluated on `device`).

    `search` (docs/ENGINE.md 110.3): 'flat' (the default: `sims` on every decision) or 'mix' (self-play's
    simulation mix from the run's config, `mix_settings`: the driver's own playout cap and minor decisions,
    an evaluation seat otherwise - no noise, no temperature, nothing recorded, the run's move rule). The value
    pick (`value_pick`) is always the flat seat's: a value choice searches nothing. `evaluator`: 'default'
    (zero.infer.NetEvaluator) or 'rowexact' (`batch.RowExact`: each row's answer independent of its batch;
    `rows` per chunk, CUDA `graphs` (None: on), CPU `workers`)."""

    def __init__(self, spec='heuristic', sims=64, chance_k=0, c_puct=1.0, device='cpu', key='net', search='flat',
                 evaluator='default', rows=None, graphs=None, workers=None):
        if search not in SEARCHES:
            raise ValueError(f'pilot search {search!r}: one of {SEARCHES}')
        if evaluator not in EVALS:
            raise ValueError(f'pilot evaluator {evaluator!r}: one of {EVALS}')
        self.spec, self.sims, self.chance_k, self.c_puct, self.device, self.key = spec, sims, chance_k, c_puct, device, key
        self.is_net = spec not in ('random', 'heuristic')
        self.search, self.evaluator, self.rows, self.graphs = search, evaluator, rows, graphs
        self.workers = workers
        self.mix = mix_settings(spec) if self.is_net and search == 'mix' else None
        self._model = None
        self._evs = {}

    def describe(self):
        if not self.is_net:
            return self.spec
        out = {'checkpoint': self.spec, 'sims': self.sims, 'chance_k': self.chance_k, 'device': self.device,
               'search': 'flat'}
        if self.mix is not None:
            out['sims'] = self.mix['sims']
            out['search'] = dict(self.mix, kind='mix', note="self-play's simulation mix (the run config's)")
        if self.evaluator != 'default':
            out['evaluator'] = self._ev('rowexact').describe() if self._model is not None else {
                'evaluator': self.evaluator, 'rows': self.rows, 'graphs': self.graphs}
        return out

    def seat(self, search=None):
        """The pilot's driver seat; `search` 'flat' forces the flat seat (the value pick's)."""
        G = _games()
        if not self.is_net:
            return G.seat_dict(self.spec)
        from zero import h2h
        if (search or self.search) == 'flat':
            s = G.nn_seat(self.key, self.sims, self.c_puct, chance_k=self.chance_k)
        else:
            m = self.mix
            # self-play's split as self-play draws it (the driver's playout cap: `sims` on a share p_full,
            # else sims_fast; sims_minor on minor decisions, which the driver applies to self-play seats
            # only), and nothing else of self-play: eps 0 (the root's Dirichlet mix leaves the priors as
            # they are, bit for bit), no temperature (temp_moves 0: every move by the run's move rule),
            # nothing recorded (record False; the race's drivers record nothing either)
            s = G.nn_seat(self.key, m['sims'], self.c_puct, selfplay=True, eps=0.0, temp_moves=0,
                          sims_fast=m['sims_fast'], p_full=m['p_full'], chance_k=self.chance_k,
                          sims_minor=m['sims_minor'])
            s['record'] = False
        s.update(h2h.run_rule(self.spec))
        return s

    def model(self):
        if self._model is None:
            _games()                                # (MTG Zero's game package registered: the encoding guard)
            from zero.net import load
            self._model, _ = load(self.spec, self.device, game='mtg_eoe')
        return self._model

    def _ev(self, kind):
        if kind not in self._evs:
            if kind == 'default':
                from zero.infer import NetEvaluator
                self._evs[kind] = NetEvaluator(self.model(), self.device)
            else:
                from .batch import RowExact
                self._evs[kind] = RowExact(self.model(), self.device, rows=self.rows, graphs=self.graphs,
                                           workers=self.workers)
        return self._evs[kind]

    def evaluators(self, kind=None):
        """{key: the evaluator} (`kind`: 'default' | 'rowexact', None: the pilot's)."""
        if not self.is_net:
            return {}
        return {self.key: self._ev(kind or self.evaluator)}

    def same(self, other):
        return other is None or other is self or (
            (other.spec, other.sims, other.chance_k, other.search, other.evaluator, other.rows, other.graphs)
            == (self.spec, self.sims, self.chance_k, self.search, self.evaluator, self.rows, self.graphs))


# ----------------------------------------------------------------------------- one game's spec

def game_seed(seed, k):
    return mix(seed, GAME_SALT, k)


def chooser_of(k):
    """Game k's chooser of play or draw (CR 103.1)."""
    return (k // 2) % 2


def game_spec(cand, opp, seed_k, hero, chooser, ordered=True, max_turns=60):
    """The `mtg` sealed spec of one game: the candidate (laid-out deck) in seat `hero`, the opponent
    in the other, the game seed and the chooser."""
    seat = {'deck': cand['deck'], 'sideboard': cand.get('sideboard', [])}
    if ordered:
        seat['order'] = 'given'
    sealed = {'set': 'eoe', f'p{hero}': seat,
              f'p{1 - hero}': {'deck': opp['deck'], 'sideboard': opp.get('sideboard', [])}}
    return {'sealed': sealed, 'max_turns': int(max_turns), 'seed': int(seed_k), 'chooser': int(chooser)}


# ----------------------------------------------------------------------------- statistics

def mean_se(xs):
    n = len(xs)
    if n == 0:
        return 0.0, 0.0, 0.0
    m = sum(xs) / n
    var = sum((x - m) ** 2 for x in xs) / (n - 1) if n > 1 else 0.0
    return m, math.sqrt(var / n), var


def elo(p):
    p = min(max(p, 1e-3), 1 - 1e-3)
    return 400 * math.log10(p / (1 - p))


def paired_stats(a, b):
    """Paired statistics of two candidates' scores over common games (lists, game for game)."""
    d = [x - y for x, y in zip(a, b)]
    n = len(d)
    ma, sa, va = mean_se(a)
    mb, sb, vb = mean_se(b)
    md, sd, vd = mean_se(d)
    out = {'n': n, 'a': round(ma, 4), 'a_se': round(sa, 4), 'b': round(mb, 4), 'b_se': round(sb, 4),
           'diff': round(md, 4), 'se': round(sd, 4), 'ci95': [round(md - Z95 * sd, 4), round(md + Z95 * sd, 4)],
           'se_unpaired': round(math.sqrt((va + vb) / n), 4) if n else 0.0,
           'var_ratio': round(vd / (va + vb), 4) if va + vb > 0 else None,
           'elo_vs_field': round(elo(ma) - elo(mb), 1) if n else 0.0,
           'same': sum(1 for x in d if x == 0), 'a_better': sum(1 for x in d if x > 0),
           'b_better': sum(1 for x in d if x < 0)}
    return out


def gsprt_llr(obs, mu0, mu1):
    """The GSPRT's log-likelihood ratio of H1 (mean mu1) against H0 (mu0) for observations `obs`:
    n (mu1 - mu0) (2 mean - mu0 - mu1) / (2 var), var the observations' (1 / n) variance with zero.h2h's
    floor - zero.h2h.sprt_llr's formula on the observations' own scale."""
    n = len(obs)
    if n < 2:
        return 0.0
    m = sum(obs) / n
    var = max(sum((v - m) ** 2 for v in obs) / n, 1e-6)
    return n * (mu1 - mu0) * (2 * m - mu0 - mu1) / (2 * var)


def sprt_bounds(alpha, beta):
    """(lower, upper): below accepts H0, above H1 (Wald)."""
    return math.log(beta / (1 - alpha)), math.log((1 - beta) / alpha)


# ----------------------------------------------------------------------------- playing

class Comparison:
    """Candidates against a field, one pilot, game by game (module docs). `cands`: [{'name', 'deck',
    'sideboard'}]; `pairing`: 'full' | 'sorted' | 'opponent' | 'none'; `block`: games per driver (a
    driver per candidate per block; game k's pilot streams are its block's driver seed and its place
    in the block); `threads`: the driver's search threads."""

    def __init__(self, cands, field, pilot, seed=0, pairing='full', max_turns=60, block=128, threads=4,
                 opp_pilot=None, max_steps=20_000, log=None, progress=None):
        if pairing not in PAIRINGS:
            raise ValueError(f'pairing {pairing!r}: one of {PAIRINGS}')
        if len(cands) < 2:
            raise ValueError('a comparison needs two candidates or more')
        if len({c['name'] for c in cands}) != len(cands):
            raise ValueError('candidate names must differ')
        check_decks(cands)
        opp_pilot = None if pilot.same(opp_pilot) else opp_pilot
        self.cands, self.field, self.pilot, self.opp_pilot = cands, field, pilot, opp_pilot
        self.seed, self.pairing, self.max_turns, self.block = int(seed), pairing, int(max_turns), int(block)
        self.threads, self.max_steps, self.log = int(threads), int(max_steps), log
        self.progress = progress                    # seconds between progress lines of a driver (None: none)
        laid = slots([c['deck'] for c in cands]) if pairing == 'full' else [list(c['deck']) for c in cands]
        self.laid = [dict(c, deck=d) for c, d in zip(cands, laid)]
        self.scores = [dict() for _ in cands]     # candidate -> {k: score}
        self.info = {}                              # k -> {'hero', 'chooser', 'opp'}
        self.failed = []                            # games an engine error removed (repro specs)
        self.unfinished = [0] * len(cands)
        self.seconds = 0.0

    def spec_of(self, i, k, hero):
        """Candidate i's game k with its deck in seat `hero` (JSON-ready dict)."""
        chooser = chooser_of(k)
        n = len(self.cands)
        if self.pairing == 'none':
            j = k * n + i
            opp, seed_k = self.field.opponent(j), mix(self.seed, INDEP_SALT, j)
        elif self.pairing == 'opponent':
            opp, seed_k = self.field.opponent(k), mix(self.seed, INDEP_SALT, k, i)
        else:
            opp, seed_k = self.field.opponent(k), game_seed(self.seed, k)
        return game_spec(self.laid[i], opp, seed_k, hero, chooser, ordered=self.pairing == 'full',
                         max_turns=self.max_turns)

    def driver_seed(self, i, b):
        if self.pairing in ('opponent', 'none'):
            return mix(self.seed, DRIVER_SALT, b, i + 1)
        return mix(self.seed, DRIVER_SALT, b)

    def _seats(self):
        h = self.pilot.seat()
        if self.opp_pilot is None:
            return [[h, h]]
        o = self.opp_pilot.seat()
        return [[h, o], [o, h]]                     # the deck under test sits in seat (game id % 2)

    def _evaluators(self):
        evs = dict(self.pilot.evaluators())
        if self.opp_pilot is not None:
            evs.update(self.opp_pilot.evaluators())
        return evs

    def driver(self, i, b, ks, threads=None, sparse=False):
        """Candidate i's driver of games `ks` (block b, in order: the game ids 0, 1, ... of one driver; the
        deck under test in seat id % 2), started with every game queued. `sparse`: sparse network batches
        (the same features, the same games: zero-core docs/search.md 2.2; the batched races' drivers)."""
        G = _games()
        specs = [json.dumps(self.spec_of(i, k, g % 2), separators=(',', ':')) for g, k in enumerate(ks)]
        # (Game mtg_eoe takes only sealed specs, docs/ENGINE.md 105: the driver's own spec is the
        # block's first game's; every game is enqueued with its own.)
        kw = {'sparse': True} if sparse else {}
        drv = G.Driver('mtg_eoe', self._seats(), spec=specs[0], concurrency=max(1, len(ks)), seed=self.driver_seed(i, b),
                       threads=self.threads if threads is None else threads, groups=2 if self._evaluators() else 1,
                       record=False, max_steps=self.max_steps, rolling=True, queue_only=True, **kw)
        drv.resume(BIG, 0)
        for k, s in zip(ks, specs):
            drv.enqueue(str(k), s)
        return drv

    def _run(self, i, b, ks):
        """Candidate i plays games `ks` (block b: `driver`) -> {k: row}."""
        from zero.infer import drive
        if not ks:
            return {}
        drv = self.driver(i, b, ks)
        rows = {}
        evs = self._evaluators()
        t0 = last = time.time()

        def tick(_lb):
            # a progress line at most every `progress` seconds (a network block can take hours: ENGINE.md 110.5)
            nonlocal last
            now = time.time()
            if now - last >= self.progress:
                last = now
                left = drv.in_flight()['games'] + drv.queued
                self.log('# progress ' + json.dumps({'candidate': self.cands[i]['name'], 'block': b,
                                                     'games': len(ks) - left, 'of': len(ks),
                                                     'leaves': sum(int(getattr(e, 'leaves', 0)) for e in evs.values()),
                                                     'seconds': round(now - t0, 1)}))
        while True:
            drive(drv, evs, on_batch=tick if self.log and self.progress else None)
            for r in drv.take_results():
                rows[int(r['tag'])] = r
            if not drv.queued and not drv.in_flight()['games']:
                break
            time.sleep(0.01)
        drv.end_phase()
        del drv
        return rows

    def play_block(self, b, alive):
        """Block b's games (k in [b * block, (b + 1) * block)) for the candidates `alive`, the deck
        under test in seat (its place in the block) % 2 (= k % 2 with even blocks). A game an engine
        error stops (a panic: an engine bug) is removed for every candidate - its spec kept in `failed`
        for the repro - and the block is played again without it (deterministic: the same failures
        every time)."""
        ks = self.block_games(b)
        t0 = time.time()
        while True:
            got = {}
            try:
                for i in alive:
                    got[i] = self._run(i, b, ks)
            except RuntimeError as e:
                k = self.record_failure(i, b, ks, e)
                ks = [x for x in ks if x != k]
                continue
            break
        self.record_block(b, alive, ks, got)
        self.seconds += time.time() - t0
        return ks

    def block_games(self, b):
        """Block b's games, those an engine error removed left out."""
        gone = {f['k'] for f in self.failed}
        return [k for k in range(b * self.block, (b + 1) * self.block) if k not in gone]

    def record_failure(self, i, b, ks, e):
        """Candidate i's driver of block b (games `ks`) stopped with `e`: the game it names (its queued setup)
        is kept in `failed` with its repro -> its k; an error that names no game is raised again."""
        import re
        m = re.search(r'queued setup "(\d+)"', str(e))
        if not m:
            raise e
        k = int(m.group(1))
        self.failed.append({'k': k, 'candidate': self.cands[i]['name'], 'error': str(e)[:2000],
                            'spec': self.spec_of(i, k, ks.index(k) % 2),
                            'driver_seed': self.driver_seed(i, b), 'game_id': ks.index(k)})
        if self.log:
            self.log(f'# game {k} failed for {self.cands[i]["name"]} (left out for every candidate): {str(e)[:300]}')
        return k

    def record_block(self, b, alive, ks, got):
        """Block b's games `ks` played by the candidates `alive` (`got`: candidate -> {k: result row})."""
        for i in alive:
            for g, k in enumerate(ks):
                r = got[i][k]
                self.scores[i][k] = float(r['result'][g % 2])
                self.unfinished[i] += (not r['over']) or bool(r.get('aborted'))
        for g, k in enumerate(ks):
            opp = self.field.opponent(k) if self.pairing != 'none' else None
            self.info[k] = {'hero': g % 2, 'chooser': chooser_of(k), 'opp_pool_seed': opp.get('pool_seed') if opp else None}

    def common(self, i, j):
        ks = sorted(set(self.scores[i]) & set(self.scores[j]))
        return [self.scores[i][k] for k in ks], [self.scores[j][k] for k in ks]

    def stats(self, i, j):
        a, b = self.common(i, j)
        return paired_stats(a, b)

    def summary(self, i):
        xs = list(self.scores[i].values())
        m, se, _ = mean_se(xs)
        return {'name': self.cands[i]['name'], 'games': len(xs), 'score': round(m, 4), 'se': round(se, 4),
                'unfinished': self.unfinished[i]}

    def setup(self):
        """What the comparison is (for its output)."""
        out = {'pairing': self.pairing, 'seed': self.seed, 'field': self.field.describe(),
               'pilot': self.pilot.describe(), 'max_turns': self.max_turns, 'block': self.block}
        if self.opp_pilot is not None:
            out['opp_pilot'] = self.opp_pilot.describe()
        if self.pairing == 'full':
            out['shared_slots'] = {f'{self.cands[i]["name"]}~{self.cands[j]["name"]}': shared_slots(self.laid[i]['deck'], self.laid[j]['deck'])
                                   for i in range(len(self.cands)) for j in range(i + 1, len(self.cands))}
        sizes = {len(c['deck']) for c in self.cands}
        if len(sizes) > 1:
            out['note'] = ('decks of different sizes: their setup shuffles share no permutation (seed, opponent, seat '
                           'and chooser still shared)')
        return out


def play(comp, games):
    """Every candidate plays games 0..games-1 (rounded up to whole blocks). -> comp."""
    alive = list(range(len(comp.cands)))
    for b in range((games + comp.block - 1) // comp.block):
        comp.play_block(b, alive)
        if comp.log:
            comp.log('# block ' + json.dumps({'block': b, 'games': len(comp.scores[0]), 'seconds': round(comp.seconds, 1)}))
    return comp


def compare_out(comp):
    """Every candidate's score and each pair's paired difference (`compare`'s report)."""
    n = len(comp.cands)
    out = {'setup': comp.setup(), 'candidates': [comp.summary(i) for i in range(n)],
           'pairs': {f'{comp.cands[i]["name"]}-{comp.cands[j]["name"]}': comp.stats(i, j)
                     for i in range(n) for j in range(i + 1, n)},
           'failed': [{k: f[k] for k in ('k', 'candidate', 'error')} for f in comp.failed],
           'seconds': round(comp.seconds, 1), 'game_scores': game_scores(comp)}
    return out


def game_scores(comp):
    """Every candidate's score in each of its games: {name: [[k, score], ...]} (k ascending)."""
    return {c['name']: [[k, comp.scores[i][k]] for k in sorted(comp.scores[i])] for i, c in enumerate(comp.cands)}


def compare(comp, games):
    """Play `games` and report every candidate's score and each pair's paired difference."""
    play(comp, games)
    return compare_out(comp)


class FixedState:
    """`compare`'s games as one round of every block (the batched driver's form of `play`: docs/ENGINE.md 110)."""

    def __init__(self, comp, games):
        self.comp, self.games = comp, int(games)
        self.alive = list(range(len(comp.cands)))
        self.stop = None

    def stage_name(self):
        return 'fixed'

    def round_blocks(self):
        return list(range((self.games + self.comp.block - 1) // self.comp.block))

    def end_round(self):
        self.stop = 'done'
        return {'games': len(self.comp.scores[0]), 'seconds': round(self.comp.seconds, 1)}

    def result(self):
        return compare_out(self.comp)


class RaceState:
    """A race's sequential elimination (`race`), round by round: `round_blocks()` -> the next round's blocks
    (every survivor plays them), `end_round()` once they are recorded -> the round's row (the tests, the
    drops, `stop`), `result()` once `stop` is set. The per-process `race` and the batched driver
    (`batch.run_races`, docs/ENGINE.md 110) run the same state.

    One stage by default: the race of ENGINE.md 104.4. **Screening** (`screen_delta`, docs/ENGINE.md 110.4):
    the same tests at the looser `screen_delta` first, at `screen_alpha` (default alpha / 2) shared over
    each build's N - 1 tests, until one build is left (decided), every survivor pair accepted H0 at the
    screening delta, or `screen_max_games`; then the final stage at `delta` and alpha - screen_alpha (per
    test: / (N - 1), N the race's builds), on all the games played (data reused: each test is checked at
    more times, never with fewer games). The best build is dropped with probability at most screen_alpha +
    (alpha - screen_alpha) = alpha: a union bound over its 2 (N - 1) tests, each crossing its upper bound
    with probability at most its level when its true difference is >= 0 (Wald; checking at round ends
    only lowers it). Beta is the final stage's: a build worse by delta survives a final 'equivalent' with
    probability at most beta per pair; the screening's acceptances of H0 only end the screening."""

    def __init__(self, comp, delta=0.03, alpha=0.05, beta=0.05, round_games=None, max_games=4096, min_games=128,
                 screen_delta=None, screen_alpha=None, screen_max_games=None, screen_min_games=None):
        n = len(comp.cands)
        round_games = int(round_games or comp.block)
        if round_games % comp.block:
            raise ValueError(f'round_games {round_games} is not a whole number of blocks ({comp.block})')
        self.comp, self.n = comp, n
        self.delta, self.alpha, self.beta = delta, alpha, beta
        self.round_games, self.max_games, self.min_games = round_games, max_games, min_games
        stages = []
        final_alpha = alpha
        if screen_delta is not None:
            sa = alpha / 2 if screen_alpha is None else float(screen_alpha)
            if not 0 < sa < alpha:
                raise ValueError(f'screen_alpha {sa}: in (0, alpha {alpha}) (the final stage keeps the rest)')
            if not screen_delta > delta:
                raise ValueError(f'screen_delta {screen_delta}: looser (larger) than delta {delta}')
            stages.append({'name': 'screen', 'delta': screen_delta, 'alpha': sa,
                           'max_games': int(screen_max_games or max_games),
                           'min_games': int(min_games if screen_min_games is None else screen_min_games)})
            final_alpha = alpha - sa
        stages.append({'name': 'final', 'delta': delta, 'alpha': final_alpha, 'max_games': int(max_games),
                       'min_games': int(min_games)})
        for s in stages:
            s['alpha_per_test'] = s['alpha'] / (n - 1)
            s['lower'], s['upper'] = sprt_bounds(s['alpha_per_test'], beta)
        self.stages, self.s = stages, 0
        self.screening = screen_delta is not None
        self.alive = list(range(n))
        self.dropped = {}
        self.rounds = []
        self.stop = None
        self.b = 0

    def stage_name(self):
        return self.stages[self.s]['name']

    def round_blocks(self):
        k = self.round_games // self.comp.block
        bs = list(range(self.b, self.b + k))
        self.b += k
        return bs

    def end_round(self):
        comp = self.comp
        alive = self.alive
        played = len(comp.scores[alive[0]])
        while True:
            st = self.stages[self.s]
            llr = {}
            if played >= st['min_games']:
                for i in alive:
                    for j in alive:
                        if i != j:
                            a, c = comp.common(i, j)
                            llr[(i, j)] = gsprt_llr([x - y for x, y in zip(a, c)], 0.0, -st['delta'])
                drop = {}
                for i in alive:
                    worst = max(((llr[(i, j)], j) for j in alive if j != i), default=None)
                    if worst and worst[0] >= st['upper']:
                        drop[i] = worst[1]
                for i, j in drop.items():
                    self.dropped[i] = {'at_games': played, 'by': comp.cands[j]['name'], 'llr': round(llr[(i, j)], 3)}
                    if self.screening:
                        self.dropped[i]['stage'] = st['name']
                alive = [i for i in alive if i not in drop]
            if st['name'] == 'screen' and len(alive) > 1:
                equiv = llr and all(llr[(i, j)] <= st['lower'] for i in alive for j in alive if i != j)
                if equiv or played >= st['max_games']:
                    st['ended'] = {'games': played, 'reason': 'equivalent' if equiv else 'max_games',
                                   'survivors': [comp.cands[i]['name'] for i in alive]}
                    self.s += 1                       # the final stage's tests, on the same games, now
                    continue
            break
        self.alive = alive
        row = {'games': played, 'alive': [comp.cands[i]['name'] for i in alive],
               'scores': {comp.cands[i]['name']: comp.summary(i)['score'] for i in alive},
               'seconds': round(comp.seconds, 1)}
        if self.screening:
            row['stage'] = st['name']
        self.rounds.append(row)
        if len(alive) == 1:
            self.stop = 'one left'
        elif st['name'] == 'final' and llr and all(llr[(i, j)] <= st['lower'] for i in alive for j in alive if i != j):
            self.stop = 'equivalent'
        elif played >= self.max_games:
            self.stop = 'max_games'
        return row

    def result(self):
        comp, n, alive = self.comp, self.n, self.alive
        fin = self.stages[-1]
        lead = max(alive, key=lambda i: (comp.summary(i)['score'], -i))
        test = {'name': 'pairwise GSPRT against every survivor (zero.h2h.sprt_llr on the paired differences)',
                'h0': 'E[D] = 0', 'h1': f'E[D] = -{self.delta}', 'delta': self.delta, 'alpha': self.alpha,
                'alpha_per_test': round(fin['alpha_per_test'], 5), 'beta': self.beta, 'lower': round(fin['lower'], 3),
                'upper': round(fin['upper'], 3), 'round_games': self.round_games, 'min_games': self.min_games,
                'max_games': self.max_games}
        if self.screening:
            sc = self.stages[0]
            test['final_alpha'] = round(fin['alpha'], 5)
            test['screen'] = {'delta': sc['delta'], 'alpha': round(sc['alpha'], 5),
                              'alpha_per_test': round(sc['alpha_per_test'], 5), 'lower': round(sc['lower'], 3),
                              'upper': round(sc['upper'], 3), 'min_games': sc['min_games'], 'max_games': sc['max_games'],
                              'ended': sc.get('ended')}
            test['error_rates'] = (f"the best build is dropped with probability <= screen alpha {round(sc['alpha'], 5)} + "
                                   f"final alpha {round(fin['alpha'], 5)} = {self.alpha} (union bound over its 2 (N - 1) "
                                   f"tests, each at alpha / (N - 1), N = {n}); beta {self.beta} per pair in the final stage")
        return {'setup': comp.setup(), 'test': test,
                'winner': comp.cands[lead]['name'], 'decided': self.stop == 'one left', 'stop': self.stop,
                'games': len(comp.scores[lead]),
                'survivors': [comp.cands[i]['name'] for i in alive],
                'candidates': [dict(comp.summary(i), **({'dropped': self.dropped[i]} if i in self.dropped else {}))
                               for i in range(n)],
                'vs_winner': {comp.cands[i]['name']: comp.stats(lead, i) for i in range(n) if i != lead},
                'rounds': self.rounds,
                'failed': [{k: f[k] for k in ('k', 'candidate', 'error')} for f in comp.failed],
                'seconds': round(comp.seconds, 1), 'game_scores': game_scores(comp)}


def race(comp, delta=0.03, alpha=0.05, beta=0.05, round_games=None, max_games=4096, min_games=128,
         screen_delta=None, screen_alpha=None, screen_max_games=None, screen_min_games=None):
    """Sequential elimination (module docs): rounds of `round_games` (default: one block) for every
    survivor; after each round with at least `min_games` played, candidate i is dropped once a
    survivor j's GSPRT of H0: E[D_ij] = 0 against H1: E[D_ij] = -delta accepts H1 at alpha / (N - 1),
    beta. Stops at one survivor, at every survivor pair equivalent (H0 accepted both ways), or at
    `max_games` per survivor. -> the verdict: the winner (or the leader, undecided), each candidate's
    games, score and when it was dropped, and the paired difference of each against the winner.
    `screen_delta`: a screening stage first (`RaceState`, docs/ENGINE.md 110.4)."""
    st = RaceState(comp, delta, alpha, beta, round_games, max_games, min_games, screen_delta, screen_alpha,
                   screen_max_games, screen_min_games)
    while st.stop is None:
        for b in st.round_blocks():
            comp.play_block(b, st.alive)
            if comp.log:
                comp.log('# block ' + json.dumps({'block': b, 'alive': len(st.alive),
                                                  'games': len(comp.scores[st.alive[0]]),
                                                  'seconds': round(comp.seconds, 1)}))
        row = st.end_round()
        if comp.log:
            comp.log('# round ' + json.dumps(row))
    return st.result()


# ----------------------------------------------------------------------------- builds and the yardstick

def builder_build(pool_names, seed, builder=None, name=None):
    b = R.build(pool_names, seed, builder)
    return {'name': name or f'b{seed}', 'deck': b['deck'], 'sideboard': b['sideboard'],
            'colors': b['colors'] + (('+' + b['splash']) if b['splash'] else ''), 'lands': b['lands']}


def yardstick_candidates(pool, build, ours, builder=None):
    """A 17lands pool's human build and our builder's builds of the same pool (seeds `ours`) as
    candidates. A YARDSTICK ONLY (DESIGN.md 0)."""
    from . import yardstick as Y
    names = Y._expand(pool.cards)
    cands = [{'name': f'human{build.index}', 'deck': Y.deck(build), 'sideboard': Y.sideboard(pool, build),
              'colors': build.main_colors + (('+' + build.splash_colors) if build.splash_colors else '')}]
    cands += [builder_build(names, s, builder, name=f'ours{s}') for s in ours]
    return cands


def value_pick(pool_names, pilot, k=32, worlds=8, seed=0, collation='arena', max_turns=60):
    """The deck the pilot's network chooses for a pool (docs/ENGINE.md 107.2): one game of the deck-choice spec with the
    pool in seat 0 (`"seed"` fixes the game: its candidates are `engine.candidates(spec, seed)`), its first move by
    the pilot's seat with value choices (`worlds`, temperature 0: the best mean value; zero-core docs/search.md 3.17),
    the driver stopped after it -> (j, the candidates). A bot pilot takes candidate 0 (the heuristic's choice)."""
    G = _games()
    spec = {'sealed': {'set': 'eoe', 'collation': collation, 'p0': {'pool': list(pool_names)},
                       'deck_choice': {'k': int(k), 'explore': 0.0}}, 'max_turns': max_turns, 'seed': int(seed)}
    cands = R.candidates(spec, seed)[0]['candidates']
    if not pilot.is_net:
        return 0, cands
    # (the flat seat and the usual evaluator whatever the race's pilot search and evaluator: a value choice
    # searches nothing, and every pilot setting picks the same build - docs/ENGINE.md 110.3)
    s0 = dict(pilot.seat('flat'), value_choice_worlds=int(worlds), value_choice_temp=0.0)
    drv = G.Driver('mtg_eoe', [[s0, G.seat_dict('random')]], spec=json.dumps(spec), concurrency=1, seed=int(seed),
                   max_steps=1, move_log=1, threads=1)
    from zero.infer import drive
    drive(drv, pilot.evaluators('default'))
    res = drv.take_results()
    if len(res) != 1 or not res[0].get('moves'):
        raise RuntimeError(f'value_pick: no deck choice was played ({res})')
    seat, a, legal = res[0]['moves'][0]
    if seat != 0 or len(legal) != len(cands) or drv.stats()['value_choices'] != 1:
        raise RuntimeError(f'value_pick: the first move is not seat 0\'s value choice ({res[0]["moves"][0]})')
    return list(legal).index(a), cands


def choice_candidates(pool_names, pilot, k=32, worlds=8, seed=0, vs=3, collation='arena'):
    """The value-picked build of a pool (`value_pick`) and the first `vs` other randomised candidates, as comparison
    candidates (docs/ENGINE.md 107.2's demonstration: the picked build against random builds of the same pool)."""
    j, cands = value_pick(pool_names, pilot, k, worlds, seed, collation)
    def cand(i, name):
        b = cands[i]
        return {'name': name, 'deck': b['deck'], 'sideboard': b['sideboard'],
                'colors': b['colors'] + (('+' + b['splash']) if b['splash'] else ''), 'lands': b['lands']}
    out = [cand(j, f'picked{j}')]
    out += [cand(i, f'random{i}') for i in range(1, len(cands)) if i != j][:vs]
    return j, out


def evalpick_candidates(pool_names, evaluators, pilot, k=32, worlds=8, seed=0, vs=3, collation='arena'):
    """The deck evaluators' top builds of a pool (S4a, docs/ENGINE.md 108: `mtgz.sealed.evaluator.rank` over the deck
    choice's K candidates, the first best on a tie), the pilot's value-picked build of the same candidates
    (`value_pick`; a bot pilot: candidate 0, the best guess), each build once, and the first `vs` other randomised
    candidates. `evaluators`: [(label, Evaluator)] (or one Evaluator, label 'eval'). A picked build is named after
    who picked it ('linear7', 'linear+value7'); the rest 'random<i>' -> (info, comparison candidates)."""
    from . import evaluator as EV
    if not isinstance(evaluators, (list, tuple)):
        evaluators = [('eval', evaluators)]
    jv, cands = value_pick(pool_names, pilot, k, worlds, seed, collation)
    scores, picks = {}, {}
    for label, e in evaluators:
        s = [float(x) for x in EV.rank(pool_names, cands, e)]
        scores[label] = [round(x, 4) for x in s]
        picks[label] = max(range(len(cands)), key=lambda i: (s[i], -i))
    picks['value'] = jv
    by = {}
    for label, j in picks.items():
        by.setdefault(j, []).append(label)

    def cand(i, name):
        b = cands[i]
        return {'name': name, 'deck': b['deck'], 'sideboard': b['sideboard'],
                'colors': b['colors'] + (('+' + b['splash']) if b['splash'] else ''), 'lands': b['lands']}
    out = [cand(j, '+'.join(labels) + str(j)) for j, labels in by.items()]
    out += [cand(i, f'random{i}') for i in range(1, len(cands)) if i not in by][:vs]
    names = {label: '+'.join(by[j]) + str(j) for label, j in picks.items()}
    info = {'picks': picks, 'names': names, 'scores': scores,
            'rank_of_value_pick': {label: sorted(range(len(cands)), key=lambda i: (-s[i], i)).index(jv)
                                   for label, s in scores.items()}}
    return info, out


def evalpick_paired(comp, names):
    """The paired differences of an evalpick comparison (`Comparison.stats`, over the games both played): each
    evaluator's pick against the value pick (None when they are the same build) and against each random build, and the
    value pick against each random build. `names`: {label: its pick's candidate name} (`evalpick_candidates`' info)."""
    at = {c['name']: i for i, c in enumerate(comp.cands)}
    rnd = [i for i, c in enumerate(comp.cands) if c['name'].startswith('random')]
    out = {}
    for label, name in names.items():
        i = at[name]
        e = {'vs_random': {comp.cands[r]['name']: comp.stats(i, r) for r in rnd}}
        if label != 'value':
            e['vs_value'] = None if name == names['value'] else comp.stats(i, at[names['value']])
        out[label] = e
    return out


def evalpick_summary(paired):
    """Over pools (a list of `evalpick_paired`): per picker, its mean paired difference against the random builds (a
    pool's random builds averaged first) and, for an evaluator, against the value pick (0 in a pool where they are the
    same build), each with the se over pools."""
    def agg(xs):
        n = len(xs)
        if not n:
            return None
        m = sum(xs) / n
        se = math.sqrt(sum((x - m) ** 2 for x in xs) / (n - 1) / n) if n > 1 else None
        return {'pools': n, 'mean': round(m, 4), 'se': None if se is None else round(se, 4),
                'per_pool': [round(x, 4) for x in xs]}
    out = {}
    for label in (paired[0] if paired else []):
        e = {'vs_random': agg([sum(s['diff'] for s in p[label]['vs_random'].values()) / len(p[label]['vs_random'])
                               for p in paired if p[label]['vs_random']])}
        if label != 'value':
            e['vs_value'] = agg([p[label]['vs_value']['diff'] if p[label]['vs_value'] else 0.0 for p in paired])
        out[label] = e
    return out


# ----------------------------------------------------------------------------- the command line

def _eval_kind(a):
    """The pilots' evaluator: --eval, else 'rowexact' with --batched and 'default' without (ENGINE.md 110.2)."""
    e = getattr(a, 'eval', None) or ('rowexact' if getattr(a, 'batched', False) else 'default')
    if e == 'batchwise' and not getattr(a, 'batched', False):
        raise SystemExit('--eval batchwise is a mode of --batched (each driver\'s batch alone: the per-process answers)')
    return 'default' if e == 'batchwise' else e


def _pilot(a, prefix=''):
    """The pilot of --pilot (prefix 'opp_': --opp-pilot), made once per command (every pool and comparison of it
    shares its network and evaluators)."""
    cache = a.__dict__.setdefault('_pilots', {})
    if prefix not in cache:
        spec = getattr(a, f'{prefix}pilot')
        cache[prefix] = None if spec is None else Pilot(
            spec, sims=a.sims, chance_k=a.chance_k, device=a.device, key=f'{prefix}net' if prefix else 'net',
            search=getattr(a, 'pilot_search', 'flat'), evaluator=_eval_kind(a), rows=getattr(a, 'eval_rows', None),
            workers=getattr(a, 'eval_workers', None))
    return cache[prefix]


def _field(a):
    if a.field:
        d = read_deck(a.field)
        return Field(decks=[d])
    return Field(seed=a.field_seed, collation=a.collation, builder=json.loads(a.field_builder) if a.field_builder else None)


def _log(a):
    return (lambda s: print(s, file=sys.stderr, flush=True)) if not a.quiet else None


def _comparison(a, cands):
    return Comparison(cands, _field(a), _pilot(a), seed=a.seed, pairing=a.pairing, max_turns=a.max_turns,
                      block=a.block, threads=a.threads, opp_pilot=_pilot(a, 'opp_'), log=_log(a),
                      progress=a.progress)


def _state(a, comp):
    """The race (`RaceState`) or the fixed number of games (`FixedState`) the command asks for."""
    if getattr(a, 'race', False):
        return RaceState(comp, a.delta, a.alpha, a.beta, round_games=a.games, max_games=a.max_games,
                         min_games=a.min_games, screen_delta=a.screen_delta, screen_alpha=a.screen_alpha,
                         screen_max_games=a.screen_max_games, screen_min_games=a.screen_min_games)
    return FixedState(comp, a.games)


def _batched(a, comps, on_done=None):
    """Every comparison of `comps` [(label, comp)] at once in one batched driver (docs/ENGINE.md 110) -> ({label:
    its result}, the batched run's statistics)."""
    from . import batch as BT
    evs = {}
    for _, comp in comps:
        evs.update(comp._evaluators())
    mode = 'each' if getattr(a, 'eval', None) == 'batchwise' else 'merged'
    outs, st = BT.run_races([(label, comp, _state(a, comp)) for label, comp in comps], evs, mode=mode,
                            threads=a.driver_threads, log=_log(a), progress=a.progress, on_done=on_done)
    st['evaluator'] = 'batchwise' if mode == 'each' else 'rowexact'
    return outs, st


def _common(p):
    p.add_argument('--pilot', default='heuristic', help="'random', 'heuristic' or a checkpoint path (default heuristic)")
    p.add_argument('--opp-pilot', default=None, help='the field\'s pilot (default: the same as --pilot)')
    p.add_argument('--sims', type=int, default=64, help='a network pilot\'s simulations')
    p.add_argument('--chance-k', type=int, default=0, help='a network pilot\'s chance_k (default 0)')
    p.add_argument('--device', default='cpu', help='a network pilot\'s device')
    p.add_argument('--field', default=None, help='a fixed opponent deck file (default: generated pools, our builder)')
    p.add_argument('--field-seed', type=int, default=0)
    p.add_argument('--collation', default='arena', choices=['arena', 'paper'])
    p.add_argument('--field-builder', default=None, help='the field builder\'s settings (JSON)')
    p.add_argument('--seed', type=int, default=0, help='the comparison seed (games, shuffles, pilot streams)')
    p.add_argument('--pairing', default='full', choices=PAIRINGS)
    p.add_argument('--max-turns', type=int, default=60)
    p.add_argument('--block', type=int, default=128, help='games per driver')
    p.add_argument('--threads', type=int, default=4, help='driver threads')
    p.add_argument('--json', default=None, help='also write the result to this file')
    p.add_argument('--quiet', action='store_true')
    # docs/ENGINE.md 110: the batched driver, the pilot's search, the evaluator, the machine
    p.add_argument('--batched', action='store_true',
                   help='every build (evalpick: every pool) in one process, its drivers\' batches answered together '
                        '(ENGINE.md 110; default: one driver at a time, as before)')
    p.add_argument('--eval', default=None, choices=['default', 'rowexact', 'batchwise'],
                   help="a network pilot's evaluator: 'default' (zero.infer.NetEvaluator; the default without "
                        "--batched), 'rowexact' (each row's answer independent of its batch; the default with "
                        "--batched), 'batchwise' (--batched only: each driver's batch alone, the per-process answers)")
    p.add_argument('--eval-rows', type=int, default=None, help='rowexact: rows per evaluated chunk (the answers '
                                                               'depend on it; default 512 on CUDA, 16 on the CPU)')
    p.add_argument('--eval-workers', type=int, default=None,
                   help='rowexact on the CPU: chunks evaluated side by side on this many threads (each kernel on one '
                        'thread: the answers are the same for any number; default min(8, CPUs))')
    p.add_argument('--pilot-search', default='flat', choices=list(SEARCHES),
                   help="'flat' (--sims on every decision, the default) or 'mix' (self-play's simulation mix: the "
                        "pilot run's sims / sims_fast / p_full / sims_minor; ENGINE.md 110.3)")
    p.add_argument('--driver-threads', type=int, default=1, help='--batched: search threads of each driver')
    p.add_argument('--progress', type=float, default=60.0, help='seconds between progress lines (0: none)')
    p.add_argument('--cpus', type=int, default=None, help='run on this many logical CPUs (process affinity)')
    p.add_argument('--torch-threads', type=int, default=None,
                   help='torch\'s intra-op threads (the default evaluator; rowexact on the CPU runs each kernel on one)')


def _race_args(p):
    p.add_argument('--race', action='store_true', help='sequential elimination instead of a fixed number of games')
    p.add_argument('--games', type=int, default=1024, help='games per candidate (fixed), or per round with --race')
    p.add_argument('--delta', type=float, default=0.03, help='race: the difference in score worth detecting')
    p.add_argument('--alpha', type=float, default=0.05)
    p.add_argument('--beta', type=float, default=0.05)
    p.add_argument('--max-games', type=int, default=8192)
    p.add_argument('--min-games', type=int, default=128)
    p.add_argument('--screen-delta', type=float, default=None,
                   help='race: a screening stage at this looser delta first (e.g. 0.05; ENGINE.md 110.4; default none)')
    p.add_argument('--screen-alpha', type=float, default=None,
                   help='race: the screening stage\'s share of alpha (default alpha / 2; the final stage the rest)')
    p.add_argument('--screen-max-games', type=int, default=None,
                   help='race: games per build after which the screening ends (default --max-games)')
    p.add_argument('--screen-min-games', type=int, default=None,
                   help='race: games per build before the screening stage tests (default --min-games)')


def _run(a, cands, with_comp=False):
    comp = _comparison(a, cands)
    if getattr(a, 'batched', False):
        outs, st = _batched(a, [('0', comp)])
        out = outs['0']
        out['batched'] = st
    elif getattr(a, 'race', False):
        out = race(comp, a.delta, a.alpha, a.beta, round_games=a.games, max_games=a.max_games, min_games=a.min_games,
                   screen_delta=a.screen_delta, screen_alpha=a.screen_alpha, screen_max_games=a.screen_max_games,
                   screen_min_games=a.screen_min_games)
    else:
        out = compare(comp, a.games)
    out['decks'] = [{k: c[k] for k in c if k in ('name', 'colors', 'lands')} for c in cands]
    return (out, comp) if with_comp else out


def _machine(a):
    """--cpus, --torch-threads; stderr line-buffered (progress in a log as it happens)."""
    from . import batch as BT
    BT.line_buffered()
    if getattr(a, 'cpus', None):
        BT.limit_cpus(a.cpus)
    if getattr(a, 'torch_threads', None):
        import torch
        torch.set_num_threads(int(a.torch_threads))


def main(argv=None):
    ap = argparse.ArgumentParser(description='paired deck comparison (docs/ENGINE.md 104)')
    sub = ap.add_subparsers(dest='cmd', required=True)
    pp = sub.add_parser('pair', help='two decks, paired games')
    pp.add_argument('decks', nargs=2)
    _common(pp)
    _race_args(pp)
    pr = sub.add_parser('race', help='several decks, sequential elimination')
    pr.add_argument('decks', nargs='+')
    _common(pr)
    _race_args(pr)
    py = sub.add_parser('yardstick', help='a 17lands pool: the human build against our builds (YARDSTICK ONLY)')
    py.add_argument('--event', default='Sealed', choices=['Sealed', 'TradSealed'])
    py.add_argument('--pool', default='0', help='the pool: an index among the fully supported pools with a 40-card '
                                                'build, or a 17lands pool id')
    py.add_argument('--build', type=int, default=None, help='the human build index (default: the first 40-card one)')
    py.add_argument('--ours', default='0', help='our builder seeds, comma-separated')
    py.add_argument('--builder', default=None, help='our builder\'s settings (JSON)')
    _common(py)
    _race_args(py)
    pc = sub.add_parser('choice', help='a pool\'s value-picked build (the pilot\'s deck choice, ENGINE.md 107.2) against '
                                       'random builds of the same pool')
    pc.add_argument('--pool-seed', type=int, required=True)
    pc.add_argument('--k', type=int, default=32, help='candidates of the deck choice')
    pc.add_argument('--worlds', type=int, default=8, help='the value choice\'s worlds per candidate')
    pc.add_argument('--vs', type=int, default=3, help='random candidates to race against the picked one')
    _common(pc)
    _race_args(pc)
    pe = sub.add_parser('evalpick', help='a pool\'s build picked by the deck evaluator (S4a, ENGINE.md 108) against the '
                                         'value-picked build and random builds of the same pool')
    pe.add_argument('--evaluator', required=True, help='saved deck evaluators (mtgz.sealed.evaluator train), '
                                                       'comma-separated: their picks race in the same games')
    pe.add_argument('--pool-seed', required=True, help='a pool seed, or several comma-separated (one race each)')
    pe.add_argument('--k', type=int, default=32, help='candidates of the deck choice')
    pe.add_argument('--worlds', type=int, default=8, help='the value pick\'s worlds per candidate')
    pe.add_argument('--vs', type=int, default=3, help='random candidates to race against the picks')
    _common(pe)
    _race_args(pe)
    pb = sub.add_parser('builds', help='our builder\'s builds of a generated pool, as deck files')
    pb.add_argument('--pool-seed', type=int, required=True)
    pb.add_argument('--collation', default='arena', choices=['arena', 'paper'])
    pb.add_argument('--seeds', default='0')
    pb.add_argument('--builder', default=None)
    pb.add_argument('--out', required=True)
    a = ap.parse_args(argv)
    if a.cmd != 'builds':
        _machine(a)
    if a.cmd == 'builds':
        p = R.pool(a.pool_seed, collation=a.collation)
        os.makedirs(a.out, exist_ok=True)
        for s in (int(x) for x in a.seeds.split(',')):
            b = builder_build(p['cards'], s, json.loads(a.builder) if a.builder else None)
            path = os.path.join(a.out, f'pool{a.pool_seed}_b{s}.json')
            with open(path, 'w', encoding='utf-8') as f:
                json.dump(b, f, indent=1)
            print(path, b['colors'], b['lands'], 'lands')
        return 0
    if a.cmd == 'choice':
        p = R.pool(a.pool_seed, collation=a.collation)
        j, cands = choice_candidates(p['cards'], _pilot(a), a.k, a.worlds, a.pool_seed, a.vs, a.collation)
        out = _run(a, cands)
        out['choice'] = {'pool_seed': a.pool_seed, 'k': a.k, 'worlds': a.worlds, 'picked': j,
                         'pilot': _pilot(a).describe()}
        if a.json:
            with open(a.json, 'w', encoding='utf-8') as f:
                json.dump(out, f, indent=1)
        print(json.dumps(out), flush=True)
        return 0
    if a.cmd == 'evalpick':
        from . import evaluator as EV
        evs = []
        for path in a.evaluator.split(','):
            e = EV.Evaluator.load(path)
            label = e.kind if e.kind not in [x[0] for x in evs] else os.path.splitext(os.path.basename(path))[0]
            evs.append((label, e))
        per = []
        seeds = [int(x) for x in str(a.pool_seed).split(',')]
        if len(set(seeds)) != len(seeds):
            raise SystemExit(f'--pool-seed {a.pool_seed}: a pool named twice')

        def finish(ps, info, o, comp):
            o['evalpick'] = dict(info, pool_seed=ps, k=a.k, worlds=a.worlds, evaluators=a.evaluator.split(','),
                                 pilot=_pilot(a).describe(), paired=evalpick_paired(comp, info['names']))
            if not a.quiet:
                print('# pool ' + json.dumps({'pool_seed': ps, 'picks': info['picks'],
                                              'vs_random': {k: {n: s['diff'] for n, s in v['vs_random'].items()}
                                                            for k, v in o['evalpick']['paired'].items()}}),
                      file=sys.stderr, flush=True)
            return o
        if a.batched:
            # every pool's candidates first (the evaluators' and the value pick: one decision each), then every
            # pool's race at once in one batched driver (docs/ENGINE.md 110)
            pools = {}
            for ps in seeds:
                p = R.pool(ps, collation=a.collation)
                info, cands = evalpick_candidates(p['cards'], evs, _pilot(a), a.k, a.worlds, ps, a.vs, a.collation)
                pools[str(ps)] = (ps, info, cands, _comparison(a, cands))
                if not a.quiet:
                    print('# candidates ' + json.dumps({'pool_seed': ps, 'picks': info['picks'],
                                                        'builds': [c['name'] for c in cands]}), file=sys.stderr,
                          flush=True)
            done = {}

            def on_done(label, o):
                ps, info, cands, comp = pools[label]
                o['decks'] = [{k: c[k] for k in c if k in ('name', 'colors', 'lands')} for c in cands]
                done[label] = finish(ps, info, o, comp)
            _, st = _batched(a, [(label, v[3]) for label, v in pools.items()], on_done=on_done)
            per = [done[str(ps)] for ps in seeds]
        else:
            # every pool's candidates before any race, as the batched path (a value pick runs on the usual evaluator
            # at the process's torch threads; the row-exact evaluator, made for the first race, sets them to 1)
            picked = []
            for ps in seeds:
                p = R.pool(ps, collation=a.collation)
                picked.append((ps,) + evalpick_candidates(p['cards'], evs, _pilot(a), a.k, a.worlds, ps, a.vs,
                                                          a.collation))
            for ps, info, cands in picked:
                o, comp = _run(a, cands, with_comp=True)
                per.append(finish(ps, info, o, comp))
        out = per[0] if len(per) == 1 else {'pools': per}
        out['summary'] = evalpick_summary([o['evalpick']['paired'] for o in per])
        if a.batched:
            out['batched'] = st
        if a.json:
            with open(a.json, 'w', encoding='utf-8') as f:
                json.dump(out, f, indent=1)
        print(json.dumps(out), flush=True)
        return 0
    if a.cmd == 'yardstick':
        from . import seventeen_lands as L
        from . import yardstick as Y
        pools = L.load(a.event)
        table = Y.lookup(pools)
        ok = [p for p in pools if not Y.unsupported(p, table) and not p.incomplete
              and any(b.deck_size == 40 for b in p.builds)]
        pool = next((p for p in pools if p.id == a.pool), None) if not a.pool.isdigit() else ok[int(a.pool)]
        if pool is None:
            raise SystemExit(f'no pool {a.pool!r}')
        build = (next(b for b in pool.builds if b.deck_size == 40) if a.build is None
                 else next(b for b in pool.builds if b.index == a.build))
        cands = yardstick_candidates(pool, build, [int(x) for x in a.ours.split(',')],
                                     json.loads(a.builder) if a.builder else None)
        out = _run(a, cands)
        out['yardstick'] = {'event': a.event, 'pool': pool.id, 'build': build.index, 'human_record':
                            [build.wins, build.losses], 'note': 'a yardstick only (DESIGN.md 0)'}
    else:
        out = _run(a, [read_deck(p) for p in a.decks])
    if a.json:
        with open(a.json, 'w', encoding='utf-8') as f:
            json.dump(out, f, indent=1)
    print(json.dumps(out), flush=True)
    return 0


if __name__ == '__main__':
    sys.exit(main())
