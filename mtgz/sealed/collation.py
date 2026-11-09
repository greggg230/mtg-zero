"""Edge of Eternities Play Booster collation and a seeded sealed-pool generator, in two
collations: `paper` (Wizards' published Play Booster) and `arena` (MTG Arena's boosters as
17lands' data shows them). docs/sealed_data.md 3-4.

    generate(seed, collation='arena')            # -> SealedPool (cards, boosters, meta)
    sealed_pool(seed, collation='paper')         # -> the 78 nonbasic names

**paper** (Wizards, "Collecting Edge of Eternities", 2025-07,
https://magic.wizards.com/en/news/feature/collecting-edge-of-eternities): 14 cards and a token.

| slot | what |
|---|---|
| commons (7) | 7 of the 81 main-set commons; in 1.8% of boosters a Special Guest replaces one |
| uncommons (3) | 3 of the 100 main-set uncommons |
| wildcard (1) | any rarity: `WILDCARD` (Stellar Sights 12.5%: the "1 in 8 boosters") |
| rare (1) | rare or mythic rare: `RARE` |
| foil (1) | a traditional foil of any rarity: `FOIL` (the finish changes no name) |
| land (1) | a basic land (default frame or borderless celestial, foil or not; uniform by name) |

Rates are per 100,000 boosters. Wizards prints some as "less than 1%": each such entry gets an
equal share of what the printed rates of its slot leave. Booster-fun printings (borderless viewport
lands EOE 277-286, triumphant 287-302, surreal space 303-316) are main-set cards in another frame:
each printing of a group is equally likely, so they make their card's name come up more often.
Each slot is an independent draw (Wizards publishes no in-pack constraint). Optional: the prerelease
pack's traditional foil year-stamped rare or mythic (`prerelease_promo`; assumed: main-set rare vs
mythic as the rare slot's 80.4 : 14.2, uniform within each).

**arena** (MTG Arena; inferred from 17lands' EoE data, an environment model the user allowed on
2026-11-08): 13 cards, no land slot (Arena's basics are free) and no foils (the foil slot is a
nonfoil card of any rarity, `ARENA_FOIL`); booster-fun printings don't weight names (their rates
go to the main-set rarity); Stellar Sights at the rate Arena shows (15.22% of boosters, 95% CI
14.95-15.49%, against paper's 14.0%: the extra comes from the wildcard's main-set rares and
mythics, `ARENA_WILDCARD`). Colour balance, as a print run:

* **Commons** are a window of k consecutive cards (k = 6 to 9: the commons slots plus a common
  wildcard or foil card) at a uniformly random start in a fresh run of the 81 commons: the nine
  GROUP9 commons at every ninth place, the 6 colourless commons never adjacent and at most two in
  any nine places, and one random order of W U B R G repeated over the 75 coloured places. So
  no card repeats, at most one GROUP9 card (with probability k/9), at most two colourless, and
  every colour has one or two commons (a 6-common pack, with a Special Guest, can miss a colour
  when it holds two colourless: ~3.5% of those).
* **Uncommons** are a window of k = 3 to 5 consecutive cards in a fresh run of the 100 uncommons
  in which no two cards within `gap` - 1 places share a colour (a multicolour card counts for both
  its colours, colourless is a colour of its own): gap 4 for up to four uncommons (Arena never
  repeats a colour among them), gap 3 for five (Arena repeats one or two); colourless uncommons
  always at least 5 apart (never two in a pack). The run is built greedily (`_uncommon_weight`,
  `LANE_WEIGHT`), restarting on a dead end.
* Rare-or-higher cards: no repeat in a pack (a repeat is drawn again in its slot).

A window over a run holds each card of the run with the same probability, k / run length, so the
balance changes no card's frequency (no filtering). Paradox Haze, the Special Guest the engine
excludes, is skipped by default in both collations: drawn again within the Special Guest slot
(`paradox_haze=True` keeps it, for statistics).

Randomness is splitmix64 with integer-only sampling (`Rng`), so a Rust port reproduces a pool bit
for bit from its seed and collation; the order of the draws is the order of the code in
`_paper_booster` and `_arena_booster`.
"""
import functools
from dataclasses import dataclass, field

from . import cards as C

