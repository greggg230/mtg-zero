"""Publish a run to delverai.greggg230.com: its progress (every pass) and games of its latest
generation (once per new checkpoint).

  python tools/delverai_publish.py runs/delver1 [--loop 300] [--games 6] [--device cuda]
      [--url https://delverai.greggg230.com] [--key-file F] [--dry-run]

Each pass:
  * progress: the run's metrics (metrics.jsonl with evals.jsonl merged, as zerodash), Elo
    ratings, best checkpoint, run state (running / paused / ...), settings and the decklist, and
    what is happening now (`now`: the step the run is in and since when, self-play's games so far,
    an evaluation in progress, this publisher's recording, the last durations of each step for
    the site's estimates), POSTed to <url>/api/progress/upload; with --loop, from its own thread
    every --progress-every seconds (60), recording or not;
  * games: when models/ holds a checkpoint newer than the last one recorded (and its file has
    settled; with `--every N` only iterations divisible by N count), `mtgz.az.replays` records
    `--games` games of it against itself with the run's
    self-play simulations (evaluation settings: no noise, the most visited option) and uploads
    them. Only the newest checkpoint is recorded; ones skipped while the publisher was down are
    not back-filled. The recorded iterations are kept in <run>/delverai.json.

`--loop N`: repeat every N seconds (default: one pass), waiting for a run that has not started
yet. `--follow STATE`: publish the run a JSON file names as its "main" (the A/B experiments'
runs/ab/state.json: the main line moves to a fork's directory when the fork wins), read at every
pass; the run argument is the fallback.

Head-to-heads (`h2h` in each iteration row): the evaluated generation's own games in its Elo pool
(`<run>/elo/<select tag>/results.json`, zero-core docs/az.md 5.4 and 20.1): against the generations
1, 2, 4, 8 back (elo_offsets) and the pool's fixed anchors, each {back, iter} or {anchor, iter: the
generation it is a copy of (the run's checkpoint with its sha256), elo}, with the generation's
points, the games and its win rate. The pool's other games (the bots', the anchors' calibration,
the matches where it is the earlier generation) are not its own. A bot or random evaluation of 0
games (--eval-games 0) measured nothing: its yard_win / rand_win are None.

Lineage (`lineage` in the progress record): the run's history since its first ancestor, for the
site's charts from the beginning. A fork (the A/B experiments' `done` entries of --follow's file:
the fork's directory, the run it forked from and at which iteration, its changed setting) keeps
its parent's numbering (`--forks FILE`, repeatable: more such state files, e.g. an earlier round of
experiments whose forks are still in the main line's history); a migration (MIGRATED_FROM.json: a layout widen) starts again at 0 with
the source's checkpoint, so its rows get a global generation = the source's + its own. Each
segment's rows are read from its own run directory, up to where its child took over. The upload key: --key-file,
$MTGZ_DELVERAI_KEY or ~/delverai/.upload_key (the site's Worker secret UPLOAD_KEY).
Recording runs on the CPU by default (`--threads` torch threads): zero.replay's Python driver
sends batches of a few positions, which on a GPU shared with the run (and other projects) wait
behind its kernels (measured 2026-10-28: ~70 ms a batch on the busy GPU; on 6 CPU threads two
mtg5-network Delver games of 36 and 45 turns took 5.5 min).
"""
import argparse
import json
import os
import re
import sys
import threading
import time
import traceback

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from mtgz.az import decklists, replays  # noqa: E402
from zero import elo as zelo, runctl  # noqa: E402

SETTINGS = ('preset', 'iters', 'games', 'sims', 'sims_fast', 'p_full', 'sims_minor', 'temp_moves', 'arch', 'hidden',
            'blocks', 'heads', 'ff', 'policy_target', 'eval_games', 'eval_sims', 'eval_random_games', 'elo_games',
            'elo_sims', 'eval_every', 'yardstick', 'window', 'lr', 'seed', 'eval_select', 'eval_select_min_visits',
            'elo_offsets', 'elo_opponents', 'elo_anchor_games', 'elo_anchor_from')


def _json(path, default=None):
    try:
        with open(path, encoding='utf-8') as f:
            return json.load(f)
    except (OSError, ValueError):
        return default


