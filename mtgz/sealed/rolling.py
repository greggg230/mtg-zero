"""Run 2's deck choice by the evaluator, retrained inside the run (docs/ENGINE.md 112).

The self-play spec chooses each seat's deck with an exported deck evaluator (the sealed spec's `"deck_choice": {...,
"by": "evaluator", "evaluator": <file>}`: the engine's Rust scorer, no Python in self-play). This module is the run's
side, through zero-core's game-package hooks (its docs/az.md 40; `mtgz.az` registers them for `mtg_eoe`):

* **The versions** (`selfplay_spec`, the `selfplay_specs` hook, asked at the start of every self-play phase): the run's
  evaluators are immutable files `<run>/evaluators/v<iter>.json` (`iter`: the last iteration whose games trained it),
  each named in the spec by its path and sha256. The first is the spec's own file, copied in when the run first plays
  (`v<the iteration before>.json`). Every phase plays the newest version published before it began - a version
  published during a phase is used from the next one (zero-core switches the rolling driver's spec between phases: the
  games it starts from then on; games in flight keep theirs). `evaluators/used.jsonl` records each iteration's version,
  `evaluators/index.jsonl` each version (its sha256, its training window, its held-out numbers). Every game's spec line
  in the game log names its version's file and sha256, and every chosen deck its sha256 (docs/ENGINE.md 112): a game
  replays exactly from its line whatever the run did later.
* **The rolling retrain** (run setting `evaluator_every` N > 0): at the start of iteration it, when b = it - 1 is a
  multiple of N, at least `evaluator_min_window` iterations of the run's own games follow its first version and no job
  runs, a background job starts (`python -m mtgz.sealed.rolling retrain RUN --iter b --from-iter lo ...`: CPU only,
  below-normal priority, `evaluator_threads` threads) on the window lo..b - the newest `evaluator_window` iterations of
  the run's own games. It snapshots the window's games (the log lines of those iterations and the metrics up to b),
  values them with the checkpoints that played them (`python -m mtgz.sealed.evaluator values ... --checkpoint played`:
  the leak-free pre-game value, docs/ENGINE.md 108.2), trains (`python -m mtgz.sealed.evaluator train ... --split time
  --time-from b + 1 --val-iters V --kinds K --pools 0 --from-iter lo [--half-life H] [--pair-base B]`: the window's last
  V iterations validate - the selection of the distillation mix and the L2, early stopping -, the rest train, weighted
  by recency with `evaluator_half_life` H > 0, ENGINE.md 111), exports the selected `evaluator_<kind>.pt`
  (`Evaluator.export`, the export format of ENGINE.md 111.6; its informational meta's NaN metrics of the empty test
  split as null), checks the engine against the export's numpy reference and torch on candidate decks
  (`mtgz.sealed.scorer`), and publishes it atomically as `v<b>.json`. Self-play never waits for it. A failed job is
  logged loudly at the next phase (EVALUATOR RETRAIN FAILED ...) and the run keeps the version it has; a job outliving
  its run process is found by its pid file and waited for, never doubled.
* **The deck-choice evaluation** (`deckchoice`, a yardstick of zero-core's `evals`; run setting
  `deckchoice_eval_games` N > 0): the checkpoint's seat chooses its deck among its K candidates of the fixed evaluation
  pools by the evaluator its iteration's self-play used (`used.jsonl`; shipped to a remote evaluation with
  `remote_eval_inputs`), against the yardstick (the heuristic) playing its pool's best guess; N games, the network in
  each seat half the time, the spec's `"seats"` naming the chooser. The run's own evaluations (the fixed decks against
  the heuristic, random, Elo) are run 1's, unchanged. `python -m mtgz.sealed.rolling deckeval CKPT --evaluator FILE`
  plays the same for any checkpoint (run 1's, for the comparison), `--best-guess` the network's best guess instead.

Pure self-play: the run's own games and networks train every version.
"""
import argparse
import glob
import json
import os
import re
import shutil
import subprocess
import sys
import time
import traceback

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
# the package's run settings (zero-core GamePackage.settings; registered by mtgz.az)
SETTINGS = {
    'evaluator_every': 0,         # N > 0: retrain the deck evaluator every N iterations (a background job); 0: off
    'evaluator_window': 40,       # a retrain reads at most the newest W iterations of the run's own games
    'evaluator_min_window': 8,    # ... and waits for this many after the run's first version
    'evaluator_val_iters': 2,     # of them, the newest V validate
    'evaluator_kind': 'set',      # the model retrained and exported: 'linear', 'set' or 'pair'
    'evaluator_pair_base': 'linear',  # a pair evaluator's strength model ('linear' or 'set': train's --pair-base)
    'evaluator_threads': 4,       # the job's CPU threads
    'evaluator_half_life': 0.0,   # > 0: train's recency weighting --half-life (iterations); 0: none
    'deckchoice_eval_games': 0,   # N > 0: the deck-choice evaluation's games per evaluated iteration; 0: off
}
VERSION_RE = re.compile(r'^v(\d{4,})\.json$')
_JOBS = {}                         # run directory -> (iteration, Popen) of this process's job