TOTAL = 100_000
COMMONS, UNCOMMONS = 7, 3
SPECIAL_GUEST = 1_800            # boosters per 100,000 whose 7th common is a Special Guest
BOOSTERS = 6                     # a sealed pool
COLLATIONS = ('paper', 'arena')
EXCLUDED = ('Paradox Haze',)     # the Special Guest the engine excludes (no IR)
GENERATOR = 'mtgz.sealed.collation 2'

# (printing groups, rarity, boosters per 100,000). A group is a sheet of eoe_set.json (`eoe` the
# main set, `eos` Stellar Sights) or a booster-fun treatment of main-set cards.
WILDCARD = (
    (('eoe',), 'common', 12_500),
    (('eoe',), 'uncommon', 62_500),
    (('eoe',), 'rare', 10_600),
    (('eoe',), 'mythic', 225),                              # "< 1%"
    (('eos',), 'rare', 10_000),
    (('eos',), 'mythic', 2_500),
    (('viewport',), 'rare', 1_000),
    (('viewport',), 'mythic', 225),                         # "< 1%"
    (('triumphant', 'surreal'), 'rare', 225),               # "< 1%"
    (('triumphant', 'surreal'), 'mythic', 225),             # "< 1%"
)
RARE = (
    (('eoe',), 'rare', 80_400),
    (('eoe',), 'mythic', 14_200),
    (('triumphant',), 'rare', 2_000),
    (('surreal',), 'rare', 2_000),
    (('triumphant',), 'mythic', 467),                       # "< 1%"
    (('surreal',), 'mythic', 467),                          # "< 1%"
    (('viewport',), 'mythic', 466),                         # "< 1%"
)
FOIL = (
    (('eoe',), 'common', 58_000),
    (('eoe',), 'uncommon', 32_000),
    (('eoe',), 'rare', 6_400),
    (('eoe',), 'mythic', 1_100),
    (('eos',), 'rare', 1_000),
    (('eos',), 'mythic', 500),                              # "< 1%"
    (('viewport', 'triumphant', 'surreal'), 'rare', 500),   # "< 1%"
    (('viewport', 'triumphant', 'surreal'), 'mythic', 500), # "< 1%"
)
PROMO = ((('eoe',), 'rare', 80_400), (('eoe',), 'mythic', 14_200))
# Arena: booster-fun shares folded into the main-set rarity; Stellar Sights 15,218 per 100,000
# boosters in all (docs/sealed_data.md 4.4): 13,718 in the wildcard (2:1 per card, rare : mythic,
# as paper) with the 1,218 over paper's 12,500 taken from the wildcard's main-set rares (11,825 ->
# 10,673) and mythics (675 -> 609) in proportion; the foil slot's 1,500 as paper.
ARENA_WILDCARD = (
    (('eoe',), 'common', 12_500),
    (('eoe',), 'uncommon', 62_500),
    (('eoe',), 'rare', 10_673),
    (('eoe',), 'mythic', 609),
    (('eos',), 'rare', 10_974),
    (('eos',), 'mythic', 2_744),
)
ARENA_RARE = (
    (('eoe',), 'rare', 84_400),
    (('eoe',), 'mythic', 15_600),
)
ARENA_FOIL = (
    (('eoe',), 'common', 58_000),
    (('eoe',), 'uncommon', 32_000),
    (('eoe',), 'rare', 6_900),
    (('eoe',), 'mythic', 1_600),
    (('eos',), 'rare', 1_000),
    (('eos',), 'mythic', 500),
)
ARENA_STELLAR_SIGHTS = {'rate': 0.15218, 'ci95': (0.1495, 0.1549), 'boosters': 69_886}
SLOTS = {'paper': {'wildcard': WILDCARD, 'rare': RARE, 'foil': FOIL, 'promo': PROMO},
         'arena': {'wildcard': ARENA_WILDCARD, 'rare': ARENA_RARE, 'foil': ARENA_FOIL}}
TREATMENTS = ('viewport', 'triumphant', 'surreal')

# Arena's print runs (docs/sealed_data.md 4.3).
GROUP9 = ('Chrome Companion', 'Command Bridge', 'Drill Too Deep', "Meltstrider's Gear", 'Nanoform Sentinel',
          'Red Tiger Mechan', 'Temporal Intervention', 'Thaumaton Torpedo', 'Zealous Display')
