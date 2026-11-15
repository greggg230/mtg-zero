"""The run's standing yardsticks (docs/ENGINE.md 164): every evaluated checkpoint of a run (models/iter_N.pt, N a
multiple of its eval_every) measured on what its own evaluation does not measure, once each, in order:

  * bo1        paired best-of-one head-to-heads (`zero.h2h.match`, `paired`: games 2k and 2k + 1 share their deal,
               seats swapped) against a fixed list of opponents - the run's game-1 spec (a best-of-three run's
               evaluation spec without its best-of-three keys: the mains, open decklists; for the Delver runs
               exactly SPEC_DELVER), both sides with the run's evaluation move rule and root belief (`zero.h2h.
               run_rule` of the run's checkpoint; the deck tilt left out: refused on open decklists, the engine's
               belief.rs - it deals nothing differently there), the same seed every generation (the same deals);
  * blunders   the canary library (tools/delverai_blunders.py: the user-kept spots, the Play tab's setup - 1024
               simulations, the most-visited move, 8 searches a spot, passed = right in 5 of 8) on the CPU, through
               delverai_blunders' per-run cache (runs/<run>/blunders/library_scores.json), so a checkpoint is never
               searched twice on a spot and the site's blunder chart (record `blunders.<run>`) is the same numbers.

The run setting `yardsticks` (a dict, {} / null = off; on in the delver-mirror* presets as DELVER_YARDSTICKS, off in
the smokes):

  bo1_games        paired games per opponent (400 = 200 pairs; 0: no best-of-one head-to-heads)
  bo1_sims         simulations per decision of both sides (64)
  bo1_opponents    the opponents, in order: 'anchors' (each of the run's elo_anchors: e.g. pb2's a3198), 'iter0'
                   (the run's own generation 0), or 'LABEL=PATH' (a checkpoint of the run's layout)
  bo1_device       'cuda' (the GPU's share is a 64-row batch at a time, below normal CPU priority) or 'cpu'
  bo1_backend      where the best-of-one games are played (docs/ENGINE.md 165): 'local' (zero.h2h.match here, on
                   bo1_device), 'farm' (a farm play job of the run's pool worker build: mtgz.az.farm_campaign - the same
                   seats, spec and seeds; its games traced, 5% replayed here, the games the farm has not delivered by
                   bo1_farm_wait_s played here by the same worker), or null = auto: 'farm' for an opponent whose match
                   the farm can play exactly (the run has a farm pool - `pool:<farm run>` in selfplay_remote with
                   pool_build and pool_worker - and both sides' seats are one eval job's: `match_settings` accepts
                   them), else 'local' (decided per opponent before anything is played, never as a fallback)
  bo1_farm_share   the farm job's share of the farm's run leases beside the run's own farm self-play (0.25)
  bo1_farm_wait_s  seconds before the farm's missing games are played here (null: 3/4 of timeout_s)
  bo1_threads, bo1_groups   the match's search threads and driver groups (4, 2: zero.h2h's defaults)
  bo1_spec         the best-of-one spec JSON (default: derived from the run's eval_spec, see above)
  bo1_seed         the deals' seed (0, as the by-hand yardsticks of 11-12 to 11-14)
  blunders         the canary library on each checkpoint (true)
  blunders_sims    its simulations (1024: the Play tab's)
  library          the canary library file (default: this checkout's data/delver_canary_library.json)
  from             no checkpoint before this iteration (0)
  settle           seconds a checkpoint's file must be unchanged before it is measured (60)
  retry_s, retry_max_s   a failed measurement is tried again after retry_s, doubling up to retry_max_s (300, 7200)
  timeout_s        a measurement running longer than this is killed and counts as failed (7200)
  poll             seconds between looks for new checkpoints (60)

Results: one JSON line each in <run>/yardsticks.jsonl (append-only; the watcher is the only writer, under the lock
<run>/yardsticks/lock.json):

  {"kind": "bo1", "iter", "opp", "opp_label", "opp_sha" (12 hex), "score", "se", "elo_diff", "elo_se", "games",
   "unfinished", "pairs" {n, both_won, split, both_lost, with_draw, pentanomial, score, se}, "pentanomial", "sims",
   "select", "spec_sha", "seed", "seconds", "time"}
  {"kind": "blunders", "iter", "passed", "total", "share", "kinds" {kind: [passed, n]}, "library", "spots", "stale",
   "sims", "repeats", "select", "seconds", "time"}
  a failure: {"kind", "iter", ("opp"), "error" (its last lines), "attempt", "retry_at", "time"} - said loudly on the
   watcher's console, tried again (in order: the oldest pending measurement first, a failed one once its wait is
   over), never dropped; status() lists it as failed until a result lands, and an evaluated checkpoint without a
   result is missing (the site draws neither as a point).

A measurement is done when the file holds its result: bo1 per (iteration, opponent label), blunders per (iteration,
library id) - a changed library (spots kept or dropped on the review page) makes every checkpoint pending again, the
cache searching only the new spots (as delverai_blunders always counted every generation on the current set).

Where it runs: the run's companion (docs/ENGINE.md 163, zero-core docs/az.md 49) beside the publisher
(mtgz.az.companions), from the run's own checkout - or by hand: `python tools/run_yardsticks.py watch <run>`. Two
lanes, each in order: the head-to-heads (the GPU) and the canary library (the CPU), each measurement a child process
at below normal priority.

Light at import (no torch, no numpy): the supervisor imports it through the companion hook.
"""
import hashlib
import json
import os
import re
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
LOG = 'yardsticks_console.txt'
ROWS = 'yardsticks.jsonl'
DIR = 'yardsticks'

