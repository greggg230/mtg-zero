"""The match-noise study (docs/ENGINE.md 50.1): how noisy is one team-draft event's outcome - match
luck against real differences between the two teams' decks - and how many matches per event pay?

Drafted 4v4 events (8 seats, 3 packs of 15, decks built, 12 best-of-three matches: 3 per player,
3 of the 4 Latin-square rotations) in two regimes: every seat a `random` drafter (random picks, a
random legal 40-card deck: close to an untrained draft network) or a `consensus` drafter (M1 pick
strength and the heuristic deckbuilder: closer to a trained one). Each event's 12 matches are
played K times with different match seeds (the shuffles) and search seeds, as a co-training run
plays them: both seats the play network with mtg-4's self-play search (64 / 16 simulations, p_full
0.25, minor decisions at 4, temperature for 60 decisions, root noise), and as a contrast by the
heuristic bot on both sides. Then per regime: the variance of the team match difference, the
margin, the team result and each player's own record across replays of one event (luck, sigma^2)
and across events (sigma^2 + tau^2), the per-match spread, the reliability of a 12-match label and
what 16 or 24 matches would give, and the noise-optimal matches per event m* = sqrt(d sigma^2 /
tau^2) (d: the draft's cost in matches).

    python tools/match_noise.py all --out ~/mtgz-noise --model ~/mtgz-noise/mtg5_iter_0014.pt   # the study
    python tools/match_noise.py gen --events 40                # the events (DIR/events/<regime>.jsonl)
    python tools/match_noise.py play --player heuristic --replays 16
    python tools/match_noise.py play --player net --model M --replays 4
    python tools/match_noise.py analyze                        # DIR/summary.json, DIR/summary.md
    python tools/match_noise.py bench --model M --matches 96 --conc 256 --threads 12 --groups 2 --infer-workers 3
    python tools/match_noise.py repro --tag random/30/4/6 --player heuristic   # one match replay alone (a failure's repro)

Resumable: every finished match is a line of DIR/results/<player>.jsonl (regime, event, match,
replay, result ...); `play` skips what is there. The play loop rewrites DIR/progress.json each
minute and DIR/summary.* every 10 minutes, so a study still running has partial results (replays
are played replay by replay: two complete replays already give every estimate). It backs off
while a remote evaluation holds its lock on the Mac (tools/mac_backoff.py: it stops asking for
leaf batches, so the search threads idle) and runs at nice 19. The engine is this checkout's
mtgz/_engine (bash tools/build_py.sh).
"""
import argparse
import json
import math
import os
import random
import re
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
sys.path.insert(0, os.path.join(ROOT, 'tools'))

import mac_backoff  # noqa: E402

BIG = 1 << 62
MASK64 = (1 << 64) - 1
REGIMES = ('random', 'consensus')


def splitmix(x):
    x = (x + 0x9E3779B97F4A7C15) & MASK64
    z = x
    z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK64
    z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK64
    return z ^ (z >> 31)


def replay_seed(seed, k):
    """Replay k of a match: replay 0 is the event's own match seed."""
    return seed if k == 0 else splitmix(seed ^ ((k * 0x9E3779B97F4A7C15) & MASK64))


def log(out, msg):
    line = time.strftime('%H:%M:%S ') + msg
    print(line, flush=True)
    with open(os.path.join(out, 'log.txt'), 'a', encoding='utf-8') as f:
        f.write(time.strftime('%Y-%m-%d ') + line + '\n')


def write_json(path, obj):
    with open(path + '.tmp', 'w', encoding='utf-8') as f:
        json.dump(obj, f, indent=1)
    os.replace(path + '.tmp', path)


def read_jsonl(path):
    if not os.path.exists(path):
        return []
    out = []
    with open(path, encoding='utf-8') as f:
        for line in f:
            line = line.strip()
            if line:
                try:
                    out.append(json.loads(line))
                except ValueError:
                    pass                           # a torn last line (killed mid-write)
    return out


def setup_engine(torch_threads=None):
    if torch_threads:
        os.environ.setdefault('OMP_NUM_THREADS', str(torch_threads))
    import mtgz.az  # noqa: F401  (registers the game package)
    from zero import games as G
    from zero.run import DEFAULTS, PRESETS
    return G, dict(DEFAULTS, **PRESETS['mtg-4'])


def nn_eval_seat(G, c):
    """mtg-4's evaluation seat (zero.run.Run.nn_seat at eval_sims): no temperature, no root noise."""
    return G.nn_seat('m', c['eval_sims'], c['c_puct'], chance_k=c['chance_k'], margin_w=c['margin_w'],
                     redet=bool(c['redet']))


def nn_selfplay_seat(G, c, greedy=False):
    """mtg-4's self-play seat (zero.run.Run.nn_seat(selfplay=True)): what co-training's resolver plays.
    `greedy`: the same search (simulations, playout caps, minor decisions) without temperature or root
    noise (the study's contrast 'net-greedy': how much of the luck is the self-play's own randomness)."""
    eps, temp = (0.0, 0) if greedy else (c['eps'], c['temp_moves'])
    return G.nn_seat('m', c['sims'], c['c_puct'], True, c['alpha'], eps, temp, c['sims_fast'],
                     c['p_full'], c['chance_k'], c['margin_w'], bool(c['redet']), c.get('policy_target', 'visits'),
                     c.get('cq_c_visit', 50.0), c.get('cq_c_scale', 1.0), sims_minor=c.get('sims_minor', 0) or None)