COMMON_RUN, GROUP_STEP = 81, 9
COLOURLESS_GAP = 2           # colourless commons: never adjacent (two apart at least) ...
COLOURLESS_PER_9 = 2         # ... and at most two in any nine consecutive places
UNCOMMON_RUN, UNCOMMON_GAP, UNCOMMON_GAP_5 = 100, 4, 3
MULTI_WEIGHT = 10
LANE_WEIGHT = {4: 1, 3: 5}   # by run gap: the weight of continuing the colour `gap` places back
COLOURLESS_UNCOMMON_GAP = 5  # colourless uncommons never share a pack (Arena: 0 of ~90 expected)
COLOURS = 'WUBRG'
COLOURLESS_BIT = 1 << 5      # in an uncommon run, colourless is a category of its own
MASK = (1 << 64) - 1


class Rng:
    """splitmix64 (Steele, Lea, Flood 2014), with integer-only sampling."""

    def __init__(self, seed):
        self.state = seed & MASK

    def next_u64(self):
        self.state = (self.state + 0x9E3779B97F4A7C15) & MASK
        z = self.state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
        return z ^ (z >> 31)

    def below(self, n):
        """Uniform in [0, n) for 0 < n <= 2**64: rejection of the top partial block, then modulo."""
        if n <= 0:
            raise ValueError('below(n) needs n > 0')
        limit = (1 << 64) - (1 << 64) % n
        while True:
            x = self.next_u64()
            if x < limit:
                return x % n

    def choice(self, seq):
        return seq[self.below(len(seq))]

    def sample(self, seq, k):
        """k distinct positions of seq (partial Fisher-Yates over a copy)."""
        a = list(seq)
        for i in range(k):
            j = i + self.below(len(a) - i)
            a[i], a[j] = a[j], a[i]
        return a[:k]

    def weighted(self, weights):
        """An index with probability weight / sum (integer weights)."""
        x = self.below(sum(weights))
        for i, w in enumerate(weights):
            if x < w:
                return i
            x -= w
        raise AssertionError('unreachable')


# ---------------------------------------------------------------------------------------------
# Sheets.

@functools.lru_cache(None)
def _sheets(path, booster_fun):
    """{(group, rarity): (card names, one per printing)}; `booster_fun=False` folds the booster-fun
    printings into the plain main-set sheets."""
    doc = C.eoe_set(path)
    sheets = {}
    for c in doc['cards']:
        sheets.setdefault((c['sheet'], c['rarity']), []).append(c['name'])
    for t in TREATMENTS:
        for row in doc['treatments'][t]:
            sheets.setdefault((t, row['rarity']), []).append(row['name'])
    sheets = {k: tuple(v) for k, v in sheets.items()}
    if not booster_fun:
        for t in TREATMENTS:
            for r in ('rare', 'mythic'):
                sheets[(t, r)] = sheets[('eoe', r)]
    return sheets


def _colour(name, cards):
    c = cards[name]['colors']
    return c if len(c) == 1 else ('C' if not c else c)


@functools.lru_cache(None)
def _runs(path):
    """The fixed inputs of Arena's print runs: commons by (colour, GROUP9 or not), uncommons by
    colour mask."""
    cards = C.cards(path)
    com = [n for n, c in cards.items() if c['sheet'] == 'eoe' and c['rarity'] == 'common']
    unc = [n for n, c in cards.items() if c['sheet'] == 'eoe' and c['rarity'] == 'uncommon']
    g9 = {}
    rest = {}
    for n in com:
        (g9 if n in GROUP9 else rest).setdefault(_colour(n, cards), []).append(n)
    if sorted(len(v) for v in g9.values()) != [1, 1, 1, 1, 2, 3] or len(g9.get('C', ())) != 3:
        raise AssertionError('GROUP9 must be one card of four colours, two of the fifth, three colourless')
    (doubled,) = [c for c, v in g9.items() if c != 'C' and len(v) == 2]
    for c in COLOURS + 'C':
        want = (15 if c != 'C' else 6) - len(g9.get(c, ()))
        if len(rest.get(c, ())) != want:
            raise AssertionError(f'commons of colour {c}: {len(rest.get(c, ()))}, the cycle needs {want}')
    bit = {c: 1 << i for i, c in enumerate(COLOURS)}
    groups = {}
    for n in sorted(unc):
        m = sum(bit[c] for c in cards[n]['colors']) or COLOURLESS_BIT
        groups.setdefault(m, []).append(n)
    masks = tuple(sorted(groups, key=lambda m: (bin(m).count('1'), m)))
    incidences = [sum(len(groups[m]) for m in masks if m >> b & 1) for b in range(5)]
    return {'g9': {k: tuple(sorted(v)) for k, v in g9.items()}, 'rest': {k: tuple(sorted(v)) for k, v in rest.items()},
            'doubled': doubled, 'umasks': masks, 'ugroups': {m: tuple(groups[m]) for m in masks},
            'incidences': tuple(incidences)}


