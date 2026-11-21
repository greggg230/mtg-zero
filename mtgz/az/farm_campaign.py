"""Farm analysis campaigns: `play` jobs (docs/ENGINE.md 165; zero-farm PROTOCOL.md 5.3; zero-core docs/az.md 50) -
"play N games with checkpoint X (against Y) under spec S over game ids [0, N) and give me every game's moves", played
by friends' PCs (zero-farm), replayed and checked here, so the desktop spends no CPU on it beside the live run.

One job model (the 2026-11-15 architecture review, hub fable_review_iteration_and_farm_2026-11-15.md 3.1): a `play`
job = {build, networks by sha256, the play settings, seats, base seed, ids [0, N)} -> one traced game record + digest
per id, positive coverage, a spot replay. Options: `trace` (always on here: the moves come back) and `samples` (the
search's policy targets and values too: self-play seats with their noise, temperature and playout cap - what a
training game is). On the wire today (PROTOCOL rev 13) a play job without samples is an eval batch with `eval_trace`
(evaluation seats: Run.nn_seat(key, eval_sims)); one with samples is the farm's self-play (records, the pool's format)
of a run of its own. Results have one schema either way (`games.jsonl`: gid, seats, result, rounds, over, digest, rec,
by), so the run's evaluation, the yardsticks, the census and analyses can move onto it without a new revision.

A campaign is a farm run of its own (`cmp-<name>`) holding one such job:

  * build      a signed release `mtgz-worker` that knows `eval_trace` (its executable: `worker`; its runtime pack)
  * networks   gen 1 = A, gen 2 = B (absent for a self-play campaign: both seats gen 1), exported for the farm as a
               run's generation is (zero.farm_h2h.export with the reference set from a short self-play,
               zero-core docs/az.md 46.9), cached by checkpoint and spec
  * settings   a run's evaluation settings (`Run.nn_seat(key, eval_sims)`, `Run.driver` of a match) with
               `eval_trace` 1: every game's row carries its record (seed, every real move, result, setup) and its
               digest; `match_settings` builds them so the seats are zero.h2h's (`h2h.match`: the yardsticks'),
               checked dict for dict
  * seats      [{"net": 1}, {"net": 2}] (A vs B; paired: games 2k, 2k + 1 share the deal, seats swapped) or
               [{"net": 1}, {"net": 1}] (self-play of A; with `samples` the only form, unpaired)
  * seed       the batch's base seed (the run's job `seed`: game g's seed is game_seed(seed, g, paired), the seed a
               local zero.h2h match of the same games uses)
  * games      ids [0, games); `games_per_lease` ('auto' = ~7.5 min of a machine)
  * share      the campaign's share of the farm's run leases while a live run's self-play also wants them
               (PROTOCOL rev 13; default 0.25; a coordinator before rev 13 ignores it: evaluation goes first)
  * deadline   seconds after which the games the farm has not delivered are played here (`local`), by the same
               worker executable on the same graphs (so they are the same games)
  * alternates the routes beside the build's CPU worker (zero-farm PROTOCOL 9.4, 11.4, 14; docs/ENGINE.md 215): by
               default every alternate the release index holds for the build (mtgz.az.farm_builds: its CUDA build,
               its DirectML build for AMD GPUs, its published macOS build - the same engine), each GPU alternate
               playing its model variant (`variants`, DEFAULT_VARIANTS 'gpu=fp16,dml=matmul@amd': zero.farm_pool's
               forms, exported per network beside its graph, signed and registered with it) once the graph it plays
               passed this machine's GPU self-test through that alternate's own signed worker and runtime pack, a
               macOS alternate once the settings replay exactly there (check-config --platform); min_agent raised
               to the variants' agent. Any failure refuses the campaign before anything is put. `alternates` None:
               none (CPU only - said in the log and the state, as is a build the index has no release of); a list:
               exactly those build ids

Collecting (`collect`): every result file the coordinator lists is downloaded (sha256 checked), each credited row's
digest checked against its record (zero.farm_eval.check_digest), then `spot` (default 5%) of the games - a random
choice seeded by the campaign's name - replayed through the engine from seed + moves (zero.farm_eval.replay_trace:
every move legal, the row's result, rounds and setup); any mismatch refuses the whole campaign (CampaignError, the
state says why). Coverage is positive: every game id is done (farm or local, with its machine) or failed (with the
reason); a game neither done nor failed after the deadline is played locally, or with `local=False` the campaign
fails loudly naming the missing ranges. The result: `games.jsonl` (one traced row per game, in gid order), the games'
digest (zero.farm_eval.games_digest: the same games give the same digest wherever and however they were played) and,
with `describe`, `records.jsonl.gz`: each game replayed into its moves - who decided, the decision's kind and the
option chosen in words (the engine's spectator view, engine/src/az/view.rs), per game of a match - for analyses.

State: `~/.zero-farm/campaigns/<run>/` (state.json, ckpt/, results/, local/, games.jsonl). Every step is idempotent;
`collect` resumes. The admin token, data key and farm-release are zero.farm_def's (never printed).
"""
import gzip
import hashlib
import json
import math
import os
import random
import re
import subprocess
import sys
import time

from zero import local_config

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
STATE_ROOT = os.path.join(os.path.expanduser('~'), '.zero-farm', 'campaigns')
EXPORT_CACHE = os.path.join(STATE_ROOT, '_exports')
PREFIX = 'cmp-'
DEFAULT_SHARE = 0.25
DEFAULT_SPOT = 0.05
DEFAULT_DEADLINE_S = 3 * 3600
EXPIRES_DAYS = 30
SLUG_RE = re.compile(r'^[a-z0-9][a-z0-9-]{0,62}$')
GAME = 'mtg'
RELEASE = 'release'                     # `alternates`: the release index's for the build (docs/ENGINE.md 215)
# the live runs' variants (zero-core docs/az.md 46.10, 67): NVIDIA's CUDA alternate the float16 graph, AMD's DirectML
# alternate the float32 MatMul form (the one zero-onnx/2 graph DirectML runs deterministically there)
DEFAULT_VARIANTS = 'gpu=fp16,dml=matmul@amd'


class CampaignError(RuntimeError):
    """A campaign refused (a trace mismatch, a game failed, games missing past the deadline, a bad setup)."""


def log_stderr(m):
    print('# ' + m, file=sys.stderr, flush=True)


def slug(name):
    """The campaign's farm run: 'cmp-<name>' (lower case, [a-z0-9-], at most 63 characters)."""
    s = re.sub(r'[^a-z0-9-]+', '-', str(name).lower()).strip('-')
    s = (s if s.startswith(PREFIX) else PREFIX + s)[:63].rstrip('-')
    if not SLUG_RE.match(s):
        raise CampaignError(f'campaign name {name!r}: no farm run name in it')
    return s


def state_dir(run, root=None):
    return os.path.join(root or STATE_ROOT, run)


def save_state(d, st):
    os.makedirs(d, exist_ok=True)
    tmp = os.path.join(d, f'state.json.{os.getpid()}.tmp')
    with open(tmp, 'w', encoding='utf-8') as f:
        json.dump(st, f, indent=1)
    os.replace(tmp, os.path.join(d, 'state.json'))


def load_state(d):
    p = os.path.join(d, 'state.json')
    if not os.path.exists(p):
        raise CampaignError(f'no campaign here: {p}')
    with open(p, encoding='utf-8') as f:
        return json.load(f)