def net_seat(G, c, player):
    return {'net': lambda: nn_selfplay_seat(G, c), 'net-greedy': lambda: nn_selfplay_seat(G, c, greedy=True),
            'net-eval': lambda: nn_eval_seat(G, c)}[player]()


NETS = ('net', 'net-greedy', 'net-eval')


class Backoff:
    """Cooperative back-off for a driver loop: called on every batch, it blocks (the search threads
    then idle for want of evaluations) while a remote evaluation holds its lock."""

    def __init__(self, out=None, every=2.0):
        self.out, self.every, self.t, self.paused = out, every, 0.0, 0.0

    def __call__(self):
        now = time.time()
        if now - self.t < self.every:
            return
        self.t = now
        why = mac_backoff.held()
        if not why:
            return
        if self.out:
            log(self.out, f'backing off: remote evaluation ({why})')
        while mac_backoff.held():
            time.sleep(5)
        took = time.time() - now
        self.paused += took
        if self.out:
            log(self.out, f'continuing after {took:.0f} s')


# ---------------------------------------------------------------------------- events

def gen(out, regimes, events, seed, threads):
    G, _ = setup_engine()
    from zero.infer import drive
    os.makedirs(os.path.join(out, 'events'), exist_ok=True)
    for ri, regime in enumerate(regimes):
        path = os.path.join(out, 'events', f'{regime}.jsonl')
        have = read_jsonl(path)
        if len(have) >= events:
            log(out, f'events {regime}: {len(have)} already')
            continue
        dseed = seed * 1000 + ri
        bot = G.seat_dict(regime)
        drv = G.Driver('team_draft', [[bot] * 8] * events, spec='{}', concurrency=events, seed=dseed, threads=threads,
                       groups=1, record=False, max_steps=20_000)
        t0 = time.time()
        drive(drv, {})
        parked = sorted(drv.take_deferred())
        if len(parked) != events:
            raise SystemExit(f'events {regime}: {len(parked)} parked of {events}')
        with open(path + '.tmp', 'w', encoding='utf-8') as f:
            for i, (gid, payload) in enumerate(parked):
                f.write(json.dumps({'regime': regime, 'event': i, 'gid': gid, 'driver_seed': dseed,
                                    'payload': json.loads(payload)}) + '\n')
        os.replace(path + '.tmp', path)
        ms = parked and json.loads(parked[0][1])['matches']
        log(out, f'events {regime}: {events} drafted and built in {time.time() - t0:.1f} s (driver seed {dseed}; '
                 f'{len(ms)} matches each)')


def load_events(out, regimes):
    ev = {}
    for regime in regimes:
        rows = read_jsonl(os.path.join(out, 'events', f'{regime}.jsonl'))
        if not rows:
            raise SystemExit(f'no events for {regime} in {out}/events (run gen first)')
        for r in rows:
            ev[(regime, r['event'])] = r['payload']
    return ev


def tasks_of(events, regimes, replays, max_events=0):
    """(tag, spec) per match replay, replay by replay (partial data is whole replays); `max_events`:
    only each regime's first events."""
    from mtgz.az import drafted
    n = max(e for (_, e) in events) + 1
    if max_events:
        n = min(n, max_events)
    for k in range(replays):
        for regime in regimes:
            for e in range(n):
                p = events.get((regime, e))
                if p is None:
                    continue
                for j, m in enumerate(p['matches']):
                    spec = drafted.match_spec(p['event'], m)
                    spec['seed'] = replay_seed(int(m['seed']), k)
                    yield f'{regime}/{e}/{j}/{k}', spec


# ---------------------------------------------------------------------------- playing