# ---------------------------------------------------------------------------------------------
# Arena's print runs.

def _cyclic_gap(a, b, n):
    d = abs(a - b) % n
    return min(d, n - d)


def common_pattern(rng, runs):
    """The colour of each place of a fresh 81-common run ('W'..'G', 'C'): GROUP9 places every
    ninth (three of them colourless), three more colourless places (no two colourless places
    adjacent, at most two in any nine consecutive places), and the colour cycle over the 75
    coloured places, consistent with the GROUP9 cards' colours (the colour with two GROUP9 cards on
    the residue that two GROUP9 places share). Drawn by rejection: uniform over the valid layouts."""
    gpos = tuple(range(0, COMMON_RUN, GROUP_STEP))
    others = tuple(p for p in range(COMMON_RUN) if p % GROUP_STEP)
    while True:
        colourless = sorted(rng.sample(gpos, len(runs['g9']['C'])) + rng.sample(others, len(runs['rest']['C'])))
        m = len(colourless)
        if any(_cyclic_gap(a, b, COMMON_RUN) < COLOURLESS_GAP
               for i, a in enumerate(colourless) for b in colourless[i + 1:]):
            continue
        if any((colourless[(i + COLOURLESS_PER_9) % m] - colourless[i]) % COMMON_RUN < GROUP_STEP
               for i in range(m)):
            continue                                 # three colourless within nine places
        cl = set(colourless)
        index, i = {}, 0
        for p in range(COMMON_RUN):
            if p not in cl:
                index[p] = i
                i += 1
        res = [index[p] % 5 for p in gpos if p not in cl]
        counts = [res.count(r) for r in range(5)]
        if sorted(counts) != [1, 1, 1, 1, 2]:
            continue
        cycle = [None] * 5
        cycle[counts.index(2)] = runs['doubled']
        others_c = [c for c in COLOURS if c != runs['doubled']]
        order = rng.sample(others_c, 4)
        for r in range(5):
            if cycle[r] is None:
                cycle[r] = order.pop(0)
        return tuple('C' if p in cl else cycle[index[p] % 5] for p in range(COMMON_RUN))


def _uncommon_weight(size, mask, r, left, lane_weight):
    """Integer weight of placing an uncommon of colour `mask` with `size` cards of that colour left:
    by cards left; times MULTI_WEIGHT for a multicolour card; times `lane_weight` (LANE_WEIGHT of
    the run's gap) when it continues the colour of the card `gap` places back; times each colour's
    urgency squared (its
    incidences left r_c against the run left, (r_c * run)^2 / (21 * left)^2, put over a common
    denominator so the weight stays an integer; colourless has no urgency)."""
    w = size * (MULTI_WEIGHT if bin(mask & 31).count('1') == 2 else 1) * lane_weight
    nc = 0
    for b in range(5):
        if mask >> b & 1:
            w *= (r[b] * UNCOMMON_RUN) ** 2
            nc += 1
    return w * (21 * left) ** (2 * (2 - nc))


