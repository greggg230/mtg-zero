"""A metagame run on delverai.greggg230.com (docs/ENGINE.md 185): the parts of `tools/delverai_publish.py`'s progress
record and recording that a run of the metagame spec (docs/ENGINE.md 176: one network, every ordered pairing of a list
of 75s, each game's pairing drawn from its seed) needs and a Delver run does not. The publisher calls these when the
run's spec has `"metagame"`; nothing here writes inside the run's directory (the publisher's own files are in its
--state-dir).

The record's `run_kind` is 'metagame' and its `metagame` block holds:

* `decks` - the 75s in the spec's order: name, label ('Izzet Delver', 'Doomsday', 'Sneak and Show'), the list's
  provenance (data/decks/<name>.json: player, event, date, result, source) and the main and sideboard as
  [[copies, card], ...].
* `weights` - the spec's pairing weights (n x n, row = seat 0's deck), uniform when the spec gives none.
* `wheel` - the forced find (docs/ENGINE.md 180) of the run's self-play: the spell, the card, the schedule
  (`forced_find_schedule`), its p at every trained iteration from the schedule's start through its end (and the newest),
  and p now (the iteration being played). A pure function of the run's settings (mtgz.az.forced_find.p_at): the p the
  run's self-play spec carries at that iteration.
* `cells` - the cells yardstick (docs/ENGINE.md 177): mtgz.az.yardsticks.cells_report without local paths or error
  text - the reference ladder, every measured checkpoint's 9 cells against its reference (gain, se, ...), the per-deck
  and overall means and the anchored Elo, gen over gen, the suggested promotion - plus the cells still missing or
  failing; and `fit` when the run's yardsticks.jsonl has rows of the joint fit against a reference pool (kind
  `cells_fit`, another lane's, additive): per measured checkpoint the 9 cell ratings +- se (each with the reference it
  was rated against), per-deck, overall, anchored and its flags (`fit_row`: a pluggable reader of the fields it knows;
  the site prefers them to the cells' gains when present).
* `selfplay` - the run's own self-play results by ordered pairing, per iteration (`selfplay_tally`): read from the
  iteration's training shards (<run>/data/iter_NNNN/*.npz, the rows every self-play game wrote - local, the Mac's, the
  farm's; resigned games included, which the game log leaves out: docs/ENGINE.md 176.9). See `shard_games`.
* `combo` - combo statistics of every --combo-every'th generation (tools/combo_stats.py play: the checkpoint against
  itself with fixed decks per seat, no forced find, at --combo-sims; `combo_pass`): Doomsday's resolutions by where
  the Oracle was (voluntary / forced / in hand / not findable / missed), Oracle wins, the exit gate (the Oracle in its
  own piles in at least half of the resolutions, and Oracle wins against Delver); Sneak and Show's Show and Tell,
  Sneak Attack and fatties.

Recorded games of a metagame run (`covering_seed`): the recorder's driver seed is chosen so that the first n games
(n = --games, at most the number of cells) are n different pairings - with --games 9 every generation recorded plays
each of the 9 cells once. The listing record carries the game's `pairing` (mtgz.az.replays.summary) and each seat's
deck as its `seat_labels`.
"""
import glob
import json
import math
import os
import random
import re
import time
import traceback
import zipfile

import numpy as np

import mtgz.az  # noqa: F401  (this checkout's pinned zero-core on sys.path; the game package registered)

# The decks' names on the site (data/decks/<name>.json's own `name` otherwise).
LABELS = {'izzet_delver_8016541': 'Izzet Delver', 'doomsday_8027957': 'Doomsday',
          'sneak_show_8014220': 'Sneak and Show'}
PROVENANCE = ('player', 'event', 'date', 'result', 'source', 'platform', 'archetype')

# The encoding's opponent-pool bag (engine/src/az/encode.rs bag::OPP_POOL; a best-of-one with open decklists: the
# opponent's whole pool, 0.25 a copy) at belief.O_BAGS + OPP_POOL * D, and the deck target's segment of the auxiliary
# target (mtgz.az.belief: [belief D | deck D], engine MtgAz::aux_target: the frame opponent's cards the decider does not
# know). Checked against the engine in tests/test_delverai_meta.py.
OPP_POOL = 15



