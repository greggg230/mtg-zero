"""CPU evidence for self-play exploration (ENGINE.md 75.5): self-play games of a checkpoint at a run's
self-play settings with exploration on, and the same games (same driver seed: the same deals and
streams) off as a control; then what the episodes did and what followed. Read-only on the run: the
checkpoint and the run's config.json are read (copy them to scratch first), nothing is written
under runs/.

  python tools/explore_eval.py play --model CKPT --config RUN/config.json --games 96 --out on.json \\
      --explore all [--explore-games 1] [--explore-q-margin 0.1] [--explore-p ...] [--explore-window ...] \\
      [--explore-pick first|game] [--explore-min-visits 4] [--explore-max-actions 8]
      [--sims 64 --sims-fast 16] [--conc 48] [--threads 4] [--torch-threads 4] [--seed 1]
  python tools/explore_eval.py play ... --out off.json                     # no --explore: the control
  python tools/explore_eval.py report on.json off.json [on2.json off2.json ...] [--json report.json]
  python tools/explore_eval.py fingerprint x.json [...]       # the games' fingerprints alone (an A/B arm)

`play` writes every game's result row (with its move log and its `explore`), the samples' masked /
dropped counts and timing. `report`: per tag the opportunities, gate blocks, episodes, overridden
decisions, Q gaps, deviations to an unvisited option, the trigger turns; masked / dropped shares; the
games rebuilt from their move logs (the engine's spectator view, `log` on: what the replays store) and
fingerprinted with tools/delver_fingerprints.py - explore games against their control twins - and per
episode what followed: the twins identical up to the deviation (checked), the deviating seat's result
against its twin's (a paired estimate of the deviation's value, noisy), and per tag a follow-up
(fetch_crack: a Brainstorm by that seat while it held a fetch before its next crack; daze_tapout: the
opponent's Daze that turn; keep_one_lander: the mulligan's hand)."""
import argparse
import collections
import json
import math
import os
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
sys.path.insert(0, os.path.join(ROOT, 'tools'))

M64 = (1 << 64) - 1


def split64(x):
    z = (x + 0x9E3779B97F4A7C15) & M64
    z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & M64
    z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & M64
    return z ^ (z >> 31)


def rotl(x, k):
    return ((x << k) | (x >> (64 - k))) & M64


def first_draw(seed):
    """zero-core's `Rng::new(seed).next_u64()` (xoshiro256++ seeded by splitmix64)."""
    s, x = [], seed
    for _ in range(4):
        x = (x + 0x9E3779B97F4A7C15) & M64
        s.append(split64(x))
    if not any(s):
        s[0] = 1
    return (rotl((s[0] + s[3]) & M64, 23) + s[0]) & M64


def game_seed(driver_seed, gid):
    """The game seed `init_slot` gives game `gid` of a driver seeded `driver_seed` (not paired)."""
    return first_draw(split64(driver_seed ^ split64(gid + 1)))


# ------------------------------------------------------------------------------------------- play

