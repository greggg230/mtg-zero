"""The EoE sealed game log (docs/ENGINE.md 107.1): every finished `mtg_eoe` game of a run - self-play and
evaluation - with both seats' 40-card decks, which builder made each (the mixed field's `best` guess or an
`explore` build; `given` for a given deck) and the result, for scoring decks later.

zero-core keeps no per-game record (its result rows stay in memory; its shards hold samples without a game id
or seed), so the game writes it: `mtgz.az.run` turns the log on for every run it starts
(`<run>/games/games-<pid>.jsonl`, one file per process; `MTGZ_GAME_LOG=0` off), and the engine appends a line
per finished live game (`engine/src/az/game_log.rs`):

    {"v": 1, "spec": key, "seed", "deck_seed", "eval": pair or null,
     "seats": [{"mode", "pool_seed", "colors", "splash", "lands", "deck": {name: copies}}, ...],
     "searched": [seat 0's search dealt worlds, seat 1's], "result": [r0, r1] or null (aborted), "aborted",
     "turns", "time"}

after a spec line per spec (`{"v": 1, "spec": key, "sealed": {...}, "decks": "random" | "eval", ...}`).
`searched` tells a network seat (its search deals worlds) from a bot's; `decks` "eval" marks an evaluation
game (the run's evaluation set). The iteration is the one whose metrics line (written at the end of its
evaluation) is the first after the game's time.

    python -m mtgz.sealed.games RUN [--check] [--json PATH]

prints a summary (games by phase and iteration, the modes of the seats, the best guess's score against
the explore builds in self-play); `--check` rebuilds every game's decks from its spec and deck seed with
this checkout's builder (`sealed_seats`) and compares them with the logged ones (the log is the record:
a later builder may build other decks from the same seeds).
"""
import argparse
import bisect
import glob
import json
import math
import os
import sys


def log_path(argv, pid=None):
    """The game log's file of a run started with `argv` (its `--run`): `<run>/games/games-<pid>.jsonl`; None
    without `--run` or with `MTGZ_GAME_LOG=0` (what `mtgz.az.run` passes to `_engine.game_log`)."""
    if os.environ.get('MTGZ_GAME_LOG', '1') == '0':
        return None
    ap = argparse.ArgumentParser(add_help=False)
    ap.add_argument('--run')
    run = ap.parse_known_args(argv)[0].run
    return os.path.join(run, 'games', f'games-{pid or os.getpid()}.jsonl') if run else None


def files(run):
    """The log files of `run` (a run directory, or a directory of .jsonl files)."""
    d = os.path.join(run, 'games')
    return sorted(glob.glob(os.path.join(d if os.path.isdir(d) else run, '*.jsonl')))


def load(run):
    """({spec key: spec line}, [game line, ...]) of every log file of `run`, each game line with its file
    ('file') and phase ('phase': 'selfplay' or 'eval', from its spec's decks)."""
    specs, games = {}, []
    for f in files(run):
        with open(f, encoding='utf-8') as fh:
            for n, line in enumerate(fh, 1):
                line = line.strip()
                if not line:
                    continue
                try:
                    x = json.loads(line)
                except json.JSONDecodeError as e:
                    raise ValueError(f'{f}:{n}: not JSON ({e})') from None
                if 'seats' in x:
                    if x['spec'] not in specs:
                        raise ValueError(f'{f}:{n}: game of spec {x["spec"]} before its spec line')
                    x['file'] = os.path.basename(f)
                    x['phase'] = 'eval' if specs[x['spec']]['decks'] == 'eval' else 'selfplay'
                    games.append(x)
                elif 'sealed' in x:
                    specs[x['spec']] = x
                else:
                    raise ValueError(f'{f}:{n}: neither a game nor a spec line')
    return specs, games


def deck_counts(names):
    out = {}
    for n in names:
        out[n] = out.get(n, 0) + 1
    return out


def check(specs, games):
    """Rebuild every game's two seats from its spec and deck seed with this checkout's builder and compare the
    decks, modes and pool seeds with the logged ones: {'checked', 'mismatch': [(file, seed, seat, what)]}. A deck-choice
    game's seat (docs/ENGINE.md 107.2) is its logged candidate (chosen, or the explore pick) of the rebuilt candidates."""
    from mtgz.sealed import engine as SE
    bad = []
    for g in games:
        sl = specs[g['spec']]['sealed']
        sp = json.dumps({'sealed': sl, 'decks': 'random'})
        seats = SE.seats(sp, g['deck_seed'])
        cands = SE.candidates(sp, g['deck_seed']) if sl.get('deck_choice') else [None, None]
        for p, (got, want) in enumerate(zip(seats, g['seats'])):
            if cands[p] is not None:
                j = want.get('candidate')
                ok = j is not None and 0 <= j < len(cands[p]['candidates'])
                if not ok or deck_counts(cands[p]['candidates'][j]['deck']) != want['deck']:
                    bad.append((g['file'], g['seed'], p, 'deck'))
                exp = cands[p]['explore']
                if want['mode'] not in (('explore',) if exp is not None else ('chosen',)) or (exp is not None and exp != j):
                    bad.append((g['file'], g['seed'], p, 'mode'))
                if cands[p]['pool_seed'] != want['pool_seed']:
                    bad.append((g['file'], g['seed'], p, 'pool_seed'))
                continue
            for what, a, b in (('deck', deck_counts(got['deck']), want['deck']), ('mode', got['mode'], want['mode']),
                               ('pool_seed', got['pool_seed'], want['pool_seed'])):
                if a != b:
                    bad.append((g['file'], g['seed'], p, what))
    return {'checked': len(games), 'mismatch': bad}