def _lay():
    """mtgz.az.layouts (docs/ENGINE.md 236): the game - layout v7 `mtg` or v8 `mtg_v8` - of the checkpoints this tool loads
    (`load_used`), every game it plays and encodes (`current`: layout v8 until a checkpoint is loaded)."""
    from mtgz.az import layouts
    return layouts

def spec_of(cfg):
    try:
        s = json.loads(cfg.get('spec') or '{}')
    except ValueError:
        return {}
    return s if isinstance(s, dict) else {}


def is_metagame(cfg):
    return 'metagame' in spec_of(cfg)


def label(name):
    if name in LABELS:
        return LABELS[name]
    try:
        from mtgz.az import decklists
        return decklists.load(name).get('name') or name
    except (OSError, ValueError):
        return name


def _counts(cards):
    """A list of card names (with repeats) -> [[copies, card], ...] in first-seen order."""
    out = {}
    for c in cards or []:
        out[c] = out.get(c, 0) + 1
    return [[n, c] for c, n in out.items()]


def decks(cfg):
    """The metagame's 75s for the record (module docs)."""
    from mtgz.az import decklists
    out = []
    for d in spec_of(cfg)['metagame']['decks']:
        e = {'name': d['name'], 'label': label(d['name']), 'main': _counts(d['main']),
             'sideboard': _counts(d.get('sideboard'))}
        try:
            src = decklists.load(d['name'])
        except (OSError, ValueError):
            src = {}
        for k in PROVENANCE:
            if src.get(k):
                e[k] = src[k]
        if src and 'sideboard' not in d and src.get('sideboard'):
            # the spec plays the mains only (best of one): the list's sideboard, for the 75 on the site
            e['sideboard'] = [[n, c] for n, c in src['sideboard']]
        out.append(e)
    return out


def weights(cfg):
    m = spec_of(cfg)['metagame']
    n = len(m['decks'])
    return m.get('weights') or [[1] * n for _ in range(n)]


# ---------------------------------------------------------------------------------------------------- the wheel
def wheel(cfg, last_iter):
    """The forced find of the run's self-play (module docs) or None without one."""
    from mtgz.az import forced_find as FF
    ff = spec_of(cfg).get(FF.KEY)
    if not ff:
        return None
    sched = FF.check_schedule(cfg.get('forced_find_schedule'))
    last = max(1, int(last_iter or 0) + 1)          # the iteration being played (self-play's first is 1)
    if sched:
        end = int(sched.get('from', 0)) + int(sched.get('iters', 0))
        its = sorted(set(range(1, min(last, end) + 1)) | {last})
        points = [[it, FF.p_at(sched, it)] for it in its]
        now = FF.p_at(sched, last)
    else:
        points, now = [[1, ff.get('p')], [last, ff.get('p')]], ff.get('p')
    return {'spell': ff.get('spell'), 'card': ff.get('card'), 'unless_in_hand': bool(ff.get('unless_in_hand')),
            'schedule': sched or None, 'points': points, 'now': {'iter': last, 'p': now}}


# ---------------------------------------------------------------------------------------------------- the cells
CELL_ROW = ('iter', 'ref', 'prev', 'sha', 'summary')