def file_sha256(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for b in iter(lambda: f.read(1 << 20), b''):
            h.update(b)
    return h.hexdigest()


def ranges_of(gids):
    """Sorted ids -> [[lo, hi), ...]."""
    out = []
    for g in sorted(set(int(x) for x in gids)):
        if out and out[-1][1] == g:
            out[-1][1] = g + 1
        else:
            out.append([g, g + 1])
    return out


def ids_of(rs):
    return {g for lo, hi in (rs or []) for g in range(int(lo), int(hi))}


# ------------------------------------------------------------------------------------------------ settings and seats
def match_seat(ckpt, sims, rule, chance_k=2, c_puct=1.0):
    """The seat zero.h2h.match gives a side (`h2h._seats`): games.nn_seat(key, sims, c_puct, chance_k, its run's
    margin_w / redet / eval_cache) updated with the move rule `rule` (a seat dict: h2h.run_rule, with_belief) - without
    its key."""
    from zero import games as G, h2h
    s = G.nn_seat('a', int(sims), float(c_puct), chance_k=int(chance_k), **h2h.run_seat_keys(ckpt))
    s.update(rule)
    s.pop('key', None)
    return s


def farm_seats(settings, seats):
    """The driver seats an eval job plays at `settings` (zero.farm_eval.seat_dicts: Run.nn_seat), without keys."""
    from zero import farm_eval as FE
    out = []
    for s in FE.seat_dicts(settings, seats):
        s = dict(s)
        s.pop('key', None)
        out.append(s)
    return out


def match_settings(ckpt_a, sims, spec, tilts=None, ckpt_b=None, chance_k=2, c_puct=1.0, max_steps=5000):
    """The eval job's settings of zero.h2h.match(ckpt_a, ckpt_b, sims=sims, chance_k, c_puct, spec=spec,
    seat_a=seat_b=<A's run's rule with root belief tilts `tilts`>, paired) - the yardsticks' best-of-one
    (mtgz.az.yardsticks.bo1_seat: tilts = the run's without 'deck'): A's run's settings resolved, then eval_sims,
    c_puct, chance_k, max_steps, eval_spec, the belief tilts, sparse 0 (h2h's dense batches), eval_trace 1. Checked:
    the farm's seats (Run.nn_seat) are exactly the match's for both sides (B with its own run's margin_w / redet /
    eval_cache), else CampaignError naming the difference (e.g. an opponent whose run evaluates with another cache:
    an eval job has one seat setting for both sides) -> (settings, rule)."""
    from zero import games as G, h2h
    cfg = dict(h2h.run_settings(ckpt_a))
    if not cfg:
        raise CampaignError(f'{ckpt_a}: no run config next to it (runs/<run>/config.json)')
    rule = dict(h2h.run_rule(ckpt_a))
    if tilts is not None:
        rule = h2h.with_belief(rule, tilts=dict(tilts))
        cfg['eval_belief_tilts'] = {k: float(v) for k, v in sorted(dict(tilts).items()) if v}
    cfg.update(eval_sims=int(sims), c_puct=float(c_puct), chance_k=int(chance_k), max_steps=int(max_steps),
               eval_spec=G.spec_json(spec), sparse=0, eval_trace=1, game=GAME)
    want = [match_seat(ckpt_a, sims, rule, chance_k, c_puct),
            match_seat(ckpt_b or ckpt_a, sims, rule, chance_k, c_puct)]
    got = farm_seats(cfg, [{'net': 1}, {'net': 2}])
    for side, (w, g) in enumerate(zip(want, got)):
        if w != g:
            diff = {k: (w.get(k), g.get(k)) for k in sorted(set(w) | set(g)) if w.get(k) != g.get(k)}
            raise CampaignError(f'seat {"AB"[side]}: the farm would play {diff} (match vs farm): not the local '
                                f'match\'s seat - an eval job has one seat setting for both sides')
    return cfg, rule


def selfplay_settings(ckpt, overrides=None):
    """A play job with samples: the self-play settings of `ckpt`'s run (resolved as the run resolves them) with
    `overrides` over them (e.g. a spec with other dealt decks, resignation off: selfplay_resign_t 0, the most visited
    move: eps 0 and temp_moves 0) - the farm's self-play seat is Run.nn_seat('m', sims, selfplay=True) of these."""
    from zero import h2h
    cfg = dict(h2h.run_settings(ckpt))
    if not cfg:
        raise CampaignError(f'{ckpt}: no run config next to it (runs/<run>/config.json)')
    cfg.update(overrides or {})
    cfg['game'] = GAME
    return cfg


# ------------------------------------------------------------------------------------------------ the worker
def worker_info(worker):
    from zero import farm_h2h as FH
    rc, info, text = FH.worker_json(worker, 'info', '--ort-dll', 'none')
    if rc != 0 or not isinstance(info, dict):
        raise CampaignError(f'{worker} info: {text.strip()[-500:]}')
    return info


def job_settings(worker, build_id, cfg, d, log, samples=False):
    """The template's settings: the keys the worker build calls `play` for an eval job (zero.farm_h2h.play_settings:
    the seats checked the same as the full settings'; `eval_trace` must be among them - a build before it refuses or
    drops it: CampaignError) or, with `samples`, for its self-play job (zero.farm_pool.play_settings: the driver checked
    the same as the full settings')."""
    from zero import farm_h2h as FH, farm_pool as FP
    try:
        if samples:
            return FP.play_settings(worker, build_id, cfg, d, log)
        play = FH.play_settings(worker, build_id, cfg, d, log)
    except SystemExit as e:
        raise CampaignError(str(e)) from None
    if not play.get('eval_trace'):
        raise CampaignError(f'worker build {build_id} does not trace eval games (eval_trace, zero-core docs/az.md 50): '
                            f'build a worker of a commit that has it')
    return play


# ------------------------------------------------------------------------------------------------ networks
def export_gen(ckpt, gen, game, spec, run, log, cache=None):
    """`ckpt` exported for the farm (zero.farm_h2h.export, the reference set from a short self-play of the network at
    `spec`: the run's export path, zero.farm_pool._export) -> {model, sha256, model_size, reference, reference_sha256,
    reference_size, checkpoint, checkpoint_sha256}. Cached by (checkpoint sha256, spec): the same checkpoint is
    exported once for every campaign. The graph's network id: `gen`."""
    from zero import games as G
    sha = file_sha256(ckpt)
    key = hashlib.sha256(f'{sha}|{G.spec_json(spec)}|{gen}'.encode()).hexdigest()[:20]
    d = os.path.join(cache or EXPORT_CACHE, key)
    meta = os.path.join(d, 'export.json')
    if os.path.exists(meta):
        with open(meta, encoding='utf-8') as f:
            info = json.load(f)
        if all(os.path.exists(info[k]) and file_sha256(info[k]) == info[s] for k, s in
               (('model', 'sha256'), ('reference', 'reference_sha256'))):
            log(f'gen {gen}: the cached export of {os.path.basename(ckpt)} ({info["sha256"][:12]})')
            return info
    from zero import farm_h2h as FH
    from zero.onnx_export import load_checkpoint
    t0 = time.time()
    m, _ = load_checkpoint(ckpt, game)
    rows = FH.record_rows(m, game, G.spec_json(spec))
    # (the graph's checkpoint label names the checkpoint, not a campaign: one export serves every campaign)
    info = FH.export(ckpt, gen, d, game, [rows], f'ckpt-{sha[:12]}', checkpoint_iter=gen)
    info = {k: info[k] for k in ('model', 'sha256', 'model_size', 'reference', 'reference_sha256', 'reference_size',
                                 'checkpoint', 'checkpoint_sha256')}
    info['export_s'] = round(time.time() - t0, 1)
    with open(meta, 'w', encoding='utf-8') as f:
        json.dump(info, f, indent=1)
    log(f'gen {gen}: {os.path.basename(ckpt)} exported in {info["export_s"]} s ({info["sha256"][:12]})')
    return info


# ------------------------------------------------------------------------------------------------ alternates, variants
def alternates_want(alternates, variants):
    """submit's `alternates` and `variants` -> the state's `alt_want` {"alternates": RELEASE | [ids], "variants": the
    pool_variants text}: RELEASE (the default) = the release index's alternates for the build; None / [] / '' / 'none'
    = none; a list (or comma-separated text) of worker build ids = exactly those, in that order. CampaignError for a bad
    id or variant setting."""
    from zero import farm_def as FD, farm_pool as FP
    if alternates is None or (isinstance(alternates, str) and alternates.strip().lower() in ('', 'none')):
        ids = []
    elif isinstance(alternates, str) and alternates.strip() == RELEASE:
        ids = RELEASE
    else:
        ids = FP.alternate_ids(alternates)
        if any(not FD.BUILD_RE.match(b) for b in ids) or len(ids) > FD.MAX_ALTERNATES or len(set(ids)) != len(ids):
            raise CampaignError(f'alternates {alternates!r}: up to {FD.MAX_ALTERNATES} distinct worker build ids (24 hex)')
    v = '' if variants is None or str(variants).strip().lower() in ('', 'none') else str(variants).strip()
    try:
        FP.variant_setting(v)
    except ValueError as e:
        raise CampaignError(f'variants: {e}') from None
    return {'alternates': ids, 'variants': v}


def release_alternates(build_id, path=None):
    """The alternates the release index (mtgz.az.farm_builds; tools/farm_release.py writes it) holds for worker build
    `build_id`: its newest published entry whose build it is -> (ids, notes): ids in order of preference - the CUDA
    alternate, the DirectML alternate for AMD GPUs, the published macOS build (each the same engine, signed for its
    pack); notes: each route the release lacks, or all of them when the index has no release of the build."""
    from . import farm_builds as FB
    hit = [e for e in FB.load(path)['builds'] if e.get('published') and e.get('build_id') == build_id]
    if not hit:
        return [], [f'build {build_id} has no published release in the release index '
                    f'({path or FB.index_path()}; tools/farm_release.py): no alternates, CPU only']
    e = hit[-1]
    rel = f'release of {e["commit"][:12]}'
    ids, notes = [], []
    for route, what in (('cuda', 'CUDA'), ('dml', 'DirectML (AMD GPUs)')):
        b = (e.get(route) or {}).get('build_id')
        if b:
            ids.append(b)
        else:
            notes.append(f'the {rel} has no {what} alternate')
    mac = (e.get('mac') or {}).get('release') or {}
    if mac.get('build_id') and mac.get('published'):
        ids.append(mac['build_id'])
    elif mac.get('build_id'):
        notes.append(f'the {rel} has a macOS build {mac["build_id"]} that is signed but not published '
                     f'(tools/farm_release.py --mac-publish): not listed, Macs take none of the leases')
    else:
        notes.append(f'the {rel} has no macOS build (tools/farm_release.py --mac): Macs take none of the leases')
    return ids, notes


def local_build(cl, build_id, kind):
    """A registered build's signed directory on this machine (mtgz.az.farm_builds.builds_dir()/<id>/: the desktop's
    copies of published builds) checked against the coordinator's signed manifest (the local manifest's bytes are its
    id; a worker's executable its manifest's sha256; its platform this machine's - another platform's executable is
    never run here) -> (dir, manifest). CampaignError when it is not here."""
    from zero import farm_def as FD
    from . import farm_builds as FB
    base = FB.builds_dir()
    d = os.path.join(base, build_id) if base else None
    mp = os.path.join(d, 'manifest.json') if d else None
    if not mp or not os.path.exists(mp):
        raise CampaignError(f'{kind} build {build_id}: no copy of it on this machine ({d}): the self-test of the graph '
                            f'its alternate plays needs its signed directory (tools/farm_release.py publishes it there)')
    with open(mp, 'rb') as f:
        b = f.read()
    if hashlib.sha256(b).hexdigest()[:24] != build_id:
        raise CampaignError(f'{mp}: not the manifest of build {build_id} (sha256)')
    man = json.loads(b)
    if man.get('kind') != kind:
        raise CampaignError(f'build {build_id} is a {man.get("kind")!r} build, not a {kind}')
    plat = man.get('platform') or FD.PLATFORM_DEFAULT
    if plat != FD.host_platform():
        raise CampaignError(f'build {build_id} is a {plat} build: this machine ({FD.host_platform()}) cannot run it')
    if kind == 'worker':
        fe = next((x for x in man.get('files') or [] if x.get('path') == man.get('entry')), None)
        exe = os.path.join(d, man.get('entry') or '')
        if fe is None or not os.path.isfile(exe) or file_sha256(exe) != fe['sha256']:
            raise CampaignError(f'build {build_id}: {exe} is not the executable its manifest signs')
    return d, man


def gpu_selftest(exe, pack_dir, provider, model, reference, timeout=900):
    """The worker's self-test of graph `model` on this machine's GPU through `provider` (the alternate's pack's) and on
    the CPU, against the gen's reference set (`probe-providers --providers <provider>,cpu --reps 2`: within the
    graph's tolerance - float16's for a float16 graph - and the same answers twice; tools/farm_release.py's
    cuda-selftest / dml-selftest) -> {provider: {max_abs, deterministic, rows_per_s, gpu}, 'cpu': {...}}; CampaignError
    unless both pass."""
    env = dict(os.environ, PATH=pack_dir + os.pathsep + os.environ.get('PATH', ''))   # (cuDNN loads its parts by name)
    cmd = [exe, 'probe-providers', '--runtime-dir', pack_dir, '--model', model, '--reference', reference,
           '--providers', f'{provider},cpu', '--reps', '2']
    kw = {'creationflags': 0x08000000 | 0x00004000} if os.name == 'nt' else {}
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, encoding='utf-8', errors='replace', env=env,
                           stdin=subprocess.DEVNULL, timeout=timeout, **kw)
    except subprocess.TimeoutExpired:
        raise CampaignError(f'{os.path.basename(model)}: the {provider} self-test took over {timeout} s') from None
    try:
        probe = json.loads(p.stdout)
    except ValueError:
        raise CampaignError(f'{os.path.basename(model)}: the {provider} self-test answered no JSON (exit '
                            f'{p.returncode}): {p.stderr.strip()[-800:]}') from None
    ok = {}
    for r in probe.get('results') or []:                    # (a provider passes when any adapter of it passed)
        if r.get('ok') or r.get('provider') not in ok:
            ok[r.get('provider')] = r
    if p.returncode or probe.get('providers') != [provider, 'cpu'] or not all(ok.get(x, {}).get('ok') for x in (provider, 'cpu')):
        why = {k: (v.get('ok'), v.get('why'), (v.get('selftest') or {}).get('max_abs')) for k, v in ok.items()}
        raise CampaignError(f'{os.path.basename(model)}: the self-test on {provider} and cpu did not pass (providers '
                            f'{probe.get("providers")}: {why}) {p.stderr.strip()[-600:]}')
    out = {}
    for x in (provider, 'cpu'):
        s = ok[x].get('selftest') or {}
        out[x] = {'max_abs': s.get('max_abs'), 'deterministic': s.get('deterministic'),
                  'rows_per_s': ok[x].get('rows_per_s'), 'gpu': (ok[x].get('gpu') or {}).get('vendor')}
    return out