def play(a):
    os.environ.setdefault('CUDA_VISIBLE_DEVICES', '-1')
    import numpy as np
    import torch
    import mtgz.az  # noqa: F401
    from zero import explore as X, games as G
    from zero.infer import NetEvaluator, drive
    from zero.net import load
    from zero.run import Run
    torch.set_num_threads(a.torch_threads)
    with open(a.config, encoding='utf-8') as f:
        cfg = json.load(f)
    for k in ('selfplay_remote', 'eval_remote', 'heldout_shards', 'elo_anchors'):
        cfg[k] = ''
    if a.sims:
        cfg['sims'] = a.sims
    if a.sims_fast:
        cfg['sims_fast'] = a.sims_fast
    cfg['selfplay_explore'] = a.explore or ''
    cfg['selfplay_explore_games'] = a.explore_games
    cfg['selfplay_explore_q_margin'] = a.explore_q_margin
    cfg['selfplay_explore_mask_z'] = a.explore_mask_z
    cfg['selfplay_explore_p'] = a.explore_p
    cfg['selfplay_explore_window'] = a.explore_window
    cfg['selfplay_explore_pick'] = a.explore_pick
    cfg['selfplay_explore_min_visits'] = a.explore_min_visits
    cfg['selfplay_explore_max_actions'] = a.explore_max_actions
    cfg['threads'] = a.threads
    scratch = os.path.join(os.path.dirname(os.path.abspath(a.out)), 'run_stub')
    R = Run(scratch, cfg)                          # (attributes only: nothing written)
    seat = R.nn_seat('m', cfg['sims'], selfplay=True)
    m, _ = load(a.model, 'cpu', game=cfg['game'])
    ev = NetEvaluator(m, 'cpu')
    kw = {'aux': True} if R.aux else {}
    if R.explore:
        kw['explore'] = R.explore
    drv = G.Driver(cfg['game'], [[seat, seat]] * a.games, spec=cfg['spec'], concurrency=a.conc, seed=a.seed, record=True,
                   threads=a.threads, groups=2, max_steps=cfg['max_steps'], move_log=1_000_000, **kw)
    rows, masked, emitted = [], 0, 0
    t0 = time.time()

    def take(_lb=None):
        nonlocal masked, emitted
        rows.extend(drv.take_results())
        s = drv.take_samples()
        if s is not None:
            emitted += len(s['player'])
            if 'masked' in s:
                masked += int(np.asarray(s['masked']).sum())
    drive(drv, {'m': ev}, on_batch=take)
    take()
    secs = time.time() - t0
    st = drv.stats()
    out = {'model': a.model, 'config': a.config, 'seed': a.seed, 'games': a.games, 'sims': cfg['sims'],
           'sims_fast': cfg['sims_fast'], 'explore': R.explore, 'seconds': round(secs, 1), 'leaves': st['leaves'],
           'leaves_per_s': round(st['leaves'] / max(secs, 1e-9), 1), 'samples': emitted, 'masked': masked,
           'spec': cfg['spec'], 'rows': rows}
    if R.explore:
        out['tally'] = X.summary(X.tally(rows, X.new_tally(R.info, R.explore)))
    with open(a.out, 'w', encoding='utf-8') as f:
        json.dump(out, f)
    print(json.dumps({k: v for k, v in out.items() if k not in ('rows', 'spec')}), flush=True)


# ------------------------------------------------------------------------------------------- replay

def rebuild(spec, driver_seed, row):
    """The game of a result row rebuilt from its move log: a zero-replay/1-like dict (one spectator
    state per step, before it, and the final one; frames {s, p, k, a}) the fingerprint tool reads."""
    import mtgz.az  # noqa: F401
    from zero import games as G
    sp = json.loads(spec)
    sp['log'] = True
    g = G.engine().Game('mtg', json.dumps(sp), 2, game_seed(driver_seed, int(row['gid'])))
    states, frames = [], []

    def step(a, kind):
        states.append(json.loads(g.export_state()))
        frames.append({'s': len(states) - 1, 'p': g.to_move, 'k': kind, 'a': int(a)})
        g.step(int(a))

    def auto():
        while g.to_move is not None and len(g.legal()) == 1:
            step(g.legal()[0], 'forced')
    for seat, a, legal in row['moves']:
        auto()
        if g.to_move != seat or list(g.legal()) != list(legal):
            raise SystemExit(f'game {row["gid"]}: the rebuilt game left its move log at frame {len(frames)}')
        step(a, 'nn')
    auto()
    if g.to_move is not None:
        raise SystemExit(f'game {row["gid"]}: the move log ended before the game')
    states.append(json.loads(g.export_state()))
    return {'format': 'zero-replay/1', 'states': states, 'frames': frames, 'result': row['result']}


def log_lines(rep):
    from delver_fingerprints import full_log
    return full_log(rep)


def frame_of_step(rep, step):
    """The frame index of the game's real move number `step` (the move log's index): the k-th 'nn'
    frame (auto-stepped single options are 'forced' frames in between)."""
    k = -1
    for i, fm in enumerate(rep['frames']):
        if fm['k'] == 'nn':
            k += 1
            if k == step:
                return i
    raise ValueError(step)


FETCH = {'Flooded Strand', 'Misty Rainforest', 'Scalding Tarn', 'Polluted Delta'}


