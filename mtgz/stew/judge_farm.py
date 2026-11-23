"""The stew's judging on zero-farm (docs/ENGINE.md 249): `recipe.judge.backend` 'farm' plays a generation's judge jobs
(mtgz.stew.judge.plan_jobs) as farm match campaigns (mtgz.az.farm_campaign: traced eval jobs, every row's digest
checked, a spot replay, positive coverage) and writes the result files the local match process writes
(judge/gen_NNN/<job id>.json), so `judge.valid_result`, the reports and the resume read them unchanged.

**What a campaign can play, and how a judge job maps onto it.** A farm eval job plays game ids [0, N) of one spec
between two seats, seats[0] in seat gid % 2 (zero.farm_eval: `seats_of`), with one seed stream per game, or per pair
(`paired`: games 2k and 2k + 1 share every random stream, zero_search `game_seed`). A judge job is N matches on one
spec with a `layout` (judge.play_job): 'paired' (seats alternate, pairs share their streams) or fixed seats ('a0': the
job's net A in seat 0 of every match; 'a1': in seat 1), one stream per match. A **unit** (`units`) is one campaign and
the jobs it answers:

* a 'paired' job (gen over gen): a paired campaign of its N matches, seats [A, B], the job's seed - its game ids are
  the job's (`take` 'all');
* a fixed-seat job whose two nets are one network (arm B: the specialist against itself): an unpaired campaign of its N
  matches, seats [net 1, net 1], the job's seed - the seats are one seat, so the worker's alternation changes nothing
  (`take` 'all');
* fixed-seat jobs of two networks: a **paired** campaign of 2 N games with the job's seed. Its pair k has the stream
  of the job's match k (game_seed(seed, 2k, paired) = game_seed(seed, k, unpaired): both hash (seed, k)); its even
  game 2k seats [A, B] - the 'a0' job's match k -, its odd game 2k + 1 seats [B, A] - the 'a1' job's match k. Arm M's
  two orientations of a pairing share net, opponent, spec and seed (judge.cell_seed: common deals), so one campaign
  answers both jobs and no game is wasted; a unit with one of the two layouts (arm A: the two orientations pilot
  different deck stews; every mirror: its orientations have their own seeds) keeps its half and **discards** the other
  (`discard`: N games played and not used). The worker cannot fix seats (it always alternates), so this is the price
  of playing exactly the local job's matches; the plan and the log say how many.

So every match the farm plays for a job has the local job's spec, seats (settings, simulations, chance_k, the
specialists' move rule: checked dict for dict against `judge.job_seats`, the seats the local process plays), deal and
random streams. Refused by name, before any campaign is put (`FarmRefused`): `judge.precision` other than fp32, a seat
whose streams hang on the game id itself where a unit renumbers ids (`GID_SEAT_KEYS`), seats the farm job would play
differently, checkpoints of another game than the specialists', and h2h_farm's machine conditions (the farm's keys, a
clean checkout, a published build of the stew's commit, the build's own check-config, an export this Python cannot
make).

**What is not the same** (named, not approximated away): the arithmetic. The local judge evaluates with torch on this
machine's GPU; the farm's workers with ONNX Runtime on their CPUs (float32) or GPUs (the float16 / MatMul variants,
docs/ENGINE.md 215), in other batches. Network outputs differ in their last bits, a search can then choose another
move, and a match can end differently - as for every farm head-to-head (docs/ENGINE.md 217.1). Hence one backend per
generation (`claim_backend`): the arms of a generation are compared on common deals, and a control from another
backend would put the backends' difference into the gap. A result row's `decisions` (the local driver's count of
searched decisions) is None: a farm row does not carry it; nothing reads it. `games` (a match's games) is counted
from the traced record's episode boundaries.

**Stop, pause, resume.** The runner is one process (`python -m mtgz.stew.judge_farm run PLAN.json`, started and
polled by mtgz.stew.control.Ops.judge_farm); the stew's STOP or PAUSE kills it. Campaigns already put stay active on
the coordinator and go on being played (they are not paused there: mtgz.az.farm_campaign has no un-pause); their
state is under ~/.zero-farm/campaigns. The next run plans the same jobs, so the same names: finished jobs are skipped
by their result files, a campaign that exists is resumed by `farm_campaign.submit` (nothing put twice) and collected.
A campaign's deadline runs from its put whether or not a runner is alive: past it, the games the farm has not delivered
are played on this machine by the same worker, each piece in a slot of tools/slotd.sh beside a live run
(`farm_campaign.collect`, h2h_farm.topup_wrap).
"""
import hashlib
import json
import os
import shutil
import sys
import tempfile
import time
import types