def edir(run):
    return os.path.join(run, 'evaluators')


def versions(run):
    """The run's published versions [(iter, path)], oldest first."""
    out = []
    for p in glob.glob(os.path.join(edir(run), 'v*.json')):
        m = VERSION_RE.match(os.path.basename(p))
        if m:
            out.append((int(m.group(1)), p.replace(os.sep, '/')))
    return sorted(out)


def _append(path, row):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, 'a', encoding='utf-8') as f:
        f.write(json.dumps(row) + '\n')


def _rows(path):
    if not os.path.exists(path):
        return []
    with open(path, encoding='utf-8') as f:
        return [json.loads(x) for x in f if x.strip()]


def deck_choice_of(spec):
    """The sealed spec's deck choice by an evaluator (its dict), or None."""
    sp = json.loads(spec) if isinstance(spec, str) else spec
    dc = (sp.get('sealed') or {}).get('deck_choice')
    return dc if isinstance(dc, dict) and dc.get('by') == 'evaluator' else None


def with_evaluator(spec, path, sha):
    """`spec` (JSON) with its deck choice's evaluator the file `path` of sha256 `sha`."""
    sp = json.loads(spec)
    sp['sealed']['deck_choice'] = dict(sp['sealed']['deck_choice'], evaluator=os.path.abspath(path).replace(os.sep, '/'),
                                       evaluator_sha=sha)
    return json.dumps(sp)


def _log(R, msg):
    R.log(msg)


# ----------------------------------------------------------------------------- the self-play hook

def selfplay_spec(R, it):
    """zero-core's `selfplay_specs` hook (module docs): the spec of iteration `it`'s self-play, or None (the run's)."""
    from . import scorer as S
    c = R.cfg
    dc = deck_choice_of(c['spec'])
    every = int(c.get('evaluator_every', 0) or 0)
    if dc is None:
        if every > 0:
            raise ValueError('evaluator_every needs the self-play spec\'s deck choice by an evaluator '
                             '("deck_choice": {"by": "evaluator", ...}; docs/ENGINE.md 112)')
        return None
    if every <= 0:
        return None                                # the spec's evaluator, as given
    run = R.run
    vs = versions(run)
    if not vs:
        # the first version: the spec's own file, copied into the run (its iteration: the one before the first phase)
        src = dc['evaluator']
        sha = S.sha256_file(src)
        if dc.get('evaluator_sha') and dc['evaluator_sha'] != sha:
            raise ValueError(f'the spec\'s evaluator {src} has sha256 {sha}, not the spec\'s {dc["evaluator_sha"]}')
        dst = os.path.join(edir(run), f'v{it - 1:04d}.json')
        os.makedirs(edir(run), exist_ok=True)
        tmp = dst + f'.{os.getpid()}.tmp'
        shutil.copyfile(src, tmp)
        os.replace(tmp, dst)
        _append(os.path.join(edir(run), 'index.jsonl'), {'version': f'v{it - 1:04d}', 'iter': it - 1, 'sha256': sha,
                                                         'source': os.path.abspath(src).replace(os.sep, '/'),
                                                         'initial': True, 'time': time.time()})
        _log(R, f'iter {it} deck evaluator: the spec\'s {src} is the run\'s first version v{it - 1:04d} ({sha[:12]})')
        vs = versions(run)
    poll(R)
    launch_if_due(R, it, vs)
    vi, path = vs[-1]
    sha = S.sha256_file(path)
    prev = (_rows(os.path.join(edir(run), 'used.jsonl')) or [{}])[-1]
    _append(os.path.join(edir(run), 'used.jsonl'), {'iter': it, 'version': os.path.basename(path), 'sha256': sha,
                                                    'time': time.time()})
    if prev.get('sha256') != sha:
        _log(R, f'iter {it} self-play: decks chosen by the deck evaluator {os.path.basename(path)} ({sha[:12]})')
    return with_evaluator(c['spec'], path, sha)


