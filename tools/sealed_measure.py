"""Measure the EoE sealed-pool generator against what Arena opened (docs/sealed_data.md 4).

A YARDSTICK USE of the 17lands data (DESIGN.md 0): it reports where the generator agrees with
Arena's pools and packs and where it doesn't; nothing here feeds back into the generator's rates.

    python tools/sealed_measure.py                 # 20,000 generated pools per variant
    python tools/sealed_measure.py --pools 5000 --json out.json

Pools (17lands Premier and Traditional Sealed, complete pools): size; rarity counts by sheet (main
set C/U/R/M, Stellar Sights R/M, Special Guests); pools with a Stellar Sights land / a Special
Guest / Paradox Haze; cards per colour (a card counts for each of its colours), multicolour,
colourless; commons per colour; duplicates. Per card: each main-set rare's and mythic's copies per
pool, cards with a booster-fun printing (EOE 277-316) against the rest; the commons' uniformity.

Packs (the fresh PremierDraft packs, `seventeen_lands.load_draft_packs()`): rarity counts, the
commons' colours, the uncommons' spread over the colours, duplicates in a pack, and GROUP9 (nine
commons that never share an Arena pack).
"""
import argparse
import collections
import json
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from mtgz.sealed import cards as C  # noqa: E402
from mtgz.sealed import collation as K  # noqa: E402
from mtgz.sealed import seventeen_lands as L  # noqa: E402

POOL_COUNTS = ('size', 'eoe:common', 'eoe:uncommon', 'eoe:rare', 'eoe:mythic', 'eos:rare', 'eos:mythic',
               'spg:mythic', 'eoe:rare+mythic', 'W', 'U', 'B', 'R', 'G', 'multicolour', 'colourless',
               'common:W', 'common:U', 'common:B', 'common:R', 'common:G', 'common:colourless',
               'uncommon:W', 'uncommon:multicolour', 'distinct', 'duplicated', 'max_copies')
POOL_SHARES = ('has_eos', 'has_spg', 'has_paradox_haze')
PACK_COUNTS = ('size', 'eoe:common', 'eoe:uncommon', 'eoe:rare', 'eoe:mythic', 'eos', 'spg:mythic',
               'common_colours', 'common_max_colour', 'common:colourless', 'uncommon_max_colour',
               'uncommon:multicolour', 'duplicated', 'group9')
PACK_SHARES = ('commons_1_or_2_per_colour', 'uncommons_at_most_1_per_colour', 'no_duplicate', 'group9_at_most_1')
# Nine commons that are never together in any of the 6,018 fresh Arena packs (about 50 shared packs
# expected per pair), though each is as common as any other common (docs/sealed_data.md 4.3).
GROUP9 = frozenset({'Chrome Companion', 'Command Bridge', 'Drill Too Deep', "Meltstrider's Gear",
                    'Nanoform Sentinel', 'Red Tiger Mechan', 'Temporal Intervention', 'Thaumaton Torpedo',
                    'Zealous Display'})


def _common_colour(c):
    return c['colors'] if len(c['colors']) == 1 else ('colourless' if not c['colors'] else 'multicolour')


def features(cards_in):
    """{name: count} (a pool or a pack) -> {feature: value}."""
    cards = C.cards()
    f = collections.Counter()
    unc = collections.Counter()
    for n, k in cards_in.items():
        c = cards[n]
        f['size'] += k
        f[f'{c["sheet"]}:{c["rarity"]}'] += k
        if c['sheet'] == 'eoe' and c['rarity'] in ('rare', 'mythic'):
            f['eoe:rare+mythic'] += k
        for col in c['colors']:
            f[col] += k
        if len(c['colors']) > 1:
            f['multicolour'] += k
        if not c['colors']:
            f['colourless'] += k
        if c['sheet'] == 'eoe' and c['rarity'] in ('common', 'uncommon'):
            f[f'{c["rarity"]}:{_common_colour(c)}'] += k
        if c['sheet'] == 'eoe' and c['rarity'] == 'uncommon':
            for col in c['colors']:
                unc[col] += k
        if n in GROUP9:
            f['group9'] += k
    com = [f[f'common:{x}'] for x in C.COLOURS]
    f['eos'] = f['eos:rare'] + f['eos:mythic']
    f['common_colours'] = sum(1 for x in com if x)
    f['common_max_colour'] = max(com)
    f['uncommon_max_colour'] = max(unc[x] for x in C.COLOURS)
    f['distinct'] = len(cards_in)
    f['duplicated'] = sum(1 for k in cards_in.values() if k > 1)
    f['max_copies'] = max(cards_in.values())
    f['has_eos'] = int(f['eos'] > 0)
    f['has_spg'] = int(f['spg:mythic'] > 0)
    f['has_paradox_haze'] = int('Paradox Haze' in cards_in)
    f['commons_1_or_2_per_colour'] = int(all(1 <= x <= 2 for x in com))
    f['uncommons_at_most_1_per_colour'] = int(f['uncommon_max_colour'] <= 1)
    f['no_duplicate'] = int(f['duplicated'] == 0)
    f['group9_at_most_1'] = int(f['group9'] <= 1)
    return f