from . import judge as J
from .state import jload, jsave, stamp

UNIT_V = 1
BACKEND_FILE = 'backend.json'
# seat settings whose random streams hang on the game id itself, not on the game's stream (zero_search driver:
# `view_pick`, `noresign_pick`): a unit that renumbers game ids (a fixed-seat job's match k is campaign game 2k or
# 2k + 1) would not replay them. Evaluation seats carry neither; a recipe that gave one is refused.
GID_SEAT_KEYS = ('view_samples', 'resign_t')


class FarmRefused(RuntimeError):
    """The farm cannot play these judge jobs exactly (the reason); nothing was put."""


class BackendMix(RuntimeError):
    """A generation's judge directory holds another backend's results."""


def _log(m):
    print(f"{time.strftime('%H:%M:%S')} {m}", flush=True)


# ------------------------------------------------------------------ one backend per generation
def results_in(jdir):
    """{job id: result} of the result files in a generation's judge directory."""
    out = {}
    if os.path.isdir(jdir):
        for f in sorted(os.listdir(jdir)):
            if f.endswith('.json') and not f.endswith('.job.json') and f != BACKEND_FILE:
                r = jload(os.path.join(jdir, f))
                if isinstance(r, dict) and 'rows' in r and 'id' in r:
                    out[r['id']] = r
    return out


def result_backend(r):
    """Where a result's matches were played (a result written before the farm backend names none: here)."""
    return r.get('backend') or 'local'


def backend_of(jdir):
    """The backend a generation's judging is on -> 'local' | 'farm' | None (not started): backend.json, else its
    results' own (every result before the farm backend was played here)."""
    rec = jload(os.path.join(jdir, BACKEND_FILE))
    if isinstance(rec, dict) and rec.get('backend') in ('local', 'farm'):
        return rec['backend']
    seen = sorted({result_backend(r) for r in results_in(jdir).values()})
    if len(seen) > 1:
        raise BackendMix(f'{jdir}: results of {seen} and no {BACKEND_FILE}: one backend per generation')
    return seen[0] if seen else None


def claim_backend(jdir, backend, tag=''):
    """This generation's judging is on `backend` from here on (written to backend.json at its first phase). BackendMix
    when the generation already judges on the other one, or when a result file in the directory was played on it: the
    arms of a generation are compared with each other, so they stay on one backend (as tools/ab.py's judge does)."""
    have = backend_of(jdir)
    if have is not None and have != backend:
        raise BackendMix(
            f"{jdir}: this generation's judging is on backend {have!r} and recipe.judge.backend is {backend!r} - one "
            f"backend per generation (a gap compares arms on common deals; another backend's control would put the "
            f"backends' difference into it). Finish the generation with judge.backend {have!r}, or move its judge "
            f"directory aside to judge all of it on {backend!r}")
    bad = sorted(i for i, r in results_in(jdir).items() if result_backend(r) != backend)
    if bad:
        raise BackendMix(f'{jdir}: results played on another backend than {backend!r}: {bad[:6]}'
                         f'{" ..." if len(bad) > 6 else ""}')
    if have is None or not os.path.exists(os.path.join(jdir, BACKEND_FILE)):
        jsave(os.path.join(jdir, BACKEND_FILE), {'backend': backend, 'since': stamp(), 'first': tag})
    return backend


def valid_result(job, res):
    """A farm result of `job`: judge.valid_result's checks and the backend's name."""
    return J.valid_result(job, res) and res.get('backend') == 'farm'


# ------------------------------------------------------------------ the units
def same_path(a, b):
    return os.path.normcase(os.path.abspath(a)) == os.path.normcase(os.path.abspath(b))