def _jsonl(path):
    rows = []
    if os.path.exists(path):
        with open(path, encoding='utf-8') as f:
            for line in f:
                try:
                    rows.append(json.loads(line))
                except ValueError:
                    pass                          # a line being written
    return rows


def _r(x, n=4):
    return round(x, n) if isinstance(x, (int, float)) else None


def iteration_rows(run):
    """metrics.jsonl (remote evaluations merged) -> one compact row per iteration."""
    evals = {e['iter']: e for e in _jsonl(os.path.join(run, 'evals.jsonl')) if 'iter' in e}
    pools = {}                                    # select tag -> {iteration: its head-to-heads}
    out = []
    for m in _jsonl(os.path.join(run, 'metrics.jsonl')):
        e = evals.get(m.get('iter'), {})
        y, rnd, elo = m.get('eval_yard') or e.get('eval_yard'), m.get('eval_random') or e.get('eval_random'), \
            m.get('elo') or e.get('elo')
        # the evaluation move rule that rated it (zero-core docs/az.md 5.4: each rule its own Elo pool)
        tag = (elo or {}).get('select') or (y or {}).get('select') or 'visits'
        if tag not in pools:
            pools[tag] = head_to_heads(run, tag)
        # 0 games against the bot or random (--eval-games 0: no yardstick) measured nothing
        ym, rm = (y if (y or {}).get('games') else None), (rnd if (rnd or {}).get('games') else None)
        sp, tr = m.get('selfplay') or {}, m.get('train') or {}
        out.append({
            'iter': m.get('iter'), 'time': m.get('time'), 'seconds': _r(m.get('seconds'), 1),
            'elo': _r((elo or {}).get('elo'), 1), 'elo_se': _r((elo or {}).get('se'), 1),
            'select': tag,
            'yard_win': _r((ym or {}).get('win')), 'yard_se': _r((ym or {}).get('se')), 'yard_games': (y or {}).get('games'),
            'yard_margin': _r((ym or {}).get('margin'), 2), 'yard_turns': _r((ym or {}).get('rounds'), 2),
            'rand_win': _r((rm or {}).get('win')), 'rand_se': _r((rm or {}).get('se')), 'rand_games': (rnd or {}).get('games'),
            'h2h': pools[tag].get(m.get('iter'), []),
            'sp_games': sp.get('games'), 'sp_samples': sp.get('samples'), 'sp_turns': _r(sp.get('rounds'), 2),
            'sp_decisions': _r(sp.get('decisions_per_game'), 1), 'sp_games_per_h': _r(sp.get('games_per_h'), 0),
            'sp_unfinished': sp.get('unfinished'),
            'policy_loss': _r(tr.get('policy')), 'value_loss': _r(tr.get('value')), 'entropy': _r(tr.get('entropy')),
            'value_acc': _r(tr.get('value_acc')), 'phases': {k: _r(v, 1) for k, v in (m.get('phases') or {}).items()},
            'eval_s': _eval_seconds(m, e),
        })
    return out


def _iter_of(name):
    m = re.fullmatch(r'iter_(\d+)', name)
    return int(m.group(1)) if m else None


_SHA = {}


def _sha256(path):
    """A file's sha256, cached by its size and mtime (each checkpoint hashed once per process)."""
    import hashlib
    st = os.stat(path)
    key = (st.st_size, st.st_mtime_ns)
    got = _SHA.get(path)
    if got and got[0] == key:
        return got[1]
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    _SHA[path] = (key, h.hexdigest())
    return h.hexdigest()