def cells(run, cfg):
    """The cells yardstick's report for the site (module docs) or None when the run measures no cells."""
    from mtgz.az import yardsticks as YS
    try:
        st = YS.status(run, cfg)
    except Exception as e:                 # noqa: BLE001 - said; never a reason to drop the progress post
        print(f'cells: not in the record this time ({e!r})', flush=True)
        return None
    c = (st or {}).get('cells')
    if not c:
        return None
    rep = c['report']
    ys = YS.of_run(run, cfg) or {}
    fit = cells_fit(run, ys.get('cells_decks') or [])
    keep = lambda r: {k: r[k] for k in ('iter', 'ref', 'item', 'pair', 'base', 'attempts') if k in r}  # noqa: E731
    return {'refs': [{k: r[k] for k in ('label', 'since', 'sha', 'elo', 'se', 'how') if k in r and r[k] is not None}
                     for r in rep['refs'] if 'label' in r],
            'cells': [{k: r[k] for k in CELL_ROW if k in r} for r in rep['cells']],
            'gen': [{k: r[k] for k in CELL_ROW if k in r} for r in rep['gen']],
            'promote': rep.get('promote'),
            'missing': [dict(keep(r), kind=r['kind']) for r in c['missing']],
            'failed': [dict(keep(r), kind=r['kind'], attempts=r.get('attempts')) for r in c['failed']],
            'settings': {k: ys[k] for k in ('cells_games', 'cells_gen_games', 'cells_every',
                                                        'cells_gen_every', 'cells_sims', 'cells_target_se',
                                                        'cells_promote', 'cells_base_elo', 'cells_decks')
                         if ys.get(k) is not None},
            **({'fit': fit} if fit else {})}


# The joint fit of the cells against a reference pool (another lane's `cells_fit` rows in <run>/yardsticks.jsonl, an
# additive kind; the cells / cells_gen / cells_base rows stay): read when present, else the site shows the cells
# above. A pluggable reader: the fields it knows are taken, anything else of the row is left out (so no path or error
# text reaches the site), a row that does not read is skipped and said once.
FIT_KIND = 'cells_fit'
FIT_FLAGS = ('transitivity', 'transitive', 'no_improvement', 'improved', 'flags')
PATHISH = re.compile(r'[/\\]')          # (a flag's text with a slash may be a path: left out)


def _es(v):
    """{elo, se} of a rating (its `elo` or `rating`, its `se` or `elo_se`) or None."""
    if not isinstance(v, dict):
        return None
    e = v.get('elo', v.get('rating'))
    se = v.get('se', v.get('elo_se'))
    if not isinstance(e, (int, float)) or not isinstance(se, (int, float)) or se < 0:
        return None
    out = {'elo': round(float(e), 1), 'se': round(float(se), 1)}
    for k in ('games', 'score'):
        if isinstance(v.get(k), (int, float)):
            out[k] = v[k]
    if isinstance(v.get('ref'), str) and len(v['ref']) <= 64:
        out['ref'] = v['ref']
    return out


def _flag(v):
    if isinstance(v, bool) or (isinstance(v, (int, float)) and math.isfinite(v)):
        return v
    if isinstance(v, str) and len(v) <= 120 and not PATHISH.search(v):
        return v
    if isinstance(v, list):
        return [x for x in (_flag(y) for y in v[:20]) if x is not None]
    if isinstance(v, dict):
        return {str(k)[:40]: x for k, y in list(v.items())[:20] if (x := _flag(y)) is not None}
    return None


def fit_row(r, decks):
    """One `cells_fit` row -> {iter, ref, scale: 'rating', summary {cells {'X|Y': {elo, se, ref?}}, decks, overall,
    anchored, missing, complete}, flags} or None when it does not read."""
    if not isinstance(r, dict) or r.get('kind') != FIT_KIND or not isinstance(r.get('iter'), int) or r.get('error'):
        return None
    raw = r.get('cells') or r.get('ratings') or {}
    if not isinstance(raw, dict):
        return None
    cells = {k: x for k, v in raw.items() if isinstance(k, str) and '|' in k and (x := _es(v))}
    if not cells:
        return None
    ids = [f'{x}|{y}' for x in decks for y in decks]
    dk = r.get('decks') if isinstance(r.get('decks'), dict) else {}
    out = {'iter': r['iter'], 'ref': str(r.get('ref') or r.get('pool') or 'pool')[:64], 'scale': 'rating',
           'summary': {'cells': cells, 'decks': {k: x for k, v in dk.items() if (x := _es(v))},
                       'overall': _es(r.get('overall')), 'anchored': _es(r.get('anchored')),
                       'missing': [c for c in ids if c not in cells], 'complete': all(c in cells for c in ids)}}
    flags = {k: f for k in FIT_FLAGS if k in r and (f := _flag(r[k])) is not None}
    if flags:
        out['flags'] = flags
    return out