def units(jobs, same=same_path):
    """The campaigns of a phase's judge jobs (module docs) -> [unit]: {'id', 'a', 'b', 'one' (both nets one network),
    'spec', 'seed', 'play', 'paired', 'games' (the campaign's), 'matches' (a job's), 'jobs': [{'id', 'take': 'all' |
    'even' | 'odd'}], 'discard' (campaign games no job takes)}. `same(a, b)`: the two checkpoints are one network."""
    out, groups = [], {}
    for j in jobs:
        lay, n = j['layout'], int(j['games'])
        base = {'v': UNIT_V, 'a': j['a'], 'b': j['b'], 'spec': j['spec'], 'seed': int(j['seed']), 'play': j['play'],
                'matches': n}
        one = bool(same(j['a'], j['b']))
        if lay == 'paired':
            if n % 2:
                raise ValueError(f"judge job {j['id']}: paired matches, an even number ({n})")
            out.append(dict(base, one=one, paired=True, games=n, jobs=[{'id': j['id'], 'take': 'all'}]))
        elif lay in ('a0', 'a1') and one:
            out.append(dict(base, one=True, paired=False, games=n, jobs=[{'id': j['id'], 'take': 'all'}]))
        elif lay in ('a0', 'a1'):
            key = json.dumps([j['a'], j['b'], j['spec'], int(j['seed']), n, j['play']], sort_keys=True)
            u = groups.get(key)
            if u is None:
                u = groups[key] = dict(base, one=False, paired=True, games=2 * n, jobs=[])
                out.append(u)
            take = 'even' if lay == 'a0' else 'odd'
            if any(x['take'] == take for x in u['jobs']):
                raise ValueError(f"judge job {j['id']}: a second job of layout {lay} with the nets, spec and seed of "
                                 f"{[x['id'] for x in u['jobs']]}")
            u['jobs'].append({'id': j['id'], 'take': take})
        else:
            raise ValueError(f"judge job {j['id']}: layout {lay!r}")
    for u in out:
        u['id'] = '+'.join(x['id'] for x in u['jobs'])
        u['discard'] = u['games'] - u['matches'] * len(u['jobs'])
    return out


def plan_line(us):
    """The units in words: campaigns, games, the discarded halves."""
    games = sum(u['games'] for u in us)
    lost = sum(u['discard'] for u in us)
    line = f'{len(us)} farm campaigns, {games} matches'
    if lost:
        line += (f' ({lost} of them the seat-swapped halves of {sum(1 for u in us if u["discard"])} fixed-seat '
                 f'campaigns: played, not used - a farm job alternates its seats)')
    return line


# ------------------------------------------------------------------ a unit's farm job
def local_seats(play):
    """The two seats the local match process plays (judge.job_seats), without their keys -> [A's, B's]."""
    out = []
    for k in ('a', 'b'):
        s = dict(J.job_seats(play)[k])
        s.pop('key', None)
        out.append(s)
    return out


def unit_job(u, rule_ckpt, FC):
    """The eval job of unit `u` -> (settings, seats, b): farm_campaign.match_settings of the specialists' checkpoint
    whose rule every judge seat plays (`rule_ckpt`: Stew.judge_rule_ckpt) with the job's simulations, chance_k, spec
    and max_steps; seats [net 1, net 2] (or [net 1, net 1], `b` None: one network). FarmRefused naming what the farm
    would play differently."""
    p = u['play']
    if p.get('precision') not in (None, 'fp32'):
        raise FarmRefused(f"{u['id']}: judge.precision {p['precision']!r} is local-only (torch's arithmetic on this "
                          f"machine's GPU)")
    want = local_seats(p)
    if any(x['take'] != 'all' for x in u['jobs']):
        bad = sorted({k for s in want for k in GID_SEAT_KEYS if s.get(k)})
        if bad:
            raise FarmRefused(f"{u['id']}: the judge seat sets {bad}, whose random streams hang on the game id; a "
                              f"fixed-seat job's matches are renumbered in its campaign (match k = game 2k / 2k + 1), "
                              f"so the farm would not play the local job's matches")
    try:
        cfg, _ = FC.match_settings(rule_ckpt, int(p['sims']), u['spec'], chance_k=int(p['chance_k']), c_puct=1.0,
                                   max_steps=int(p['max_steps']))
    except FC.CampaignError as e:
        raise FarmRefused(f"{u['id']}: {e}") from None
    seats = [{'net': 1}, {'net': 1}] if u['one'] else [{'net': 1}, {'net': 2}]
    try:
        got = FC.farm_seats(cfg, seats)
    except (ValueError, SystemExit) as e:
        raise FarmRefused(f"{u['id']}: the farm refuses the seats: {e}") from None
    for side, (w, g) in enumerate(zip(want, got)):
        if w != g:
            diff = {k: (w.get(k), g.get(k)) for k in sorted(set(w) | set(g)) if w.get(k) != g.get(k)}
            raise FarmRefused(f"{u['id']}: seat {'AB'[side]}: the farm would play {diff} (local judge vs farm)")
    b = None if u['one'] else u['b']
    try:
        game = FC.ckpt_game(u['a'], b)
    except (ValueError, FC.CampaignError) as e:
        raise FarmRefused(f"{u['id']}: {e}") from None
    if game != cfg.get('game'):
        raise FarmRefused(f"{u['id']}: its checkpoints play {game!r}, the specialists' settings {cfg.get('game')!r}")
    return cfg, seats, b


