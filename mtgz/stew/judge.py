"""The stew's judging (docs/ENGINE.md 239.3): best-of-three matches on common deals, the deck following the net.

**Matches where the deck follows the net.** zero.h2h seats side A in seat gid % 2, so a match of A against B on
pair_spec(X, Y) moves A onto Y every other game. A stew comparison is a property of one cell (X|Y: X's pilot against
Y's), so its matches are played here with the seats FIXED (zero.games.Driver takes one seat list per game): the
measured pilot of X sits in X's seat of the spec for every match - pair_spec(X, Y) with it in seat 0 and
pair_spec(Y, X) with it in seat 1 (the two **orientations**; a mirror's two orientations are its two seats). Each
orientation is played with the spec's `chooser` 0 and 1 on one driver seed: match g of the two chooser halves has the
same game seed (the cells yardstick's unit, docs/ENGINE.md 177.1), so the **unit** is a deal played once with X's
side choosing play or draw and once with Y's side choosing - both play/draw orders. Best of three (`"format": "bo3"`,
`game_episodes`, `start_game` 1, the specialists' spec): the chooser is game 1's, later games' the previous game's
loser; one result per match (the contest's row, zero-core docs/az.md 42). `judge.matches` per comparison = 4 jobs (2
orientations x 2 choosers) of matches / 4, i.e. matches / 2 units.

**Arms of a cell X|Y** (specialist S = {X, Y}, the round's version g + 1), all on the same deals (each orientation's
seed is the pair's: `cell_seed`):
* B: S against S - X's side's score is the matchup table's P(X beats Y) under the specialist (played once per pair,
  read from either side for X|Y and Y|X; a mirror's is 0.5 by symmetry and not played);
* A: deck stew X (version g + 1) piloting X against S piloting Y - the **distillation gap** A - B;
* M: the main stew (version g + 1) piloting X against S piloting Y - main - deck stew (M - A) and main - specialist
  (M - B).
A gap is `yardsticks.cell_stats` of the two arms' units: Elo = 400 log10 of the odds ratio of the two mean scores,
its se by the delta method over the paired units (the covariance of the common deals included), a bootstrap over
units beside it.

**Specialist gen over gen**: version g + 1 against version g on pair_spec(A, B), a standard paired head-to-head
(games 2k and 2k + 1 one deal with the seats swapped: both nets play both decks), `judge.gen_matches` matches; the
pair scores' mean, se, Elo.

**The matchup table**: arm B per cell; each candidate's expected field win rate = sum over the field's decks Y of
share(Y) x P(C beats Y), se in quadrature (the cells' deals are disjoint). The same with the deck stews and the main
stew as the candidate's pilot (arms A and M) beside it.

The match process: `python -m mtgz.stew.judge play JOB.json OUT.json` (a job's matches, its result written
atomically; the orchestrator runs `judge.procs` of them at once and reuses a result whose job hash matches).

**Game records** (`recipe.judge.records`, docs/ENGINE.md 250; off by default): a job that carries `records`
({records: the directory, kind, it, labels {a, b}, meta}) writes its matches' game records (seed + moves,
zero.gamerec: one file a job under `<records>/<kind>/iter_NNNN/`, each record's `players` the two nets per seat by
their stew names and versions) and names the file in its result (`records`). The driver's game log changes no game
(zero-core docs/az.md 59.3), so `records` is no part of the job's hash: a result played without it stays valid.
"""
import hashlib
import json
import math
import sys
import time

from . import config as C

JOB_V = 1
ARMS = ('B', 'A', 'M')
RECORDS_KIND = 'stew_judge'


# ------------------------------------------------------------------ the jobs
def seed_of(*parts):
    h = hashlib.sha256('|'.join(str(p) for p in parts).encode()).digest()
    return int.from_bytes(h[:4], 'little')


def orientations(x, y):
    """Cell X|Y's orientations: (seat 0's deck, seat 1's deck, X's seat)."""
    return [(x, y, 0), (y, x, 1)] if x != y else [(x, x, 0), (x, x, 1)]


def cell_seed(base, p, q, xs):
    """An orientation's driver seed: the pair's own (the same for every arm and every round: common deals); a mirror's
    two orientations their own seeds (the same deal with the seats swapped would be one unit, not two)."""
    return seed_of('stew-cell', base, p, q) if p != q else seed_of('stew-cell', base, p, q, xs)