def follow_up(tag, rep, ep):
    """What followed an episode in its game (from the deviation on), by tag."""
    import re
    p = int(ep['seat'])
    i0 = frame_of_step(rep, int(ep['step']))
    lines = [(i, ln) for i, ln in log_lines(rep) if i >= i0]
    st = rep['states']
    if tag == 'fetch_crack':
        # the seat's next fetch activation after the hold, and whether it cast Brainstorm before it
        # (while it held the fetch: a shuffle to follow)
        bs = crack = None
        for i, ln in lines:
            if bs is None and re.search(rf'P{p} casts Brainstorm', ln):
                bs = i
            if crack is None and re.search(rf'P{p} activates ({"|".join(FETCH)})', ln):
                crack = i
                break
        turns = None
        if crack is not None:
            turns = st[crack]['turn'] - st[i0]['turn'] if st[crack].get('turn') is not None else None
        had = any(c.get('name') == 'Brainstorm' for c in st[i0]['players'][p]['hand'])
        return {'cracked_later': crack is not None, 'crack_turns_later': turns, 'had_brainstorm': had,
                'brainstorm_then_crack': bs is not None and crack is not None and bs <= crack,
                'brainstorm_while_holding': bs is not None and (crack is None or bs <= crack),
                'had_bs_and_cast_it_holding': had and bs is not None and (crack is None or bs <= crack)}
    if tag == 'daze_tapout':
        t = st[i0].get('turn')
        dazed = any(re.search(rf'P{1 - p} casts Daze', ln) for i, ln in lines if st[i].get('turn') == t)
        cast = any(re.search(rf'P{p} casts ', ln) for i, ln in lines if st[i].get('turn') == t)
        return {'opp_dazed_that_turn': dazed, 'cast_later_that_turn': cast}
    return {}


# ------------------------------------------------------------------------------------------- report

def binom(k, n):
    if not n:
        return None
    r = k / n
    return {'rate': round(r, 4), 'n': n, 'se': round(math.sqrt(r * (1 - r) / n), 4)}


def fingerprints(reps):
    from delver_fingerprints import Band, METRICS, scan_game
    B = Band()
    for r in reps:
        scan_game(r, B)
    out = {'games': B.games}
    for label, num, den in METRICS:
        out[label] = binom(B.c[num], B.c[den])
    keeps = collections.defaultdict(lambda: [0, 0])
    for (lands, pos), v in B.mull.items():
        keeps['<=1' if lands <= 1 else '2+'][0] += v[0]
        keeps['<=1' if lands <= 1 else '2+'][1] += v[1]
    out['keep_one_lander'] = binom(*keeps['<=1'])
    out['fetch_crack_counts'] = {k: B.c[k] for k in ('fetch_now', 'fetch_later this turn', 'fetch_next turn',
                                                      'fetch_2+ turns', 'fetch_never')}
    return out


def pairs_of(files):
    """[(on, off)] of the play files given as ON OFF [ON OFF ...]: twins (same seed, spec, games)."""
    if len(files) < 2 or len(files) % 2:
        raise SystemExit('report: ON OFF [ON OFF ...]')
    out = []
    for fo, ff in zip(files[0::2], files[1::2]):
        on, off = (json.load(open(f, encoding='utf-8')) for f in (fo, ff))
        if on['seed'] != off['seed'] or on['spec'] != off['spec'] or not on.get('explore') or off.get('explore'):
            raise SystemExit(f'{fo} / {ff}: not twins (the same seed and spec, exploration on then off)')
        out.append((on, off))
    return out


