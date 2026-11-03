"""The canary library's results per generation for delverai.greggg230.com's Progress tab (the user's ask of
11-02: "how many of the blunder cases we passed"). Every evaluated checkpoint of a run (models/iter_N.pt, N a
multiple of its eval_every) is searched on the user-kept spots of the current library with the Play tab's
setup (the user, 11-02: "the same exact set up as the play tab" - the run's evaluation seat at 1024
simulations, the most-visited move; tools/delver_blunders.py library_score, 8 searches a spot, CPU) and a spot
is PASSED when a right answer won at least 5 of its 8 searches. Scores are cached per run and spot
(runs/<run>/blunders/library_scores.json, keyed by the setup): a spot added is scored on every checkpoint, a
spot dropped costs nothing, so every generation is counted on the current set. The site record is `blunders.<run>` (kind
'blunders', delverai worker/index.js): {for_run, library {id, spots, kinds, labels}, sims, repeats,
pass_rule, iters [{iter, passed, total, share, kinds {kind: [passed, n]}}]}.

  python tools/delverai_blunders.py RUN [RUN ...] [--min-iter N] [--max-iter N] [--engine CHECKOUT]
      [--sims 1024] [--select visits] [--upload] [--url https://delverai.greggg230.com] [--key-file F] [--dry-run]

RUN: a run directory or name (runs/<name>). --max-iter bounds a run another one continues (the main line up to
its fork point). The sidecar (`delver_blunders.py watch --site`) calls `sync` after each measurement.

The Blunders tab's games (tools/delverai_blunder_games.py: a generation playing each test case) carry their own
8 searches a spot; their tallies (runs/<run>/blunders/games/iter_NNNN/tally.json) replace the cache's scores of
those generations and spots whenever the cache is read under its lock (`merge_games`), so the Progress tab's
counts and the Blunders tab's verdicts are the same searches. `refresh` merges and uploads without scoring.
"""
import argparse
import hashlib
import json
import os
import re
import sys
import time
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import delver_blunders as B  # noqa: E402
import delver_fingerprints as F  # noqa: E402

PASS = 5                                  # right answers out of REPEATS searches for a spot to pass
REPEATS = 8
SIMS, SELECT = 1024, 'visits'             # the Play tab's (delver-lab play.PLAY_SELECT, its 1024 simulations)
URL = 'https://delverai.greggg230.com'
KEY_ENV = 'MTGZ_DELVERAI_KEY'
KEY_FILE = os.path.join(os.path.expanduser('~'), 'delverai', '.upload_key')
# the chart's caveat (the site shows it under the chart): where the positions come from
NOTE = ("Most positions were collected from generation 550's own games, where it made these mistakes, so the "
        "generations around 550 score low by construction: read the line's trend after that, not its level there.")
LABELS = {
    'tamiyo_flip_precombat': 'Tamiyo flipped before attacking',
    'murktide_delve_short': 'Murktide delve cost a counter',
    'burn_no_kill': 'Burn that killed nothing',
    'land_drop_tapout': 'Tapped out before the land drop',
    'daze_payable_pool': 'Daze into floating mana',
    'play': 'Reported from the Play tab',
    'daze_free_not_cast': 'Free Daze not cast',
    'clue_not_cracked_end': 'Clue not cracked at end of turn',
    'suicide_attack': 'Attacked into a bigger blocker',
    'chump_block': 'Chump block at high life',
    'fow_over_free_daze': 'Force of Will where Daze was free',
    'fourth_land_empty_hand': 'Played out a 4th land from an empty hand',
    'bauble_trick_missed': 'Missed the Bauble trick',
}


def scored_spots(L):
    """The spots the site counts: only those the user kept on the review page (the user, 11-02: not unsure, not
    yet reviewed)."""
    return [sp for sp in L['spots'] if (sp.get('review') or {}).get('verdict') == 'keep' and (sp.get('review') or {}).get('by') == 'user']