def job_hash(job):
    j = {k: v for k, v in job.items() if k not in ('label', 'records')}
    return hashlib.sha256(json.dumps(j, sort_keys=True, separators=(',', ':')).encode()).hexdigest()[:16]


def _job(jid, kind, a, b, decks, chooser, layout, games, seed, spec, play, label, nets=None):
    """`nets`: the stew names and versions of sides a and b (('deck:X v2', 'spec:X|Y v2')): the players' labels of
    the job's game records (`with_records` moves them into the job's `records` block, or drops them)."""
    job = {'v': JOB_V, 'id': jid, 'kind': kind, 'a': a, 'b': b, 'decks': list(decks), 'chooser': chooser,
           'layout': layout, 'games': int(games), 'seed': int(seed), 'spec': spec, 'play': play, 'label': label}
    if nets is not None:
        job['nets'] = list(nets)
    return job


def with_records(jobs, records, g):
    """The jobs with their game records on (`recipe.judge.records`): each job's `records` block - where
    (`<records>/stew_judge/iter_<g>/`), the two nets' labels, the job's place in the judging (meta) - in place of its
    `nets`. `records` None: the jobs without (`nets` dropped: the jobs of before, key for key)."""
    out = []
    for j in jobs:
        j = dict(j)
        nets = j.pop('nets')
        if records is not None:
            j['records'] = {'records': records, 'kind': RECORDS_KIND, 'it': int(g), 'labels': {'a': nets[0], 'b': nets[1]},
                            'meta': {'tool': 'stew_judge', 'job': j['id'], 'arm': j['id'][0], 'decks': list(j['decks']),
                                     'chooser': j['chooser'], 'layout': j['layout'], 'round': int(g)}}
        out.append(j)
    return out


def arm_jobs(arm, pilot, spec_ckpt, x, y, alias, jcfg, spec_fn, play, labels):
    """Arm A or M of cell X|Y: `pilot` (a checkpoint) piloting X against the specialist `spec_ckpt` piloting Y, both
    orientations x both choosers -> [job]."""
    n = jcfg['matches'] // 4
    out = []
    for p, q, xs in orientations(x, y):
        for c in (0, 1):
            jid = f'{arm}.{alias[p]}-{alias[q]}.s{xs}.c{c}'
            out.append(_job(jid, 'cell', pilot, spec_ckpt, (p, q), c, f'a{xs}', n, cell_seed(jcfg['seed'], p, q, xs),
                            spec_fn(p, q, c), play, f"{labels[0]} as {x} vs {labels[1]} as {y} (chooser seat {c})",
                            nets=labels))
    return out


def pair_b_jobs(spec_ckpt, x, y, alias, jcfg, spec_fn, play, label):
    """Arm B of specialist {X, Y} (X != Y): the specialist against itself in both orientations x both choosers."""
    n = jcfg['matches'] // 4
    out = []
    for p, q, _ in orientations(x, y):
        for c in (0, 1):
            out.append(_job(f'B.{alias[p]}-{alias[q]}.c{c}', 'cell', spec_ckpt, spec_ckpt, (p, q), c, 'a0', n,
                            cell_seed(jcfg['seed'], p, q, None), spec_fn(p, q, c), play,
                            f'{label} vs itself, {p} vs {q} (chooser seat {c})', nets=(label, label)))
    return out


def gen_job(new, old, a, b, alias, jcfg, spec_fn, play, label, nets=None):
    return _job(f'G.{alias[a]}-{alias[b]}', 'gen', new, old, (a, b), None, 'paired', jcfg['gen_matches'],
                seed_of('stew-gen', jcfg['seed'], a, b), spec_fn(a, b, None), play, label, nets=nets)


def window_job(new, old, a, b, alias, scfg, seed, k, spec_fn, play, label, nets=None):
    """Part `k` of a specialist's stall test (docs/ENGINE.md 251): its new version against its version `window`
    trained turns before, paired (`scfg['matches']` matches); each part its own deals."""
    return _job(f'W.{alias[a]}-{alias[b]}.{k}', 'gen', new, old, (a, b), None, 'paired', scfg['matches'],
                seed_of('stew-window', seed, a, b, k), spec_fn(a, b, None), play, label, nets=nets)


def trained_versions(st, sn):
    """A specialist's versions where it trained (its first included; a round it sat out - `skipped` - is not a turn)."""
    return [v for v in sorted(st.net(sn)['versions'], key=lambda v: v['gen']) if not v.get('skipped')]


