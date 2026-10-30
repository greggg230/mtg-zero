"""Is a belief over the opponent's known top cards worth building? (docs/ENGINE.md 72.8): the stage-1
test of 66.6 for the top of the library.

  python tools/top_probe.py --run runs/r3-2b-beliefsearch --ckpt runs/r3-2b-beliefsearch/models/iter_0196.pt \
      --dir E:/mtg-zero-scratch/topprobe [--skip-dump]

1. The run's recorded games (`<run>/replays/*.json.gz`, `python -m mtgz.az.replays`) -> `games.txt`
   (each game's seed: zero-core's driver seeds game k's generator from the batch seed's k-th draw;
   its frames' actions) and `spec.json`.
2. `cargo test --release -p engine --test az top_known_probe_dump -- --ignored` replays them exactly
   (it fails if one diverges) and dumps every decision where the decider's opponent knows top cards
   of its library that the decider does not: the decider's features, the cards, what taught the
   opponent each, the decider's unknown pool of the opponent's cards, whether each card was drawn.
3. Per known card, 10 folds by game, cross-entropy in nats (standard errors by a bootstrap over
   games): uniform (today's deal: the unknown pool), count-matching (66.6), count-matching per source
   (Brainstorm, Ponder, Dragon's Rage Channeler, other: public), and residual heads (66.6's shape) on
   the checkpoint's frozen trunk z plus the twelve floats of 72.2, without and with the source.

Uses CARGO_TARGET_DIR if set (else E:/cargo-target/mtg-zero/<checkout dir> on Windows, as build_py.sh).
"""
import argparse
import collections
import glob
import gzip
import json
import os
import random
import subprocess
import sys

import numpy as np

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
sys.path.insert(0, os.path.join(ROOT, 'tools'))

D = 1024
SOURCES = ['Brainstorm', 'Ponder', "Dragon's Rage Channeler"]


def game_seed(batch_seed, k):
    """The seed zero-core's Python driver gave game k of a batch (zero/play.py: Slot)."""
    drv = random.Random(batch_seed)
    for _ in range(k + 1):
        s = drv.randrange(1 << 30)
    slot = random.Random(s)
    slot.randrange(1 << 30)   # its numpy generator
    slot.randrange(1 << 30)   # its search generator
    return slot.getrandbits(63)


def extract(run, out):
    fs = sorted(glob.glob(os.path.join(run, 'replays', '*.json.gz')))
    if not fs:
        sys.exit(f'no replays in {run}/replays')
    spec = None
    with open(os.path.join(out, 'games.txt'), 'w') as f:
        for p in fs:
            r = json.load(gzip.open(p))
            if spec is None:
                spec = r['spec']
            if r['spec'] != spec:
                sys.exit(f'{p}: another spec than the first replay\'s')
            acts = [fr['a'] for fr in r['frames']]
            f.write(f"{game_seed(r['seed'], r['game_index'])} {r['result'][0]} {r['result'][1]} "
                    f"{r['source']['iter']} " + ' '.join(map(str, acts)) + '\n')
    with open(os.path.join(out, 'spec.json'), 'w') as f:
        f.write(spec)
    print(f'{len(fs)} games from {run}/replays')


def dump(out):
    env = dict(os.environ, TOPPROBE_DIR=out)
    if 'CARGO_TARGET_DIR' not in env and os.name == 'nt':
        env['CARGO_TARGET_DIR'] = 'E:/cargo-target/mtg-zero/' + os.path.basename(ROOT)
    r = subprocess.run(['cargo', 'test', '--release', '-p', 'engine', '--test', 'az', 'top_known_probe_dump', '--',
                        '--ignored', '--nocapture'], cwd=ROOT, env=env, capture_output=True, text=True)
    line = [l for l in r.stdout.splitlines() if l.startswith('games replayed')]
    if r.returncode != 0 or not line:
        sys.exit(f'the dump failed (exit {r.returncode}):\n{r.stdout[-3000:]}\n{r.stderr[-3000:]}')
    print(line[0])


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--run', required=True, help='a run directory with replays/')
    ap.add_argument('--ckpt', required=True, help='the checkpoint whose frozen trunk the heads read')
    ap.add_argument('--dir', required=True, help='scratch directory (x.bin is ~150 KB a row)')
    ap.add_argument('--skip-dump', action='store_true', help='reuse the dir\'s dump')
    ap.add_argument('--seed', type=int, default=0)
    a = ap.parse_args(argv)
    os.makedirs(a.dir, exist_ok=True)
    if not a.skip_dump:
        extract(a.run, a.dir)
        dump(a.dir)

    analyse(a)