def conditions(HF):
    """h2h_farm's machine conditions for a campaign of this checkout (docs/ENGINE.md 217.1, conditions 3-5) -> the
    release index's entry of its commit; FarmRefused with the reason."""
    why = HF.can_put()
    if why:
        raise FarmRefused(why)
    try:
        dirty = HF.tracked_changes()
        if dirty:
            raise FarmRefused(f'the stew\'s checkout has uncommitted changes to tracked files ({"; ".join(dirty[:3])}'
                              f'{" ..." if len(dirty) > 3 else ""}): the farm plays its commit\'s build')
        return HF.release_of()
    except HF.NotHere as e:
        raise FarmRefused(str(e)) from None


def unit_key(plan, u, seats, build_id, sha):
    """What a unit's campaign is (its name's hash, the state's `stew_key`): the stew, generation and unit, both
    networks' sha256, the local seats, spec, seed, pairing, games and build."""
    doc = {'v': UNIT_V, 'stew': plan['stew'], 'gen': int(plan['gen']), 'unit': u['id'], 'a': sha(u['a']),
           'b': sha(u['b']), 'seats': seats, 'local_seats': local_seats(u['play']), 'spec': u['spec'],
           'max_steps': int(u['play']['max_steps']), 'seed': int(u['seed']), 'paired': bool(u['paired']),
           'games': int(u['games']), 'build': build_id}
    return hashlib.sha256(json.dumps(doc, sort_keys=True, default=str).encode()).hexdigest()


def unit_name(plan, u, key):
    """'stew-<stew>-g<generation>-<arm>-<hash12>' (the campaign's farm run: 'cmp-' + it, at most 63)."""
    arm = u['jobs'][0]['id'].split('.')[0].lower()[:1] or 'x'
    return f"stew-{plan['stew']}-g{int(plan['gen']):03d}-{arm}-{key[:12]}"


def check_name(FC, run, key, client):
    """A campaign of that name here is this unit's (its `stew_key`); one on the coordinator without a state here is not
    this machine's: both refused rather than resumed (h2h_farm.Runner's rule)."""
    p = os.path.join(FC.state_dir(run), 'state.json')
    if os.path.exists(p):
        st = FC.load_state(os.path.dirname(p))
        if (st.get('meta') or {}).get('stew_key') != key:
            raise FarmRefused(f'campaign {run} exists here with other inputs (not this judge unit\'s): refused rather '
                              f'than resumed')
        return
    if client is None:
        from zero import farm_def as FD
        client = FD.Client(FD.URL)
    if client.get_or_none(f'/v1/admin/run/{run}') is not None:
        raise FarmRefused(f'the coordinator has a run {run} but this machine has no state for it: refused (not this '
                          f'machine\'s campaign)')


def deadline_s(plan, u, HF):
    """A campaign's deadline after its put: judge.farm_deadline_h, else h2h_farm's 1 h + 0.25 s per game-simulation
    (a best-of-three match counted as its three games), at most 12 h."""
    if plan.get('deadline_h') is not None:
        return float(plan['deadline_h']) * 3600.0
    per = 3 if plan.get('format') == 'bo3' else 1
    return min(HF.DEADLINE_MAX_S, HF.DEADLINE_BASE_S + HF.DEADLINE_PER_WORK_S * u['games'] * per * int(u['play']['sims']))