def export_variant(form, g, gen, spec, log, cache=None):
    """Gen `gen`'s graph again in variant form `form` (zero.farm_pool.VARIANT_FORMS, the pool's per-gen variants:
    'fp16' = onnx_export.to_float16 of the gen's graph; 'matmul' / 'matmul-fp16' = its checkpoint exported again as
    graph 'fast-matmul' by zero.farm_h2h.export_cached - its weight-swap template under <cache>/templates/, checked
    against torch there -, float16 for 'matmul-fp16'), beside the gen's export (cached there: one file per checkpoint,
    spec and form) -> {form, model, sha256, model_size}. The gen's reference set serves it (zero-farm PROTOCOL 14.1)."""
    from zero import farm_h2h as FH, farm_pool as FP, games as G
    from zero.onnx_export import to_float16
    if form not in FP.VARIANT_FORMS:
        raise CampaignError(f'variant form {form!r}: one of {FP.VARIANT_FORMS}')
    d = os.path.dirname(g['model'])
    meta = os.path.join(d, f'variant-{form}.json')
    if os.path.exists(meta):
        with open(meta, encoding='utf-8') as f:
            info = json.load(f)
        if os.path.exists(info['model']) and file_sha256(info['model']) == info['sha256']:
            log(f'gen {gen}: the cached {form} variant ({info["sha256"][:12]})')
            return info
    dst = os.path.join(d, f'model-{form}.onnx')
    t0 = time.time()
    try:
        if form == 'fp16':
            to_float16(g['model'], dst)
        else:
            ex = FH.export_cached(g['checkpoint'], gen, os.path.join(d, f'v-{form}'), GAME, G.spec_json(spec),
                                  os.path.join(cache or EXPORT_CACHE, 'templates'), f'ckpt-{g["checkpoint_sha256"][:12]}',
                                  checkpoint_iter=gen, graph='fast-matmul')
            if form == 'matmul':
                import shutil
                shutil.copyfile(ex['model'], dst)
            else:
                to_float16(ex['model'], dst)
    except (SystemExit, ValueError, RuntimeError, OSError) as e:
        raise CampaignError(f'gen {gen}: its {form} variant could not be exported: {e}') from None
    info = {'form': form, 'model': dst, 'sha256': file_sha256(dst), 'model_size': os.path.getsize(dst),
            'export_s': round(time.time() - t0, 1)}
    with open(meta, 'w', encoding='utf-8') as f:
        json.dump(info, f, indent=1)
    log(f'gen {gen}: its {form} variant exported in {info["export_s"]} s ({info["sha256"][:12]})')
    return info


def _selftest_cached(g, key, run):
    """A self-test result kept beside the gen's export by (graph sha256, alternate build, pack): run once."""
    p = os.path.join(os.path.dirname(g['model']), 'selftests.json')
    have = {}
    if os.path.exists(p):
        with open(p, encoding='utf-8') as f:
            have = json.load(f)
    if key in have:
        return have[key], True
    have[key] = run()
    tmp = f'{p}.{os.getpid()}.tmp'
    with open(tmp, 'w', encoding='utf-8') as f:
        json.dump(have, f, indent=1)
    os.replace(tmp, p)
    return have[key], False


