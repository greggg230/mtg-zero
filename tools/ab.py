"""A/B experiments of training settings (docs/ENGINE.md 193): arms forked from one checkpoint of a source run, each
with its own setting overrides, trained one after the other on this machine, then judged by the metagame cells
(`tools/run_yardsticks.py cells-match`) and published to delverai's Head-to-heads tab.

  python tools/ab.py check   SPEC          # the plan: K, each arm's settings, the code, the source idle? publishing ok?
  python tools/ab.py fork    SPEC          # the arms' run directories (tools/fork_run.py --copy; existing ones kept)
  python tools/ab.py go      SPEC [--no-publish]   # fork if needed, each arm to its end in order, then judge
                                           #   (foreground: run it with nohup / in the background)
  python tools/ab.py launch  SPEC ARM      # one arm's supervisor (hidden), returns at once
  python tools/ab.py status  SPEC          # each arm's checkpoint, supervisor and last metrics
  python tools/ab.py stop    SPEC          # PAUSE_NEXT each live arm at its boundary (pause_run.py --wait), then STOP
  python tools/ab.py judge   SPEC [--no-publish] [--ref CKPT --ref-label L]   # the cells matches + the metrics
                                           #   table -> <out>/judge/, published (--ref: an outside reference too)
  python tools/ab.py publish SPEC [--dry-run] [--replace]  # (again) the judge's results to the Head-to-heads tab
  python tools/ab.py farm-gates SPEC [ARM]  # tools/farm_gates.py on each farm arm's config + checkpoint K (its worker)
  python tools/ab.py farm-pause SPEC ARM    # the arm's coordinator farm run -> "paused" (go does it after each arm)

THE SPEC (JSON):
  {
    "id": "meta3-adaptive-budget",            # the experiment: h2h.<id> on the site; arm runs <run_prefix><arm>
    "title": "...", "description": "...",    # the site's experiment title / description
    "source": {"run": "E:/mtg-zero/runs/legacy-meta3-r1", "iter": 207},   # iter: a number or "newest"
    "iterations": 28,                         # each arm trains K+1 .. K+iterations
    "arms": {"ctrl": {"p_full_rule": "random"}, "adapt": {...}},           # name -> setting overrides (--set)
    "control": "ctrl",                        # default: the first arm; every other arm is a treatment
    "reference": {"path": "E:/.../iter_0262.pt", "label": "pb1@262"},     # optional: a network from OUTSIDE the
                                              #   experiment every arm is judged against too (one arm is enough then)
    "labels": {"ctrl": "Coin (control)", "adapt": "Prior-uncertainty budget"},   # names on the site (default: names)
    "common": {"selfplay_remote": "", ...},   # overrides of every arm (not `publish`, not `iters`: ab.py's)
    "runs_dir": "E:/mtg-zero/runs",           # default: the source run's parent
    "run_prefix": "meta3-ab-",                # default "<id>-"
    "out": ".",                               # judge/ and ab_state.json here (relative: to the spec's directory)
    "code": {"worktree": "...", "sha": "887d83c0", "python": "...",       # default: this checkout, this python
             "arms": {"ctrl": {"worktree": "...", "sha": "..."}}},         # an arm's own code (the rest: the above)
    "farm": {"gates": true, "pause_when_done": true},                       # opt-in: the farm arms (below)
    "env": {"ZERO_TOKEN_GRAPHS": "1"},        # extra environment of the arms' supervisors
    "require_source_idle": true,              # go refuses while the source's supervisor is live (the one GPU)
    "judge": {"games": 200, "device": "cuda", "sims": null, "seed": null, "decks": null, "yardsticks": null,
              "vs_start": true},              # cells-match's options; vs_start: each arm against the start too
    "publish": {"h2h": true,                  # the judge publishes (go / judge; --no-publish skips it)
                "live": true,                 # each arm's supervisor runs the run-owned publisher as a SIDE run
                "live_options": {"every": 8, "games": 6},   # the arm's `publish` dict (mtgz.az.publish KEYS)
                "link": "legacy-meta3-r1",    # the run the experiment links to (default: the source run's name)
                "url": null}                  # the site (default delverai.greggg230.com)
  }

THE ARMS: tools/fork_run.py --source <source run> --dest <runs_dir>/<run_prefix><arm> --iter K --copy (the checkpoint,
the optimizer state, the training window's shards copied, metrics up to K; the source is only read) with --set for
common + the arm's overrides + `iters` K + iterations + `publish`: with publish.live, the run-owned publisher
(mtgz.az.publish, ENGINE.md 163) as a side run - {"side": true, "title": "<id>: <arm>", "note": ..., "forks": [],
**live_options} - so each arm's progress is on the site while it trains, never the site's default run; without it
`publish` {}. K is kept in <out>/ab_state.json. The arms train one after the other (`go`: the control first, then the
treatments in the spec's order), never at once.

THE JUDGE (<out>/judge/): for each treatment T, cells-match T's last checkpoint against the control's (T_vs_<ctrl>
.json: {summary, rows}); with judge.vs_start, every arm's last checkpoint against the common start (<arm>_vs_start
.json; the start copied to judge/start_iter_KKKK.pt); then report.json / report.md (the matches' summaries, each arm's
metrics over its iterations). One baseline cache (judge/cells_base) for all of them.

AN OUTSIDE REFERENCE (docs/ENGINE.md 2XX; "reference" in the spec, or `--ref CKPT --ref-label LABEL` on judge / go /
check / publish, kept in ab_state.json for a later `publish`): a fixed checkpoint from outside the experiment - e.g.
pb1@262, generation 262 of delver-bo3-pb1 - that every arm's last checkpoint is also judged against (<arm>_vs_ref
.json: the arm pilots each deck against the reference piloting each deck; label default: its run and generation). With
a reference the spec may have ONE arm (a single-arm experiment: a fork trained on, judged against a fixed yardstick;
that arm is the control, there are no treatments). Published as each arm's overall match against the reference and
one match per cell (keys ranked 7 / 3).

PUBLISHING (default on; `--no-publish`, or "publish": {"h2h": false}, said loudly: PUBLISH OFF on stderr at go and at
the judge, with the command that posts later): the judge posts one Head-to-heads experiment
h2h.<id> through delverai's tools/upload_h2h.py --manifest (mtgz.az.site_h2h finds it): for each treatment the overall
match (the mean of the cells) and one match per cell (treatment piloting each deck against the control piloting each
deck), and each arm's overall match against the start. A cell's Elo difference is the treatment's GAIN over the control
in that seat on the same deals (not a raw score; the uploader's notes say so, its docstring has the mapping). The tab
lists the treatments' overall matches first, then the arms against the start, then the cells (keys ranked 9 / 5 / 1;
one date: when the judge finished). Labels only - "<label>, gen N" - never a path. A post that fails says PUBLISH
FAILED with the retry command (`ab.py publish SPEC`), exit 3; the judge's files stand.

THE FARM BUILD: an override `"pool_build": "published"` (in `common` or an arm) is the published farm build of the
code's commit (`code.sha`, else its worktree's HEAD; "published:<rev>" another commit) with its pool_runtime,
pool_worker and pool_alternates unless the overrides give them (tools/farm_release.py lookup; docs/ENGINE.md 196).

PER-ARM CODE (code.arms): an arm whose engine differs (an A/B of an engine change) trains from its own pinned
worktree: its supervisor, fork_run, pause_run and farm gates run there (`check` checks each worktree's sha and engine
build). The judge runs in the common code's worktree (`code.worktree`): the checkout every arm is measured on.

THE FARM (opt-in, "farm": {...}): an arm self-plays on the farm when its settings name a pool (`selfplay_remote`
'pool:<farm run>' with pool_build / pool_worker / pool_runtime / pool_alternates ...: setting overrides like any other,
in `common` or per arm - the same lease settings for every arm unless the A/B is about them; the worker build must play
the arm's engine: one build per engine, published with tools/farm_build.py beforehand). With "gates" (default true),
`farm-gates` runs tools/farm_gates.py on each farm arm's forked config.json and checkpoint K with its pool_worker
(<out>/farm_gates/<arm>/summary.json), and `launch` refuses a farm arm without a passed summary for that worker
(AGENTS.md: farm compatibility is required; `go` runs the missing gates first). The coordinator's farm run is created
by the pool's first PUT; with "pause_when_done" (default true) `go` pauses it ("state": "paused": the whole body as GET
returns it minus alternates[].problems) when the arm ends, so the farm stops serving its last generation.
`go --no-judge` trains the arms and stops before the cells judge (an A/B judged otherwise).

STOP: `ab.py stop SPEC`; `go` exits when an arm stops before its last iteration (resume: `ab.py launch SPEC ARM`, or
`go` again: complete arms are skipped). ROLLBACK: stop; delete the arms' run directories; the source was never touched.
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
from mtgz.az import site_h2h as S  # noqa: E402

RESERVED = ('publish', 'iters')
LIVE_DEFAULTS = {'every': 8, 'games': 6, 'device': 'cpu', 'threads': 4, 'census': 0}
JUDGE_DEFAULTS = {'games': 200, 'device': 'cuda', 'sims': None, 'seed': None, 'decks': None, 'yardsticks': None,
                  'vs_start': True}
TERMINAL = ('done', 'stopped', 'refused', 'gave_up', 'paused')


# ------------------------------------------------------------------ the spec
def load_spec(path, ref_given=False):
    """The spec file -> normalized; `ref_given`: an outside reference comes from the command line (one arm is enough)."""
    with open(path, encoding='utf-8') as f:
        spec = json.load(f)
    return normalize(spec, os.path.dirname(os.path.abspath(path)), ref_given)


def normalize(spec, base='.', ref_given=False):
    """The spec with its defaults filled in -> dict; SystemExit naming what is wrong."""
    s = dict(spec)
    for k in ('id', 'title', 'source', 'iterations', 'arms'):
        if k not in s:
            raise SystemExit(f'spec: no {k!r}')
    if not S.NAME_RE.match(str(s['id'])):
        raise SystemExit(f"spec: id {s['id']!r}: letters, digits, . _ - (at most 60)")
    s['reference'] = reference_of(s.get('reference'))
    if not isinstance(s['arms'], dict) or len(s['arms']) < (1 if s['reference'] or ref_given else 2):
        raise SystemExit('spec: arms {name: overrides} - at least two (or one with an outside "reference")')
    for name, ov in [('common', s.get('common') or {})] + list(s['arms'].items()):
        if not isinstance(ov, dict):
            raise SystemExit(f'spec: {name}: a JSON object of setting overrides')
        bad = [k for k in RESERVED if k in ov]
        if bad:
            raise SystemExit(f'spec: {name} sets {bad}: ab.py sets those (iterations, publish.live / live_options)')
        if name != 'common' and not S.NAME_RE.match(name):
            raise SystemExit(f'spec: arm {name!r}: letters, digits, . _ -')
    if not isinstance(s['iterations'], int) or s['iterations'] < 1:
        raise SystemExit('spec: iterations - a positive number')
    src = s['source']
    if not isinstance(src, dict) or not src.get('run') or not (src.get('iter') == 'newest' or isinstance(src.get('iter'), int)):
        raise SystemExit('spec: source {"run": DIR, "iter": N | "newest"}')
    s['control'] = s.get('control') or next(iter(s['arms']))
    if s['control'] not in s['arms']:
        raise SystemExit(f"spec: control {s['control']!r} is no arm")
    s['treatments'] = [a for a in s['arms'] if a != s['control']]
    s['labels'] = {a: (s.get('labels') or {}).get(a) or a for a in s['arms']}
    if len(set(s['labels'].values())) != len(s['labels']):
        raise SystemExit('spec: two arms with one label')
    s['description'] = s.get('description') or ''
    s['common'] = dict(s.get('common') or {})
    s['runs_dir'] = s.get('runs_dir') or os.path.dirname(os.path.normpath(src['run']))
    s['run_prefix'] = s.get('run_prefix') or f"{s['id']}-"
    s['out'] = os.path.normpath(os.path.join(base, s.get('out') or '.'))
    code = dict(s.get('code') or {})
    code.setdefault('worktree', None)
    code.setdefault('sha', None)
    code.setdefault('python', None)
    code['arms'] = dict(code.get('arms') or {})
    for a, c in code['arms'].items():
        if a not in s['arms'] or not isinstance(c, dict) or set(c) - {'worktree', 'sha', 'python'}:
            raise SystemExit(f'spec: code.arms.{a}: an arm\'s {{"worktree", "sha", "python"}}')
    s['code'] = code
    farm = s.get('farm')
    if farm is not None and not isinstance(farm, dict):
        raise SystemExit('spec: farm - {"gates": true, "pause_when_done": true}')
    s['farm'] = dict({'gates': True, 'pause_when_done': True}, **farm) if farm is not None else None
    s['env'] = {str(k): str(v) for k, v in (s.get('env') or {}).items()}
    s['require_source_idle'] = s.get('require_source_idle', True)
    s['judge'] = dict(JUDGE_DEFAULTS, **(s.get('judge') or {}))
    if s['judge']['games'] % 2:
        raise SystemExit('spec: judge.games - an even number (games per cell)')
    pub = dict({'h2h': True, 'live': True, 'live_options': {}, 'link': None, 'url': None}, **(s.get('publish') or {}))
    pub['link'] = pub['link'] or os.path.basename(os.path.normpath(src['run']))
    s['publish'] = pub
    return s


def reference_of(r):
    """The spec's outside reference -> {'path', 'label'} or None; SystemExit for a bad one (docs/ENGINE.md 2XX)."""
    if r is None:
        return None
    if not isinstance(r, dict) or not isinstance(r.get('path'), str) or not r['path'] or set(r) - {'path', 'label'}:
        raise SystemExit('spec: reference {"path": CKPT, "label": "pb1@262"} (label optional)')
    label = r.get('label') or S.checkpoint_label(r['path'])
    if not isinstance(label, str) or not label.strip() or len(label) > 60:
        raise SystemExit(f'spec: reference label {label!r}: a name of at most 60 characters')
    return {'path': r['path'], 'label': label.strip()}