def anchor_copies(run, anchors):
    """{anchor player: the run's iteration whose checkpoint has the anchor's sha256, else None}.
    The candidates: the numbers in the anchor's label and file name, and the iteration in its path
    in the run's elo_anchors ('a192=.../r3-2b_iter_0192.pt' -> 192)."""
    cfg = _json(os.path.join(run, 'config.json'), {}) or {}
    try:
        paths = {zelo.anchor_name(lb): p for lb, p in zelo.parse_anchors(cfg.get('elo_anchors', ''))}
    except ValueError:
        paths = {}
    out = {}
    for name, a in anchors.items():
        cands = [int(x) for s in (a.get('label'), a.get('file')) for x in re.findall(r'\d+', str(s or ''))]
        cands += [int(x) for x in re.findall(r'iter_(\d+)', str(paths.get(name, '')))]
        out[name] = None
        for k in dict.fromkeys(cands):
            f = os.path.join(run, 'models', f'iter_{k:04d}.pt')
            try:
                if a.get('sha256') and os.path.exists(f) and _sha256(f) == a['sha256']:
                    out[name] = k
                    break
            except OSError:
                pass
    return out


def head_to_heads(run, tag='visits'):
    """{iteration: its own games in the Elo pool `tag`}: against an earlier generation
    {back, iter} (by back) and against each fixed anchor {anchor (its label), iter (the generation
    it is a copy of, or None), elo (its fixed rating), sha} (in the pool's order), each with the
    generation's points, the games and its win rate. Not its own: the bots' games, the anchors'
    calibration, the matches in which it is the earlier generation (a later one's)."""
    res = _json(os.path.join(zelo.pool_dir(run, tag), 'results.json'), {}) or {}
    anchors = res.get('anchors') or {}
    order = {n: i for i, n in enumerate(anchors)}
    copies = anchor_copies(run, anchors) if anchors else {}
    out = {}
    for key, v in (res.get('pairs') or {}).items():
        a, _, b = key.partition('|')
        try:
            pa, n = float(v[0]), int(v[1])
        except (TypeError, ValueError, IndexError):
            continue
        if n <= 0:
            continue
        ia, ib = _iter_of(a), _iter_of(b)
        if ia is not None and ib is not None and ia != ib:
            k, j = max(ia, ib), min(ia, ib)
            pts = pa if k == ia else n - pa       # the pair is [points of a, games]
            m = {'back': k - j, 'iter': j}
        elif (ia is None) != (ib is None) and (a if ia is None else b) in anchors:
            k, opp = (ib, a) if ia is None else (ia, b)
            pts = n - pa if ia is None else pa
            an = anchors[opp]
            m = {'anchor': an.get('label') or opp, 'iter': copies.get(opp), 'elo': _r(an.get('elo'), 1),
                 'sha': (an.get('sha256') or '')[:12] or None, '_order': order[opp]}
        else:
            continue
        m.update(points=_r(pts, 2), games=n, win=_r(pts / n))
        out.setdefault(k, []).append(m)
    for ms in out.values():
        ms.sort(key=lambda m: (1, m['_order']) if 'anchor' in m else (0, m['back']))
        for m in ms:
            m.pop('_order', None)
    return out


def _eval_seconds(m, e):
    """How long the iteration's evaluation took: a remote one's (evals.jsonl), else the run's own
    eval + Elo phases."""
    if isinstance(e.get('seconds'), (int, float)):
        return _r(e['seconds'], 1)
    ph = m.get('phases') or {}
    if 'eval_s' in ph or 'elo_s' in ph:
        return _r((ph.get('eval_s') or 0) + (ph.get('elo_s') or 0), 1)
    return None


# This publisher's recording in progress (set by one_pass, read by the progress thread).
RECORDING = {}


def _last(rows, f):
    """The most recent positive value of f(row)."""
    for r in reversed(rows):
        v = f(r)
        if isinstance(v, (int, float)) and v > 0:
            return v
    return None