# ------------------------------------------------------------------ results
def episodes_of(rec):
    """A traced record's games: its episode boundaries (the objects among its steps: a chained contest's next
    episode, mtgz.az.farm_campaign.describe) + 1."""
    return 1 + sum(1 for s in rec.get('steps') or [] if isinstance(s, dict))


def job_rows(take, rows):
    """A job's result rows from its campaign's rows (in game id order): 'all' = every game, its id; 'even' / 'odd' =
    match k is game 2k / 2k + 1."""
    pick = rows if take == 'all' else rows[0::2] if take == 'even' else rows[1::2]
    return [{'gid': k, 'result': [float(v) for v in r['result']], 'over': bool(r.get('over')),
             'games': episodes_of(r.get('rec') or {}), 'decisions': None} for k, r in enumerate(pick)]


def write_results(plan, u, by, seats, d, res, FC):
    """The unit's campaign `d` (collected: `res`) -> each of its jobs' result file, as judge.play_job writes it, with
    `backend` 'farm' and the campaign's facts (`farm`)."""
    from zero import farm_eval as FE, h2h
    rows = sorted(FC.games(d), key=lambda r: int(r['gid']))
    if [int(r['gid']) for r in rows] != list(range(u['games'])):
        raise FC.CampaignError(f"campaign {res.get('run')}: {len(rows)} games of {u['games']}")
    sw = FE.check_seats(seats)
    for r in rows:
        if r.get('seats') != FE.seats_of(sw, int(r['gid'])):
            raise FC.CampaignError(f"campaign {res.get('run')}: game {r['gid']} was seated {r.get('seats')}, not "
                                   f"{FE.seats_of(sw, int(r['gid']))}")
    cov = res.get('coverage') or {}
    try:
        sel = h2h._tag(J.job_seats(u['play'])['a'])
    except Exception:                          # noqa: BLE001 - a name only
        sel = None
    out = {}
    for x in u['jobs']:
        job = by[x['id']]
        jr = job_rows(x['take'], rows)
        if len(jr) != job['games']:
            raise FC.CampaignError(f"{job['id']}: {len(jr)} matches of {job['games']} in campaign {res.get('run')}")
        r = {'v': J.JOB_V, 'id': job['id'], 'hash': J.job_hash(job),
             'seconds': round(float(res.get('seconds') or 0) / len(u['jobs']), 1), 'select': sel,
             'unfinished': sum(1 for y in jr if not y['over']), 'rows': jr, 'backend': 'farm',
             'farm': {'campaign': res.get('run'), 'unit': u['id'], 'take': x['take'], 'paired': u['paired'],
                      'seed': u['seed'], 'campaign_games': u['games'], 'discarded': u['discard'],
                      'build': res.get('build_id'), 'digest': res.get('digest'),
                      'coverage': {'farm': cov.get('farm'), 'local': cov.get('local'),
                                   'failed': len(cov.get('failed') or [])},
                      'spot': {k: (res.get('spot') or {}).get(k) for k in ('checked', 'ok')},
                      'by_route': {lab: v.get('games') for lab, v in (res.get('by_route') or {}).items()}}}
        if not valid_result(job, r):
            raise FC.CampaignError(f"{job['id']}: the campaign's rows are not the job's result")
        jsave(os.path.join(plan['dir'], job['id'] + '.json'), r)
        out[job['id']] = r
    return out