DEFAULTS = {'bo1_games': 400, 'bo1_sims': 64, 'bo1_opponents': ['anchors', 'iter0'], 'bo1_device': 'cuda',
            'bo1_threads': 4, 'bo1_groups': 2, 'bo1_spec': None, 'bo1_seed': 0, 'bo1_backend': None,
            'bo1_farm_share': 0.25, 'bo1_farm_wait_s': None, 'blunders': True,
            'blunders_sims': 1024, 'library': None, 'from': 0, 'settle': 60, 'retry_s': 300, 'retry_max_s': 7200,
            'timeout_s': 7200, 'poll': 60}
# the Delver presets' (the user, 2026-11-14: "our evaluations should test bo1 ability at all to look for regressions",
# and the blunder tracker on every evaluation)
DELVER_YARDSTICKS = {'bo1_games': 400, 'bo1_sims': 64, 'bo1_opponents': ['anchors', 'iter0'], 'blunders': True}

_NUM = (int, float)
KEYS = {'bo1_games': int, 'bo1_sims': int, 'bo1_opponents': list, 'bo1_device': str, 'bo1_threads': int,
        'bo1_groups': int, 'bo1_spec': str, 'bo1_seed': int, 'bo1_backend': str, 'bo1_farm_share': _NUM,
        'bo1_farm_wait_s': _NUM, 'blunders': bool, 'blunders_sims': int, 'library': str,
        'from': int, 'settle': _NUM, 'retry_s': _NUM, 'retry_max_s': _NUM, 'timeout_s': _NUM, 'poll': _NUM}
# best-of-three keys of a spec (docs/ENGINE.md 120, 121, 130, 154): dropped for the run's game 1
BO3_KEYS = ('format', 'game_episodes', 'start_game', 'dealt_decks')
PASS, REPEATS = 5, 8                      # delverai_blunders' rule (a spot passes when right in 5 of 8 searches)


def no_yardsticks(argv):
    """`--no-yardsticks` -> `--yardsticks {}` in a run's command line (`python -m mtgz.az.run` does it)."""
    return [x for a in argv for x in (('--yardsticks', '{}') if a == '--no-yardsticks' else (a,))]


