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
  timeout_s        a measurement running longer than this is killed and counts as failed (7200); a farm best of
                   one (lane 'farm'): one this long without progress, its console unchanged (docs/ENGINE.md 175.6)
  poll             seconds between looks for new checkpoints (60)

The cost settings (docs/ENGINE.md 182; all off by default: the measurements of before, number for number):

  bo1_every, blunders_every, cells_every, cells_gen_every
                   a kind's cadence: measured only on evaluated checkpoints whose iteration is a multiple of it
                   (null / 0: every evaluated checkpoint; give a multiple of eval_every). cells_gen's previous
                   checkpoint is then the previous one on its own cadence (gen over N iterations)
  bo1_target_se, bo1_sprt, bo1_batch
                   a sequential best of one (zero.h2h.sequential, local games only): batches of bo1_batch paired
                   games (100; batch k seeded bo1_seed + 7919 k, batch 0 the plain match's first games) until the
                   score's se <= bo1_target_se (e.g. 0.02) or the SPRT [elo0, elo1] (alpha = beta = 0.05) decides, at
                   most bo1_games games; the row says why it stopped ('stop') and its batches
  cells_target_se, cells_batch
                   sequential cells (local games only): each deck pair played in batches of cells_batch games a cell
                   (100; a multiple of 4 dividing cells_games and cells_gen_games; batch k of a spec seeded its seed +
                   7919 k, h2h.sequential's schedule) until every cell of the pair has an Elo se <= cells_target_se
                   (e.g. 35; at least two batches), at most the pair's games; the baseline is played on the same
                   batches to the full games (cached under its own key: the batch is in it) and each cell is judged
                   against the baseline's same first units (common random numbers kept)
  sparse           the local head-to-heads' sparse batches (true: zero.h2h's driver ships sparse rows, as self-play
                   does - the network's answers are bit for bit the dense rows', zero-core tests/test_tail_graphs.py;
                   false: dense rows, as before 11-16)

The metagame cells (docs/ENGINE.md 177; off unless cells_games or cells_gen_games > 0): every ordered (my deck,
opponent's deck) cell of `cells_decks`, the checkpoint piloting my deck against a frozen reference piloting the
other, against the reference piloting both on the same seeds (common random numbers) - see the section "the cells":

  cells_games      games per cell against the current reference (even; 0 = off): cells_games / 2 seeds, each played
                   with my side choosing play or draw and with the other side choosing (CR 103.1: spec `chooser`)
  cells_gen_games  games per cell of the gen-over-gen kind: the checkpoint against the previously evaluated one (0)
  cells_decks      the decks (data/decks names; default the three of the metagame: Izzet Delver, Doomsday, Sneak and
                   Show)
  cells_refs       the reference ladder's start: 'LABEL=PATH', 'iter0' or {"label", "path", "since"?, "elo"?,
                   "elo_se"?}; later references are frozen by `run_yardsticks.py cells-freeze` (an explicit command:
                   <run>/yardsticks/cells_ladder.json), never automatically
  cells_sims, cells_device   (null: bo1_sims, bo1_device)
  cells_backend    'local', 'farm', 'mac' or null = auto (as bo1_backend, per reference; 'mac': explicit only)
  cells_mac_jobs, cells_mac_threads, cells_mac_groups, cells_mac_torch_threads, cells_mac_nice
                   the 'mac' backend (docs/ENGINE.md 198; mtgz.az.mac_judge): the cells' games played on the MacBook
                   by the watcher's own commit (its Mac checkout ~/mtg-zero-wt/run-<sha12>, `bash tools/mac.sh
                   --eval-checkout`), each chooser match one process there at nice cells_mac_nice (10) with
                   cells_mac_threads search threads (4), cells_mac_groups driver groups (2) and
                   cells_mac_torch_threads torch / OMP threads (2); the watcher's lane 'mac' runs cells_mac_jobs // 2
                   measurements at once (cells_mac_jobs 4: 4 processes on the Mac), the two chooser matches of each
                   at once; the same games, seeds, seats and baselines (the same cache keys) as 'local', every game
                   recorded as a game record (src 'mac')
  cells_local_jobs, cells_local_split
                   the cells' local games (docs/ENGINE.md 199; off by default: one measurement at a time, its chooser
                   matches one after the other): the watcher's local cells lane ('gpu' with cells_device 'cuda', else
                   'cpu_bo1') runs cells_local_jobs measurements at once (1; the oldest due first, a cell after its
                   baseline: lane_parallel - a local best of one in that lane is one of them), and with
                   cells_local_split (false) each measurement plays its two chooser matches at once, each in a process
                   of its own (mtgz.az.local_judge: play_pair_local's h2h.match exactly - the same games, results and
                   baseline cache keys); up to 2 x cells_local_jobs match processes, each with bo1_threads search
                   threads and bo1_groups driver groups (with several processes: bo1_groups 1, bo1_threads about the
                   cores over the processes); refused with cells_backend 'farm' / 'mac' (nothing local), and
                   cells_local_split with sequential cells
  cells_seed       the seeds' base (0: the same deals every generation)
  cells_format     'bo1' (the mains; 'bo3' cells: the extension point `_cell_spec`)
  cells_max_turns  the cell specs' max_turns (60, as decklists.mirror_spec)
  cells_promote    a cell whose expected score against the reference (the logistic of its Elo gain) passes this
                   flags a new reference in the report (0.80)
  cells_base_elo   the first reference's anchored Elo (1000, zero.elo's anchor)
  cells_cache      the reference-vs-reference baselines' cache directory (default <run>/yardsticks/cells_base)
  cells_boot       bootstrap resamples of each cell's paired seeds (1000; 0 = the delta method only)
  refmatch_publish a mirror cells result against a reference from OUTSIDE the run (a cells_refs 'LABEL=PATH' that is
                   no file of the run) posts the run's reference-match record refmatch.<run> on delverai
                   (mtgz.az.site_refmatch, docs/ENGINE.md 225; true; false: PUBLISH OFF, said at each such result)
  cells_reuse_base a cell's control - its pair's reference-vs-reference baseline - is played on the cells' own backend
                   (docs/ENGINE.md 217: one backend per comparison; 'local' and 'mac' are one, torch's): a cached
                   baseline of another backend is not reused but played again and kept beside it (`resolve_base`).
                   true: reuse it (said in the log and in the row's `base_backend`) - the opt-out (false)

The reference pool and the joint fit (docs/ENGINE.md 186; mtgz.az.cells_fit; off unless cells_mode is 'pool'):

  cells_mode       'ladder' (177: one reference at a time, chained) or 'pool': each deck pair of a checkpoint played
                   against the pool member whose predicted score is nearest 0.5 and the next-older one, every cells
                   game fitted in one model per pair (a 'cells_fit' row per measured checkpoint)
  cells_pool_active     promoted references kept active beside generation 0 (3)
  cells_promote_every   a checkpoint is promoted this many iterations after the newest reference (64; 0 = never on
                   the cadence), or sooner when a cell's expected score against it passes cells_promote / falls below
                   1 - cells_promote
  cells_auto_promote    promote by the fit itself (false: suggested, `run_yardsticks.py cells-freeze` does it)
  cells_main_share the main member's share of cells_games (0.75; the rest: the continuity member)
  cells_gen0_share generation 0's fixed share when it is neither the main nor the continuity member (0.1, at least
                   one batch; 0 = none): the long-range anchor kept tight
  cells_fit_boot   the fit's bootstrap resamples over deals (200)

Results: one JSON line each in <run>/yardsticks.jsonl (append-only; the watcher is the only writer, under the lock
<run>/yardsticks/lock.json):

  {"kind": "bo1", "iter", "opp", "opp_label", "opp_sha" (12 hex), "score", "se", "elo_diff", "elo_se", "games",
   "unfinished", "pairs" {n, both_won, split, both_lost, with_draw, pentanomial, score, se}, "pentanomial", "sims",
   "select", "spec_sha", "seed", "seconds", "time"}
  {"kind": "blunders", "iter", "passed", "total", "share", "kinds" {kind: [passed, n]}, "library", "spots", "stale",
   "sims", "repeats", "select", "seconds", "time"}
  {"kind": "cells" | "cells_gen", "iter", "ref", "ref_sha", "sha", "pair" [deck, deck], "cells" {"X|Y": the cell's
   statistics (cell_stats)}, "games", "unfinished", "base", "results" {"0", "1": the checkpoint's result per game id
   of each chooser spec, '0' / 'h' / '1'}, "sims", "select", "seed", "spec_sha", "backend", "seconds", "time"}
  {"kind": "cells_base", "ref", "ref_sha", "pair", "base" (its cache key), "games", "score", "sims", "select",
   "seconds", "time"} - the reference-vs-reference games themselves are in its cache file
  the pool mode: a cells row also has "role" ('main' | 'cont') and "base_games"; and {"kind": "cells_fit", "iter",
   "sha", "fit_v", the checkpoint's "cells" / "decks" / "overall" / "anchored" ratings, "flags", "promote", "plan",
   "pool", "pairs", "gens" (every measured generation's), "method", ...} - docs/ENGINE.md 186.6
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
import math
import os
import random
import re
import shutil
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
            'timeout_s': 7200, 'poll': 60,
            # the metagame cells (docs/ENGINE.md 177)
            'cells_games': 0, 'cells_gen_games': 0, 'cells_decks': None, 'cells_refs': [], 'cells_sims': None,
            'cells_seed': 0, 'cells_device': None, 'cells_backend': None, 'cells_format': 'bo1', 'cells_max_turns': 60,
            'cells_promote': 0.80, 'cells_base_elo': 1000.0, 'cells_cache': None, 'cells_boot': 1000,
            'cells_reuse_base': False,
            # the cost settings (docs/ENGINE.md 182): cadences, sequential stopping, sparse rows
            'bo1_every': None, 'blunders_every': None, 'cells_every': None, 'cells_gen_every': None,
            'bo1_target_se': None, 'bo1_sprt': None, 'bo1_batch': 100, 'cells_target_se': None, 'cells_batch': 100,
            'sparse': True,
            # the reference pool and the joint fit (docs/ENGINE.md 186)
            'cells_mode': 'ladder', 'cells_pool_active': 3, 'cells_promote_every': 64, 'cells_auto_promote': False,
            'cells_main_share': 0.75, 'cells_gen0_share': 0.1, 'cells_fit_boot': 200,
            # the cells' games on the MacBook (cells_backend 'mac', docs/ENGINE.md 198)
            'cells_mac_jobs': 4, 'cells_mac_threads': 4, 'cells_mac_groups': 2, 'cells_mac_torch_threads': 2,
            'cells_mac_nice': 10,
            # the cells' local games in parallel (docs/ENGINE.md 199)
            'cells_local_jobs': 1, 'cells_local_split': False,
            # cells against an outside reference posted as the run's reference-match record (docs/ENGINE.md 225)
            'refmatch_publish': True}
# the Delver presets' (the user, 2026-11-14: "our evaluations should test bo1 ability at all to look for regressions",
# and the blunder tracker on every evaluation)
DELVER_YARDSTICKS = {'bo1_games': 400, 'bo1_sims': 64, 'bo1_opponents': ['anchors', 'iter0'], 'blunders': True}

_NUM = (int, float)
KEYS = {'bo1_games': int, 'bo1_sims': int, 'bo1_opponents': list, 'bo1_device': str, 'bo1_threads': int,
        'bo1_groups': int, 'bo1_spec': str, 'bo1_seed': int, 'bo1_backend': str, 'bo1_farm_share': _NUM,
        'bo1_farm_wait_s': _NUM, 'blunders': bool, 'blunders_sims': int, 'library': str,
        'from': int, 'settle': _NUM, 'retry_s': _NUM, 'retry_max_s': _NUM, 'timeout_s': _NUM, 'poll': _NUM,
        'cells_games': int, 'cells_gen_games': int, 'cells_decks': list, 'cells_refs': list, 'cells_sims': int,
        'cells_seed': int, 'cells_device': str, 'cells_backend': str, 'cells_format': str, 'cells_max_turns': int,
        'cells_promote': _NUM, 'cells_base_elo': _NUM, 'cells_cache': str, 'cells_boot': int, 'cells_reuse_base': bool,
        'bo1_every': int, 'blunders_every': int, 'cells_every': int, 'cells_gen_every': int, 'bo1_target_se': _NUM,
        'bo1_sprt': list, 'bo1_batch': int, 'cells_target_se': _NUM, 'cells_batch': int, 'sparse': bool,
        'cells_mode': str, 'cells_pool_active': int, 'cells_promote_every': int, 'cells_auto_promote': bool,
        'cells_main_share': _NUM, 'cells_gen0_share': _NUM, 'cells_fit_boot': int,
        'cells_mac_jobs': int, 'cells_mac_threads': int, 'cells_mac_groups': int, 'cells_mac_torch_threads': int,
        'cells_mac_nice': int, 'cells_local_jobs': int, 'cells_local_split': bool, 'refmatch_publish': bool}
# a kind's cadence setting (docs/ENGINE.md 182)
EVERY = {'bo1': 'bo1_every', 'blunders': 'blunders_every', 'cells': 'cells_every', 'cells_gen': 'cells_gen_every'}
SEQ_SEED = 7919                           # batch k's seed offset (zero.h2h.sequential's)
# The metagame of the 3-deck run (docs/ENGINE.md 177): Izzet Delver, Legacy Doomsday, Sneak and Show (data/decks)
CELL_DECKS = ['izzet_delver_8016541', 'doomsday_8027957', 'sneak_show_8014220']
CELLS_FORMATS = ('bo1',)                  # ('bo3' cells: the extension point, _cell_spec)
CELL_KINDS = ('cells', 'cells_gen', 'cells_base')
FIT_KIND = 'cells_fit'                    # the pool mode's joint fit, one row per measured checkpoint (ENGINE.md 186)
CELLS_MODES = ('ladder', 'pool')
CELLS_BACKENDS = (None, 'farm', 'local', 'mac')   # ('mac': docs/ENGINE.md 198, mtgz.az.mac_judge)
KINDS = ('bo1', 'blunders') + CELL_KINDS + (FIT_KIND,)
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
    if not out['bo1_games'] and not out['blunders'] and not out['cells_games'] and not out['cells_gen_games']:
        raise ValueError('yardsticks: neither bo1_games, blunders nor cells: nothing to measure ({} turns it off)')
    _normalize_cells(out)
    _normalize_cost(out)
    for k in ('retry_s', 'retry_max_s', 'timeout_s', 'poll'):
        if out[k] <= 0:
            raise ValueError(f'yardsticks: {k} {out[k]} must be > 0')
    if out['settle'] < 0 or out['from'] < 0:
        raise ValueError('yardsticks: settle and from must be >= 0')
    return out


def _normalize_cost(out):
    """The cost settings checked in place (docs/ENGINE.md 182): cadences >= 0 (0 = null), sequential stopping only
    with local games, batches that fit."""
    for k in EVERY.values():
        if out[k] is not None and out[k] < 0:
            raise ValueError(f'yardsticks: {k} {out[k]}: an iteration count >= 1 (null / 0: every evaluated checkpoint)')
        if not out[k]:
            out[k] = None
    if out['bo1_sprt'] is not None:
        e = out['bo1_sprt']
        if len(e) != 2 or not all(isinstance(x, _NUM) and not isinstance(x, bool) for x in e) or e[1] <= e[0]:
            raise ValueError(f'yardsticks: bo1_sprt {e!r}: [elo0, elo1] with elo1 > elo0')
    if out['bo1_target_se'] is not None and out['bo1_target_se'] <= 0:
        raise ValueError('yardsticks: bo1_target_se must be > 0 (a score standard error, e.g. 0.02)')
    if bo1_sequential(out):
        if out['bo1_batch'] < 2 or out['bo1_batch'] % 2 or out['bo1_batch'] > max(2, out['bo1_games']):
            raise ValueError(f"yardsticks: bo1_batch {out['bo1_batch']}: an even number of paired games, at most "
                             f"bo1_games ({out['bo1_games']})")
        if out['bo1_backend'] == 'farm':
            raise ValueError('yardsticks: a sequential best of one (bo1_target_se / bo1_sprt) plays here: '
                             "bo1_backend 'farm' is refused with it")
    if out['cells_target_se'] is not None:
        if out['cells_target_se'] <= 0:
            raise ValueError('yardsticks: cells_target_se must be > 0 (a cell\'s Elo standard error, e.g. 35)')
        b = out['cells_batch']
        if b < 4 or b % 4:
            raise ValueError(f'yardsticks: cells_batch {b}: a multiple of 4 (games a cell per batch)')
        for k in ('cells_games', 'cells_gen_games'):
            if out[k] % b:
                raise ValueError(f'yardsticks: {k} {out[k]} is not a multiple of cells_batch {b}')
        if out['cells_backend'] in ('farm', 'mac'):
            raise ValueError(f"yardsticks: sequential cells (cells_target_se) play here: cells_backend "
                             f"{out['cells_backend']!r} is refused with it")
        if out['cells_local_split']:
            raise ValueError('yardsticks: sequential cells (cells_target_se) play their batches in order, both chooser '
                             'specs a batch: cells_local_split is refused with it')


def bo1_sequential(ys):
    """Whether the best of one stops sequentially (bo1_target_se or bo1_sprt)."""
    return ys.get('bo1_target_se') is not None or ys.get('bo1_sprt') is not None


def cells_sequential(ys):
    return ys.get('cells_target_se') is not None


def due(ys, kind, it):
    """Whether kind `kind` measures evaluated checkpoint `it` (its cadence, docs/ENGINE.md 182; a cells_base goes
    with the measurement that needs it)."""
    n = ys.get(EVERY.get(kind, ''))
    return not n or int(it) % int(n) == 0


def _normalize_cells(out):
    """The cells' options checked and resolved in place (cells_sims / cells_device: bo1's when null; cells_decks: the
    metagame's three when null)."""
    for k in ('cells_games', 'cells_gen_games'):
        if out[k] < 0 or out[k] % 2:
            raise ValueError(f'yardsticks: {k} {out[k]}: an even number >= 0 (seeds played from both sides; 0 = none)')
    out['cells_decks'] = list(out['cells_decks'] or CELL_DECKS)
    ds = out['cells_decks']
    if not ds or not all(isinstance(x, str) and re.fullmatch(r'[A-Za-z0-9_.-]+', x) for x in ds) or len(set(ds)) != len(ds):
        raise ValueError(f'yardsticks: cells_decks {ds!r}: distinct data/decks names')
    if out['cells_sims'] is None:
        out['cells_sims'] = out['bo1_sims']
    if out['cells_device'] is None:
        out['cells_device'] = out['bo1_device']
    if out['cells_sims'] < 1:
        raise ValueError(f"yardsticks: cells_sims {out['cells_sims']} must be >= 1")
    if out['cells_device'] not in ('cuda', 'cpu'):
        raise ValueError(f"yardsticks: cells_device {out['cells_device']!r}: 'cuda' or 'cpu'")
    if out['cells_backend'] not in CELLS_BACKENDS:
        raise ValueError(f"yardsticks: cells_backend {out['cells_backend']!r}: 'farm', 'local', 'mac' or null (auto)")
    if out['cells_backend'] == 'mac' and out['cells_device'] != 'cpu':
        raise ValueError(f"yardsticks: cells_backend 'mac' plays on the Mac's CPU (no CUDA there): cells_device "
                         f"{out['cells_device']!r} is refused with it (give 'cpu')")
    for k in ('cells_mac_threads', 'cells_mac_groups', 'cells_mac_torch_threads'):
        if out[k] < 1:
            raise ValueError(f'yardsticks: {k} {out[k]} must be >= 1')
    if out['cells_mac_jobs'] < 2:
        raise ValueError(f"yardsticks: cells_mac_jobs {out['cells_mac_jobs']}: >= 2 (a measurement's two chooser "
                         f"matches play at once)")
    if not 0 <= out['cells_mac_nice'] <= 20:
        raise ValueError(f"yardsticks: cells_mac_nice {out['cells_mac_nice']}: in [0, 20]")
    if out['cells_local_jobs'] < 1:
        raise ValueError(f"yardsticks: cells_local_jobs {out['cells_local_jobs']}: >= 1 (measurements at once in the "
                         f"cells' local lane; 1: one at a time)")
    if out['cells_backend'] in ('farm', 'mac') and (out['cells_local_jobs'] > 1 or out['cells_local_split']):
        raise ValueError(f"yardsticks: cells_local_jobs / cells_local_split are the cells' local games: cells_backend "
                         f"{out['cells_backend']!r} plays none here (refused: they would do nothing)")
    if out['cells_format'] not in CELLS_FORMATS:
        raise ValueError(f"yardsticks: cells_format {out['cells_format']!r}: {list(CELLS_FORMATS)} (best-of-three cells "
                         f"are not built yet: docs/ENGINE.md 177)")
    if not 0.5 < out['cells_promote'] < 1:
        raise ValueError(f"yardsticks: cells_promote {out['cells_promote']}: in (0.5, 1)")
    if out['cells_boot'] < 0 or out['cells_max_turns'] < 1:
        raise ValueError('yardsticks: cells_boot >= 0 and cells_max_turns >= 1')
    for r in out['cells_refs']:
        _ref_entry(r)                           # (ValueError for a bad one)
    if out['cells_games'] and not out['cells_refs']:
        raise ValueError("yardsticks: cells_games without cells_refs (the first reference: 'iter0' or 'LABEL=PATH')")
    if out['cells_mode'] not in CELLS_MODES:
        raise ValueError(f"yardsticks: cells_mode {out['cells_mode']!r}: {list(CELLS_MODES)}")
    if out['cells_pool_active'] < 1 or out['cells_promote_every'] < 0 or out['cells_fit_boot'] < 0:
        raise ValueError('yardsticks: cells_pool_active >= 1, cells_promote_every >= 0 (0: the threshold only) and '
                         'cells_fit_boot >= 0')
    if not 0 < out['cells_main_share'] <= 1:
        raise ValueError(f"yardsticks: cells_main_share {out['cells_main_share']}: in (0, 1]")
    if not 0 <= out['cells_gen0_share'] < 1 or out['cells_main_share'] + out['cells_gen0_share'] > 1:
        raise ValueError(f"yardsticks: cells_gen0_share {out['cells_gen0_share']}: in [0, 1), with cells_main_share "
                         f"at most 1")
    if out['cells_mode'] == 'pool' and not out['cells_games']:
        raise ValueError('yardsticks: cells_mode pool needs cells_games (the games against the pool, docs/ENGINE.md 186)')


def cells_pool(ys):
    """Whether the cells are measured against a reference pool with the joint fit (docs/ENGINE.md 186)."""
    return (ys or {}).get('cells_mode') == 'pool' and bool(ys.get('cells_games'))


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
    from .forced_find import refuse
    if ys.get('bo1_spec'):
        refuse(ys['bo1_spec'], 'yardsticks bo1_spec')
        return ys['bo1_spec']
    s = cfg.get('eval_spec') or cfg.get('spec')
    if not s:
        raise ValueError('yardsticks: the run has no eval_spec / spec to play best-of-one from (give bo1_spec)')
    d = json.loads(s) if isinstance(s, str) else dict(s)
    if 'forced_find' in d:
        # (the run's self-play spec's forced find, docs/ENGINE.md 180: a training aid - the yardstick plays without it)
        d.pop('forced_find')
        s = json.dumps(d, separators=(',', ':'))
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
                if isinstance(r, dict) and r.get('kind') in KINDS and (
                        isinstance(r.get('iter'), int) or (r.get('kind') == 'cells_base' and r.get('base'))):
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
    """A measurement's key: ('bo1', iter, opponent), ('blunders', iter, library id), ('cells' | 'cells_gen', iter,
    reference, pair) or ('cells_base', its cache key)."""
    r = row_or_item
    if r['kind'] == 'bo1':
        return ('bo1', r['iter'], r['opp'])
    if r['kind'] in ('cells', 'cells_gen'):
        return (r['kind'], r['iter'], r['ref'], tuple(r['pair']))
    if r['kind'] == 'cells_base':
        return ('cells_base', r['base'])
    if r['kind'] == FIT_KIND:
        return (FIT_KIND, r['iter'])
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


def wanted(run, cfg, ys, lib=None, now=None, settle=None, rows=None):
    """Every measurement the run's evaluated checkpoints call for, in order (by iteration; bo1 in opponent order) ->
    [{'lane', 'kind', 'iter', 'opp'?, 'library'?}]. `lib`: the canary library's id (None: no readable library with
    kept spots - its measurements are wanted all the same and fail loudly until there is one)."""
    its = evaluated(run, cfg, ys, now, settle)
    opps = opponents(run, cfg, ys) if ys['bo1_games'] else []
    cw = CellsWant(run, cfg, ys, now, rows) if (ys['cells_games'] or ys['cells_gen_games']) else None
    out = []
    for it in its:
        for o in (opps if due(ys, 'bo1', it) else []):
            if it == 0 and (o['name'] == 'gen0' or same_file(o['path'], model_path(run, 0))):
                continue                        # (generation 0 against itself, or a copy of it, measures nothing)
            out.append({'lane': bo1_lane(run, cfg, ys, o), 'kind': 'bo1', 'iter': it,
                        'opp': o['name']})
        if cw is not None:                      # (the cells: each pair's baseline first, once)
            out += cw.items(it)
        if ys['blunders'] and due(ys, 'blunders', it):     # (no library: measured all the same - fails loudly, retried)
            out.append({'lane': 'cpu', 'kind': 'blunders', 'iter': it, 'library': lib})
    return out


def pending(run, cfg, ys, lane, rows=None, lib=None, now=None, settle=None, busy=()):
    """The lane's next measurement: the oldest wanted one without a result whose retry wait (after a failure) is
    over -> item or None. `busy`: the keys of the lane's measurements under way (a lane running several at once,
    lane_parallel: skipped - and so is a cell whose baseline is under way, item_ready)."""
    now = time.time() if now is None else now
    rows = read_rows(run) if rows is None else rows
    res, fails = tally(rows)
    for item in wanted(run, cfg, ys, lib, now, settle, rows=rows):
        if item['lane'] != lane:
            continue
        k = key_of(item, lib)
        if k in res or k in busy or item_done(run, ys, item):
            continue
        if not item_ready(run, ys, item):
            continue                            # (a cell waiting for its baseline, which comes first in the lane)
        if any(a not in res for a in item.get('after') or ()):
            continue                            # (a checkpoint's fit waiting for its measurements)
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
    miss, failed, cmiss, cfailed = [], [], [], []
    for item in wanted(run, cfg, ys, lib, now, settle=0, rows=rows):
        k = key_of(item, lib)
        if k in res or item_done(run, ys, item):
            continue
        cell = item['kind'] in CELL_KINDS or item['kind'] == FIT_KIND         # (the cells' own lists: the site's block knows bo1 and blunders)
        x = {kk: item[kk] for kk in ('kind', 'iter', 'opp') + (('ref', 'pair', 'base', 'games', 'role') if cell else ())
             if kk in item}
        f = fails.get(k)
        if f:
            (cfailed if cell else failed).append(dict(x, attempts=len(f), error=str(f[-1].get('error'))[-400:],
                                                      retry_at=f[-1].get('retry_at')))
        else:
            (cmiss if cell else miss).append(x)
    try:
        opps = [{'name': o['name'], 'label': o['label']} for o in opponents(run, cfg, ys)] if ys['bo1_games'] else []
    except ValueError:
        opps = []
    cells = None
    if ys['cells_games'] or ys['cells_gen_games']:
        cells = {'report': cells_report(run, cfg, ys, rows), 'missing': cmiss, 'failed': cfailed}
        if cells_pool(ys):                      # (the newest joint fit, docs/ENGINE.md 186.6)
            cells['fit'] = cells['report'].get('fit')
    return {'expected': evaluated(run, cfg, ys, now, settle=0), 'cells': cells,
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
    if bo1_sequential(ys):
        return 'local'                          # (a sequential best of one plays here, docs/ENGINE.md 182)
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


def bo1_precision(cfg):
    """The network arithmetic of a local best-of-one (both sides): the run's `infer_precision` (zero-core docs/az.md
    57; 'fp32' when unset). The cells stay float32 (their cached baselines are float32; docs/ENGINE.md 183), and so
    does a farm job (ONNX float32)."""
    return str(cfg.get('infer_precision') or 'fp32').lower()


def records_dir(run, cfg):
    """The run's game records directory when it keeps its games (zero-core's run setting `game_records`, docs/gamedb.md):
    every yardstick game is then a record under <run>/records/<kind>/ (kinds bo1, cells, cells_gen, cells_base); else
    None (nothing written, the games of before)."""
    return os.path.join(run, 'records') if (cfg or {}).get('game_records') else None


def _sink(records, kind, it, a_label, b_label, meta=None):
    """A measurement's game records writer (zero.gamerec.MatchSink: A and B as zero.h2h's sides), None without
    `records`."""
    if not records:
        return None
    from zero import gamerec as GR
    return GR.MatchSink(records, kind, it=it, labels={'a': a_label, 'b': b_label}, meta=meta)


def _ck_label(it, ck):
    from zero import gamerec as GR
    return f'iter_{int(it):04d}' if it is not None else GR.ckpt_id(ck)


def _farm_records(sink, rows, spec, a, b, sims, rule, meta=None):
    """A farm play job's traced rows (each with its game's record: eval_trace, mtgz.az.farm_campaign) -> the sink's
    records, A (`a`) in seat gid % 2 as the job's seats [net 1, net 2] put it (a baseline: both the reference)."""
    if sink is None:
        return
    from zero import gamerec as GR
    pa = GR.net_player(a, sink.labels.get('a'), {'sims': sims}, rule)
    pb = GR.net_player(b, sink.labels.get('b'), {'sims': sims}, rule)
    items = []
    for r in rows:
        if not isinstance(r.get('rec'), dict):
            raise RuntimeError(f'{sink.kind} (farm): game {r.get("gid")} has no record (eval_trace off?): not recorded')
        items.append(GR.from_trace(r['rec'], spec, sink.kind, [pa, pb] if int(r['gid']) % 2 == 0 else [pb, pa], sink.it,
                                   'farm'))
    sink.add(items, meta)


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
    from zero import gamerec as GR
    sink = _sink(records_dir(run, cfg), 'bo1', it, _ck_label(it, ck), o['name'], {'opp': opp})
    with GR.recording(sink):
        return _measure_bo1_local(run, it, opp, o, ck, cfg, ys)


def _measure_bo1_local(run, it, opp, o, ck, cfg, ys):
    from zero import h2h
    spec = bo1_spec(cfg, ys)
    seat = bo1_seat(ck)
    t0 = time.time()
    kw = dict(seed=ys['bo1_seed'], threads=ys['bo1_threads'], groups=ys['bo1_groups'], device=ys['bo1_device'],
              spec=spec, seat_a=dict(seat), seat_b=dict(seat), paired=True, sparse=bool(ys['sparse']))
    prec = bo1_precision(cfg)
    if prec != 'fp32':                          # (docs/ENGINE.md 183: both sides at the run's infer_precision)
        kw['precision'] = prec
    seq = bo1_sequential(ys)
    if seq:                                     # (docs/ENGINE.md 182: batches until the stop rule, at most bo1_games)
        out = h2h.sequential(ck, o['path'], batch_games=ys['bo1_batch'], max_games=ys['bo1_games'],
                             sprt=tuple(ys['bo1_sprt']) if ys['bo1_sprt'] is not None else None,
                             target_se=ys['bo1_target_se'], sims=ys['bo1_sims'], **kw)
        ok = 0 < out.get('games', 0) <= ys['bo1_games'] and out['games'] % 2 == 0
    else:
        out = h2h.match(ck, o['path'], ys['bo1_games'], ys['bo1_sims'], **kw)
        ok = out.get('games') == ys['bo1_games']
    if not ok:
        raise RuntimeError(f"best-of-one {it} vs {opp}: {out.get('games')} games of {ys['bo1_games']}")
    pairs = out.get('pairs') or {}
    row = {'kind': 'bo1', 'iter': int(it), 'opp': opp, 'opp_label': o['label'], 'opp_sha': sha12(o['path']),
           'score': out['a_score'], 'se': out['se'], 'elo_diff': out.get('elo_diff'), 'elo_se': out.get('elo_se'),
           'games': out['games'], 'unfinished': out.get('unfinished', 0), 'pairs': pairs,
           'pentanomial': pairs.get('pentanomial'), 'sims': ys['bo1_sims'], 'select': out.get('select_a'),
           'spec_sha': hashlib.sha256(spec.encode()).hexdigest()[:12], 'seed': ys['bo1_seed'],
           'seconds': round(time.time() - t0, 1), 'time': round(time.time())}
    if seq:
        row.update(stop=out.get('stop'), batches=len(out.get('batches') or []), batch=ys['bo1_batch'],
                   max_games=ys['bo1_games'], target_se=ys['bo1_target_se'])
        if out.get('sprt'):
            row['sprt'] = out['sprt']
    if prec != 'fp32':
        row['precision'] = prec
    return row


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
    from . import h2h_farm as HF
    res = C.collect(d, client=client, threads=ys['bo1_threads'], wrap=HF.topup_wrap, describe_games=False, opts=opts,
                    **kw)
    out = res['readout']
    if out is None or out.get('games') != ys['bo1_games']:
        raise RuntimeError(f"best-of-one {it} vs {o['name']} (farm): {out and out.get('games')} games of {ys['bo1_games']}")
    sink = _sink(records_dir(run, cfg), 'bo1', it, _ck_label(it, ck), o['name'], {'opp': o['name']})
    if sink is not None:
        # (game records, docs/gamedb.md: the job's traced games, A = net 1 in seat gid % 2)
        _farm_records(sink, sorted(C.games(d), key=lambda r: r['gid']), spec, ck, o['path'], ys['bo1_sims'],
                      h2h._tag(C.match_seat(ck, ys['bo1_sims'], rule)),
                      {'driver_seed': int(ys['bo1_seed']), 'paired': True, 'backend': 'farm'})
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


# ------------------------------------------------------------------------------------------------ the cells
# The metagame cells (docs/ENGINE.md 177): the 3-deck run's network pilots every deck against every deck, so its
# progress is measured per ordered cell (my deck X, the opponent's deck Y). For a cell, checkpoint C pilots X against a
# frozen reference R piloting Y, and the baseline is R piloting X against R piloting Y on the SAME seeds (common
# random numbers: the same shuffles, the same chooser): the cell's Elo gain is 400 log10 of the odds ratio of the two
# scores. One unit is one seed played twice, once with X's side choosing play or draw (CR 103.1, the spec's
# `chooser`) and once with Y's side choosing - the bo1 yardstick's pentanomial pair, with the side that chooses
# swapped instead of the seats. The four games of a unit (C or R as X, X's side choosing or not) share the unit's
# random numbers and are correlated; units are independent. So every standard error is taken over units: the delta
# method on the units' (C, R) score pairs (their covariance included: the CRN's gain) and a bootstrap over units.
#
# The games: an unordered deck pair {X, Y} is two specs (p0 = X, p1 = Y; chooser 0 and chooser 1), each a zero.h2h
# match of A against B with A in seat gid % 2 (its convention; a farm eval job's too), unpaired, the pair's own seed.
# Even game ids are cell X|Y (A pilots X), odd ones cell Y|X; game g of the chooser-0 spec and game g of the chooser-1
# spec are one unit (the same seed: game_seed(seed, g)). A mirror {X, X}: every game is cell X|X, and the baseline is
# 0.5 exactly (R against R with the sides swapped by the chooser is symmetric), so it is the ordinary paired
# head-to-head and plays no baseline games. The baseline of a non-mirror pair (R against R, the same two specs and
# seeds) is cached by (reference, specs, seed, games, sims, seats, engine build): computed once.

def _ref_entry(r):
    """A cells_refs entry -> {'label', 'path' (None: generation 0 of the run), 'since', ...}; ValueError for a bad one."""
    if isinstance(r, str):
        if r == 'iter0':
            return {'label': 'gen0', 'path': None, 'since': 0}
        lb, sep, p = r.partition('=')
        if not sep or not p.strip():
            raise ValueError(f"yardsticks: cells_refs entry {r!r}: 'iter0', 'LABEL=PATH' or an object")
        e = {'label': lb.strip(), 'path': p.strip(), 'since': 0}
    elif isinstance(r, dict):
        bad = sorted(set(r) - {'label', 'path', 'since', 'elo', 'elo_se', 'iter', 'sha', 'from_ref', 'gain', 'gain_se',
                               'time'})
        if bad or not isinstance(r.get('label'), str) or not isinstance(r.get('path'), str):
            raise ValueError(f'yardsticks: cells_refs entry {r!r}: {{"label", "path", "since"?, "elo"?, "elo_se"?}}')
        e = dict(r, since=int(r.get('since') or 0))
    else:
        raise ValueError(f'yardsticks: cells_refs entry {r!r}: a string or an object')
    if not re.fullmatch(r'[A-Za-z0-9_.-]+', e['label']) or e['label'].startswith('iter_'):
        raise ValueError(f"yardsticks: cells reference label {e['label']!r}: [A-Za-z0-9_.-], not iter_* (the "
                         f"gen-over-gen kind's)")
    return e


def ladder_path(run):
    """The references frozen by `run_yardsticks.py cells-freeze` (the only writer: an explicit command)."""
    return os.path.join(run, DIR, 'cells_ladder.json')


def read_ladder(run):
    try:
        with open(ladder_path(run), encoding='utf-8') as f:
            d = json.load(f)
    except FileNotFoundError:
        return []
    return [_ref_entry(r) for r in d.get('refs') or []]


def cell_refs(run, cfg, ys):
    """The reference ladder, oldest first: the setting's cells_refs, then the frozen ones (cells_ladder.json) ->
    [{'label', 'path', 'since', ...}]. A checkpoint is measured against the newest reference whose `since` it reached
    (ref_for). ValueError: a label twice, or `since` going backwards."""
    out = []
    for r in list(ys['cells_refs']) + (read_ladder(run) if run else []):
        e = _ref_entry(r) if not isinstance(r, dict) or 'since' not in r else dict(r)
        if e['path'] is None:
            e['path'] = model_path(run, 0)
        out.append(e)
    seen = set()
    for i, e in enumerate(out):
        if e['label'] in seen:
            raise ValueError(f"yardsticks: two cells references labelled {e['label']!r}")
        seen.add(e['label'])
        if i and e['since'] < out[i - 1]['since']:
            raise ValueError(f"yardsticks: cells reference {e['label']!r} since {e['since']} < the one before it")
    return out


def ref_for(refs, it):
    """The reference checkpoint `it` is measured against: the newest whose `since` <= it (None: none yet)."""
    cur = None
    for r in refs:
        if r['since'] <= it:
            cur = r
    return cur


def ref_path(run, cfg, ys, label):
    """A reference label's checkpoint: 'iter_NNNN' (the gen-over-gen kind's) or a ladder reference."""
    m = re.fullmatch(r'iter_(\d+)', label)
    if m:
        return model_path(run, int(m.group(1)))
    for r in cell_refs(run, cfg, ys):
        if r['label'] == label:
            return r['path']
    raise ValueError(f'yardsticks: no cells reference {label!r} in this run\'s ladder')


def cell_pairs(ys):
    """The unordered deck pairs, in cells_decks order: (X, X) and (X, Y) for X before Y."""
    d = ys['cells_decks']
    return [(d[i], d[j]) for i in range(len(d)) for j in range(i, len(d))]


def cell_id(x, y):
    """Cell 'X|Y': X piloted by the measured side, against Y."""
    return f'{x}|{y}'


def _cell_spec(x, y, chooser, ys):
    """The spec of p0 = X against p1 = Y with `chooser` choosing play or draw (JSON): decklists.pair_spec (docs/ENGINE.md
    176: exactly the metagame spec's games of that pairing) with the spec's `chooser` set (CR 103.1: the side that
    chooses, the unit's two halves). Best-of-three cells (the extension point): pair_spec(fmt='bo3') builds the pair's
    match; there the chooser is game 1's only and each later game's is the previous game's loser (ENGINE.md 35), and
    the statistics take one result per game id, a match's as well as a game's - not turned on yet (cells_format)."""
    if ys['cells_format'] != 'bo1':
        raise NotImplementedError(f"cells_format {ys['cells_format']!r}: best-of-three cells are not built yet")
    k = (x, y, int(chooser), int(ys['cells_max_turns']))
    if k not in _SPECS:                         # (memoized: the pool's watcher asks for every member's keys each look)
        from . import decklists as D
        spec = json.loads(D.pair_spec(x, y, fmt='bo1', max_turns=int(ys['cells_max_turns'])))
        spec['chooser'] = int(chooser)
        from .forced_find import refuse
        refuse(spec, 'the cells yardstick')     # (never: pair_spec has no forced find, docs/ENGINE.md 180)
        _SPECS[k] = json.dumps(spec, separators=(',', ':'))
    return _SPECS[k]


_SPECS = {}


def pair_seed(ys, pair):
    """A deck pair's seed (cells_seed and the pair's names: each pair its own deals, the same every generation)."""
    h = hashlib.sha256(f"cells|{ys['cells_seed']}|{pair[0]}|{pair[1]}".encode()).digest()
    return int.from_bytes(h[:4], 'little')


def pair_jobs(pair, games, ys):
    """The pair's two matches -> [{'chooser', 'spec', 'games', 'seed'}]: `games` per cell (a mirror: games / 2 a
    spec, every game its cell's; else `games` a spec, half of them each cell's)."""
    n = games // 2 if pair[0] == pair[1] else games
    return [{'chooser': c, 'spec': _cell_spec(pair[0], pair[1], c, ys), 'games': n, 'seed': pair_seed(ys, pair)}
            for c in (0, 1)]


def engine_sha():
    """The engine build's sha256 (12 hex) - the module `import mtgz._engine` loads - or None."""
    import importlib.machinery
    for suf in importlib.machinery.EXTENSION_SUFFIXES:
        p = os.path.join(ROOT, 'mtgz', '_engine' + suf)
        if os.path.exists(p):
            return sha12(p)
    return None


_SEAT = {}


def cells_seat(run):
    """Both sides' seat in every cell match (bo1_seat: the run's evaluation rule without the deck tilt), memoized by
    the run's config file."""
    p = os.path.join(run, 'config.json')
    try:
        k = (os.path.normcase(os.path.abspath(run)), os.path.getmtime(p))
    except OSError:
        k = (os.path.normcase(os.path.abspath(run)), None)
    if k not in _SEAT:
        _SEAT[k] = bo1_seat(model_path(run, 0))
    return dict(_SEAT[k])


_SEATKEYS = {}


def _seat_keys(ref):
    from zero import h2h
    k = os.path.normcase(os.path.abspath(ref))
    if k not in _SEATKEYS:
        _SEATKEYS[k] = h2h.run_seat_keys(ref)
    return _SEATKEYS[k]


def base_key(ref, pair, games, ys, seat, sha=None, eng=None):
    """The cache key of a pair's reference-vs-reference baseline -> (16 hex, the dict it hashes): the reference's
    network and its own seat keys, the two specs, the seed, the games, sims, the seat (move rule) and the engine
    build. OSError when the reference file is missing. `sha` / `eng`: the reference's sha12 and engine_sha() when
    the caller has them (memoized on everything the key reads)."""
    sha = sha or sha12(ref)
    eng = engine_sha() if eng is None else eng
    mk = (os.path.normcase(os.path.abspath(ref)), sha, tuple(pair), games, json.dumps(seat, sort_keys=True, default=str),
          ys['cells_sims'], ys['cells_seed'], ys['cells_batch'] if cells_sequential(ys) else None, ys['cells_format'],
          ys['cells_max_turns'], eng)
    if mk in _BKEY:
        return _BKEY[mk][0], dict(_BKEY[mk][1])
    d = {'v': 1, 'ref': sha, 'ref_seat': _seat_keys(ref), 'pair': list(pair), 'seed': pair_seed(ys, pair),
         'specs': [hashlib.sha256(j['spec'].encode()).hexdigest()[:12] for j in pair_jobs(pair, games, ys)],
         'games': games, 'sims': ys['cells_sims'], 'seat': seat, 'chance_k': 2, 'c_puct': 1.0, 'engine': eng}
    if cells_sequential(ys):
        d['batch'] = ys['cells_batch']          # (batch k seeded seed + 7919 k: other games than the plain match's)
    _BKEY[mk] = (hashlib.sha256(json.dumps(d, sort_keys=True, default=str).encode()).hexdigest()[:16], d)
    return _BKEY[mk][0], dict(d)


_BKEY = {}


def cache_dir(run, ys):
    return ys['cells_cache'] or os.path.join(run, DIR, 'cells_base')


def base_file(cdir, key):
    return os.path.join(cdir, f'{key}.json')


def load_base(cdir, key):
    try:
        with open(base_file(cdir, key), encoding='utf-8') as f:
            return json.load(f)
    except (OSError, ValueError):
        return None


def backend_family(backend):
    """A baseline's backend as the same-backend rule reads it: 'farm', or 'local' (this machine's torch and the Mac's:
    the 'mac' backend plays play_pair_local's matches there, docs/ENGINE.md 198; a baseline without one: 'local')."""
    return 'farm' if backend == 'farm' else 'local'


def resolve_base(cdir, key, backend, reuse=False):
    """The cache key of a pair's baseline for cells played on `backend` (docs/ENGINE.md 217: one backend per
    comparison) -> (key, note): the baseline's own key (`base_key`) when its file is absent (the first writer's) or holds
    a baseline of the same backend family (`backend_family`), else the key beside it for this backend (`key` + the
    family, hashed) - a baseline of another backend is never this comparison's control. `reuse` (cells_reuse_base, the
    opt-out): the own key whatever played it, `note` naming the other backend."""
    data = load_base(cdir, key)
    if data is None:
        return key, None
    have = backend_family(data.get('backend'))
    if have == backend_family(backend):
        return key, None
    if reuse:
        return key, f"reusing the control's baseline {key} played on {data.get('backend') or 'local'} (cells_reuse_base)"
    return hashlib.sha256(f'{key}|{backend_family(backend)}'.encode()).hexdigest()[:16], None


def item_done(run, ys, item):
    """A baseline is done when its cache file exists (whoever computed it); every other measurement by its row."""
    return item['kind'] == 'cells_base' and bool(item.get('base')) and os.path.exists(
        base_file(cache_dir(run, ys), item['base']))


def item_ready(run, ys, item):
    """A non-mirror cell measurement waits for its pair's baseline (wanted before it, in the same lane)."""
    if item['kind'] not in ('cells', 'cells_gen') or not item.get('base'):
        return True
    return os.path.exists(base_file(cache_dir(run, ys), item['base']))


_CBACKEND = {}


def cells_backend(run, cfg, ys, ref):
    """Where a reference's cells are played: cells_backend, else (auto, as bo1_backend) 'farm' when the run has a
    farm pool and one eval job seats both sides exactly (match_settings), else 'local'."""
    if ys.get('cells_backend'):
        return ys['cells_backend']
    if cells_sequential(ys):
        return 'local'                          # (sequential cells play here, docs/ENGINE.md 182)
    from . import farm_campaign as C
    if C.run_pool(cfg) is None:
        return 'local'
    pair = next((p for p in cell_pairs(ys) if p[0] != p[1]), cell_pairs(ys)[0])
    k = (os.path.normcase(os.path.abspath(run)), ref, ys['cells_sims'], tuple(ys['cells_decks']))
    if k not in _CBACKEND:
        try:
            C.match_settings(model_path(run, 0), ys['cells_sims'], _cell_spec(pair[0], pair[1], 0, ys),
                             tilts=bo1_tilts(cfg), ckpt_b=ref)
            _CBACKEND[k] = 'farm'
        except (C.CampaignError, ValueError, OSError):
            _CBACKEND[k] = 'local'
    return _CBACKEND[k]


def cells_lane(run, cfg, ys, ref):
    b = cells_backend(run, cfg, ys, ref)
    if b in ('farm', 'mac'):
        return b                                # (the Mac's lane: its own, measurements in parallel - lane_parallel)
    return cells_local_lane(ys)


def cells_local_lane(ys):
    """The watcher lane of the cells' local games: 'gpu' (cells_device 'cuda') or 'cpu_bo1' (a local best of one on
    the same device shares it)."""
    return 'gpu' if ys['cells_device'] == 'cuda' else 'cpu_bo1'


def lane_parallel(ys, lane):
    """Measurements a watcher lane runs at once: the 'mac' lane cells_mac_jobs // 2 (each measurement's two chooser
    matches at once on the Mac: cells_mac_jobs processes there; docs/ENGINE.md 198), the cells' local lane
    cells_local_jobs (1 by default; docs/ENGINE.md 199), every other lane one at a time."""
    if not ys:
        return 1
    if lane == 'mac':
        return max(1, int(ys['cells_mac_jobs']) // 2)
    n = int(ys.get('cells_local_jobs') or 1)
    if n > 1 and lane == cells_local_lane(ys):
        return n
    return 1


def lanes(run, cfg, ys):
    """Every lane of the setting (the watcher's threads): the best-of-ones', the cells' and the canary library's."""
    out = set(bo1_lanes(run, cfg, ys))
    if ys['cells_games']:
        out |= {cells_lane(run, cfg, ys, r['path']) for r in cell_refs(run, cfg, ys)}
    if ys['cells_gen_games']:
        out.add(cells_lane(run, cfg, ys, model_path(run, 0)))
    if cells_pool(ys):
        out.add('fit')                          # (the joint fit of each measured checkpoint, docs/ENGINE.md 186)
    if ys['blunders']:
        out.add('cpu')
    return sorted(out)


class CellsWant:
    """The cells measurements of each evaluated checkpoint (wanted): per deck pair, its baseline (once per cache key,
    before the first measurement that needs it) and the checkpoint's measurement; against the current reference
    ('cells') and against the previously evaluated checkpoint ('cells_gen'). The pool mode (docs/ENGINE.md 186): the
    pool's baselines, each pair against its most informative member and the continuity one, then the fit."""

    def __init__(self, run, cfg, ys, now=None, rows=None):
        self.run, self.cfg, self.ys = run, cfg, ys
        self.refs = cell_refs(run, cfg, ys) if ys['cells_games'] else []
        self.by_label = {r['label']: r for r in self.refs}
        self.rows, self._res, self._fits = rows, None, None
        self._shas, self._eng = {}, None
        self.prev_fit = None                    # (the previous checkpoint's fit item: items() goes oldest first)
        # (cells_gen's chain: the evaluated checkpoints on its cadence)
        self.all = [x for x in evaluated(run, cfg, dict(ys, **{'from': 0}), now, settle=0) if due(ys, 'cells_gen', x)] \
            if ys['cells_gen_games'] else []
        self.seen = set()
        self._seat = None

    def seat(self):
        if self._seat is None:
            self._seat = cells_seat(self.run)
        return self._seat

    def _pairs(self, kind, it, label, path, games):
        out = []
        lane = cells_lane(self.run, self.cfg, self.ys, path)
        for pair in cell_pairs(self.ys):
            key = None
            if pair[0] != pair[1]:
                key = self._key(path, pair, games, cells_backend(self.run, self.cfg, self.ys, path))
                if key and key not in self.seen:
                    self.seen.add(key)
                    out.append({'lane': lane, 'kind': 'cells_base', 'ref': label, 'pair': list(pair), 'games': games,
                                'base': key})
            out.append({'lane': lane, 'kind': kind, 'iter': it, 'ref': label, 'pair': list(pair), 'base': key})
        return out

    def res(self):
        if self._res is None:
            self._res = tally(read_rows(self.run) if self.rows is None else self.rows)[0]
        return self._res

    def _key(self, path, pair, games, backend='local'):
        try:
            if path not in self._shas:
                self._shas[path] = sha12(path)
            if self._eng is None:
                self._eng = [engine_sha()]
            key = base_key(path, pair, games, self.ys, self.seat(), sha=self._shas[path], eng=self._eng[0])[0]
        except OSError:                         # (no reference file: the measurement fails loudly, retried)
            return None
        # (the baseline of the cells' own backend, docs/ENGINE.md 217)
        return resolve_base(cache_dir(self.run, self.ys), key, backend, self.ys.get('cells_reuse_base'))[0]

    def fits(self):
        """The point fit of every recorded game (memoized by the rows: every lane looks each poll)."""
        if self._fits is None:
            from . import cells_fit as CF
            rows = read_rows(self.run) if self.rows is None else self.rows
            k = (os.path.normcase(os.path.abspath(self.run)), len(rows), rows[-1].get('time') if rows else None,
                 json.dumps(self.ys, sort_keys=True, default=str))
            if k not in _POINT:
                _POINT.clear()
                _POINT[k] = CF.point_fits(self.run, self.cfg, self.ys, rows)
            self._fits = _POINT[k]
        return self._fits

    def choose(self, it, pair, pool):
        """The pool member whose predicted scores against checkpoint `it` are nearest 0.5 in the pair (its own
        ratings once rated in the pair, else the newest measured checkpoint's before it)."""
        from . import cells_fit as CF
        if len(pool) == 1:
            return pool[0]['label']
        fits, col = self.fits()
        f = fits.get(tuple(pair))
        roles = ('X',) if pair[0] == pair[1] else ('X', 'Y')

        def rated(sha):
            return f is not None and sha is not None and all(f['count'].get((r, sha)) for r in roles)
        try:
            c = sha12(model_path(self.run, it))
        except OSError:
            c = None
        if not rated(c):
            olds = sorted(((n['iter'], s) for s, n in col['names'].items() if s in col['measured']
                           and n['iter'] is not None and n['iter'] < it and rated(s)), reverse=True)
            c = olds[0][1] if olds else None
        cands = []
        for r in pool:
            try:
                cands.append(dict(r, sha=sha12(r['path'])))
            except OSError:
                cands.append(dict(r, sha=None))
        return CF.choose(fits, tuple(pair), c, cands)[0]

    def plan(self, it, pool):
        """Checkpoint `it`'s games per pair -> {pair: [(reference, games, role)]}: the most informative member
        ('main', cells_main_share of cells_games), the next-older one ('cont', the rest; none when the main one is
        the anchor) and, when the anchor is neither, the anchor ('gen0', cells_gen0_share). Fixed once played: a
        pair with a result row keeps its references (a row of the ladder mode, no role: as it was measured)."""
        from . import cells_fit as CF
        fixed = {}
        for r in self.res().values():
            if r['kind'] == 'cells' and r.get('iter') == it:
                fixed.setdefault(tuple(r['pair']), []).append(r)
        out = {}
        G = self.ys['cells_games']
        for pair in cell_pairs(self.ys):
            rs = fixed.get(pair, [])
            if any(r.get('role') is None for r in rs):
                out[pair] = [(r['ref'], G, None) for r in rs]
                continue
            main = next((r['ref'] for r in rs if r.get('role') == 'main'), None)
            cont = next((r['ref'] for r in rs if r.get('role') == 'cont'), None)
            if main is None:
                main = self.choose(it, pair, [r for r in pool if r['label'] != cont] or pool)
            order = [r for r in self.refs if r in pool or r['label'] == main]
            if cont is None:
                cont = CF.continuity(order, main)
            anchor = self.refs[0]['label']
            g0 = anchor not in (main, cont) and anchor in self.by_label
            gm, gc, gz = CF.split_games(self.ys, cont is not None, gen0=g0)
            out[pair] = ([(main, gm, 'main')] + ([(cont, gc, 'cont')] if cont and gc else [])
                         + ([(anchor, gz, 'gen0')] if gz else []))
        return out

    def _pool(self, it):
        from . import cells_fit as CF
        ck = model_path(self.run, it)
        pool = [r for r in CF.active_refs(self.refs, it, self.ys['cells_pool_active']) if not same_file(ck, r['path'])]
        if not pool:
            return []
        G = self.ys['cells_games']
        out = []

        def base(r, pair):
            key = self._key(r['path'], pair, G) if pair[0] != pair[1] else None
            if key and key not in self.seen:
                self.seen.add(key)
                out.append({'lane': cells_lane(self.run, self.cfg, self.ys, r['path']), 'kind': 'cells_base',
                            'ref': r['label'], 'pair': list(pair), 'games': G, 'base': key})
            return key
        for r in pool:                          # (every member's baselines, once: its games' and the fit's links)
            for pair in cell_pairs(self.ys):
                base(r, pair)
        for pair, legs in self.plan(it, pool).items():
            for label, games, role in legs:
                r = self.by_label.get(label)
                if r is None:
                    continue                    # (a reference no longer in the ladder: its row stays a record)
                key = base(r, pair)
                out.append({'lane': cells_lane(self.run, self.cfg, self.ys, r['path']), 'kind': 'cells', 'iter': it,
                            'ref': label, 'pair': list(pair), 'base': key, 'games': games, 'role': role})
        return out

    def items(self, it):
        out = []
        ck = model_path(self.run, it)
        if self.ys['cells_games'] and due(self.ys, 'cells', it) and cells_pool(self.ys):
            out += self._pool(it)
        elif self.ys['cells_games'] and due(self.ys, 'cells', it):
            r = ref_for(self.refs, it)
            if r is not None and not same_file(ck, r['path']):      # (the reference itself measures nothing)
                out += self._pairs('cells', it, r['label'], r['path'], self.ys['cells_games'])
        if self.ys['cells_gen_games'] and due(self.ys, 'cells_gen', it):
            prev = max((x for x in self.all if x < it), default=None)
            if prev is not None and not same_file(ck, model_path(self.run, prev)):
                out += self._pairs('cells_gen', it, f'iter_{prev:04d}', model_path(self.run, prev),
                                   self.ys['cells_gen_games'])
        if cells_pool(self.ys) and any(x['kind'] == 'cells' for x in out):
            # (once its measurements are in, and after the previous checkpoint's fit: promotions go in order)
            after = [key_of(x) for x in out if x['kind'] in ('cells', 'cells_gen')]
            if self.prev_fit is not None:
                after.append((FIT_KIND, self.prev_fit))
            out.append({'lane': 'fit', 'kind': FIT_KIND, 'iter': it, 'after': after})
            self.prev_fit = it
        return out


_POINT = {}


# ---------------------------------------------------------------- the statistics
ELO_K = 400.0 / math.log(10.0)          # Elo points per unit of log-odds
_CODE = {0.0: '0', 0.5: 'h', 1.0: '1'}
_DECODE = {'0': 0.0, 'h': 0.5, '1': 1.0}


def res_code(xs):
    """A match's results of side A per game id -> '0' / 'h' / '1' per game (a list of floats if any is another
    value)."""
    xs = [float(x) for x in xs]
    if all(x in _CODE for x in xs):
        return ''.join(_CODE[x] for x in xs)
    return [round(x, 6) for x in xs]


def res_decode(s):
    return [_DECODE[c] for c in s] if isinstance(s, str) else [float(x) for x in s]


def cell_units(pair, res):
    """The pair's two matches' results of A (res {0: [per game id], 1: [...]}: chooser seat 0, chooser seat 1) ->
    {cell id: [(A's side chose, the other side chose) per unit]} - A in seat gid % 2 piloting that seat's deck."""
    r = (list(res[0]), list(res[1]))
    if len(r[0]) != len(r[1]):
        raise ValueError(f'cells: the two chooser matches have {len(r[0])} and {len(r[1])} games')
    out = {}
    for g in range(len(r[0])):
        seat = g % 2
        cid = cell_id(pair[seat], pair[1 - seat])
        out.setdefault(cid, []).append((r[seat][g], r[1 - seat][g]))
    return out


def logit(p):
    return math.log(p / (1.0 - p))


def expected_score(elo):
    return 1.0 / (1.0 + 10.0 ** (-float(elo) / 400.0))


def _mean(xs):
    return sum(xs) / len(xs)


def _var(xs):
    n = len(xs)
    if n < 2:
        return 0.0
    m = _mean(xs)
    return sum((x - m) ** 2 for x in xs) / (n - 1)


def cell_stats(a, b=None, boot=1000, seed=0):
    """One cell's statistics. `a`: the measured side's units [(its side chose, the other chose)], `b`: the
    baseline's units on the same seeds, in the same order (None: a mirror, whose baseline is 0.5 exactly).

    elo = 400 log10( pa/(1-pa) / (pb/(1-pb)) ) with pa, pb the two mean scores (clamped to half a game from 0 and 1:
    `clamped`). se, the delta method over the n paired units: with u_i, v_i the units' mean scores and
    w = 1/(p(1-p)), the estimate's variance is Var(wa u_i - wb v_i) / n, the covariance of u and v included (the
    same seeds: positively correlated, which is the point); se_unpaired leaves it out (as if the baseline were
    independent games) to show what the pairing saves. boot_se / boot_95: `boot` resamples of whole units (seeded),
    the same estimate. vs_ref: the logistic of elo - the measured side's expected score against the baseline's
    pilot in the cell's terms (0.5 = no gain), which the ladder's promotion threshold reads."""
    n = len(a)
    if n == 0:
        raise ValueError('cells: a cell with no units')
    if b is not None and len(b) != n:
        raise ValueError(f'cells: {n} units against a baseline of {len(b)}')
    games = 2 * n
    eps = 0.5 / games
    clamp = lambda p: min(max(p, eps), 1.0 - eps)  # noqa: E731
    u = [(x + y) / 2 for x, y in a]
    v = [(x + y) / 2 for x, y in b] if b is not None else None
    pa = _mean(u)
    pb = _mean(v) if v is not None else 0.5
    ca, cb = clamp(pa), clamp(pb)
    elo = ELO_K * (logit(ca) - logit(cb))
    wa, wb = 1.0 / (ca * (1 - ca)), 1.0 / (cb * (1 - cb))
    d = [wa * ui - (wb * vi if v is not None else 0.0) for ui, vi in zip(u, v or [0.0] * n)]
    se = ELO_K * math.sqrt(_var(d) / n)
    se_unp = ELO_K * math.sqrt((wa * wa * _var(u) + (wb * wb * _var(v) if v is not None else 0.0)) / n)
    out = {'units': n, 'games': games, 'score': round(pa, 4), 'base': round(pb, 4), 'elo': round(elo, 1),
           'se': round(se, 1), 'se_unpaired': round(se_unp, 1), 'vs_ref': round(expected_score(elo), 4),
           'clamped': pa != ca or pb != cb,
           'chose': {'score': round(_mean([x for x, _ in a]), 4), 'base': round(_mean([x for x, _ in b]), 4)
                     if b is not None else None},
           'other_chose': {'score': round(_mean([y for _, y in a]), 4), 'base': round(_mean([y for _, y in b]), 4)
                           if b is not None else None},
           'pentanomial': [sum(1 for x, y in a if int(round(2 * (x + y))) == k) for k in range(5)],
           'corr': None}
    if v is not None and _var(u) > 0 and _var(v) > 0:
        mu, mv = _mean(u), _mean(v)
        cov = sum((x - mu) * (y - mv) for x, y in zip(u, v)) / (n - 1)
        out['corr'] = round(cov / math.sqrt(_var(u) * _var(v)), 4)
    if boot and n > 1:
        rng = random.Random(seed)
        es = []
        for _ in range(int(boot)):
            idx = [rng.randrange(n) for _ in range(n)]
            qa = clamp(sum(u[i] for i in idx) / n)
            qb = clamp(sum(v[i] for i in idx) / n) if v is not None else 0.5
            es.append(ELO_K * (logit(qa) - logit(qb)))
        es.sort()
        out['boot_se'] = round(math.sqrt(_var(es)), 1)
        out['boot_95'] = [round(es[int(0.025 * (len(es) - 1))], 1), round(es[int(0.975 * (len(es) - 1))], 1)]
    return out


def _cell_seed(cid):
    return int.from_bytes(hashlib.sha256(cid.encode()).digest()[:4], 'little')


def summarize(cells, decks, base=None, promote=0.80):
    """The 9 (N x N) cells of one checkpoint against one reference -> per-deck pilot Elo (the mean of the deck's
    cells as the measured side's deck), the overall Elo (the mean of every cell), and with `base` (the reference's
    anchored (elo, se)) the anchored Elo: base + overall. The cells are independent (disjoint seeds), so a mean of k
    cells has se sqrt(sum se^2) / k; an anchored se adds the reference's in quadrature. A mean needs every one of its
    cells (no partial means: `missing` names the absent ones). `promote`: the cells whose vs_ref passes the threshold
    (a new reference is suggested)."""
    ids = [cell_id(x, y) for x in decks for y in decks]
    out = {'cells': {c: {k: cells[c][k] for k in ('elo', 'se', 'boot_se', 'score', 'base', 'vs_ref', 'games',
                                                   'clamped') if k in cells[c]} for c in ids if c in cells},
           'missing': [c for c in ids if c not in cells], 'decks': {}, 'overall': None, 'anchored': None}

    def mean(cs, nd=1):
        return {'elo': round(_mean([cells[c]['elo'] for c in cs]), nd),
                'se': round(math.sqrt(sum(cells[c]['se'] ** 2 for c in cs)) / len(cs), nd)}
    for x in decks:
        cs = [cell_id(x, y) for y in decks]
        if all(c in cells for c in cs):
            out['decks'][x] = mean(cs)
    out['complete'] = not out['missing']
    if out['complete']:
        out['overall'] = mean(ids)
        if base is not None and base[0] is not None:
            raw = mean(ids, 9)
            out['anchored'] = {'elo': round(base[0] + raw['elo'], 1),
                               'se': round(math.sqrt((base[1] or 0.0) ** 2 + raw['se'] ** 2), 1)}
    out['promote'] = [c for c in ids if c in cells and cells[c]['vs_ref'] > promote]
    return out


# ---------------------------------------------------------------- the games
def batch_games(pair, ys):
    """Sequential cells: games of each chooser spec per batch (cells_batch games a cell: a mirror's spec plays half,
    every game its one cell's)."""
    return ys['cells_batch'] // 2 if pair[0] == pair[1] else ys['cells_batch']


def play_pair_local(a, b, pair, games, ys, seat, log=print, stop=None):
    """The pair's two matches A (`a`) against B (`b`; the same file for a baseline) by zero.h2h.match (unpaired: A in
    seat gid % 2), both sides at `seat` -> {'results': {'0': [A's result per game id], '1': ...}, 'unfinished',
    'select'}. Sequential cells (cells_target_se, docs/ENGINE.md 182): the specs in batches (batch k of each seeded
    its seed + 7919 k, game ids following on), `stop(results)` asked after each batch (None: every batch, a
    baseline's) -> also 'batches', 'stop'. cells_local_split (docs/ENGINE.md 199): the two matches at once, each this
    same h2h.match call in a process of its own (mtgz.az.local_judge) -> the same dict."""
    if cells_sequential(ys):
        return _play_pair_batches(a, b, pair, games, ys, seat, log, stop)
    if ys.get('cells_local_split'):
        from . import local_judge as LJ
        return LJ.play_pair_split(a, b, pair, games, ys, seat, log=log)
    from zero import gamerec as GR, h2h
    out = {'results': {}, 'unfinished': 0, 'select': None}
    sink = GR.active()
    for j in pair_jobs(pair, games, ys):
        t0 = time.time()
        with GR.recording(sink.with_meta(pair=list(pair), chooser=j['chooser']) if sink is not None else None):
            m = h2h.match(a, b, j['games'], ys['cells_sims'], seed=j['seed'], threads=ys['bo1_threads'],
                          groups=ys['bo1_groups'], device=ys['cells_device'], spec=j['spec'], seat_a=dict(seat),
                          seat_b=dict(seat), paired=False, rows=True, sparse=bool(ys['sparse']))
        rows = m['rows']
        if [r['gid'] for r in rows] != list(range(j['games'])):
            raise RuntimeError(f"cells {pair} chooser {j['chooser']}: {len(rows)} games of {j['games']}")
        out['results'][str(j['chooser'])] = [float(r['result'][r['gid'] % 2]) for r in rows]
        out['unfinished'] += int(m.get('unfinished', 0))
        out['select'] = m.get('select_a')
        log(f"cells {pair[0]} / {pair[1]} chooser seat {j['chooser']}: {j['games']} games, A {m['a_score']:.4f}, "
            f"{time.time() - t0:.0f} s")
    return out


def _play_pair_batches(a, b, pair, games, ys, seat, log, stop):
    from zero import gamerec as GR, h2h
    sink = GR.active()
    jobs = pair_jobs(pair, games, ys)
    per = batch_games(pair, ys)
    nb = jobs[0]['games'] // per
    out = {'results': {str(j['chooser']): [] for j in jobs}, 'unfinished': 0, 'select': None, 'batches': 0,
           'stop': 'max_games'}
    t0 = time.time()
    for k in range(nb):
        for j in jobs:
            with GR.recording(sink.with_meta(pair=list(pair), chooser=j['chooser'], batch=k, gid_offset=k * per)
                              if sink is not None else None):
                m = h2h.match(a, b, per, ys['cells_sims'], seed=j['seed'] + SEQ_SEED * k, threads=ys['bo1_threads'],
                              groups=ys['bo1_groups'], device=ys['cells_device'], spec=j['spec'], seat_a=dict(seat),
                              seat_b=dict(seat), paired=False, rows=True, sparse=bool(ys['sparse']))
            rows = m['rows']
            if [r['gid'] for r in rows] != list(range(per)):
                raise RuntimeError(f"cells {pair} chooser {j['chooser']} batch {k}: {len(rows)} games of {per}")
            out['results'][str(j['chooser'])] += [float(r['result'][r['gid'] % 2]) for r in rows]
            out['unfinished'] += int(m.get('unfinished', 0))
            out['select'] = m.get('select_a')
        out['batches'] = k + 1
        if k + 1 < nb and stop is not None and k >= 1 and stop(out['results']):
            out['stop'] = 'target_se'
            break
    n = len(out['results']['0'])
    log(f"cells {pair[0]} / {pair[1]}: {out['batches']} of {nb} batches ({n} games a chooser spec), stop "
        f"{out['stop']}, {time.time() - t0:.0f} s")
    return out


def play_pair_farm(run, cfg, rule_ck, a, b, pair, games, ys, name, log=None, client=None, opts=None, pool=None,
                   own_base=False):
    """The pair's two matches as farm play jobs (docs/ENGINE.md 165; mtgz.az.farm_campaign): the run's pool worker
    build (`pool`: another - the release index's build of the checkout, a standalone cells-match's, docs/ENGINE.md 217),
    the seats of play_pair_local checked dict for dict (match_settings of `rule_ck`, the run's checkpoint whose rule
    both sides play, against the reference; `own_base`: each side's own run's margin_w / redet / eval_cache as its
    seat's set, farm_campaign.match_job - a reference without a run config, e.g. an A/B's copied start), unpaired, each
    its own campaign `name`-c<chooser> (a retry resumes it), its games traced, spot-replayed here, the missing ones played
    here after bo1_farm_wait_s (each piece holding a slot of the heavy-command throttle while a run is live here:
    h2h_farm.topup_wrap). A baseline (`a` is `b`): self seats [net 1, net 1] of the reference. -> play_pair_local's dict
    + 'farm' {chooser: digest, spot, local, by_route}."""
    from zero import farm_eval as FE, gamerec as GR, h2h
    from . import farm_campaign as C, h2h_farm as HF
    pool = pool or C.run_pool(cfg)
    if pool is None:
        raise RuntimeError('cells_backend farm: the run has no farm pool (pool:<farm run> with pool_build, pool_worker)')
    same = os.path.normcase(os.path.abspath(a)) == os.path.normcase(os.path.abspath(b))
    ref = b
    wait = ys['bo1_farm_wait_s'] or 0.75 * float(ys['timeout_s'])
    kw = {'log': log} if log else {}
    out = {'results': {}, 'unfinished': 0, 'select': None, 'farm': {}}
    for j in pair_jobs(pair, games, ys):
        if own_base:
            settings, rule, seats = C.match_job(rule_ck, a, b, ys['cells_sims'], j['spec'], tilts=bo1_tilts(cfg))
        else:
            settings, rule = C.match_settings(rule_ck, ys['cells_sims'], j['spec'], tilts=bo1_tilts(cfg), ckpt_b=ref)
            seats = [{'net': 1}, {'net': 1}] if same else [{'net': 1}, {'net': 2}]
        d = C.submit(f"{name}-c{j['chooser']}", a, j['games'], settings, seats, j['seed'], pool['build_id'],
                     pool['worker'], pool['runtime'], b=None if same else b, paired=False, share=ys['bo1_farm_share'],
                     deadline_s=wait, min_agent=pool.get('min_agent'),
                     meta={'yardstick': 'cells', 'run': run, 'pair': list(pair), 'chooser': j['chooser']},
                     client=client, opts=opts, **kw)
        res = C.collect(d, client=client, threads=ys['bo1_threads'], wrap=HF.topup_wrap, describe_games=False, opts=opts,
                        **kw)
        rows = sorted(C.games(d), key=lambda r: r['gid'])
        FE.a_seat_of(rows, {'net': 1})
        if [r['gid'] for r in rows] != list(range(j['games'])):
            raise RuntimeError(f"cells {pair} chooser {j['chooser']} (farm): {len(rows)} games of {j['games']}")
        # (game records, docs/gamedb.md: the job's traced games under the measurement's sink, A = net 1 in seat gid % 2)
        _farm_records(GR.active(), rows, j['spec'], a, b, ys['cells_sims'],
                      h2h._tag(C.match_seat(rule_ck, ys['cells_sims'], rule)),
                      {'pair': list(pair), 'chooser': j['chooser'], 'driver_seed': int(j['seed']), 'paired': False,
                       'backend': 'farm'})
        out['results'][str(j['chooser'])] = [float(r['result'][r['gid'] % 2]) for r in rows]
        out['unfinished'] += sum(1 for r in rows if not r.get('over'))
        out['select'] = h2h._tag(C.match_seat(rule_ck, ys['cells_sims'], rule))
        out['farm'][str(j['chooser'])] = {'run': res['run'], 'digest': res['digest'],
                                          'spot': {k: res['spot'][k] for k in ('checked', 'ok', 'of')},
                                          'local': res['coverage']['local'], 'seconds': res['seconds'],
                                          'by_route': res.get('by_route')}
    return out


def _play(backend, a, b, pair, games, ys, seat, farm, log, stop=None):
    if backend == 'farm':
        return play_pair_farm(farm['run'], farm['cfg'], farm['rule_ck'], a, b, pair, games, ys, farm['name'],
                              log=farm.get('log'), client=farm.get('client'), opts=farm.get('opts'),
                              pool=farm.get('pool'), own_base=farm.get('own_base', False))
    if backend == 'mac':                       # (docs/ENGINE.md 198: play_pair_local's matches on the MacBook)
        from . import mac_judge as MJ
        return MJ.play_pair_mac(a, b, pair, games, ys, seat, log=log)
    kw = {'stop': stop} if stop is not None else {}  # (sequential cells only)
    return play_pair_local(a, b, pair, games, ys, seat, log=log, **kw)


def _prefix(res, n):
    """A pair's results {chooser: [per game id]} cut to the first `n` game ids of each spec."""
    return {int(c): list(r)[:n] for c, r in res.items()}


def _spec_sha(pair, games, ys):
    return hashlib.sha256('\n'.join(j['spec'] for j in pair_jobs(pair, games, ys)).encode()).hexdigest()[:12]


def measure_base_core(ref, label, pair, games, ys, seat, cdir, backend='local', farm=None, log=print, records=None):
    """A pair's baseline (the reference against itself, the two specs and seeds), from the cache or played and
    cached -> its 'cells_base' row. `records`: the game records directory (docs/gamedb.md; kind 'cells_base': the
    games played, none for a cached baseline), None: no records."""
    if pair[0] == pair[1]:
        raise ValueError(f'cells: a mirror ({pair[0]}) has no baseline games (0.5 exactly)')
    key, kd = base_key(ref, pair, games, ys, seat)
    key, note = resolve_base(cdir, key, backend, ys.get('cells_reuse_base'))   # (the control on this backend: 217)
    if note:
        log(f'cells {pair[0]} / {pair[1]}: {note}')
    data = load_base(cdir, key)
    t0 = time.time()
    cached = data is not None
    if data is None:
        from zero import gamerec as GR
        with GR.recording(_sink(records, 'cells_base', None, label, label, {'ref': label})):
            p = _play(backend, ref, ref, pair, games, ys, seat, farm, log)
        data = dict(kd, key=key, results={c: res_code(r) for c, r in p['results'].items()},
                    unfinished=p['unfinished'], select=p['select'], backend=backend,
                    seconds=round(time.time() - t0, 1), time=round(time.time()))
        os.makedirs(cdir, exist_ok=True)
        tmp = base_file(cdir, key) + f'.{os.getpid()}.tmp'
        with open(tmp, 'w', encoding='utf-8', newline='') as f:
            json.dump(data, f, separators=(',', ':'))
        os.replace(tmp, base_file(cdir, key))
    units = cell_units(pair, {int(c): res_decode(r) for c, r in data['results'].items()})
    return {'kind': 'cells_base', 'ref': label, 'ref_sha': sha12(ref), 'pair': list(pair), 'base': key,
            'games': games, 'scores': {c: round(_mean([(x + y) / 2 for x, y in us]), 4) for c, us in units.items()},
            'unfinished': data.get('unfinished', 0), 'sims': ys['cells_sims'], 'select': data.get('select'),
            'backend': data.get('backend'), 'cached': cached, 'seconds': round(time.time() - t0, 1),
            'time': round(time.time()), **({'base_backend': data.get('backend') or 'local', 'for_backend': backend}
                                           if note else {})}


def measure_cells_core(kind, ck, it, ref, label, pair, games, ys, seat, cdir, backend='local', farm=None, log=print,
                       base_games=None, role=None, records=None):
    """Checkpoint `ck` against reference `ref` in the pair's cells (its baseline from the cache; none for a mirror)
    -> the 'cells' / 'cells_gen' row. The pool mode (docs/ENGINE.md 186): `games` a share of the pair's, against the
    baseline of `base_games` (the reference's one baseline, cells_games: its first game ids are these games' seeds)
    cut to the same first units; `role` ('main' | 'cont' | 'gen0') goes in the row. `records`: the game records
    directory (docs/gamedb.md: kind `kind`), None: no records."""
    for p in (ck, ref):
        if not os.path.exists(p):
            raise FileNotFoundError(p)
    base_games = int(base_games or games)
    if games > base_games or games % 2:
        raise ValueError(f'cells {pair}: {games} games against a baseline of {base_games}')
    key, bunits, bres = None, None, None
    base_backend = None
    if pair[0] != pair[1]:
        key, _ = base_key(ref, pair, base_games, ys, seat)
        key, note = resolve_base(cdir, key, backend, ys.get('cells_reuse_base'))   # (the control on this backend: 217)
        data = load_base(cdir, key)
        if data is None:
            raise RuntimeError(f'cells {pair}: no baseline {key} in {cdir} for the {backend_family(backend)} backend '
                               f'(measured first: kind cells_base)')
        if note:
            log(f'cells {pair[0]} / {pair[1]}: {note}')
            base_backend = data.get('backend') or 'local'
        bres = {int(c): res_decode(r) for c, r in data['results'].items()}
    stop = None
    if cells_sequential(ys):
        target = float(ys['cells_target_se'])

        def stop(res):
            n = len(res['0'])
            us = cell_units(pair, {int(c): r for c, r in res.items()})
            bs = cell_units(pair, _prefix(bres, n)) if bres is not None else None
            return all(cell_stats(u, bs[c] if bs is not None else None, boot=0)['se'] <= target for c, u in us.items())
    t0 = time.time()
    from zero import gamerec as GR
    with GR.recording(_sink(records, kind, it, _ck_label(it, ck), label, {'ref': label, **({'role': role} if role else {})})):
        p = _play(backend, ck, ref, pair, games, ys, seat, farm, log, stop=stop)
    units = cell_units(pair, {int(c): r for c, r in p['results'].items()})
    if bres is not None:                        # (the baseline's same first units: all of them unless stopped early)
        bunits = cell_units(pair, _prefix(bres, len(p['results']['0'])))
    cells = {c: cell_stats(us, bunits[c] if bunits is not None else None, boot=ys['cells_boot'], seed=_cell_seed(c))
             for c, us in units.items()}
    row = {'kind': kind, 'iter': it, 'ref': label, 'ref_sha': sha12(ref), 'sha': sha12(ck), 'pair': list(pair),
           'cells': cells, 'games': sum(len(r) for r in p['results'].values()), 'unfinished': p['unfinished'],
           'base': key, 'results': {c: res_code(r) for c, r in p['results'].items()}, 'sims': ys['cells_sims'],
           'select': p['select'], 'seed': pair_seed(ys, pair), 'spec_sha': _spec_sha(pair, games, ys),
           'backend': backend, 'seconds': round(time.time() - t0, 1), 'time': round(time.time())}
    if p.get('farm'):
        row['farm'] = p['farm']
    if p.get('mac'):
        row['mac'] = p['mac']
    if base_backend is not None:                # (the opt-out: a control of another backend, said in the row)
        row['base_backend'] = base_backend
    if role is not None:                        # (the pool mode's legs, docs/ENGINE.md 186.2)
        row.update(role=role, base_games=base_games)
    if cells_sequential(ys):
        row.update(stop=p.get('stop'), batches=p.get('batches'), batch=ys['cells_batch'], max_games=games,
                   target_se=ys['cells_target_se'])
    return row


def _farm(run, cfg, rule_ck, kind, it, ref, pair, games, ys):
    h = hashlib.sha256(json.dumps([kind, it, sha12(ref), list(pair), games, ys['cells_seed'], ys['cells_sims']]).encode())
    return {'run': run, 'cfg': cfg, 'rule_ck': rule_ck,
            'name': f"{os.path.basename(os.path.normpath(run))[:24]}-{kind.replace('_', '')}-{h.hexdigest()[:10]}"}


def measure_cells(run, it, label, pair, cfg=None, ys=None, kind='cells', log=print, games=None, role=None):
    """A run's checkpoint `it` in one deck pair's cells against reference `label` (a ladder reference, or 'iter_NNNN'
    for the gen-over-gen kind) -> the row. `games` / `role`: a pool leg (docs/ENGINE.md 186: part of cells_games,
    against the reference's baseline of cells_games)."""
    cfg = cfg if cfg is not None else cfg_of(run)
    ys = ys if ys is not None else of_run(run, cfg)
    pair = tuple(pair)
    full = ys['cells_games'] if kind == 'cells' else ys['cells_gen_games']
    games = int(games or full)
    ref = ref_path(run, cfg, ys, label)
    backend = cells_backend(run, cfg, ys, ref)
    ck = model_path(run, it)
    return measure_cells_core(kind, ck, int(it), ref, label, pair, games, ys, cells_seat(run), cache_dir(run, ys),
                              backend, _farm(run, cfg, ck, kind, it, ref, pair, games, ys), log, base_games=full,
                              role=role, records=records_dir(run, cfg))


def measure_cells_base(run, label, pair, games, cfg=None, ys=None, log=print):
    cfg = cfg if cfg is not None else cfg_of(run)
    ys = ys if ys is not None else of_run(run, cfg)
    pair = tuple(pair)
    ref = ref_path(run, cfg, ys, label)
    if not os.path.exists(ref):
        raise FileNotFoundError(ref)
    backend = cells_backend(run, cfg, ys, ref)
    return measure_base_core(ref, label, pair, int(games), ys, cells_seat(run), cache_dir(run, ys), backend,
                             _farm(run, cfg, model_path(run, 0), 'cells_base', None, ref, pair, games, ys), log,
                             records=records_dir(run, cfg))


def standalone_history(cdir, rows_out=None):
    """The backends a standalone comparison's earlier games were played on (docs/ENGINE.md 217): its cache's baselines
    and its rows file's rows -> [(time, family)], oldest first."""
    import glob
    seen = []
    for p in glob.glob(os.path.join(cdir or '', '*.json')) if cdir else []:
        d = load_base(os.path.dirname(p), os.path.splitext(os.path.basename(p))[0])
        if isinstance(d, dict) and 'results' in d:
            seen.append((d.get('time') or os.path.getmtime(p), backend_family(d.get('backend'))))
    if rows_out and os.path.exists(rows_out):
        with open(rows_out, encoding='utf-8') as f:
            for line in f:
                try:
                    r = json.loads(line)
                except ValueError:
                    continue
                if r.get('kind') in ('cells', 'cells_gen', 'cells_base'):
                    seen.append((r.get('time') or 0, backend_family(r.get('backend'))))
    return sorted(seen)


def standalone_backend(ck, ref, ys, cdir, ask='auto', rows_out=None):
    """A standalone cells comparison's backend (`run_yardsticks.py cells-match`, `tools/ab.py judge`; docs/ENGINE.md
    217), decided before any game -> (backend, why, farm): 'local' (asked; or under 'auto' its earlier games - the
    cache's baselines, the rows file's rows - were played here: a comparison keeps its backend; or what keeps the farm
    from it) or 'farm' with the farm dict of `_play` (the release index's build of this checkout as its pool, each
    side's own base keys as its seat's set: farm_campaign.match_job). The farm's conditions are zero.h2h's farm
    backend's (mtgz.az.h2h_farm): the machine's farm keys, a clean checkout, a published build for its commit, a run
    config next to `ck` (its rule is both sides'), every chooser match's and baseline's seats the local ones, the
    build's check-config. 'farm' asked and impossible: SystemExit with the reason; earlier games on the farm and the
    farm impossible now: SystemExit too (never a mixed comparison by default)."""
    import shutil
    import tempfile
    from . import farm_campaign as C, h2h_farm as HF
    if ask == 'local':
        return 'local', 'asked (--backend local)', None
    seen = standalone_history(cdir, rows_out) if ask == 'auto' else []
    if seen and seen[-1][1] == 'local':
        return 'local', f'kept: the comparison\'s earlier games ({len(seen)} baselines / rows in its cache and rows ' \
                        f'file) were played here', None
    try:
        why = HF.can_put()
        if why:
            raise HF.NotHere(why)
        dirty = HF.tracked_changes()
        if dirty:
            raise HF.NotHere(f'the checkout has uncommitted changes to tracked files ({"; ".join(dirty[:3])}): the farm '
                             f'plays its commit\'s build')
        e = HF.release_of()
        run = os.path.dirname(os.path.dirname(os.path.abspath(ck)))
        if not os.path.exists(os.path.join(run, 'config.json')):
            raise HF.NotHere(f'{os.path.basename(ck)} has no run config next to models/ (its rule is both sides\')')
        cfg = cfg_of(run)
        games = ys['cells_games'] or ys['cells_gen_games']
        first = None
        for pair in cell_pairs(ys):
            for j in pair_jobs(pair, games, ys):
                for a, b in ((ck, ref), (ref, ref)):
                    try:
                        job = C.match_job(ck, a, b, ys['cells_sims'], j['spec'], tilts=bo1_tilts(cfg))
                    except (C.CampaignError, ValueError, OSError) as x:
                        raise HF.NotHere(f'cells {pair[0]} / {pair[1]}: {x}') from None
                    first = first or job
        d = tempfile.mkdtemp(prefix='cells-farm-')
        try:
            C.job_settings(e['worker'], e['build_id'], first[0], d, lambda m: None, seats=first[2])
        except C.CampaignError as x:
            raise HF.NotHere(f'build {e["build_id"]} refuses the job: {str(x)[-400:]}') from None
        finally:
            shutil.rmtree(d, ignore_errors=True)
    except HF.NotHere as x:
        if ask == 'farm' or seen:
            raise SystemExit(f'cells-match: backend farm refused - {x}' + (
                ' (the comparison\'s earlier games were played on the farm: --backend local mixes them explicitly)'
                if seen else ''))
        return 'local', str(x), None
    pool = {'build_id': e['build_id'], 'runtime': e['runtime'], 'worker': e['worker'], 'min_agent': None}
    farm = {'run': run, 'cfg': cfg, 'rule_ck': ck, 'pool': pool, 'own_base': True}
    return 'farm', f'build {e["build_id"][:8]} + {HF.routes_of(e)}', farm


def _standalone_name(farm, kind, ck, ref, pair, games, ys, base=None):
    """A standalone comparison's campaign (its chooser matches '-c0' / '-c1'): deterministic from what it plays, so a
    rerun resumes or collects it - a baseline by its cache key, a cell by the checkpoint, the reference and the pair."""
    h = hashlib.sha256(json.dumps([kind, base or [sha12(ck), sha12(ref)], list(pair), games, ys['cells_seed'],
                                   ys['cells_sims'], farm['pool']['build_id']]).encode()).hexdigest()[:12]
    return dict(farm, name=f"cm-{kind.replace('_', '')}-{h}")


def measure_cells_standalone(ck, ref, ys, cdir, label='ref', seat=None, rows_out=None, log=print, kind='cells',
                             records=None, backend='local', farm=None):
    """Every pair's cells of checkpoint `ck` against `ref` outside a run's watcher (the baselines from / into
    `cdir`): `seat` (default: bo1_seat of `ck` - its run's rule) -> (rows, summary). Each row is also appended
    to `rows_out` (a JSONL file) as it lands. `records`: a game records directory (docs/gamedb.md: every game played
    under <records>/cells/misc/ and <records>/cells_base/misc/), None: none. `backend` 'local' (here) or 'farm' with
    `farm` (`standalone_backend`'s): every baseline and cell a farm campaign, each pair's control (its baseline) played
    on the same backend (`resolve_base`; docs/ENGINE.md 217)."""
    seat = dict(seat) if seat is not None else bo1_seat(ck)
    rows, cells = [], {}
    games = ys['cells_games'] or ys['cells_gen_games']
    for pair in cell_pairs(ys):
        todo = []
        fb = fc = None
        if farm is not None:
            fb = _standalone_name(dict(farm, log=log), 'cells_base', ck, ref, pair, games, ys,
                                  base=base_key(ref, pair, games, ys, seat)[0])
            fc = _standalone_name(dict(farm, log=log), kind, ck, ref, pair, games, ys)
        if pair[0] != pair[1]:
            todo.append(lambda pair=pair, fb=fb: measure_base_core(ref, label, pair, games, ys, seat, cdir, backend, fb,
                                                                   log=log, records=records))
        todo.append(lambda pair=pair, fc=fc: measure_cells_core(kind, ck, None, ref, label, pair, games, ys, seat, cdir,
                                                                backend, fc, log=log, records=records))
        for f in todo:
            r = f()
            rows.append(r)
            if rows_out:
                append_jsonl(rows_out, r)
            if r['kind'] != 'cells_base':
                cells.update(r['cells'])
    return rows, summarize(cells, ys['cells_decks'], promote=ys['cells_promote'])


def append_jsonl(path, row):
    with open(path, 'a', encoding='utf-8', newline='') as f:
        f.write(json.dumps(row, separators=(',', ':')) + '\n')


# ---------------------------------------------------------------- the report and the ladder
def _groups(rows, kind):
    """{(iter, ref): {'cells': {cell id: stats}, 'sha', 'ref_sha'}} of the newest result rows of `kind`."""
    res, _ = tally(rows)
    out = {}
    for k, r in sorted(res.items(), key=lambda kv: kv[1].get('time', 0)):
        if r['kind'] != kind:
            continue
        g = out.setdefault((r['iter'], r['ref']), {'cells': {}, 'sha': r.get('sha'), 'ref_sha': r.get('ref_sha')})
        g['cells'].update(r['cells'])
    return out


def ladder(run, cfg, ys, rows=None):
    """The references with their chained anchored Elo, oldest first -> [{'label', 'path', 'since', 'sha', 'elo',
    'se', 'how'}]: the first at cells_base_elo (se 0); a later one at its entry's frozen `elo` / `elo_se` (written by
    cells-freeze), else the one before's + its own checkpoint's overall gain against that one (a complete 'cells'
    measurement of a checkpoint with its sha), se in quadrature; else unchained (elo None)."""
    refs = cell_refs(run, cfg, ys)
    groups = _groups(read_rows(run) if rows is None else rows, 'cells')
    out = []
    for i, r in enumerate(refs):
        try:
            sha = sha12(r['path'])
        except OSError:
            sha = None
        e = dict(r, sha=sha, elo=None, se=None, how='unchained')
        if i == 0:
            e.update(elo=float(ys['cells_base_elo']), se=0.0, how='base (cells_base_elo)')
        elif r.get('elo') is not None:
            e.update(elo=float(r['elo']), se=float(r.get('elo_se') or 0.0), how='frozen')
        elif out[-1]['elo'] is not None and sha:
            for (it, lb), g in groups.items():
                if lb == out[-1]['label'] and g['sha'] == sha:
                    s = summarize(g['cells'], ys['cells_decks'])
                    if s['complete']:
                        e.update(elo=out[-1]['elo'] + s['overall']['elo'],
                                 se=math.sqrt(out[-1]['se'] ** 2 + s['overall']['se'] ** 2),
                                 how=f"iteration {it} against {lb}")
                        break
        out.append(e)
    return out


def cells_report(run, cfg, ys, rows=None):
    """The cells' state -> {'refs': ladder, 'cells': [{iter, ref, sha, summary (summarize, anchored through the
    ladder)}], 'gen': [{iter, prev, summary}], 'promote': the newest complete measurement's flagged cells and its
    checkpoint (a new reference suggested: `run_yardsticks.py cells-freeze`)}."""
    rows = read_rows(run) if rows is None else rows
    try:
        lad = ladder(run, cfg, ys, rows) if ys['cells_games'] else []
    except ValueError as e:
        lad = [{'error': str(e)}]
    elo = {r['label']: (r.get('elo'), r.get('se')) for r in lad if 'label' in r}
    out = {'refs': [{k: r.get(k) for k in ('label', 'since', 'sha', 'elo', 'se', 'how', 'error') if k in r} for r in lad],
           'cells': [], 'gen': [], 'promote': None}
    order = {r['label']: i for i, r in enumerate(lad) if 'label' in r}
    try:
        refs = cell_refs(run, cfg, ys) if ys['cells_games'] else []
    except ValueError:
        refs = []
    for (it, lb), g in sorted(_groups(rows, 'cells').items(), key=lambda kv: (kv[0][0], order.get(kv[0][1], -1))):
        s = summarize(g['cells'], ys['cells_decks'], base=elo.get(lb), promote=ys['cells_promote'])
        out['cells'].append({'iter': it, 'ref': lb, 'sha': g['sha'], 'summary': s})
        cur = ref_for(refs, it)
        if s['complete'] and cur is not None and cur['label'] == lb:     # (the newest against its own reference)
            out['promote'] = {'iter': it, 'ref': lb, 'cells': s['promote']} if s['promote'] else None
    for (it, lb), g in sorted(_groups(rows, 'cells_gen').items()):
        out['gen'].append({'iter': it, 'prev': int(lb[5:]), 'sha': g['sha'],
                           'summary': summarize(g['cells'], ys['cells_decks'], promote=ys['cells_promote'])})
    out['mode'] = ys.get('cells_mode', 'ladder')
    if cells_pool(ys):                          # (docs/ENGINE.md 186: the joint fit's newest record, its promotion)
        f = newest_fit(rows)
        out['fit'] = f
        out['promote'] = None
        p = (f or {}).get('promote')
        if p and not p.get('label') and not (refs and refs[-1]['since'] >= p['iter']):
            out['promote'] = {'iter': p['iter'], 'ref': p.get('vs'), 'cells': p.get('cells') or [], 'why': p['why']}
    return out


def freeze_ref(run, it, label=None, to=None, cfg=None, ys=None, rows=None):
    """Freeze checkpoint `it` as the next reference (the explicit command `run_yardsticks.py cells-freeze`; nothing
    else writes the ladder): its complete measurement against its current reference is required (the chain link);
    the checkpoint is copied to `to` (default <run>/yardsticks/cells_refs/<label>.pt) and appended to
    <run>/yardsticks/cells_ladder.json with since = it, its chained elo / elo_se and the link's gain -> the entry.
    Checkpoints from `it` on are then measured against it."""
    cfg = cfg if cfg is not None else cfg_of(run)
    ys = ys if ys is not None else of_run(run, cfg)
    if ys is None or not ys['cells_games']:
        raise ValueError('cells-freeze: the run measures no cells (cells_games 0)')
    if cells_pool(ys):
        return freeze_pool(run, it, label, to, cfg, ys, rows)
    it = int(it)
    refs = cell_refs(run, cfg, ys)
    cur = ref_for(refs, it)
    if cur is None:
        raise ValueError(f'cells-freeze: no reference for iteration {it}')
    if refs[-1]['since'] > it:
        raise ValueError(f"cells-freeze: the newest reference {refs[-1]['label']} starts at {refs[-1]['since']} > {it}")
    label = label or f'g{it}'
    _ref_entry({'label': label, 'path': 'x'})
    if any(r['label'] == label for r in refs):
        raise ValueError(f'cells-freeze: a reference labelled {label!r} exists')
    rows = read_rows(run) if rows is None else rows
    g = _groups(rows, 'cells').get((it, cur['label']))
    s = summarize(g['cells'], ys['cells_decks']) if g else None
    if not s or not s['complete']:
        raise ValueError(f"cells-freeze: iteration {it} has no complete measurement against {cur['label']} "
                         f"(missing: {s['missing'] if s else 'all'}) - the chain link")
    src = model_path(run, it)
    if g['sha'] != sha12(src):
        raise ValueError(f'cells-freeze: {src} is not the network measured (sha {g["sha"]})')
    lad = {r['label']: r for r in ladder(run, cfg, ys, rows)}
    base = lad[cur['label']]
    if base['elo'] is None:
        raise ValueError(f"cells-freeze: the reference {cur['label']} is not chained")
    e = {'label': label, 'since': it, 'iter': it, 'sha': g['sha'],
         'from_ref': cur['label'], 'gain': s['overall']['elo'], 'gain_se': s['overall']['se'],
         'elo': round(base['elo'] + s['overall']['elo'], 1),
         'elo_se': round(math.sqrt(base['se'] ** 2 + s['overall']['se'] ** 2), 1), 'time': round(time.time())}
    return _install_ref(run, src, label, to, e)


def _install_ref(run, src, label, to, e):
    """A frozen reference: `src` copied (default <run>/yardsticks/cells_refs/<label>.pt, sha256 checked against
    e['sha']) and entry `e` (with its path) appended to the ladder file -> the entry."""
    dst = to or os.path.join(run, DIR, 'cells_refs', f'{label}.pt')
    if os.path.exists(dst):
        raise ValueError(f'cells-freeze: {dst} exists')
    os.makedirs(os.path.dirname(os.path.abspath(dst)), exist_ok=True)
    shutil.copy2(src, dst)
    if sha12(dst) != e['sha']:
        raise RuntimeError(f'cells-freeze: the copy {dst} differs from {src}')
    e = dict(e, path=os.path.abspath(dst))
    try:
        with open(ladder_path(run), encoding='utf-8') as f:
            d = json.load(f)
    except FileNotFoundError:
        d = {'refs': []}
    d['refs'].append(e)
    os.makedirs(os.path.dirname(ladder_path(run)), exist_ok=True)
    tmp = ladder_path(run) + '.tmp'
    with open(tmp, 'w', encoding='utf-8', newline='') as f:
        json.dump(d, f, indent=1)
    os.replace(tmp, ladder_path(run))
    return e


def sha_of_iter(run, it, rows=None):
    """Checkpoint `it`'s sha12: its file's, else its rows' (a pruned checkpoint), else None."""
    try:
        return sha12(model_path(run, it))
    except OSError:
        pass
    for r in (read_rows(run) if rows is None else rows):
        if r.get('kind') in ('cells', 'cells_gen') and r.get('iter') == int(it) and r.get('sha'):
            return r['sha']
    return None


def freeze_pool(run, it, label=None, to=None, cfg=None, ys=None, rows=None, fit=None):
    """The pool mode's promotion (docs/ENGINE.md 186.3; `cells-freeze` in the pool mode, or the fit's own with
    cells_auto_promote): checkpoint `it`, rated in all its cells by the joint fit, joins the pool as the newest
    reference - copied as freeze_ref does, its ladder entry's elo / elo_se the fit's anchored Elo (from_ref
    'pool-fit'). Its earlier measurement games are its links to the pool; its baselines are wanted from `since` on."""
    from . import cells_fit as CF
    cfg = cfg if cfg is not None else cfg_of(run)
    ys = ys if ys is not None else of_run(run, cfg)
    it = int(it)
    refs = cell_refs(run, cfg, ys)
    if refs and refs[-1]['since'] >= it:
        raise ValueError(f"cells-freeze: the newest reference {refs[-1]['label']} starts at {refs[-1]['since']} >= {it}")
    label = label or f'g{it}'
    _ref_entry({'label': label, 'path': 'x'})
    if any(r['label'] == label for r in refs):
        raise ValueError(f'cells-freeze: a reference labelled {label!r} exists')
    src = model_path(run, it)
    sha = sha12(src)
    for r in refs:
        try:
            if sha12(r['path']) == sha:
                raise ValueError(f"cells-freeze: iteration {it} is reference {r['label']} already")
        except OSError:
            pass
    rows = read_rows(run) if rows is None else rows
    fit = fit or CF.fit_run(run, cfg, ys, rows)
    g = CF.gen_of(fit, sha)
    if g is None or not g['complete']:
        raise ValueError(f"cells-freeze: iteration {it} is not rated in all its cells by the fit "
                         f"(rated: {sorted((g or {}).get('cells', {}))})")
    e = {'label': label, 'since': it, 'iter': it, 'sha': sha, 'from_ref': 'pool-fit',
         'elo': g['anchored']['elo'], 'elo_se': g['anchored']['se'], 'time': round(time.time())}
    return _install_ref(run, src, label, to, e)


def measure_cells_fit(run, it, cfg=None, ys=None, log=print):
    """The pool mode's record of measured checkpoint `it` (docs/ENGINE.md 186.6): the joint fit of every recorded
    cells game (bootstrap cells_fit_boot), its ratings, flags and the reference pool's promotion rule - done here
    when cells_auto_promote, else suggested -> the 'cells_fit' row."""
    from . import cells_fit as CF
    cfg = cfg if cfg is not None else cfg_of(run)
    ys = ys if ys is not None else of_run(run, cfg)
    t0 = time.time()
    rows = read_rows(run)
    fit = CF.fit_run(run, cfg, ys, rows)
    sha = sha_of_iter(run, it, rows)
    if sha is None:
        raise FileNotFoundError(model_path(run, it))
    refs = cell_refs(run, cfg, ys)
    pool = CF.active_refs(refs, int(it), ys['cells_pool_active'])
    prom = CF.promote_rule(fit, refs, int(it), sha, ys) if cells_pool(ys) else None
    a = CF.against_newest(fit, refs, int(it), sha, ys) if cells_pool(ys) else None
    below = {'vs': a[0], 'cells': a[2]} if a and a[2] else None     # (regressed against it: never promoted)
    if prom is not None:
        prom['auto'] = bool(ys['cells_auto_promote'])
        if prom['auto']:
            try:
                e = freeze_pool(run, it, cfg=cfg, ys=ys, rows=rows, fit=fit)
                prom.update(label=e['label'], elo=e['elo'], elo_se=e['elo_se'])
                log(f"cells pool: iteration {it} promoted ({prom['why']}) as {e['label']}, Elo {e['elo']} +- "
                    f"{e['elo_se']}")
            except (ValueError, OSError) as ex:
                prom['error'] = str(ex)[-400:]
                log(f'cells pool: iteration {it} not promoted: {ex}')
    return CF.fit_row(fit, int(it), sha, ys, rows, pool=pool, promote=prom, seconds=time.time() - t0, below=below)


def newest_fit(rows):
    """The newest 'cells_fit' row (the highest iteration's, then the latest), or None."""
    fs = [r for r in rows if r.get('kind') == FIT_KIND and r.get('error') is None]
    return max(fs, key=lambda r: (r['iter'], r.get('time') or 0)) if fs else None


# ------------------------------------------------------------------------------------------------ the companion
def companion(run, cfg):
    """The yardsticks' watcher as a companion process (mtgz.az.companions composes it with the publisher), or None
    when the run's setting is off. Raises on a bad setting."""
    ys = normalize(cfg['yardsticks']) if 'yardsticks' in cfg else None
    if ys is None:
        return None
    return {'name': 'yardsticks', 'cmd': [sys.executable, os.path.join(ROOT, 'tools', 'run_yardsticks.py'), 'watch',
                                          run, '--supervised'], 'log': LOG}
