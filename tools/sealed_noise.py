"""The paired deck comparison's noise (docs/ENGINE.md 104.5): with the heuristic pilot on both sides,
how many paired games separate two builds that differ by one card, by the land count, by the colour
pair - and how much each part of the pairing removes against unpaired games.

Per pool (a seeded stream of fully supported arena pools, `--pool-seed`): A = our builder's build
(builder seed 0) and four variants:

* `card`  - one of A's nonland spells (drawn at random) swapped for a random sideboard spell castable in
            A's colours (its colours within A's main colours and splash);
* `land+` - A's most expensive spell out, a basic of A's most common basic type in (one more land);
* `land-` - a basic of that type out, a random castable sideboard spell in (one land fewer);
* `pair`  - the builder's build of the pool with the first seed >= 1 whose main colour pair differs.

Each (A, variant) plays `--games` paired games with `mtgz.sealed.compare`'s full pairing, and
`--games-modes` games in each weaker design (`sorted`, `opponent`, `none`), against the generated field
(`--field-seed`). One JSON line per (pool, variant, mode) in OUT/results.jsonl (resumable: a line there
is not played again); `--summary` (also at the end) writes OUT/summary.json and prints the tables.

    python tools/sealed_noise.py --out E:/mtg-zero-scratch/sealed-noise --pools 12 --procs 4 --threads 2
    python tools/sealed_noise.py --out ... --summary
"""
import argparse
import json
import math
import os
import random
import statistics
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)

VARIANTS = ('card', 'land+', 'land-', 'pair')
MODES = ('full', 'sorted', 'opponent', 'none')
Z80 = (1.959964 + 0.841621) ** 2                   # two-sided 5%, power 80%
_FIELDS = {}


def pools(n, seed):
    from mtgz.sealed import compare as K
    from mtgz.sealed import engine as R
    out, i = [], 0
    while len(out) < n:
        ps = K.mix(seed, 0x5EED_9001, i)
        i += 1
        p = R.pool(ps, collation='arena')
        if all(isinstance(v, int) for v in R.lookup(p['cards']).values()):
            out.append(p)
    return out


def castable(name, colors):
    from mtgz.sealed import cards as C
    c = C.cards()[name]
    return 'Land' not in c['type_line'] and set(c['colors']) <= set(colors)


def variants(p, seed):
    """{variant: (A, B)} for pool p (module docs); deterministic from `seed`."""
    from mtgz.sealed import cards as C
    from mtgz.sealed import compare as K
    rng = random.Random(seed)
    a = K.builder_build(p['cards'], 0, name='A')
    cols = a['colors'].replace('+', '')
    spells = [j for j, n in enumerate(a['deck']) if 'Land' not in C.cards()[n]['type_line']]
    side = sorted(a['sideboard'])
    ok_side = [n for n in side if castable(n, cols)] or [n for n in side if 'Land' not in C.cards()[n]['type_line']]
    basics = [n for n in a['deck'] if n in C.BASICS]
    main_basic = max(sorted(set(basics)), key=basics.count)

    def swap(deck, sb, j, new, name):
        deck = list(deck)
        sb = list(sb)
        old = deck[j]
        deck[j] = new
        if new in sb:
            sb.remove(new)
        if old not in C.BASICS:
            sb.append(old)
        return {'name': name, 'deck': deck, 'sideboard': sorted(sb), 'colors': a['colors']}

    out = {}
    j = rng.choice(spells)
    out['card'] = swap(a['deck'], a['sideboard'], j, rng.choice(ok_side), 'B')
    j = max(spells, key=lambda x: (C.cards()[a['deck'][x]]['mana_value'], a['deck'][x]))
    out['land+'] = swap(a['deck'], a['sideboard'], j, main_basic, 'B')
    j = a['deck'].index(main_basic)
    out['land-'] = swap(a['deck'], a['sideboard'], j, rng.choice(ok_side), 'B')
    pair = lambda b: ''.join(sorted(b['colors'].split('+')[0]))   # noqa: E731
    for s in range(1, 200):
        b = K.builder_build(p['cards'], s, name='B')
        if pair(b) != pair(a):
            out['pair'] = b
            break
    return {v: (a, b) for v, b in out.items()}