def normalize(ys):
    """The `yardsticks` setting -> its dict with the defaults filled in (validated), or None when off. ValueError for
    a bad one."""
    if ys is None or ys is False or ys == {}:
        return None
    if not isinstance(ys, dict):
        raise ValueError(f'yardsticks {ys!r}: a JSON object of the yardsticks\' options, or {{}} / null = off')
    bad = sorted(set(ys) - set(KEYS))
    if bad:
        raise ValueError(f'yardsticks: unknown option(s) {bad} (known: {sorted(KEYS)})')
    for k, v in ys.items():
        t = KEYS[k]
        if v is None:
            continue
        if isinstance(v, bool) and t is not bool:
            raise ValueError(f'yardsticks: {k} {v!r} is not of the right type ({t})')
        if not isinstance(v, t):
            raise ValueError(f'yardsticks: {k} {v!r} is not of the right type ({t})')
    out = dict(DEFAULTS, **{k: v for k, v in ys.items() if v is not None})
    for k in ('bo1_sims', 'bo1_threads', 'bo1_groups', 'blunders_sims'):
        if out[k] < 1:
            raise ValueError(f'yardsticks: {k} {out[k]} must be >= 1')
    if out['bo1_games'] < 0 or out['bo1_games'] % 2:
        raise ValueError(f"yardsticks: bo1_games {out['bo1_games']}: an even number >= 0 (paired games; 0 = none)")
    if out['bo1_backend'] not in (None, 'farm', 'local'):
        raise ValueError(f"yardsticks: bo1_backend {out['bo1_backend']!r}: 'farm', 'local' or null (auto)")
    if not 0 < out['bo1_farm_share'] <= 1:
        raise ValueError(f"yardsticks: bo1_farm_share {out['bo1_farm_share']}: in (0, 1]")
    if out['bo1_farm_wait_s'] is not None and out['bo1_farm_wait_s'] <= 0:
        raise ValueError('yardsticks: bo1_farm_wait_s must be > 0 (or null)')
    if out['bo1_device'] not in ('cuda', 'cpu'):
        raise ValueError(f"yardsticks: bo1_device {out['bo1_device']!r}: 'cuda' or 'cpu'")
    if not all(isinstance(x, str) and x for x in out['bo1_opponents']):
        raise ValueError(f"yardsticks: bo1_opponents {out['bo1_opponents']!r}: 'anchors', 'iter0' or 'LABEL=PATH'")
    for x in out['bo1_opponents']:
        if x not in ('anchors', 'iter0') and '=' not in x:
            raise ValueError(f"yardsticks: bo1 opponent {x!r}: 'anchors', 'iter0' or 'LABEL=PATH'")
    if out['bo1_games'] and not out['bo1_opponents']:
        raise ValueError('yardsticks: bo1_games without bo1_opponents')
    if not out['bo1_games'] and not out['blunders']:
        raise ValueError('yardsticks: neither bo1_games nor blunders: nothing to measure ({} turns it off)')
    for k in ('retry_s', 'retry_max_s', 'timeout_s', 'poll'):
        if out[k] <= 0:
            raise ValueError(f'yardsticks: {k} {out[k]} must be > 0')
    if out['settle'] < 0 or out['from'] < 0:
        raise ValueError('yardsticks: settle and from must be >= 0')
    return out


def cfg_of(run):
    with open(os.path.join(run, 'config.json'), encoding='utf-8') as f:
        return json.load(f)


def of_run(run, cfg=None):
    """The run's yardsticks setting (normalized) - its config.json's, else its preset's (a run that saved none yet,
    started before the setting existed, as zero.run resolves it at its next start) -> dict or None."""
    cfg = cfg if cfg is not None else cfg_of(run)
    if 'yardsticks' in cfg:
        return normalize(cfg.get('yardsticks'))
    from . import PRESETS
    return normalize(PRESETS.get(cfg.get('preset') or '', {}).get('yardsticks'))


# ------------------------------------------------------------------------------------------------ what is measured
def bo1_spec(cfg, ys):
    """The best-of-one spec JSON of the run: `bo1_spec`, else its eval_spec (or spec) - a best-of-three one without
    its best-of-three keys and sideboards (game 1: the mains, open decklists). ValueError when the run has none."""
    if ys.get('bo1_spec'):
        return ys['bo1_spec']
    s = cfg.get('eval_spec') or cfg.get('spec')
    if not s:
        raise ValueError('yardsticks: the run has no eval_spec / spec to play best-of-one from (give bo1_spec)')
    d = json.loads(s) if isinstance(s, str) else dict(s)
    if d.get('format') != 'bo3':
        return s if isinstance(s, str) else json.dumps(d, separators=(',', ':'))
    d = {k: v for k, v in d.items() if k not in BO3_KEYS}
    if isinstance(d.get('decks'), dict):
        d['decks'] = {k: v for k, v in d['decks'].items() if not k.endswith('_sideboard')}
    return json.dumps(d, separators=(',', ':'))