def window_base(st, sn, g, window):
    """The stall test of round g's turn: (new version, base version) - the base `window` trained turns before the
    new one - or None (the specialist sat round g out, or has not trained `window` turns yet)."""
    tv = trained_versions(st, sn)
    if not tv or tv[-1]['gen'] != g + 1 or len(tv) < window + 1:
        return None
    return tv[-1], tv[-1 - window]


def plan_jobs(phase, st, g, jcfg, spec_fn, play, records=None, stall=None, parts=None):
    """The judge jobs of `phase` ('judge_deck_gap': gen over gen, arms B and A, and the stall tests; 'judge': arm M;
    'judge_specialists', a generation without distillation, docs/ENGINE.md 257: the stall tests, and of
    `jcfg['between']` 'gen' = gen over gen, 'matchup' = arm B - nothing that needs a stew net) of round `g`. `records`: the directory of the jobs' game records (`recipe.judge.records`; None: none kept).
    `stall`: recipe.stall (None: no stall test); `parts`: {specialist: parts of its stall test} (default 1 each)."""
    jobs = _plan_jobs(phase, st, g, jcfg, spec_fn, play)
    if phase in ('judge_deck_gap', C.SKIP_PHASE) and stall is not None:
        al = {d['name']: d['alias'] for d in st.decks()}
        for a, b in C.pairings(st.decks(), st.candidates()):
            sn = C.spec_name(a, b)
            wb = window_base(st, sn, g, stall['window'])
            if wb is None:
                continue
            new, old = wb
            for k in range((parts or {}).get(sn, 1)):
                jobs.append(window_job(st.abs(new['path']), st.abs(old['path']), a, b, al, stall, jcfg['seed'], k,
                                       spec_fn, play, f"{sn} v{new['gen']} vs v{old['gen']} (stall test, part {k})",
                                       nets=(f"{sn} v{new['gen']}", f"{sn} v{old['gen']}")))
    return with_records(jobs, records, g)


def _plan_jobs(phase, st, g, jcfg, spec_fn, play):
    decks = st.decks()
    al = {d['name']: d['alias'] for d in decks}
    pairs = C.pairings(decks, st.candidates())
    out = []
    for a, b in pairs:
        sn = C.spec_name(a, b)
        new = st.ckpt(sn, g + 1)
        between = (jcfg.get('between') or []) if phase == C.SKIP_PHASE else []
        if phase == 'judge_deck_gap' or between:
            old = st.version(sn, g)
            sat_out = bool((st.version(sn, g + 1) or {}).get('skipped'))     # (the same net as v{g}: nothing to compare)
            want_gen = jcfg['gen_over_gen'] if phase == 'judge_deck_gap' else 'gen' in between
            if want_gen and old is not None and not sat_out:
                out.append(gen_job(new, st.abs(old['path']), a, b, al, jcfg, spec_fn, play,
                                   f'{sn} v{g + 1} vs v{g}', nets=(f'{sn} v{g + 1}', f'{sn} v{g}')))
            if a != b and (phase == 'judge_deck_gap' or 'matchup' in between):
                out += pair_b_jobs(new, a, b, al, jcfg, spec_fn, play, f'{sn} v{g + 1}')
        for x, y in ([(a, b), (b, a)] if a != b else [(a, a)]):
            if phase == 'judge_deck_gap':
                out += arm_jobs('A', st.ckpt(C.deck_net(x), g + 1), new, x, y, al, jcfg, spec_fn, play,
                                (f'deck:{x} v{g + 1}', f'{sn} v{g + 1}'))
            elif phase == 'judge' and jcfg['main_arms']:
                out += arm_jobs('M', st.ckpt('main', g + 1), new, x, y, al, jcfg, spec_fn, play,
                                (f'main v{g + 1}', f'{sn} v{g + 1}'))
    ids = [j['id'] for j in out]
    if len(set(ids)) != len(ids):
        raise RuntimeError(f'judge: duplicate job ids {sorted(i for i in ids if ids.count(i) > 1)}')
    return out


# ------------------------------------------------------------------ plan vs no plan (docs/ENGINE.md 260)
def _seat75(decks, s):
    from collections import Counter
    return dict(Counter(list(decks[f'p{s}']) + list(decks.get(f'p{s}_sideboard') or [])))