def with_reference(s, path=None, label=None):
    """The spec with the outside reference of the command line (`--ref`, saved in ab_state.json for a later publish),
    else the spec's, else the one a judge saved -> s."""
    st = jload(state_path(s), {}) or {}
    if path:
        s['reference'] = reference_of({'path': path, 'label': label} if label else {'path': path})
        if st.get('reference') != s['reference']:
            jsave(state_path(s), dict(st, reference=s['reference']))
    elif label:
        raise SystemExit('--ref-label without --ref CKPT')
    elif not s.get('reference') and st.get('reference'):
        s['reference'] = reference_of(st['reference'])
    return s


def publish_off(s, why):
    """The loud line of a judge that will not post (stderr)."""
    print(f"PUBLISH OFF: h2h.{s['id']} ({why}) - the judge's results will NOT appear on the Delver AI site's "
          f"Head-to-heads tab; post them later: python tools/ab.py publish SPEC (docs/ENGINE.md 193, 2XX)",
          file=sys.stderr, flush=True)


def worktree(s, arm=None):
    return ((s['code'].get('arms') or {}).get(arm) or {}).get('worktree') or s['code'].get('worktree') or ROOT


def python(s, arm=None):
    return ((s['code'].get('arms') or {}).get(arm) or {}).get('python') or s['code'].get('python') or sys.executable