def platform_check(st, d, platform, log):
    """A platform alternate (a macOS build) plays the games only if they replay exactly there: the default build's
    worker judges the job's settings as a `platform` worker would (`check-config --platform`, with `--eval` for an
    eval job: zero.eval_pool's check, zero-core docs/az.md 62) - CampaignError on a libm refusal or a build too old to
    judge."""
    from zero.farm_h2h import worker_json
    path = os.path.join(d, f'settings-{platform}.json')
    with open(path, 'w', encoding='utf-8') as f:
        json.dump(st['settings'], f)
    args = ['check-config', path, '--json'] + ([] if samples_job(st) else ['--eval']) + ['--platform', platform]
    rc, out, text = worker_json(st['worker'], *args)
    if not isinstance(out, dict) or out.get('platform') != platform:
        raise CampaignError(f'{st["worker"]} cannot judge the settings for {platform} (a build before zero-core docs/az.md '
                            f'62): no {platform} alternate without that check ({text.strip()[-300:]})')
    if out.get('libm_refusals'):
        raise CampaignError(f'the games would replay differently on {platform} (its libm, zero-core docs/az.md 62): '
                            f'{"; ".join(out["libm_refusals"])} - leave its alternate out')
    log(f'{platform}: the games replay exactly there (check-config --platform {platform})')


def prepare_alternates(cl, d, st, log, cache=None):
    """The campaign's alternates from `alt_want` (alternates_want), once, before anything is put: the ids (the release
    index's for the build, or the caller's) checked as a run's (zero.farm_def.run_alternates: worker builds of the
    build's game, each requiring its pack; `when.vendor` from a GPU pack, `when.platform` for another platform's build)
    and each the default build's engine (its manifest's engine_commit); a platform alternate's settings checked to
    replay there (platform_check); each GPU alternate named its variant (zero.farm_pool.variant_of: its vendor's, else
    the first without one; none: the gen's default graph), each named variant exported per gen (export_variant), and
    every graph a GPU alternate will play self-tested on this machine's GPU through that alternate's own worker and pack
    (gpu_selftest). min_agent: at least farm_def.VARIANT_AGENT when a variant is named (zero-farm PROTOCOL 14.1); each
    alternate's own agent minimum stays its build's (the coordinator holds a route from older agents per machine,
    PROTOCOL 9.4 condition 6 - a run-wide minimum would keep them from every route). Any failure: CampaignError, the
    reason in the state (`alternates_error`), nothing put; none at all: said in the log and the state
    (`alternates_note`)."""
    from zero import farm_def as FD, farm_pool as FP
    want = st['alt_want']
    notes = []
    try:
        if want['alternates'] == RELEASE:
            ids, notes = release_alternates(st['build_id'])
        else:
            ids = list(want['alternates'])
            if not ids:
                notes = ['no alternates asked for (--no-alternates): CPU only']
        for n in notes:
            log(f'alternates: {n}')
        if not ids:
            st.update(alternates=[], builds=[st['build_id']], alt_routes={}, variants={},
                      alternates_note='; '.join(notes), alternates_ready=True)
            st.pop('alternates_error', None)
            save_state(d, st)
            log('alternates: none - the campaign plays on the build\'s CPU worker only')
            return
        try:
            alternates, builds = FD.run_alternates(cl, st['build_id'], st['runtime'], ids)
        except SystemExit as e:
            raise CampaignError(f'alternates {ids}: {e}') from None
        base = FD.build_manifest(cl, st['build_id'])
        mans = {x['build_id']: FD.build_manifest(cl, x['build_id']) for x in alternates}
        for b, m in mans.items():
            if not base.get('engine_commit') or m.get('engine_commit') != base['engine_commit']:
                raise CampaignError(f'alternate {b} is the engine of commit {str(m.get("engine_commit"))[:12]}, the build '
                                    f'{st["build_id"]} of {str(base.get("engine_commit"))[:12]}: not the same games')
        for p in sorted({x['when'].get('platform') for x in alternates} - {None}):
            platform_check(st, d, p, log)
        variants = FP.variant_setting(want['variants'])
        for x in alternates:
            if 'vendor' in x['when']:
                n = FP.variant_of(variants, x['when']['vendor'])
                if n:
                    x['model'] = n
        used = {x['model'] for x in alternates if x.get('model')}
        forms = {n: f for n, f, _ in variants if n in used}
        for n, _, _ in variants:
            if n not in used:
                log(f'variants: {n} is named by no listed GPU alternate: not exported')
        for gen, g in sorted(st['gens'].items()):
            vs = {}
            for n, f in sorted(forms.items()):
                vs[n] = dict(export_variant(f, g, int(gen), st['spec'], log, cache), name=n)
            g['variants'] = vs or None
            save_state(d, st)
        selftests = {}
        for x in alternates:
            if 'vendor' not in x['when']:
                continue
            exe_dir, man = local_build(cl, x['build_id'], 'worker')
            pack_dir, pack = local_build(cl, x['runtime'], 'runtime')
            provs = ((pack.get('gpu') or {}).get('providers') or [])
            if len(provs) != 1:
                raise CampaignError(f'alternate {x["build_id"]}: its pack {x["runtime"]} names GPU providers {provs}, '
                                    f'not one: which one to self-test is not clear')
            exe = os.path.join(exe_dir, man['entry'])
            for gen, g in sorted(st['gens'].items()):
                gm = (g.get('variants') or {}).get(x.get('model')) or g
                what = f'gen {gen} {x.get("model") or "default graph"} on {provs[0]} ({x["build_id"]})'
                key = f'{gm["sha256"]}|{x["build_id"]}|{x["runtime"]}'
                res, cached = _selftest_cached(g, key, lambda: gpu_selftest(exe, pack_dir, provs[0], gm['model'], g['reference']))
                selftests.setdefault(gen, {})[x['build_id']] = dict(res, model=x.get('model'), sha256=gm['sha256'])
                log(f'self-test {what}: {provs[0]} max abs {res[provs[0]]["max_abs"]:.3g}, cpu '
                    f'{res["cpu"]["max_abs"]:.3g}, deterministic{" (cached)" if cached else ""}')
        need = FD.VARIANT_AGENT if used else None
        if need and FP.version_tuple(st.get('min_agent')) < FP.version_tuple(need):
            log(f'min_agent {st.get("min_agent") or "none"} -> {need} (a definition that signs a variant)')
            st['min_agent'] = need
        st.update(alternates=alternates, builds=builds, variants=forms, selftests=selftests,
                  alt_routes={b: {'when': next(x['when'] for x in alternates if x['build_id'] == b),
                                  'min_agent': m.get('min_agent'), 'version': m.get('version')} for b, m in mans.items()},
                  alternates_note='; '.join(notes) or None, alternates_ready=True)
        st.pop('alternates_error', None)
        save_state(d, st)
        log('alternates: ' + ', '.join(f'{x["build_id"]} ({" ".join(str(v) for v in x["when"].values())}'
                                         f'{", variant " + x["model"] if x.get("model") else ""}, agents from '
                                         f'{st["alt_routes"][x["build_id"]]["min_agent"]})' for x in alternates)
            + f'; run min_agent {st.get("min_agent") or "none"}')
    except CampaignError as e:
        st['alternates_error'] = str(e)
        save_state(d, st)
        raise


# ------------------------------------------------------------------------------------------------ the farm's side
def samples_job(st):
    return bool((st.get('job') or {}).get('samples'))


def eval_template(st):
    """The definition's template - also the job without its seed (PROTOCOL.md 7): eval {settings, seats, paired};
    with samples (self-play) {settings}."""
    if samples_job(st):
        return {'settings': st['settings']}
    return {'settings': st['settings'], 'seats': st['seats'], 'paired': st['paired']}


def run_body(st, eval_on=True, state=None):
    b = {'game': st['build_game'], 'build_id': st['build_id'], 'runtime': st['runtime'], 'priority': st['priority']}
    if st.get('min_agent'):
        b['min_agent'] = st['min_agent']
    if st.get('alternates'):                    # (PROTOCOL 9.4, 11.4, 14.3; none - a campaign before them: no key)
        b['alternates'] = st['alternates']
    b.update(selfplay=None, eval=None, state=state or st.get('run_state', 'active'))
    if eval_on and samples_job(st):
        # (the ids [0, games): the quota with no slack - the coordinator never issues more than accepted + in flight
        # allows, so ids stay below `games`; checkpoints never change, so max_stale is moot)
        sp = {'quota': st['games'], 'slack': 0, 'max_stale': 1000, 'games_per_lease': st['games_per_lease'],
              'gpu': 'any', 'needs': st.get('needs'), 'job': dict(eval_template(st), seed=int(st['seed']))}
        if st.get('share') is not None:
            sp['share'] = st['share']
        b['selfplay'] = sp
    elif eval_on:
        job = dict(eval_template(st), seed=int(st['seed']))
        ev = {'batch': st['batch'], 'games': st['games'], 'checkpoints': sorted(int(g) for g in st['gens']),
              'games_per_lease': st['games_per_lease'], 'gpu': 'any', 'needs': st.get('needs'), 'job': job}
        if st.get('share') is not None:
            ev['share'] = st['share']
        b['eval'] = ev
    return b