def cells_fit(run, decks):
    """The run's `cells_fit` rows for the site, newest last (the last row of an iteration wins)."""
    from mtgz.az import yardsticks as YS
    by, bad = {}, 0
    try:
        with open(YS.rows_path(run), encoding='utf-8') as f:
            for line in f:
                if FIT_KIND not in line:
                    continue
                try:
                    r = fit_row(json.loads(line), decks)
                except ValueError:
                    r = None
                if r is None:
                    bad += 1
                    continue
                by[r['iter']] = r
    except OSError:
        return []
    if bad:
        print(f'cells_fit: {bad} row(s) not read (an unknown shape): left out', flush=True)
    return [by[k] for k in sorted(by)]


# ------------------------------------------------------------------------------------- self-play by pairing
def _member(path, name):
    """One array of an .npz: memory-mapped when it is stored (the shard writer's big local shards), else loaded."""
    with zipfile.ZipFile(path) as z:
        info = z.getinfo(name + '.npy')
        stored = info.compress_type == zipfile.ZIP_STORED
    if not stored:
        with np.load(path) as f:
            return f[name]
    with open(path, 'rb') as f:
        f.seek(info.header_offset)
        h = f.read(30)
        f.seek(info.header_offset + 30 + int.from_bytes(h[26:28], 'little') + int.from_bytes(h[28:30], 'little'))
        ver = np.lib.format.read_magic(f)
        read = np.lib.format.read_array_header_1_0 if ver == (1, 0) else np.lib.format.read_array_header_2_0
        shape, fortran, dtype = read(f)
        off = f.tell()
    return np.memmap(path, dtype=dtype, mode='r', shape=shape, offset=off, order='F' if fortran else 'C')


class DeckReader:
    """A row's opponent deck from its opponent-pool bag: the one deck of the metagame whose main holds every card of
    the bag (the bag leaves nothing of the opponent's pool out in a best-of-one with open decklists), else None."""

    def __init__(self, deck_list):
        from mtgz.az import belief
        self.names = belief.names()
        self.lo = belief.O_BAGS + OPP_POOL * belief.D
        self.hi = self.lo + belief.D
        self.sets = [set(c for _, c in d['main']) for d in deck_list]
        self.cache = {}

    def deck(self, cols):
        key = cols.tobytes()
        got = self.cache.get(key)
        if got is None and key not in self.cache:
            nm = {self.names[int(c) - self.lo] for c in cols}
            hit = [i for i, s in enumerate(self.sets) if nm and nm <= s]
            got = self.cache[key] = hit[0] if len(hit) == 1 else None
        return got


def _rows(path, reader):
    """A shard's rows -> (player, seat 0's result, opponent deck index or -1, the deck target's total)."""
    p = np.asarray(_member(path, 'player')).astype(np.int64)
    z = np.asarray(_member(path, 'z')).astype(np.float64)
    xp = np.asarray(_member(path, 'x_ptr')).astype(np.int64)
    xi = _member(path, 'x_idx')
    aux = _member(path, 'aux')
    from mtgz.az import belief
    dk = np.asarray(aux[:, belief.D:2 * belief.D], dtype=np.float32).sum(1)
    r0 = np.where(p == 0, z[:, 0], z[:, 1])
    opp = np.full(len(p), -1, np.int64)
    for i in range(len(p)):
        seg = np.asarray(xi[xp[i]:xp[i + 1]]).astype(np.int64)
        d = reader.deck(seg[(seg >= reader.lo) & (seg < reader.hi)])
        opp[i] = -1 if d is None else d
    return p, r0, opp, dk