def report(a):
    import mtgz.az  # noqa: F401
    from zero import explore as X, games as G
    info = G.info('mtg')
    names = [t[0] for t in info['explore_tags']]
    pairs = pairs_of(a.files)
    on0 = pairs[0][0]
    acc = None
    for on, _ in pairs:
        if on['explore'] != on0['explore']:
            raise SystemExit('the explore runs differ in their settings')
        acc = X.merge(acc, X.tally(on['rows'], X.new_tally(info, on['explore'])))
    t = X.summary(acc)
    out = {'pairs': len(pairs), 'games': sum(len(on['rows']) for on, _ in pairs), 'sims': on0['sims'],
           'sims_fast': on0['sims_fast'], 'explore': on0['explore'],
           'seconds': {'on': sum(on['seconds'] for on, _ in pairs), 'off': sum(off['seconds'] for _, off in pairs)},
           'leaves_per_s': {'on': [on['leaves_per_s'] for on, _ in pairs], 'off': [off['leaves_per_s'] for _, off in pairs]},
           'leaves_per_game': {'on': round(sum(on['leaves'] for on, _ in pairs) / max(1, sum(len(on['rows']) for on, _ in pairs)), 1),
                               'off': round(sum(off['leaves'] for _, off in pairs) / max(1, sum(len(off['rows']) for _, off in pairs)), 1)},
           'samples': {'on': sum(on['samples'] for on, _ in pairs), 'off': sum(off['samples'] for _, off in pairs)},
           'tally': t, 'masked_share': t['masked_share'], 'dropped_share': t['dropped_share']}
    per = collections.defaultdict(lambda: {'episodes': 0, 'prefix_ok': 0, 'seat_result': [], 'twin_result': [],
                                           'q_gap': [], 'alt_visits': [], 'overridden': [], 'turns': [],
                                           'ends': collections.Counter(), 'follow': [], 'twin_follow': []})
    reps_on, reps_off, by_tag_on, by_tag_off = [], [], collections.defaultdict(list), collections.defaultdict(list)
    todo = []
    for on, off in pairs:
        twin = {r['gid']: r for r in off['rows']}
        todo += [(on, off, r, twin.get(r['gid'])) for r in sorted(on['rows'], key=lambda r: r['gid'])]
    for on, off, r, tw in todo:
        if tw is None:
            continue
        ro, rf = rebuild(on['spec'], on['seed'], r), rebuild(off['spec'], off['seed'], tw)
        reps_on.append(ro)
        reps_off.append(rf)
        ep = (r.get('explore') or {}).get('episode')
        if ep is None:
            continue
        tag = names[int(ep['tag']) - 1]
        e = per[tag]
        e['episodes'] += 1
        k = int(ep['step'])
        # (the twins are one game until the first exploring search with min_visits, the deviation without)
        k0 = r['explore'].get('first_search')
        k0 = k if k0 is None else min(k, int(k0))
        e['prefix_ok'] += r['moves'][:k0] == tw['moves'][:k0] and r['moves'][k0][0] == tw['moves'][k0][0]
        p = int(ep['seat'])
        e['seat_result'].append(r['result'][p])
        e['twin_result'].append(tw['result'][p])
        e['q_gap'].append(float(ep['q_gap']))
        e['alt_visits'].append(int(ep['alt_visits']))
        e['overridden'].append(int(ep['overridden']))
        e['turns'].append(int(ep['turn']))
        e['ends'][ep['end']] += 1
        e['follow'].append(follow_up(tag, ro, ep))
        if r['moves'][:k] == tw['moves'][:k]:
            e['twin_follow'].append(follow_up(tag, rf, ep))
        by_tag_on[tag].append(ro)
        by_tag_off[tag].append(rf)
    tags = {}
    for tag in names:
        e = per.get(tag)
        tt = t['tags'].get(tag)
        if tt is None:
            continue
        d = {'drawn': tt.get('drawn', 0), 'opportunities': tt['opportunities'], 'blocked': tt['blocked'], 'episodes': tt['episodes'],
             'unsearched': tt.get('unsearched', 0), 'alt_under_min_visits': tt.get('alt_under', 0),
             'gate_block_rate': round(tt['blocked'] / max(1, tt['blocked'] + tt.get('unsearched', 0) + tt['episodes']), 4),
             'unsearched_rate': round(tt.get('unsearched', 0) / max(1, tt['blocked'] + tt.get('unsearched', 0) + tt['episodes']), 4),
             'overridden': tt['overridden'], 'q_gap_mean': tt['q_gap'], 'alt_unvisited': tt['alt_unvisited'],
             'ends': tt['ends'], 'turns': tt['turns']}
        if e:
            n = len(e['seat_result'])
            diff = [x - y for x, y in zip(e['seat_result'], e['twin_result'])]
            mu = sum(diff) / n
            sd = math.sqrt(sum((x - mu) ** 2 for x in diff) / max(1, n - 1)) if n > 1 else 0.0
            vis = [g for g, v in zip(e['q_gap'], e['alt_visits']) if v > 0]
            unv = [g for g, v in zip(e['q_gap'], e['alt_visits']) if v == 0]
            d.update(q_gap_visited=round(sum(vis) / len(vis), 4) if vis else None, q_gap_visited_max=round(max(vis), 4) if vis else None,
                     q_gap_unvisited=round(sum(unv) / len(unv), 4) if unv else None,
                     overridden_per_episode=round(sum(e['overridden']) / n, 2))
            d.update(prefix_identical=f'{e["prefix_ok"]}/{e["episodes"]}', seat_score=round(sum(e['seat_result']) / n, 4),
                     twin_score=round(sum(e['twin_result']) / n, 4), paired_diff=round(mu, 4),
                     paired_se=round(sd / math.sqrt(n), 4) if n > 1 else None,
                     alt_visits_median=sorted(e['alt_visits'])[n // 2])
            fk = collections.Counter()
            for f in e['follow']:
                for k2, v in f.items():
                    if isinstance(v, bool):
                        fk[k2] += v
            if fk or e['follow']:
                d['follow'] = {k2: binom(v, len(e['follow'])) for k2, v in fk.items()}
                tf = collections.Counter()
                for f in e['twin_follow']:
                    for k2, v in f.items():
                        if isinstance(v, bool):
                            tf[k2] += v
                d['twin_follow'] = {k2: binom(v, len(e['twin_follow'])) for k2, v in tf.items()}
                lat = [f['crack_turns_later'] for f in e['follow'] if f.get('crack_turns_later') is not None]
                if lat:
                    d['crack_turns_later'] = dict(sorted(collections.Counter(lat).items()))
            d['fingerprints_episode_games'] = fingerprints(by_tag_on[tag])
            d['fingerprints_twins'] = fingerprints(by_tag_off[tag])
        tags[tag] = d
    out['tags'] = tags
    out['fingerprints'] = {'explore': fingerprints(reps_on), 'control': fingerprints(reps_off)}
    s = json.dumps(out, indent=1)
    print(s)
    if a.json:
        with open(a.json, 'w', encoding='utf-8') as f:
            f.write(s)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    sub = ap.add_subparsers(dest='cmd', required=True)
    p = sub.add_parser('play')
    p.add_argument('--model', required=True)
    p.add_argument('--config', required=True, help="the run's config.json (a copy)")
    p.add_argument('--games', type=int, default=96)
    p.add_argument('--conc', type=int, default=48)
    p.add_argument('--threads', type=int, default=4)
    p.add_argument('--torch-threads', type=int, default=4)
    p.add_argument('--seed', type=int, default=1)
    p.add_argument('--sims', type=int, default=0, help='0 = the run\'s')
    p.add_argument('--sims-fast', type=int, default=0, help='0 = the run\'s')
    p.add_argument('--explore', default='', help="the run setting selfplay_explore ('' = off: the control)")
    p.add_argument('--explore-games', type=float, default=1.0)
    p.add_argument('--explore-q-margin', type=float, default=0.1)
    p.add_argument('--explore-mask-z', type=int, default=1)
    p.add_argument('--explore-p', default='')
    p.add_argument('--explore-window', default='')
    p.add_argument('--explore-pick', default='first', help="'first' (any tag's first trigger) | 'game' (a tag drawn per game)")
    p.add_argument('--explore-min-visits', type=int, default=4, help='searched alternatives (0 = off: unvisited ones gated on v_mix)')
    p.add_argument('--explore-max-actions', type=int, default=8)
    p.add_argument('--out', required=True)
    r = sub.add_parser('report', help='ON OFF [ON OFF ...]: twin play files, exploration on then off')
    r.add_argument('files', nargs='+')
    r.add_argument('--json', default=None)
    f = sub.add_parser('fingerprint', help="play files' games rebuilt and fingerprinted (one checkpoint's, an A/B arm's)")
    f.add_argument('files', nargs='+')
    a = ap.parse_args(argv)
    if a.cmd == 'play':
        play(a)
    elif a.cmd == 'report':
        report(a)
    else:
        reps = []
        for fn in a.files:
            d = json.load(open(fn, encoding='utf-8'))
            reps += [rebuild(d['spec'], d['seed'], r) for r in d['rows']]
        print(json.dumps(fingerprints(reps), indent=1))


if __name__ == '__main__':
    main()
