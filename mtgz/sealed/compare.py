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
    (--pilot random | heuristic | runs/<run>/models/iter_NNNN.pt [--sims 64 --device cuda])

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
        return {'name': d.get('name', name), 'deck': list(d['deck']), 'sideboard': list(d.get('sideboard', []))}
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


class Pilot:
    """Who plays both seats: 'random', 'heuristic' (the engine's bots) or a checkpoint path (a
    network seat as zero.h2h plays it: `sims` simulations, `chance_k`, its run's evaluation move rule
    from runs/<run>/config.json next to models/; evaluated on `device`)."""

    def __init__(self, spec='heuristic', sims=64, chance_k=0, c_puct=1.0, device='cpu', key='net'):
        self.spec, self.sims, self.chance_k, self.c_puct, self.device, self.key = spec, sims, chance_k, c_puct, device, key
        self.is_net = spec not in ('random', 'heuristic')
        self._ev = None

    def describe(self):
        if not self.is_net:
            return self.spec
        return {'checkpoint': self.spec, 'sims': self.sims, 'chance_k': self.chance_k, 'device': self.device}

    def seat(self):
        G = _games()
        if not self.is_net:
            return G.seat_dict(self.spec)
        from zero import h2h
        s = G.nn_seat(self.key, self.sims, self.c_puct, chance_k=self.chance_k)
        s.update(h2h.run_rule(self.spec))
        return s

    def evaluators(self):
        if not self.is_net:
            return {}
        if self._ev is None:
            from zero.infer import NetEvaluator
            from zero.net import load
            m, _ = load(self.spec, self.device, game='mtg')
            self._ev = NetEvaluator(m, self.device)
        return {self.key: self._ev}

    def same(self, other):
        return other is None or other is self or (other.spec, other.sims, other.chance_k) == (self.spec, self.sims, self.chance_k)


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
                 opp_pilot=None, max_steps=20_000, log=None):
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

    def _run(self, i, b, ks):
        """Candidate i plays games `ks` (block b, in order: the game ids 0, 1, ... of one driver; the
        deck under test in seat id % 2) -> {k: row}."""
        G = _games()
        from zero.infer import drive
        specs = [json.dumps(self.spec_of(i, k, g % 2), separators=(',', ':')) for g, k in enumerate(ks)]
        drv = G.Driver('mtg', self._seats(), spec='{}', concurrency=max(1, len(ks)), seed=self.driver_seed(i, b),
                       threads=self.threads, groups=2 if self._evaluators() else 1, record=False,
                       max_steps=self.max_steps, rolling=True, queue_only=True)
        drv.resume(BIG, 0)
        for k, s in zip(ks, specs):
            drv.enqueue(str(k), s)
        rows = {}
        evs = self._evaluators()
        while True:
            drive(drv, evs)
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
        import re
        ks = [k for k in range(b * self.block, (b + 1) * self.block) if k not in {f['k'] for f in self.failed}]
        t0 = time.time()
        while True:
            got = {}
            try:
                for i in alive:
                    got[i] = self._run(i, b, ks)
            except RuntimeError as e:
                m = re.search(r'queued setup "(\d+)"', str(e))
                if not m:
                    raise
                k = int(m.group(1))
                self.failed.append({'k': k, 'candidate': self.cands[i]['name'], 'error': str(e)[:2000],
                                    'spec': self.spec_of(i, k, ks.index(k) % 2),
                                    'driver_seed': self.driver_seed(i, b), 'game_id': ks.index(k)})
                if self.log:
                    self.log(f'# game {k} failed for {self.cands[i]["name"]} (left out for every candidate): {str(e)[:300]}')
                ks = [x for x in ks if x != k]
                continue
            break
        for i in alive:
            for g, k in enumerate(ks):
                r = got[i][k]
                self.scores[i][k] = float(r['result'][g % 2])
                self.unfinished[i] += (not r['over']) or bool(r.get('aborted'))
        for g, k in enumerate(ks):
            opp = self.field.opponent(k) if self.pairing != 'none' else None
            self.info[k] = {'hero': g % 2, 'chooser': chooser_of(k), 'opp_pool_seed': opp.get('pool_seed') if opp else None}
        self.seconds += time.time() - t0
        return ks

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


def compare(comp, games):
    """Play `games` and report every candidate's score and each pair's paired difference."""
    play(comp, games)
    n = len(comp.cands)
    out = {'setup': comp.setup(), 'candidates': [comp.summary(i) for i in range(n)],
           'pairs': {f'{comp.cands[i]["name"]}-{comp.cands[j]["name"]}': comp.stats(i, j)
                     for i in range(n) for j in range(i + 1, n)},
           'failed': [{k: f[k] for k in ('k', 'candidate', 'error')} for f in comp.failed],
           'seconds': round(comp.seconds, 1)}
    return out