def definition_fields(st):
    cks = {g: {'model_sha256': x['sha256'], 'model_size': x['model_size'], 'reference_sha256': x['reference_sha256'],
               'reference_size': x['reference_size']} for g, x in sorted(st['gens'].items())}
    for g, x in st['gens'].items():             # (PROTOCOL 14.1: each gen's variants; none: no key)
        if x.get('variants'):
            cks[g]['variants'] = {n: {'model_sha256': v['sha256'], 'model_size': v['model_size']}
                                  for n, v in sorted(x['variants'].items())}
    # (the alternates' builds after the default one: a definition authorizes an alternate by listing it, PROTOCOL 7)
    return {'game': st['build_game'], 'builds': st.get('builds') or [st['build_id']], 'runtime': st['runtime'], 'checkpoints': cks,
            'jobs': {'selfplay': eval_template(st), 'eval': None, 'fuzz': None} if samples_job(st) else
                    {'selfplay': None, 'eval': eval_template(st), 'fuzz': None}}


def setup_farm(cl, d, st, log, opts):
    """The coordinator's side, each step once (state flags): the run (no batch), its signed definition, the gens'
    files and registrations, then the batch (its job = the template + the base seed)."""
    from zero import farm_def as FD
    run = st['run']
    if not st.get('build_game'):
        st['build_game'] = FD.worker_game(cl, st['build_id'], st['runtime'])
        save_state(d, st)
    if not st.get('run_put'):
        cl.call('PUT', f'/v1/admin/run/{run}', body=run_body(st, eval_on=False))
        st['run_put'] = True
        save_state(d, st)
        alts = ''.join(f', alternate {x["build_id"]} ({" ".join(str(v) for v in x["when"].values())}'
                       f'{", variant " + x["model"] if x.get("model") else ""})' for x in st.get('alternates') or [])
        log(f'farm run {run} created (worker build {st["build_id"]}, runtime {st["runtime"]}{alts}, min_agent '
            f'{st.get("min_agent") or "none"})')
    info = st.get('definition')
    if not (info and info.get('published')):
        if not info:
            info = FD.prepare(cl, 'run', run, expires_days=opts.get('expires_days', EXPIRES_DAYS),
                              root=opts.get('defs_dir'), **definition_fields(st))
            st['definition'] = info
            save_state(d, st)
        st['definition'] = FD.publish(cl, info, key=opts.get('key'), farm_release=opts.get('farm_release'), log=log)
        save_state(d, st)
    for gen, g in sorted(st['gens'].items()):
        if g.get('registered'):
            continue
        keys = {'model': f'ckpt/{run}/{gen}/model.onnx', 'reference': f'ckpt/{run}/{gen}/reference.npz'}
        cl.put_artifact(keys['model'], g['model'], g['sha256'])
        cl.put_artifact(keys['reference'], g['reference'], g['reference_sha256'])
        body = {'model': keys['model'], 'sha256': g['sha256'], 'reference': keys['reference'],
                'reference_sha256': g['reference_sha256']}
        if g.get('variants'):                   # (PROTOCOL 14.2: each variant uploaded, then registered with the gen)
            body['variants'] = {}
            for n, v in sorted(g['variants'].items()):
                k = f'ckpt/{run}/{gen}/model-{n}.onnx'
                cl.put_artifact(k, v['model'], v['sha256'])
                body['variants'][n] = {'model': k, 'sha256': v['sha256']}
        cl.call('PUT', f'/v1/admin/run/{run}/checkpoint/{gen}', body=body)
        g['registered'] = True
        save_state(d, st)
        log(f'gen {gen} uploaded and registered ({os.path.basename(g["checkpoint"])}, {g["sha256"][:12]}'
            + (f'; variants {", ".join(f"{n} {v["sha256"][:12]}" for n, v in sorted(g["variants"].items()))}'
               if g.get('variants') else '') + ')')
    if not st.get('eval_put'):
        cl.call('PUT', f'/v1/admin/run/{run}', body=run_body(st))
        st['eval_put'] = True
        st['submitted'] = time.time()
        save_state(d, st)
        log(f'batch {st["batch"]}: {st["games"]} games (share {st.get("share")}, priority {st["priority"]})')


def submit(name, a, games, settings, seats, seed, build_id, worker, runtime, b=None, paired=True, share=DEFAULT_SHARE,
           priority=0.5, games_per_lease='auto', deadline_s=DEFAULT_DEADLINE_S, spot=DEFAULT_SPOT, ort_dll=None,
           min_agent=None, needs=None, meta=None, client=None, root=None, log=log_stderr, opts=None, samples=False,
           alternates=RELEASE, variants=DEFAULT_VARIANTS):
    """A new campaign (or the existing one of that name, resumed where it stopped) -> its state directory. `settings`:
    a run's evaluation settings (match_settings); `seats` [{"net": 1}, {"net": 2}] (A vs B, `b` given) or
    [{"net": 1}, {"net": 1}] (self-play of A); `seed`: the batch's base seed; `worker`: the build's executable
    (`ort_dll`: an ONNX Runtime the local fallback loads; default the runtime pack next to the build); `meta`: the
    caller's notes (kept in the state). `samples`: a play job with the search's samples (self-play seats of
    `settings` = selfplay_settings; seats [{"net": 1}, {"net": 1}], unpaired). `alternates` / `variants`: the farm's
    other routes and the graphs their GPUs play (module docstring; prepare_alternates): RELEASE (the default) the
    release index's for the build, None none, or a list of build ids; `variants` a pool_variants text ('' none: the GPU
    alternates play each gen's default graph). A resumed campaign keeps what it was created with."""
    run = slug(name)
    d = state_dir(run, root)
    opts = opts or {}
    if os.path.exists(os.path.join(d, 'state.json')):
        st = load_state(d)
        log(f'campaign {run}: resuming ({len(st.get("have", []))}/{st["games"]} games in)')
    else:
        if seats not in ([{'net': 1}, {'net': 2}], [{'net': 1}, {'net': 1}]):
            raise CampaignError(f'seats {seats}: [{{"net": 1}}, {{"net": 2}}] or [{{"net": 1}}, {{"net": 1}}]')
        if (b is None) != (seats[1] == {'net': 1}):
            raise CampaignError('a second network b goes with seats [{"net": 1}, {"net": 2}], none with self-play')
        if samples and (b is not None or paired):
            raise CampaignError('a play job with samples is self-play of one network, unpaired')
        if games < 1 or (paired and games % 2):
            raise CampaignError(f'games {games}: >= 1 (and even when paired)')
        if not (share is None or 0 < float(share) <= 1):
            raise CampaignError(f'share {share}: in (0, 1]')
        want = alternates_want(alternates, variants)
        info = worker_info(worker)
        if info.get('build_id') != build_id or 'eval' not in (info.get('kinds') or []):
            raise CampaignError(f'{worker} is build {info.get("build_id")} ({info.get("kinds")}), not the eval worker '
                                f'build {build_id}')
        st = {'run': run, 'name': name, 'game': GAME, 'a': os.path.abspath(a), 'b': os.path.abspath(b) if b else None,
              'seats': seats, 'paired': bool(paired), 'seed': int(seed), 'games': int(games), 'batch': 'c1',
              'games_per_lease': games_per_lease, 'share': None if share is None else float(share),
              'priority': float(priority), 'build_id': build_id, 'runtime': runtime, 'worker': os.path.abspath(worker),
              'ort_dll': ort_dll, 'min_agent': min_agent, 'needs': needs, 'deadline_s': float(deadline_s),
              'spot': float(spot), 'created': time.time(), 'meta': meta or {}, 'have': [], 'failed': {},
              'machines': {}, 'leases': {}, 'cursor': 0, 'job': {'kind': 'play', 'trace': True, 'samples': bool(samples)},
              'alt_want': want}
        os.makedirs(d, exist_ok=True)
        st['settings'] = job_settings(worker, build_id, settings, d, log, samples)
        st['spec'] = st['settings']['spec' if samples else 'eval_spec']
        save_state(d, st)
    return _setup(d, st, client, opts, log)