def segment(p, r0, opp, dk, full=60.0):
    """Rows of one shard writer, in order -> games [(seat 0's deck or None, seat 1's deck or None, seat 0's result)].
    The writer keeps a game's rows together (zero-core ShardWriter: whole games); a new game starts where seat 0's result
    changes, where a player's opponent deck changes, or where a player's deck target is the whole opponent main again
    after it was less (the first decision of a game: nothing of the opponent's is known yet). Every row of a game has
    the game's result (`z` is the game's outcome from the row's player's side) and each player's rows the same opponent
    deck. Two games in a row with the same pairing and result, where neither player ever learnt an opponent's card (a
    resignation before any card was seen), read as one game; a game whose opponent card goes back into the hidden
    zones reads as two (counted against the iteration's games: `read` vs `games`)."""
    out, cur, last = [], None, {}
    for i in range(len(p)):
        pl, o = int(p[i]), int(opp[i])
        o = None if o < 0 else o
        isfull = abs(float(dk[i]) - full) < 1e-3
        new = (cur is None or r0[i] != cur[2] or (o is not None and cur[1].get(pl, o) != o)
               or (isfull and last.get(pl, full) < full - 1e-3))
        if new:
            cur = [None, {}, float(r0[i])]
            out.append(cur)
            last = {}
        if o is not None:
            cur[1][pl] = o
        last[pl] = float(dk[i])
    # player 0's opponent is seat 1's deck, player 1's is seat 0's
    return [(c[1].get(1), c[1].get(0), c[2]) for c in out]


def _writer(f):
    """A shard's writer (its file name less the trailing _NNN: 'r00', 'm01-a31b') and index."""
    m = re.fullmatch(r'(.+)_(\d+)\.npz', os.path.basename(f))
    return (m.group(1), int(m.group(2))) if m else (os.path.basename(f), 0)


def shard_games(d, reader):
    """The games of one iteration's shard directory: each writer's shards concatenated in order, then `segment`."""
    by = {}
    for f in glob.glob(os.path.join(d, '*.npz')):
        w, k = _writer(f)
        by.setdefault(w, []).append((k, f))
    games = []
    for w in sorted(by):
        parts = [_rows(f, reader) for _, f in sorted(by[w])]
        games += segment(*(np.concatenate([x[j] for x in parts]) for j in range(4)))
    return games


def iteration_tally(run, it, reader, n_decks):
    """One iteration's self-play by ordered pairing -> {iter, games (the iteration's: local + remote, DONE.json),
    read (games segmented from the shards), unread (a game whose pairing could not be read: one player's rows only),
    cells {'i|j': [games, seat 0's points]}, resigned (the run's own count, DONE.json)} or None when the iteration's
    shards are not complete (no DONE.json) or gone."""
    d = os.path.join(run, 'data', f'iter_{it:04d}')
    done = os.path.join(d, 'DONE.json')
    if not os.path.exists(done):
        return None
    with open(done, encoding='utf-8') as f:
        sp = (json.load(f) or {}).get('selfplay') or {}
    games = shard_games(d, reader)
    cells, unread = {}, 0
    for a, b, r in games:
        if a is None or b is None:
            unread += 1
            continue
        c = cells.setdefault(f'{a}|{b}', [0, 0.0])
        c[0] += 1
        c[1] += r
    rem = sp.get('remote') or {}
    expected = (sp.get('games') or 0) + (rem.get('accepted', rem.get('games')) or 0)
    return {'iter': it, 'games': expected, 'read': len(games), 'unread': unread,
            'cells': {k: [v[0], round(v[1], 1)] for k, v in sorted(cells.items())},
            'resigned': (sp.get('resign') or {}).get('resigned')}


SELFPLAY_FILE = 'selfplay_cells.json'