def summarise(items, counts, shares, per_card_stats=True):
    feats = [features(p) for p in items]
    n = len(feats)
    out = {'n': n}
    for k in counts:
        v = [f[k] for f in feats]
        m = sum(v) / n
        sd = math.sqrt(sum((x - m) ** 2 for x in v) / n)
        out[k] = {'mean': round(m, 4), 'sd': round(sd, 4), 'se': round(sd / math.sqrt(n), 4),
                  'dist': dict(sorted(collections.Counter(v).items()))}
    for k in shares:
        v = sum(f[k] for f in feats)
        out[k] = {'share': round(v / n, 4), 'se': round(math.sqrt(v / n * (1 - v / n) / n), 4)}
    if per_card_stats:
        out['per_card'] = per_card(items)
    return out


def per_card(pools):
    """Mean copies per pool of each main-set rare / mythic, cards with a booster-fun printing vs
    the rest, and a chi-square of the commons' copies against uniform."""
    cards = C.cards()
    doc = C.eoe_set()
    fun = collections.Counter(r['name'] for t in K.TREATMENTS for r in doc['treatments'][t])
    tot = collections.Counter()
    for p in pools:
        tot.update(p)
    n = len(pools)
    out = {}
    for rarity in ('rare', 'mythic'):
        names = [c for c, row in cards.items() if row['sheet'] == 'eoe' and row['rarity'] == rarity]
        a = [tot[x] / n for x in names if fun[x]]
        b = [tot[x] / n for x in names if not fun[x]]
        out[rarity] = {'booster_fun_cards': len(a), 'per_pool_booster_fun': round(sum(a) / max(1, len(a)), 4),
                       'other_cards': len(b), 'per_pool_other': round(sum(b) / max(1, len(b)), 4),
                       'ratio': round((sum(a) / len(a)) / (sum(b) / len(b)), 4) if a and b else None}
    commons = [c for c, row in cards.items() if row['sheet'] == 'eoe' and row['rarity'] == 'common']
    obs = [tot[x] for x in commons]
    exp = sum(obs) / len(obs)
    out['commons_chi2'] = {'chi2': round(sum((o - exp) ** 2 / exp for o in obs), 1), 'df': len(obs) - 1}
    return out


def table(results, counts, shares, per_card_rows=True):
    names = list(results)
    rows = [('feature',) + tuple(names)]
    for k in counts:
        rows.append((k,) + tuple(f'{results[x][k]["mean"]:.3f} +-{results[x][k]["se"]:.3f} (sd {results[x][k]["sd"]:.2f})'
                                 for x in names))
    for k in shares:
        rows.append((k,) + tuple(f'{results[x][k]["share"]:.4f} +-{results[x][k]["se"]:.4f}' for x in names))
    if per_card_rows:
        for r in ('rare', 'mythic'):
            rows.append((f'{r}: booster-fun / other per card',) + tuple(
                f'{results[x]["per_card"][r]["ratio"]} ({results[x]["per_card"][r]["per_pool_booster_fun"]} / '
                f'{results[x]["per_card"][r]["per_pool_other"]})' for x in names))
        rows.append(('commons chi2 (df 80)',) + tuple(str(results[x]['per_card']['commons_chi2']['chi2'])
                                                      for x in names))
    w = [max(len(r[i]) for r in rows) for i in range(len(rows[0]))]
    return '\n'.join('  '.join(c.ljust(w[i]) for i, c in enumerate(r)).rstrip() for r in rows)


def generated_pools(n, booster_fun):
    col = K.Collation(booster_fun=booster_fun)
    return [collections.Counter(col.sealed_pool(seed)) for seed in range(n)]


def generated_packs(n_pools, booster_fun):
    col = K.Collation(booster_fun=booster_fun)
    return [collections.Counter(name for _, name in b) for seed in range(n_pools) for b in col.boosters(seed)]


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('--pools', type=int, default=20000, help='generated pools per variant (seeds 0..n-1)')
    ap.add_argument('--json', help='write every number here')
    a = ap.parse_args(argv)
    pools = {
        '17lands Sealed': [p.cards for p in L.load('Sealed') if not p.incomplete],
        '17lands TradSealed': [p.cards for p in L.load('TradSealed') if not p.incomplete],
        'generator': generated_pools(a.pools, False),
        'generator, paper booster-fun': generated_pools(a.pools, True),
    }
    pool_res = {k: summarise(v, POOL_COUNTS, POOL_SHARES) for k, v in pools.items()}
    print(f'POOLS (generated: seeds 0..{a.pools - 1})')
    print(table(pool_res, POOL_COUNTS, POOL_SHARES))
    packs = {
        '17lands PremierDraft fresh packs': [p['cards'] for p in L.load_draft_packs()
                                             if sum(p['cards'].values()) == 13],
        'generator': generated_packs(a.pools // 6, False),
    }
    pack_res = {k: summarise(v, PACK_COUNTS, PACK_SHARES, per_card_stats=False) for k, v in packs.items()}
    print(f'\nPACKS (17lands: 13-card fresh packs; generated: the boosters of seeds 0..{a.pools // 6 - 1})')
    print(table(pack_res, PACK_COUNTS, PACK_SHARES, per_card_rows=False))
    if a.json:
        with open(a.json, 'w', encoding='utf-8', newline='') as f:
            json.dump({'pools': pool_res, 'packs': pack_res}, f, indent=1)


if __name__ == '__main__':
    sys.exit(main())
