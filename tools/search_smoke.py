"""The search smoke (docs/ENGINE.md 15): zero-core's Rust driver and search play the game `mtg`
with a uniform evaluator (no network, no GPU) and self-play seats as a run preset sets them
(determinized worlds, playout caps, minor decisions, records), over several driver seeds and
specs. Every game must end and nothing may panic. A gate before pinning an engine for a run:
self-play search reaches states that random play (tests/fuzz.rs) does not.

  python tools/search_smoke.py                                   # seeds 0-3 x specs random,eval,match
  python tools/search_smoke.py --seeds 0-15 --games 400 --threads 8 --sims 64 --preset mtg-3
  python tools/search_smoke.py --specs team_draft,team_draft_2v2 --seeds 0-1

The team draft (M6 part 3, docs/ENGINE.md 45): `team_draft` (4v4) and `team_draft_2v2` draft and
build `--events` events with the self-play seat of the co-training preset (team_draft-4v4: picks
64 / 16 simulations, deckbuilding 32 / 8) and a uniform evaluator through zero-core's driver,
which parks each event at its deferred terminal; the matches of every parked event (the `mtg`
specs of `mtgz.az.drafted`) are played by a second driver of heuristic seats, and the results
resolve the event (zero-core's co-training coordinator). Every event must resolve with a sane
outcome (a team result 1 / 0 / 0.5 that its match wins give, samples emitted) and every match
must end.

Each (seed, spec) runs in a process of its own: a search worker's panic can leave zero-core's
driver waiting for ever (ENGINE.md 15), so a configuration with no batch for --stall seconds, or
over --timeout in all, is reported as a hang, with the panic messages it printed. The engine is
this checkout's mtgz/_engine.pyd (bash tools/build_py.sh). Exit code 0 iff every configuration
finished cleanly.
"""
import argparse
import json
import os
import subprocess
import sys
import threading
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SPECS = {
    # the self-play spec of the runs (new random decks every game)
    'random': {'max_turns': 60, 'eval_decks': 16, 'eval_seed': 1},
    # the evaluation decks (16 fixed pairs)
    'eval': {'max_turns': 60, 'eval_decks': 16, 'eval_seed': 1, 'decks': 'eval'},
    # no mulligans, more lands (other openings)
    'nomull': {'max_turns': 60, 'mulligans': False, 'lands': [18, 20]},
    # best-of-three matches with sideboarding, decks from random pools (M8b, docs/ENGINE.md 36)
    'match': {'max_turns': 60, 'eval_decks': 16, 'eval_seed': 1, 'format': 'bo3'},
    # ... on the fixed evaluation pools
    'match_eval': {'max_turns': 60, 'eval_decks': 16, 'eval_seed': 1, 'format': 'bo3', 'decks': 'eval'},
}
# team draft events (M6 part 3): (players, the team_draft spec); their matches as a co-training run plays them
DRAFT_SPECS = {
    'team_draft': (8, {'build_sims_div': 2}),
    'team_draft_2v2': (4, {'build_sims_div': 2}),
}
DRAFT_PRESET = 'team_draft-4v4'