def now_record(run, cfg, prog, rows, state):
    """What the run is doing now, for the site's progress bars. Self-play reports its games;
    the other steps report only when they began, and the site estimates them from `est`, the
    last durations of each step."""
    prog = prog or {}
    rs = _json(os.path.join(run, 'run_state.json'), {}) or {}
    last = rows[-1] if rows else None
    out = {'phase': prog.get('phase'), 'iter': prog.get('iter'), 'at': prog.get('time')}
    # Self-play's start: the previous iteration's row (written as its training ended), or the
    # run's (re)start, whichever is later.
    begun = max([t for t in ((last or {}).get('time'), rs.get('started')) if isinstance(t, (int, float))], default=None)
    if prog.get('phase') == 'selfplay':
        out['selfplay'] = {'games': prog.get('games'), 'target': prog.get('target'), 'since': begun}
        if prog.get('remote'):             # remote self-play (zero-core remoteplay): its games this iteration
            r = prog['remote']
            out['selfplay']['remote'] = {k: r.get(k) for k in ('host', 'workers', 'state', 'games')}
        if 'local' in prog:                # the shared queue: games / target are both machines', local this one's
            out['selfplay']['local'] = prog['local']
    elif prog.get('phase') in ('train', 'eval', 'remote shards'):
        out['since'] = prog.get('time')
    # The latest checkpoint's evaluation while it runs (a remote one runs beside the next
    # self-play; a local one is the run's 'eval' phase).
    every = int(cfg.get('eval_every') or 1)
    if last and last.get('elo') is None and last.get('yard_games') is None and last['iter'] % every == 0:
        if cfg.get('eval_remote'):
            out['evaluating'] = {'iter': last['iter'], 'since': last.get('time'), 'host': cfg['eval_remote']}
        elif prog.get('phase') == 'eval':
            out['evaluating'] = {'iter': last['iter'], 'since': prog.get('time'), 'host': None}
    if RECORDING:
        out['recording'] = dict(RECORDING)
    out['est'] = {
        'selfplay_s': _last(rows, lambda r: (r.get('phases') or {}).get('selfplay_s')),
        'train_s': _last(rows, lambda r: (r.get('phases') or {}).get('train_s')),
        'eval_s': _last(rows, lambda r: r.get('eval_s')),
        'record_s': (state.get('record_s') or [None])[-1],
    }
    return out


def deck_of(cfg):
    """The decklist the run's spec plays (both seats: the mirror), with its provenance."""
    try:
        spec = json.loads(cfg.get('spec') or '{}')
    except ValueError:
        return None
    if not spec.get('constructed'):
        return None
    d = decklists.load(decklists.IZZET_DELVER)
    main = sorted(set(spec['decks']['p0']), key=spec['decks']['p0'].index)
    counts = {n: spec['decks']['p0'].count(n) for n in main}
    same = counts == {n: c for c, n in d['main']}
    info = {k: d[k] for k in ('name', 'player', 'event', 'date', 'result', 'source', 'platform') if k in d} if same else {}
    info['main'] = [[counts[n], n] for n in main]
    info['mirror'] = spec['decks']['p0'] == spec['decks']['p1']
    info['format'] = spec.get('format', 'bo1')
    info['max_turns'] = spec.get('max_turns')
    return info


def _norm(path):
    return os.path.normcase(os.path.abspath(path))


def _label(flag, name=None):
    """A fork's label on the charts: its one changed setting ('--value-q 0.6' -> 'value_q 0.6'), or
    the experiment's name when it changes several (theirs would run over the other labels)."""
    parts = str(flag or '').split()
    if name and sum(x.startswith('--') for x in parts) > 1:
        return str(name)
    return ' '.join([parts[0].lstrip('-').replace('-', '_')] + parts[1:]) if parts else str(name or flag)


def _run_label(run, name=None):
    """A run's label from its directory ('r3-2b-beliefsearch' -> 'beliefsearch'), else `name`."""
    m = re.match(r'r\d+-\d+[a-z]?-(.+)$', os.path.basename(os.path.normpath(run)))
    return m.group(1) if m else str(name or os.path.basename(run))