def library_id(L):
    """The library's id: its scored spots and their answers (a spot added, dropped, held or re-answered changes it)."""
    key = sorted((sp['id'], sp['answer'], sp['watch']) for sp in scored_spots(L))
    return hashlib.sha1(json.dumps(key).encode()).hexdigest()[:12]


def checkpoints(run_dir, lo=None, hi=None, every=None):
    """{iter: path} of a run's evaluated checkpoints (N a multiple of eval_every, or of `every`) in [lo, hi]."""
    with open(os.path.join(run_dir, 'config.json'), encoding='utf-8') as f:
        every = int(every or json.load(f).get('eval_every', 8))
    out = {}
    for name in os.listdir(os.path.join(run_dir, 'models')):
        m = re.match(r'iter_(\d+)\.pt$', name)
        if m and int(m.group(1)) % every == 0 and (lo is None or int(m.group(1)) >= lo) and (hi is None or int(m.group(1)) <= hi):
            out[int(m.group(1))] = os.path.join(run_dir, 'models', name)
    return out


def cache_path(run_dir):
    return os.path.join(run_dir, 'blunders', 'library_scores.json')


def setup_key(sims, select):
    return f'{select or "run"}@{sims}x{REPEATS}'


def load_cache(run_dir, key):
    try:
        with open(cache_path(run_dir), encoding='utf-8') as f:
            c = json.load(f)
        if c.get('setup') == key:
            return c
    except (OSError, ValueError):
        pass
    return {'setup': key, 'scores': {}, 'stale': {}}


def save_cache(run_dir, c):
    os.makedirs(os.path.dirname(cache_path(run_dir)), exist_ok=True)
    tmp = cache_path(run_dir) + '.tmp'
    with open(tmp, 'w', encoding='utf-8') as f:
        json.dump(c, f)
    os.replace(tmp, cache_path(run_dir))


def game_tallies(run_dir, key):
    """The Blunders tab's tallies at this setup (tools/delverai_blunder_games.py writes
    runs/<run>/blunders/games/iter_NNNN/tally.json: {setup, scores {spot id: right-answer share}}) -> {iter: scores}."""
    out = {}
    root = os.path.join(run_dir, 'blunders', 'games')
    if not os.path.isdir(root):
        return out
    for name in os.listdir(root):
        m = re.match(r'iter_(\d+)$', name)
        p = os.path.join(root, name, 'tally.json')
        if not m or not os.path.exists(p):
            continue
        try:
            with open(p, encoding='utf-8') as f:
                t = json.load(f)
        except (OSError, ValueError):
            continue
        if t.get('setup') == key:
            out[int(m.group(1))] = t.get('scores') or {}
    return out


def merge_games(run_dir, c):
    """The Blunders tab's tallies into the cache `c` (they replace its scores of those generations and spots: the
    searches the tab shows) -> whether anything changed."""
    changed = False
    for it, sc in game_tallies(run_dir, c['setup']).items():
        have = c['scores'].setdefault(str(it), {})
        for sid, v in sc.items():
            if have.get(sid) != v:
                have[sid] = v
                changed = True
    return changed


class Busy(Exception):
    """Another process is scoring this run (a backfill, or the sidecar's hook)."""


def _lock(run_dir):
    """Take the run's scoring lock (a file holding the owner's pid; a dead owner's lock is taken over)."""
    import psutil
    path = cache_path(run_dir) + '.lock'
    os.makedirs(os.path.dirname(path), exist_ok=True)
    for _ in range(2):
        try:
            fd = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
            os.write(fd, str(os.getpid()).encode())
            os.close(fd)
            return path
        except FileExistsError:
            try:
                pid = int(open(path).read().strip() or 0)
            except (OSError, ValueError):
                pid = 0
            if pid and pid != os.getpid() and psutil.pid_exists(pid):
                raise Busy(f'{path}: pid {pid} is scoring this run')
            os.remove(path)
    raise Busy(path)


def score(run_dir, L, engine_path, sims=SIMS, select=SELECT, lo=None, hi=None, batch=8, log=B.say, every=None):
    lock = _lock(run_dir)
    try:
        return _score(run_dir, L, engine_path, sims, select, lo, hi, batch, log, every)
    finally:
        try:
            os.remove(lock)
        except OSError:
            pass