def code_sha(s, arm=None):
    c = (s['code'].get('arms') or {}).get(arm) or {}
    return c.get('sha') if c.get('worktree') or c.get('sha') else s['code'].get('sha')


def farm_run(cfg):
    """The farm run an arm's settings self-play on ('pool:<name>' in selfplay_remote), or None."""
    for r in str((cfg or {}).get('selfplay_remote') or '').split(','):
        if r.strip().startswith('pool:'):
            return r.strip()[5:]
    return None


def run_dir(s, arm):
    return os.path.join(s['runs_dir'], s['run_prefix'] + arm).replace('\\', '/')


def judge_dir(s):
    return os.path.join(s['out'], 'judge')


def live_publish(s, arm):
    """The arm's `publish` setting: the run-owned publisher as a side run, or {} (off)."""
    if not s['publish'].get('live'):
        return {}
    note = f"Arm {s['labels'][arm]!r} of the A/B {s['title']!r}" + (f": {s['description']}" if s['description'] else '')
    pub = dict(LIVE_DEFAULTS, **(s['publish'].get('live_options') or {}))
    pub.update(side=True, title=f"{s['id']}: {arm}", note=note[:300])
    pub.setdefault('forks', [])
    if s['publish'].get('url'):
        pub.setdefault('url', s['publish']['url'])
    from mtgz.az import publish as P
    try:
        P.normalize(pub)
    except ValueError as e:
        raise SystemExit(f'spec: publish.live_options: {e}')
    return pub


def overrides(s, arm, k):
    o = dict(s['common'], **s['arms'][arm])
    o['iters'] = k + s['iterations']
    o['publish'] = live_publish(s, arm)
    return published_build(s, o, arm)