def job_dir(run, b):
    return os.path.join(edir(run), 'work', f'v{b:04d}')


def _alive(pid, b):
    """Whether process `pid` is a retrain job of iteration `b` (its command line: never another process that took the
    number of a dead job)."""
    try:
        import psutil
        p = psutil.Process(int(pid))
        cmd = p.cmdline()
        return p.status() != 'zombie' and 'mtgz.sealed.rolling' in cmd and 'retrain' in cmd and str(b) in cmd
    except Exception:                              # noqa: BLE001 (no such process)
        return False


def running(R):
    """This run's retrain job in progress (its iteration), else None: this process's, or (after a restart) one its pid
    file names that is still alive."""
    j = _JOBS.get(R.run)
    if j is not None and j[1].poll() is None:
        return j[0]
    for d in glob.glob(os.path.join(edir(R.run), 'work', 'v*')):
        b = int(os.path.basename(d)[1:])
        if os.path.exists(os.path.join(d, 'status.json')) or (j is not None and j[0] == b):
            continue
        pf = os.path.join(d, 'pid')
        if os.path.exists(pf):
            with open(pf) as f:
                pid = f.read().strip()
            if pid and _alive(pid, b):
                return b
    return None


def poll(R):
    """Report every finished job once (its status.json; a job dir with no status and no live process: failed)."""
    if running(R) is not None:
        return
    _JOBS.pop(R.run, None)
    for d in sorted(glob.glob(os.path.join(edir(R.run), 'work', 'v*'))):
        if os.path.exists(os.path.join(d, 'reported')):
            continue
        name = os.path.basename(d)
        st = os.path.join(d, 'status.json')
        if os.path.exists(st):
            with open(st, encoding='utf-8') as f:
                s = json.load(f)
        else:
            s = {'ok': False, 'error': 'the job ended without a status (killed?); its log: ' + os.path.join(d, 'log.txt')}
        if s.get('ok'):
            _log(R, f'deck evaluator {name} published: sha256 {s["sha256"][:12]}, {s.get("games")} games of iterations '
                    f'{s.get("window")}, validation {json.dumps(s.get("val"))}, {s.get("seconds")} s')
        else:
            keep = versions(R.run)
            kept = os.path.basename(keep[-1][1]) if keep else "the spec's evaluator"
            _log(R, f'EVALUATOR RETRAIN FAILED {name}: {str(s.get("error"))[-600:]} - keeping {kept}')
        with open(os.path.join(d, 'reported'), 'w') as f:
            f.write(str(time.time()))


def launch_if_due(R, it, vs):
    """Start the retrain of iteration b = it - 1 when it is due (module docs)."""
    c = R.cfg
    every = int(c['evaluator_every'])
    W, V = int(c.get('evaluator_window', 40)), int(c.get('evaluator_val_iters', 2))
    M = int(c.get('evaluator_min_window', 8))
    b = it - 1
    first = vs[0][0]                               # the run's first version: no game of the run before it
    lo = max(first + 1, b - W + 1)                 # the window: the newest W iterations of the run's own games
    if b % every or b - first < M or any(v == b for v, _ in vs) or os.path.isdir(job_dir(R.run, b)):
        return
    if not 1 <= V < M <= W:
        raise ValueError(f'evaluator_val_iters {V} < evaluator_min_window {M} <= evaluator_window {W}: not so')
    r = running(R)
    if r is not None:
        _log(R, f'iter {it} deck evaluator: the retrain of v{r:04d} is still running; v{b:04d} skipped')
        return
    d = job_dir(R.run, b)
    os.makedirs(d, exist_ok=True)
    cmd = [sys.executable, '-m', 'mtgz.sealed.rolling', 'retrain', R.run, '--iter', str(b), '--from-iter', str(lo),
           '--val-iters', str(V), '--kind', str(c.get('evaluator_kind', 'set')),
           '--pair-base', str(c.get('evaluator_pair_base', 'linear')), '--threads', str(int(c.get('evaluator_threads', 4))),
           '--half-life', str(float(c.get('evaluator_half_life', 0.0) or 0.0))]
    log = open(os.path.join(d, 'log.txt'), 'w', encoding='utf-8')
    p = subprocess.Popen(cmd, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL,
                         env=job_env(int(c.get('evaluator_threads', 4))), **background_flags())
    log.close()
    with open(os.path.join(d, 'pid'), 'w') as f:
        f.write(str(p.pid))
    _JOBS[R.run] = (b, p)
    _log(R, f'iter {it} deck evaluator: the retrain of v{b:04d} started (the games of iterations {lo}-{b}, '
            f'pid {p.pid}, {d})')


