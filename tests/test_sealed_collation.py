"""The EoE Play Booster collations, paper and arena, and the sealed-pool generator
(mtgz/sealed/collation.py, docs/sealed_data.md 3-4). Pure Python: no engine module needed."""
import collections
import hashlib
import json

import pytest

from mtgz.sealed import cards as C
from mtgz.sealed import collation as K

CARDS = C.cards()


def _is(name, sheet, *rarities):
    return CARDS[name]['sheet'] == sheet and CARDS[name]['rarity'] in rarities


def _category(name):
    return CARDS[name]['colors'] or 'C'


def test_splitmix64_matches_the_reference_stream():
    r = K.Rng(0)
    assert [r.next_u64() for _ in range(3)] == [0xE220A8397B1DCDAF, 0x6E789E6AA1B965F4, 0x06C45D188009454F]


def test_rng_sampling_is_in_range_and_distinct():
    r = K.Rng(7)
    assert all(0 <= r.below(13) < 13 for _ in range(1000))
    for _ in range(200):
        s = r.sample(range(81), 7)
        assert len(set(s)) == 7 and all(0 <= x < 81 for x in s)
    counts = collections.Counter(r.weighted([1, 0, 3]) for _ in range(4000))
    assert counts[1] == 0 and 2.5 < counts[2] / counts[0] < 3.5
    with pytest.raises(ValueError):
        r.below(0)


def test_the_collation_must_be_named():
    with pytest.raises(TypeError):
        K.generate(1)                               # no default: say paper or arena
    with pytest.raises(ValueError):
        K.generate(1, collation='mtgo')
    with pytest.raises(ValueError):
        K.generate(1, collation='arena', prerelease_promo=True)


def test_every_slot_table_sums_to_100_percent():
    for name in K.COLLATIONS:
        for slot, table in K.SLOTS[name].items():
            if slot != 'promo':
                assert sum(w for *_, w in table) == K.TOTAL, (name, slot)
        K.Collation(name)                           # checks every group has printings
    arena_ss = sum(w for g, _, w in K.ARENA_WILDCARD + K.ARENA_FOIL if g == ('eos',))
    assert arena_ss == round(K.ARENA_STELLAR_SIGHTS['rate'] * K.TOTAL)
    lo, hi = K.ARENA_STELLAR_SIGHTS['ci95']
    assert lo < K.ARENA_STELLAR_SIGHTS['rate'] < hi
    paper_ss = sum(w for g, _, w in K.WILDCARD + K.FOIL if g == ('eos',))
    assert paper_ss == 14_000


def test_the_same_seed_gives_the_same_pool_and_the_pool_says_how_it_was_made():
    for name in K.COLLATIONS:
        a = K.generate(12345, collation=name)
        assert a.cards == K.generate(12345, collation=name).cards == K.sealed_pool(12345, collation=name)
        assert a.cards != K.sealed_pool(12346, collation=name)
        assert len(a.cards) == 78 and a.cards == sorted(a.cards)
        assert all(n in CARDS and n not in C.BASICS for n in a.cards)
        assert a.meta == {'seed': 12345, 'collation': name, 'boosters': 6, 'prerelease_promo': False,
                          'paradox_haze': False, 'generator': K.GENERATOR}
    assert len(K.generate(3, collation='paper').basics) == 6 and K.generate(3, collation='arena').basics == []


