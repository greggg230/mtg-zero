"""zero.h2h's farm backend for MTG Zero (docs/ENGINE.md 217; zero-core docs/az.md 69): `python -m mtgz.az.h2h A B ...`
plays its games on zero-farm (friends' idle PCs and the Macs, through mtgz.az.farm_campaign) whenever the farm can
play exactly that match - the default, `--backend auto` - and on this machine otherwise, decided before any game with
one line saying which and why. `--backend farm`: the farm or a refusal (exit 2, the reason); `--backend local`: here.

The conditions (`plan`, in this order; the first that fails is the line's reason, e.g. `h2h: backend local -
--lookahead is local-only`):

  1. no local-only option: zero.seat_settings.LOCAL_ONLY (--graphs / --a-graphs ..., --lookahead / --a-lookahead ...:
     torch's CUDA graphs, the Python lookahead), --precision other than fp32 (torch's arithmetic), --move-log (the local
     driver's log). The local resources --device, --torch-threads, --conc, --groups, --sparse, --max-rows mean nothing
     on the farm and are ignored there; --threads is the local top-up's (below).
  2. both checkpoints have their run's config.json next to models/ (the job's settings are A's run's; B's seat its
     own run's settings as its set).
  3. this machine can put a campaign: the farm's admin token, data key and farm-release (zero.farm_def's paths); and
     (checked after 6) this Python can export the networks the campaign still needs (onnx, onnxruntime, onnxscript;
     not needed when the export cache has them, `missing_exports`).
  4. the checkout's tracked files are clean (`git status --untracked-files=no`, the zero-core submodule included): the
     farm plays the commit's build, so an uncommitted change would make the two backends play different games.
  5. a published farm build for the checkout's commit (mtgz.az.farm_builds.lookup: its own release, or that of a
     commit with the same Rust inputs; `python tools/farm_release.py release <sha>` makes one, ~20 minutes - never run
     from here: a release builds, gates and publishes, a heavier step than a head-to-head should take on its own), its
     signed worker on this machine.
  6. the farm job seats both sides exactly as the local match does (`job`): the job's settings are A's run's resolved
     settings with the match's simulations, c_puct, chance_k, spec and max_steps (5000), each side's seat
     `{"net": gen, "set": ...}` carrying every evaluation-seat setting (zero.seat_settings.SEAT_KEYS) in which that
     side's own settings - its run's, with zero.h2h's per-side options over them (`h2h.sides_of`) - differ from the
     job's; the farm's seats (zero.farm_eval.seat_dicts) are then compared with the local match's, dict for dict. And
     the build's own check-config takes the settings and seats (mtgz.az.farm_campaign.job_settings).
  7. under 'auto' only, big enough: games x the sides' mean simulations >= --farm-min-work (12,800 = 200 games at 64
     simulations; a sequential match: its --max-games). Below it the farm's fixed costs dominate: the first export of a
     checkpoint and its GPU variants, ~5-8 minutes of this machine, and the queue behind the farm's rolling self-play
     and fuzz leases, often 20-45 minutes; such a match is a few minutes of this machine's GPU. 0 = any size.

The decision line of a match that passes 1-6 carries an estimate (`estimate`), so the choice is visible: the farm's
minutes - setup (the exports this campaign still needs, from the export cache), queue and play at the farm's measured
rates (the recent campaigns on this machine: their put-to-first-result wait and their game-simulations a minute of
play; else 30 minutes and 270 a minute) - against this machine's minutes at its local rate (--device: the GPU 2,560,
the CPU 45 game-simulations a minute).

The farm path (`Runner`): each batch (the plain match: one; `--sprt` / `--target-se`: zero.h2h's `batch_loop`, batch k
seeded seed + 7919 k) is one analysis campaign (docs/ENGINE.md 165, 215) - `cmp-h2h-<run>-<A>v<B>-<hash>[-b<k>]`, the
hash over both networks' sha256, the job's play settings, seats, seed, games, pairing and build, so the same command
again resumes or collects the same campaign instead of playing it again (a campaign of that name with other inputs, or
one on the coordinator without its state here, is refused). Every alternate of the build's release (CUDA, DirectML for
AMD, macOS when its settings replay exactly there) with the GPU variants, share --farm-share (0.5, `AUTO_SHARE`: above
the campaigns' 0.25 while a run is live, because the alternative is this machine's GPU under that run). The deadline
(per batch): --farm-deadline-h, else 1 h + 0.25 s per game-simulation, at most 12 h; then the games the farm has not
delivered are played here by the same worker (its --threads), as every campaign does - each piece holding a slot of the
machine's heavy-command throttle (tools/slotd.sh, docs/ENGINE.md 166) while a run is live here (`topup_wrap`), so it
never piles onto a live run's iteration. Its rows come back to zero.h2h
(the summary and the line are a local match's; --records from the traced records), and the line adds `campaign`, the
build and its `routes`, `coverage` (farm / local games), `machines` (a short hash of each farm machine id, as the game
records keep it, with its games; 'local' for the top-up), `by_route` (A's score per route: 'cpu', 'cuda:gpu' - the CUDA
alternate's float16 variant -, 'dml:dml', 'macos-arm64', 'local'; mtgz.az.farm_campaign.by_route, from the leases
table), `games_digest`, `spot` and the times (`setup_s`, `queue_s`: the batch put to its first result, `wall_s`).

Results are not bit-identical across backends (torch here, the ONNX worker there; the GPU routes play float16 /
MatMul variants): the line's `backend` says which, so series are never mixed by accident.
"""
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
NAME = 'farm'
MIN_WORK = 12800
DEADLINE_BASE_S = 3600.0
DEADLINE_PER_WORK_S = 0.25
DEADLINE_MAX_S = 12 * 3600.0
MAX_STEPS = 5000                                   # (zero.h2h.match's default: the local match's driver)
# an auto campaign's share of the farm's run leases beside a live run's self-play (zero-farm PROTOCOL 5.4): above the
# campaigns' 0.25 (tools/farm_campaign.py's default) - the alternative is this machine's GPU, i.e. pausing or slowing
# the live run that trains on it for hours (2026-11-20: 72 minutes), and a head-to-head campaign has a deadline
AUTO_SHARE = 0.5
# the estimate on the decision line (game-simulations a minute): this machine's local rate, measured - the GPU from
# the 2026-11-20 ISMCTS check (4 x 2,400 Legacy games at 64 simulations in ~240 minutes, conc 512, 6 threads), the CPU
# from this section's 8-game check (docs/ENGINE.md 217); the farm's from the recent campaigns on this machine
# (`farm_history`), else these defaults (215.3: 64 games at 32 simulations in 7.5 minutes of play, 24 queued)
LOCAL_RATE = {'cuda': 2560.0, 'cpu': 45.0}
FARM_RATE = 270.0
FARM_QUEUE_MIN = 30.0
EXPORT_MIN = 2.5                                   # (a checkpoint's first export, 215.3: 128-150 s)
VARIANT_MIN = 2.4                                  # (its first fast-matmul variant of an architecture: 140 s)