def iteration_times(run):
    """[(time, iter)] of the run's metrics lines (the end of each iteration's evaluation), by time."""
    p = os.path.join(run, 'metrics.jsonl')
    out = []
    if os.path.exists(p):
        with open(p, encoding='utf-8') as f:
            for line in f:
                if line.strip():
                    m = json.loads(line)
                    if 'time' in m and 'iter' in m:
                        out.append((m['time'], m['iter']))
    return sorted(out)


def iteration_of(t, times):
    """The iteration a game finished at time `t` belongs to: the first metrics line at or after it (None
    after the last: an iteration in progress)."""
    i = bisect.bisect_left([x[0] for x in times], t)
    return times[i][1] if i < len(times) else None


def score(games, seat_of):
    """{'games', 'score', 'se'}: the mean result (1 win, 0.5 draw, 0 loss) of the seat `seat_of(game)` over
    finished games (aborted ones left out)."""
    xs = [g['result'][seat_of(g)] for g in games if g['result'] is not None]
    n = len(xs)
    if not n:
        return {'games': 0}
    m = sum(xs) / n
    sd = math.sqrt(sum((x - m) ** 2 for x in xs) / max(1, n - 1))
    return {'games': n, 'score': round(m, 4), 'se': round(sd / math.sqrt(n), 4)}


def summary(run, specs, games):
    times = iteration_times(run)
    out = {'run': run, 'files': len(files(run)), 'specs': len(specs), 'games': len(games)}
    for phase in ('selfplay', 'eval'):
        gs = [g for g in games if g['phase'] == phase]
        modes = {}
        for g in gs:
            k = '-'.join(sorted(s['mode'] for s in g['seats']))
            modes[k] = modes.get(k, 0) + 1
        iters = {}
        for g in gs:
            it = iteration_of(g['time'], times)
            iters[str(it)] = iters.get(str(it), 0) + 1
        mixed = [g for g in gs if sorted(s['mode'] for s in g['seats']) == ['best', 'explore']]
        # the deck choice (docs/ENGINE.md 107.2): the chosen candidates, and a chosen deck against an explore one
        chosen = [s['candidate'] for g in gs for s in g['seats'] if s['mode'] == 'chosen']
        cvx = [g for g in gs if sorted(s['mode'] for s in g['seats']) == ['chosen', 'explore']]
        out[phase] = {
            'games': len(gs), 'aborted': sum(g['aborted'] for g in gs),
            'draws': sum(1 for g in gs if g['result'] is not None and g['result'][0] == g['result'][1]),
            'turns': round(sum(g['turns'] for g in gs) / max(1, len(gs)), 1), 'modes': modes, 'iters': iters,
            'searched': {k: sum(1 for g in gs if ''.join('n' if s else 'b' for s in g['searched']) == k)
                         for k in ('nn', 'nb', 'bn', 'bb')},
            # the best guess's score against an explore build of another pool (both seats the same player in
            # self-play): a yardstick of the builder, never a training signal
            'best_vs_explore': score(mixed, lambda g: [s['mode'] for s in g['seats']].index('best')),
        }
        if chosen:
            out[phase]['chosen'] = {'seats': len(chosen), 'best_guess': sum(1 for j in chosen if j == 0),
                                    'candidates': {str(j): chosen.count(j) for j in sorted(set(chosen))}}
            out[phase]['chosen_vs_explore'] = score(cvx, lambda g: [s['mode'] for s in g['seats']].index('chosen'))
    return out


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0], formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('run', help='a run directory (its games/*.jsonl), or a directory of game log files')
    ap.add_argument('--check', action='store_true', help="rebuild each game's decks with this checkout's builder")
    ap.add_argument('--json', help='also write the summary here')
    a = ap.parse_args(argv)
    specs, games = load(a.run)
    if not games:
        # (a log that recorded nothing is a failure to see, never an empty success)
        print(f'no game lines in {a.run} ({len(files(a.run))} log files)', file=sys.stderr)
        raise SystemExit(1)
    out = summary(a.run, specs, games)
    if a.check:
        c = check(specs, games)
        out['check'] = {'checked': c['checked'], 'mismatches': len(c['mismatch']), 'first': c['mismatch'][:10]}
    s = json.dumps(out)
    print(s)
    if a.json:
        with open(a.json, 'w', encoding='utf-8') as f:
            f.write(s + '\n')
    if a.check and out['check']['mismatches']:
        raise SystemExit(1)
    return out


if __name__ == '__main__':
    main(sys.argv[1:])