def test_the_generator_is_pinned():
    # A change to the draw order, the rates, the runs or the card list changes these: on purpose
    # only (a Rust port must reproduce them from the seed).
    assert K.get('paper').generate(1).boosters[0] == [
        ('common', 'Nebula Dragon'), ('common', 'Comet Crawler'), ('common', 'Brightspear Zealot'),
        ('common', 'Biosynthic Burst'), ('common', 'Selfcraft Mechan'), ('common', 'Nanoform Sentinel'),
        ('common', 'Hullcarver'), ('uncommon', 'Codecracker Hound'), ('uncommon', 'Tragic Trajectory'),
        ('uncommon', 'Haliya, Ascendant Cadet'), ('wildcard', 'Brightspear Zealot'),
        ('rare', 'Synthesizer Labship'), ('foil', 'Susur Secundi, Void Altar'), ('land', 'Island')]
    assert K.get('arena').generate(1).boosters[0] == [
        ('common', 'Radiant Strike'), ('common', 'Gravblade Heavy'), ('common', 'Gene Pollinator'),
        ('common', 'Thaumaton Torpedo'), ('common', 'Illvoi Light Jammer'), ('common', 'Melded Moxite'),
        ('common', 'Exosuit Savior'), ('uncommon', 'Bygone Colossus'), ('uncommon', 'Broodguard Elite'),
        ('uncommon', 'All-Fates Stalker'), ('wildcard', 'Molecular Modifier'), ('foil', 'Tapestry Warden'),
        ('rare', 'Sothera, the Supervoid')]
    for name, want in (('paper', '9976058bbcce5a66'), ('arena', '7ececb4b1964e72c')):
        pools = json.dumps([K.sealed_pool(s, collation=name) for s in range(20)]).encode()
        assert hashlib.sha256(pools).hexdigest()[:16] == want, name


def test_paper_booster_slot_structure():
    col = K.get('paper')
    guests = 0
    for seed in range(300):
        for b in col.generate(seed).boosters:
            slots = [s for s, _ in b]
            assert len(b) == 14
            guest = 'special_guest' in slots
            guests += guest
            assert slots.count('common') == 7 - guest and slots.count('uncommon') == 3
            assert slots.count('wildcard') == slots.count('rare') == slots.count('foil') == slots.count('land') == 1
            commons = [n for s, n in b if s == 'common']
            uncommons = [n for s, n in b if s == 'uncommon']
            assert len(set(commons)) == len(commons) and len(set(uncommons)) == 3
            assert all(_is(n, 'eoe', 'common') for n in commons) and all(_is(n, 'eoe', 'uncommon') for n in uncommons)
            (rare,) = [n for s, n in b if s == 'rare']
            assert _is(rare, 'eoe', 'rare', 'mythic')
            for s, n in b:
                assert s != 'special_guest' or (_is(n, 'spg', 'mythic') and n != 'Paradox Haze')
                assert s != 'land' or n in C.BASICS
    assert 0 < guests < 300 * 6 * 0.05


def test_paper_rates_come_out_as_published():
    col = K.get('paper')
    n = 0
    tally = collections.Counter()
    for seed in range(2000):
        for b in col.generate(seed).boosters:
            n += 1
            for slot, name in b:
                tally[(slot, CARDS[name]['sheet'], CARDS[name]['rarity'])] += 1
    share = {k: v / n for k, v in tally.items()}
    assert abs(sum(v for (s, sh, r), v in share.items() if s == 'wildcard' and sh == 'eos') - 0.125) < 0.01
    assert abs(sum(v for (s, sh, r), v in share.items() if s == 'special_guest') - 0.018) < 0.004
    assert abs(share[('rare', 'eoe', 'mythic')] - 0.156) < 0.01          # 14.2% + the booster-fun mythics
    assert abs(share[('foil', 'eoe', 'common')] - 0.58) < 0.015
    assert abs(share[('wildcard', 'eoe', 'uncommon')] - 0.625) < 0.015


def test_paper_booster_fun_weights_raise_those_names_and_arena_does_not():
    fun = {r['name'] for t in K.TREATMENTS for r in C.eoe_set()['treatments'][t]}
    rares = [n for n in CARDS if _is(n, 'eoe', 'rare')]

    def ratio(name, pools):
        t = collections.Counter(n for s in range(pools) for n in K.sealed_pool(s, collation=name))
        a = sum(t[x] for x in rares if x in fun) / sum(1 for x in rares if x in fun)
        b = sum(t[x] for x in rares if x not in fun) / sum(1 for x in rares if x not in fun)
        return a / b
    assert ratio('paper', 1500) > 1.05
    sheets = K.get('arena').sheets
    assert sheets[('viewport', 'rare')] == sheets[('eoe', 'rare')]          # folded into the main set