def _score(run_dir, L, engine_path, sims=SIMS, select=SELECT, lo=None, hi=None, batch=8, log=B.say, every=None):
    """Score every evaluated checkpoint on the kept spots its cache lacks (the checkpoints missing the same
    spots go in one library_score call) -> the cache {setup, scores {iter: {spot id: right-answer share}},
    stale {iter: [ids]}}."""
    kept = scored_spots(L)
    c = load_cache(run_dir, setup_key(sims, select))
    if merge_games(run_dir, c):
        save_cache(run_dir, c)
    ck = checkpoints(run_dir, lo, hi, every)
    groups = {}
    for i in sorted(ck):
        have = set(c['scores'].get(str(i), {})) | set(c['stale'].get(str(i), []))
        missing = tuple(sp['id'] for sp in kept if sp['id'] not in have)
        if missing:
            groups.setdefault(missing, []).append(i)
    for missing, its in groups.items():
        spots = [sp for sp in kept if sp['id'] in set(missing)]
        log(f'{os.path.basename(run_dir)}: {len(spots)} spots on {len(its)} checkpoints ({sims} simulations, '
            f'{select or "the run's rule"}, {REPEATS} searches a spot)')
        for k in range(0, len(its), batch):
            part = its[k:k + batch]
            R = B.library_score(dict(L, spots=spots), [ck[i] for i in part], [sims], REPEATS, 'cpu', engine_path,
                                rules=[(select, None)] if select else None, log=log)
            for lab, by in R['scores'].items():
                it = int(re.match(r'.*_(\d+)(?:_[a-z][\w.]*)?$', lab).group(1))
                c['scores'].setdefault(str(it), {}).update(by[str(sims)]['spots'])
                c['stale'][str(it)] = sorted(set(c['stale'].get(str(it), [])) | set(R['stale']))
            save_cache(run_dir, c)
    return c


def record(run_name, L, c, sims, select=SELECT):
    """The site record of a run from its cache."""
    kinds = {}
    for sp in scored_spots(L):
        kinds[sp['kind']] = kinds.get(sp['kind'], 0) + 1
    kind_of = {sp['id']: sp['kind'] for sp in scored_spots(L)}
    iters = []
    for it, sc in sorted(c['scores'].items(), key=lambda x: int(x[0])):
        sc = {k: v for k, v in sc.items() if k in kind_of}
        if not sc:
            continue
        per = {}
        for sid, v in sc.items():
            p, n = per.get(kind_of[sid], (0, 0))
            per[kind_of[sid]] = (p + (v * REPEATS >= PASS - 1e-9), n + 1)
        iters.append({'iter': int(it), 'passed': sum(p for p, _ in per.values()), 'total': len(sc),
                      'share': round(sum(sc.values()) / len(sc), 4), 'kinds': {k: [p, n] for k, (p, n) in sorted(per.items())}})
    return {'run': f'blunders.{run_name}', 'kind': 'blunders', 'for_run': run_name, 'updated': int(time.time()),
            'sims': sims, 'repeats': REPEATS, 'rule': select or 'run',
            'pass_rule': f'a right answer in at least {PASS} of {REPEATS} searches',
            'library': {'id': library_id(L), 'spots': len(kind_of), 'kinds': kinds,
                        'labels': {k: LABELS.get(k, k.replace('_', ' ')) for k in kinds}, 'note': NOTE},
            'iters': iters}


def upload_key(key_file=None):
    if os.environ.get(KEY_ENV):
        return os.environ[KEY_ENV].strip()
    with open(key_file or KEY_FILE, encoding='utf-8') as f:
        return f.read().strip()


def upload(rec, url=URL, key=None, timeout=60):
    body = json.dumps(rec).encode()
    if re.search(r'[A-Za-z]:[\\/]|/Users/|/home/', body.decode()):
        raise ValueError('a local path in the blunder record')
    req = urllib.request.Request(url.rstrip('/') + '/api/progress/upload', data=body, method='POST', headers={
        'User-Agent': 'delverai-blunders/1', 'Content-Type': 'application/json', 'Authorization': f'Bearer {key or upload_key()}'})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read().decode() or '{}')