def _with_seat_tables(spec, tabs, start_game=None):
    """`spec` (JSON, a fixed pair spec) with each seat's choice table (`tabs` {seat: table or None}) on that seat
    (`decks.pN_table`; a table that is not the seat's 75 is refused) and `start_game` (2: post-board only, no chooser)."""
    sp = json.loads(spec)
    for s, t in tabs.items():
        if t is None:
            continue
        if {n: int(k) for n, k in t['list']} != _seat75(sp['decks'], s):
            raise ValueError(f"plan arm: table {t['version']} is not seat {s}'s 75")
        sp['decks'][f'p{s}_table'] = t
    if start_game is not None:
        sp['start_game'] = int(start_game)
        sp.pop('chooser', None)
    return json.dumps(sp, separators=(',', ':'))


def plan_arm_jobs(st, g, jcfg, spec_fn, play, tables):
    """`recipe.judge.plan_arm`: per specialist of round g that trained (its version g + 1 not `skipped`) with a choice
    table, its newest net on both seats, one seat its table (the plan), the other the all-main table of the same 75 (no
    plan); `jcfg['plan_matches']` matches an arm. `tables`: {specialist: {deck: {'table', 'main'}}} (the newest matchup
    version at its checkpoint, sbhealth.newest_tables). -> (jobs, arms: what sbhealth.plan_stats reads).

    A mirror: post-board only (`start_game` 2, no chooser: the engine refuses one there), the plan in seat 0 on one
    job and in seat 1 on the other, one deal seed (the seats swapped on common deals: paired). Two different 75s (the
    engine refuses start_game 2 there, ENGINE.md 147.4): an arm per deck X with a table - X's table against X all main
    on the same deals (game 1, the mains, is the same match in both), the other deck Y on its own table (all main when
    it has none), both orientations x both game-1 choosers."""
    from .sbhealth import all_main_table, main_only_table
    al = {d['name']: d['alias'] for d in st.decks()}
    n = int(jcfg['plan_matches'])
    jobs, arms = [], []
    for a, b in C.pairings(st.decks(), st.candidates()):
        sn = C.spec_name(a, b)
        v = st.version(sn, g + 1)
        tb = tables.get(sn) or {}
        if v is None or v.get('skipped'):
            continue
        net = st.abs(v['path'])
        lab = f"{sn} v{g + 1}"
        seed = seed_of('stew-plan', jcfg['seed'], a, b, g)
        if a == b:
            t = (tb.get(a) or {}).get('table')
            if t is None:
                continue
            none = all_main_table(t, tb[a]['main'])
            ids = []
            for s in (0, 1):
                tabs = {s: t, 1 - s: none}
                jid = f'P.{al[a]}-{al[a]}.o{s}'
                ids.append(jid)
                jobs.append(_job(jid, 'plan', net, net, (a, a), None, 'a0', n // 2, seed,
                                 _with_seat_tables(spec_fn(a, a, None), tabs, start_game=2), play,
                                 f'{lab}: its table in seat {s}, all main in seat {1 - s} (post-board)', nets=(lab, lab)))
            arms.append({'name': f'{sn} {a}', 'kind': 'mirror', 'deck': a, 'jobs': ids})
            continue
        for x, y in ((a, b), (b, a)):
            t = (tb.get(x) or {}).get('table')
            if t is None:
                continue
            none = all_main_table(t, tb[x]['main'])
            ty = (tb.get(y) or {}).get('table')
            if ty is None:                         # (Y without a table of its own: a fixed opponent, all main)
                ty = main_only_table(f"{t['version']}-{y}", tb[y]['names'], tb[y]['counts'], tb[y]['main'])
            pairs = []
            for p, q, xs in orientations(x, y):
                for c in (0, 1):
                    base = spec_fn(p, q, c)
                    js = []
                    for what, tx in (('plan', t), ('none', none)):
                        jid = f'P.{al[x]}-{al[y]}.s{xs}.c{c}.{what}'
                        js.append(jid)
                        jobs.append(_job(jid, 'plan', net, net, (p, q), c, 'a0', n // 4, cell_seed(seed, p, q, xs),
                                         _with_seat_tables(base, {xs: tx, 1 - xs: ty}), play,
                                         f"{lab}: {x} {'with its table' if what == 'plan' else 'all main'} vs {y} "
                                         f"(chooser seat {c})", nets=(lab, lab)))
                    pairs.append((js[0], js[1], xs))
            arms.append({'name': f'{sn} {x}', 'kind': 'pair', 'deck': x, 'pairs': pairs})
    return jobs, arms