def one_draft(seed, spec, events, threads, conc, stall):
    """One team draft configuration (module docs) -> a result dict."""
    sys.path.insert(0, ROOT)
    import mtgz.az  # noqa: F401  (registers the game package)
    from mtgz.az import drafted
    from zero import games as G
    from zero.cotrain import BIG, Coordinator, run_pair
    from zero.infer import UniformEvaluator
    from zero.run import DEFAULTS, PRESETS
    players, dspec = DRAFT_SPECS[spec]
    c = dict(DEFAULTS, **PRESETS[DRAFT_PRESET])
    seat = G.nn_seat('m', c['sims'], c['c_puct'], True, c['alpha'], c['eps'], c['temp_moves'], c['sims_fast'],
                     c['p_full'], c['chance_k'], c['margin_w'], bool(c['redet']), c.get('policy_target', 'visits'),
                     c.get('cq_c_visit', 50.0), c.get('cq_c_scale', 1.0))
    pdrv = G.Driver('team_draft', [[seat] * players] * events, spec=json.dumps(dspec), concurrency=min(conc, events),
                    seed=seed, threads=threads, groups=c['groups'], record=True, max_steps=20_000)
    h = G.seat_dict('heuristic')
    rdrv = G.Driver('mtg', [[h, h]], spec='{"format": "bo3"}', concurrency=64, seed=seed + 1, threads=threads,
                    groups=2, max_steps=60_000, rolling=True, queue_only=True)
    t0 = time.time()
    last = [time.time()]
    samples = [0]
    parked = [0]

    def watchdog():
        while True:
            time.sleep(1)
            if time.time() - last[0] > stall:
                print(f'HANG: no batch for {stall:.0f} s (a search worker panicked?)', file=sys.stderr, flush=True)
                os._exit(3)

    threading.Thread(target=watchdog, daemon=True).start()

    def on_primary(d):
        last[0] = time.time()
        s = d.take_samples()
        if s is not None:
            samples[0] += len(s['player'])
        parked[0] = max(parked[0], d.stats()['parked_bytes'])

    def on_resolver(d):
        last[0] = time.time()

    co = Coordinator(drafted.episodes, drafted.results)
    run_pair(co, pdrv, {'m': UniformEvaluator()}, rdrv, {}, on_primary=on_primary, on_resolver=on_resolver,
             resume=(BIG, 0))
    on_primary(pdrv)
    rows = pdrv.take_results()
    matches = co.take_rows()
    st = pdrv.stats()
    t, nm = players // 2, {8: 12, 4: 4}[players]
    bad = []
    for r in rows:
        res, sc = r['result'], r['scores']
        team = (round(res[0] * t, 6), round(res[1] * t, 6))
        want = (1.0, 0.0) if sc[0] > sc[1] else (0.0, 1.0) if sc[1] > sc[0] else (0.5, 0.5)
        if not r['over'] or abs(sum(res) - 1) > 1e-5 or team != want or sc[0] + sc[1] > nm or len(res) != players:
            bad.append(r)
    return {'seed': seed, 'spec': spec, 'games': len(rows), 'unfinished': len(bad) + pdrv.pending,
            'matches': len(matches), 'matches_unfinished': sum(1 for m in matches if not m['over']),
            'team_a_wins': sum(1 for r in rows if r['result'][0] > r['result'][1]),
            'draws': sum(1 for r in rows if r['result'][0] == r['result'][1]),
            'decisions': sum(r['decisions'] for r in rows), 'samples': samples[0], 'sims': st['sims'],
            'parked': st['deferred'], 'parked_mb_max': round(parked[0] / 1e6, 2),
            'seconds': round(time.time() - t0, 1)}