def play(out, player, regimes, replays, model, conc, threads, groups, infer_workers, torch_threads, seed,
         analyze_every=600, max_lives=200, max_events=0):
    """Play every match replay not in DIR/results/<player>.jsonl yet. A search worker's panic fails
    zero-core's driver (an engine bug: today the payment panics branch m6-payfix fixes): the match it
    names is recorded in DIR/results/<player>.failed.jsonl and left out, and a new driver plays the
    rest (the matches in flight then start again: each replay is one independent sample anyway)."""
    G, c = setup_engine(torch_threads)
    from zero.infer import NetEvaluator, drive
    events = load_events(out, regimes)
    os.makedirs(os.path.join(out, 'results'), exist_ok=True)
    rpath = os.path.join(out, 'results', f'{player}.jsonl')
    fpath = os.path.join(out, 'results', f'{player}.failed.jsonl')
    tasks = list(tasks_of(events, regimes, replays, max_events))
    if player in NETS:
        import torch
        from zero.net import load
        torch.set_num_threads(int(torch_threads))
        m, _ = load(model, 'cpu')
        evs = {'m': NetEvaluator(m, 'cpu', workers=infer_workers)}
        seat = net_seat(G, c, player)
    else:
        evs = {}
        seat = G.seat_dict(player)
    back = Backoff(out)
    done0 = {(r['regime'], r['event'], r['match'], r['replay']) for r in read_jsonl(rpath)}
    total = len(tasks)
    state = {'n': 0, 't0': time.time(), 'tlog': time.time(), 'tan': time.time(), 'decisions': 0, 'leaves': 0,
             'lives': 0, 'failed': 0}
    log(out, f'play {player}: {len(done0)} match replays done of {total} (replays {replays}, regimes {list(regimes)}, '
             f'{len(events)} events)')
    cur = {'drv': None, 'st0': None}
    f = open(rpath, 'a', encoding='utf-8')

    def leaves_now():
        d = cur['drv']
        try:
            return state['leaves'] + (d.stats()['leaves'] - cur['st0']['leaves'] if d is not None else 0)
        except Exception:  # noqa: BLE001 - a failed driver
            return state['leaves']

    def collect():
        rows = cur['drv'].take_results()
        for r in rows:
            regime, e, j, k = _tag(r['tag'])
            pl = events[(regime, e)]['matches'][j]
            f.write(json.dumps({'regime': regime, 'event': e, 'match': j, 'replay': k, 'round': pl['round'],
                                'seats': pl['seats'], 'result': r['result'][:2], 'scores': r['scores'][:2],
                                'rounds': r['rounds'], 'over': r['over'], 'decisions': r['decisions'],
                                'steps': r['steps'], 't': round(time.time(), 1)}) + '\n')
            state['decisions'] += r['decisions']
        if rows:
            f.flush()
        state['n'] += len(rows)
        now = time.time()
        if now - state['tlog'] > 60:
            state['tlog'] = now
            progress(final=False)
        if analyze_every and now - state['tan'] > analyze_every:
            state['tan'] = now
            try:
                analyze(out, quiet=True)
            except Exception as ex:  # noqa: BLE001 - a partial analysis must not stop the play
                log(out, f'analysis failed: {ex!r}')

    def progress(final):
        active = max(1e-9, time.time() - state['t0'] - back.paused)
        leaves = leaves_now()
        ev = evs.get('m')
        d = cur['drv']
        left = total - len(done0) - state['n'] - state['failed']
        try:
            inflight, queued = d.in_flight()['games'], d.queued
        except Exception:  # noqa: BLE001
            inflight, queued = None, None
        pr = {'player': player, 'done': len(done0) + state['n'], 'total': total, 'failed': state['failed'],
              'this_session': state['n'], 'active_s': round(active), 'paused_s': round(back.paused),
              'matches_per_h': round(state['n'] / active * 3600, 1), 'leaves': leaves,
              'leaves_per_s': round(leaves / active), 'leaves_per_match': round(leaves / max(1, state['n'])),
              'decisions_per_match': round(state['decisions'] / max(1, state['n']), 1),
              'avg_batch': round(ev.leaves / max(1, ev.calls), 1) if ev else None, 'drivers': state['lives'],
              'in_flight': inflight, 'queued': queued,
              'eta_h': round(left / max(1e-9, state['n'] / active) / 3600, 2) if state['n'] else None,
              'settings': {'conc': conc, 'threads': threads, 'groups': groups, 'infer_workers': infer_workers,
                           'torch_threads': torch_threads}, 'time': time.time(), 'final': final}
        write_json(os.path.join(out, f'progress_{player}.json'), pr)
        log(out, f'{player}: {pr["done"]}/{pr["total"]} match replays ({pr["failed"]} failed), {pr["matches_per_h"]} '
                 f'per h, {pr["leaves_per_s"]} leaves/s (batch {pr["avg_batch"]}), {pr["leaves_per_match"]} leaves and '
                 f'{pr["decisions_per_match"]} decisions per match, paused {pr["paused_s"]} s, eta {pr["eta_h"]} h')
        return pr

    def on_batch(_lb=None):
        back()
        collect()

    try:
        while True:
            done = {(r['regime'], r['event'], r['match'], r['replay']) for r in read_jsonl(rpath)}
            failed = {r['tag'] for r in read_jsonl(fpath)}
            todo = [(t, s) for t, s in tasks if tuple(_tag(t)) not in done and t not in failed]
            if not todo:
                break
            if state['lives'] >= max_lives:
                raise SystemExit(f'play {player}: {max_lives} drivers failed; stopping')
            state['lives'] += 1
            rseed = seed * 7919 + len(done) + 1009 * state['lives'] + (104729 if player.startswith('net') else 0)
            drv = G.Driver('mtg', [[seat, seat]], spec=c['spec'], concurrency=conc, seed=rseed, threads=threads,
                           groups=groups, record=False, max_steps=c['max_steps'], rolling=True, queue_only=True)
            drv.resume(BIG, 0)
            for tag, spec in todo:
                drv.enqueue(tag, json.dumps(spec, separators=(',', ':')))
            cur.update(drv=drv, st0=drv.stats())
            try:
                while True:
                    drive(drv, evs, on_batch=on_batch)
                    collect()
                    if not drv.queued and not drv.in_flight()['games']:
                        break
                    back()
                    time.sleep(0.2)                 # bots only: drive returns once the queue is played
            except RuntimeError as e:
                msg = str(e)
                mt = re.search(r'queued setup "([^"]+)"', msg)
                try:
                    collect()                       # rows finished before the failure
                except Exception:  # noqa: BLE001
                    pass
                state['leaves'] = leaves_now()
                cur.update(drv=None)
                del drv
                if not mt:
                    raise
                state['failed'] += 1
                with open(fpath, 'a', encoding='utf-8') as g:
                    g.write(json.dumps({'tag': mt.group(1), 'error': msg[:4000], 't': time.time()}) + '\n')
                log(out, f'{player}: match {mt.group(1)} failed (left out; a new driver plays the rest): {msg[:300]}')
                continue
            state['leaves'] = leaves_now()
            cur['st0'] = drv.stats()                # (counted: the delta is now 0)
            break
    finally:
        f.close()
    pr = progress(final=True)
    if cur['drv'] is not None:
        cur['drv'].end_phase()
    return pr