def job_env(threads):
    env = dict(os.environ)
    env.update({'CUDA_VISIBLE_DEVICES': '-1', 'OMP_NUM_THREADS': str(threads), 'MKL_NUM_THREADS': str(threads),
                'MTGZ_GAME_LOG': '0', 'PYTHONIOENCODING': 'utf-8'})
    return env


def background_flags():
    """A job below normal priority with no window (its children inherit the class on Windows)."""
    if os.name == 'nt':
        return {'creationflags': 0x00004000 | 0x08000000}   # BELOW_NORMAL_PRIORITY_CLASS | CREATE_NO_WINDOW
    return {'start_new_session': True, 'preexec_fn': lambda: os.nice(10)}


# ----------------------------------------------------------------------------- the retrain job

def snapshot(run, lo, hi, out):
    """The game log lines of iterations lo..hi (each file's, with the spec lines its games need) and the metrics up to hi
    into `out` (a run directory of its own) -> (games, iterations seen)."""
    from . import games as GL
    times = GL.iteration_times(run)
    os.makedirs(os.path.join(out, 'games'), exist_ok=True)
    with open(os.path.join(run, 'metrics.jsonl'), encoding='utf-8') as f:
        keep = [x for x in f if x.strip() and json.loads(x).get('iter', -1) <= hi]
    with open(os.path.join(out, 'metrics.jsonl'), 'w', encoding='utf-8', newline='') as f:
        f.writelines(keep)
    n, seen = 0, set()
    for path in GL.files(run):
        specs, lines = {}, []
        fi = GL.file_iteration(path)               # (a remote evaluation's file: its games' iteration by its name)
        with open(path, encoding='utf-8') as f:
            for x in f:
                if not x.endswith('\n'):
                    break                          # (a line being written)
                v = json.loads(x)
                if 'seats' not in v:
                    specs[v['spec']] = x
                    continue
                g = dict(v, file=path, **({'iter': fi} if fi is not None else {}))
                itx = GL.game_iteration(g, times)
                if itx is not None and lo <= itx <= hi:
                    lines.append(x)
                    seen.add(itx)
        if lines:
            need = {json.loads(x)['spec'] for x in lines}
            with open(os.path.join(out, 'games', os.path.basename(path)), 'w', encoding='utf-8', newline='') as f:
                f.writelines([specs[k] for k in sorted(need)] + lines)
            n += len(lines)
    return n, sorted(seen)


def _run(cmd, log, threads):
    log(f'$ {" ".join(cmd)}')
    t0 = time.time()
    r = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True, env=job_env(threads), encoding='utf-8',
                       errors='replace', **({'creationflags': 0x08000000} if os.name == 'nt' else {}))
    log((r.stdout or '')[-4000:] + (r.stderr or '')[-8000:])
    if r.returncode != 0:
        raise RuntimeError(f'{" ".join(cmd[1:4])} exit {r.returncode}: {(r.stderr or r.stdout or "")[-1500:]}')
    log(f'# {time.time() - t0:.1f} s')
    return r.stdout


def _finite(x):
    """A JSON-able copy with every non-finite float None (an export's informational meta: the job's empty test split
    gives NaN metrics, which JSON parsers refuse)."""
    if isinstance(x, dict):
        return {k: _finite(v) for k, v in x.items()}
    if isinstance(x, (list, tuple)):
        return [_finite(v) for v in x]
    if isinstance(x, float) and (x != x or x in (float('inf'), float('-inf'))):
        return None
    return x


