"""`python -m mtgz.az.keep_h2h`: the sideboarding yardstick of a constructed best-of-three (docs/ENGINE.md 130) - a
head-to-head where side B never sideboards (it keeps its 60 at every sideboarding: `PASS` at its first sideboarding
decision) and side A searches its sideboarding as it searches everything else.

  python -m mtgz.az.keep_h2h runs/<run>/models/iter_NNNN.pt [B.pt] --games 400 --sims 64 --device cuda
      [--spec SPEC_JSON] [--seed 0 --threads 4 --groups 2 --conc N --max-steps 60000]

B defaults to A: one network, sideboarding against keeping. The spec defaults to the delver-mirror-bo3 preset's
(post-sideboard games only, one episode per game, the pinned 75s: `mtgz.az.SPEC_DELVER_BO3`); any constructed
best-of-three spec of `mtg` works. B's seat is the spec's `"keep_deck"` (an engine setting: its seat's only
sideboarding option is `PASS`; in A's worlds B sideboards as any player, and A deals B's hidden 60 by the
constructed prior as always - nothing A knows or deals changes, ENGINE.md 130's twin test). zero.h2h alternates the
seats in one driver with one spec, so the seat that keeps would alternate between the sides; here the games are
two drivers, A in seat 0 (`"keep_deck": [1]`) and A in seat 1 (`"keep_deck": [0]`), half the games each, with the
same driver seed: game k of each half has the same seed (the same drawn game 1 and deals, the sides' seats
swapped), the pairs of zero.h2h's `--paired`. Both sides search with their checkpoint's run's evaluation settings
(zero.h2h's `run_rule`: eval_select, belief tilts, root search, ...), no noise, no temperature.

Prints zero.h2h's summary as one JSON line: A's match score (a match counts its drawn game 1 with
`"start_game": 2`), its paired se, Elo, and `per_game` - A's games won (games 2 and 3): the measure of what
sideboarding is worth - plus `keeps`: 'b'.
"""
import argparse
import json
import time

from mtgz.az import decklists  # noqa: F401  (the package's registration first)
import mtgz.az as A
from zero import games as G
from zero import h2h as H
from zero.infer import drive


def half_specs(spec):
    """The two halves' specs: A in seat 0 with seat 1 keeping, A in seat 1 with seat 0 keeping. Refused unless
    a constructed best-of-three spec without its own `keep_deck`."""
    s = json.loads(spec)
    if not (s.get('constructed') and s.get('format') == 'bo3'):
        raise ValueError('keep_h2h: a constructed best-of-three spec ("constructed": true, "format": "bo3"; docs/ENGINE.md 130)')
    if 'keep_deck' in s:
        raise ValueError('keep_h2h: the spec names keep_deck itself (the tool sets it per half)')
    return [json.dumps(dict(s, keep_deck=[1 - a_seat]), separators=(',', ':')) for a_seat in (0, 1)]


def keep_match(a, b=None, games=400, sims=64, chance_k=2, c_puct=1.0, seed=0, threads=4, groups=2, device='cpu',
               spec=None, conc=None, max_steps=60_000, seat_a=None, seat_b=None):
    """Checkpoint `a` (sideboards) against `b` (keeps its deck; None: `a`), `games` matches (even). -> dict
    (zero.h2h's summary, paired: A in seat `gid % 2`, games 2k and 2k + 1 a pair). `seat_a` / `seat_b`: a side's
    move-rule seat settings (None: its checkpoint's run's evaluation settings, `run_rule`)."""
    if games < 2 or games % 2:
        raise ValueError(f'keep_h2h: an even number of games, at least 2 ({games})')
    b = b or a
    spec = A.SPEC_DELVER_BO3 if spec is None else spec
    specs = half_specs(spec)
    ma, mb, game = H._load_pair(a, b, device, None)
    if game != 'mtg':
        raise ValueError(f'keep_h2h: game mtg (constructed best-of-three), not {game}')
    sa, sb = H._seats(a, b, ma, mb, game, sims, c_puct, chance_k, seat_a, seat_b, None, None)
    evs = H._evaluators(ma, mb, device, None)
    t0 = time.time()
    rows = []
    for k, seats in enumerate(([sa, sb], [sb, sa])):
        n = games // 2
        drv = G.Driver(game, [seats] * n, spec=specs[k], concurrency=conc or min(n, 1024), seed=seed, threads=threads,
                       groups=groups, max_steps=max_steps)
        drive(drv, evs)
        for r in drv.take_results():
            r = dict(r)
            # (A's seat is gid % 2, as zero.h2h's rows: half k's game g is game 2g + k, the pair of game g)
            r['gid'] = 2 * r['gid'] + k
            rows.append(r)
    out = dict({'a': a, 'b': b, 'keeps': 'b', 'game': game, 'sims': sims, 'chance_k': chance_k},
               **H.summary(rows, paired=True))
    out.update(seconds=round(time.time() - t0, 1), select_a=H._tag(sa), select_b=H._tag(sb), spec=json.loads(spec))
    return out


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0], formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('a', help='the checkpoint that sideboards (searches every decision)')
    ap.add_argument('b', nargs='?', default=None, help='the checkpoint that keeps its deck (default: a)')
    ap.add_argument('--games', type=int, default=400, help='matches (even; half with A in each seat)')
    ap.add_argument('--sims', type=int, default=64)
    ap.add_argument('--chance-k', type=int, default=2)
    ap.add_argument('--c-puct', type=float, default=1.0)
    ap.add_argument('--seed', type=int, default=0)
    ap.add_argument('--threads', type=int, default=4)
    ap.add_argument('--groups', type=int, default=2)
    ap.add_argument('--device', default='cpu', help="network device ('cuda' works; the GPU is shared)")
    ap.add_argument('--spec', default=None, help='a constructed best-of-three spec (default: the delver-mirror-bo3 preset\'s)')
    ap.add_argument('--conc', type=int, default=None)
    ap.add_argument('--max-steps', type=int, default=60_000, help='a match\'s step cap (the preset\'s: up to three games)')
    ap.add_argument('--torch-threads', type=int, default=1)
    a = ap.parse_args(argv)
    import torch
    torch.set_num_threads(max(1, a.torch_threads))
    try:
        out = keep_match(a.a, a.b, a.games, a.sims, a.chance_k, a.c_puct, a.seed, a.threads, a.groups, a.device,
                         a.spec, a.conc, a.max_steps)
    except ValueError as e:
        ap.error(str(e))
    print(json.dumps(out), flush=True)


if __name__ == '__main__':
    main()