def model_path(run, it):
    return os.path.join(run, 'models', f'iter_{int(it):04d}.pt')


_SHA = {}


def same_file(a, b):
    """Whether two checkpoint files hold the same network (sha256; memoized by path, size and modification time):
    a run's generation 0 against an anchor that is a copy of it measures nothing."""
    try:
        return sha12(a) == sha12(b)
    except OSError:
        return False


def sha12(path):
    st = os.stat(path)
    k = (os.path.normcase(os.path.abspath(path)), st.st_size, st.st_mtime)
    if k in _SHA:
        return _SHA[k]
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for b in iter(lambda: f.read(1 << 20), b''):
            h.update(b)
    _SHA[k] = h.hexdigest()[:12]
    return _SHA[k]


def _anchors(s):
    """zero.elo.parse_anchors without its import: 'path,label=path,...' or a list -> [(label, path)]."""
    if s in (None, ''):
        return []
    out = []
    for x in (s.split(',') if isinstance(s, str) else list(s)):
        x = str(x).strip()
        if not x:
            continue
        label, sep, path = x.partition('=')
        if not sep or any(ch in label for ch in ':/\\'):
            label, path = os.path.splitext(os.path.basename(x))[0], x
        out.append((label.strip(), path.strip()))
    return out


def opp_label(name):
    """A readable label: an anchor 'a3198' -> '3198' (a frozen copy of generation 3198), 'gen0' -> 'generation 0'."""
    m = re.fullmatch(r'a(\d+)', name)
    if m:
        return m.group(1)
    if name == 'gen0':
        return 'generation 0'
    return name


def opponents(run, cfg, ys):
    """The best-of-one opponents in order -> [{'name', 'label', 'path'}] (names distinct)."""
    out = []
    for x in ys['bo1_opponents']:
        if x == 'anchors':
            out += [{'name': lb, 'label': opp_label(lb), 'path': p} for lb, p in _anchors(cfg.get('elo_anchors'))]
        elif x == 'iter0':
            out.append({'name': 'gen0', 'label': opp_label('gen0'), 'path': model_path(run, 0)})
        else:
            lb, _, p = x.partition('=')
            out.append({'name': lb.strip(), 'label': opp_label(lb.strip()), 'path': p.strip()})
    seen = set()
    for o in out:
        if o['name'] in seen:
            raise ValueError(f"yardsticks: two best-of-one opponents named {o['name']!r}")
        seen.add(o['name'])
    return out


def evaluated(run, cfg, ys, now=None, settle=None):
    """The run's evaluated checkpoints to measure, oldest first: models/iter_N.pt with N a multiple of eval_every,
    N >= `from`, its file unchanged for `settle` seconds."""
    every = int(cfg.get('eval_every') or 1)
    now = time.time() if now is None else now
    settle = ys['settle'] if settle is None else settle
    out = []
    try:
        names = os.listdir(os.path.join(run, 'models'))
    except OSError:
        return out
    for name in names:
        m = re.fullmatch(r'iter_(\d+)\.pt', name)
        if not m:
            continue
        it = int(m.group(1))
        if it % every or it < ys['from']:
            continue
        try:
            if now - os.path.getmtime(os.path.join(run, 'models', name)) < settle:
                continue
        except OSError:
            continue
        out.append(it)
    return sorted(out)


def library_path(ys):
    return ys.get('library') or os.path.join(ROOT, 'data', 'delver_canary_library.json')


def _blunder_tools():
    sys.path.insert(0, os.path.join(ROOT, 'tools'))
    import delverai_blunders as DB  # noqa: E402
    return DB