def sync(run, engine_path, sims=SIMS, lo=None, hi=None, do_upload=True, url=URL, library=B.LIBRARY_PATH, log=B.say,
         select=SELECT, every=None):
    """Score what the cache lacks and upload the run's record (the sidecar's hook) -> the record."""
    run_dir = F.run_dir(run)
    L = B.load_library(library)
    if not L or not scored_spots(L):
        return None
    try:
        c = score(run_dir, L, engine_path, sims, select, lo, hi, log=log, every=every)
    except Busy as e:
        log(f'{os.path.basename(os.path.normpath(run_dir))}: blunder checks not updated this time ({e})')
        return None
    rec = record(os.path.basename(os.path.normpath(run_dir)), L, c, sims, select)
    if do_upload:
        upload(rec, url)
        last = rec['iters'][-1] if rec['iters'] else None
        log(f'{rec["run"]}: uploaded {len(rec["iters"])} generations' + (f'; gen {last["iter"]}: {last["passed"]} of {last["total"]} passed' if last else ''))
    return rec


def refresh(run, do_upload=True, url=URL, library=B.LIBRARY_PATH, log=B.say, wait=0, sims=SIMS, select=SELECT):
    """The run's record again without scoring: the Blunders tab's tallies merged into the cache (under its lock,
    waiting up to `wait` s for a scorer), then uploaded -> the record, or None (no library, or still busy)."""
    run_dir = F.run_dir(run)
    L = B.load_library(library)
    if not L or not scored_spots(L):
        return None
    t0 = time.time()
    while True:
        try:
            lock = _lock(run_dir)
            break
        except Busy as e:
            if time.time() - t0 >= wait:
                log(f'{os.path.basename(os.path.normpath(run_dir))}: blunder checks not refreshed ({e})')
                return None
            time.sleep(30)
    try:
        c = load_cache(run_dir, setup_key(sims, select))
        if merge_games(run_dir, c):
            save_cache(run_dir, c)
    finally:
        try:
            os.remove(lock)
        except OSError:
            pass
    rec = record(os.path.basename(os.path.normpath(run_dir)), L, c, sims, select)
    if do_upload:
        upload(rec, url)
        log(f'{rec["run"]}: uploaded {len(rec["iters"])} generations (refreshed)')
    return rec


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('runs', nargs='+')
    ap.add_argument('--min-iter', type=int, default=None)
    ap.add_argument('--max-iter', type=int, default=None)
    ap.add_argument('--every', type=int, default=None, help="score every Nth generation (default: the run's eval_every)")
    ap.add_argument('--engine', default=B.ROOT)
    ap.add_argument('--sims', type=int, default=SIMS)
    ap.add_argument('--select', default=SELECT, help="the move rule ('visits', the Play tab's; 'run' = the run's own)")
    ap.add_argument('--library', default=B.LIBRARY_PATH)
    ap.add_argument('--upload', action='store_true')
    ap.add_argument('--url', default=URL)
    ap.add_argument('--dry-run', action='store_true', help='print the records, upload nothing')
    a = ap.parse_args(argv)
    os.environ.setdefault('ZERO_TOKEN_GRAPHS', '1')
    for run in a.runs:
        rec = sync(run, a.engine, a.sims, a.min_iter, a.max_iter, do_upload=a.upload and not a.dry_run, url=a.url, library=a.library,
                   select=None if a.select == 'run' else a.select, every=a.every)
        if rec is None:
            print('no library')
            continue
        if a.dry_run or not a.upload:
            print(json.dumps({k: v for k, v in rec.items() if k != 'iters'}))
            for r in rec['iters']:
                print(f"  gen {r['iter']:4d}: {r['passed']:3d} / {r['total']} passed (share {r['share']:.3f}) {r['kinds']}")
    return 0


if __name__ == '__main__':
    sys.exit(main())