def selfplay_pass(run, cfg, state_dir, log=print):
    """Tally every iteration whose shards are complete and not tallied yet (state_dir/selfplay_cells.json: {iter:
    tally}); an iteration whose shards are already gone (the run keeps `keep_data` iterations) is noted `gone`. ->
    the iterations tallied now."""
    path = os.path.join(state_dir, SELFPLAY_FILE)
    try:
        with open(path, encoding='utf-8') as f:
            have = json.load(f)
    except (OSError, ValueError):
        have = {}
    data = os.path.join(run, 'data')
    its = sorted(int(m.group(1)) for x in (os.listdir(data) if os.path.isdir(data) else [])
                 if (m := re.fullmatch(r'iter_(\d+)', x)))
    todo = [it for it in its if str(it) not in have]
    if not todo:
        return []
    dl = decks(cfg)
    reader = DeckReader(dl)
    done = []
    for it in todo:
        t0 = time.time()
        try:
            t = iteration_tally(run, it, reader, len(dl))
        except FileNotFoundError:
            continue                       # (deleted under us: the next pass marks it gone)
        if t is None:
            continue
        have[str(it)] = t
        done.append(it)
        log(f'self-play by pairing, iteration {it}: {t["read"]} games read of {t["games"]} '
            f'({t["unread"]} unread) in {time.time() - t0:.1f} s')
    os.makedirs(state_dir, exist_ok=True)
    with open(path + '.tmp', 'w', encoding='utf-8') as f:
        json.dump(have, f, separators=(',', ':'))
    os.replace(path + '.tmp', path)
    return done


def selfplay_record(state_dir):
    """The tallies for the record, by iteration."""
    try:
        with open(os.path.join(state_dir, SELFPLAY_FILE), encoding='utf-8') as f:
            have = json.load(f)
    except (OSError, ValueError):
        return []
    return [have[k] for k in sorted(have, key=int)]


# ---------------------------------------------------------------------------------------------- recorded games
def game_seed(driver_seed, k):
    """The engine seed of game k of a zero.replay recording with driver seed `driver_seed` (zero-core play.PyDriver /
    replay.RecordingDriver: game k's slot takes random.Random(driver rng.randrange(2^30)) and seeds, in order, its numpy
    generator, its search generator and its Game - `getrandbits(63)`). Checked against the driver in the tests."""
    rng = random.Random(driver_seed)
    for _ in range(k + 1):
        r = random.Random(rng.randrange(1 << 30))
    r.randrange(1 << 30)
    r.randrange(1 << 30)
    return r.getrandbits(63)


def pairings(spec, driver_seed, n):
    """The pairings (seat 0's deck, seat 1's) of the first n games of a recording with this driver seed."""
    from zero.games import engine
    E = engine()
    rng = random.Random(driver_seed)
    out = []
    for _ in range(n):
        r = random.Random(rng.randrange(1 << 30))
        r.randrange(1 << 30)
        r.randrange(1 << 30)
        g = E.Game(_lay().current(), spec, 2, r.getrandbits(63))
        out.append(tuple(json.loads(g.export_state())['match']['pairing']['decks']))
    return out


def covering_seed(spec, n, cells, start=None, tries=200_000):
    """A driver seed whose first min(n, cells) games are all different pairings (module docs); the best seen after
    `tries` (said by the caller) otherwise. -> (seed, its first n pairings)."""
    seed = random.randrange(1 << 30) if start is None else int(start)
    want = min(n, cells)
    best = None
    for k in range(tries):
        ps = pairings(spec, seed + k, n)
        distinct = len(set(ps[:want]))
        if best is None or distinct > best[0]:
            best = (distinct, seed + k, ps)
        if distinct == want:
            break
    return best[1], best[2]


def seat_labels(rep, deck_list):
    """A recorded game's seats by deck (its first state's pairing) -> [label, label] or None."""
    st = (rep.get('states') or [{}])[0]
    p = ((st or {}).get('match') or {}).get('pairing')
    if not p:
        return None
    return [deck_list[i]['label'] if i < len(deck_list) else p['names'][k] for k, i in enumerate(p['decks'])]


# ------------------------------------------------------------------------------------------- combo statistics
COMBO_DIR = 'combo'
DOOMSDAY = 'doomsday_8027957'
DELVER = 'izzet_delver_8016541'
SNEAK = 'sneak_show_8014220'
FF_CLASSES = ('voluntary', 'forced', 'in_hand', 'not_findable', 'missed')
GATE_SHARE = 0.5            # the exit gate (docs/ENGINE.md 180.7): the Oracle in its own piles in >= half the resolutions