def _log(m):
    print('# ' + m, file=sys.stderr, flush=True)


def _flag(dest):
    return '--' + dest.replace('_', '-')


class NotHere(Exception):
    """The farm cannot play this match (the reason)."""


# ------------------------------------------------------------------------------------------------ the conditions
def local_only(a):
    """The first local-only option given (condition 1) as a reason, else None."""
    from zero import seat_settings as SS
    extra = sorted(set(SS.given(a, local=True)) - set(SS.given(a)))
    if extra:
        k = extra[0]
        base = k[2:] if k[:2] in ('a_', 'b_') else k
        return f'{_flag(k)} is local-only ({SS.LOCAL_ONLY[base]})'
    if getattr(a, 'precision', None) not in (None, 'fp32'):
        return f"--precision {a.precision} is local-only (torch's arithmetic on this machine's GPU)"
    if getattr(a, 'move_log', 0):
        return "--move-log is local-only (the local driver's move log)"
    return None


def tracked_changes(root=ROOT):
    """The checkout's uncommitted changes to tracked files (`git status --porcelain --untracked-files=no`: the zero-core
    submodule included) -> [line]; NotHere when git cannot say."""
    p = subprocess.run(['git', 'status', '--porcelain', '--untracked-files=no'], cwd=root, capture_output=True,
                       text=True, encoding='utf-8', errors='replace')
    if p.returncode:
        raise NotHere(f'git status failed in the checkout ({p.stderr.strip()[:200]})')
    return [x for x in p.stdout.splitlines() if x.strip()]


