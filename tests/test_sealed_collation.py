"""The EoE Play Booster collation and the sealed-pool generator (mtgz/sealed/collation.py,
docs/sealed_data.md 3). Pure Python: no engine module needed."""
import collections

import pytest

from mtgz.sealed import cards as C
from mtgz.sealed import collation as K


def test_splitmix64_matches_the_reference_stream():
    r = K.Rng(0)
    assert [r.next_u64() for _ in range(3)] == [0xE220A8397B1DCDAF, 0x6E789E6AA1B965F4, 0x06C45D188009454F]


def test_rng_sampling_is_in_range_and_distinct():
    r = K.Rng(7)
    assert all(0 <= r.below(13) < 13 for _ in range(1000))
    for _ in range(200):
        s = r.sample(range(81), 7)
        assert len(set(s)) == 7 and all(0 <= x < 81 for x in s)
    with pytest.raises(ValueError):
        r.below(0)


def test_every_slot_sums_to_100_percent_and_every_group_has_printings():
    for table in K.SLOTS.values():
        assert sum(w for *_, w in table) == K.TOTAL
    for fun in (False, True):
        sh = K.Collation(booster_fun=fun).sheets
        assert len(sh[('eoe', 'common')]) == 81 and len(sh[('eoe', 'uncommon')]) == 100
        assert len(sh[('spg', 'mythic')]) == 10
    paper = K.Collation(booster_fun=True).sheets
    assert paper[('viewport', 'rare')] == ('Breeding Pool', 'Godless Shrine', 'Sacred Foundry', 'Stomping Ground',
                                           'Watery Grave')
    assert paper[('triumphant', 'rare')].count('Starwinder') == 1 and paper[('surreal', 'rare')].count('Starwinder') == 1
    arena = K.Collation().sheets
    assert arena[('viewport', 'rare')] == arena[('eoe', 'rare')]          # folded into the main set


def test_the_same_seed_gives_the_same_pool():
    a = K.sealed_pool(12345)
    assert a == K.sealed_pool(12345) == K.Collation().sealed_pool(12345)
    assert a != K.sealed_pool(12346)
    assert len(a) == 78 and a == sorted(a)
    assert all(n in C.cards() and n not in C.BASICS for n in a)


def test_the_generator_is_pinned():
    # A change to the draw order, the rates or the card list changes these: on purpose only
    # (a Rust port must reproduce them from the seed).
    import hashlib
    import json
    assert K.Collation().boosters(1)[0] == [
        ('common', 'Nebula Dragon'), ('common', 'Comet Crawler'), ('common', 'Brightspear Zealot'),
        ('common', 'Biosynthic Burst'), ('common', 'Selfcraft Mechan'), ('common', 'Nanoform Sentinel'),
        ('common', 'Hullcarver'), ('uncommon', 'Codecracker Hound'), ('uncommon', 'Tragic Trajectory'),
        ('uncommon', 'Haliya, Ascendant Cadet'), ('wildcard', 'Brightspear Zealot'),
        ('rare', 'Synthesizer Labship'), ('foil', 'Adagia, Windswept Bastion')]
    pools = json.dumps([K.sealed_pool(s) for s in range(100)]).encode()
    assert hashlib.sha256(pools).hexdigest()[:16] == '6aa6a58750e64302'


def test_booster_slot_structure():
    col = K.Collation()
    cards = C.cards()
    guests = 0
    for seed in range(300):
        for land in (False, True):
            for b in col.boosters(seed, land_slot=land):
                slots = [s for s, _ in b]
                assert len(b) == 13 + land
                guest = 'special_guest' in slots
                guests += guest and not land
                assert slots.count('common') == 7 - guest and slots.count('uncommon') == 3
                assert slots.count('wildcard') == slots.count('rare') == slots.count('foil') == 1
                commons = [n for s, n in b if s == 'common']
                uncommons = [n for s, n in b if s == 'uncommon']
                assert len(set(commons)) == len(commons) and len(set(uncommons)) == 3
                assert all(cards[n]['sheet'] == 'eoe' and cards[n]['rarity'] == 'common' for n in commons)
                assert all(cards[n]['sheet'] == 'eoe' and cards[n]['rarity'] == 'uncommon' for n in uncommons)
                (rare,) = [n for s, n in b if s == 'rare']
                assert cards[rare]['sheet'] == 'eoe' and cards[rare]['rarity'] in ('rare', 'mythic')
                for s, n in b:
                    if s == 'special_guest':
                        assert cards[n]['sheet'] == 'spg'
                    if s == 'land':
                        assert n in C.BASICS
    assert 0 < guests < 300 * 6 * 0.05


def test_rates_come_out_as_published():
    col = K.Collation()
    cards = C.cards()
    n = 0
    tally = collections.Counter()
    for seed in range(2000):
        for b in col.boosters(seed):
            n += 1
            for slot, name in b:
                c = cards[name]
                tally[(slot, c['sheet'], c['rarity'])] += 1
    share = {k: v / n for k, v in tally.items()}
    assert abs(sum(v for (s, sh, r), v in share.items() if s == 'wildcard' and sh == 'eos') - 0.125) < 0.01
    assert abs(sum(v for (s, sh, r), v in share.items() if s == 'special_guest') - 0.018) < 0.004
    assert abs(share[('rare', 'eoe', 'mythic')] - 0.156) < 0.01          # 14.2% + the booster-fun mythics
    assert abs(share[('foil', 'eoe', 'common')] - 0.58) < 0.015
    assert abs(share[('wildcard', 'eoe', 'uncommon')] - 0.625) < 0.015


def test_paper_booster_fun_weights_raise_those_names():
    paper, arena = K.Collation(booster_fun=True), K.Collation()
    fun = {r['name'] for t in K.TREATMENTS for r in C.eoe_set()['treatments'][t]}
    def share(col):
        t = collections.Counter(n for s in range(1500) for n in col.sealed_pool(s))
        rares = [n for n, c in C.cards().items() if c['sheet'] == 'eoe' and c['rarity'] == 'rare']
        a = sum(t[x] for x in rares if x in fun) / sum(1 for x in rares if x in fun)
        b = sum(t[x] for x in rares if x not in fun) / sum(1 for x in rares if x not in fun)
        return a / b
    assert share(paper) > 1.05 and abs(share(arena) - 1) < 0.06


def test_land_slot_and_names_outside_the_table():
    pool = K.sealed_pool(3, land_slot=True)
    assert len(pool) == 84 and sum(n in C.BASICS for n in pool) == 6
    assert K.in_table('Annul') and not K.in_table('Paradox Haze')
    hazes = sum('Paradox Haze' in K.sealed_pool(s) for s in range(3000))
    assert 10 < hazes < 70                      # ~1.07% of pools: emitted, never dropped