def work(args):
    """One pool's comparisons (a worker process): every (variant, mode) not in done."""
    pi, p, opts, done = args
    try:
        import psutil
        psutil.Process().nice(psutil.BELOW_NORMAL_PRIORITY_CLASS if os.name == 'nt' else 10)
    except Exception:                              # noqa: BLE001
        pass
    from mtgz.sealed import compare as K
    if opts['field_seed'] not in _FIELDS:            # (one field per worker process: its decks are cached)
        _FIELDS[opts['field_seed']] = K.Field(seed=opts['field_seed'])
    field = _FIELDS[opts['field_seed']]
    rows = []
    for v, (a, b) in variants(p, K.mix(opts['seed'], pi)).items():
        for mode in MODES:
            if (pi, v, mode) in done:
                continue
            games = opts['games'] if mode == 'full' else opts['games_modes']
            t0 = time.time()
            comp = K.Comparison([a, b], field, K.Pilot('heuristic'), seed=K.mix(opts['seed'], pi, VARIANTS.index(v)),
                                pairing=mode, block=opts['block'], threads=opts['threads'])
            out = K.compare(comp, games)
            st = out['pairs']['A-B']
            row = {'pool': pi, 'pool_seed': p['seed'], 'variant': v, 'mode': mode, 'a_colors': a['colors'],
                   'b_colors': b['colors'], 'a_lands': sum(1 for n in a['deck'] if 'Land' in K.C.cards()[n]['type_line']),
                   'b_lands': sum(1 for n in b['deck'] if 'Land' in K.C.cards()[n]['type_line']),
                   'shared_slots': K.shared_slots(*K.slots([a['deck'], b['deck']])), **st,
                   'failed': len(out['failed']), 'failures': [dict(f, spec_seed=g['spec']['seed'])
                                                              for f, g in zip(out['failed'], comp.failed)],
                   'unfinished': [c['unfinished'] for c in out['candidates']],
                   'seconds': round(time.time() - t0, 1)}
            rows.append(row)
            with open(os.path.join(opts['out'], 'results.jsonl'), 'a', encoding='utf-8') as f:
                f.write(json.dumps(row) + '\n')
            print(f"pool {pi} {v:5s} {mode:8s} n {st['n']} diff {st['diff']:+.4f} se {st['se']:.4f} "
                  f"ratio {st['var_ratio']} same {st['same']} ({row['seconds']} s)", flush=True)
    return rows


def read(out):
    path = os.path.join(out, 'results.jsonl')
    if not os.path.exists(path):
        return []
    rows = []
    with open(path, encoding='utf-8') as f:
        for line in f:
            if line.strip():
                rows.append(json.loads(line))
    return rows


def var_d(r):
    return (r['se'] ** 2) * r['n']


def var_unpaired(r):
    return (r['se_unpaired'] ** 2) * r['n']


