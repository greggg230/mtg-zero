"""The matchup model's games from the run's game records (docs/ENGINE.md 273): every self-play game an iteration's quota
accepted - the desktop's driver's, a remote host's workers', the farm's - as the best-of-three game log's lines
(docs/ENGINE.md 168.2), so the matchup model (mtgz.az.matchup) reads farm self-play as it reads local self-play.

**Why.** The game log (`<run>/games/games-<pid>.jsonl`) is written by the engine of the run's own process: only its local
self-play logs. A run whose self-play is on the farm (`selfplay_local` 0: the stew's specialists) logs no game, and its
matchup model starved (6,411 games, then none: "no logged self-play game"). Its game records (zero-core `game_records`,
`<run>/records/iter_NNNN/*.jsonl.gz` + DONE.json, zero/gamerec.py) hold every accepted game, local and farm.

**How.** Each record - a best-of-three match recorded as one chained contest (zero-core records/1: one episode per
game, `{"next": seat}` boundaries in its steps, docs/ENGINE.md 121) - is replayed by tools/gamedb.py's own replay
(`replay_match`, light: every check it makes - each episode's end, a concession, the match's result, the game log's 60s
against the match block's - a mismatch raising) with the engine's game log on for that match alone; the lines the
engine writes are the lines the live match wrote to its process's game log (the same engine: `MatchState::end_game`
writes them, a chained match's next episode carries the log on). They become game dicts exactly as the log's
(`mtgz.az.matchup.game_line`: the same filters - aborted games and specs without dealt decks left out), with `iter` the
record's accepting iteration and `time` the record's own (`t`; the replay's wall clock is not the game's).

**The engine.** A record replays only on the engine that played it (gamedb's identity check, docs/gamedb.md "The
engine": every records file's build line). An iteration whose records another engine played (an earlier pin: the run
re-pinned since) is EXCLUDED, never replayed on the wrong engine: `games_of` returns it with the reason, its matches and
games counted, and the caller says so (the version's `data`, its report, the run log). Excluding rather than refusing
the whole window: a re-pin would otherwise starve the model for a whole window (the very failure this fixes); the left-out
share is exact and said every time.

**Cost.** A light replay is ~5-15 ms a game (tools/gamedb.py bench); `jobs` > 1 replays in that many processes at
below-normal priority (gamedb ingest's). An iteration's replayed lines are cached (`cache`: a directory, one
`iter_NNNN.json.gz` each, keyed by its DONE.json, the engine and this module's format), so a rolling retrain replays each
iteration once.
"""
import gzip
import hashlib
import json
import os
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
FORMAT = 'mtgz-bo3-records/1'
CHUNK = 32                     # records per replay task


def gamedb():
    """tools/gamedb.py (its replay and its engine identity: the code this module reuses)."""
    for p in (os.path.join(ROOT, 'tools'), ROOT):
        if p not in sys.path:
            sys.path.insert(0, p)
    import gamedb as GDB
    return GDB


def episodes_of(rec):
    """The number of games (episodes) a match record holds: its `episodes`, else its episode boundaries + 1."""
    if rec.get('episodes'):
        return len(rec['episodes'])
    if rec.get('actions') is not None:
        return 1
    return 1 + sum(1 for a in rec.get('steps') or [] if isinstance(a, dict))


def replay_lines(items):
    """[(record, spec JSON)] -> [each record's game log lines] (a process pool's task; module docs). A record that is not
    a chained best-of-three match, or that does not replay, raises (never dropped)."""
    GDB = gamedb()
    import mtgz.az  # noqa: F401
    from mtgz import _engine as E
    import combo_stats as C
    out = []
    for rec, spec_txt in items:
        spec = json.loads(spec_txt)
        where = f'iteration {rec.get("it")} source {rec.get("src")} match {rec.get("gid")} (seed {rec.get("seed")})'
        if not GDB.chained(rec, spec):
            raise ValueError(f'{where}: not a best-of-three match recorded as one chained contest (the spec\'s '
                             f'game_episodes, docs/ENGINE.md 121): the matchup model reads chained matches only')
        decks, mains = GDB.seat_decks(spec, rec.get('setup'))
        lines = []
        try:
            GDB.replay_match(E, C, spec_txt, rec, decks, mains, False, glog_out=lines)
        except Exception as e:                 # (loud: the caller's retrain fails with it)
            raise RuntimeError(f'{where}: {type(e).__name__}: {e}') from e
        out.append(lines)
    return out


def _worker_init():
    gamedb()._worker_init()