def combo_pairings(names):
    """The combo measurement's pairings (seat 0, seat 1) among the run's decks: Doomsday against every deck (the mirror
    reads both seats), Sneak and Show against Delver and its mirror; only decks the run plays."""
    out = []
    if DOOMSDAY in names:
        out += [(DOOMSDAY, x) for x in names]
    if SNEAK in names:
        out += [(SNEAK, x) for x in names if x != DOOMSDAY]      # (Doomsday vs Sneak and Show above reads it too)
    return out


def combo_path(state_dir, it):
    return os.path.join(state_dir, COMBO_DIR, f'combo_{it:04d}.json')


def _short(name):
    return {DOOMSDAY: 'dd', DELVER: 'delver', SNEAK: 'sns'}.get(name, re.sub(r'[^a-z0-9]+', '', name.lower())[:10])


def combo_entry(it, sums, sims, games, seconds):
    """combo_stats `summarize` dicts (one per pairing) -> the record's compact entry for generation `it`."""
    dd, sns = {}, {}
    for s in sums:
        d0, d1 = s['decks']
        for seat, me, opp in ((0, d0, d1), (1, d1, d0)):
            score = s['p0_score'] if seat == 0 else 1.0 - s['p0_score']
            x = ((s.get('doomsday') or {}).get(f'p{seat}'))
            if me == DOOMSDAY and x is not None:
                e = dd.setdefault(opp, {'games': 0, 'points': 0.0, 'casts': 0, 'resolved': 0, 'oracle_wins': 0,
                                        'oracle': {k: 0 for k in FF_CLASSES}})
                e['games'] += s['games']
                e['points'] += score * s['games']
                e['casts'] += x['casts']
                e['resolved'] += x['resolved']
                e['oracle_wins'] += x['oracle_wins']
                for k in FF_CLASSES:
                    e['oracle'][k] += int((x.get('oracle') or {}).get(k) or 0)
            y = ((s.get('sneak_show') or {}).get(f'p{seat}'))
            if me == SNEAK and y is not None:
                e = sns.setdefault(opp, {'games': 0, 'points': 0.0, 'show_cast': 0, 'show_resolved': 0,
                                         'sneak_games': 0, 'fatty_games': 0, 'omniscience_games': 0,
                                         'won_by_damage': 0})
                e['games'] += s['games']
                e['points'] += score * s['games']
                e['show_cast'] += sum((y.get('show') or {}).values())
                e['show_resolved'] += y['games_show_resolved']
                e['sneak_games'] += y['games_sneak_activated']
                e['fatty_games'] += y['games_fatty']
                e['omniscience_games'] += y['games_omniscience']
                e['won_by_damage'] += y['won_by_damage']
    for e in list(dd.values()) + list(sns.values()):
        e['points'] = round(e['points'], 1)
    out = {'iter': it, 'sims': sims, 'games': games, 'seconds': round(seconds, 1), 'time': int(time.time())}
    if dd:
        tot = {k: sum(e['oracle'][k] for e in dd.values()) for k in FF_CLASSES}
        res = sum(e['resolved'] for e in dd.values())
        share = tot['voluntary'] / res if res else None
        wins_delver = (dd.get(DELVER) or {}).get('oracle_wins', 0)
        out['doomsday'] = {'vs': dd, 'oracle': tot, 'resolved': res, 'own_piles': share,
                           'own_piles_se': math.sqrt(share * (1 - share) / res) if res else None,
                           'gate': {'share': GATE_SHARE, 'piles_met': share is not None and share >= GATE_SHARE,
                                    'oracle_wins_vs_delver': wins_delver,
                                    'met': share is not None and share >= GATE_SHARE and wins_delver > 0}}
    if sns:
        out['sneak_show'] = {'vs': sns}
    return out


