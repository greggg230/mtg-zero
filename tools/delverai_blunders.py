"""The canary library's results per generation for delverai.greggg230.com's Progress tab (the user's ask of
11-02: "how many of the blunder cases we passed"). Every evaluated checkpoint of a run (models/iter_N.pt, N a
multiple of its eval_every) is searched on the current library (tools/delver_blunders.py library_score: the
run's evaluation seat, 8 searches a spot, CPU) and a spot is PASSED when a right answer won at least 5 of
its 8 searches. The results are cached per run (runs/<run>/blunders/library_scores.json, keyed by the
library's id: a changed library - a spot added, dropped or held - scores every checkpoint again, so a run's
generations are always counted on one set of positions). The site record is `blunders.<run>` (kind
'blunders', delverai worker/index.js): {for_run, library {id, spots, kinds, labels}, sims, repeats,
pass_rule, iters [{iter, passed, total, share, kinds {kind: [passed, n]}}]}.

  python tools/delverai_blunders.py RUN [RUN ...] [--min-iter N] [--max-iter N] [--engine CHECKOUT]
      [--sims 32] [--upload] [--url https://delverai.greggg230.com] [--key-file F] [--dry-run]

RUN: a run directory or name (runs/<name>). --max-iter bounds a run another one continues (the main line up to
its fork point). The sidecar (`delver_blunders.py watch --site`) calls `sync` after each measurement.
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
}


def scored_spots(L):
    """The spots the site counts: only those the user kept on the review page (the user, 11-02: not unsure, not
    yet reviewed)."""
    return [sp for sp in L['spots'] if (sp.get('review') or {}).get('verdict') == 'keep' and (sp.get('review') or {}).get('by') == 'user']


def library_id(L):
    """The library's id: its scored spots and their answers (a spot added, dropped, held or re-answered changes it)."""
    key = sorted((sp['id'], sp['answer'], sp['watch']) for sp in scored_spots(L))
    return hashlib.sha1(json.dumps(key).encode()).hexdigest()[:12]


def checkpoints(run_dir, lo=None, hi=None):
    """{iter: path} of a run's evaluated checkpoints (N a multiple of eval_every) in [lo, hi]."""
    with open(os.path.join(run_dir, 'config.json'), encoding='utf-8') as f:
        every = int(json.load(f).get('eval_every', 8))
    out = {}
    for name in os.listdir(os.path.join(run_dir, 'models')):
        m = re.match(r'iter_(\d+)\.pt$', name)
        if m and int(m.group(1)) % every == 0 and (lo is None or int(m.group(1)) >= lo) and (hi is None or int(m.group(1)) <= hi):
            out[int(m.group(1))] = os.path.join(run_dir, 'models', name)
    return out


def cache_path(run_dir):
    return os.path.join(run_dir, 'blunders', 'library_scores.json')


def load_cache(run_dir, lib_id, sims):
    try:
        with open(cache_path(run_dir), encoding='utf-8') as f:
            c = json.load(f)
        if c.get('library') == lib_id and c.get('sims') == sims and c.get('repeats') == REPEATS:
            return c
    except (OSError, ValueError):
        pass
    return {'library': lib_id, 'sims': sims, 'repeats': REPEATS, 'scores': {}, 'stale': {}}


def save_cache(run_dir, c):
    os.makedirs(os.path.dirname(cache_path(run_dir)), exist_ok=True)
    tmp = cache_path(run_dir) + '.tmp'
    with open(tmp, 'w', encoding='utf-8') as f:
        json.dump(c, f)
    os.replace(tmp, cache_path(run_dir))


def score(run_dir, L, engine_path, sims=32, lo=None, hi=None, batch=8, log=B.say):
    """Score the evaluated checkpoints the cache lacks on library L -> the cache {library, scores {iter: {spot
    id: right-answer share}}, stale {iter: [ids]}}."""
    lib_id = library_id(L)
    kept = dict(L, spots=scored_spots(L))
    c = load_cache(run_dir, lib_id, sims)
    ck = checkpoints(run_dir, lo, hi)
    todo = sorted(i for i in ck if str(i) not in c['scores'])
    if todo:
        log(f'{os.path.basename(run_dir)}: scoring {len(todo)} checkpoints on library {lib_id} ({len(kept["spots"])} kept spots, '
            f'{sims} simulations, {REPEATS} searches a spot)')
    for k in range(0, len(todo), batch):
        its = todo[k:k + batch]
        R = B.library_score(kept, [ck[i] for i in its], [sims], REPEATS, 'cpu', engine_path, log=log)
        for lab, by in R['scores'].items():
            it = int(re.search(r'_(\d+)$', lab).group(1))
            c['scores'][str(it)] = by[str(sims)]['spots']
            c['stale'][str(it)] = R['stale']
        save_cache(run_dir, c)
    return c


def record(run_name, L, c, sims):
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
            'sims': sims, 'repeats': REPEATS, 'pass_rule': f'a right answer in at least {PASS} of {REPEATS} searches',
            'library': {'id': c['library'], 'spots': len(kind_of), 'kinds': kinds,
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


def sync(run, engine_path, sims=32, lo=None, hi=None, do_upload=True, url=URL, library=B.LIBRARY_PATH, log=B.say):
    """Score what the cache lacks and upload the run's record (the sidecar's hook) -> the record."""
    run_dir = F.run_dir(run)
    L = B.load_library(library)
    if not L or not scored_spots(L):
        return None
    c = score(run_dir, L, engine_path, sims, lo, hi, log=log)
    rec = record(os.path.basename(os.path.normpath(run_dir)), L, c, sims)
    if do_upload:
        upload(rec, url)
        last = rec['iters'][-1] if rec['iters'] else None
        log(f'{rec["run"]}: uploaded {len(rec["iters"])} generations' + (f'; gen {last["iter"]}: {last["passed"]} of {last["total"]} passed' if last else ''))
    return rec


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('runs', nargs='+')
    ap.add_argument('--min-iter', type=int, default=None)
    ap.add_argument('--max-iter', type=int, default=None)
    ap.add_argument('--engine', default=B.ROOT)
    ap.add_argument('--sims', type=int, default=32)
    ap.add_argument('--library', default=B.LIBRARY_PATH)
    ap.add_argument('--upload', action='store_true')
    ap.add_argument('--url', default=URL)
    ap.add_argument('--dry-run', action='store_true', help='print the records, upload nothing')
    a = ap.parse_args(argv)
    os.environ.setdefault('ZERO_TOKEN_GRAPHS', '1')
    for run in a.runs:
        rec = sync(run, a.engine, a.sims, a.min_iter, a.max_iter, do_upload=a.upload and not a.dry_run, url=a.url, library=a.library)
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
