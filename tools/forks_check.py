"""Branched self-play games and archive starts on the Delver mirror, checked at the driver (docs/ENGINE.md
87; zero-core docs/az.md 24): one self-play driver of the run's seat (a fixed list of games, each branch played after its game) (a checkpoint's network, or
uniform priors / values without one), with branching and archive starts at the given shares, then the
same games with both off (twins by gid):

* the games (not branches, not archive starts) are their twins move for move: steps, scores, every record;
* branches: their share of the games, deviation moves drawn 1..N and played, the first deviation equal to
  the game's own move, the branch point's place in its game; each branch row's z its own outcome; no
  branch row is a state its game recorded (the branch point is not recorded twice), and its rows are at
  most its searched decisions after the deviations;
* archive starts: their share, each entry's kind, every entry's game starts (the driver checks them all at
  creation).

    python tools/forks_check.py --archive archive.json [--checkpoint iter_K.pt] [--games 400] [--sims 16]
        [--branch 0.2] [--moves 3] [--temp 2] [--archive-p 0.1] [--out report.json]
CPU only.
"""
import argparse
import collections
import json
import math
import os
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
import mtgz.az  # noqa: E402,F401  (the pinned zero-core on sys.path)
import numpy as np  # noqa: E402

from zero import forks as FK  # noqa: E402
from zero import games as G  # noqa: E402
from zero.infer import NetEvaluator, UniformEvaluator, drive  # noqa: E402
from zero.workqueue import split_games  # noqa: E402


