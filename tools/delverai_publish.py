"""Publish a run to delverai.greggg230.com: its progress (every pass) and games of its latest
generation (once per new checkpoint).

  python tools/delverai_publish.py runs/delver1 [--loop 300] [--games 6] [--device cuda]
      [--url https://delverai.greggg230.com] [--key-file F] [--dry-run]

Each pass:
  * progress: the run's metrics (metrics.jsonl with evals.jsonl merged, as zerodash), Elo
    ratings, best checkpoint, run state (running / paused / ...), settings and the decklist,
    POSTed to <url>/api/progress/upload;
  * games: when models/ holds a checkpoint newer than the last one recorded (and its file has
    settled), `mtgz.az.replays` records `--games` games of it against itself with the run's
    self-play simulations (evaluation settings: no noise, the most visited option) and uploads
    them. Only the newest checkpoint is recorded; ones skipped while the publisher was down are
    not back-filled. The recorded iterations are kept in <run>/delverai.json.

`--loop N`: repeat every N seconds (default: one pass). The upload key: --key-file,
$MTGZ_DELVERAI_KEY or ~/delverai/.upload_key (the site's Worker secret UPLOAD_KEY).
Recording uses the GPU next to the run (~a minute per generation); `--device cpu` keeps off it.
"""
import argparse
import json
import os
import re
import sys
import time
import traceback

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from mtgz.az import decklists, replays  # noqa: E402
from zero import runctl  # noqa: E402

SETTINGS = ('preset', 'iters', 'games', 'sims', 'sims_fast', 'p_full', 'sims_minor', 'temp_moves', 'arch', 'hidden',
            'blocks', 'heads', 'ff', 'policy_target', 'eval_games', 'eval_sims', 'eval_random_games', 'elo_games',
            'elo_sims', 'eval_every', 'yardstick', 'window', 'lr', 'seed')


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
    out = []
    for m in _jsonl(os.path.join(run, 'metrics.jsonl')):
        e = evals.get(m.get('iter'), {})
        y, rnd, elo = m.get('eval_yard') or e.get('eval_yard'), m.get('eval_random') or e.get('eval_random'), \
            m.get('elo') or e.get('elo')
        sp, tr = m.get('selfplay') or {}, m.get('train') or {}
        out.append({
            'iter': m.get('iter'), 'time': m.get('time'), 'seconds': _r(m.get('seconds'), 1),
            'elo': _r((elo or {}).get('elo'), 1), 'elo_se': _r((elo or {}).get('se'), 1),
            'yard_win': _r((y or {}).get('win')), 'yard_se': _r((y or {}).get('se')), 'yard_games': (y or {}).get('games'),
            'yard_margin': _r((y or {}).get('margin'), 2), 'yard_turns': _r((y or {}).get('rounds'), 2),
            'rand_win': _r((rnd or {}).get('win')), 'rand_se': _r((rnd or {}).get('se')), 'rand_games': (rnd or {}).get('games'),
            'sp_games': sp.get('games'), 'sp_samples': sp.get('samples'), 'sp_turns': _r(sp.get('rounds'), 2),
            'sp_decisions': _r(sp.get('decisions_per_game'), 1), 'sp_games_per_h': _r(sp.get('games_per_h'), 0),
            'sp_unfinished': sp.get('unfinished'),
            'policy_loss': _r(tr.get('policy')), 'value_loss': _r(tr.get('value')), 'entropy': _r(tr.get('entropy')),
            'value_acc': _r(tr.get('value_acc')), 'phases': {k: _r(v, 1) for k, v in (m.get('phases') or {}).items()},
        })
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