def library_id(ys):
    """The canary library's id (delverai_blunders.library_id: its kept spots and their answers), or None when there
    is no library or no kept spot (then the blunders lane has nothing to measure: said by status())."""
    DB = _blunder_tools()
    L = DB.B.load_library(library_path(ys))
    if not L or not DB.scored_spots(L):
        return None
    return DB.library_id(L)


# ------------------------------------------------------------------------------------------------ the results file
def rows_path(run):
    return os.path.join(run, ROWS)


def read_rows(run):
    """<run>/yardsticks.jsonl's rows (a torn last line - a writer killed mid-line - is skipped)."""
    out = []
    try:
        with open(rows_path(run), encoding='utf-8') as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                try:
                    r = json.loads(line)
                except ValueError:
                    continue
                if isinstance(r, dict) and r.get('kind') in ('bo1', 'blunders') and isinstance(r.get('iter'), int):
                    out.append(r)
    except OSError:
        pass
    return out


def append_row(run, row):
    """One row appended (flushed and synced: the next look sees it)."""
    with open(rows_path(run), 'a', encoding='utf-8', newline='') as f:
        f.write(json.dumps(row, separators=(',', ':')) + '\n')
        f.flush()
        os.fsync(f.fileno())


def key_of(row_or_item, lib=None):
    """A measurement's key: ('bo1', iter, opponent) or ('blunders', iter, library id)."""
    r = row_or_item
    if r['kind'] == 'bo1':
        return ('bo1', r['iter'], r['opp'])
    return ('blunders', r['iter'], r.get('library', lib))


def tally(rows):
    """-> (results {key: row}, failures {key: [failure rows since its last result]})."""
    res, fails = {}, {}
    for r in rows:
        k = key_of(r)
        if r.get('error') is not None:
            if k not in res:
                fails.setdefault(k, []).append(r)
        else:
            res[k] = r
            fails.pop(k, None)
    return res, fails


def backoff(ys, attempts):
    """The wait after the `attempts`-th failure in a row: retry_s doubling, at most retry_max_s."""
    return min(float(ys['retry_s']) * 2 ** max(0, attempts - 1), float(ys['retry_max_s']))


def wanted(run, cfg, ys, lib=None, now=None, settle=None):
    """Every measurement the run's evaluated checkpoints call for, in order (by iteration; bo1 in opponent order) ->
    [{'lane', 'kind', 'iter', 'opp'?, 'library'?}]. `lib`: the canary library's id (None: no readable library with
    kept spots - its measurements are wanted all the same and fail loudly until there is one)."""
    its = evaluated(run, cfg, ys, now, settle)
    opps = opponents(run, cfg, ys) if ys['bo1_games'] else []
    out = []
    for it in its:
        for o in opps:
            if it == 0 and (o['name'] == 'gen0' or same_file(o['path'], model_path(run, 0))):
                continue                        # (generation 0 against itself, or a copy of it, measures nothing)
            out.append({'lane': bo1_lane(run, cfg, ys, o), 'kind': 'bo1', 'iter': it,
                        'opp': o['name']})
        if ys['blunders']:                      # (no library: measured all the same - it fails loudly, retried)
            out.append({'lane': 'cpu', 'kind': 'blunders', 'iter': it, 'library': lib})
    return out


def pending(run, cfg, ys, lane, rows=None, lib=None, now=None, settle=None):
    """The lane's next measurement: the oldest wanted one without a result whose retry wait (after a failure) is
    over -> item or None."""
    now = time.time() if now is None else now
    res, fails = tally(read_rows(run) if rows is None else rows)
    for item in wanted(run, cfg, ys, lib, now, settle):
        if item['lane'] != lane:
            continue
        k = key_of(item, lib)
        if k in res:
            continue
        f = fails.get(k)
        if f and now < f[-1].get('retry_at', 0):
            continue
        return dict(item, attempt=len(f or []) + 1)
    return None