def retrain(run, b, lo, val_iters=2, kind='set', threads=4, half_life=0.0, pair_base='linear', check_decks=400,
            log=print):
    """The job (module docs): the games of iterations lo..b -> its status dict (also written to status.json; a failure
    raises after writing it)."""
    from . import scorer as S
    from .evaluator import Evaluator
    t0 = time.time()
    d = job_dir(run, b)
    os.makedirs(d, exist_ok=True)
    status = {'ok': False, 'version': f'v{b:04d}', 'iter': b, 'window': [lo, b], 'kind': kind, 'half_life': half_life,
              'started': t0}
    try:
        snap = os.path.join(d, 'snap')
        if os.path.isdir(snap):
            shutil.rmtree(snap)
        n, seen = snapshot(run, lo, b, snap)
        status.update(games_lines=n, iterations=seen)
        log(f'# snapshot: {n} games of iterations {seen}')
        if len(seen) < val_iters + 1:
            raise RuntimeError(f'the window {lo}-{b} has games of {len(seen)} iterations: too few to train')
        py = sys.executable
        values = os.path.join(d, 'values.npz')
        _run([py, '-m', 'mtgz.sealed.evaluator', 'values', snap, '--checkpoint', 'played', '--models',
              os.path.join(run, 'models'), '--out', values, '--threads', str(threads)], log, threads)
        tdir = os.path.join(d, 'train')
        cmd = [py, '-m', 'mtgz.sealed.evaluator', 'train', snap, '--values', values, '--out', tdir, '--split', 'time',
               '--time-from', str(b + 1), '--val-iters', str(val_iters), '--kinds', kind, '--pools', '0',
               '--threads', str(threads), '--seed', str(b), '--from-iter', str(lo)]
        if kind == 'pair':
            cmd += ['--pair-base', pair_base]
        if half_life > 0:
            cmd += ['--half-life', str(half_life)]
        _run(cmd, log, threads)
        with open(os.path.join(tdir, 'report.json'), encoding='utf-8') as f:
            rep = json.load(f)
        pt = os.path.join(tdir, f'evaluator_{kind}.pt')
        staged = os.path.join(d, f'v{b:04d}.json')
        e = Evaluator.load(pt)
        e.meta = _finite(e.meta)
        e.export(staged)                           # (mtgz.sealed.evaluator's export format, ENGINE.md 111.6)
        sha = S.sha256_file(staged)
        cmp = S.compare(staged, S.sample_decks(check_decks, seed=b), pt)
        log('# export check ' + json.dumps(cmp))
        why = S.failed(cmp)
        if why:
            raise RuntimeError(f'the export does not score as its reference and torch: {why}')
        dst = os.path.join(edir(run), f'v{b:04d}.json')
        tmp = dst + f'.{os.getpid()}.tmp'
        shutil.copyfile(staged, tmp)
        os.replace(tmp, dst)
        sel = rep['models'][kind]['selected']
        status.update(ok=True, sha256=sha, games=rep['dataset']['games'], selected={'lam': sel.get('lam'), 'hyper': sel['hyper']},
                      val=min((g['val_logloss'] for g in rep['models'][kind]['grid']), default=None),
                      export_check=cmp, seconds=round(time.time() - t0, 1))
        _append(os.path.join(edir(run), 'index.jsonl'), {'version': f'v{b:04d}', 'iter': b, 'sha256': sha, 'window':
                                                         status['window'], 'iterations': seen, 'games': status['games'],
                                                         'kind': kind, 'selected': status['selected'],
                                                         'val_logloss': status['val'], 'time': time.time()})
        shutil.rmtree(snap, ignore_errors=True)
    except Exception as e:                         # noqa: BLE001 (recorded, then raised: the run logs it loudly)
        status.update(ok=False, error=f'{type(e).__name__}: {e}', traceback=traceback.format_exc()[-3000:],
                      seconds=round(time.time() - t0, 1))
        with open(os.path.join(d, 'status.json'), 'w', encoding='utf-8') as f:
            json.dump(status, f, indent=1)
        raise
    with open(os.path.join(d, 'status.json'), 'w', encoding='utf-8') as f:
        json.dump(status, f, indent=1)
    return status