def race(comp, delta=0.03, alpha=0.05, beta=0.05, round_games=None, max_games=4096, min_games=128):
    """Sequential elimination (module docs): rounds of `round_games` (default: one block) for every
    survivor; after each round with at least `min_games` played, candidate i is dropped once a
    survivor j's GSPRT of H0: E[D_ij] = 0 against H1: E[D_ij] = -delta accepts H1 at alpha / (N - 1),
    beta. Stops at one survivor, at every survivor pair equivalent (H0 accepted both ways), or at
    `max_games` per survivor. -> the verdict: the winner (or the leader, undecided), each candidate's
    games, score and when it was dropped, and the paired difference of each against the winner."""
    n = len(comp.cands)
    round_games = int(round_games or comp.block)
    if round_games % comp.block:
        raise ValueError(f'round_games {round_games} is not a whole number of blocks ({comp.block})')
    a1 = alpha / (n - 1)
    lo, hi = sprt_bounds(a1, beta)
    alive = list(range(n))
    dropped = {}
    rounds = []
    stop = None
    b = 0
    while stop is None:
        for _ in range(round_games // comp.block):
            comp.play_block(b, alive)
            b += 1
        played = len(comp.scores[alive[0]])
        llr = {}
        if played >= min_games:
            for i in alive:
                for j in alive:
                    if i != j:
                        a, c = comp.common(i, j)
                        llr[(i, j)] = gsprt_llr([x - y for x, y in zip(a, c)], 0.0, -delta)
            drop = {}
            for i in alive:
                worst = max(((llr[(i, j)], j) for j in alive if j != i), default=None)
                if worst and worst[0] >= hi:
                    drop[i] = worst[1]
            for i, j in drop.items():
                dropped[i] = {'at_games': played, 'by': comp.cands[j]['name'], 'llr': round(llr[(i, j)], 3)}
            alive = [i for i in alive if i not in drop]
        row = {'games': played, 'alive': [comp.cands[i]['name'] for i in alive],
               'scores': {comp.cands[i]['name']: comp.summary(i)['score'] for i in alive},
               'seconds': round(comp.seconds, 1)}
        rounds.append(row)
        if comp.log:
            comp.log('# round ' + json.dumps(row))
        if len(alive) == 1:
            stop = 'one left'
        elif llr and all(llr[(i, j)] <= lo for i in alive for j in alive if i != j):
            stop = 'equivalent'
        elif played >= max_games:
            stop = 'max_games'
    lead = max(alive, key=lambda i: (comp.summary(i)['score'], -i))
    out = {'setup': comp.setup(),
           'test': {'name': 'pairwise GSPRT against every survivor (zero.h2h.sprt_llr on the paired differences)',
                    'h0': 'E[D] = 0', 'h1': f'E[D] = -{delta}', 'delta': delta, 'alpha': alpha,
                    'alpha_per_test': round(a1, 5), 'beta': beta, 'lower': round(lo, 3), 'upper': round(hi, 3),
                    'round_games': round_games, 'min_games': min_games, 'max_games': max_games},
           'winner': comp.cands[lead]['name'], 'decided': stop == 'one left', 'stop': stop,
           'games': len(comp.scores[lead]),
           'survivors': [comp.cands[i]['name'] for i in alive],
           'candidates': [dict(comp.summary(i), **({'dropped': dropped[i]} if i in dropped else {})) for i in range(n)],
           'vs_winner': {comp.cands[i]['name']: comp.stats(lead, i) for i in range(n) if i != lead},
           'rounds': rounds,
           'failed': [{k: f[k] for k in ('k', 'candidate', 'error')} for f in comp.failed],
           'seconds': round(comp.seconds, 1)}
    return out


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


# ----------------------------------------------------------------------------- the command line

def _pilot(a, prefix=''):
    spec = getattr(a, f'{prefix}pilot')
    if spec is None:
        return None
    return Pilot(spec, sims=a.sims, chance_k=a.chance_k, device=a.device, key=f'{prefix}net' if prefix else 'net')


def _field(a):
    if a.field:
        d = read_deck(a.field)
        return Field(decks=[d])
    return Field(seed=a.field_seed, collation=a.collation, builder=json.loads(a.field_builder) if a.field_builder else None)


def _comparison(a, cands):
    log = (lambda s: print(s, file=sys.stderr, flush=True)) if not a.quiet else None
    return Comparison(cands, _field(a), _pilot(a), seed=a.seed, pairing=a.pairing, max_turns=a.max_turns,
                      block=a.block, threads=a.threads, opp_pilot=_pilot(a, 'opp_'), log=log)


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


def _race_args(p):
    p.add_argument('--race', action='store_true', help='sequential elimination instead of a fixed number of games')
    p.add_argument('--games', type=int, default=1024, help='games per candidate (fixed), or per round with --race')
    p.add_argument('--delta', type=float, default=0.03, help='race: the difference in score worth detecting')
    p.add_argument('--alpha', type=float, default=0.05)
    p.add_argument('--beta', type=float, default=0.05)
    p.add_argument('--max-games', type=int, default=8192)
    p.add_argument('--min-games', type=int, default=128)


def _run(a, cands):
    comp = _comparison(a, cands)
    if getattr(a, 'race', False):
        out = race(comp, a.delta, a.alpha, a.beta, round_games=a.games, max_games=a.max_games, min_games=a.min_games)
    else:
        out = compare(comp, a.games)
    out['decks'] = [{k: c[k] for k in c if k in ('name', 'colors', 'lands')} for c in cands]
    return out


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
    pb = sub.add_parser('builds', help='our builder\'s builds of a generated pool, as deck files')
    pb.add_argument('--pool-seed', type=int, required=True)
    pb.add_argument('--collation', default='arena', choices=['arena', 'paper'])
    pb.add_argument('--seeds', default='0')
    pb.add_argument('--builder', default=None)
    pb.add_argument('--out', required=True)
    a = ap.parse_args(argv)
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