# ------------------------------------------------------------------ the runner
def run(plan, client=None, log=_log, poll=60.0, sleep=time.sleep):
    """A phase's judge jobs on the farm (module docs): every unit without its result files checked (any refusal before
    anything is put), then put - `at_once` campaigns out at a time - and collected into the jobs' result files.
    -> {'units', 'played' (campaigns collected now), 'skipped' (units whose results were there)}."""
    import mtgz.az  # noqa: F401  (the pinned core + the game package)
    from mtgz.az import farm_campaign as FC, h2h_farm as HF
    if plan.get('v') != UNIT_V:
        raise FarmRefused(f"plan version {plan.get('v')!r}, this checkout reads {UNIT_V}")
    jdir, jobs = plan['dir'], plan['jobs']
    by = {j['id']: j for j in jobs}
    shas = {}

    def sha(p):
        k = os.path.normcase(os.path.abspath(p))
        if k not in shas:
            shas[k] = FC.file_sha256(p)
        return shas[k]

    us = units(jobs, lambda a, b: same_path(a, b) or sha(a) == sha(b))
    todo = [u for u in us if not all(valid_result(by[x['id']], jload(os.path.join(jdir, x['id'] + '.json')))
                                     for x in u['jobs'])]
    log(f"judge on the farm: {plan_line(us)}; {len(us) - len(todo)} with their results already")
    if not todo:
        return {'units': len(us), 'played': 0, 'skipped': len(us)}
    e = conditions(HF)
    lack = HF.no_onnx()
    share = HF.AUTO_SHARE if plan.get('share') is None else float(plan['share'])
    ready = []
    for u in todo:                             # (every check first: nothing is put for a phase that would be refused)
        cfg, seats, b = unit_job(u, plan['rule_ckpt'], FC)
        if lack and HF.missing_exports(types.SimpleNamespace(a=u['a']), cfg, b, e):
            raise FarmRefused(f"this Python cannot export the networks for the farm (no {', '.join(lack)}; "
                              f"code.python: an environment with them)")
        tmp = tempfile.mkdtemp(prefix='stew-judge-farm-')
        try:
            FC.job_settings(e['worker'], e['build_id'], cfg, tmp, lambda m: None, seats=seats)
        except FC.CampaignError as x:
            raise FarmRefused(f"{u['id']}: build {e['build_id']} refuses the job: {str(x)[-600:]}") from None
        finally:
            shutil.rmtree(tmp, ignore_errors=True)
        key = unit_key(plan, u, seats, e['build_id'], sha)
        name = unit_name(plan, u, key)
        check_name(FC, FC.slug(name), key, client)
        ready.append((u, cfg, seats, b, key, name))
    at_once = max(1, int(plan.get('at_once') or 1))
    active, played = [], 0
    while ready or active:
        while ready and len(active) < at_once:
            u, cfg, seats, b, key, name = ready.pop(0)
            log(f"campaign cmp-{name}: {u['id']} ({u['games']} matches{', paired' if u['paired'] else ''}"
                + (f", {u['discard']} discarded" if u['discard'] else '') + ')')
            d = FC.submit(name, u['a'], u['games'], cfg, seats, u['seed'], e['build_id'], e['worker'], e['runtime'],
                          b=b, paired=u['paired'], share=share, deadline_s=deadline_s(plan, u, HF),
                          meta={'cmd': 'stew-judge', 'stew_key': key, 'stew': plan['stew'], 'gen': int(plan['gen']),
                                'phase': plan.get('phase'), 'unit': u['id']},
                          client=client, log=log)
            active.append((u, seats, d))
        still, got = [], 0
        for u, seats, d in active:
            res = FC.collect(d, client=client, threads=int(plan.get('threads') or 4), wrap=HF.topup_wrap,
                             describe_games=False, wait=False, log=log)
            if res is None:
                still.append((u, seats, d))
                continue
            write_results(plan, u, by, seats, d, res, FC)
            cov = res.get('coverage') or {}
            log(f"campaign {res.get('run')}: collected ({cov.get('farm')} farm + {cov.get('local')} local games) -> "
                f"{', '.join(x['id'] for x in u['jobs'])}")
            got += 1
        active = still
        played += got
        if active and not got:
            sleep(poll)
    return {'units': len(us), 'played': played, 'skipped': len(us) - len(todo)}


# ------------------------------------------------------------------ the command line (the runner's process)
def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    if len(argv) != 2 or argv[0] != 'run':
        raise SystemExit('usage: python -m mtgz.stew.judge_farm run PLAN.json (docs/ENGINE.md 249)')
    import mtgz.az  # noqa: F401
    from mtgz.az import farm_campaign as FC
    plan = jload(argv[1])
    if not isinstance(plan, dict):
        raise SystemExit(f'{argv[1]}: not a plan')
    try:
        out = run(plan)
    except FarmRefused as e:
        print(f'REFUSED: {e}', flush=True)
        return 2
    except FC.CampaignError as e:
        print(f'FAILED: {e}', flush=True)
        return 1
    print(f"judge on the farm: {out['played']} campaigns collected, {out['skipped']} units were done", flush=True)
    return 0


if __name__ == '__main__':
    sys.exit(main())