# ----------------------------------------------------------------------------- the deck-choice evaluation

def evaluator_of(R, it):
    """(path, sha256) of the evaluator iteration `it`'s self-play chose decks with: its `used.jsonl` row (in this run
    directory or mirror), else the spec's own file (without the rolling retrain, or before any self-play); None when
    there is none here (a remote evaluation of a checkpoint no self-play used: an error the run logs)."""
    from . import scorer as S
    rows = [r for r in _rows(os.path.join(edir(R.run), 'used.jsonl')) if r['iter'] == it]
    if rows:
        p = os.path.join(edir(R.run), rows[-1]['version'])
        sha = S.sha256_file(p)
        if sha != rows[-1]['sha256']:
            raise RuntimeError(f'{p}: sha256 {sha}, not the {rows[-1]["sha256"]} iteration {it} used')
        return p, sha
    # (no row: the run's own spec's file - without the rolling retrain every iteration's, else the first version's
    # source, for a checkpoint evaluated before any self-play: a fresh run's iteration 0)
    dc = deck_choice_of(R.cfg['spec'])
    if dc is not None and os.path.isfile(dc['evaluator']):
        return dc['evaluator'], S.sha256_file(dc['evaluator'])
    return None


def remote_eval_inputs(R, it):
    """zero-core's `remote_eval_inputs` hook: the evaluator file the deck-choice evaluation of `it` reads (and the
    record naming it), when that evaluation is on and the run retrains its evaluator."""
    if int(R.cfg.get('deckchoice_eval_games', 0) or 0) <= 0 or int(R.cfg.get('evaluator_every', 0) or 0) <= 0:
        return []
    rows = [r for r in _rows(os.path.join(edir(R.run), 'used.jsonl')) if r['iter'] == it]
    return ['evaluators/used.jsonl', f'evaluators/{rows[-1]["version"]}'] if rows else []


def deckchoice_spec(R, path, sha, p, choose=True):
    """The evaluation spec with seat p choosing by the evaluator `path` (the other seat, and with `choose` False seat p
    too, playing its pool's best guess)."""
    from zero import package
    c = R.cfg
    ev = json.loads(c.get('eval_spec') or package.active().eval_spec_of(c['game'], c['spec']))
    dc = deck_choice_of(c['spec']) or {}
    ev['sealed'] = dict(ev['sealed'], best_share=1.0)
    ev['sealed'].pop('deck_choice', None)
    if choose:
        ev['sealed']['deck_choice'] = {'k': int(dc.get('k', 32)), 'explore': 0.0, 'by': 'evaluator',
                                       'evaluator': os.path.abspath(path).replace(os.sep, '/'), 'evaluator_sha': sha,
                                       'seats': [p]}
    return json.dumps(ev)


def play_deckchoice(R, ckpt, path, sha, n, sims, seed, choose=True):
    """`n` games of the checkpoint `ckpt` (its seat choosing by the evaluator, or with `choose` False playing its best
    guess) against the run's yardstick on the evaluation pools -> zero.run.summarize's dict."""
    from zero import games as G
    from zero.infer import drive
    from zero.run import summarize
    c = R.cfg
    rows = []
    evs = {'m': R.evaluator(ckpt)}
    for p in (0, 1):
        k = n // 2 + (n % 2 if p == 0 else 0)
        if k <= 0:
            continue
        a, o = R.nn_seat('m', sims), R.opp_seat(R.yard, sims)
        seats = [[a, o] if p == 0 else [o, a] for _ in range(k)]
        drv = G.Driver(c['game'], seats, spec=deckchoice_spec(R, path, sha, p, choose), concurrency=min(c['conc'], k),
                       seed=seed + p, record=False, threads=c['threads'], groups=c['groups'], max_steps=c['max_steps'],
                       rolling=False, net=0)
        drive(drv, evs, {}, check=lambda: R.check('evaluation'))
        for r in sorted(drv.take_results(), key=lambda r: r['gid']):
            sc = r['scores']
            rows.append((r['result'][p], sc[p] - sc[1 - p], r['rounds'], r['over']))
    return summarize(rows)