def one(seed, spec, games, threads, sims, conc, preset, stall):
    """One configuration, in this process: -> a result dict (raises on a driver error; exits
    with code 3 when no batch comes for `stall` seconds)."""
    sys.path.insert(0, ROOT)
    import mtgz.az  # noqa: F401  (registers the game package)
    from zero import games as G
    from zero.infer import UniformEvaluator, drive
    from zero.run import DEFAULTS, PRESETS
    c = dict(DEFAULTS, **PRESETS[preset])
    seat = G.nn_seat('m', sims, c['c_puct'], True, c['alpha'], c['eps'], c['temp_moves'], max(1, sims // 4),
                     c['p_full'], c['chance_k'], c['margin_w'], bool(c['redet']), c.get('policy_target', 'visits'),
                     c.get('cq_c_visit', 50.0), c.get('cq_c_scale', 1.0), sims_minor=c.get('sims_minor', 0) or None)
    # (a match is up to three games: three times the step cap)
    steps = c['max_steps'] * (3 if SPECS[spec].get('format') == 'bo3' else 1)
    drv = G.Driver('mtg', [[seat, seat]] * games, spec=json.dumps(SPECS[spec]), concurrency=conc, seed=seed,
                   threads=threads, groups=c['groups'], record=True, max_steps=steps)
    t0 = time.time()
    last = [time.time()]

    def watchdog():
        # With a uniform evaluator a batch comes every few milliseconds: a minute without one is
        # the driver waiting for a group whose worker died (ENGINE.md 15), not a slow search.
        while True:
            time.sleep(1)
            if time.time() - last[0] > stall:
                print(f'HANG: no batch for {stall:.0f} s (a search worker panicked?)', file=sys.stderr, flush=True)
                os._exit(3)

    threading.Thread(target=watchdog, daemon=True).start()

    def on_batch(lb):
        last[0] = time.time()
        drv.take_samples()

    drive(drv, {'m': UniformEvaluator()}, on_batch=on_batch)
    res = drv.take_results()
    st = drv.stats()
    return {'seed': seed, 'spec': spec, 'games': len(res), 'unfinished': sum(1 for r in res if not r['over']),
            'decisions': sum(r['decisions'] for r in res), 'sims': st['sims'], 'seconds': round(time.time() - t0, 1)}


def parse_seeds(s):
    out = []
    for part in s.split(','):
        a, _, b = part.partition('-')
        out += list(range(int(a), int(b) + 1)) if b else [int(a)]
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('--seeds', default='0-3', help='driver seeds: 0-3 or 0,5,9')
    ap.add_argument('--specs', default='random,eval,match', help=f'comma-separated, of {sorted(SPECS)}')
    ap.add_argument('--games', type=int, default=400)
    ap.add_argument('--events', type=int, default=24, help='team draft events per configuration')
    ap.add_argument('--threads', type=int, default=8)
    ap.add_argument('--sims', type=int, default=64)
    ap.add_argument('--conc', type=int, default=256)
    ap.add_argument('--preset', default='mtg-3', help='the run preset whose self-play seat is used')
    ap.add_argument('--timeout', type=float, default=1800, help='seconds per configuration before it counts as a hang')
    ap.add_argument('--stall', type=float, default=60, help='seconds without a batch before it counts as a hang')
    ap.add_argument('--one', nargs=2, metavar=('SEED', 'SPEC'), help=argparse.SUPPRESS)
    a = ap.parse_args()
    if a.one:
        if a.one[1] in DRAFT_SPECS:
            r = one_draft(int(a.one[0]), a.one[1], a.events, a.threads, a.conc, a.stall)
        else:
            r = one(int(a.one[0]), a.one[1], a.games, a.threads, a.sims, a.conc, a.preset, a.stall)
        print('RESULT ' + json.dumps(r), flush=True)
        return 0
    t0 = time.time()
    bad = []
    for spec in a.specs.split(','):
        if spec not in SPECS and spec not in DRAFT_SPECS:
            sys.exit(f'unknown spec {spec!r} (have {sorted(SPECS) + sorted(DRAFT_SPECS)})')
        for seed in parse_seeds(a.seeds):
            cmd = [sys.executable, os.path.abspath(__file__), '--one', str(seed), spec, '--games', str(a.games),
                   '--events', str(a.events),
                   '--threads', str(a.threads), '--sims', str(a.sims), '--conc', str(a.conc), '--preset', a.preset,
                   '--stall', str(a.stall)]
            t = time.time()
            try:
                p = subprocess.run(cmd, capture_output=True, text=True, timeout=a.timeout, cwd=ROOT,
                                   env=dict(os.environ, RUST_BACKTRACE=os.environ.get('RUST_BACKTRACE', '1')))
                out, err, code = p.stdout, p.stderr, p.returncode
            except subprocess.TimeoutExpired as e:
                out = e.stdout.decode(errors='replace') if isinstance(e.stdout, bytes) else (e.stdout or '')
                err = e.stderr.decode(errors='replace') if isinstance(e.stderr, bytes) else (e.stderr or '')
                code = 'timeout'
            line = next((x for x in out.splitlines() if x.startswith('RESULT ')), None)
            if code == 0 and line and spec in DRAFT_SPECS:
                r = json.loads(line[7:])
                ok = r['games'] == a.events and not r['unfinished'] and not r['matches_unfinished']
                print(f'{"ok   " if ok else "BAD  "} seed {seed} {spec}: {r["games"]} events resolved ({r["unfinished"]} '
                      f'bad or pending; team A won {r["team_a_wins"]}, {r["draws"]} drawn), {r["matches"]} matches '
                      f'({r["matches_unfinished"]} unfinished), {r["decisions"]} draft decisions, {r["samples"]} samples, '
                      f'{r["sims"]} sims, {r["parked"]} parked, parked records max {r["parked_mb_max"]} MB, '
                      f'{r["seconds"]} s', flush=True)
                if not ok:
                    bad.append((seed, spec, 'events missing, unresolved or insane, or matches unfinished'))
            elif code == 0 and line:
                r = json.loads(line[7:])
                print(f'ok    seed {seed} {spec}: {r["games"]} games ({r["unfinished"]} unfinished), '
                      f'{r["decisions"]} decisions, {r["sims"]} sims, {r["seconds"]} s', flush=True)
                if r['games'] != a.games or r['unfinished']:
                    bad.append((seed, spec, 'games missing or unfinished'))
            else:
                what = {'timeout': 'HANG (timeout)', 3: 'HANG (stalled)'}.get(code, f'FAIL (exit {code})')
                lines = err.splitlines()
                # each panic's location and message (the next line), the driver's error
                panics = [x for i, x in enumerate(lines) if 'panicked at' in x or (i and 'panicked at' in lines[i - 1])
                          or x.startswith(('RuntimeError', 'HANG'))]
                print(f'{what} seed {seed} {spec} after {time.time() - t:.0f} s', flush=True)
                for x in (panics or lines[-20:]):
                    print('    ' + x[:2000], flush=True)
                bad.append((seed, spec, what))
    print(f'search smoke: {len(bad)} failed of {len(parse_seeds(a.seeds)) * len(a.specs.split(","))} '
          f'configurations, {time.time() - t0:.0f} s', flush=True)
    return 1 if bad else 0


if __name__ == '__main__':
    sys.exit(main())