# ------------------------------------------------------------------ the match process
def play_job(job):
    """A job's matches (the process's side; tests call it here) -> {'v', 'id', 'hash', 'rows', ...}."""
    from zero import games as G
    from zero.infer import NetEvaluator, drive
    from zero.net import check_belief_head, load
    if job.get('v') != JOB_V:
        raise SystemExit(f"stew judge: job version {job.get('v')!r}, this checkout reads {JOB_V}")
    p = job['play']
    ma, _ = load(job['a'], p['device'])
    from zero import package
    game = package.record_game(ma.config, f'the judge net {job["a"]}', default='mtg')
    mb, _ = load(job['b'], p['device'], game=game)
    seats = {}
    for key, m, path in (('a', ma, job['a']), ('b', mb, job['b'])):
        s = G.nn_seat(key, int(p['sims']), 1.0, chance_k=int(p['chance_k']), **p['seat_keys'])
        s.update(p['seat'])
        if s.get('belief_tilt') or s.get('belief_tilts'):
            check_belief_head(m, game, f'the judge seat\'s belief of {path}')
        seats[key] = s
    kw = {}
    if p.get('precision') and p['precision'] != 'fp32':
        kw['precision'] = p['precision']
    evs = {'a': NetEvaluator(ma, p['device'], **kw), 'b': NetEvaluator(mb, p['device'], **kw)}
    n = int(job['games'])
    sa, sb = seats['a'], seats['b']
    lay = job['layout']
    if lay == 'a0':
        sl = [[sa, sb] for _ in range(n)]
    elif lay == 'a1':
        sl = [[sb, sa] for _ in range(n)]
    elif lay == 'paired':
        if n % 2:
            raise ValueError(f'paired matches: an even number ({n})')
        sl = [[sa, sb] if g % 2 == 0 else [sb, sa] for g in range(n)]
    else:
        raise ValueError(f'judge job layout {lay!r}')
    dkw = {}
    if lay == 'paired':
        dkw['paired'] = True
    if p.get('sparse'):
        dkw['sparse'] = True
    t0 = time.time()
    drv = G.Driver(game, sl, spec=job['spec'], concurrency=min(n, 1024), seed=int(job['seed']),
                   threads=int(p['threads']), groups=int(p['groups']), max_steps=int(p['max_steps']), **dkw)
    rc = job.get('records')
    if rc:
        drv.set_game_log(True)                 # (game records, zero-core docs/az.md 59.3: the games unchanged)
    drive(drv, evs)
    rows = sorted(drv.take_results(), key=lambda r: r['gid'])
    if [r['gid'] for r in rows] != list(range(n)):
        raise RuntimeError(f"stew judge {job['id']}: {len(rows)} matches of {n}")
    try:
        from zero.h2h import _tag
        sel = _tag(sa)
    except Exception:                          # noqa: BLE001 - a name only
        sel = None
    out = {'v': JOB_V, 'id': job['id'], 'hash': job_hash(job), 'seconds': round(time.time() - t0, 1),
           'select': sel, 'unfinished': sum(1 for r in rows if not r.get('over'))}
    if rc:
        out['records'] = write_records(job, rows, sl, sa, sb, sel)
    out['rows'] = [{'gid': int(r['gid']), 'result': [float(v) for v in r['result']], 'over': bool(r.get('over')),
                    'games': len(r.get('episodes') or []) or 1, 'decisions': int(r.get('decisions') or 0)}
                   for r in rows]
    return out


def write_records(job, rows, sl, sa, sb, sel):
    """The job's matches as game records, one file (zero.gamerec.MatchSink): each record's `players` its two nets in
    the match's seat order (`sl`: the seats per match, each `sa` or `sb`), labelled by their stew names and versions.
    -> {file (its name), dir (kind/iter_NNNN under the records directory), games}."""
    import os

    from zero import gamerec as GR
    rc = job['records']
    sink = GR.MatchSink(rc['records'], rc['kind'], it=rc['it'], src='local', meta=rc['meta'], labels=rc['labels'])
    pa = GR.net_player(job['a'], rc['labels']['a'], sa, sel)
    pb = GR.net_player(job['b'], rc['labels']['b'], sb, sel)
    path = sink.add_rows(rows, lambda g: [pa if s is sa else pb for s in sl[g]], {'driver_seed': int(job['seed'])})
    return {'file': os.path.basename(path), 'dir': f"{rc['kind']}/iter_{int(rc['it']):04d}", 'games': len(rows)}


def valid_result(job, res):
    return (isinstance(res, dict) and res.get('hash') == job_hash(job) and res.get('id') == job['id']
            and [r['gid'] for r in res.get('rows') or []] == list(range(job['games'])))