def deckchoice_eval(R, it):
    """zero-core's `evals` entry `deckchoice` (module docs): None when off."""
    n = int(R.cfg.get('deckchoice_eval_games', 0) or 0)
    if n <= 0:
        return None
    ev = evaluator_of(R, it)
    if ev is None:
        raise RuntimeError(f'deckchoice: no deck evaluator recorded for iteration {it} (evaluators/used.jsonl)')
    from zero.run import model_path
    path, sha = ev
    s = play_deckchoice(R, model_path(R.run, it), path, sha, n, R.cfg['eval_sims'], 7919 * it + 5)
    return dict(s, value=s['win'], label=f'vs {R.yard}, its deck chosen by the evaluator', opp=R.yard,
                evaluator=os.path.basename(path), sha256=sha[:16], pct=True)


EVALS = {'deckchoice': deckchoice_eval}


# ----------------------------------------------------------------------------- the command line

def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    sub = ap.add_subparsers(dest='cmd', required=True)
    pr = sub.add_parser('retrain', help='one retrain of a run\'s evaluator (the background job)')
    pr.add_argument('run')
    pr.add_argument('--iter', type=int, required=True)
    pr.add_argument('--from-iter', type=int, required=True, help="the window's first iteration")
    pr.add_argument('--val-iters', type=int, default=2)
    pr.add_argument('--kind', default='set', choices=['linear', 'set', 'pair'])
    pr.add_argument('--pair-base', default='linear', choices=['linear', 'set'])
    pr.add_argument('--threads', type=int, default=4)
    pr.add_argument('--half-life', type=float, default=0.0)
    pd = sub.add_parser('deckeval', help='the deck-choice evaluation of any checkpoint')
    pd.add_argument('checkpoint')
    pd.add_argument('--evaluator', required=True, help='a deck evaluator export (mtgz.sealed.evaluator export)')
    pd.add_argument('--games', type=int, default=100)
    pd.add_argument('--sims', type=int, default=None, help='default: the checkpoint run\'s eval_sims')
    pd.add_argument('--seed', type=int, default=5)
    pd.add_argument('--best-guess', action='store_true', help='the network plays its best guess instead (the baseline)')
    pd.add_argument('--config', default=None, help='the run settings (default: config.json beside the checkpoint\'s models/)')
    pd.add_argument('--set', nargs='*', default=[], help='key=value overrides (JSON values), e.g. threads=4 infer_device=cpu')
    pd.add_argument('--k', type=int, default=32)
    a = ap.parse_args(argv)
    if a.cmd == 'retrain':
        def log(s):
            print(s, flush=True)
        st = retrain(a.run, a.iter, a.from_iter, a.val_iters, a.kind, a.threads, a.half_life, a.pair_base, log=log)
        print(json.dumps({k: v for k, v in st.items() if k != 'export_check'}))
        return st
    import mtgz.az  # noqa: F401
    from zero.run import Run
    from . import scorer as S
    cfgp = a.config or os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(a.checkpoint))), 'config.json')
    with open(cfgp, encoding='utf-8') as f:
        cfg = json.load(f)
    for kv in a.set:
        k, _, v = kv.partition('=')
        try:
            cfg[k] = json.loads(v)
        except ValueError:
            cfg[k] = v
    import tempfile
    sp = json.loads(cfg['spec'])
    sp['sealed']['deck_choice'] = {'k': a.k, 'explore': 0.0, 'by': 'evaluator', 'evaluator': a.evaluator}
    cfg['spec'] = json.dumps(sp)
    # (a scratch directory as the Run's: the checkpoint's run is only read)
    R = Run(tempfile.mkdtemp(prefix='mtgz-deckeval-'), cfg)
    sha = S.sha256_file(a.evaluator)
    t0 = time.time()
    s = play_deckchoice(R, a.checkpoint, a.evaluator, sha, a.games, a.sims or cfg['eval_sims'], a.seed, not a.best_guess)
    out = dict(s, checkpoint=a.checkpoint, evaluator=a.evaluator, sha256=sha[:16], arm='best_guess' if a.best_guess else
               'evaluator', seconds=round(time.time() - t0, 1))
    print(json.dumps(out))
    return out


if __name__ == '__main__':
    main(sys.argv[1:])