def _tag(t):
    regime, e, j, k = t.split('/')
    return regime, int(e), int(j), int(k)


def repro(out, tag, player, model, seed):
    """Play one match replay (`regime/event/match/replay`) alone: its `mtg` spec (printed) with the
    heuristic on both sides (deterministic: a failing match fails again) or the network."""
    G, c = setup_engine()
    from zero.infer import NetEvaluator, drive
    regime, e, j, k = _tag(tag)
    events = load_events(out, [regime])
    m = events[(regime, e)]['matches'][j]
    from mtgz.az import drafted
    spec = drafted.match_spec(events[(regime, e)]['event'], m)
    spec['seed'] = replay_seed(int(m['seed']), k)
    js = json.dumps(spec, separators=(',', ':'))
    print('SPEC ' + js, flush=True)
    if player in NETS:
        from zero.net import load
        evs = {'m': NetEvaluator(load(model, 'cpu')[0], 'cpu')}
        seat = net_seat(G, c, player)
    else:
        evs, seat = {}, G.seat_dict(player)
    drv = G.Driver('mtg', [[seat, seat]], spec=c['spec'], concurrency=1, seed=seed, threads=1, groups=1,
                   record=False, max_steps=c['max_steps'], rolling=True, queue_only=True)
    drv.resume(BIG, 0)
    drv.enqueue(tag, js)
    drive(drv, evs)
    print('RESULT ' + json.dumps(drv.take_results()), flush=True)


# ---------------------------------------------------------------------------- the throughput bench

def bench(out, model, matches, conc, threads, groups, infer_workers, torch_threads, seed, record=True, seconds=0):
    """mtg-4 self-play (generated pools, best-of-three) with the network: leaves/s and matches/h of a
    rolling phase of `matches` finished matches, as zero.run's self-play measures them (`seconds`: the
    phase ends after that long instead, the matches in flight unfinished: leaves/s only)."""
    G, c = setup_engine(torch_threads)
    import torch
    from zero.infer import NetEvaluator, drive
    from zero.net import load
    torch.set_num_threads(int(torch_threads))
    m, _ = load(model, 'cpu')
    ev = NetEvaluator(m, 'cpu', workers=infer_workers)
    seat = nn_selfplay_seat(G, c)
    drv = G.Driver('mtg', [[seat, seat]], spec=c['spec'], concurrency=conc, seed=seed, threads=threads, groups=groups,
                   record=record, max_steps=c['max_steps'], rolling=True)
    back = Backoff(out)
    samples = [0]

    ended = [False]

    def on_batch(_lb):
        back()
        s = drv.take_samples()
        if s is not None:
            samples[0] += len(s['player'])
        if seconds and not ended[0] and time.time() - t0[1] - back.paused > seconds:
            ended[0] = True
            drv.end_phase()

    drv.resume(matches if not seconds else BIG, 0)
    st0 = drv.stats()
    t0 = time.process_time(), time.time()
    drive(drv, {'m': ev}, on_batch=on_batch)
    wall = time.time() - t0[1] - back.paused
    cpu = time.process_time() - t0[0]
    rows = drv.take_results()
    st = drv.stats()
    leaves = st['leaves'] - st0['leaves']
    r = {'matches': len(rows), 'seconds': round(wall, 1), 'paused_s': round(back.paused), 'leaves': leaves,
         'leaves_per_s': round(leaves / wall), 'matches_per_h': round(len(rows) / wall * 3600, 1),
         'leaves_per_match': round(leaves / max(1, len(rows))), 'avg_batch': round(ev.leaves / max(1, ev.calls), 1),
         'infer_s': round(ev.seconds, 1), 'cpu_cores_busy': round(cpu / wall, 1), 'samples': samples[0],
         'decisions_per_match': round(sum(x['decisions'] for x in rows) / max(1, len(rows)), 1),
         'rounds_per_match': round(sum(x['rounds'] for x in rows) / max(1, len(rows)), 2),
         'settings': {'conc': conc, 'threads': threads, 'groups': groups, 'infer_workers': infer_workers,
                      'torch_threads': torch_threads, 'record': record, 'seconds': seconds}}
    drv.end_phase()
    return r


# ---------------------------------------------------------------------------- the analysis

def _var(xs):
    n = len(xs)
    if n < 2:
        return float('nan')
    m = sum(xs) / n
    return sum((x - m) ** 2 for x in xs) / (n - 1)


def _mean(xs):
    return sum(xs) / len(xs) if xs else float('nan')


def components(groups_):
    """One-way random effects (unbalanced allowed): `groups_` = list of lists of replicate values.
    -> (within variance sigma^2 (pooled), between variance tau^2 of the group means' true values,
    mean replicates). tau^2 = Var(group means) - sigma^2 * mean(1/K)."""
    gs = [g for g in groups_ if len(g) >= 1]
    ss, df = 0.0, 0
    for g in gs:
        if len(g) >= 2:
            m = sum(g) / len(g)
            ss += sum((x - m) ** 2 for x in g)
            df += len(g) - 1
    s2 = ss / df if df else float('nan')
    means = [sum(g) / len(g) for g in gs]
    inv_k = _mean([1.0 / len(g) for g in gs])
    t2 = _var(means) - s2 * inv_k
    return s2, t2, _mean([len(g) for g in gs])