# ------------------------------------------------------------------ the statistics
def _units(res, arm, p, q, xs, alias):
    """[(X's side chose, the other chose)] of one orientation of an arm, X in seat xs."""
    if arm == 'B':
        ida = f'B.{alias[p]}-{alias[q]}.c{xs}'
        idb = f'B.{alias[p]}-{alias[q]}.c{1 - xs}'
    else:
        ida = f'{arm}.{alias[p]}-{alias[q]}.s{xs}.c{xs}'
        idb = f'{arm}.{alias[p]}-{alias[q]}.s{xs}.c{1 - xs}'
    if ida not in res or idb not in res:
        return None
    ra, rb = res[ida]['rows'], res[idb]['rows']
    if len(ra) != len(rb):
        raise ValueError(f'judge: {ida} has {len(ra)} matches, {idb} {len(rb)}')
    return [(x['result'][xs], y['result'][xs]) for x, y in zip(ra, rb)]


def cell_units(res, arm, x, y, alias):
    """Arm `arm`'s units of cell X|Y (both orientations), or None when a job of it is missing."""
    out = []
    for p, q, xs in orientations(x, y):
        u = _units(res, arm, p, q, xs, alias)
        if u is None:
            return None
        out += u
    return out


def winrate(units):
    """X's side's mean score and its se over units."""
    from mtgz.az.yardsticks import _var
    if not units:
        return None
    u = [(a + b) / 2 for a, b in units]
    n = len(u)
    m = sum(u) / n
    return {'p': round(m, 4), 'se': round(math.sqrt(_var(u) / n), 4), 'units': n, 'matches': 2 * n}


def gap(a, b, cid, boot=1000):
    """Arm a's gain over arm b on the same units (`yardsticks.cell_stats`; b None: 0.5 exactly, a mirror)."""
    from mtgz.az.yardsticks import cell_stats
    s = cell_stats(a, b, boot=boot, seed=seed_of('stew-boot', cid))
    return {k: s[k] for k in ('elo', 'se', 'boot_se', 'boot_95', 'score', 'base', 'units', 'games', 'corr',
                              'clamped', 'se_unpaired') if k in s}


def gen_stats(res):
    """A paired gen-over-gen job's result -> A's (the new version's) score, se over pairs, Elo +- se."""
    from mtgz.az.yardsticks import ELO_K, _var
    rows = res['rows']
    ps = [(rows[k]['result'][0] + rows[k + 1]['result'][1]) / 2 for k in range(0, len(rows) - 1, 2)]
    n = len(ps)
    if not n:
        return None
    m = sum(ps) / n
    se = math.sqrt(_var(ps) / n)
    eps = 0.25 / n
    c = min(max(m, eps), 1 - eps)
    elo = ELO_K * math.log(c / (1 - c))
    return {'score': round(m, 4), 'se': round(se, 4), 'elo': round(elo, 1),
            'elo_se': round(ELO_K * se / (c * (1 - c)), 1), 'pairs': n, 'matches': 2 * n,
            'pentanomial': [sum(1 for x in ps if int(round(4 * x)) == k) for k in range(5)], 'clamped': c != m}


def window_stats(res, al, a, b):
    """A specialist's stall test over its parts present (W.<a>-<b>.0, .1, ... in order, up to the first missing) ->
    gen_stats of their matches together + 'z' (the gain's one-sided z: (score - 1/2) / se) + 'parts', or None."""
    rows, k = [], 0
    while f'W.{al[a]}-{al[b]}.{k}' in res:
        rows += res[f'W.{al[a]}-{al[b]}.{k}']['rows']
        k += 1
    if not k:
        return None
    s = gen_stats({'rows': rows})
    if s is None:
        return None
    s['parts'] = k
    if s['se'] > 0:
        s['z'] = round((s['score'] - 0.5) / s['se'], 3)
    else:                                   # (every pair the same score: no spread; JSON has no infinity)
        s['z'], s['se_zero'] = (99.0 if s['score'] > 0.5 else -99.0 if s['score'] < 0.5 else 0.0), True
    return s


def field_rate(rates, sh, cand, decks):
    """sum_Y share(Y) P(C beats Y), se in quadrature -> {'p', 'se'} or None when a cell is missing."""
    p, v = 0.0, 0.0
    for y in decks:
        r = rates.get(C.pair_key(cand, y))
        if r is None:
            return None
        p += sh[y] * r['p']
        v += (sh[y] * r['se']) ** 2
    return {'p': round(p, 4), 'se': round(math.sqrt(v), 4)}