def published_build(s, o, arm=None):
    """`"pool_build": "published"` (or "published:<rev>") -> the published farm build of the arms' code commit (or
    <rev>'s) and, unless the overrides give them, its pool_runtime, pool_worker and pool_alternates (tools/farm_release.py
    lookup, docs/ENGINE.md 196). SystemExit when there is none."""
    v = o.get('pool_build')
    if not (isinstance(v, str) and (v == 'published' or v.startswith('published:'))):
        return o
    from mtgz.az import farm_builds as FB
    wt = worktree(s, arm)                      # (an arm with its own code: its own commit's build)
    rev = v.split(':', 1)[1] if ':' in v else (code_sha(s, arm) or 'HEAD')
    e = FB.lookup(rev, wt)
    if e is None:
        raise SystemExit(f'pool_build {v!r}: no published farm build for {rev} of {wt} (python tools/farm_release.py release {rev})')
    out = dict(o)
    for key, val in FB.pool_settings(e).items():
        if key == 'pool_build' or key not in o:
            out[key] = val
    return out


# ------------------------------------------------------------------ state
def jload(p, d=None):
    try:
        with open(p, encoding='utf-8') as f:
            return json.load(f)
    except (OSError, ValueError):
        return d


def jsave(p, obj):
    os.makedirs(os.path.dirname(os.path.abspath(p)), exist_ok=True)
    tmp = f'{p}.{os.getpid()}.tmp'
    with open(tmp, 'w', encoding='utf-8', newline='') as f:
        json.dump(obj, f, indent=1)
        f.write('\n')
    os.replace(tmp, p)


def newest(run):
    d = os.path.join(run, 'models')
    its = [int(f[5:-3]) for f in os.listdir(d) if f.startswith('iter_') and f.endswith('.pt')] if os.path.isdir(d) else []
    return max(its) if its else None


def alive(pid):
    if not pid:
        return False
    try:
        import psutil
        return psutil.pid_exists(int(pid))
    except ImportError:
        out = subprocess.run(['tasklist', '/FI', f'PID eq {pid}', '/NH'], capture_output=True, text=True).stdout
        return str(pid) in out


def supervisor(run):
    st = jload(os.path.join(run, 'supervisor_state.json'), {}) or {}
    return st, alive(st.get('pid')) and st.get('state') != 'done'


def state_path(s):
    return os.path.join(s['out'], 'ab_state.json')


def fork_k(s):
    """K: the forked arms' (ab_state.json), else the spec's source.iter when a number."""
    st = jload(state_path(s), {}) or {}
    if st.get('k') is not None:
        return int(st['k'])
    if isinstance(s['source']['iter'], int):
        return s['source']['iter']
    raise SystemExit(f"{state_path(s)}: no K yet (fork first, or give source.iter a number)")


def source_k(s):
    return newest(s['source']['run']) if s['source']['iter'] == 'newest' else int(s['source']['iter'])


# ------------------------------------------------------------------ commands
def check_code(wt, sha):
    head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=wt, text=True).strip()
    if sha and not head.startswith(sha):
        raise SystemExit(f"{wt} is at {head[:12]}, not {sha}")
    if not any(f.startswith('_engine') and f.endswith(('.pyd', '.so')) for f in os.listdir(os.path.join(wt, 'mtgz'))):
        raise SystemExit(f'{wt}: no mtgz/_engine build (bash tools/build_py.sh there)')
    return head


def check(s):
    wt = worktree(s)
    head = check_code(wt, s['code']['sha'])
    for a in s['code']['arms']:
        print(f'  {a}: code {worktree(s, a)} at {check_code(worktree(s, a), code_sha(s, a))[:12]}; python {python(s, a)}')
    k = source_k(s)
    src = s['source']['run']
    if k is None or not os.path.isfile(os.path.join(src, 'models', f'iter_{k:04d}.pt')):
        raise SystemExit(f'{src}: no checkpoint iter_{k}')
    st, live = supervisor(src)
    print(f"A/B {s['id']} ({s['title']}): source {src} at {k}; its supervisor {st.get('state')!r} pid {st.get('pid')} "
          f"({'LIVE' if live else 'not running'})")
    for arm in s['arms']:
        role = 'control' if arm == s['control'] else 'treatment'
        ov = overrides(s, arm, k)
        print(f"  {arm} ({role}, {s['labels'][arm]!r}): {run_dir(s, arm)} iters {k + 1}..{k + s['iterations']}: "
              f"{json.dumps(ov)}")
        if farm_run(ov) and s['farm'] is None:
            raise SystemExit(f'{arm} self-plays on the farm ({farm_run(ov)}): the spec needs "farm": {{...}} (the gates)')
    print(f"code: {wt} at {head[:12]}; python {python(s)}; out {s['out']}")
    if s['reference']:
        ok = os.path.isfile(s['reference']['path'])
        print(f"outside reference: {s['reference']['label']!r} ({s['reference']['path']}{'' if ok else ': MISSING'}): "
              f"every arm judged against it too")
    pub = s['publish']
    if pub['h2h'] or pub['live']:
        up = S.uploader_path()
        print(f"publishing: h2h.{s['id']} {'on' if pub['h2h'] else 'off'} (uploader {'found' if os.path.isfile(up) else 'MISSING'}"
              f"), live side runs {'on' if pub['live'] else 'off'}, upload key {'found' if S.has_key() else 'MISSING'}")
    return k, live


def fork(s, k=None):
    k = source_k(s) if k is None else k
    st = jload(state_path(s), {}) or {}
    if st.get('k') is not None and st['k'] != k:
        raise SystemExit(f"{state_path(s)}: the arms were forked at {st['k']}, not {k}")
    for arm in s['arms']:
        d = run_dir(s, arm)
        if os.path.exists(os.path.join(d, 'config.json')):
            print(f'{d}: forked already')
            continue
        cmd = [python(s, arm), 'tools/fork_run.py', '--source', s['source']['run'], '--dest', d, '--iter', str(k), '--copy']
        ov = overrides(s, arm, k)
        for key, v in ov.items():
            cmd += ['--set', f'{key}={json.dumps(v)}']
        print(subprocess.list2cmdline(cmd), flush=True)
        subprocess.run(cmd, cwd=worktree(s, arm), check=True)
        cfg = jload(os.path.join(d, 'config.json'), {})
        for key, v in ov.items():
            if cfg.get(key) != v:
                raise SystemExit(f'{d}: config.json {key} = {cfg.get(key)!r}, not {v!r}')
    jsave(state_path(s), dict(st, k=k, id=s['id'], arms={a: run_dir(s, a) for a in s['arms']},
                              forked=st.get('forked') or time.strftime('%Y-%m-%dT%H:%M:%S')))
    return k


