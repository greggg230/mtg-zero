"""Edge of Eternities Play Booster collation and a seeded sealed-pool generator
(docs/sealed_data.md 2-3).

The spec is Wizards' "Collecting Edge of Eternities" (2025-07,
https://magic.wizards.com/en/news/feature/collecting-edge-of-eternities): a Play Booster holds 14
cards and a token:

| slot | what |
|---|---|
| commons (7) | 7 of the 81 main-set commons; in 1.8% of boosters one of the 10 Special Guests replaces one |
| uncommons (3) | 3 of the 100 main-set uncommons |
| wildcard (1) | any rarity: `WILDCARD` (Stellar Sights 12.5%: the "1 in 8 boosters") |
| rare (1) | rare or mythic rare: `RARE` |
| foil (1) | a traditional foil of any rarity: `FOIL` (the foil only changes the finish) |
| land (1) | a basic land (default frame or borderless celestial, foil or not) |

Rates are per 100,000 boosters. Wizards prints some as "less than 1%": each such entry gets an
equal share of what the printed rates of its slot leave (the same assumption as mtg.wtf's
data/boosters/eoe/play.yaml for the wildcard slot). Booster-fun printings (borderless viewport
lands EOE 277-286, triumphant 287-302, surreal space 303-316) are main-set cards in another frame:
in paper they make their card's name come up more often (`booster_fun=True`, each printing of a
group equally likely). Arena's pools show no such weighting (docs/sealed_data.md 4.2), so by
default their rates go to the main-set sheet of their rarity (`booster_fun=False`).

**Sealed** is six Play Boosters (paper prerelease and sealed, and Arena), with basic lands added
freely, so a pool is the boosters' 13 nonbasic cards each: 78 cards. Arena's boosters have no land
slot (its basics are free; 17lands never records one in a pool) and no foils, which changes no
nonbasic name: `sealed_pool()` follows that by default (`land_slot=False`).

**Not modelled (docs/sealed_data.md 4.3):** the slots here are independent draws. Arena's packs
are colour-balanced (every colour has one or two commons, uncommons spread over the colours, no
card twice in a pack, a group of nine commons at most once per pack: the signature of a print-run
collation), which narrows a pool's colour depth a lot; and Arena's Stellar Sights rate is ~15-16%
of boosters against the 14% of the paper rates. The open question of S2.

Randomness is splitmix64 with integer-only sampling (`Rng`), so a Rust port can reproduce a pool
bit for bit from its seed. The draw order per booster: the Special Guest roll, the commons (a
partial Fisher-Yates shuffle), the guest, the uncommons, then the wildcard, rare and foil slots
(a category roll, then a printing), then the land.
"""
import functools

from . import cards as C

TOTAL = 100_000
COMMONS, UNCOMMONS = 7, 3
SPECIAL_GUEST = 1_800            # boosters per 100,000 whose 7th common is a Special Guest
BOOSTERS = 6                     # a sealed pool

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
SLOTS = {'wildcard': WILDCARD, 'rare': RARE, 'foil': FOIL}
TREATMENTS = ('viewport', 'triumphant', 'surreal')
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
        """Uniform in [0, n): rejection of the top partial block, then modulo."""
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


@functools.lru_cache(None)
def _sheets(path, booster_fun):
    """{(group, rarity): [card names, one per printing]}; `booster_fun=False` folds the booster-fun
    printings into the plain main-set sheets (their slots' rates go to the main set's rarity)."""
    doc = C.eoe_set(path)
    sheets = {}
    for c in doc['cards']:
        sheets.setdefault((c['sheet'], c['rarity']), []).append(c['name'])
    for t in TREATMENTS:
        for row in doc['treatments'][t]:
            sheets.setdefault((t, row['rarity']), []).append(row['name'])
    for k in sheets:
        sheets[k] = tuple(sheets[k])
    if not booster_fun:
        for t in TREATMENTS:
            for r in ('rare', 'mythic'):
                sheets[(t, r)] = sheets[('eoe', r)]
    return sheets


def _check(sheets):
    for name, table in SLOTS.items():
        if sum(w for *_, w in table) != TOTAL:
            raise AssertionError(f'{name} slot rates do not sum to {TOTAL}')
        for groups, rarity, _ in table:
            for g in groups:
                if not sheets.get((g, rarity)):
                    raise AssertionError(f'{name} slot: no {rarity} printings in {g}')


class Collation:
    """The EoE Play Booster (module docstring). `booster_fun=True` weights names by the paper
    booster-fun printings; the default (Arena, measured) gives every rare or mythic of the main set
    the same chance in each slot."""

    def __init__(self, booster_fun=False, path=C.EOE_SET):
        self.booster_fun = booster_fun
        self.sheets = _sheets(path, booster_fun)
        _check(self.sheets)
        self.basics = tuple(n for n in C.BASICS)
        self._slot_cards = {name: [(w, tuple(n for g in groups for n in self.sheets[(g, r)]))
                                   for groups, r, w in table] for name, table in SLOTS.items()}

    def _slot(self, rng, name):
        x = rng.below(TOTAL)
        for w, printings in self._slot_cards[name]:
            if x < w:
                return rng.choice(printings)
            x -= w
        raise AssertionError('unreachable: rates sum to TOTAL')

    def booster(self, rng, land_slot=False):
        """One Play Booster as (slot, card name) pairs; `land_slot` adds the basic land."""
        guest = rng.below(TOTAL) < SPECIAL_GUEST
        commons = COMMONS - (1 if guest else 0)
        out = [('common', n) for n in rng.sample(self.sheets[('eoe', 'common')], commons)]
        if guest:
            out.append(('special_guest', rng.choice(self.sheets[('spg', 'mythic')])))
        out += [('uncommon', n) for n in rng.sample(self.sheets[('eoe', 'uncommon')], UNCOMMONS)]
        for slot in ('wildcard', 'rare', 'foil'):
            out.append((slot, self._slot(rng, slot)))
        if land_slot:
            out.append(('land', rng.choice(self.basics)))
        return out

    def boosters(self, seed, n=BOOSTERS, land_slot=False):
        rng = Rng(seed)
        return [self.booster(rng, land_slot) for _ in range(n)]

    def sealed_pool(self, seed, n=BOOSTERS, land_slot=False):
        """The pool's card names (sorted; repeats for copies)."""
        return sorted(name for b in self.boosters(seed, n, land_slot) for _, name in b)


@functools.lru_cache(None)
def default():
    return Collation()


def sealed_pool(seed, n=BOOSTERS, land_slot=False):
    """A sealed pool of `n` EoE Play Boosters from `seed`: 78 nonbasic card names by default.
    A name can be outside the engine's card table (Paradox Haze, a Special Guest without IR,
    ~1.1% of pools): check with `in_table()`; never drop it silently."""
    return default().sealed_pool(seed, n, land_slot)


def in_table(name):
    return name in C.ir_names()