def fork_registry(paths):
    """The A/B experiments' forks (their state files' `done`): fork run directory ->
    {parent, at, label, won}. A test's arm A (raw.a's run) forked from main_before at k - its
    first record only: a later test of the same run's checkpoint (an evaluation-only head-to-head)
    is not where it forked. An adoption whose main_after is a new run (neither main_before nor the
    arm) started that run from the tested checkpoint: a fork of the arm at that iteration."""
    out = {}
    done = [d for path in paths if path for d in ((_json(path, {}) or {}).get('done') or [])]
    for d in done:
        a = (d.get('raw') or {}).get('a')
        if not a or d.get('k') is None or not d.get('main_before'):
            continue
        a = a.replace('\\', '/')
        fork = os.path.dirname(os.path.dirname(a))
        if _norm(fork) not in out and _norm(fork) != _norm(d['main_before']):
            out[_norm(fork)] = {'parent': d['main_before'], 'at': int(d['k']), 'label': _label(d.get('flag'), d.get('name')),
                                'won': _norm(d.get('main_after') or '') == _norm(fork)}
        after = d.get('main_after')
        m = re.search(r'iter_(\d+)\.pt$', a)
        if after and m and _norm(after) not in (_norm(fork), _norm(d['main_before'])) and _norm(after) not in out:
            out[_norm(after)] = {'parent': fork, 'at': int(m.group(1)), 'label': _run_label(after, d.get('name')), 'won': True}
    return out


def lineage(run, forks, depth=0):
    """The run's segments from its first ancestor: [{dir, run, label, kind (root / fork /
    migrate), first, last (its own iterations: None = to the end), offset (global generation =
    offset + its iteration)}]."""
    name = os.path.basename(os.path.abspath(run))
    if depth > 20:
        return [{'dir': run, 'run': name, 'label': name, 'kind': 'root', 'first': 0, 'last': None, 'offset': 0}]
    mig = _json(os.path.join(run, 'MIGRATED_FROM.json'))
    if isinstance(mig, dict) and mig.get('migrated_from'):
        src = mig['migrated_from'].replace('\\', '/')
        m = re.search(r'iter_(\d+)\.pt$', src)
        src_run = os.path.dirname(os.path.dirname(src))
        if m and os.path.isdir(src_run):
            k = int(m.group(1))
            segs = _cut(lineage(src_run, forks, depth + 1), k)
            layout = str(mig.get('layout') or '')
            lab = 'layout ' + layout.split('->')[-1].split('(')[0].strip() if '->' in layout else 'widened'
            return segs + [{'dir': run, 'run': name, 'label': lab, 'kind': 'migrate', 'first': 0, 'last': None,
                            'offset': segs[-1]['offset'] + k}]
    f = forks.get(_norm(run))
    if f and os.path.isdir(f['parent']):
        segs = _cut(lineage(f['parent'], forks, depth + 1), f['at'])
        return segs + [{'dir': run, 'run': name, 'label': f['label'], 'kind': 'fork', 'first': f['at'] + 1, 'last': None,
                        'offset': segs[-1]['offset']}]
    return [{'dir': run, 'run': name, 'label': name, 'kind': 'root', 'first': 0, 'last': None, 'offset': 0}]


def _cut(segs, k):
    """The segments up to the last one's iteration k (where a child took over)."""
    segs = [dict(x) for x in segs]
    segs[-1]['last'] = k
    return segs


def lineage_record(run, forks):
    """The progress record's `lineage`: the segments and every row of them, each with its global
    generation `g`, its run and its own iteration `gen`. None for a run without ancestors."""
    segs = lineage(run, forks)
    if len(segs) < 2:
        return None
    rows = []
    for k, sg in enumerate(segs):
        for r in iteration_rows(sg['dir']):
            it = r.get('iter')
            if not isinstance(it, int) or it < sg['first'] or (sg['last'] is not None and it > sg['last']):
                continue
            # the opponents' global generations too (in the run's own numbering: offset + its iteration)
            h2h = [dict(m, g=None if m.get('iter') is None else sg['offset'] + m['iter']) for m in r.get('h2h') or []]
            rows.append(dict(r, g=sg['offset'] + it, gen=it, run=sg['run'], seg=k, h2h=h2h))
    return {'segments': [{x: sg[x] for x in ('run', 'label', 'kind', 'first', 'last', 'offset')}
                         | {'from_g': sg['offset'] + sg['first']} for sg in segs], 'rows': rows}


def migrated_from(run):
    """A run continuing another's network (MIGRATED_FROM.json, e.g. a layout widen: runs/delver2
    from ab2-cqscale03's iter_0137): {run, iter, layout, time} of the source, else None."""
    m = _json(os.path.join(run, 'MIGRATED_FROM.json'))
    if not isinstance(m, dict) or not m.get('migrated_from'):
        return None
    src = m['migrated_from'].replace(os.sep, '/').replace('\\', '/')
    it = re.search(r'iter_(\d+)\.pt$', src)
    return {'run': os.path.basename(os.path.dirname(os.path.dirname(src))), 'iter': int(it.group(1)) if it else None,
            'layout': m.get('layout'), 'time': m.get('time')}