def launch(s, arm):
    d = run_dir(s, arm)
    _, live = supervisor(d)
    if live:
        raise SystemExit(f'{arm}: its supervisor is live')
    for a in s['arms']:
        if a != arm and supervisor(run_dir(s, a))[1]:
            raise SystemExit(f'{a} is live: one arm at a time')
    why = farm_gate_missing(s, arm)
    if why:
        raise SystemExit(f'{arm}: {why}')
    env = dict(os.environ, **s['env'])
    env.pop('CUDA_VISIBLE_DEVICES', None)
    py = python(s, arm)
    with open(os.path.join(d, 'launch_out.txt'), 'a', encoding='utf-8') as lf:
        r = subprocess.run([py, 'tools/launch_hidden.py', os.path.join(d, 'supervisor_console.txt'), py,
                            'tools/supervise.py', d], cwd=worktree(s, arm), env=env, stdin=subprocess.DEVNULL, stdout=lf,
                           stderr=subprocess.STDOUT)
    print(f'{arm}: launch exit {r.returncode}', flush=True)
    return r.returncode


def wait(s, arm, last):
    """Until the arm has its last checkpoint and its supervisor is gone -> True; False when it stopped early."""
    d = run_dir(s, arm)
    time.sleep(60)
    while True:
        st = jload(os.path.join(d, 'supervisor_state.json'), {}) or {}
        have = newest(d) or 0
        gone = not alive(st.get('pid'))
        if have >= last and gone:
            return True
        if gone and st.get('state') in TERMINAL:
            print(f'{arm}: supervisor ended ({st.get("state")}) at checkpoint {have} of {last}', flush=True)
            return have >= last
        time.sleep(30)


def read_metrics(d):
    p = os.path.join(d, 'metrics.jsonl')
    if not os.path.exists(p):
        return []
    with open(p, encoding='utf-8') as f:
        return [json.loads(x) for x in f if x.strip()]


def status(s):
    for arm in s['arms']:
        d = run_dir(s, arm)
        if not os.path.exists(os.path.join(d, 'config.json')):
            print(f'{arm}: not forked')
            continue
        st, live = supervisor(d)
        print(f'{arm}: checkpoint {newest(d)}, supervisor {st.get("state")!r} pid {st.get("pid")} '
              f'({"live" if live else "gone"})')
        for r in read_metrics(d)[-1:]:
            sp = r.get('selfplay') or {}
            print(f'   iter {r["iter"]}: {sp.get("games")} games in {sp.get("seconds", 0):.0f} s, train '
                  f'{json.dumps({k: (r.get("train") or {}).get(k) for k in ("policy", "entropy", "value")})}'
                  + (f', budget {json.dumps(sp["budget"])}' if sp.get('budget') else ''))


def stop(s):
    for arm in s['arms']:
        d = run_dir(s, arm)
        if supervisor(d)[1]:
            subprocess.run([python(s, arm), 'tools/pause_run.py', d, '--wait'], cwd=worktree(s, arm))
            open(os.path.join(d, 'STOP'), 'w').close()
            print(f'{arm}: paused at its boundary, STOP written')


def _mean(xs):
    xs = [x for x in xs if x is not None]
    return round(sum(xs) / len(xs), 4) if xs else None


def summarize(s, arm, k):
    """The arm's metrics over its own iterations (K+1 ..): speed, losses; the playout budget when it logs one."""
    rows = [r for r in read_metrics(run_dir(s, arm)) if r.get('iter', 0) > k]
    if not rows:
        return None
    sp = [r['selfplay'] for r in rows if 'selfplay' in r]
    tr = [r['train'] for r in rows if 'train' in r]
    out = {'iters': len(rows),
           'games_per_min': round(60 * sum(x.get('games', 0) for x in sp) / max(1e-9, sum(x.get('seconds', 0) for x in sp)), 1),
           'selfplay_s': _mean([x.get('seconds') for x in sp]), 'train_s': _mean([t.get('seconds') for t in tr]),
           'decisions_per_game': _mean([x.get('decisions_per_game') for x in sp]),
           'samples_per_iter': _mean([x.get('samples') for x in sp]),
           'resigned_share': _mean([(x.get('resign') or {}).get('resigned_share') for x in sp]),
           'policy_last8': _mean([t.get('policy') for t in tr[-8:]]),
           'entropy_last8': _mean([t.get('entropy') for t in tr[-8:]]),
           'value_last8': _mean([t.get('value') for t in tr[-8:]])}
    b = [x['budget'] for x in sp if x.get('budget')]
    n = sum(x.get('decisions', 0) for x in b)
    if n:
        out['sims_per_decision'] = round(sum(x['sims_per_decision'] * x['decisions'] for x in b) / n, 3)
        out['full_share'] = round(sum(x.get('full', 0) for x in b) / n, 4)
    return out


def judge_matches(s, k):
    """The judge's cells matches -> [(key, A arm, B arm or 'start')]: each treatment against the control, then (with
    vs_start) every arm against the start."""
    out = [(f"{t}_vs_{s['control']}", t, s['control']) for t in s['treatments']]
    if s.get('reference'):
        out += [(f'{a}_vs_ref', a, 'ref') for a in s['arms']]
    if s['judge']['vs_start']:
        out += [(f'{a}_vs_start', a, 'start') for a in s['arms']]
    return out