def progress(run):
    """The site's progress record of a run."""
    cfg = _json(os.path.join(run, 'config.json'), {}) or {}
    st = runctl.run_status(run)
    prog = _json(os.path.join(run, 'progress.json'))
    idx = _json(os.path.join(run, 'models', 'index.json'), {}) or {}
    ratings = (_json(os.path.join(run, 'elo', 'ratings.json'), {}) or {})
    log = os.path.join(run, 'log.txt')
    return {
        'run': os.path.basename(os.path.abspath(run)), 'updated': time.time(),
        'state': st.get('state'), 'detail': st.get('detail'), 'paused_since': st.get('paused_since'),
        'progress': prog, 'log_age_s': _r(time.time() - os.path.getmtime(log), 0) if os.path.exists(log) else None,
        'settings': {k: cfg[k] for k in SETTINGS if k in cfg}, 'deck': deck_of(cfg),
        'iters': iteration_rows(run), 'best': idx.get('best'),
        'ratings': {k: {'elo': _r(v.get('elo'), 1), 'se': _r(v.get('se'), 1), 'games': v.get('games')}
                    for k, v in (ratings.get('ratings') or {}).items()},
        'anchor': {'name': ratings.get('anchor'), 'elo': ratings.get('anchor_elo')},
        'replayed': (_json(os.path.join(run, 'delverai.json'), {}) or {}).get('replayed', []),
    }


def latest_checkpoint(run, settle_s=30):
    """(iteration, path) of the newest models/iter_NNNN.pt whose file has settled, else None."""
    best = None
    d = os.path.join(run, 'models')
    for f in os.listdir(d) if os.path.isdir(d) else []:
        m = re.fullmatch(r'iter_(\d+)\.pt', f)
        if m and (best is None or int(m.group(1)) > best[0]):
            best = (int(m.group(1)), os.path.join(d, f))
    if best and time.time() - os.path.getmtime(best[1]) < settle_s:
        return None
    return best


def one_pass(run, a, key, log=print):
    p = progress(run)
    if a.dry_run:
        log(json.dumps({k: v for k, v in p.items() if k != 'iters'})[:800])
        log(f'{len(p["iters"])} iteration rows')
    else:
        replays.post(a.url.rstrip('/') + '/api/progress/upload', json.dumps(p).encode('utf-8'), key)
    ck = latest_checkpoint(run)
    state_path = os.path.join(run, 'delverai.json')
    state = _json(state_path, {}) or {}
    done = state.get('replayed', [])
    if ck is None or ck[0] in done or (a.min_iter is not None and ck[0] < a.min_iter):
        return False
    it, path = ck
    log(f'recording {a.games} games of iteration {it} ({path})')
    reps = replays.record(path, games=a.games, sims=a.sims, device=a.device,
                          title=f'Generation {it}', log=log)
    out = os.path.join(run, 'replays')
    for r in reps:
        replays.core.save(r, out)
        if a.dry_run:
            log(f'  (dry run) {replays.summary(r)}')
        else:
            log(f'  uploaded {replays.upload(r, a.url, key)}')
    state['replayed'] = sorted(set(done) | {it})
    with open(state_path + '.tmp', 'w', encoding='utf-8') as f:
        json.dump(state, f)
    os.replace(state_path + '.tmp', state_path)
    if not a.dry_run:     # the progress record lists the recorded generations
        p['replayed'] = state['replayed']
        replays.post(a.url.rstrip('/') + '/api/progress/upload', json.dumps(p).encode('utf-8'), key)
    return True


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('run')
    ap.add_argument('--loop', type=float, default=None, help='repeat every N seconds')
    ap.add_argument('--games', type=int, default=6)
    ap.add_argument('--sims', type=int, default=None, help="default: the run's self-play sims")
    ap.add_argument('--device', default=None, help='default: cuda when available')
    ap.add_argument('--min-iter', type=int, default=None, help='record no checkpoint older than this')
    ap.add_argument('--url', default=replays.SITE)
    ap.add_argument('--key-file', default=None)
    ap.add_argument('--dry-run', action='store_true', help='record, but upload nothing')
    a = ap.parse_args(argv)
    if not os.path.isdir(a.run):
        raise SystemExit(f'{a.run}: no such run directory')
    key = None if a.dry_run else replays.upload_key(a.key_file)

    def log(msg):
        print(f'{time.strftime("%H:%M:%S")} {msg}', flush=True)

    while True:
        try:
            one_pass(a.run, a, key, log)
        except Exception:                  # a network hiccup or a half-written file: try again next pass
            if a.loop is None:
                raise
            log('pass failed:\n' + traceback.format_exc())
        if a.loop is None:
            break
        time.sleep(a.loop)


if __name__ == '__main__':
    main()