def analyse(a):
    import torch
    import belief_eval as BE
    import mtgz.az  # noqa: F401  (registers the game package)
    from mtgz.az import belief as B
    from zero.net import load
    lay = json.load(open(os.path.join(ROOT, 'mtgz', 'az', 'layout_v5.json')))
    g5 = next(g['offset'] for g in lay['globals'] if g['name'] == 'globals_v5')
    feats = lay.get('features') or 37620
    # g5::TOP_OWNER .. PLAYER_END of both per-player blocks (24 floats each; engine/src/az/encode.rs)
    cols = [g5 + 24 * side + k for side in (0, 1) for k in range(16, 22)]
    R = [l.rstrip('\n').split('\t') for l in open(os.path.join(a.dir, 'rows.tsv'), encoding='utf-8')]
    N = len(R)
    x = np.memmap(os.path.join(a.dir, 'x.bin'), dtype='<f4', mode='r', shape=(N, feats))
    gi = np.array([int(r[0]) for r in R])
    t = np.zeros((N, D), np.float32)
    prior = np.zeros((N, D), np.float64)
    for i, r in enumerate(R):
        for idt in r[5].split(','):
            t[i, int(idt)] += 1
        for kv in r[7].split(','):
            k, v = kv.split(':')
            prior[i, int(k)] = float(v)
    prior /= prior.sum(1, keepdims=True)
    src = np.array([r[6].split('|')[0] for r in R])
    drawn = np.array([r[8].split(',')[0] == '0' for r in R])

    E = [l.rstrip('\n').split('\t') for l in open(os.path.join(a.dir, 'episodes.tsv'), encoding='utf-8')]
    oc = collections.Counter(e[6] for e in E)
    what = {'0': 'drawn', '1': 'shuffled away', '2': 'left the library otherwise', '3': "at the game's end"}
    print(f'{len(E)} known top cards: ' + ', '.join(f'{what[k]} {v / len(E):.0%}' for k, v in sorted(oc.items())))
    print('taught by: ' + ', '.join(f'{k} {v}' for k, v in collections.Counter(e[4] for e in E).most_common(8)))

    torch.manual_seed(a.seed)
    model, _ = load(a.ckpt)
    z = BE.trunk_z(model, x)
    flags = torch.from_numpy(np.ascontiguousarray(x[:, cols]).astype(np.float32))
    groups = [src == s for s in SOURCES]
    groups.append(~np.any(np.stack(groups), 0))
    onehot = torch.from_numpy(np.stack(groups, 1).astype(np.float32))
    zz = torch.cat([z, flags], 1)
    zz2 = torch.cat([zz, onehot], 1)

    def fit(inp, base, tr, va):
        h = torch.nn.Sequential(torch.nn.Linear(inp.shape[1], 128), torch.nn.GELU(), torch.nn.Linear(128, D))
        torch.nn.init.zeros_(h[2].weight)
        torch.nn.init.zeros_(h[2].bias)
        tt, vv = torch.from_numpy(tr), torch.from_numpy(va)
        h, _ = BE.fit_head(h, inp[tt], t[tr], (inp[vv], t[va]), epochs=30, lr=1e-3, wd=0.01, patience=4,
                           base=base[tt], vbase=base[vv], log=lambda s: None)
        with torch.no_grad():
            return torch.softmax(h(inp) + base, 1).numpy().astype(np.float64)

    names = ['uniform', 'count-matching', 'count-matching per source', 'residual head', 'residual head + source']
    P = {n: np.zeros_like(prior) for n in names}
    fold = gi % 10
    for f in range(10):
        te, va = fold == f, fold == (f + 1) % 10
        tr = ~te & ~va
        P['uniform'][te] = prior[te]
        pcm = BE.apply_weights(prior, BE.count_matching(prior[tr | va], t[tr | va]))
        P['count-matching'][te] = pcm[te]
        psrc = np.zeros_like(prior)
        for g in groups:
            psrc[g] = BE.apply_weights(prior[g], BE.count_matching(prior[(tr | va) & g], t[(tr | va) & g]))
        P['count-matching per source'][te] = psrc[te]
        P['residual head'][te] = fit(zz, torch.from_numpy(np.log(np.maximum(pcm, 1e-12)).astype(np.float32)), tr, va)[te]
        P['residual head + source'][te] = fit(zz2, torch.from_numpy(np.log(np.maximum(psrc, 1e-12)).astype(np.float32)),
                                              tr, va)[te]

    def ce(p, k):
        n = t[k].sum(1)
        return -(t[k] / n[:, None] * np.log(np.maximum(p[k], 1e-9))).sum(1)

    rng = np.random.default_rng(a.seed)

    def se(vals, k):
        per = [vals[gi[k] == g] for g in np.unique(gi[k])]
        return float(np.std([np.concatenate([per[i] for i in rng.choice(len(per), len(per))]).mean()
                             for _ in range(300)]))

    print(f'\n{N} rows ({N / max(1, len(set(gi))):.0f} a game); 10 folds by game; cross-entropy per known top card (nats)')
    for label, k in [('all rows', np.ones(N, bool)), ('cards later drawn', drawn)] + [(s, src == s) for s in SOURCES]:
        b = ce(P['uniform'], k).mean()
        cells = []
        for n in names:
            c = ce(P[n], k)
            cells.append(f'{n} {c.mean():.3f}' + ('' if n == 'uniform' else f' ({c.mean() - b:+.3f})')
                         + (f' se {se(c, k):.3f}' if n in ('uniform', 'count-matching per source') else ''))
        print(f'  {label:24s} rows {k.sum():5d} | ' + ' | '.join(cells))
    nm = B.names()
    n = t.sum(1)
    for s, g in zip(SOURCES, groups):
        true = t[g].sum(0) / n[g].sum()
        exp = (n[g][:, None] * prior[g]).sum(0) / n[g].sum()
        top = [i for i in np.argsort(-(true - exp)) if true[i] > 0.02][:5]
        print(f'  {s}: most over uniform ' + ', '.join(f'{nm[i].split(" //")[0]} {true[i]:.0%} vs {exp[i]:.0%}' for i in top))


if __name__ == '__main__':
    main()