def judge(s, publish=True):
    out = judge_dir(s)
    os.makedirs(out, exist_ok=True)
    k = fork_k(s)
    last = k + s['iterations']
    start = os.path.join(out, f'start_iter_{k:04d}.pt')
    if s['judge']['vs_start'] and not os.path.exists(start):
        shutil.copyfile(os.path.join(run_dir(s, s['control']), 'models', f'iter_{k:04d}.pt'), start)
    fin = {a: os.path.join(run_dir(s, a), 'models', f'iter_{last:04d}.pt') for a in s['arms']}
    fin['start'] = start
    if s.get('reference'):
        fin['ref'] = s['reference']['path']
        if not os.path.isfile(fin['ref']):
            raise SystemExit(f"the outside reference {s['reference']['label']!r}: no checkpoint {fin['ref']}")
    for a in s['arms']:
        if not os.path.isfile(fin[a]):
            raise SystemExit(f'{a}: no checkpoint {last} yet ({fin[a]})')
    if not publish:
        publish_off(s, '--no-publish')
    elif not s['publish']['h2h']:
        publish_off(s, 'the spec\'s "publish": {"h2h": false}')
    env = dict(os.environ)
    env.pop('CUDA_VISIBLE_DEVICES', None)
    j = s['judge']
    res = {}
    for key, a, b in judge_matches(s, k):
        jp = os.path.join(out, key + '.json')
        if not os.path.exists(jp):
            cmd = [python(s), 'tools/run_yardsticks.py', 'cells-match', fin[a], '--ref', fin[b], '--ref-label', b,
                   '--games', str(j['games']), '--cache', os.path.join(out, 'cells_base'),
                   '--out', os.path.join(out, key + '.rows.jsonl'), '--json']
            for opt in ('sims', 'seed', 'device'):
                if j.get(opt) is not None:
                    cmd += [f'--{opt}', str(j[opt])]
            if j.get('decks'):
                cmd += ['--decks', ','.join(j['decks']) if isinstance(j['decks'], list) else str(j['decks'])]
            if j.get('yardsticks'):
                cmd += ['--yardsticks', json.dumps(j['yardsticks'])]
            print(subprocess.list2cmdline(cmd), flush=True)
            t0 = time.time()
            p = subprocess.run(cmd, cwd=worktree(s), env=env, capture_output=True, text=True)
            with open(os.path.join(out, key + '.log'), 'w', encoding='utf-8') as f:
                f.write(p.stderr)
            if p.returncode != 0:
                raise SystemExit(f'{key}: cells-match failed ({p.returncode}); see {key}.log')
            r = json.loads(p.stdout)
            r['seconds'] = round(time.time() - t0, 1)
            jsave(jp, r)
        res[key] = (jload(jp) or {})['summary']
    rep = {'id': s['id'], 'k': k, 'iters': s['iterations'], 'control': s['control'], 'matches': res,
           'arms': {a: summarize(s, a, k) for a in s['arms']}}
    if s.get('reference'):
        rep['reference'] = {'label': s['reference']['label']}
    jsave(os.path.join(out, 'report.json'), rep)
    lines = [f"# {s['title']} (A/B {s['id']}: K {k}, {s['iterations']} iterations each)", '']
    for a, m in rep['arms'].items():
        if m:
            lines.append(f"* {a}: " + ', '.join(f'{x} {v}' for x, v in m.items()))
    lines.append('')
    for key, sm in res.items():
        ov = sm.get('overall') or {}
        lines.append(f"* {key}: overall {ov.get('elo')} +- {ov.get('se')} Elo; decks {json.dumps(sm.get('decks'))}")
    with open(os.path.join(out, 'report.md'), 'w', encoding='utf-8', newline='') as f:
        f.write('\n'.join(lines) + '\n')
    print('\n'.join(lines))
    if publish and s['publish']['h2h']:
        return publish_judge(s)
    return 0


def arm_note(s, a, m):
    if not m:
        return None
    bits = [f"{m['games_per_min']} self-play games a minute"]
    if m.get('sims_per_decision') is not None:
        bits.append(f"{m['sims_per_decision']} simulations a decision (full searches {100 * m['full_share']:.1f}%)")
    if m.get('policy_last8') is not None:
        bits.append(f"policy loss {m['policy_last8']:.3f}, value loss {m['value_last8']:.3f} (last 8 iterations)")
    return f"{s['labels'][a]} over its {m['iters']} iterations: " + '; '.join(bits) + '.'


def manifest(s, k, arms_metrics=None, date=None):
    """The uploader's manifest of the judge's results (delverai tools/upload_h2h.py --manifest; cells entries)."""
    last = k + s['iterations']
    jd = judge_dir(s)
    lab = lambda a: f"{s['labels'][a]}, gen {last}"  # noqa: E731
    src = os.path.basename(os.path.normpath(s['source']['run']))
    desc = (s['description'] + ' ' if s['description'] else '') + (
        f"Arms forked from {src} at generation {k}, {s['iterations']} training iterations each, judged by the "
        f"metagame cells ({s['judge']['games']} games a cell, best of one).")
    ms = []
    for t in s['treatments']:
        c = s['control']
        ms.append({'experiment': s['id'], 'cells': os.path.join(jd, f'{t}_vs_{c}.json'), 'a_label': lab(t),
                   'b_label': lab(c), 'key_prefix': f'9-{t}-vs-{c}.', 'cell_key_prefix': f'1-{t}-vs-{c}.'})
    ref = s.get('reference')
    if ref:
        desc += (f" Each arm is also judged against {ref['label']}, a fixed network from outside the experiment "
                 f"(its gain over it in each seat).")
        for a in s['arms']:
            ms.append({'experiment': s['id'], 'cells': os.path.join(jd, f'{a}_vs_ref.json'), 'a_label': lab(a),
                       'b_label': ref['label'], 'key_prefix': f'7-{a}-vs-ref.', 'cell_key_prefix': f'3-{a}-vs-ref.'})
    if s['judge']['vs_start']:
        for a in s['arms']:
            e = {'experiment': s['id'], 'cells': os.path.join(jd, f'{a}_vs_start.json'), 'a_label': lab(a),
                 'b_label': f'Start, gen {k}', 'key_prefix': f'5-{a}-vs-start.', 'overall_only': True}
            note = arm_note(s, a, (arms_metrics or {}).get(a))
            if note:
                e['note'] = f"Both arms start from {src} gen {k}: how far this arm moved. {note}"
            ms.append(e)
    for e in ms:
        if date:
            e['date'] = date
    return {'experiments': [{'id': s['id'], 'title': s['title'], 'description': desc[:1000],
                             'run': s['publish']['link']}], 'matches': ms}