def release_of(root=ROOT):
    """The published farm build for the checkout's commit (condition 5) -> its index entry; NotHere naming the command
    that makes one."""
    from . import farm_builds as FB
    try:
        e = FB.lookup('HEAD', root)
    except (ValueError, OSError) as x:
        raise NotHere(f'the release index cannot be read ({x})') from None
    if e is None:
        try:
            sha = FB.resolve('HEAD', root)[:12]
        except ValueError:
            sha = 'HEAD'
        where = FB.index_path()
        raise NotHere(f'no published farm build for {sha} ({"no release index on this machine" if not where else where}'
                      f'; python tools/farm_release.py release {sha})')
    if not e.get('worker') or not os.path.isfile(e['worker']):
        raise NotHere(f'build {e["build_id"]}: its signed worker is not on this machine ({e.get("worker")})')
    return e


def can_put():
    """What this machine lacks to put a campaign (condition 3), else None."""
    from zero import farm_def as FD
    if not os.environ.get('ZEROFARM_ADMIN_TOKEN') and not os.path.isfile(FD.token_path()):
        return 'no farm admin token on this machine (zero.farm_def.token_path)'
    if not os.path.isfile(FD.data_key_path()):
        return 'no farm data key on this machine (definitions cannot be signed)'
    fr = FD.farm_release_path()
    if not (os.path.isfile(fr) or shutil.which(fr)):
        return f'no farm-release on this machine ({fr})'
    return None


def local_seats(a, sides):
    """The two seats a local zero.h2h match plays (`h2h._seats` without loading the networks), without keys."""
    from zero import games as G, h2h
    out = []
    for side, ck, sims, seat in (('a', a.a, a.a_sims, sides[0]), ('b', a.b, a.b_sims, sides[1])):
        s = G.nn_seat(side, a.sims if sims is None else int(sims), a.c_puct, chance_k=a.chance_k, **h2h.run_seat_keys(ck))
        s.update(seat)
        s.pop('key', None)
        out.append(s)
    return out


def job(a, sides):
    """The farm job of a command line (condition 6) -> (settings, seats, b): the eval job's full settings, its seats
    (each side's set: every seat setting in which its own settings differ from the job's) and B's checkpoint (None: A's
    network on both seats). NotHere when the farm's seats are not the local match's."""
    from zero import games as G, h2h, seat_settings as SS
    from . import farm_campaign as C
    spec = h2h.match_spec(a.a, a.b, C.ckpt_game(a.a, a.b), a.spec)
    own = [h2h.side_settings(ck, a.sims, a.c_puct, a.chance_k) for ck in (a.a, a.b)]
    try:
        sets = SS.sets_of(a, own[0], own[1])
    except ValueError as e:
        raise NotHere(str(e)) from None
    full = [SS.seat_cfg(own[k], sets[k] or None) for k in range(2)]
    cfg = dict(h2h.run_settings(a.a))
    cfg.update(eval_sims=int(a.sims), c_puct=float(a.c_puct), chance_k=int(a.chance_k), max_steps=MAX_STEPS,
               eval_spec=G.spec_json(spec), sparse=0, eval_trace=1, game=C.ckpt_game(a.a, a.b))
    seat_sets = []
    for k, f in enumerate(full):
        lack = sorted(x for x in SS.SEAT_KEYS if x in cfg and x not in f)
        if lack:
            raise NotHere(f'side {"AB"[k]}: its settings have no {lack} (a seat set cannot remove a setting)')
        seat_sets.append(SS.changed({x: cfg[x] for x in SS.SEAT_KEYS if x in cfg},
                                    {x: f[x] for x in SS.SEAT_KEYS if x in f}))
    same = (os.path.normcase(os.path.realpath(a.a)) == os.path.normcase(os.path.realpath(a.b))
            or C.file_sha256(a.a) == C.file_sha256(a.b))
    seats = [SS.net_seat(1, seat_sets[0] or None), SS.net_seat(1 if same else 2, seat_sets[1] or None)]
    try:
        got = C.farm_seats(cfg, seats)
    except (ValueError, SystemExit) as e:
        raise NotHere(f'the farm refuses the seats: {e}') from None
    for k, (w, g) in enumerate(zip(local_seats(a, sides), got)):
        if w != g:
            diff = {x: (w.get(x), g.get(x)) for x in sorted(set(w) | set(g)) if w.get(x) != g.get(x)}
            raise NotHere(f'side {"AB"[k]}: the farm would play {diff} (local vs farm)')
    return cfg, seats, None if same else a.b