def uncommon_pattern(rng, runs, gap=None):
    """The colour mask of each place of a fresh 100-uncommon run in which no two cards within
    `gap` - 1 places share a colour, colourless counting as a colour and no two colourless within
    COLOURLESS_UNCOMMON_GAP - 1 places (cyclically): built left to right, each place drawn among the
    allowed colour groups by `_uncommon_weight`; a dead end restarts the run."""
    masks, groups, inc = runs['umasks'], runs['ugroups'], runs['incidences']
    if any(x != 21 for x in inc):
        raise AssertionError('the urgency weights assume 21 colour incidences per colour')
    n = UNCOMMON_RUN
    gap = gap or UNCOMMON_GAP
    while True:
        rem = [len(groups[m]) for m in masks]
        r = list(inc)
        seq = []
        for step in range(n):
            recent = 0
            for m in seq[-(gap - 1):]:
                recent |= m
            for m in seq[-(COLOURLESS_UNCOMMON_GAP - 1):]:
                recent |= m & COLOURLESS_BIT
            left = n - step
            for j in range(max(0, COLOURLESS_UNCOMMON_GAP - left)):
                recent |= seq[j] & (COLOURLESS_BIT if j >= gap - left else ~0)
            back = seq[step - gap] if step >= gap else 0
            allowed = [i for i, m in enumerate(masks) if rem[i] and not (m & recent)]
            if not allowed:
                break
            weights = [_uncommon_weight(rem[i], masks[i], r, left, LANE_WEIGHT[gap] if masks[i] & back else 1)
                       for i in allowed]
            i = allowed[rng.weighted(weights)]
            seq.append(masks[i])
            rem[i] -= 1
            for b in range(5):
                if masks[i] >> b & 1:
                    r[b] -= 1
        else:
            return tuple(seq)


def _window(rng, run_length, k):
    s = rng.below(run_length)
    return [(s + j) % run_length for j in range(k)]


def arena_commons(rng, runs, k):
    """k commons: a window of a fresh common run."""
    pattern = common_pattern(rng, runs)
    out, used = [], {}
    for p in _window(rng, COMMON_RUN, k):
        c = pattern[p]
        if p % GROUP_STEP == 0:
            out.append(rng.choice(runs['g9'][c]))
        else:
            avail = [x for x in runs['rest'][c] if x not in used.get(c, ())]
            x = rng.choice(avail)
            used.setdefault(c, set()).add(x)
            out.append(x)
    return out


def arena_uncommons(rng, runs, k):
    """k uncommons: a window of a fresh uncommon run."""
    pattern = uncommon_pattern(rng, runs, UNCOMMON_GAP if k <= UNCOMMON_GAP else UNCOMMON_GAP_5)
    out, used = [], set()
    for p in _window(rng, UNCOMMON_RUN, k):
        avail = [x for x in runs['ugroups'][pattern[p]] if x not in used]
        x = rng.choice(avail)
        used.add(x)
        out.append(x)
    return out


# ---------------------------------------------------------------------------------------------
# Boosters and pools.

@dataclass
class SealedPool:
    """A generated sealed pool. `boosters`: each booster's (slot, name) pairs; `cards`: the pool's
    nonbasic names, sorted (the promo included); `basics`: the land slots' basics (paper);
    `meta`: how it was generated (seed, collation, ...)."""
    seed: int
    collation: str
    boosters: list
    promo: str | None = None
    meta: dict = field(default_factory=dict)

    @property
    def cards(self):
        names = [n for b in self.boosters for s, n in b if s != 'land']
        return sorted(names + ([self.promo] if self.promo else []))

    @property
    def basics(self):
        return [n for b in self.boosters for s, n in b if s == 'land']