def status(run, cfg=None, ys=None, lib=None, rows=None, now=None):
    """The yardsticks' state for the publisher and `run_yardsticks.py status` -> {'expected': [evaluated
    iterations], 'bo1': [result rows], 'blunders': [result rows of the current library], 'missing': [{kind, iter,
    opp?}: wanted, no result, not failing], 'failed': [{kind, iter, opp?, attempts, error, retry_at}],
    'opponents': [{name, label}], 'library': id} or None when off."""
    cfg = cfg if cfg is not None else cfg_of(run)
    ys = ys if ys is not None else of_run(run, cfg)
    if ys is None:
        return None
    if lib is None and ys['blunders']:
        try:
            lib = library_id(ys)
        except Exception:                      # noqa: BLE001 - a broken library file: said as no blunder results
            lib = None
    rows = read_rows(run) if rows is None else rows
    res, fails = tally(rows)
    miss, failed = [], []
    for item in wanted(run, cfg, ys, lib, now, settle=0):
        k = key_of(item, lib)
        if k in res:
            continue
        x = {kk: item[kk] for kk in ('kind', 'iter', 'opp') if kk in item}
        f = fails.get(k)
        if f:
            failed.append(dict(x, attempts=len(f), error=str(f[-1].get('error'))[-400:], retry_at=f[-1].get('retry_at')))
        else:
            miss.append(x)
    try:
        opps = [{'name': o['name'], 'label': o['label']} for o in opponents(run, cfg, ys)] if ys['bo1_games'] else []
    except ValueError:
        opps = []
    return {'expected': evaluated(run, cfg, ys, now, settle=0),
            'bo1': sorted((r for r in res.values() if r['kind'] == 'bo1'), key=lambda r: (r['iter'], r['opp'])),
            'blunders': sorted((r for r in res.values() if r['kind'] == 'blunders' and r.get('library') == lib),
                               key=lambda r: r['iter']),
            'missing': miss, 'failed': failed, 'opponents': opps, 'library': lib,
            'settings': {k: ys[k] for k in ('bo1_games', 'bo1_sims', 'blunders', 'blunders_sims')}}


# ------------------------------------------------------------------------------------------------ the measurements
def bo1_seat(ckpt):
    """Both sides' seat in the best-of-one head-to-head: the run's evaluation move rule and root belief
    (zero.h2h.run_rule of the run's checkpoint), the deck tilt left out (refused on open decklists)."""
    from zero import h2h
    seat = h2h.run_rule(ckpt)
    tilts = {k: v for k, v in (seat.get('belief_tilts') or {}).items() if k != 'deck'}
    return h2h.with_belief(seat, tilts=tilts)


_BACKEND = {}


def bo1_backend(run, cfg, ys, o):
    """Where the best-of-one against opponent `o` is played: `bo1_backend` when set, else 'farm' when the run has a
    farm pool and the farm can play the match exactly (mtgz.az.farm_campaign.match_settings accepts both sides' seats:
    an eval job has one seat setting for both), else 'local' (memoized per run and opponent: settings only, no
    network is loaded)."""
    if ys.get('bo1_backend'):
        return ys['bo1_backend']
    from . import farm_campaign as C
    if C.run_pool(cfg) is None:
        return 'local'
    k = (os.path.normcase(os.path.abspath(run)), o['name'], o['path'], ys['bo1_sims'])
    if k not in _BACKEND:
        try:
            ck = model_path(run, 0)
            tilts = bo1_tilts(cfg)
            C.match_settings(ck, ys['bo1_sims'], bo1_spec(cfg, ys), tilts=tilts, ckpt_b=o['path'])
            _BACKEND[k] = 'farm'
        except (C.CampaignError, ValueError, OSError):
            _BACKEND[k] = 'local'
    return _BACKEND[k]


def bo1_lane(run, cfg, ys, o):
    """The watcher lane of a best-of-one: 'farm' (it waits on the farm, no local GPU or CPU), else the device's."""
    if bo1_backend(run, cfg, ys, o) == 'farm':
        return 'farm'
    return 'gpu' if ys['bo1_device'] == 'cuda' else 'cpu_bo1'


def bo1_lanes(run, cfg, ys):
    """Every lane the best-of-ones of the setting use (the watcher starts one thread per lane)."""
    if not ys['bo1_games']:
        return []
    return sorted({bo1_lane(run, cfg, ys, o) for o in opponents(run, cfg, ys)})