def work_of(a, seq):
    """The match's game-simulations: games (a sequential match: its most) x the sides' mean simulations."""
    games = (a.max_games or 4 * a.games) if seq else a.games
    sa = a.sims if a.a_sims is None else a.a_sims
    sb = a.sims if a.b_sims is None else a.b_sims
    return int(games * (sa + sb) / 2)


def deadline_s(a, games):
    """A batch's deadline (seconds after its put): --farm-deadline-h, else 1 h + 0.25 s per game-simulation, at most
    12 h."""
    if getattr(a, 'farm_deadline_h', None) is not None:
        return float(a.farm_deadline_h) * 3600.0
    sa = a.sims if a.a_sims is None else a.a_sims
    sb = a.sims if a.b_sims is None else a.b_sims
    return min(DEADLINE_MAX_S, DEADLINE_BASE_S + DEADLINE_PER_WORK_S * games * (sa + sb) / 2)


def routes_of(e):
    """The release entry's routes beside its CPU build, in words ('CUDA/DML/macOS')."""
    from . import farm_builds as FB
    out = [w for k, w in (('cuda', 'CUDA'), ('dml', 'DML')) if (e.get(k) or {}).get('build_id')]
    if FB.mac_build(e):
        out.append('macOS')
    return '/'.join(out) or 'CPU only'