def combo_pass(run, cfg, state_dir, it, ckpt, games, sims, threads, device='cpu', log=print, progress=None):
    """Generation `it`'s combo statistics (module docs): tools/combo_stats.py play for each of combo_pairings (games
    played and scanned under state_dir/combo/gen_NNNN; a batch on disk is not played again), then summarized into
    state_dir/combo/combo_NNNN.json. -> the entry."""
    import combo_stats as CS
    names = [d['name'] for d in spec_of(cfg)['metagame']['decks']]
    out = os.path.join(state_dir, COMBO_DIR, f'gen_{it:04d}')
    t0 = time.time()
    sums = []
    for k, (a, b) in enumerate(combo_pairings(names)):
        lab = f'{_short(a)}-{_short(b)}'
        if progress is not None:
            progress.update(pairing=k + 1, of=len(combo_pairings(names)))
        CS.play(ckpt, [a, b], [sims, sims], games, out, label=lab, seed=1000 + it, batch=games, threads=threads,
                device=device, log=log,
                # (the run keeps its games, docs/gamedb.md: these too, kind 'combo' under the run's records)
                records=os.path.join(run, 'records') if cfg.get('game_records') else None, records_it=it)
        files = sorted(glob.glob(os.path.join(out, f'{lab}.b*.games.json.gz')))
        res = CS.scan(files, log=log)
        bad = sum(len(e) for _, _, e in res)
        if bad:
            raise RuntimeError(f'combo statistics of generation {it}: {bad} game(s) of {lab} did not replay')
        rows, errors = CS.read_stats([f[:-len('.games.json.gz')] + '.stats.jsonl' for f in files])
        if errors or not rows:
            raise RuntimeError(f'combo statistics of generation {it}: {lab}: {len(rows)} games, {len(errors)} errors')
        sums.append(CS.summarize(rows))
    e = combo_entry(it, sums, sims, games, time.time() - t0)
    path = combo_path(state_dir, it)
    with open(path + '.tmp', 'w', encoding='utf-8') as f:
        json.dump(e, f, separators=(',', ':'))
    os.replace(path + '.tmp', path)
    return e


def combo_record(state_dir):
    out = []
    for f in sorted(glob.glob(os.path.join(state_dir, COMBO_DIR, 'combo_*.json'))):
        try:
            with open(f, encoding='utf-8') as fh:
                out.append(json.load(fh))
        except (OSError, ValueError):
            continue
    return sorted(out, key=lambda e: e['iter'])


def combo_due(state, state_dir, run, every, min_iter=None, settle_s=30):
    """The newest checkpoint with an iteration divisible by `every` (and >= min_iter) that has no combo statistics yet
    and has not failed -> (iteration, path) or None."""
    if not every:
        return None
    d = os.path.join(run, 'models')
    best = None
    for f in os.listdir(d) if os.path.isdir(d) else []:
        m = re.fullmatch(r'iter_(\d+)\.pt', f)
        if m and int(m.group(1)) % every == 0 and (best is None or int(m.group(1)) > best[0]):
            best = (int(m.group(1)), os.path.join(d, f))
    if best is None or (min_iter is not None and best[0] < min_iter):
        return None
    if os.path.exists(combo_path(state_dir, best[0])) or best[0] in (state.get('combo_failed') or []):
        return None
    if time.time() - os.path.getmtime(best[1]) < settle_s:
        return None
    return best


# ---------------------------------------------------------------------------------------------- the record
def record(run, cfg, rows, state_dir):
    """The record's `metagame` block (module docs)."""
    last = max([r['iter'] for r in rows if isinstance(r.get('iter'), int)], default=0)
    out = {'decks': decks(cfg), 'weights': weights(cfg), 'wheel': wheel(cfg, last),
           'selfplay': selfplay_record(state_dir), 'combo': combo_record(state_dir)}
    c = cells(run, cfg)
    if c is not None:
        out['cells'] = c
    return out


def safe(f):
    """Run f(); a failure is said with its traceback and None returned (a part of the record is never a reason to
    drop the post)."""
    try:
        return f()
    except Exception:                      # noqa: BLE001
        print('metagame record part failed:\n' + traceback.format_exc(), flush=True)
        return None