def _table(rows, player, regime):
    """{(event, replay): {match: x}} with x = A's result: 1 win, -1 loss, 0 draw."""
    t = {}
    for r in rows:
        if r['regime'] != regime:
            continue
        a, b = r['result']
        x = 1 if a > b else -1 if b > a else 0
        t.setdefault((r['event'], r['replay']), {})[r['match']] = (x, r)
    return t


def _event_labels(t, nmatch):
    """Per complete (event, replay): D (A's match wins - B's), the margin D / n, the team result,
    and per seat its own record (its match wins - losses over its matches)."""
    out = {}
    for (e, k), ms in t.items():
        if len(ms) != nmatch:
            continue
        d = sum(x for x, _ in ms.values())
        own = {}
        for x, r in ms.values():
            a, b = r['seats']
            own[a] = own.get(a, 0) + x
            own[b] = own.get(b, 0) - x
        out[(e, k)] = {'D': d, 'margin': d / nmatch, 'T': 1.0 if d > 0 else 0.0 if d < 0 else 0.5, 'own': own}
    return out


def _dist_D(pw, pl):
    """The distribution of D (A's match wins - B's) over independent matches, +1 w.p. pw[j], -1 w.p.
    pl[j], a draw otherwise: an array over -m..m."""
    import numpy as np
    m = len(pw)
    d = np.zeros(2 * m + 1)
    d[m] = 1.0
    for a, b in zip(pw, pl):
        n = d * (1.0 - a - b)
        n[1:] += d[:-1] * a
        n[:-1] += d[1:] * b
        d = n
    return d