def progress(run, every=1, forks=None):
    """The site's progress record of a run (`every`: which iterations get games recorded;
    `forks`: fork_registry, for the lineage)."""
    cfg = _json(os.path.join(run, 'config.json'), {}) or {}
    st = runctl.run_status(run)
    prog = _json(os.path.join(run, 'progress.json'))
    idx = _json(os.path.join(run, 'models', 'index.json'), {}) or {}
    tag, pool = zelo.active_pool(run)             # the Elo pool of the run's evaluation move rule now
    ratings = (_json(os.path.join(pool, 'ratings.json'), {}) or {})
    log = os.path.join(run, 'log.txt')
    rows = iteration_rows(run)
    state = _json(os.path.join(run, 'delverai.json'), {}) or {}
    return {
        'run': os.path.basename(os.path.abspath(run)), 'updated': time.time(),
        'state': st.get('state'), 'detail': st.get('detail'), 'paused_since': st.get('paused_since'),
        'progress': prog, 'log_age_s': _r(time.time() - os.path.getmtime(log), 0) if os.path.exists(log) else None,
        'settings': {k: cfg[k] for k in SETTINGS if k in cfg}, 'deck': deck_of(cfg),
        'iters': rows, 'best': idx.get('best'), 'now': now_record(run, cfg, prog, rows, state),
        'ratings': {k: {'elo': _r(v.get('elo'), 1), 'se': _r(v.get('se'), 1), 'games': v.get('games')}
                    for k, v in (ratings.get('ratings') or {}).items()},
        'anchor': {'name': ratings.get('anchor'), 'elo': ratings.get('anchor_elo')}, 'select': tag,
        'replayed': state.get('replayed', []), 'record_every': every, 'migrated_from': migrated_from(run),
        'lineage': lineage_record(run, forks or {}),
    }


def latest_checkpoint(run, every=1, settle_s=30):
    """(iteration, path) of the newest models/iter_NNNN.pt with an iteration divisible by `every`
    whose file has settled, else None."""
    best = None
    d = os.path.join(run, 'models')
    for f in os.listdir(d) if os.path.isdir(d) else []:
        m = re.fullmatch(r'iter_(\d+)\.pt', f)
        if m and int(m.group(1)) % every == 0 and (best is None or int(m.group(1)) > best[0]):
            best = (int(m.group(1)), os.path.join(d, f))
    if best and time.time() - os.path.getmtime(best[1]) < settle_s:
        return None
    return best


def post_progress(run, a, key, log=print):
    p = progress(run, a.every, fork_registry([a.follow] + (a.forks or [])))
    if a.dry_run:
        log(json.dumps({k: v for k, v in p.items() if k != 'iters'})[:800])
        log(f'{len(p["iters"])} iteration rows')
    else:
        replays.post(a.url.rstrip('/') + '/api/progress/upload', json.dumps(p).encode('utf-8'), key)


def one_pass(run, a, key, log=print, kick=None):
    """Record the newest checkpoint's games if they aren't yet. Without a progress thread
    (`kick`, its wake-up event) the progress is posted here, before and after."""
    if kick is None:
        post_progress(run, a, key, log)
    ck = latest_checkpoint(run, a.every)
    state_path = os.path.join(run, 'delverai.json')
    state = _json(state_path, {}) or {}
    done = state.get('replayed', [])
    if ck is None or ck[0] in done or (a.min_iter is not None and ck[0] < a.min_iter):
        return False
    it, path = ck
    log(f'recording {a.games} games of iteration {it} ({path})')
    t0 = time.time()
    RECORDING.update(iter=it, since=t0, games=a.games)
    if kick:
        kick.set()
    try:
        reps = replays.record(path, games=a.games, sims=a.sims, device=a.device,
                              title=f'Generation {it}', log=log)
    finally:
        RECORDING.clear()
    out = os.path.join(run, 'replays')
    for r in reps:
        replays.core.save(r, out)
        if a.dry_run:
            log(f'  (dry run) {replays.summary(r)}')
        else:
            log(f'  uploaded {replays.upload(r, a.url, key)}')
    state['replayed'] = sorted(set(done) | {it})
    state['record_s'] = (state.get('record_s') or [])[-9:] + [round(time.time() - t0, 1)]
    with open(state_path + '.tmp', 'w', encoding='utf-8') as f:
        json.dump(state, f)
    os.replace(state_path + '.tmp', state_path)
    if kick:              # the progress record lists the recorded generations
        kick.set()
    else:
        post_progress(run, a, key, log)
    return True