def test_arena_packs_are_balanced_print_runs():
    col = K.get('arena')
    stats = collections.Counter()
    for seed in range(40):
        for b in col.generate(seed).boosters:
            names = [n for _, n in b]
            assert len(b) == 13 and 'land' not in [s for s, _ in b]
            assert len(set(names)) == 13                                     # no repeated card
            commons = [n for n in names if _is(n, 'eoe', 'common')]
            uncommons = [n for n in names if _is(n, 'eoe', 'uncommon')]
            cc = collections.Counter(_category(n) for n in commons)
            assert cc['C'] <= 2 and sum(n in K.GROUP9 for n in commons) <= 1
            if len(commons) >= 7 or cc['C'] <= 1:
                assert all(1 <= cc[x] <= 2 for x in 'WUBRG'), commons
            inc = collections.Counter(x for n in uncommons for x in _category(n))
            assert max(inc.values()) <= (1 if len(uncommons) <= 4 else 2), uncommons
            stats['packs'] += 1
            stats['group9'] += sum(n in K.GROUP9 for n in commons)
    assert stats['packs'] == 240 and 0.7 < stats['group9'] / 240 < 1.0          # k/9: ~0.85


def test_arena_runs_hold_every_card_once_with_their_spacing():
    runs = K._runs(C.EOE_SET)
    rng = K.Rng(5)
    for _ in range(10):
        pat = K.common_pattern(rng, runs)
        assert collections.Counter(pat) == {'W': 15, 'U': 15, 'B': 15, 'R': 15, 'G': 15, 'C': 6}
        g9 = sorted(pat[p] for p in range(0, 81, 9))
        assert g9 == sorted(_category(n) for n in K.GROUP9)
        cl = [p for p, c in enumerate(pat) if c == 'C']
        assert all((b - a) % 81 >= 2 and (a - b) % 81 >= 2 for a in cl for b in cl if a != b)
        for s in range(81):
            w = [pat[(s + j) % 81] for j in range(9)]
            assert w.count('C') <= 2
            for k in (7, 8, 9):
                assert all(1 <= w[:k].count(x) <= 2 for x in 'WUBRG')
    for gap in (4, 3):
        seq = K.uncommon_pattern(rng, runs, gap)
        assert sorted(seq) == sorted(m for m in runs['umasks'] for _ in runs['ugroups'][m])
        n = len(seq)
        for i in range(n):
            assert all(not (seq[i] & seq[(i + d) % n]) for d in range(1, gap))
            assert all(not (seq[i] & seq[(i + d) % n] & K.COLOURLESS_BIT) for d in range(1, K.COLOURLESS_UNCOMMON_GAP))


def test_arena_stellar_sights_rate():
    col = K.get('arena')
    boosters = [b for s in range(150) for b in col.generate(s).boosters]
    ss = sum(1 for b in boosters for _, n in b if CARDS[n]['sheet'] == 'eos')
    assert abs(ss / len(boosters) - 0.152) < 0.035


def test_paradox_haze_is_skipped_unless_asked_for():
    def hazes(**kw):
        return sum('Paradox Haze' in K.sealed_pool(s, collation='paper', **kw) for s in range(3000))
    assert hazes() == 0
    assert 10 < hazes(paradox_haze=True) < 70          # ~1.07% of pools
    assert K.generate(1, collation='arena', paradox_haze=True).meta['paradox_haze'] is True
    assert K.in_table('Annul') and not K.in_table('Paradox Haze')


def test_the_paper_prerelease_promo():
    p = K.generate(8, collation='paper', prerelease_promo=True)
    assert p.promo and _is(p.promo, 'eoe', 'rare', 'mythic')
    assert len(p.cards) == 79 and p.promo in p.cards and p.meta['prerelease_promo'] is True
    assert K.generate(8, collation='paper').boosters == p.boosters      # drawn after the boosters
