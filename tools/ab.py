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
  python tools/ab.py judge   SPEC [--no-publish]   # the cells matches + the metrics table -> <out>/judge/, published
  python tools/ab.py publish SPEC [--dry-run] [--replace]  # (again) the judge's results to the Head-to-heads tab

THE SPEC (JSON):
  {
    "id": "meta3-adaptive-budget",            # the experiment: h2h.<id> on the site; arm runs <run_prefix><arm>
    "title": "...", "description": "...",    # the site's experiment title / description
    "source": {"run": "E:/mtg-zero/runs/legacy-meta3-r1", "iter": 207},   # iter: a number or "newest"
    "iterations": 28,                         # each arm trains K+1 .. K+iterations
    "arms": {"ctrl": {"p_full_rule": "random"}, "adapt": {...}},           # name -> setting overrides (--set)
    "control": "ctrl",                        # default: the first arm; every other arm is a treatment
    "labels": {"ctrl": "Coin (control)", "adapt": "Prior-uncertainty budget"},   # names on the site (default: names)
    "common": {"selfplay_remote": "", ...},   # overrides of every arm (not `publish`, not `iters`: ab.py's)
    "runs_dir": "E:/mtg-zero/runs",           # default: the source run's parent
    "run_prefix": "meta3-ab-",                # default "<id>-"
    "out": ".",                               # judge/ and ab_state.json here (relative: to the spec's directory)
    "code": {"worktree": "...", "sha": "887d83c0", "python": "..."},       # default: this checkout, this python
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

PUBLISHING (default on; `--no-publish`, or "publish": {"h2h": false}): the judge posts one Head-to-heads experiment
h2h.<id> through delverai's tools/upload_h2h.py --manifest (mtgz.az.site_h2h finds it): for each treatment the overall
match (the mean of the cells) and one match per cell (treatment piloting each deck against the control piloting each
deck), and each arm's overall match against the start. A cell's Elo difference is the treatment's GAIN over the control
in that seat on the same deals (not a raw score; the uploader's notes say so, its docstring has the mapping). The tab
lists the treatments' overall matches first, then the arms against the start, then the cells (keys ranked 9 / 5 / 1;
one date: when the judge finished). Labels only - "<label>, gen N" - never a path. A post that fails says PUBLISH
FAILED with the retry command (`ab.py publish SPEC`), exit 3; the judge's files stand.

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
def load_spec(path):
    with open(path, encoding='utf-8') as f:
        spec = json.load(f)
    return normalize(spec, os.path.dirname(os.path.abspath(path)))


def normalize(spec, base='.'):
    """The spec with its defaults filled in -> dict; SystemExit naming what is wrong."""
    s = dict(spec)
    for k in ('id', 'title', 'source', 'iterations', 'arms'):
        if k not in s:
            raise SystemExit(f'spec: no {k!r}')
    if not S.NAME_RE.match(str(s['id'])):
        raise SystemExit(f"spec: id {s['id']!r}: letters, digits, . _ - (at most 60)")
    if not isinstance(s['arms'], dict) or len(s['arms']) < 2:
        raise SystemExit('spec: arms {name: overrides} - at least two')
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
    s['code'] = code
    s['env'] = {str(k): str(v) for k, v in (s.get('env') or {}).items()}
    s['require_source_idle'] = s.get('require_source_idle', True)
    s['judge'] = dict(JUDGE_DEFAULTS, **(s.get('judge') or {}))
    if s['judge']['games'] % 2:
        raise SystemExit('spec: judge.games - an even number (games per cell)')
    pub = dict({'h2h': True, 'live': True, 'live_options': {}, 'link': None, 'url': None}, **(s.get('publish') or {}))
    pub['link'] = pub['link'] or os.path.basename(os.path.normpath(src['run']))
    s['publish'] = pub
    return s


def worktree(s):
    return s['code']['worktree'] or ROOT


def python(s):
    return s['code']['python'] or sys.executable


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
    return o


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
def check(s):
    wt = worktree(s)
    head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=wt, text=True).strip()
    if s['code']['sha'] and not head.startswith(s['code']['sha']):
        raise SystemExit(f"{wt} is at {head[:12]}, not {s['code']['sha']}")
    if not any(f.startswith('_engine') and f.endswith(('.pyd', '.so')) for f in os.listdir(os.path.join(wt, 'mtgz'))):
        raise SystemExit(f'{wt}: no mtgz/_engine build (bash tools/build_py.sh there)')
    k = source_k(s)
    src = s['source']['run']
    if k is None or not os.path.isfile(os.path.join(src, 'models', f'iter_{k:04d}.pt')):
        raise SystemExit(f'{src}: no checkpoint iter_{k}')
    st, live = supervisor(src)
    print(f"A/B {s['id']} ({s['title']}): source {src} at {k}; its supervisor {st.get('state')!r} pid {st.get('pid')} "
          f"({'LIVE' if live else 'not running'})")
    for arm in s['arms']:
        role = 'control' if arm == s['control'] else 'treatment'
        print(f"  {arm} ({role}, {s['labels'][arm]!r}): {run_dir(s, arm)} iters {k + 1}..{k + s['iterations']}: "
              f"{json.dumps(overrides(s, arm, k))}")
    print(f"code: {wt} at {head[:12]}; python {python(s)}; out {s['out']}")
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
        cmd = [python(s), 'tools/fork_run.py', '--source', s['source']['run'], '--dest', d, '--iter', str(k), '--copy']
        ov = overrides(s, arm, k)
        for key, v in ov.items():
            cmd += ['--set', f'{key}={json.dumps(v)}']
        print(subprocess.list2cmdline(cmd), flush=True)
        subprocess.run(cmd, cwd=worktree(s), check=True)
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
    env = dict(os.environ, **s['env'])
    env.pop('CUDA_VISIBLE_DEVICES', None)
    py = python(s)
    with open(os.path.join(d, 'launch_out.txt'), 'a', encoding='utf-8') as lf:
        r = subprocess.run([py, 'tools/launch_hidden.py', os.path.join(d, 'supervisor_console.txt'), py,
                            'tools/supervise.py', d], cwd=worktree(s), env=env, stdin=subprocess.DEVNULL, stdout=lf,
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
            subprocess.run([python(s), 'tools/pause_run.py', d, '--wait'], cwd=worktree(s))
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
    for a in s['arms']:
        if not os.path.isfile(fin[a]):
            raise SystemExit(f'{a}: no checkpoint {last} yet ({fin[a]})')
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


def go(s, publish=True):
    k, live = check(s)
    if publish and s['publish']['h2h'] and not (os.path.isfile(S.uploader_path()) and S.has_key()):
        raise SystemExit(f'the judge publishes (publish.h2h) but the uploader ({S.uploader_path()}) or the upload key '
                         f'is missing: fix it, or go --no-publish (then ab.py publish SPEC later)')
    if live and s['require_source_idle']:
        raise SystemExit(f"{s['source']['run']} is live: pause it first (the arms need the GPU), or set "
                         f'"require_source_idle": false')
    k = fork(s, k)
    last = k + s['iterations']
    for arm in [s['control']] + s['treatments']:
        if (newest(run_dir(s, arm)) or 0) >= last:
            print(f'{arm}: complete already')
            continue
        launch(s, arm)
        if not wait(s, arm, last):
            raise SystemExit(f'{arm} stopped before checkpoint {last}: go ends here (resume: ab.py launch SPEC {arm}, '
                             f'then go)')
    return judge(s, publish)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0], formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('cmd', choices=['check', 'fork', 'go', 'launch', 'status', 'stop', 'judge', 'publish'])
    ap.add_argument('spec', help='the experiment spec (JSON)')
    ap.add_argument('arm', nargs='?', help='launch: the arm')
    ap.add_argument('--no-publish', action='store_true', help='go / judge: do not post to the Head-to-heads tab')
    ap.add_argument('--dry-run', action='store_true', help='publish: print the record, post nothing')
    ap.add_argument('--replace', action='store_true', help='publish: replace matches already on the site')
    a = ap.parse_args(argv)
    try:
        sys.stdout.reconfigure(errors='replace')
    except (AttributeError, ValueError):
        pass
    s = load_spec(a.spec)
    if a.cmd == 'check':
        check(s)
    elif a.cmd == 'fork':
        fork(s)
    elif a.cmd == 'launch':
        if a.arm not in s['arms']:
            ap.error(f'launch: an arm ({", ".join(s["arms"])})')
        return launch(s, a.arm)
    elif a.cmd == 'go':
        return go(s, not a.no_publish)
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