def judge_iteration(run, it, mine):
    """Iteration `it`'s records against this checkout's engine -> (specs, records, None) or (None, records, the
    exclusion {'why', 'engine', 'commit', 'core'}) (gamedb's `judge`)."""
    from zero import gamerec as GR
    GDB = gamedb()
    fb = {}
    got = GR.read_iteration(run, it, fb)
    if got is None:
        raise ValueError(f'iteration {it}: no complete records (no DONE.json)')
    specs, recs = got
    builds = [b for name in sorted(fb) for b in (fb[name] or [None])]
    builds = [b for b in builds if b is not None] if all(fb.values()) else []
    ok, why, b = GDB.judge(builds, mine)
    if not ok:
        return None, recs, {'why': why, 'engine': b.get('engine'), 'commit': b.get('commit'), 'core': b.get('core'),
                            'hint': GDB.engine_hint(b) if why == 'other_engine' else None}
    return specs, recs, None


def _cache_key(run, it, engine):
    from zero import gamerec as GR
    with open(os.path.join(GR.iter_dir(run, it), GR.DONE), 'rb') as f:
        done = f.read()
    return hashlib.sha256(f'{FORMAT}|{engine}|'.encode() + done).hexdigest()[:24]


def _cache_path(cache, it):
    return os.path.join(cache, f'iter_{it:04d}.json.gz')


def _read_cache(cache, it, key):
    if not cache:
        return None
    p = _cache_path(cache, it)
    try:
        with gzip.open(p, 'rt', encoding='utf-8') as f:
            x = json.load(f)
    except (OSError, ValueError, EOFError):
        return None
    return x if x.get('format') == FORMAT and x.get('key') == key else None


def _write_cache(cache, it, x):
    os.makedirs(cache, exist_ok=True)
    p = _cache_path(cache, it)
    tmp = p + f'.{os.getpid()}.tmp'
    with gzip.open(tmp, 'wt', encoding='utf-8', compresslevel=6) as f:
        json.dump(x, f, separators=(',', ':'))
    os.replace(tmp, p)


def prune_cache(cache, lo):
    """Delete the cached iterations before `lo` (out of every later window)."""
    if not cache or not os.path.isdir(cache):
        return
    for name in os.listdir(cache):
        if name.startswith('iter_') and name.endswith('.json.gz'):
            try:
                if int(name[5:9]) < lo:
                    os.remove(os.path.join(cache, name))
            except (ValueError, OSError):
                pass


def games_of(run, its, jobs=1, cache=None, log=None):
    """The record iterations `its` of `run` -> {it: {'matches', 'games' (episodes), 'missing' (DONE.json's accepted games
    without a record), and either 'lines': [{'gid', 'src', 't', 'lines'}] per match in record order, 'engine', 'cached',
    or 'excluded': {'why', 'engine', 'commit', 'core', 'hint'}}} (module docs). Replays in `jobs` processes."""
    from zero import gamerec as GR
    GDB = gamedb()
    log = log or (lambda m: None)
    mine = GDB.my_build()
    out, todo = {}, []
    for it in its:
        specs, recs, ex = judge_iteration(run, it, mine)
        done = GR.read_done(run, it) or {}
        o = {'matches': len(recs), 'games': sum(episodes_of(r) for r in recs), 'missing': done.get('missing')}
        out[it] = o
        if ex is not None:
            o['excluded'] = ex
            continue
        key = _cache_key(run, it, mine.get('engine'))
        c = _read_cache(cache, it, key)
        if c is not None:
            o.update(lines=c['matches'], engine=c['engine'], cached=True)
            continue
        o.update(engine=mine.get('engine'), cached=False, _key=key)
        todo.append((it, [(r, specs[r['spec']]) for r in recs]))
    tasks = [(it, k, items[k:k + CHUNK]) for it, items in todo for k in range(0, len(items), CHUNK)]
    if tasks:
        t0 = time.time()
        n = sum(len(t[2]) for t in tasks)
        log(f'# replaying {n} matches of {len(todo)} iterations ({jobs} process{"es" if jobs > 1 else ""})')
        if jobs > 1:
            from concurrent.futures import ProcessPoolExecutor
            with ProcessPoolExecutor(jobs, initializer=_worker_init) as pool:
                res = list(pool.map(replay_lines, [t[2] for t in tasks]))
        else:
            res = [replay_lines(t[2]) for t in tasks]
        by = {}
        for (it, k, items), lines in zip(tasks, res):
            by.setdefault(it, []).extend({'gid': r.get('gid'), 'src': str(r.get('src')), 't': r.get('t'), 'lines': ls}
                                         for (r, _), ls in zip(items, lines))
        for it, _ in todo:
            o = out[it]
            o['lines'] = by.get(it, [])
            if cache:
                _write_cache(cache, it, {'format': FORMAT, 'key': o['_key'], 'it': it, 'engine': o['engine'],
                                         'matches': o['lines'], 'time': time.time()})
        log(f'# replayed {n} matches in {time.time() - t0:.1f} s')
    for o in out.values():
        o.pop('_key', None)
    return out