def current_run(a, log=None):
    """The run to publish: --follow's "main" when it names a run directory, else the run argument."""
    if a.follow:
        main = (_json(a.follow, {}) or {}).get('main')
        if main and os.path.isdir(main):
            if log and os.path.normcase(os.path.abspath(main)) != os.path.normcase(os.path.abspath(a.run)):
                log(f'following the main line: {main}')
            a.run = main
    return a.run


def progress_loop(a, key, log, kick):
    """Post the progress every --progress-every seconds, and at once when kicked."""
    while True:
        kick.wait(a.progress_every)
        kick.clear()
        run = a.run
        if not os.path.exists(os.path.join(run, 'config.json')):
            continue
        try:
            post_progress(run, a, key, log)
        except Exception as e:             # the next post will do
            log(f'progress post failed: {e!r}')


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('run')
    ap.add_argument('--loop', type=float, default=None, help='repeat every N seconds')
    ap.add_argument('--progress-every', type=float, default=60, help='with --loop: seconds between progress posts')
    ap.add_argument('--games', type=int, default=6)
    ap.add_argument('--sims', type=int, default=None, help="default: the run's self-play sims")
    ap.add_argument('--device', default='cpu', help='inference device (default cpu, see above)')
    ap.add_argument('--threads', type=int, default=4, help='torch threads for CPU inference')
    ap.add_argument('--min-iter', type=int, default=None, help='record no checkpoint older than this')
    ap.add_argument('--every', type=int, default=1, help='record only iterations divisible by this (2: even ones)')
    ap.add_argument('--follow', default=None, help='a JSON file whose "main" names the run to publish (runs/ab/state.json)')
    ap.add_argument('--forks', action='append', default=[], help="more A/B state files whose forks are in the lineage (repeatable)")
    ap.add_argument('--url', default=replays.SITE)
    ap.add_argument('--key-file', default=None)
    ap.add_argument('--dry-run', action='store_true', help='record, but upload nothing')
    a = ap.parse_args(argv)
    current_run(a)
    if not os.path.isdir(a.run) and a.loop is None:
        raise SystemExit(f'{a.run}: no such run directory')
    key = None if a.dry_run else replays.upload_key(a.key_file)
    if a.device == 'cpu':
        import torch
        torch.set_num_threads(a.threads)
    # Below the run: recording games must never slow its self-play.
    import psutil
    psutil.Process().nice(psutil.BELOW_NORMAL_PRIORITY_CLASS if os.name == 'nt' else 10)

    def log(msg):
        print(f'{time.strftime("%H:%M:%S")} {msg}', flush=True)

    kick = None
    if a.loop is not None:
        kick = threading.Event()
        threading.Thread(target=progress_loop, args=(a, key, log, kick), daemon=True).start()
        kick.set()
    waiting = False
    while True:
        current_run(a, log)
        if not os.path.exists(os.path.join(a.run, 'config.json')):    # not launched yet (--loop)
            if not waiting:
                log(f'waiting for {a.run} to start')
            waiting = True
            time.sleep(a.loop)
            continue
        waiting = False
        try:
            one_pass(a.run, a, key, log, kick)
        except Exception:                  # a network hiccup or a half-written file: try again next pass
            if a.loop is None:
                raise
            log('pass failed:\n' + traceback.format_exc())
        if a.loop is None:
            break
        time.sleep(a.loop)


if __name__ == '__main__':
    main()
