"""The search smoke (docs/ENGINE.md 15): zero-core's Rust driver and search play the game `mtg`
with a uniform evaluator (no network, no GPU) and self-play seats as a run preset sets them
(determinized worlds, playout caps, minor decisions, records), over several driver seeds and
specs. Every game must end and nothing may panic. A gate before pinning an engine for a run:
self-play search reaches states that random play (tests/fuzz.rs) does not.

  python tools/search_smoke.py                                   # seeds 0-3 x specs random,eval
  python tools/search_smoke.py --seeds 0-15 --games 400 --threads 8 --sims 64 --preset mtg-3

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
}


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
    drv = G.Driver('mtg', [[seat, seat]] * games, spec=json.dumps(SPECS[spec]), concurrency=conc, seed=seed,
                   threads=threads, groups=c['groups'], record=True, max_steps=c['max_steps'])
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
    ap.add_argument('--specs', default='random,eval', help=f'comma-separated, of {sorted(SPECS)}')
    ap.add_argument('--games', type=int, default=400)
    ap.add_argument('--threads', type=int, default=8)
    ap.add_argument('--sims', type=int, default=64)
    ap.add_argument('--conc', type=int, default=256)
    ap.add_argument('--preset', default='mtg-3', help='the run preset whose self-play seat is used')
    ap.add_argument('--timeout', type=float, default=1800, help='seconds per configuration before it counts as a hang')
    ap.add_argument('--stall', type=float, default=60, help='seconds without a batch before it counts as a hang')
    ap.add_argument('--one', nargs=2, metavar=('SEED', 'SPEC'), help=argparse.SUPPRESS)
    a = ap.parse_args()
    if a.one:
        r = one(int(a.one[0]), a.one[1], a.games, a.threads, a.sims, a.conc, a.preset, a.stall)
        print('RESULT ' + json.dumps(r), flush=True)
        return 0
    t0 = time.time()
    bad = []
    for spec in a.specs.split(','):
        if spec not in SPECS:
            sys.exit(f'unknown spec {spec!r} (have {sorted(SPECS)})')
        for seed in parse_seeds(a.seeds):
            cmd = [sys.executable, os.path.abspath(__file__), '--one', str(seed), spec, '--games', str(a.games),
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
            if code == 0 and line:
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