def resume(d, client=None, opts=None, log=log_stderr):
    """An existing campaign's setup finished where it stopped (exports, the farm's run, definition, gens, batch)."""
    return _setup(d, load_state(d), client, opts or {}, log)


def _setup(d, st, client, opts, log):
    from zero import farm_def as FD
    if not st.get('gens'):
        gens = {'1': export_gen(st['a'], 1, GAME, st['spec'], st['run'], log, opts.get('export_cache'))}
        if st['b']:
            gens['2'] = export_gen(st['b'], 2, GAME, st['spec'], st['run'], log, opts.get('export_cache'))
        st['gens'] = gens
        save_state(d, st)
    cl = client or FD.Client(opts.get('url', FD.URL))
    if st.get('alt_want') is not None and not st.get('alternates_ready'):
        # (a campaign made before alternates has no alt_want: every request exactly as before)
        prepare_alternates(cl, d, st, log, opts.get('export_cache'))
    setup_farm(cl, d, st, log, opts)
    return d


# ------------------------------------------------------------------------------------------------ results
def _result_file(cl, d, key, sha=None):
    p = os.path.join(d, 'results', hashlib.sha256(key.encode()).hexdigest()[:24] + '.dat')
    if os.path.exists(p):
        with open(p, 'rb') as f:
            body = f.read()
    else:
        body = cl.call('GET', f'/v1/admin/artifact/{key}', raw=True)
        os.makedirs(os.path.dirname(p), exist_ok=True)
        tmp = p + f'.{os.getpid()}.tmp'
        with open(tmp, 'wb') as f:
            f.write(body)
        os.replace(tmp, p)
    if sha and hashlib.sha256(body).hexdigest() != sha:
        raise CampaignError(f'result {key}: sha256 mismatch')
    return body if body[:2] == bytes([0x1f, 0x8b]) else body.decode('utf-8')


def _append_rows(d, rows):
    if rows:
        with open(os.path.join(d, 'rows.jsonl'), 'a', encoding='utf-8', newline='\n') as f:
            for r in rows:
                f.write(json.dumps(r, separators=(',', ':')) + '\n')


def _refuse(d, st, why):
    st['refused'] = why
    save_state(d, st)
    raise CampaignError(f'campaign {st["run"]} refused: {why}')


def pull(cl, d, st, log=log_stderr):
    """New result files: downloaded (sha256 checked), credited rows taken (the first copy of a game wins; zero.farm_h2h
    .pull's reading of `ids`, `dup_ids` and `also`), each row's digest checked against its record (a mismatch refuses
    the campaign) and tagged with its machine and lease -> games taken."""
    from zero import farm_eval as FE
    have = set(st.get('have', []))
    taken = 0
    while True:
        ans = cl.call('GET', f'/v1/admin/run/{st["run"]}/results?after={st.get("cursor", 0)}&limit=500')
        res = ans.get('results') or []
        for r in res:
            if samples_job(st):
                if r.get('kind') != 'selfplay':
                    continue
            elif r.get('kind') != 'eval' or r.get('batch') not in (None, st['batch']):
                continue
            dup = ids_of(r.get('dup_ids'))
            parts = [(_result_file(cl, d, r['key'], r.get('sha256')), ids_of(r.get('ids')))]
            for x in r.get('also') or []:
                parts.append((_result_file(cl, d, x['key']), ids_of(x.get('ids'))))
            new = []
            for text, ids in parts:
                for row in (records_rows(st, text) if samples_job(st) else FE.read_rows(text)):
                    g = int(row['gid'])
                    if g in dup or g in have or g not in ids:
                        continue
                    try:
                        FE.check_digest(row)
                    except FE.TraceMismatch as e:
                        _refuse(d, st, f'machine {r.get("machine_id")}, lease {r.get("lease_id")}: {e}')
                    if g >= st['games']:
                        _refuse(d, st, f'game {g} is outside the campaign\'s ids [0, {st["games"]})')
                    row['by'] = {'machine': r.get('machine_id') or '?', 'lease': r.get('lease_id')}
                    have.add(g)
                    new.append(row)
            _append_rows(d, new)
            if new:
                m = st['machines'].setdefault(r.get('machine_id') or '?', 0)
                st['machines'][r.get('machine_id') or '?'] = m + len(new)
                lz = st['leases'].setdefault(r.get('lease_id') or '?', {'machine': r.get('machine_id'), 'games': 0})
                lz['games'] += len(new)
            taken += len(new)
        st['have'] = sorted(have)
        if res:
            st['cursor'] = ans.get('cursor', st.get('cursor', 0))
            save_state(d, st)
            if samples_job(st):
                # (the self-play quota: accepted + in flight; the total, PROTOCOL.md 4 - before the ack)
                cl.call('POST', f'/v1/admin/run/{st["run"]}/accept', body={'gen': 1, 'games': len(have)})
            cl.call('POST', f'/v1/admin/run/{st["run"]}/ack', body={'cursor': st['cursor']})
        if len(res) < 500:
            save_state(d, st)
            return taken


def records_rows(st, body):
    """A play job's records file with samples (the farm's self-play records, gzip'd JSON lines: a header, then one
    game record per line; PROTOCOL.md rev 8) -> rows in the campaign's schema (gid, seats, result, rounds, over,
    digest, rec; `aux` from the header, which the replay needs)."""
    from zero import farm_eval as FE
    if isinstance(body, str):
        text = body
    else:
        try:
            text = gzip.decompress(body).decode('utf-8')
        except OSError:
            text = body.decode('utf-8')
    lines = [ln for ln in text.splitlines() if ln.strip()]
    if not lines:
        return []
    head = json.loads(lines[0])
    if head.get('records') != 1:
        raise CampaignError(f'a records file without its header (records 1): {lines[0][:200]}')
    if head.get('spec') not in (None, st['spec']):
        raise CampaignError(f'a records file of another spec: {str(head.get("spec"))[:200]}')
    out = []
    for ln in lines[1:]:
        rec = json.loads(ln)
        res = [_f32(b) for b in rec.get('result') or []]
        out.append({'gid': int(rec['gid']), 'pair': None, 'seats': st['seats'], 'result': res,
                    'rounds': int(rec.get('rounds', 0)), 'over': bool(rec.get('over')), 'aux': bool(head.get('aux')),
                    'digest': FE.trace_digest(rec), 'rec': rec})
    return out


def rows_of(d):
    """Every taken row by gid (the first copy wins)."""
    out = {}
    p = os.path.join(d, 'rows.jsonl')
    if os.path.exists(p):
        with open(p, encoding='utf-8') as f:
            for line in f:
                if line.strip():
                    r = json.loads(line)
                    out.setdefault(int(r['gid']), r)
    return out


def missing(st):
    """Game ids neither done nor failed."""
    have = set(st.get('have', []))
    failed = {int(g) for g in st.get('failed', {})}
    return [g for g in range(st['games']) if g not in have and g not in failed]


def coverage(st):
    """Positive coverage: every id of [0, games) in exactly one of done (farm / local) or failed (with its reason)
    or missing -> {games, done, farm, local, failed: [[lo, hi), reason], missing: ranges}."""
    have = set(st.get('have', []))
    local = ids_of(st.get('local_ranges'))
    fails = {}
    for g, why in sorted(st.get('failed', {}).items(), key=lambda x: int(x[0])):
        fails.setdefault(why, []).append(int(g))
    return {'games': st['games'], 'done': len(have), 'farm': len(have - local), 'local': len(have & local),
            'failed': [[rs, why] for why, gs in fails.items() for rs in ranges_of(gs)],
            'missing': ranges_of(missing(st))}


def farm_state(cl, st):
    try:
        es = cl.call('GET', f'/v1/admin/run/{st["run"]}').get('eval_state')
    except Exception:                            # noqa: BLE001 - a status line without the farm's view
        return None
    return es if isinstance(es, dict) and es.get('batch') in (None, st['batch']) else None


def status(d, client=None, opts=None):
    """The campaign's state: coverage, the farm's batch progress, machines, deadline -> dict."""
    from zero import farm_def as FD
    st = load_state(d)
    cl = client or FD.Client((opts or {}).get('url', FD.URL))
    out = {'run': st['run'], 'coverage': coverage(st), 'farm': farm_state(cl, st), 'machines': st.get('machines'),
           'deadline': (st.get('submitted') or st['created']) + st['deadline_s'], 'refused': st.get('refused'),
           'alternates': st.get('alternates'), 'alternates_note': st.get('alternates_note'),
           'alternates_error': st.get('alternates_error'), 'min_agent': st.get('min_agent'), 'result': st.get('result')}
    return out