def bo1_tilts(cfg):
    """The best-of-one seats' per-target belief tilts: the run's evaluation tilts without 'deck' (bo1_seat)."""
    from zero.mcts import belief_tilts
    return {k: v for k, v in (belief_tilts(cfg, 'eval') or {}).items() if k != 'deck'}


def measure_bo1(run, it, opp, cfg=None, ys=None):
    """One best-of-one head-to-head: the run's checkpoint `it` against the opponent named `opp` -> the result row
    (played where bo1_backend says: here, or a farm play job with the same numbers)."""
    from zero import h2h
    cfg = cfg if cfg is not None else cfg_of(run)
    ys = ys if ys is not None else of_run(run, cfg)
    o = {x['name']: x for x in opponents(run, cfg, ys)}.get(opp)
    if o is None:
        raise ValueError(f'yardsticks: no best-of-one opponent {opp!r} in this run\'s setting')
    ck = model_path(run, it)
    for p in (ck, o['path']):
        if not os.path.exists(p):
            raise FileNotFoundError(p)
    if bo1_backend(run, cfg, ys, o) == 'farm':
        return measure_bo1_farm(run, it, o, cfg, ys)
    spec = bo1_spec(cfg, ys)
    seat = bo1_seat(ck)
    t0 = time.time()
    out = h2h.match(ck, o['path'], ys['bo1_games'], ys['bo1_sims'], seed=ys['bo1_seed'], threads=ys['bo1_threads'],
                    groups=ys['bo1_groups'], device=ys['bo1_device'], spec=spec, seat_a=dict(seat), seat_b=dict(seat),
                    paired=True)
    if out.get('games') != ys['bo1_games']:
        raise RuntimeError(f"best-of-one {it} vs {opp}: {out.get('games')} games of {ys['bo1_games']}")
    pairs = out.get('pairs') or {}
    return {'kind': 'bo1', 'iter': int(it), 'opp': opp, 'opp_label': o['label'], 'opp_sha': sha12(o['path']),
            'score': out['a_score'], 'se': out['se'], 'elo_diff': out.get('elo_diff'), 'elo_se': out.get('elo_se'),
            'games': out['games'], 'unfinished': out.get('unfinished', 0), 'pairs': pairs,
            'pentanomial': pairs.get('pentanomial'), 'sims': ys['bo1_sims'], 'select': out.get('select_a'),
            'spec_sha': hashlib.sha256(spec.encode()).hexdigest()[:12], 'seed': ys['bo1_seed'],
            'seconds': round(time.time() - t0, 1), 'time': round(time.time())}