def publish_judge(s, dry_run=False, replace=False):
    jd = judge_dir(s)
    k = fork_k(s)
    files = [os.path.join(jd, key + '.json') for key, _, _ in judge_matches(s, k)]
    have = [f for f in files if os.path.exists(f)]
    if not have:
        raise SystemExit(f'{jd}: no judge results yet')
    date = time.strftime('%Y-%m-%dT%H:%M:%S%z', time.localtime(max(os.path.getmtime(f) for f in have)))
    date = date[:-2] + ':' + date[-2:]
    rep = jload(os.path.join(jd, 'report.json'), {}) or {}
    arms = rep.get('arms') or {a: summarize(s, a, k) for a in s['arms']}
    man = manifest(s, k, arms, date)
    mp = os.path.join(jd, 'h2h_manifest.json')
    jsave(mp, man)
    args = ['--manifest', mp, '--only', s['id']]
    if s['publish'].get('url'):
        args += ['--url', s['publish']['url']]
    if replace:
        args.append('--replace')
    if dry_run:
        args.append('--dry-run')
    elif not S.has_key():
        print(f'PUBLISH FAILED: no upload key ({S.KEY_ENV} or {S.KEY_FILE}); retry: python tools/ab.py publish SPEC',
              file=sys.stderr)
        return S.EXIT_PUBLISH_FAILED
    if not os.path.isfile(S.uploader_path()):
        print(f'PUBLISH FAILED: no uploader at {S.uploader_path()} (${S.UPLOADER_ENV})', file=sys.stderr)
        return S.EXIT_PUBLISH_FAILED
    rc = S.run_uploader(args, log=print)
    if rc != 0:
        print('PUBLISH FAILED; the judge\'s files stand - retry: python tools/ab.py publish SPEC', file=sys.stderr)
        return S.EXIT_PUBLISH_FAILED
    return 0


def farm_gates_dir(s, arm):
    return os.path.join(s['out'], 'farm_gates', arm)


def farm_gate_missing(s, arm):
    """Why the arm may not start on the farm yet (its config names a pool and the gates have not passed with its
    worker), or None."""
    cfg = jload(os.path.join(run_dir(s, arm), 'config.json'), {}) or {}
    if not farm_run(cfg):
        return None
    if s['farm'] is None:
        return f'it self-plays on the farm ({farm_run(cfg)}): the spec needs "farm": {{...}}'
    if not s['farm']['gates']:
        return None
    sm = jload(os.path.join(farm_gates_dir(s, arm), 'summary.json'), {}) or {}
    w = str(cfg.get('pool_worker') or '')
    same = lambda x: os.path.normcase(os.path.normpath(str(x or '').replace('\\', '/')))  # noqa: E731
    if not sm.get('passed'):
        return f'no passed farm gates in {farm_gates_dir(s, arm)} (ab.py farm-gates SPEC {arm})'
    if same(sm.get('worker')) != same(w):
        return f"the farm gates passed with {sm.get('worker')}, not the arm's pool_worker {w}: run them again"
    cc = jload(os.path.join(farm_gates_dir(s, arm), 'check_config.json'), {}) or {}
    if cc.get('failures') != [] or same(cc.get('worker')) != same(w):
        return (f"the worker's check-config of the arm's settings has not passed with {w} ({cc.get('failures')}; "
                f"ab.py farm-gates SPEC {arm})")
    return None


def farm_gates(s, arm):
    """tools/farm_gates.py on the arm's forked config.json and checkpoint K with its pool_worker, in the arm's own
    code -> exit status (0: both gates passed, or no farm)."""
    d = run_dir(s, arm)
    cfg = jload(os.path.join(d, 'config.json'), {}) or {}
    if not farm_run(cfg):
        print(f'{arm}: no farm (selfplay_remote names no pool)')
        return 0
    k = fork_k(s)
    out = farm_gates_dir(s, arm)
    os.makedirs(out, exist_ok=True)
    # the worker's own check-config of the arm's settings: the farm pool refuses a template whose settings the build
    # does not know (a build older than the arm's zero-core), which the gates' games do not show
    w = str(cfg.get('pool_worker') or '')
    p = subprocess.run([w, 'check-config', os.path.join(d, 'config.json'), '--json'], capture_output=True, text=True,
                       stdin=subprocess.DEVNULL)
    try:
        failures = json.loads(p.stdout).get('failures')
    except ValueError:
        failures = [f'check-config exit {p.returncode}: {(p.stdout + p.stderr)[-300:]}']
    jsave(os.path.join(out, 'check_config.json'), {'worker': w, 'failures': failures})
    if failures:
        print(f"{arm}: the worker refuses the arm's settings {failures}: publish a worker of the arm's code", flush=True)
        return 3
    cmd = [python(s, arm), 'tools/farm_gates.py', '--config', os.path.join(d, 'config.json'),
           '--checkpoint', os.path.join(d, 'models', f'iter_{k:04d}.pt'), '--worker', str(cfg.get('pool_worker') or ''),
           '--out', out]
    print(subprocess.list2cmdline(cmd), flush=True)
    with open(os.path.join(out, 'farm_gates.log'), 'w', encoding='utf-8') as f:
        r = subprocess.run(cmd, cwd=worktree(s, arm), stdin=subprocess.DEVNULL, stdout=f, stderr=subprocess.STDOUT)
    print(f'{arm}: farm gates exit {r.returncode} (log {os.path.join(out, "farm_gates.log")})', flush=True)
    return r.returncode