def summarize(st, g, res, jobs_meta=None):
    """Every judged number of round g from the results present ({job id: result}) -> dict."""
    decks = st.decks()
    names = C.deck_names(decks)
    al = {d['name']: d['alias'] for d in decks}
    sh = C.shares(decks)
    pairs = C.pairings(decks, st.candidates())
    out = {'gen_over_gen': {}, 'cells': {}, 'matchup': {}, 'field': {}, 'matches': 0, 'seconds': 0.0,
           'games_per_match': None}
    nm, ng = 0, 0
    for r in res.values():
        out['matches'] += len(r['rows'])
        out['seconds'] += float(r.get('seconds') or 0)
        nm += len(r['rows'])
        ng += sum(x.get('games', 1) for x in r['rows'])
    out['seconds'] = round(out['seconds'], 1)
    if nm:
        out['games_per_match'] = round(ng / nm, 3)
    for a, b in pairs:
        gid = f'G.{al[a]}-{al[b]}'
        if gid in res:
            out['gen_over_gen'][C.spec_name(a, b)] = gen_stats(res[gid])
        w = window_stats(res, al, a, b)
        if w is not None:
            out.setdefault('window', {})[C.spec_name(a, b)] = w
    rates = {k: {} for k in ARMS}
    for a, b in pairs:
        for x, y in ([(a, b), (b, a)] if a != b else [(a, a)]):
            cid = C.pair_key(x, y)
            cu = {arm: cell_units(res, arm, x, y, al) for arm in ARMS}
            if x == y:
                cu['B'] = None
            cell = {'specialist': C.spec_name(a, b)}
            if x == y:
                cell['B'] = {'p': 0.5, 'se': 0.0, 'mirror': True}
            elif cu['B'] is not None:
                cell['B'] = winrate(cu['B'])
            for arm in ('A', 'M'):
                if cu[arm] is not None:
                    cell[arm] = winrate(cu[arm])
            base = None if x == y else cu['B']
            have_b = x == y or cu['B'] is not None
            if cu['A'] is not None and have_b:
                cell['gap_A_B'] = gap(cu['A'], base, f'{cid}|A-B')
            if cu['M'] is not None and cu['A'] is not None:
                cell['gap_M_A'] = gap(cu['M'], cu['A'], f'{cid}|M-A')
            if cu['M'] is not None and have_b:
                cell['gap_M_B'] = gap(cu['M'], base, f'{cid}|M-B')
            out['cells'][cid] = cell
            for arm in ARMS:
                if arm in cell:
                    rates[arm][cid] = cell[arm]
    for c in st.candidates():
        row = {}
        for arm, who in (('B', 'specialists'), ('A', 'deck stews'), ('M', 'main stew')):
            f = field_rate(rates[arm], sh, c, names)
            if f is not None:
                row[who] = f
        out['field'][c] = row
    out['matchup'] = {cid: cell.get('B') for cid, cell in out['cells'].items()}
    return out


# ------------------------------------------------------------------ the report
def _elo(x):
    if not x:
        return ''
    return f"{x['elo']:+.1f} +- {x['se']:.1f}"


def _p(x):
    if not x:
        return ''
    return f"{100 * x['p']:.1f}% +- {100 * x['se']:.1f}"


def _c(x):
    """A table cell: Markdown's column bar escaped (cell ids 'X|Y', net names 'spec:A|B')."""
    return str(x).replace('|', '\\|')