def farm_history(root=None, n=12):
    """The farm's measured pace from the newest campaigns on this machine that kept it (a `first_result` and a result
    with no local game) -> (queue minutes, play game-simulations a minute) medians, or None for each without data."""
    import glob
    from . import farm_campaign as C
    paths = sorted(glob.glob(os.path.join(root or C.STATE_ROOT, 'cmp-*', 'state.json')), key=os.path.getmtime)[-60:]
    qs, rates = [], []
    for p in reversed(paths):
        try:
            with open(p, encoding='utf-8') as f:
                st = json.load(f)
        except (OSError, ValueError):
            continue
        res, sub, first = st.get('result') or {}, st.get('submitted'), st.get('first_result')
        if not (res and sub and first) or (res.get('coverage') or {}).get('local'):
            continue
        sims = [(s.get('set') or {}).get('eval_sims', (st.get('settings') or {}).get('eval_sims', 0))
                for s in st.get('seats') or []]
        work = st['games'] * (sum(sims) / len(sims) if sims else 0)
        play_s = res.get('seconds', 0) - (first - sub)
        qs.append((first - sub) / 60.0)
        if work and play_s > 30:
            rates.append(work / (play_s / 60.0))
        if len(qs) >= n:
            break
    med = lambda v: sorted(v)[len(v) // 2] if v else None  # noqa: E731
    return med(qs), med(rates)


ONNX_MODULES = ('onnx', 'onnxruntime', 'onnxscript')   # (a network's export for the farm: zero.onnx_export)


def missing_exports(a, cfg, b, entry=None):
    """The exports a campaign of this match still needs (mtgz.az.farm_campaign's export cache: each network's graph
    and reference set, its GPU variants for the release's CUDA ('fp16') and DirectML ('matmul') routes) -> [(gen, what)]."""
    from zero import games as G
    from . import farm_campaign as C
    forms = [f for k, f in (('cuda', 'fp16'), ('dml', 'matmul')) if entry is None or (entry.get(k) or {}).get('build_id')]
    out = []
    for gen, ck in ((1, a.a), (2, b)):
        if ck is None:
            continue
        key = hashlib.sha256(f'{C.file_sha256(ck)}|{G.spec_json(cfg["eval_spec"])}|{gen}'.encode()).hexdigest()[:20]
        d = os.path.join(C.EXPORT_CACHE, key)
        if not os.path.exists(os.path.join(d, 'export.json')):
            out.append((gen, 'export'))
        out += [(gen, f) for f in forms if not os.path.exists(os.path.join(d, f'variant-{f}.json'))]
    return out


def no_onnx():
    """The modules this Python lacks for an export (`ONNX_MODULES`), [] when it has them."""
    import importlib.util
    return [m for m in ONNX_MODULES if importlib.util.find_spec(m) is None]


def estimate(a, seq, cfg, b, entry=None):
    """The decision line's estimate (module docs): 'estimate: farm ~S setup + ~Q queue + ~P play = ~T min, local ~L min
    of this machine's GPU|CPU'."""
    w = work_of(a, seq)
    setup = 0.5
    for gen, what in missing_exports(a, cfg, b, entry):
        setup += {'export': EXPORT_MIN, 'matmul': VARIANT_MIN if gen == 1 else 0.1}.get(what, 0.05)
    q, rate = farm_history()
    q = FARM_QUEUE_MIN if q is None else q
    rate = FARM_RATE if rate is None else rate
    play = w / rate
    dev = 'GPU' if str(getattr(a, 'device', 'cpu')).startswith('cuda') else 'CPU'
    loc = w / LOCAL_RATE['cuda' if dev == 'GPU' else 'cpu']
    return (f'estimate: farm ~{setup:.0f} setup + ~{q:.0f} queue + ~{play:.0f} play = ~{setup + q + play:.0f} min, '
            f"local ~{loc:.0f} min of this machine's {dev}")


def _bash():
    """Git Bash on Windows (a bare `bash` there can be WSL's: tools/fuzz_steward.py's rule; MTGZ_BASH overrides),
    `bash` elsewhere."""
    from zero import local_config
    v = local_config.get('MTGZ_BASH')
    if v:
        return v
    if os.name == 'nt':
        g = shutil.which('git')
        if g:
            base = os.path.dirname(os.path.dirname(os.path.realpath(g)))
            for c in (os.path.join(base, 'bin', 'bash.exe'), os.path.join(os.path.dirname(base), 'bin', 'bash.exe')):
                if os.path.exists(c):
                    return c
    return 'bash'


def topup_wrap():
    """A local top-up piece's command prefix (asked before each piece): a slot of the machine's heavy-command
    throttle (tools/slotd.sh, docs/ENGINE.md 166) while a run's supervisor is live here (zero.runctl.live_supervisors),
    else none."""
    from zero import runctl
    try:
        live = runctl.live_supervisors()
    except Exception:                              # noqa: BLE001 - unknown: throttle
        live = True
    return [_bash(), os.path.join(ROOT, 'tools', 'slotd.sh')] if live else None


def _merge_routes(done):
    """The batches' per-route games (a sequential match: the scores are per batch, in each batch's own by_route)."""
    out = {}
    for x in done:
        for lab, r in (x.get('by_route') or {}).items():
            out[lab] = {'games': out.get(lab, {}).get('games', 0) + r.get('games', 0)}
    return out


# ------------------------------------------------------------------------------------------------ names
def _it(path):
    stem = os.path.splitext(os.path.basename(path))[0]
    m = re.match(r'^iter_0*(\d+)$', stem)
    return m.group(1) if m else stem[:12]


def campaign_key(a, play, seats, build_id, k, n, seed, seq):
    """What a batch's campaign is (its name's hash and the state's `h2h_key`): both networks' sha256, the job's play
    settings, seats, pairing, seed, games and build."""
    from . import farm_campaign as C
    doc = {'v': 1, 'a': C.file_sha256(a.a), 'b': C.file_sha256(a.b), 'settings': play, 'seats': seats,
           'paired': bool(a.paired), 'seed': int(seed), 'games': int(n), 'build': build_id, 'batch': k if seq else None}
    return hashlib.sha256(json.dumps(doc, sort_keys=True, default=str).encode()).hexdigest()


def campaign_name(a, key, k, seq):
    run = os.path.basename(os.path.dirname(os.path.dirname(os.path.abspath(a.a))))
    run = re.sub(r'[^a-z0-9]+', '-', run.lower()).strip('-')[:16].strip('-') or 'run'
    return f'h2h-{run}-{_it(a.a)}v{_it(a.b)}-{key[:10]}' + (f'-b{k}' if seq else '')


# ------------------------------------------------------------------------------------------------ the backend
class Backend:
    """GamePackage.h2h_backend (zero-core docs/az.md 69)."""
    name = NAME

    def add_args(self, ap):
        ap.add_argument('--farm-min-work', type=int, default=MIN_WORK, metavar='N',
                        help=f"auto: the farm only for matches of at least N game-simulations (games x the sides' mean "
                             f"simulations; default {MIN_WORK} = 200 games at 64; 0 = any size; docs/ENGINE.md 217)")
        ap.add_argument('--farm-deadline-h', type=float, default=None, metavar='H',
                        help='hours after a batch is put before the games the farm has not delivered are played here '
                             'by the same worker (default 1 h + 0.25 s per game-simulation, at most 12 h)')
        ap.add_argument('--farm-share', type=float, default=None, metavar='X',
                        help=f"the campaign's share of the farm's run leases beside a live run's self-play (default "
                             f"{AUTO_SHARE}: above the campaigns' 0.25 - the alternative is this machine's GPU under that "
                             f"run)")

    def plan(self, a, ask, sides, seq):
        """-> (ok, why, runner): the conditions of the module docs, in order."""
        try:
            return True, *self._plan(a, ask, sides, seq)
        except NotHere as e:
            return False, str(e), None

    def _plan(self, a, ask, sides, seq):
        from zero import h2h
        from . import farm_campaign as C
        why = local_only(a)
        if why:
            raise NotHere(why)
        for side, ck in (('A', a.a), ('B', a.b)):
            if not h2h.run_config(ck):
                raise NotHere(f'side {side} ({os.path.basename(ck)}) has no run config next to models/ (the farm '
                              f"job's settings are a run's)")
        why = can_put()
        if why:
            raise NotHere(why)
        dirty = tracked_changes()
        if dirty:
            raise NotHere(f'the checkout has uncommitted changes to tracked files ({"; ".join(dirty[:3])}'
                          f'{" ..." if len(dirty) > 3 else ""}): the farm plays its commit\'s build')
        e = release_of()
        cfg, seats, b = job(a, sides)
        lack = no_onnx()
        if lack and missing_exports(a, cfg, b, e):
            raise NotHere(f'this Python cannot export the networks for the farm (no {", ".join(lack)}; the zero-farm '
                          f'environment has them, and a network exported once is cached for every later campaign)')
        share = AUTO_SHARE if a.farm_share is None else a.farm_share
        if not 0 < share <= 1:
            raise NotHere(f'--farm-share {share}: in (0, 1]')
        d = tempfile.mkdtemp(prefix='h2h-farm-')
        notes = []
        try:
            play = C.job_settings(e['worker'], e['build_id'], cfg, d, notes.append, seats=seats)
        except C.CampaignError as x:
            raise NotHere(f'build {e["build_id"]} refuses the job: {str(x)[-600:]}') from None
        finally:
            shutil.rmtree(d, ignore_errors=True)
        est = estimate(a, seq, cfg, b, e)
        if ask == 'auto':
            w = work_of(a, seq)
            if w < a.farm_min_work:
                raise NotHere(f'{w} game-simulations, below the farm\'s minimum (--farm-min-work {a.farm_min_work}: '
                              f'its setup and queue cost more than playing this here); {est}')
        r = Runner(a, e, cfg, play, seats, b, share, seq)
        first = r.name_of(0, (a.batch_games or a.games) if seq else a.games, a.seed)
        where = f'build {e["build_id"][:8]} + {routes_of(e)}'
        if not e.get('exact', True):
            where += f' (the release of {e["commit"][:12]}: the same Rust inputs)'
        return f'campaign cmp-{first}{" and one a batch after it" if seq else ""}, {where}; {est}', r


BACKEND = Backend()


class Runner:
    """Plays a command line's batches as campaigns (module docs) and keeps what the line reports of them."""

    def __init__(self, a, entry, cfg, play, seats, b, share, seq):
        self.a, self.e, self.cfg, self.play_settings, self.seats, self.b = a, entry, cfg, play, seats, b
        self.share, self.seq = share, seq
        self.done = []
        self.argv = {'a': os.path.abspath(a.a), 'b': os.path.abspath(a.b), 'games': a.games, 'sims': a.sims,
                     'seed': a.seed, 'paired': bool(a.paired)}

    def name_of(self, k, n, seed):
        key = campaign_key(self.a, self.play_settings, self.seats, self.e['build_id'], k, n, seed, self.seq)
        return campaign_name(self.a, key, k, self.seq)

    def _check_name(self, run, key, client):
        from zero import farm_def as FD
        from . import farm_campaign as C
        p = os.path.join(C.state_dir(run), 'state.json')
        if os.path.exists(p):
            st = C.load_state(os.path.dirname(p))
            if (st.get('meta') or {}).get('h2h_key') != key:
                raise C.CampaignError(f'campaign {run} exists here with other inputs (not this head-to-head\'s): '
                                      f'refused rather than resumed')
            return
        cl = client or FD.Client(FD.URL)
        if cl.get_or_none(f'/v1/admin/run/{run}') is not None:
            raise C.CampaignError(f'the coordinator has a run {run} but this machine has no state for it: refused (not '
                                  f'this machine\'s campaign)')

    def play(self, k, n, seed, client=None):
        """Batch k: its campaign put (or resumed), collected (the local top-up at the deadline) -> zero.h2h's rows with
        each game's traced record."""
        from zero import farm_eval as FE, gamerec as GR
        from . import farm_campaign as C
        a = self.a
        key = campaign_key(a, self.play_settings, self.seats, self.e['build_id'], k, n, seed, self.seq)
        name = campaign_name(a, key, k, self.seq)
        run = C.slug(name)
        self._check_name(run, key, client)
        t0 = time.time()
        d = C.submit(name, a.a, n, self.cfg, self.seats, seed, self.e['build_id'], self.e['worker'],
                     self.e['runtime'], b=self.b, paired=bool(a.paired), share=self.share,
                     deadline_s=deadline_s(a, n), meta={'cmd': 'h2h', 'h2h_key': key, 'batch': k if self.seq else None,
                                                        'argv': self.argv},
                     client=client, log=_log)
        t1 = time.time()
        _log(f'campaign {run}: put in {t1 - t0:.0f} s; collecting (deadline {deadline_s(a, n) / 3600:.2f} h after the '
             f'put; `python tools/farm_campaign.py collect {name} --deadline-now` plays the rest here now)')
        res = C.collect(d, client=client, threads=a.threads, wrap=topup_wrap, describe_games=False, log=_log)
        st = C.load_state(d)
        rows = sorted(C.games(d), key=lambda r: int(r['gid']))
        if [int(r['gid']) for r in rows] != list(range(n)):
            raise C.CampaignError(f'campaign {run}: {len(rows)} games of {n}')
        FE.a_seat_of(rows, self.seats[0])
        mach = {}
        for m, c in (res.get('machines') or {}).items():
            t = 'local' if m == 'local' else GR.machine_tag(m)
            mach[t] = mach.get(t, 0) + int(c)
        sub = st.get('submitted') or st.get('created')
        self.done.append({'campaign': run, 'games': n, 'coverage': res['coverage'], 'machines': mach,
                          'by_route': res.get('by_route') or {},
                          'digest': res['digest'], 'spot': {x: res['spot'][x] for x in ('checked', 'ok')},
                          'alternates': [x['build_id'] for x in res.get('alternates') or []],
                          'alternates_note': res.get('alternates_note'),
                          'setup_s': round((sub or t1) - t0, 1) if sub and sub >= t0 else None,
                          'queue_s': round(st['first_result'] - sub, 1) if st.get('first_result') and sub else None,
                          'wall_s': round(time.time() - t0, 1)})
        out = []
        for r in rows:
            x = FE.h2h_rows([r])[0]
            x['rec'] = r['rec']
            out.append(x)
        return out

    def info(self):
        """The line's own keys (module docs)."""
        if not self.done:
            return {}
        cov = {'farm': sum(x['coverage']['farm'] for x in self.done),
               'local': sum(x['coverage']['local'] for x in self.done),
               'failed': sum(len(x['coverage']['failed']) for x in self.done)}
        mach = {}
        for x in self.done:
            for m, c in x['machines'].items():
                mach[m] = mach.get(m, 0) + c
        one = len(self.done) == 1
        alts = sorted({b for x in self.done for b in x['alternates']})
        out = {'campaign': self.done[0]['campaign'] if one else [x['campaign'] for x in self.done],
               'build': self.e['build_id'], 'routes': [self.e['build_id']] + alts, 'coverage': cov,
               'machines': dict(sorted(mach.items(), key=lambda kv: -kv[1])),
               'games_digest': self.done[0]['digest'] if one else [x['digest'] for x in self.done],
               'spot': {'checked': sum(x['spot']['checked'] for x in self.done),
                        'ok': sum(x['spot']['ok'] for x in self.done)}}
        out['by_route'] = self.done[0]['by_route'] if one else _merge_routes(self.done)
        notes = sorted({x['alternates_note'] for x in self.done if x.get('alternates_note')})
        if notes:
            out['routes_note'] = '; '.join(notes)
        for k in ('setup_s', 'queue_s', 'wall_s'):
            v = [x[k] for x in self.done]
            out[k] = v[0] if one else v
        return out