def farm_pause(s, arm, client=None):
    """The arm's coordinator farm run -> state 'paused': the whole body as GET returns it, minus the GET-only
    alternates[].problems (the coordinator refuses a PUT that echoes them). -> the farm run's name, or None."""
    fr = farm_run(jload(os.path.join(run_dir(s, arm), 'config.json'), {}) or {})
    if not fr:
        return None
    if client is None:
        from zero import farm_def as FD
        client = FD.Client()
    body = dict(client.call('GET', f'/v1/admin/run/{fr}'))
    body['state'] = 'paused'
    if body.get('alternates'):
        body['alternates'] = [{k: v for k, v in x.items() if k != 'problems'} for x in body['alternates']]
    client.call('PUT', f'/v1/admin/run/{fr}', body=body)
    print(f'{arm}: farm run {fr} paused', flush=True)
    return fr


def go(s, publish=True, judge_too=True):
    k, live = check(s)
    if judge_too and not publish:
        publish_off(s, 'go --no-publish')
    elif judge_too and not s['publish']['h2h']:
        publish_off(s, 'the spec\'s "publish": {"h2h": false}')
    if publish and s['publish']['h2h'] and not (os.path.isfile(S.uploader_path()) and S.has_key()):
        raise SystemExit(f'the judge publishes (publish.h2h) but the uploader ({S.uploader_path()}) or the upload key '
                         f'is missing: fix it, or go --no-publish (then ab.py publish SPEC later)')
    if live and s['require_source_idle']:
        raise SystemExit(f"{s['source']['run']} is live: pause it first (the arms need the GPU), or set "
                         f'"require_source_idle": false')
    k = fork(s, k)
    last = k + s['iterations']
    for arm in [s['control']] + s['treatments']:
        if s['farm'] and s['farm']['gates'] and farm_gate_missing(s, arm) and farm_gates(s, arm) != 0:
            raise SystemExit(f'{arm}: the farm gates FAILED ({farm_gates_dir(s, arm)}): go ends here')
    for arm in [s['control']] + s['treatments']:
        if (newest(run_dir(s, arm)) or 0) >= last:
            print(f'{arm}: complete already')
            continue
        launch(s, arm)
        ok = wait(s, arm, last)
        if s['farm'] and s['farm']['pause_when_done']:
            try:
                farm_pause(s, arm)
            except Exception as e:  # noqa: BLE001  (the arm's result stands; retried by hand)
                print(f'{arm}: FARM PAUSE FAILED ({e}): ab.py farm-pause SPEC {arm}', flush=True)
        if not ok:
            raise SystemExit(f'{arm} stopped before checkpoint {last}: go ends here (resume: ab.py launch SPEC {arm}, '
                             f'then go)')
    return judge(s, publish) if judge_too else 0


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0], formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('cmd', choices=['check', 'fork', 'go', 'launch', 'status', 'stop', 'judge', 'publish', 'farm-gates',
                                    'farm-pause'])
    ap.add_argument('spec', help='the experiment spec (JSON)')
    ap.add_argument('arm', nargs='?', help='launch / farm-pause: the arm; farm-gates: one arm (default: every arm)')
    ap.add_argument('--no-judge', action='store_true', help='go: train the arms, then stop (no cells judge)')
    ap.add_argument('--no-publish', action='store_true', help='go / judge: do not post to the Head-to-heads tab')
    ap.add_argument('--dry-run', action='store_true', help='publish: print the record, post nothing')
    ap.add_argument('--replace', action='store_true', help='publish: replace matches already on the site')
    ap.add_argument('--ref', default=None, metavar='CKPT', help='judge / go / check / publish: an outside reference '
                    'every arm is judged against too (docs/ENGINE.md 2XX; kept for a later publish)')
    ap.add_argument('--ref-label', default=None, help="--ref's name on the site (default: its run and generation)")
    a = ap.parse_args(argv)
    try:
        sys.stdout.reconfigure(errors='replace')
    except (AttributeError, ValueError):
        pass
    s = load_spec(a.spec, True)                # (one arm: checked below, once a saved --ref counts too)
    if a.ref or a.ref_label:
        if a.cmd not in ('judge', 'go', 'check', 'publish'):
            ap.error('--ref: judge, go, check or publish')
    s = with_reference(s, a.ref, a.ref_label)
    if len(s['arms']) < 2 and not s['reference']:
        raise SystemExit('spec: one arm needs an outside reference ("reference" in the spec, or --ref CKPT)')
    if a.cmd == 'check':
        check(s)
    elif a.cmd == 'fork':
        fork(s)
    elif a.cmd == 'launch':
        if a.arm not in s['arms']:
            ap.error(f'launch: an arm ({", ".join(s["arms"])})')
        return launch(s, a.arm)
    elif a.cmd == 'go':
        return go(s, not a.no_publish, not a.no_judge)
    elif a.cmd == 'farm-gates':
        if a.arm and a.arm not in s['arms']:
            ap.error(f'farm-gates: an arm ({", ".join(s["arms"])})')
        return max(farm_gates(s, x) for x in ([a.arm] if a.arm else list(s['arms'])))
    elif a.cmd == 'farm-pause':
        if a.arm not in s['arms']:
            ap.error(f'farm-pause: an arm ({", ".join(s["arms"])})')
        farm_pause(s, a.arm)
    elif a.cmd == 'status':
        status(s)
    elif a.cmd == 'stop':
        stop(s)
    elif a.cmd == 'judge':
        return judge(s, not a.no_publish)
    elif a.cmd == 'publish':
        return publish_judge(s, a.dry_run, a.replace)
    return 0


if __name__ == '__main__':
    sys.exit(main())