def seat_of(cfg, sims):
    s = G.nn_seat('m', sims, selfplay=True, sims_fast=max(1, sims // 4), p_full=float(cfg.get('p_full', 0.25)),
                  sims_minor=cfg.get('sims_minor'), temp_moves=int(cfg.get('temp_moves', 60)), chance_k=0,
                  policy_target=cfg.get('policy_target', 'visits'))
    return s


def play(spec, seat, games, conc, seed, ev, branch=None, starts=None, threads=8):
    """A classic driver of `games` games (their branches play after them, in their slots) to the end."""
    kw = {}
    if branch:
        kw['branch'] = branch
    if starts:
        kw['starts'] = starts
    d = G.Driver('mtg', [[seat, seat]] * games, spec=spec, concurrency=conc, seed=seed, record=True, threads=threads,
                 groups=2, max_steps=20_000, **kw)
    out = []

    def take(lb=None):
        out.extend(split_games(d.take_results(), d.take_samples()))
    drive(d, {'m': ev}, on_batch=take)
    take()
    return out, d.stats()


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('--archive', required=True)
    ap.add_argument('--checkpoint', default=None)
    ap.add_argument('--games', type=int, default=400)
    ap.add_argument('--conc', type=int, default=64)
    ap.add_argument('--sims', type=int, default=16)
    ap.add_argument('--branch', type=float, default=0.2)
    ap.add_argument('--moves', type=int, default=3)
    ap.add_argument('--temp', type=float, default=2.0)
    ap.add_argument('--differ', type=int, default=0, help="1: the first deviation never the game's own move")
    ap.add_argument('--archive-p', type=float, default=0.1)
    ap.add_argument('--seed', type=int, default=11)
    ap.add_argument('--threads', type=int, default=8)
    ap.add_argument('--out', default=None)
    a = ap.parse_args(argv)
    import torch
    torch.set_num_threads(4)
    spec = G.spec_json(mtgz.az.PRESETS['delver-mirror']['spec'])
    cfg = dict(mtgz.az.PRESETS['delver-mirror'])
    if a.checkpoint:
        from zero.net import load
        model, _ = load(a.checkpoint, 'cpu', game='mtg')
        ev = NetEvaluator(model, 'cpu', workers=2)
    else:
        ev = UniformEvaluator()
    seat = seat_of(cfg, a.sims)
    A = FK.load_archive(a.archive, 'mtg', spec)
    t0 = time.time()
    on, st = play(spec, seat, a.games, a.conc, a.seed, ev, branch={'p': a.branch, 'moves': a.moves, 'temp': a.temp, 'differ': bool(a.differ)},
                     starts={'p': a.archive_p, 'specs': A['specs']}, threads=a.threads)
    t_on = time.time() - t0
    t0 = time.time()
    off, _ = play(spec, seat, a.games, a.conc, a.seed, ev, threads=a.threads)
    t_off = time.time() - t0
    twins = {r['gid']: (r, s) for r, s in off if not r.get('branch')}
    rep = {'games_quota': a.games, 'rows': len(on), 'seconds_on': round(t_on, 1), 'seconds_off': round(t_off, 1),
           'settings': {'branch': a.branch, 'moves': a.moves, 'temp': a.temp, 'differ': a.differ, 'archive_p': a.archive_p, 'sims': a.sims,
                        'checkpoint': a.checkpoint, 'archive': a.archive, 'archive_sha': A['sha'], 'archive_n': A['n']}}
    # 1. the games are their twins (games started from an archive entry have no plain twin)
    same = diff = 0
    for r, s in on:
        if r.get('branch') or r.get('start') is not None or r['gid'] not in twins:
            continue
        r0, s0 = twins[r['gid']]
        ok = (r['steps'], r['scores'], r['samples']) == (r0['steps'], r0['scores'], r0['samples'])
        if ok and s is not None:
            ok = all(np.array_equal(np.asarray(s[k]), np.asarray(s0[k])) for k in ('x', 'pi', 'z', 'q', 'opt_action'))
        same += ok
        diff += not ok
    rep['twins'] = {'identical': same, 'different': diff}
    # 2. branches
    main_rows = {r['gid']: (r, s) for r, s in on if not r.get('branch')}
    br = [(r, s) for r, s in on if r.get('branch')]
    nonbranch = len(main_rows)
    bad_z = dup = over = 0
    dup_in = []
    for r, s in br:
        b = r['branch']
        if s is not None:
            z = np.asarray(s['z'])[:, :2]
            exp = np.array([[r['result'][(p + j) % 2] for j in range(2)] for p in s['player']], np.float32)
            bad_z += int(not np.allclose(z, exp))
            par = main_rows.get(r['gid'])
            if par and par[1] is not None:
                px = {np.asarray(x).tobytes() for x in np.asarray(par[1]['x'])}
                nd = sum(np.asarray(x).tobytes() in px for x in np.asarray(s['x']))
                dup += nd
                if nd:
                    dup_in.append({'gid': r['gid'], 'rows': nd, 'same_first': b['same_first'], 'moves': b['moves'],
                                   'played': b['played']})
        over += int(r['samples'] > r['decisions'] - b['decision'] - b['played'])
    k = len(br)
    spawned = st['branches']
    rep['branch'] = {
        'branches_finished': k, 'branches_started': spawned, 'nonbranch_games': nonbranch,
        'rate_started_per_game': round(spawned / max(1, nonbranch), 4),
        'expected_rate': a.branch, 'se': round(math.sqrt(a.branch * (1 - a.branch) / max(1, nonbranch)), 4),
        'moves_drawn': dict(sorted(collections.Counter(r['branch']['moves'] for r, _ in br).items())),
        'played_mean': round(sum(r['branch']['played'] for r, _ in br) / max(1, k), 3),
        'same_first': round(sum(r['branch']['same_first'] for r, _ in br) / max(1, k), 4),
        'at_frac_mean': round(sum((r['branch']['at'] + 0.5) / r['branch']['of'] for r, _ in br) / max(1, k), 4),
        'rows_per_branch': round(sum(r['samples'] for r, _ in br) / max(1, k), 1),
        'rows_per_game': round(sum(r['samples'] for r, _ in main_rows.values()) / max(1, nonbranch), 1),
        'decisions_per_branch': round(sum(r['decisions'] - r['branch']['decision'] for r, _ in br) / max(1, k), 1),
        'decisions_per_game': round(sum(r['decisions'] for r, _ in main_rows.values()) / max(1, nonbranch), 1),
        'z_not_own_outcome': bad_z, 'rows_duplicating_a_game_row': dup, 'dup_branches': dup_in, 'rows_beyond_post_deviation_decisions': over,
        'dev_moves_stat': st['dev_moves'], 'dev_moves_rows': sum(r['branch']['played'] for r, _ in br)}
    # 3. archive starts
    starts = [r for r, _ in on if r.get('start') is not None]
    kinds = collections.Counter(A['kinds'][r['start']] for r in starts)
    rep['archive'] = {'starts': len(starts), 'nonbranch_games': nonbranch, 'rate': round(len(starts) / max(1, nonbranch), 4),
                      'expected_rate': a.archive_p, 'se': round(math.sqrt(a.archive_p * (1 - a.archive_p) / max(1, nonbranch)), 4),
                      'by_kind': dict(kinds), 'archive_kinds': dict(collections.Counter(A['kinds'])),
                      'distinct_entries': len({r['start'] for r in starts}),
                      'rows_per_start': round(sum(r['samples'] for r in starts) / max(1, len(starts)), 1),
                      'turn_at_end_mean': round(sum(r['rounds'] for r in starts) / max(1, len(starts)), 1),
                      'stat_starts': st['starts']}
    print(json.dumps(rep, indent=1))
    if a.out:
        with open(a.out, 'w', encoding='utf-8', newline='\n') as f:
            json.dump(rep, f, indent=1)
    return rep


if __name__ == '__main__':
    main()