# ------------------------------------------------------------------------------------------------ the local fallback
def runtime_dll(st):
    """The ONNX Runtime the local worker loads: `ort_dll`, else the runtime pack's next to the build
    (<builds>/<runtime>/onnxruntime.dll), else the desktop's copy of the registered pack ($ZERO_BIG_DISK/zero-farm-builds/<id>/)."""
    if st.get('ort_dll'):
        return st['ort_dll']
    looked = []
    builds = local_config.big_disk_path('zero-farm-builds')
    for base in [os.path.dirname(os.path.dirname(st['worker']))] + ([builds] if builds else []):
        p = os.path.join(base, st['runtime'] or '', 'onnxruntime.dll')
        looked.append(p)
        if st['runtime'] and os.path.exists(p):
            return p
    raise CampaignError(f'no ONNX Runtime for the local worker: give ort_dll (looked at {looked})')


def local_cmd(st, lo, hi, out, threads):
    if samples_job(st):
        x = st['gens']['1']
        return [st['worker'], 'selfplay', '--settings', os.path.join(out, 'settings.json'), '--model', x['model'],
                '--reference', x['reference'], '--sha256', x['sha256'], '--net', '1', '--games', str(hi - lo),
                '--gid-base', str(lo), '--seed', str(st['seed']), '--threads', str(threads), '--out', out,
                '--prefix', f'l{lo}', '--ort-dll', runtime_dll(st), '--output', 'records']
    cmd = [st['worker'], 'eval', '--settings', os.path.join(out, 'settings.json'), '--seats', json.dumps(st['seats']),
           '--games', str(hi - lo), '--gid-base', str(lo), '--seed', str(st['seed']), '--threads', str(threads),
           '--out', out, '--prefix', f'l{lo}', '--ort-dll', runtime_dll(st), '--lease', f'local-{lo}',
           '--flush-games', '64']
    if st['paired']:
        cmd.append('--paired')
    for g, x in sorted(st['gens'].items()):
        cmd += ['--model', f'{g}={x["model"]}', '--reference', f'{g}={x["reference"]}', '--sha256', f'{g}={x["sha256"]}']
    return cmd


def local_ranges(gids, paired, chunk=64):
    """Missing ids -> contiguous ranges cut into pieces of at most `chunk` (whole pairs when paired)."""
    out = []
    for lo, hi in ranges_of(gids):
        if paired:
            lo, hi = lo - lo % 2, hi + hi % 2
        while lo < hi:
            step = min(chunk, hi - lo)
            out.append([lo, lo + step])
            lo += step
    return out


def play_local(d, st, gids, threads=4, wrap=None, log=log_stderr):
    """Games `gids` played here by the campaign's own worker executable on its graphs (`mtgz-worker eval`: the same
    games the farm would have played), below normal priority, `wrap` (a command prefix, e.g. the desktop's slot
    throttle) around each piece. Each piece's rows are checked as the farm's (digest) and taken; a piece that fails
    marks its games failed with the worker's error (the reason)."""
    from zero import farm_eval as FE
    os.makedirs(os.path.join(d, 'local'), exist_ok=True)
    have = set(st.get('have', []))
    for lo, hi in local_ranges(gids, st['paired'], 16 if samples_job(st) else 64):
        out = os.path.join(d, 'local', f'{lo}-{hi}')
        os.makedirs(out, exist_ok=True)
        with open(os.path.join(out, 'settings.json'), 'w', encoding='utf-8') as f:
            json.dump(st['settings'], f)
        cmd = list(wrap or []) + local_cmd(st, lo, hi, out, threads)
        kw = {'creationflags': 0x08000000 | 0x00004000} if os.name == 'nt' else {}
        log(f'local: games [{lo}, {hi}) ({threads} threads)')
        t0 = time.time()
        with open(os.path.join(out, 'events.jsonl'), 'w', encoding='utf-8') as fo, \
                open(os.path.join(out, 'worker.log'), 'w', encoding='utf-8') as fe:
            p = subprocess.run(cmd, stdout=fo, stderr=fe, stdin=subprocess.DEVNULL, **kw)
        err = None
        evs = []
        with open(os.path.join(out, 'events.jsonl'), encoding='utf-8', errors='replace') as f:
            for line in f:
                try:
                    evs.append(json.loads(line))
                except ValueError:
                    pass
        errs = [e.get('message') for e in evs if e.get('ev') == 'error' and e.get('fatal')]
        if p.returncode != 0 or errs:
            with open(os.path.join(out, 'worker.log'), encoding='utf-8', errors='replace') as f:
                tail = f.read()[-600:]
            err = (errs[-1] if errs else f'exit {p.returncode}') + (f' ({tail.strip()})' if tail.strip() else '')
        new = []
        for e in evs:
            if e.get('ev') not in ('evals', 'records'):
                continue
            with open(os.path.join(out, e['path']), 'rb') as f:
                body = f.read()
            for row in (records_rows(st, body) if e['ev'] == 'records' else FE.read_rows(body.decode('utf-8'))):
                g = int(row['gid'])
                if g in have or not (lo <= g < hi):
                    continue
                try:
                    FE.check_digest(row)
                except FE.TraceMismatch as x:
                    _refuse(d, st, f'the local worker: {x}')
                row['by'] = {'machine': 'local', 'lease': f'local-{lo}'}
                have.add(g)
                new.append(row)
        _append_rows(d, new)
        st['have'] = sorted(have)
        st.setdefault('local_ranges', []).extend(ranges_of([r['gid'] for r in new]))
        st['machines']['local'] = st['machines'].get('local', 0) + len(new)
        for g in range(lo, hi):
            if g not in have:
                st['failed'][str(g)] = f'local worker: {err or "the game did not finish"}'
        save_state(d, st)
        log(f'local: [{lo}, {hi}) done in {time.time() - t0:.0f} s: {len(new)} games' + (f'; FAILED: {err}' if err else ''))


# ------------------------------------------------------------------------------------------------ verification
def spot_sample(st, gids, frac):
    """The games the spot check replays: ceil(frac x n) of `gids` (at least 1 when there are any), a random choice
    seeded by the campaign's run name (the same choice whenever it is made)."""
    gids = sorted(gids)
    if not gids or frac <= 0:
        return []
    k = min(len(gids), max(1, math.ceil(frac * len(gids))))
    rng = random.Random(int(hashlib.sha256(st['run'].encode()).hexdigest()[:16], 16))
    return sorted(rng.sample(gids, k))


def spot_check(d, st, frac=None, engine=None, log=log_stderr):
    """`frac` (the campaign's `spot`, 5%) of its games replayed through the engine from seed + moves
    (zero.farm_eval.replay_trace); any mismatch refuses the campaign -> {checked, ok, gids, by_machine}."""
    from zero import farm_eval as FE
    if engine is None:
        import mtgz.az  # noqa: F401  (registers the game)
        from mtgz import _engine as engine
    rows = rows_of(d)
    pick = spot_sample(st, list(rows), st['spot'] if frac is None else frac)
    by = {}
    t0 = time.time()
    for g in pick:
        r = rows[g]
        try:
            FE.replay_trace(r, GAME, st['spec'], engine, st['seed'], st['paired'], samples=samples_job(st),
                            aux=bool(r.get('aux')))
        except FE.TraceMismatch as e:
            _refuse(d, st, f'spot check: machine {r.get("by", {}).get("machine")}: {e}')
        m = r.get('by', {}).get('machine', '?')
        by[m] = by.get(m, 0) + 1
    out = {'checked': len(pick), 'ok': len(pick), 'of': len(rows), 'gids': pick, 'by_machine': by,
           'seconds': round(time.time() - t0, 1)}
    log(f'spot check: {len(pick)} of {len(rows)} games replayed from seed + moves, all equal ({out["seconds"]} s)')
    return out


# ------------------------------------------------------------------------------------------------ per-game records
def _f32(bits):
    import struct
    return struct.unpack('<f', struct.pack('<I', int(bits) & 0xffffffff))[0]