def _project(mus, add2, int2, pd, m, draws=16, seed=1):
    """The team result of m matches (m / 4 rounds, each a Latin-square rotation: the 4 rotations in a
    random order, cycling past 4): each event's true per-match mean `mus` (moment-matched to tau^2);
    its players' strengths, A's a_i and B's b_k centred (variance of a_i - b_k over the 16 pairings
    `add2`), and pairing interactions (variance `int2`), fixed for the event; each match won, lost or
    drawn (P(draw) = pd). Any whole number of rounds meets every player once per round, so the
    strengths cancel in the event's mean and shape only its spread. -> (reliability of T, P(two
    replays' T differ))."""
    import numpy as np
    rng = np.random.default_rng(seed)
    sv = math.sqrt(max(add2, 0.0) / 1.5)          # E[population var of 4 centred N(0, v)] = 3v / 4, twice
    si = math.sqrt(max(int2, 0.0))
    rounds = -(-m // 4)
    ets, vts, diff = [], [], []
    for mu in mus:
        for _ in range(draws):
            a = rng.normal(0.0, sv, 4)
            b = rng.normal(0.0, sv, 4)
            a -= a.mean()
            b -= b.mean()
            g = rng.normal(0.0, si, (4, 4))
            rot = rng.permutation(4)
            pairs = [(i, (i + rot[r % 4]) % 4) for r in range(rounds) for i in range(4)][:m]
            mj = np.clip(np.array([mu + a[i] - b[k] + g[i, k] for i, k in pairs]), -(1 - pd), 1 - pd)
            d = _dist_D((1 - pd + mj) / 2, (1 - pd - mj) / 2)
            w, tie = float(d[m + 1:].sum()), float(d[m])
            lo = max(0.0, 1.0 - w - tie)
            et = w + 0.5 * tie
            ets.append(et)
            vts.append(w + 0.25 * tie - et * et)
            diff.append(1 - (w * w + tie * tie + lo * lo))
    vb = _var(ets) if len(ets) > 1 else 0.0
    vw = _mean(vts)
    return vb / (vb + vw) if vb + vw > 0 else float('nan'), _mean(diff)


def _additive(xs_em, seats_em, s2_m):
    """The within-event spread of the per-match true means, split by a two-way additive fit per event
    (A's players' strengths less B's) into its additive part and the pairing interactions. -> (add2, int2)."""
    import numpy as np
    rss, within, kinv = [], [], []
    by_e = {}
    for (e, j), v in xs_em.items():
        by_e.setdefault(e, []).append((seats_em[(e, j)], sum(v) / len(v), 1.0 / len(v)))
    for e, cells in by_e.items():
        if len(cells) < 8:
            continue
        y = np.array([c[1] for c in cells])
        X = np.array([[1.0] + [1.0 if a // 2 == q else 0.0 for q in range(1, 4)] +
                      [-1.0 if (b - 1) // 2 == q else 0.0 for q in range(1, 4)] for (a, b), _, _ in cells])
        coef, _, rank, _ = np.linalg.lstsq(X, y, rcond=None)
        r = y - X @ coef
        rss.append(float((r ** 2).sum()) / max(1, len(y) - rank))
        within.append(float(np.var(y, ddof=1)))
        kinv.append(_mean([c[2] for c in cells]))
    if not rss:
        return float('nan'), float('nan')
    luck = s2_m * _mean(kinv)
    tot, inter = _mean(within) - luck, _mean(rss) - luck
    return tot - max(inter, 0.0), max(inter, 0.0)


def _information(by_e):
    """What one replay's label tells about each seat's true own record (the seat's expected wins -
    losses over its 3 matches, given the decks) and about its team's true margin: squared
    correlations from pairs of different replays of an event (independent luck), so
    Cov(label_k, own_k') estimates Cov(label, true own record). Labels from the seat's side: the
    team result (1 / 0.5 / 0), the team's margin, its own record."""
    def side(x, s, name):
        if name == 'own':
            return x['own'].get(s, 0)
        if name == 'margin':
            return x['margin'] if s % 2 == 0 else -x['margin']
        return x['T'] if s % 2 == 0 else 1.0 - x['T']
    names = ('T', 'margin', 'own')
    vals = {n: [] for n in names}
    for v in by_e.values():
        for x in v:
            for s in x['own']:
                for n in names:
                    vals[n].append(side(x, s, n))
    mean = {n: _mean(vals[n]) for n in names}
    var = {n: _var(vals[n]) for n in names}
    cov = {}
    for v in by_e.values():
        seats = sorted(v[0]['own'])
        for i in range(len(v)):
            for j in range(len(v)):
                if i == j:
                    continue
                for s in seats:
                    for a in names:
                        for b in ('own', 'margin'):
                            c = cov.setdefault((a, b), [0.0, 0])
                            c[0] += (side(v[i], s, a) - mean[a]) * (side(v[j], s, b) - mean[b])
                            c[1] += 1
    cv = {k: c[0] / c[1] for k, c in cov.items() if c[1]}
    true_own, true_margin = cv.get(('own', 'own'), float('nan')), cv.get(('margin', 'margin'), float('nan'))
    out = {}
    for a in names:
        out[a] = {'about_own_record': round(cv[(a, 'own')] ** 2 / (var[a] * true_own), 4) if true_own > 0 else None,
                  'about_team_margin': round(cv[(a, 'margin')] ** 2 / (var[a] * true_margin), 4) if true_margin > 0 else None}
    return out


def _stats(t, nmatch, K_cap=None, project=True):
    """The study's numbers for one (player, regime) table (`project`: the team result's model
    projections too)."""
    labels = _event_labels(t, nmatch)
    ev = sorted({e for (e, _) in labels})
    by_e = {e: [labels[(e, k)] for k in sorted(k for (ee, k) in labels if ee == e)] for e in ev}
    if K_cap:
        by_e = {e: v[:K_cap] for e, v in by_e.items()}
    by_e = {e: v for e, v in by_e.items() if len(v) >= 2}
    if len(by_e) < 3:
        return None
    # matches: x per (event, match) across replays of complete replays
    xs_em, seats_em = {}, {}
    for (e, k), ms in t.items():
        if (e, k) not in labels or e not in by_e:
            continue
        for j, (x, r) in ms.items():
            xs_em.setdefault((e, j), []).append(x)
            seats_em[(e, j)] = tuple(r['seats'])
    allx = [x for v in xs_em.values() for x in v]
    s2_m, t2_m, _ = components(list(xs_em.values()))         # luck per match; between-match true spread
    # event-level per-match mean: tau^2
    ev_x = {e: [] for e in by_e}
    for (e, j), v in xs_em.items():
        ev_x[e].append(_mean(v))
    ev_means = [_mean(v) for v in ev_x.values()]
    kbar = _mean([len(v) for v in by_e.values()])
    tau2 = _var(ev_means) - s2_m / (nmatch * kbar)          # true per-match mean, between events
    omega2 = t2_m - tau2                                     # pairing-specific spread within an event
    _, int2 = _additive(xs_em, seats_em, s2_m)               # its pairing interactions (additive fit) ...
    add2 = max(omega2 - int2, 0.0)                           # ... and the players' strengths: the rest
    draw = sum(1 for x in allx if x == 0) / len(allx)
    res = {'events': len(by_e), 'replays_per_event': round(kbar, 2), 'match_replays': len(allx),
           'match': {'mean_x': round(_mean(allx), 4), 'draw_rate': round(draw, 4),
                     'luck_var_per_match': round(s2_m, 4),
                     'true_var_per_match': round(t2_m, 4), 'tau2_event': round(tau2, 4),
                     'omega2_within_event': round(omega2, 4), 'omega2_additive': round(add2, 4),
                     'omega2_interaction': round(int2, 4)}}
    # the spread of the per-match win probability (moment-matched normal of the true means)
    sd = math.sqrt(max(t2_m, 0.0))
    res['match']['true_sd_per_match'] = round(sd, 4)
    res['match']['p_win_quartiles'] = [round((1 - draw + q) / 2, 3) for q in
                                       (_mean(allx) - 0.674 * sd, _mean(allx), _mean(allx) + 0.674 * sd)]
    # labels
    lab = {}
    for name in ('D', 'margin', 'T'):
        g = [[x[name] for x in v] for v in by_e.values()]
        s2, t2, _ = components(g)
        lab[name] = {'luck_var': round(s2, 5), 'true_var': round(t2, 5), 'total_var': round(s2 + t2, 5),
                     'reliability': round(t2 / (t2 + s2), 4) if t2 + s2 > 0 else None}
    # how often two replays of one event give different team results
    dis = []
    for v in by_e.values():
        ts = [x['T'] for x in v]
        for i in range(len(ts)):
            for j in range(i + 1, len(ts)):
                dis.append(ts[i] != ts[j])
    lab['T']['replays_disagree'] = round(_mean(dis), 4)
    # own records (per seat: its 3 matches)
    own_g = {}
    for e, v in by_e.items():
        for x in v:
            for s, r in x['own'].items():
                own_g.setdefault((e, s), []).append(r)
    s2o, t2o, _ = components(list(own_g.values()))
    # of the seats' true records, the team part (the event's margin x 3 per seat, sign by team)
    lab['own_record'] = {'luck_var': round(s2o, 4), 'true_var': round(t2o, 4),
                         'reliability': round(t2o / (t2o + s2o), 4) if t2o + s2o > 0 else None,
                         'matches': 3}
    res['labels'] = lab
    res['information'] = _information(by_e) if project else None
    # D's luck against independent matches' (a check: n x the per-match luck)
    lab['D']['luck_var_if_independent'] = round(nmatch * s2_m, 4)
    # projections: the margin (Spearman-Brown on the per-match components: m / (m + sigma^2 / tau^2)) and
    # the team result (the model of _project, which reproduces the measured 12-match reliability)
    c = s2_m / tau2 if tau2 > 0 else float('inf')
    res['noise_to_signal_per_match'] = round(c, 2) if math.isfinite(c) else None
    mu0 = _mean(ev_means)
    lam = tau2 / _var(ev_means) if _var(ev_means) > 0 else 0.0
    mus = [mu0 + math.sqrt(max(lam, 0.0)) * (x - mu0) for x in ev_means]
    proj = {}
    for m in ((4, 8, 12, 16, 24, 48) if project else ()):
        icc_t, dis_t = _project(mus, add2, int2, draw, m)
        proj[str(m)] = {'margin_reliability': round(m / (m + c), 4) if math.isfinite(c) else 0.0,
                        'team_result_reliability': round(icc_t, 4), 'team_result_replays_disagree': round(dis_t, 4)}
    res['projection'] = proj
    res['m_star'] = {str(d): (round(math.sqrt(d * c), 2) if math.isfinite(c) else None) for d in (1.0, 1.5, 3.0)}
    return res


def _boot(t, nmatch, n=300, seed=1):
    """Bootstrap over events: intervals of the key numbers."""
    rng = random.Random(seed)
    evs = sorted({e for (e, _) in t})
    keys = {}
    for _ in range(n):
        pick = [rng.choice(evs) for _ in evs]
        tt = {}
        for i, e in enumerate(pick):
            for (ee, k), ms in t.items():
                if ee == e:
                    tt[(i, k)] = ms
        s = _stats(tt, nmatch, project=False)
        if s is None:
            continue
        for name, v in (('margin_reliability_12', s['labels']['margin']['reliability']),
                        ('team_result_reliability_12', s['labels']['T']['reliability']),
                        ('noise_to_signal_per_match', s['noise_to_signal_per_match']),
                        ('m_star_1.5', s['m_star']['1.5'])):
            if v is not None:
                keys.setdefault(name, []).append(v)
    out = {}
    for k, v in keys.items():
        v.sort()
        out[k] = [round(v[int(0.05 * (len(v) - 1))], 3), round(v[int(0.95 * (len(v) - 1))], 3)]
    return out


def analyze(out, quiet=False, boot=300):
    res = {'time': time.time(), 'players': {}}
    for player in NETS + ('heuristic',):
        rows = read_jsonl(os.path.join(out, 'results', f'{player}.jsonl'))
        if not rows:
            continue
        res['players'][player] = {}
        for regime in sorted({r['regime'] for r in rows}):
            t = _table(rows, player, regime)
            nmatch = max(r['match'] for r in rows if r['regime'] == regime) + 1
            s = _stats(t, nmatch)
            if s is None:
                continue
            if boot:
                s['ci90'] = _boot(t, nmatch, n=boot)
            res['players'][player][regime] = s
        failed = read_jsonl(os.path.join(out, 'results', f'{player}.failed.jsonl'))
        if failed:
            res.setdefault('failed', {})[player] = [{'tag': r['tag'], 'error': r['error'][:300]} for r in failed]
    write_json(os.path.join(out, 'summary.json'), res)
    md = summary_md(res)
    with open(os.path.join(out, 'summary.md'), 'w', encoding='utf-8') as f:
        f.write(md)
    if not quiet:
        print(md)
    return res


def summary_md(res):
    L = [f'# Match-noise study ({time.strftime("%Y-%m-%d %H:%M", time.localtime(res["time"]))})', '']
    for player, regs in res['players'].items():
        for regime, s in regs.items():
            m, lab, pr = s['match'], s['labels'], s['projection']
            ci = s.get('ci90', {})
            L.append(f'## {player} players, {regime} drafters: {s["events"]} events x {s["replays_per_event"]} replays '
                     f'({s["match_replays"]} match replays)')
            L.append('')
            L.append(f'* per match (x = +1 / 0 / -1 for team A): mean {m["mean_x"]}, draws {m["draw_rate"]}; luck '
                     f'variance sigma^2 {m["luck_var_per_match"]}; true spread {m["true_var_per_match"]} (sd '
                     f'{m["true_sd_per_match"]}: P(A wins) quartiles {m["p_win_quartiles"]}), of which between events '
                     f'tau^2 {m["tau2_event"]} and within an event (pairings) {m["omega2_within_event"]} = the players\' '
                     f'strengths (additive) {m.get("omega2_additive")} + interactions {m.get("omega2_interaction")}')
            L.append(f'* noise to signal per match sigma^2/tau^2 = {s["noise_to_signal_per_match"]} '
                     f'(90% {ci.get("noise_to_signal_per_match")}); m* = sqrt(d sigma^2/tau^2): d=1 {s["m_star"]["1.0"]}, '
                     f'd=1.5 {s["m_star"]["1.5"]} (90% {ci.get("m_star_1.5")}), d=3 {s["m_star"]["3.0"]}')
            L.append(f'* 12-match labels: margin reliability {lab["margin"]["reliability"]} (90% '
                     f'{ci.get("margin_reliability_12")}), team result {lab["T"]["reliability"]} (90% '
                     f'{ci.get("team_result_reliability_12")}; two replays disagree {lab["T"]["replays_disagree"]}), '
                     f'own record (3 matches) {lab["own_record"]["reliability"]}; D luck var {lab["D"]["luck_var"]} '
                     f'(independent matches: {lab["D"].get("luck_var_if_independent")}), true var {lab["D"]["true_var"]}')
            inf = s.get('information')
            if inf:
                names = {'T': 'team result', 'margin': 'margin', 'own': 'own record'}
                L.append("* what one replay's label tells (squared correlation, from pairs of replays) about the seat's true "
                         "own record / its team's true margin: " + '; '.join(
                             f'{names[k]} {v["about_own_record"]} / {v["about_team_margin"]}' for k, v in inf.items()))
            L.append('')
            L.append('| matches per event | margin reliability | team result reliability (model) | two replays disagree |')
            L.append('|---:|---:|---:|---:|')
            for k, v in pr.items():
                L.append(f'| {k} | {v["margin_reliability"]} | {v["team_result_reliability"]} | '
                         f'{v["team_result_replays_disagree"]} |')
            L.append('')
    for player, fs in (res.get('failed') or {}).items():
        L.append(f'{player}: {len(fs)} match replays failed (an engine panic; left out): '
                 + '; '.join(f'{x["tag"]}: {x["error"][:160]}' for x in fs[:10]))
    return '\n'.join(L) + '\n'


# ---------------------------------------------------------------------------- main

def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('cmd', choices=('gen', 'play', 'analyze', 'bench', 'all', 'repro'))
    ap.add_argument('--tag', help='repro: regime/event/match/replay')
    ap.add_argument('--out', default=os.path.join(os.path.expanduser('~'), 'mtgz-noise'))
    ap.add_argument('--regimes', default=','.join(REGIMES))
    ap.add_argument('--events', type=int, default=None, help='gen, all: events per regime (default 40); play: only '
                    "each regime's first EVENTS (default: all)")
    ap.add_argument('--seed', type=int, default=1)
    ap.add_argument('--player', default='net', help="'net' (the --model network with mtg-4's self-play search), "
                    "'net-greedy' (the same search without temperature or root noise), 'net-eval' (its evaluation "
                    "search: eval_sims at every decision) or a bot ('heuristic')")
    ap.add_argument('--replays', type=int, default=4, help='replays of each match (net)')
    ap.add_argument('--heuristic-replays', type=int, default=16, help="'all': replays with the heuristic")
    ap.add_argument('--model', default=None)
    ap.add_argument('--matches', type=int, default=96, help='bench: matches')
    ap.add_argument('--seconds', type=float, default=0, help='bench: end the phase after this long (leaves/s only)')
    ap.add_argument('--conc', type=int, default=256)
    ap.add_argument('--threads', type=int, default=12)
    ap.add_argument('--groups', type=int, default=2)
    ap.add_argument('--infer-workers', type=int, default=3)
    ap.add_argument('--torch-threads', type=int, default=4)
    ap.add_argument('--no-nice', action='store_true')
    a = ap.parse_args()
    out = os.path.expanduser(a.out)
    os.makedirs(out, exist_ok=True)
    if not a.no_nice:
        mac_backoff.niceness()
    regimes = [r for r in a.regimes.split(',') if r]
    if not a.model and (a.cmd in ('bench', 'all') or a.cmd == 'play' and a.player.startswith('net')):
        ap.error('--model is needed')
    if a.cmd == 'gen':
        gen(out, regimes, a.events or 40, a.seed, a.threads)
    elif a.cmd == 'play':
        play(out, a.player, regimes, a.replays if a.player.startswith('net') else a.heuristic_replays, a.model, a.conc,
             a.threads, a.groups, a.infer_workers, a.torch_threads, a.seed, max_events=a.events or 0)
        analyze(out)
    elif a.cmd == 'repro':
        repro(out, a.tag, a.player, a.model, a.seed)
    elif a.cmd == 'analyze':
        analyze(out)
    elif a.cmd == 'bench':
        r = bench(out, a.model, a.matches, a.conc, a.threads, a.groups, a.infer_workers, a.torch_threads, a.seed,
                  seconds=a.seconds)
        print('BENCH ' + json.dumps(r), flush=True)
        with open(os.path.join(out, 'bench.jsonl'), 'a', encoding='utf-8') as f:
            f.write(json.dumps(dict(r, time=time.time())) + '\n')
    else:
        a.events = a.events or 40
        log(out, f'study: {a.events} events per regime {regimes}, heuristic x{a.heuristic_replays}, '
                 f'net x{a.replays} ({a.model})')
        gen(out, regimes, a.events, a.seed, 8)
        play(out, 'heuristic', regimes, a.heuristic_replays, None, 64, 8, 2, 1, 1, a.seed)
        analyze(out, quiet=True)
        play(out, 'net', regimes, a.replays, a.model, a.conc, a.threads, a.groups, a.infer_workers,
             a.torch_threads, a.seed)
        analyze(out)
        log(out, 'study done')
    return 0


if __name__ == '__main__':
    sys.exit(main())