def summary(out):
    rows = read(out)
    by = {}
    for r in rows:
        by.setdefault((r['variant'], r['mode']), []).append(r)
    res = {'variants': {}}
    lines = ['variant  pools  |diff| median (range)        var(D)/game  var_unpaired  ratio full / sorted / opponent / none'
             '   n80 paired @1/2/3pp      n80 unpaired @1/2/3pp   n80 paired at its own diff (median)']
    for v in VARIANTS:
        full = by.get((v, 'full'), [])
        if not full:
            continue
        diffs = [abs(r['diff']) for r in full]
        vd = statistics.median(var_d(r) for r in full)
        vu = statistics.median(var_unpaired(r) for r in full)
        ratios = {m: (statistics.median(r['var_ratio'] for r in by[(v, m)]) if by.get((v, m)) else None) for m in MODES}
        own = [Z80 * var_d(r) / r['diff'] ** 2 for r in full if r['diff'] != 0]
        signif = sum(1 for r in full if abs(r['diff']) > 1.96 * r['se'])
        d = {'pools': len(full), 'abs_diff_median': round(statistics.median(diffs), 4),
             'abs_diff_range': [round(min(diffs), 4), round(max(diffs), 4)],
             'significant_at_n': f"{signif} of {len(full)} at {full[0]['n']} games",
             'var_d': round(vd, 4), 'var_unpaired': round(vu, 4),
             'ratio': {m: (round(x, 3) if x is not None else None) for m, x in ratios.items()},
             'n80_paired': {f'{k}pp': math.ceil(Z80 * vd / (k / 100) ** 2) for k in (1, 2, 3)},
             'n80_unpaired': {f'{k}pp': math.ceil(Z80 * vu / (k / 100) ** 2) for k in (1, 2, 3)},
             'n80_own_diff_median': math.ceil(statistics.median(own)) if own else None,
             'same_share': round(statistics.median(r['same'] / r['n'] for r in full), 3),
             'per_pool': [{'pool': r['pool'], 'a': r['a_colors'], 'b': r['b_colors'], 'diff': r['diff'], 'se': r['se'],
                           'ratio': r['var_ratio']} for r in sorted(full, key=lambda r: r['pool'])]}
        res['variants'][v] = d
        lines.append(f"{v:7s}  {d['pools']:5d}  {d['abs_diff_median']:.4f} ({d['abs_diff_range'][0]:.4f}-{d['abs_diff_range'][1]:.4f})"
                     f"   {d['var_d']:.4f}       {d['var_unpaired']:.4f}        "
                     + ' / '.join('-' if ratios[m] is None else f'{ratios[m]:.3f}' for m in MODES)
                     + f"     {d['n80_paired']['1pp']}/{d['n80_paired']['2pp']}/{d['n80_paired']['3pp']}"
                     + f"     {d['n80_unpaired']['1pp']}/{d['n80_unpaired']['2pp']}/{d['n80_unpaired']['3pp']}"
                     + f"     {d['n80_own_diff_median']}")
    with open(os.path.join(out, 'summary.json'), 'w', encoding='utf-8') as f:
        json.dump(res, f, indent=1)
    print('\n'.join(lines))
    return res


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('--out', required=True)
    ap.add_argument('--pools', type=int, default=12)
    ap.add_argument('--pool-seed', type=int, default=104)
    ap.add_argument('--field-seed', type=int, default=0)
    ap.add_argument('--seed', type=int, default=1)
    ap.add_argument('--games', type=int, default=16384, help='paired games per comparison, full pairing')
    ap.add_argument('--games-modes', type=int, default=4096, help='games per comparison in the other designs')
    ap.add_argument('--block', type=int, default=512)
    ap.add_argument('--threads', type=int, default=2)
    ap.add_argument('--procs', type=int, default=4)
    ap.add_argument('--summary', action='store_true', help='only summarise OUT/results.jsonl')
    a = ap.parse_args(argv)
    os.makedirs(a.out, exist_ok=True)
    if a.summary:
        summary(a.out)
        return 0
    if a.procs * a.threads > 8:
        ap.error('at most 8 threads in all (--procs x --threads): the machine is shared')
    done = {(r['pool'], r['variant'], r['mode']) for r in read(a.out)}
    opts = {'out': a.out, 'field_seed': a.field_seed, 'seed': a.seed, 'games': a.games, 'games_modes': a.games_modes,
            'block': a.block, 'threads': a.threads}
    ps = pools(a.pools, a.pool_seed)
    jobs = [(i, p, opts, done) for i, p in enumerate(ps)]
    t0 = time.time()
    if a.procs <= 1:
        for j in jobs:
            work(j)
    else:
        import multiprocessing as mp
        with mp.get_context('spawn').Pool(a.procs) as pool:
            for _ in pool.imap_unordered(work, jobs):
                pass
    print(f'played in {time.time() - t0:.0f} s', flush=True)
    summary(a.out)
    return 0


if __name__ == '__main__':
    sys.exit(main())