def describe(row, spec, engine=None, states=False):
    """A traced row replayed into its per-game record (for analyses): {gid, seats (the job's, this game's order),
    result, rounds, over, digest, by, episodes: [{moves: [{i, seat, kind, a, t}], result}]} - each real move with the
    seat that decided it, the decision's kind and the option in words (the engine's spectator view of the state
    before the move; `t` None for an option the view doesn't word), a best-of-three's games one episode each.
    `states`: each move also keeps the whole view (`state`; large)."""
    if engine is None:
        import mtgz.az  # noqa: F401
        from mtgz import _engine as engine
    rec = row['rec']
    g = engine.Game(GAME, spec, int(rec['n']), int(rec['seed']))
    eps = [{'moves': []}]
    for i, s in enumerate(rec['steps']):
        if isinstance(s, dict):                      # a chained contest's episode boundary
            res = [float(x) for x in g.result()] if g.over and not g.aborted else None
            eps[-1]['result'] = res
            nxt, contest = g.next_episode(s.get('next'))
            if nxt is None:
                raise CampaignError(f'game {row["gid"]}: the contest ended at step {i} but the record goes on')
            g = nxt
            eps.append({'moves': []})
            continue
        if isinstance(s, str):                       # an external seat's answer
            eps[-1]['moves'].append({'i': i, 'seat': g.to_move, 'kind': 'External', 'a': None, 't': s})
            g.apply_external(s)
            continue
        st = json.loads(g.export_state())
        dec = st.get('decision') or {}
        text = next((o.get('t') for o in dec.get('options') or [] if o.get('a') == s), None)
        mv = {'i': i, 'seat': g.to_move, 'kind': dec.get('kind'), 'a': int(s), 't': text}
        if states:
            mv['state'] = st
        eps[-1]['moves'].append(mv)
        g.step(int(s))
    eps[-1]['result'] = [float(x) for x in g.result()] if g.over and not g.aborted else None
    return {'gid': row['gid'], 'seats': row['seats'], 'result': row['result'], 'rounds': row['rounds'],
            'over': row['over'], 'digest': row['digest'], 'by': row.get('by'), 'seed': rec['seed'],
            'record_result': [_f32(b) for b in rec.get('result') or []], 'episodes': eps}


def write_records(d, st, log=log_stderr, engine=None):
    """records.jsonl.gz: every game's `describe` record, in gid order."""
    rows = rows_of(d)
    p = os.path.join(d, 'records.jsonl.gz')
    tmp = p + f'.{os.getpid()}.tmp'
    t0 = time.time()
    with gzip.open(tmp, 'wt', encoding='utf-8', newline='\n') as f:
        for g in sorted(rows):
            f.write(json.dumps(describe(rows[g], st['spec'], engine), separators=(',', ':')) + '\n')
    os.replace(tmp, p)
    log(f'records: {len(rows)} games described in {time.time() - t0:.0f} s -> {p}')
    return p


def read_records(d):
    """The campaign's per-game records (records.jsonl.gz), in gid order."""
    with gzip.open(os.path.join(d, 'records.jsonl.gz'), 'rt', encoding='utf-8') as f:
        return [json.loads(line) for line in f if line.strip()]


# ------------------------------------------------------------------------------------------------ collect
def readout(d, st):
    """A vs B (seats [{"net": 1}, {"net": 2}]): zero.h2h's summary of the games (zero.farm_eval.summary, A = gen 1) -
    the fields a local zero.h2h.match prints; self-play: None."""
    from zero import farm_eval as FE
    if st['seats'][1] != {'net': 2}:
        return None
    rows = [{k: v for k, v in r.items() if k not in ('rec', 'by', 'digest', 'aux')} for r in rows_of(d).values()]
    return FE.summary(rows, st['paired'], {'net': 1}) if rows else None


def stop_farm(cl, st, log):
    """The campaign's run paused on the farm (its leases cancelled at their next beat): nothing more is played."""
    try:
        cl.call('PUT', f'/v1/admin/run/{st["run"]}', body=run_body(st, state='paused'))
        st['run_state'] = 'paused'
        log(f'farm run {st["run"]} paused')
    except Exception as e:                        # noqa: BLE001 - the campaign's result does not depend on it
        log(f'note: pausing farm run {st["run"]} failed: {e}')


def collect(d, client=None, poll=60, local=True, threads=4, wrap=None, describe_games=True, allow_failed=False,
            wait=True, log=log_stderr, engine=None, opts=None, deadline_s=None):
    """Pull until every game is in or the deadline passes; then the missing games locally (`local`; else
    CampaignError naming them); the farm run paused; every row's digest checked, the spot check, coverage (failed games
    refuse unless `allow_failed`); games.jsonl, the games' digest, the readout and (with `describe_games`) the per-game
    records -> the result dict (also in state['result']). `wait` False: one pull, then return None if games are still
    out and the deadline has not passed. `deadline_s`: a new deadline (seconds after the submit; kept in the state):
    0 = now, e.g. to stop waiting on a farm whose machines went quiet."""
    from zero import farm_def as FD
    from zero import farm_eval as FE
    st = load_state(d)
    if st.get('refused'):
        raise CampaignError(f'campaign {st["run"]} was refused: {st["refused"]}')
    if st.get('result') and os.path.exists(os.path.join(d, 'games.jsonl')):
        return st['result']
    cl = client or FD.Client((opts or {}).get('url', FD.URL))
    if deadline_s is not None:
        st['deadline_s'] = float(deadline_s) if deadline_s > 0 else time.time() - (st.get('submitted') or st['created'])
        save_state(d, st)
    deadline = (st.get('submitted') or st['created']) + st['deadline_s']
    last = None
    while True:
        pull(cl, d, st, log)
        miss = missing(st)
        if not miss:
            break
        line = f'{st["games"] - len(miss)}/{st["games"]} games in; {len(miss)} out'
        if line != last:
            es = farm_state(cl, st)
            log(line + (f' (farm: {es.get("leased")} leased, {es.get("unissued")} unissued)' if es else ''))
            last = line
        if time.time() >= deadline:
            break
        if not wait:
            return None
        time.sleep(min(poll, max(1.0, deadline - time.time())))
    miss = missing(st)
    if st.get('run_state') != 'paused':
        stop_farm(cl, st, log)
        save_state(d, st)
    if miss:
        pull(cl, d, st, log)                       # (a chunk that came while pausing)
        miss = missing(st)
    if miss:
        if not local:
            _refuse(d, st, f'games {ranges_of(miss)} missing after the deadline (no local fallback)')
        log(f'deadline: {len(miss)} games the farm did not deliver ({ranges_of(miss)}) are played here')
        play_local(d, st, miss, threads, wrap, log)
    cov = coverage(st)
    if cov['missing']:
        _refuse(d, st, f'games {cov["missing"]} neither done nor failed')
    if cov['failed'] and not allow_failed:
        raise CampaignError(f'campaign {st["run"]}: games failed: {cov["failed"]}')
    rows = rows_of(d)
    for r in rows.values():
        FE.check_digest(r)
    spot = spot_check(d, st, engine=engine, log=log)
    with open(os.path.join(d, 'games.jsonl'), 'w', encoding='utf-8', newline='\n') as f:
        for g in sorted(rows):
            f.write(json.dumps(rows[g], separators=(',', ':')) + '\n')
    leases = {}
    for r in rows.values():
        leases.setdefault((r.get('by') or {}).get('lease') or '?', []).append(r)
    res = {'run': st['run'], 'games': len(rows), 'coverage': cov, 'digest': FE.games_digest(list(rows.values())),
           'lease_digests': {k: FE.games_digest(v) for k, v in sorted(leases.items())}, 'spot': spot,
           'machines': st.get('machines'), 'seed': st['seed'], 'paired': st['paired'], 'build_id': st['build_id'],
           'alternates': st.get('alternates'), 'alternates_note': st.get('alternates_note'),
           'job': st.get('job') or {'kind': 'play', 'trace': True, 'samples': False},
           'gens': {g: {'checkpoint': x['checkpoint'], 'sha256': x['sha256']} for g, x in st['gens'].items()},
           'seconds': round(time.time() - (st.get('submitted') or st['created']), 1), 'readout': readout(d, st)}
    if describe_games:
        res['records'] = write_records(d, st, log, engine)
    st['result'] = res
    save_state(d, st)
    return res


def games(d):
    """The campaign's traced rows (games.jsonl), in gid order."""
    with open(os.path.join(d, 'games.jsonl'), encoding='utf-8') as f:
        return [json.loads(line) for line in f if line.strip()]


# ------------------------------------------------------------------------------------------------ campaigns of a run
def run_pool(cfg):
    """A run's farm pool, when its settings attach one (`selfplay_remote` names `pool:<farm run>`, with pool_build and
    pool_worker) -> {farm, build_id, runtime, worker, min_agent} or None."""
    rem = cfg.get('selfplay_remote') or ''
    names = [x.strip() for x in (rem.split(',') if isinstance(rem, str) else rem) if str(x).strip()]
    farm = next((x[5:] for x in names if str(x).startswith('pool:')), None)
    if not farm or not cfg.get('pool_build') or not cfg.get('pool_worker'):
        return None
    return {'farm': farm, 'build_id': cfg['pool_build'], 'runtime': cfg.get('pool_runtime'),
            'worker': cfg['pool_worker'], 'min_agent': cfg.get('pool_min_agent') or None}