def report_md(rep):
    """The report's Markdown (reports/gen_NNN.md)."""
    g = rep['round']
    L = [f"# Stew {rep['name']}: generation {g} (versions {g} -> {g + 1})", '',
         f"Written {rep['written']}; phase {rep['phase']}" + (' (held here)' if rep.get('held') else '') + '.', '']
    sw = rep.get('stews')
    if sw:
        L += ['## The stews (docs/ENGINE.md 257)', '', f"This generation: {sw['this_generation']}.", '',
              f"Next distillation: {sw['next']}.", '', '| stew | versions |', '|---|---|']
        L += [f"| {_c(k)} | {' '.join('v%d' % x for x in gs)} |" for k, gs in sw['versions'].items()]
        L.append('')
    tr = rep.get('training') or {}
    if tr:
        L += ['## Training', '', '| specialist | iterations | matches | games / match | rows | rows / match | hours |',
              '|---|---|---|---|---|---|---|']
        for k, t in tr.items():
            L.append(f"| {_c(k)} | {t.get('from')} -> {t.get('to')} | {t.get('games')} | {t.get('games_per_match')} | "
                     f"{t.get('samples')} | {t.get('rows_per_match')} | {t.get('hours')} |")
        L.append('')
    for net, d in (rep.get('distill') or {}).items():
        L += [f'## Distillation: {net}', '', f"sources {d.get('sources')}, steps {d.get('steps')}", '']
        srcs = d.get('per_source') or []
        if srcs:
            keys = [k for k in ('weight', 'selected', 'used', 'policy_kl', 'top1', 'value_mae', 'value_mse')
                    if any(k in s for s in srcs)]
            L += ['| source | ' + ' | '.join(keys) + ' |', '|---|' + '---|' * len(keys)]
            for s in srcs:
                L.append(f"| {_c(s.get('name'))} | " + ' | '.join(str(s.get(k, '')) for k in keys) + ' |')
            L.append('')
    j = rep.get('judge') or {}
    if j.get('gen_over_gen'):
        L += ['## Specialists, generation over generation', '', '| specialist | score | Elo | matches |',
              '|---|---|---|---|']
        for k, s in j['gen_over_gen'].items():
            if s:
                L.append(f"| {_c(k)} | {100 * s['score']:.1f}% +- {100 * s['se']:.1f} | {s['elo']:+.1f} +- {s['elo_se']:.1f} | "
                         f"{s['matches']} |")
        L.append('')
    st_rows = rep.get('stall') or {}
    if st_rows:
        L += ['## Specialists: the stall rule (docs/ENGINE.md 251)', '',
              '| specialist | mode | test (new vs base) | score | Elo | z | matches | recommendation | next round |',
              '|---|---|---|---|---|---|---|---|---|']
        for k, s in st_rows.items():
            t = s.get('test') or {}
            L.append(f"| {_c(k)} | {s.get('mode')} | {t.get('versions', '')} | "
                     + (f"{100 * t['score']:.1f}% +- {100 * t['se']:.1f} | {t['elo']:+.1f} +- {t['elo_se']:.1f} | "
                        f"{t['z']:+.2f} | {t['matches']} | " if t.get('score') is not None else '|  |  |  | ')
                     + f"{s.get('recommendation', '')} | {'trains' if s.get('trains') else 'sits out'} |")
        L.append('')
    if j.get('cells'):
        L += ['## Cells (X|Y: the net piloting X against the specialist piloting Y)', '',
              '| cell | P(X wins), specialist | deck stew as X | gap deck - spec (Elo) | main as X | main - deck | '
              'main - spec |', '|---|---|---|---|---|---|---|']
        for cid, c in j['cells'].items():
            L.append(f"| {_c(cid)} | {_p(c.get('B'))} | {_p(c.get('A'))} | {_elo(c.get('gap_A_B'))} | {_p(c.get('M'))} | "
                     f"{_elo(c.get('gap_M_A'))} | {_elo(c.get('gap_M_B'))} |")
        L.append('')
    if j.get('field'):
        L += ['## Candidates: expected field win rate (sum of share x P(win))', '',
              '| candidate | specialists | deck stews | main stew |', '|---|---|---|---|']
        for c, row in j['field'].items():
            L.append(f"| {c} | {_p(row.get('specialists'))} | {_p(row.get('deck stews'))} | {_p(row.get('main stew'))} |")
        L.append('')
    if j:
        L += [f"Judging: {j.get('matches')} matches ({j.get('games_per_match')} games a match), "
              f"{(j.get('seconds') or 0) / 3600:.2f} process-hours.", '']
    if rep.get('sb_health'):
        from .sbhealth import md_lines
        L += md_lines(rep['sb_health'])
    for n in rep.get('notes') or []:
        L.append(f'* {n}')
    return '\n'.join(L).rstrip() + '\n'


# ------------------------------------------------------------------ the command line (the match process)
def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    if len(argv) != 3 or argv[0] != 'play':
        raise SystemExit('usage: python -m mtgz.stew.judge play JOB.json OUT.json (docs/ENGINE.md 239.3)')
    import mtgz.az  # noqa: F401  (the pinned core + the game package)
    from .state import jsave
    with open(argv[1], encoding='utf-8') as f:
        job = json.load(f)
    res = play_job(job)
    jsave(argv[2], res)
    print(f"{job['id']}: {len(res['rows'])} matches in {res['seconds']:.0f} s", flush=True)
    return 0


if __name__ == '__main__':
    sys.exit(main())