class Collation:
    """One of COLLATIONS (module docstring)."""

    def __init__(self, collation, *, paradox_haze=False, path=C.EOE_SET):
        if collation not in COLLATIONS:
            raise ValueError(f'collation must be one of {COLLATIONS}, not {collation!r}')
        self.collation = collation
        self.paradox_haze = paradox_haze
        self.sheets = _sheets(path, collation == 'paper')
        self.runs = _runs(path) if collation == 'arena' else None
        self.tables = SLOTS[collation]
        for name, table in self.tables.items():
            if sum(w for *_, w in table) != (TOTAL if name != 'promo' else 94_600):
                raise AssertionError(f'{collation} {name} slot rates do not sum to {TOTAL}')
            for groups, rarity, _ in table:
                for g in groups:
                    if not self.sheets.get((g, rarity)):
                        raise AssertionError(f'{collation} {name} slot: no {rarity} printings in {g}')
        self._cats = {name: [(w, rarity, tuple(n for g in groups for n in self.sheets[(g, rarity)]))
                             for groups, rarity, w in table] for name, table in self.tables.items()}

    def _category(self, rng, slot):
        """(rarity, printings) of a slot's category roll."""
        cats = self._cats[slot]
        _, rarity, printings = cats[rng.weighted([w for w, _, _ in cats])]
        return rarity, printings

    def _guest(self, rng):
        spg = self.sheets[('spg', 'mythic')]
        name = rng.choice(spg)
        while not self.paradox_haze and name in EXCLUDED:
            name = rng.choice(spg)                   # drawn again within the slot
        return name

    def _paper_booster(self, rng):
        guest = rng.below(TOTAL) < SPECIAL_GUEST
        commons = COMMONS - (1 if guest else 0)
        out = [('common', n) for n in rng.sample(self.sheets[('eoe', 'common')], commons)]
        if guest:
            out.append(('special_guest', self._guest(rng)))
        out += [('uncommon', n) for n in rng.sample(self.sheets[('eoe', 'uncommon')], UNCOMMONS)]
        for slot in ('wildcard', 'rare', 'foil'):
            _, printings = self._category(rng, slot)
            out.append((slot, rng.choice(printings)))
        out.append(('land', rng.choice(C.BASICS)))
        return out

    def _arena_booster(self, rng):
        guest = rng.below(TOTAL) < SPECIAL_GUEST
        cats = {slot: self._category(rng, slot) for slot in ('wildcard', 'rare', 'foil')}
        extra_c = [s for s in ('wildcard', 'foil') if cats[s][0] == 'common']
        extra_u = [s for s in ('wildcard', 'foil') if cats[s][0] == 'uncommon']
        slots_c = ['common'] * (COMMONS - (1 if guest else 0)) + extra_c
        out = list(zip(slots_c, arena_commons(rng, self.runs, len(slots_c))))
        if guest:
            out.append(('special_guest', self._guest(rng)))
        slots_u = ['uncommon'] * UNCOMMONS + extra_u
        out += list(zip(slots_u, arena_uncommons(rng, self.runs, len(slots_u))))
        names = {n for _, n in out}
        for slot in ('wildcard', 'rare', 'foil'):
            rarity, printings = cats[slot]
            if rarity in ('common', 'uncommon'):
                continue
            name = rng.choice(printings)
            while name in names:
                name = rng.choice(printings)       # no repeated card in a pack
            names.add(name)
            out.append((slot, name))
        return out

    def booster(self, rng):
        return self._paper_booster(rng) if self.collation == 'paper' else self._arena_booster(rng)

    def generate(self, seed, n=BOOSTERS, prerelease_promo=False):
        if prerelease_promo and self.collation != 'paper':
            raise ValueError('the prerelease promo is a paper product')
        rng = Rng(seed)
        boosters = [self.booster(rng) for _ in range(n)]
        promo = None
        if prerelease_promo:
            _, printings = self._category(rng, 'promo')
            promo = rng.choice(printings)
        meta = {'seed': seed, 'collation': self.collation, 'boosters': n, 'prerelease_promo': prerelease_promo,
                'paradox_haze': self.paradox_haze, 'generator': GENERATOR}
        return SealedPool(seed, self.collation, boosters, promo, meta)


@functools.lru_cache(None)
def get(name, paradox_haze=False):
    """The `Collation` called `name` (cached)."""
    return Collation(name, paradox_haze=paradox_haze)


def generate(seed, *, collation, boosters=BOOSTERS, prerelease_promo=False, paradox_haze=False):
    """A sealed pool of `boosters` EoE Play Boosters from `seed`, in `collation` ('paper' or
    'arena'; no default: say which). The pool records how it was made in `meta`."""
    return get(collation, paradox_haze).generate(seed, boosters, prerelease_promo)


def sealed_pool(seed, *, collation, **kw):
    """The pool's nonbasic card names, sorted: 78 for six boosters (79 with the paper promo)."""
    return generate(seed, collation=collation, **kw).cards


def in_table(name):
    """Whether the engine's EoE card table holds `name`."""
    return name in C.ir_names()