def measure_bo1_farm(run, it, o, cfg, ys, client=None, opts=None, log=None):
    """The best-of-one as a farm play job (docs/ENGINE.md 165): the run's pool worker build, the seats, spec and seed
    of `measure_bo1`'s local match (checked dict for dict), paired, traced; collected with the spot check and the
    local fallback (the same worker on this machine for what the farm has not delivered by bo1_farm_wait_s) -> the
    local row's fields plus 'backend' 'farm' and 'farm' {run, digest, spot, machines, local, seconds}."""
    from zero import h2h
    from . import farm_campaign as C
    pool = C.run_pool(cfg)
    if pool is None:
        raise RuntimeError('bo1_backend farm: the run has no farm pool (pool:<farm run> with pool_build, pool_worker)')
    ck = model_path(run, it)
    spec = bo1_spec(cfg, ys)
    settings, rule = C.match_settings(ck, ys['bo1_sims'], spec, tilts=bo1_tilts(cfg), ckpt_b=o['path'])
    name = f"{os.path.basename(os.path.normpath(run))}-y{int(it)}-{o['name']}-{sha12(ck)[:6]}{sha12(o['path'])[:6]}"
    wait = ys['bo1_farm_wait_s'] or 0.75 * float(ys['timeout_s'])
    t0 = time.time()
    kw = {'log': log} if log else {}
    d = C.submit(name, ck, ys['bo1_games'], settings, [{'net': 1}, {'net': 2}], ys['bo1_seed'], pool['build_id'],
                 pool['worker'], pool['runtime'], b=o['path'], paired=True, share=ys['bo1_farm_share'],
                 deadline_s=wait, min_agent=pool['min_agent'], meta={'yardstick': 'bo1', 'run': run, 'iter': int(it),
                                                                      'opp': o['name']}, client=client, opts=opts, **kw)
    res = C.collect(d, client=client, threads=ys['bo1_threads'], describe_games=False, opts=opts, **kw)
    out = res['readout']
    if out is None or out.get('games') != ys['bo1_games']:
        raise RuntimeError(f"best-of-one {it} vs {o['name']} (farm): {out and out.get('games')} games of {ys['bo1_games']}")
    pairs = out.get('pairs') or {}
    return {'kind': 'bo1', 'iter': int(it), 'opp': o['name'], 'opp_label': o['label'], 'opp_sha': sha12(o['path']),
            'score': out['a_score'], 'se': out['se'], 'elo_diff': out.get('elo_diff'), 'elo_se': out.get('elo_se'),
            'games': out['games'], 'unfinished': out.get('unfinished', 0), 'pairs': pairs,
            'pentanomial': pairs.get('pentanomial'), 'sims': ys['bo1_sims'],
            'select': h2h._tag(C.match_seat(ck, ys['bo1_sims'], rule)),
            'spec_sha': hashlib.sha256(spec.encode()).hexdigest()[:12], 'seed': ys['bo1_seed'],
            'seconds': round(time.time() - t0, 1), 'time': round(time.time()), 'backend': 'farm',
            'farm': {'run': res['run'], 'digest': res['digest'], 'spot': {k: res['spot'][k] for k in ('checked', 'ok', 'of')},
                     'machines': res['machines'], 'local': res['coverage']['local'], 'seconds': res['seconds']}}


def measure_blunders(run, it, cfg=None, ys=None, log=print):
    """The canary library on the run's checkpoint `it` (delverai_blunders.score: what its cache lacks) -> the row."""
    cfg = cfg if cfg is not None else cfg_of(run)
    ys = ys if ys is not None else of_run(run, cfg)
    DB = _blunder_tools()
    L = DB.B.load_library(library_path(ys))
    if not L or not DB.scored_spots(L):
        raise RuntimeError(f'yardsticks: no canary library with kept spots at {library_path(ys)}')
    if not os.path.exists(model_path(run, it)):
        raise FileNotFoundError(model_path(run, it))
    t0 = time.time()
    c = DB.score(run, L, ROOT, ys['blunders_sims'], DB.SELECT, lo=it, hi=it, log=log,
                 every=int(cfg.get('eval_every') or 1))
    rec = DB.record(os.path.basename(os.path.normpath(run)), L, c, ys['blunders_sims'], DB.SELECT)
    row = next((r for r in rec['iters'] if r['iter'] == int(it)), None)
    if row is None:
        raise RuntimeError(f'canary library: no spot scored on iteration {it} (every spot stale on this engine?)')
    return {'kind': 'blunders', 'iter': int(it), 'passed': row['passed'], 'total': row['total'],
            'share': row['share'], 'kinds': row['kinds'], 'library': rec['library']['id'],
            'spots': rec['library']['spots'], 'stale': len((c.get('stale') or {}).get(str(it), [])),
            'sims': ys['blunders_sims'], 'repeats': DB.REPEATS, 'select': DB.SELECT,
            'seconds': round(time.time() - t0, 1), 'time': round(time.time())}


# ------------------------------------------------------------------------------------------------ the companion
def companion(run, cfg):
    """The yardsticks' watcher as a companion process (mtgz.az.companions composes it with the publisher), or None
    when the run's setting is off. Raises on a bad setting."""
    ys = normalize(cfg['yardsticks']) if 'yardsticks' in cfg else None
    if ys is None:
        return None
    return {'name': 'yardsticks', 'cmd': [sys.executable, os.path.join(ROOT, 'tools', 'run_yardsticks.py'), 'watch',
                                          run, '--supervised'], 'log': LOG}
