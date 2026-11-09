"""The position lab (docs/ENGINE.md 76): take a spot from a real game of the Delver mirror, and for each
candidate play, play games to the end from there with the network on both sides. The candidates of a
spot share their hidden-card deals (the same deal seeds: paired), and the result is each candidate's
score. CPU only (CUDA_VISIBLE_DEVICES=-1), below-normal priority; checkpoints are only read (pass
COPIES: the seats' search settings come from <copy>/../../config.json, the run's config.json).

  python tools/position_lab.py spot --model M (--replay R.json.gz --frame N | --site-id ID | --record F[:i])
        [--modes belief,actual,pinned --hold "Force of Will" --exclude Daze] [--top-k 3]
        [--cand "Activate Flooded Strand"] [--cand "Cast Mishra's Bauble;Pass priority;..."]
        [--cand '{"name": "...", "forced": [...], "follow": {"rule": "bauble_fetch", "land_min": 4}}']
        [--games 300 --sims 32 --view-sims 64 --view-reps 4] [--out report.json]
  python tools/position_lab.py bauble --model M --record spots.jsonl [--games 300] [--out demo.json]
  python tools/position_lab.py show --report report.json                (the text table again)

Input spots:
* `--replay` + `--frame`: a zero-replay/1 file (runs/<run>/replays/*.json.gz, mtgz.az.replays) and the
  index of one of its frames - the spot is the state before that frame's move;
* `--site-id`: a delverai.greggg230.com replay id, mapped to the local file the publisher saved by its
  log (`--publish-log`, default E:/mtg-zero-scratch/delverai-publish.log: each "recording N games of
  iteration I (<checkpoint>)" block's "uploaded {... 'id': ...}" lines, in game order, and the files
  <run>/replays/<recorded HHMMSS>_<game>.json.gz whose title is "Generation I");
* `--record`: a JSON object (or a .jsonl file's line i, `F:i`, every line with `bauble --record F`) with
  "replay" (a path, or "file": a name in `--replays`, default the main line's replays) and "frame"
  ("k" also accepted: measure-play's frame index).

Deal modes (the spec's start block, engine/src/az/lab.rs): `belief` (the search's deal: everything the
decider cannot see dealt again, its opponent's hand weighted by the belief head's prediction at the
spot, tilt / clip / mix of the run's evaluation seats), `actual` (the opponent's real hand and every
known card kept, only the library orders nobody knows dealt again: a hindsight check), `pinned`
(`belief` with --hold / --exclude: the opponent's hand holds those, none of these), `uniform` (the
uniform re-deal, for comparison). The decider's own unknown library order is dealt again in every mode.

Candidates: the network's `--top-k` options by prior, every `--cand` (an option label, a ';'-separated
forced sequence of the decider's labels - '*' its own choice there - or a JSON object with "forced" and
an optional "follow" rule). `bauble`: per spot (a fetch crack the network made with a Mishra's Bauble in
hand or on the battlefield) (a) the actual play, (b) the Bauble at its own library first and the network
on, (c) the same with the follow rule (crack iff the seen card is a land and the decider has
--land-min lands in play + hand).

API (the CLI's own functions, importable: `import position_lab as L`): `build_position` (a replay + frame, a
site id, a record, a scenario or an opening seed), `save_scenario` / `load_scenario` (position-lab/1 JSON),
`search_params` + `evaluate_position` (prior, value, searches with explicit settings: sims, chance_k, c_puct,
the root belief, the leaf view, a root floor, the rule, seeds), `sweep` (a grid of them), `play_out`
(candidates from a position in one deal mode, progress and cancel callbacks), `run_lab` (the whole lab).

Report per spot, mode and candidate: score (win 1, draw 0.5) +- its se, the difference to the best
candidate +- the paired se (the same deals), games, mean final turn and turns played from the spot; the
network's view of the spot (prior per option, value, the belief's expected opponent hand, a
--view-sims search's visits and Q per option with the evaluation seat's settings, --view-reps seeds);
pooled over spots for `bauble`.
"""
import os

os.environ['CUDA_VISIBLE_DEVICES'] = '-1'   # CPU only (an empty string does not hide the GPU on Windows)

import argparse                        # noqa: E402
import collections                     # noqa: E402
import glob                            # noqa: E402
import gzip                            # noqa: E402
import json                            # noqa: E402
import math                            # noqa: E402
import random                          # noqa: E402
import re                              # noqa: E402
import sys                             # noqa: E402
import time                            # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)

import mtgz.az as A                    # noqa: E402,F401  (puts the pinned zero-core on sys.path)

FETCHES = ('Flooded Strand', 'Misty Rainforest', 'Scalding Tarn', 'Polluted Delta')
BAUBLE = "Mishra's Bauble"
PASS = 'Pass priority'
REPLAYS = os.environ.get('MTGZ_LAB_REPLAYS', r'C:/Users/Gregg Keithley/mtg-zero/runs/r3-2b-beliefsearch/replays')
# The engine's play changed on purpose at this time (epoch seconds): a replay recorded before it
# can't be rebuilt action for action (the decisions differ: e.g. ENGINE.md 100.3 orders triggers of
# different objects of one card, which the old engine offered once). Bump it with every commit that
# re-records the behaviour digests; replays recorded since are the ones `check_spot` must rebuild.
RULES_EPOCH = 1794262624  # 2026-11-09 14:17:04 -0800: the loop rule reads the board (MTR 4.4, ENGINE.md 100.7; digests 3d59118e6fb9e706)
PUBLISH_LOG = r'E:/mtg-zero-scratch/delverai-publish.log'
MODES = ('belief', 'actual', 'pinned', 'uniform')


def low_priority(threads):
    import torch
    torch.set_num_threads(threads)
    try:
        import psutil
        psutil.Process().nice(psutil.BELOW_NORMAL_PRIORITY_CLASS if os.name == 'nt' else 10)
    except Exception:                              # noqa: BLE001
        pass


def engine():
    from zero.games import engine as e
    return e()


# ------------------------------------------------------------------ spots

def load_replay(path):
    op = gzip.open if path.endswith('.gz') else open
    with op(path, 'rt', encoding='utf-8') as f:
        return json.load(f)


def replay_seed(rep):
    """The game seed of a zero-replay/1 game (zero.replay's driver: Random(seed), one draw per game
    before it, the game's slot stream, its third draw; tools/root_view.py's `rebuild`)."""
    rng = random.Random(rep['seed'])
    for _ in range(rep['game_index']):
        rng.randrange(1 << 30)
    srng = random.Random(rng.randrange(1 << 30))
    srng.randrange(1 << 30)
    srng.randrange(1 << 30)
    return srng.getrandbits(63)


def spot_from_replay(rep, frame, source=''):
    """The spot before frame `frame`'s move: the source spec (the replay's, without its log), the game
    seed, the actions of the frames before it."""
    frames = rep['frames']
    if not 0 <= frame < len(frames):
        raise SystemExit(f'{source}: frame {frame} of {len(frames)}')
    f = frames[frame]
    if f.get('k') != 'nn':
        raise SystemExit(f'{source}: frame {frame} is not a network decision ({f.get("k")})')
    spec = json.loads(rep['spec'])
    spec.pop('log', None)
    m = re.search(r'Generation (\d+)', rep.get('title') or '')
    return {'id': f'{os.path.basename(source)}@{frame}', 'source': source, 'frame': frame, 'spec': spec,
            'seed': replay_seed(rep), 'actions': [int(x['a']) for x in frames[:frame]], 'decider': int(f['p']),
            'recorded': {k: f.get(k) for k in ('acts', 'n', 'p0', 'q', 'v', 'a', 'sims')},
            'state': rep['states'][f['s']], 'gen': int(m.group(1)) if m else None, 'title': rep.get('title')}


def site_index(log_path=PUBLISH_LOG):
    """{site id: (run dir, iteration, game index, recorded HHMMSS)} from the publisher's log."""
    out, cur = {}, None
    rec = re.compile(r'^(\d\d):(\d\d):(\d\d) recording (\d+) games of iteration (\d+) \((.*)\)\s*$')
    done = re.compile(r'^(\d\d):(\d\d):(\d\d) recorded \d+ games')
    up = re.compile(r"^\d\d:\d\d:\d\d\s+uploaded \{.*'id': '([0-9a-f]{12})'")
    with open(log_path, encoding='utf-8', errors='replace') as f:
        for line in f:
            m = rec.match(line)
            if m:
                ck = m.group(6).replace('\\', '/')
                cur = {'run': os.path.dirname(os.path.dirname(ck)), 'iter': int(m.group(5)), 'k': 0, 'hms': None}
                continue
            m = done.match(line)
            if m and cur is not None:
                cur['hms'] = m.group(1) + m.group(2) + m.group(3)
                continue
            m = up.match(line)
            if m and cur is not None:
                out[m.group(1)] = (cur['run'], cur['iter'], cur['k'], cur['hms'])
                cur['k'] += 1
    return out


def replay_for_site_id(sid, log_path=PUBLISH_LOG):
    """The local replay file of a site replay id (`site_index`; the recorded time +- 2 s, the title's
    generation and the game index checked)."""
    idx = site_index(log_path)
    if sid not in idx:
        raise SystemExit(f'site id {sid}: not in the publisher log {log_path}')
    run, it, k, hms = idx[sid]
    if hms is None:
        raise SystemExit(f'site id {sid}: its recording block has no "recorded" line')
    secs = int(hms[:2]) * 3600 + int(hms[2:4]) * 60 + int(hms[4:])
    for d in (0, -1, 1, -2, 2):
        s = (secs + d) % 86400
        stamp = f'{s // 3600:02d}{s % 3600 // 60:02d}{s % 60:02d}'
        for path in sorted(glob.glob(os.path.join(run, 'replays', f'*-{stamp}_{k:03d}.json.gz'))):
            rep = load_replay(path)
            if rep.get('game_index') == k and re.search(rf'Generation {it}\b', rep.get('title') or ''):
                return path
    raise SystemExit(f'site id {sid}: no local file for {run} iteration {it} game {k} recorded {hms}')


def read_records(arg):
    """`--record`: a JSON object, a .json file of one, or a .jsonl file (`F:i` its line i, else all)."""
    if arg.lstrip().startswith('{'):
        return [json.loads(arg)]
    m = re.match(r'^(.*\.jsonl?):(\d+)$', arg)
    path, line = (m.group(1), int(m.group(2))) if m else (arg, None)
    with open(path, encoding='utf-8') as f:
        if path.endswith('.jsonl'):
            recs = [json.loads(x) for x in f if x.strip()]
        else:
            d = json.load(f)
            recs = d if isinstance(d, list) else [d]
    return recs if line is None else [recs[line]]


def save_scenario(spot, path):
    """A spot as a scenario file (JSON): the source spec, seed and action prefix, the decider, its options
    and stored state; `load_scenario` reads it back (the lab's input format for built positions)."""
    keep = ('id', 'source', 'frame', 'spec', 'seed', 'actions', 'decider', 'recorded', 'state', 'gen', 'title', 'hand')
    with open(path, 'w', encoding='utf-8') as f:
        json.dump({'format': 'position-lab/1', **{k: spot.get(k) for k in keep}}, f)


def load_scenario(path):
    with open(path, encoding='utf-8') as f:
        d = json.load(f)
    if d.get('format') != 'position-lab/1':
        raise SystemExit(f'{path}: not a position-lab/1 scenario')
    d.pop('format')
    return d


def spot_from_record(rec, replays=REPLAYS):
    path = rec.get('replay') or os.path.join(replays, rec['file'])
    frame = rec.get('frame', rec.get('k'))
    if frame is None:
        raise SystemExit(f'record {rec}: no "frame"')
    s = spot_from_replay(load_replay(path), int(frame), path)
    s['record'] = rec
    return s


def start_spec(spot, start, log=False):
    """The spot's spec with a start block (the source game: the spot's seed and actions)."""
    sp = dict(spot['spec'])
    if log:
        sp['log'] = True
    sp['start'] = dict({'seed': spot['seed'], 'actions': spot['actions']}, **start)
    return json.dumps(sp, separators=(',', ':'))


def spot_game(spot, log=False):
    """The game at the spot, nothing dealt again (deal "none")."""
    return engine().Game('mtg', start_spec(spot, {'deal': 'none'}, log=log), 2, 0)


def check_spot(spot):
    """The rebuilt spot is the replay's frame: its stored state (the log too), decider, options."""
    g = spot_game(spot, log=True)
    st = json.loads(g.export_state())
    st.pop('lab', None)
    if 'known' not in spot['state']:
        # (the spectator view's `known` block, view-known 7a53953: view only - a replay recorded before it
        # has none; nothing in a game depends on it)
        st.pop('known', None)
    if st != spot['state']:
        diff = sorted(k for k in set(st) | set(spot['state']) if st.get(k) != spot['state'].get(k))
        raise SystemExit(f'{spot["id"]}: the rebuilt spot differs from the replay frame in {diff}')
    acts = spot['recorded'].get('acts')
    if g.to_move != spot['decider'] or (acts is not None and list(g.legal()) != list(acts)):
        raise SystemExit(f'{spot["id"]}: the rebuilt spot has another decision ({g.to_move}, {g.legal()} vs {acts})')
    return g


def options(g):
    st = json.loads(g.export_state())
    return [(int(o['a']), o['t']) for o in (st.get('decision') or {}).get('options', [])]


# ------------------------------------------------------------------ the network

def load_net(path):
    from zero.infer import NetEvaluator
    from zero.net import check_belief_head, load
    m, _ = load(path, 'cpu', game='mtg')
    return NetEvaluator(m, 'cpu'), m, check_belief_head


def seat_keys(model_path):
    """The run's evaluation seat settings (zero.h2h.run_rule: move rule, leaf view, root belief) and its
    c_puct; refuses a checkpoint outside a run directory (no config.json: not the run's search)."""
    from zero.h2h import run_config, run_rule
    cfg = run_config(model_path)
    if not cfg:
        raise SystemExit(f'{model_path}: no run config.json at ../../config.json (copy the run\'s config next to the copy)')
    return cfg, dict(run_rule(model_path)), float(cfg.get('c_puct', 1.0))


def evaluate(ev, rows, acts, belief=False):
    import numpy as np
    rows = np.stack([np.asarray(r, np.float32) for r in rows])
    n = len(acts)
    off = np.zeros(n + 1 + (1 if belief else 0), np.int32)
    off[1:n + 1] = np.cumsum([len(a) for a in acts])
    if belief:
        off[-1] = off[n]
        rows = np.concatenate([rows, rows[-1:]], 0)
    extra = np.zeros(len(rows), np.uint8)
    if belief:
        extra[-1] = 2
    flat = np.concatenate([np.asarray(a, np.int32) for a in acts]) if n else np.zeros(0, np.int32)
    return ev.evaluate(rows, np.full(len(rows), 2, np.int32), off, flat, extra if belief else None)


def belief_pred(ev, g, p):
    """The network's auxiliary prediction (aux_width floats) for player p at g (the root belief row)."""
    import numpy as np
    out = evaluate(ev, [g.encode(p)], [list(g.legal())], belief=True)
    return np.asarray(out[3][0], np.float32)


def search(games, ev, sims, seeds, keys, c_puct, preds=None, chance_k=0, floor=None):
    """One Rust tree per game with the evaluation seat's settings (root-view leaves, the root belief,
    the move rule), `sims` simulations each in lockstep (root_view.py's search_all); `chance_k` (an int,
    or one per tree): the tree's chance-node replays (zero-core docs/search.md 3: an edge holding >= k
    sampled outcome keys replays one of them instead of sampling afresh - the search keeps building on
    hypothetical states after a hidden outcome; the run's searches use 0); `floor` (one per tree): a root
    minimum of visits per option instead of the seat's (zero-core docs/search.md 3.5's forced walks)."""
    import numpy as np
    ks = list(chance_k) if isinstance(chance_k, (list, tuple)) else [int(chance_k)] * len(games)
    rv = keys.get('leaf_value_view', 'mover') == 'root'
    trees = []
    for i, (g, s) in enumerate(zip(games, seeds)):
        t = g.search_tree(True, int(s))
        if floor is not None and floor[i]:
            t.set_root_min(int(floor[i]))
        elif keys.get('root_min_visits'):
            t.set_root_min(int(keys['root_min_visits']) if len(g.legal()) <= keys.get('root_min_visits_max_actions', 4) else 0)
        if keys.get('belief_tilt'):
            t.set_belief(np.asarray(preds[i], np.float32), g, float(keys['belief_tilt']), float(keys['belief_clip']),
                         float(keys['belief_mix']))
        trees.append(t)
    for _ in range(sims):
        rows, acts, owners = [], [], []
        for j, (g, t) in enumerate(zip(games, trees)):
            r = t.simulate(g, c_puct, chance_k=ks[j], root_view=rv)
            if r is None:
                continue
            x, a = r[1], r[2]
            vx = r[3] if rv else None
            rows.append(x)
            acts.append(list(a))
            owners.append((j, len(rows) - 1, None if vx is None else len(rows)))
            if vx is not None:
                rows.append(vx)
                acts.append([])
        if not rows:
            continue
        pri, val, mar = evaluate(ev, rows, acts)[:3]
        off = np.zeros(len(acts) + 1, np.int64)
        off[1:] = np.cumsum([len(a) for a in acts])
        for j, b, vb in owners:
            v = b if vb is None else vb
            trees[j].expand([float(x) for x in pri[off[b]:off[b + 1]]], [float(x) for x in val[v]], [float(x) for x in mar[v]])
    sel = (keys.get('select', 'visits'), int(keys.get('select_min_visits', 0)), float(keys.get('cq_c_visit', 50.0)),
           float(keys.get('cq_c_scale', 1.0)))
    out = []
    for t in trees:
        nv, w = list(t.root_visits), list(t.root_values)
        out.append({'acts': list(t.root_acts), 'n': nv, 'q': [w[a] / nv[a] if nv[a] else None for a in range(len(nv))],
                    'p0': list(t.root_priors0), 'rule': t.root_select(*sel)})
    return out


def net_view(g, spot, ev, keys, c_puct, sims=64, reps=4):
    """The network at the spot: prior and value, the belief's expected opponent hand against the real one,
    and `reps` searches of `sims` simulations with the evaluation seat's settings."""
    import numpy as np
    d = spot['decider']
    opts = options(g)
    names = dict(opts)
    pri, val, _ = evaluate(ev, [g.encode(d)], [[a for a, _ in opts]])[:3]
    pred = belief_pred(ev, g, d)
    st = json.loads(g.export_state())
    hand = collections.Counter(c['name'] for c in st['players'][1 - d]['hand'])
    n_hand = sum(hand.values())
    # the belief's expected count per card of the opponent's hand: n x the head's distribution
    from mtgz.az import belief as B
    exp = {name: round(n_hand * float(p), 3) for name, p in B.counts_by_name(pred, top=40, digits=8) if n_hand * p >= 0.005}
    exp = dict(sorted(exp.items(), key=lambda kv: -kv[1])[:12])
    res = search([g] * reps, ev, sims, [1000 + r for r in range(reps)], keys, c_puct, [pred] * reps) if sims else []
    view = {'decider': d, 'turn': st.get('turn'), 'step': st.get('step'), 'value': round(float(val[0][0]), 4),
            'prior': {names[a]: round(float(p), 4) for (a, _), p in zip(opts, pri)},
            'belief_hand': exp, 'opponent_hand': dict(hand), 'opponent_hand_size': n_hand}
    if res:
        acts = res[0]['acts']
        view['search'] = {'sims': sims, 'reps': reps,
                          'visits': {names.get(a, a): round(float(np.mean([r['n'][i] for r in res])), 1) for i, a in enumerate(acts)},
                          'q': {names.get(a, a): (round(float(np.mean([r['q'][i] for r in res if r['q'][i] is not None])), 4)
                                                  if any(r['q'][i] is not None for r in res) else None) for i, a in enumerate(acts)},
                          'rule_moves': dict(collections.Counter(names.get(acts[r['rule']], acts[r['rule']]) for r in res))}
    return view, pred




# ------------------------------------------------------------------ candidates

def is_crack(label):
    return label.startswith('Activate ') and label[9:].split(' [')[0] in FETCHES


def is_bauble(label):
    return label in ('Cast ' + BAUBLE, 'Activate ' + BAUBLE)


def deep_settings(ks, floor):
    """The deep searches: the evaluation seat's search at each chance_k of `ks` (largest first), then with a
    root floor of `floor` visits per option (0: none) at each - the floor is the only way an option of prior
    ~0 is searched at all (a PUCT root never visits it at any budget), so it is how 'what is Bauble-first
    worth after the look' gets measured."""
    out = [{'name': f'k={k}', 'chance_k': k, 'floor': 0} for k in ks]
    if floor:
        out += [{'name': f'k={k} floor {floor}', 'chance_k': k, 'floor': floor} for k in ks]
    return out


def deep_view(g, spot, ev, keys, c_puct, pred, sims=800, settings=None, reps=8):
    """The network's search at the spot at a large budget, per setting (`deep_settings`), `reps` seeds each
    (the evaluation seat's settings otherwise: rule, root-view leaves, the root belief): per option the mean
    visit share and the mean Q (over the seeds that visited it), per seed the rule's move; the shares and
    Q of 'crack a fetch now' and 'Mishra's Bauble first'; the modal move of the rule (`pick`). Every
    setting's trees run in one lockstep batch."""
    import numpy as np
    names = dict(options(g))
    settings = settings or deep_settings((8, 0), 0)
    m = len(g.legal())
    games, seeds, kk, ff, which = [], [], [], [], []
    for si, st in enumerate(settings):
        fl = min(int(st['floor']), sims // (2 * m)) if st['floor'] else 0      # at most half the budget
        for r in range(reps):
            games.append(g)
            seeds.append(7001 + 977 * r)       # the same seeds for every setting: paired searches
            kk.append(int(st['chance_k']))
            ff.append(fl)
            which.append(si)
    t0 = time.time()
    res = search(games, ev, sims, seeds, keys, c_puct, [pred] * len(games), chance_k=kk, floor=ff)
    out = {'sims': sims, 'reps': reps, 'seconds': round(time.time() - t0, 1), 'by_setting': {}}
    for si, st in enumerate(settings):
        rr = [r for r, x in zip(res, which) if x == si]
        acts = rr[0]['acts']
        lab = [names.get(a, str(a)) for a in acts]
        share = [float(np.mean([r['n'][i] / max(1, sum(r['n'])) for r in rr])) for i in range(len(acts))]
        q = [(float(np.mean([r['q'][i] for r in rr if r['q'][i] is not None])) if any(r['q'][i] is not None for r in rr)
              else None) for i in range(len(acts))]
        moves = collections.Counter(lab[r['rule']] for r in rr)
        sel = lambda f: [i for i, x in enumerate(lab) if f(x)]  # noqa: E731
        crack, baub = sel(is_crack), sel(is_bauble)
        best = lambda ix: max((q[i] for i in ix if q[i] is not None), default=None)  # noqa: E731
        groups = {}
        for gname, f in (('bauble', is_bauble), ('crack', is_crack), ('play fetch', lambda x: x.startswith('Play ') and x[5:] in FETCHES),
                         ('play Island-type', lambda x: x.startswith('Play ') and x[5:] in ('Island', 'Volcanic Island')),
                         ('pass', lambda x: x == PASS)):
            ix = sel(f)
            if ix:
                groups[gname] = {'share': round(sum(share[i] for i in ix), 4), 'q': r4(best(ix))}
        out['by_setting'][st['name']] = {
            'chance_k': st['chance_k'], 'floor': ff[which.index(si)],
            'visit_share': {x: round(v, 4) for x, v in zip(lab, share)},
            'q': {x: (round(v, 4) if v is not None else None) for x, v in zip(lab, q)},
            'crack_share': round(sum(share[i] for i in crack), 4), 'bauble_share': round(sum(share[i] for i in baub), 4),
            'crack_q': r4(best(crack)), 'bauble_q': r4(best(baub)),
            'rule_moves': dict(moves), 'pick': moves.most_common(1)[0][0], 'groups': groups}
    return out


def cached_deep_view(cache, si, g, spot, ev, keys, c_puct, pred, deep, model_path):
    """`deep_view`, kept in the cache directory (keyed by everything it depends on): a resumed lab adds the
    same deep-pick candidates as the first run."""
    import hashlib
    key = hashlib.sha256(json.dumps({'spot': [spot['source'], spot['frame'], spot['seed'], spot['actions']], 'deep': deep,
                                     'keys': keys, 'c_puct': c_puct, 'model': model_id(model_path),
                                     'pred': [float(x) for x in pred]}, sort_keys=True).encode('utf-8')).hexdigest()
    path = os.path.join(cache, f'deep_{si:02d}.json') if cache else None
    if path and os.path.exists(path):
        with open(path, encoding='utf-8') as f:
            d = json.load(f)
        if d.get('key') == key:
            return d['view']
    v = deep_view(g, spot, ev, keys, c_puct, pred, deep['sims'], deep['settings'], deep['reps'])
    if path:
        os.makedirs(cache, exist_ok=True)
        with open(path + '.tmp', 'w', encoding='utf-8') as f:
            json.dump({'key': key, 'view': v}, f)
        os.replace(path + '.tmp', path)
    return v


def parse_cand(text):
    """A --cand: a JSON object {"name", "forced", "follow"}, or a ';'-separated label sequence."""
    text = text.strip()
    if text.startswith('{'):
        c = json.loads(text)
        if not c.get('forced') and not c.get('follow'):
            raise SystemExit(f'--cand {text}: neither a "forced" line nor a "follow" rule')
        forced = c.get('forced') or []
        return {'name': c.get('name') or ' ; '.join(map(str, forced)) or 'the rule', 'forced': forced, 'follow': c.get('follow')}
    items = [x.strip() for x in text.split(';') if x.strip()]
    return {'name': ' ; '.join(items), 'forced': [label_item(x) for x in items], 'follow': None}


def label_item(label):
    """A forced item for an option label of `options` (a disambiguated 'text [a]' is its action id)."""
    m = re.match(r'^(.*) \[(\d+)\]$', label)
    return int(m.group(2)) if m else label


def top_k(view, k):
    return [{'name': lab, 'forced': [label_item(lab)], 'follow': None}
            for lab, _ in sorted(view['prior'].items(), key=lambda kv: -kv[1])[:k]]


def bauble_line(g, d):
    """The forced line 'Mishra's Bauble at the decider's own library, the look resolved' at the spot:
    cast it first if it is in hand (and let it resolve), activate it, target the decider, let the look
    resolve ("resolve": pass while anything is on the stack - a Dragon's Rage Channeler's or a Cori-Steel
    Cutter's trigger above the Bauble -, the network answering their questions)."""
    labels = [t for _, t in options(g)]
    line = []
    if 'Activate ' + BAUBLE not in labels:
        if 'Cast ' + BAUBLE not in labels:
            raise SystemExit("the spot has no Mishra's Bauble to cast or activate")
        line += ['Cast ' + BAUBLE, 'resolve']
    return line + ['Activate ' + BAUBLE, f'Target P{d + 1}', 'resolve']


def bauble_candidates(spot, g, land_min, view=None, deep_modes=None):
    """(a) the actual play (the frame's move), (b) the Bauble line then the network, (c) the line with the
    follow rule (crack iff the seen card is a land and the decider has >= land_min lands); with a deep view
    (`deep_view`), the move each deep setting picks, forced alone and the network on - (d), (e), ... in the
    settings' order - unless it is another candidate already (then the report says which), played in
    `deep_modes` only."""
    names = dict(options(g))
    actual = names[spot['recorded']['a']]
    line = bauble_line(g, spot['decider'])
    cands = [{'name': f'(a) {actual} (the actual play)', 'forced': [label_item(actual)], 'follow': None},
             {'name': '(b) Bauble at own library first, network on', 'forced': line, 'follow': None},
             {'name': f'(c) Bauble first + rule (crack iff land & >= {land_min} lands)', 'forced': line,
              'follow': {'rule': 'bauble_fetch', 'land_min': land_min, 'fetches': list(FETCHES)}}]
    deep = (view or {}).get('deep')
    if deep:
        tags = iter('defghij')
        for name, dv in deep['by_setting'].items():
            pick = dv['pick']
            same = next((c['name'] for c in cands if c['forced'] == [label_item(pick)] and not c.get('follow')), None)
            if same:
                dv['candidate'] = same
                continue
            tag = next(tags)
            dv['candidate'] = f'({tag})'
            c = {'name': f'({tag}) {pick} (the deep {name} search\'s pick), network on', 'forced': [label_item(pick)],
                 'follow': None}
            if deep_modes:
                c['modes'] = list(deep_modes)
            cands.append(c)
    return cands


def start_block(mode, cand, deal_seed, pred=None, belief=None, hold=(), exclude=(), top=None, top_known=False):
    """The start block of one game: deal mode, deal seed, the candidate's line and rule; `top` ([[P1's], [P2's]]
    card names, top first: the deal puts them on top of the libraries, `top_known`: their owners know them)."""
    st = {'deal_seed': int(deal_seed), 'forced': cand['forced']}
    if top and any(top):
        st['top'] = [list(top[0]), list(top[1])]
        st['top_known'] = bool(top_known)
    if cand.get('follow'):
        st['follow'] = cand['follow']
    if mode in ('belief', 'pinned'):
        tilt, clip, mix = belief
        st.update(deal='belief', pred=[float(x) for x in pred], tilt=tilt, clip=clip, mix=mix)
    else:
        st['deal'] = mode
    if mode == 'pinned':
        if not hold and not exclude:
            raise SystemExit('mode pinned needs --hold and/or --exclude')
        st['hold'] = list(hold)
        st['exclude'] = list(exclude)
    return st


# ------------------------------------------------------------------ play-outs

def deal_seed(si, i):
    """Deal i of spot si: the same for every candidate and mode of the spot (paired)."""
    return 1_000_003 * si + i


class Cancelled(Exception):
    """A play-out stopped by its `cancel` callback."""


def play_one(spot, modes, cand, ci, si, ev, keys, c_puct, sims, games, seed=1, conc=512, threads=4, move_log=0,
             progress=None, cancel=None):
    """One candidate at one spot: every mode x `games` deals in one zero-core driver, queued setups in the
    order (mode, deal i) - every candidate's driver has the same seed and the same order, so game gid of
    one candidate and of another are the same deal AND the same search streams (they part where the
    games do: zero-core docs/search.md 2.3). `progress(done, total)` is called after every network batch,
    `cancel()` -> True stops the driver (Cancelled). -> ({tag: row}, perf, {tag: spec})."""
    from zero import games as G
    from zero.infer import drive
    seat = G.nn_seat('m', int(sims), c_puct, chance_k=0)
    seat.update(keys)
    setups = [(f'{si}|{mode}|{ci}|{i}', start_spec(spot, start_block(mode, cand, deal_seed(si, i), **kw)))
              for mode, kw in modes for i in range(games)]
    if not setups:
        raise SystemExit('no games to play')
    kw = {'move_log': move_log} if move_log else {}
    # a rolling queue-only driver (zero-core docs/search.md 2.9): its slots start nothing but the queued
    # setups (a classic driver starts its first games with the driver's spec at construction)
    drv = G.Driver('mtg', [[seat, seat]], spec=json.dumps(spot['spec']), concurrency=min(conc, len(setups)), seed=seed,
                   threads=threads, groups=2, max_steps=20000, rolling=True, queue_only=True, **kw)
    for tag, sp in setups:
        drv.enqueue(tag, sp)
    drv.resume(len(setups), 0)
    t0 = time.time()
    s0 = getattr(ev, 'seconds', 0.0)
    rows = {}

    def take(lb=None):
        for r in drv.take_results():
            if r.get('tag') is None:
                raise SystemExit(f"game {r['gid']} played the driver's spec, not a queued setup")
            rows[r['tag']] = r
        if progress is not None:
            progress(len(rows), len(setups))
        if cancel is not None and cancel():
            raise Cancelled(f'{len(rows)} of {len(setups)} games played')
    drive(drv, {'m': ev}, on_batch=take)
    take()
    if len(rows) != len(setups):
        raise SystemExit(f'{len(rows)} results for {len(setups)} games')
    st = drv.stats()
    perf = {'seconds': round(time.time() - t0, 1), 'games': len(rows), 'leaves': st.get('leaves'),
            'view_rows': st.get('view_rows'), 'belief_rows': st.get('belief_rows'),
            'cut': sum(1 for r in rows.values() if not r['over']),
            'net_seconds': round(getattr(ev, 'seconds', 0.0) - s0, 1), 'driver_wait_s': round(st.get('wait_s', 0.0), 1)}
    return rows, perf, dict(setups)


_WORKER = {}


def _worker_init(model_path, threads):
    low_priority(threads)
    _WORKER['ev'] = load_net(model_path)[0]
    _WORKER['model_id'] = model_id(model_path)


def _worker_job(job):
    """A worker process's (spot, candidate, mode) job: the play-outs, then each game's line diagnosis;
    with a cache directory the job's rows are written there (a long lab survives a stop: `--cache`)."""
    spot, mode, cand, ci, si, keys, c_puct, sims, games, seed, conc, threads, diag, cache = job
    rows, perf, specs = play_one(spot, [mode], cand, ci, si, _WORKER['ev'], keys, c_puct, sims, games, seed, conc, threads,
                                 move_log=150 if diag else 0)
    infos = None
    if diag and (len(cand['forced']) > 1 or cand.get('follow')):
        t0 = time.time()
        infos = {t: diagnose(specs[t], rows[t], spot['decider']) for t in rows}
        perf['diagnose_s'] = round(time.time() - t0, 1)
    for r in rows.values():
        r.pop('moves', None)
    out = (si, ci, mode[0], rows, perf, infos)
    if cache:
        path = cache_path(cache, job)
        with open(path + '.tmp', 'w', encoding='utf-8') as f:
            json.dump({'key': cache_key(job), 'rows': rows, 'perf': perf, 'infos': infos}, f)
        os.replace(path + '.tmp', path)
    return out


def cache_key(job):
    """What a job's games depend on (a cached job is reused only if every part is the same)."""
    import hashlib
    spot, mode, cand, ci, si, keys, c_puct, sims, games, seed, conc, threads, diag, cache = job
    key = {'spot': [spot['source'], spot['frame'], spot['seed'], spot['actions']], 'mode': mode, 'cand': cand, 'ci': ci,
           'si': si, 'keys': keys, 'c_puct': c_puct, 'sims': sims, 'games': games, 'seed': seed, 'diag': diag,
           'model': _WORKER.get('model_id')}
    return hashlib.sha256(json.dumps(key, sort_keys=True).encode('utf-8')).hexdigest()


def cache_path(cache, job):
    return os.path.join(cache, f'job_{job[4]:02d}_{job[3]}_{job[1][0]}.json')


def model_id(path):
    import hashlib
    with open(path, 'rb') as f:
        return hashlib.sha256(f.read()).hexdigest()[:16]


def play(jobs, model_path, keys, c_puct, sims, games, seed, conc, threads, search_threads, procs, diag, cache=None,
         log=print, cached_only=False, play_cands=None):
    """Every (spot, candidate, mode) of `jobs` ([(spot, [(mode, start kwargs)], candidates)]) in `procs`
    worker processes (each: `threads` torch threads, `search_threads` search threads, below-normal
    priority), jobs found in `cache` (same key) reused. -> (rows by tag, line infos by tag, perfs)."""
    from concurrent.futures import ProcessPoolExecutor, as_completed
    _WORKER['model_id'] = model_id(model_path)
    # mode-major (the first mode - belief by default - complete for every spot first), then spot, candidate
    order = {}
    for _, md, _ in jobs:
        for m, _ in md:
            order.setdefault(m, len(order))
    work = sorted([(spot, m, cand, ci, si, keys, c_puct, sims, games, seed, conc, search_threads, diag, cache)
                   for si, (spot, md, cands) in enumerate(jobs) for ci, cand in enumerate(cands) for m in md
                   if m[0] in cand.get('modes', [m[0]])],
                  key=lambda w: (order[w[1][0]], w[4], w[3]))
    rows, infos, perfs = {}, {}, []
    todo = []
    if cache:
        os.makedirs(cache, exist_ok=True)
    for w in work:
        path = cache_path(cache, w) if cache else None
        if path and os.path.exists(path):
            with open(path, encoding='utf-8') as f:
                d = json.load(f)
            if d.get('key') == cache_key(w):
                rows.update(d['rows'])
                infos.update(d.get('infos') or {})
                perfs.append(dict(d['perf'], spot=w[4], candidate=w[3], mode=w[1][0], cached=True))
                continue
        todo.append(w)
    if len(todo) < len(work):
        log(f'  {len(work) - len(todo)} of {len(work)} jobs from the cache {cache}')
    if cached_only:
        log(f'  --cached-only: {len(todo)} jobs not played')
        todo = []
    if play_cands is not None:
        skip = [w for w in todo if w[3] not in play_cands]
        if skip:
            log(f'  --play-cands {sorted(play_cands)}: {len(skip)} jobs of other candidates not played')
        todo = [w for w in todo if w[3] in play_cands]
    t0 = time.time()
    if todo:
        with ProcessPoolExecutor(max_workers=max(1, min(procs, len(todo))), initializer=_worker_init,
                                 initargs=(model_path, threads)) as ex:
            futs = [ex.submit(_worker_job, w) for w in todo]
            for k, f in enumerate(as_completed(futs)):
                si, ci, mode, r, perf, inf = f.result()
                rows.update(r)
                infos.update(inf or {})
                perfs.append(dict(perf, spot=si, candidate=ci, mode=mode))
                log(f'  [{k + 1}/{len(todo)}] spot {si} candidate {ci} {mode}: {perf["games"]} games in {perf["seconds"]} s'
                    f' ({perf["games"] / max(perf["seconds"], 1e-9):.2f}/s); all {time.time() - t0:.0f} s'
                    + (f'; {perf["cut"]} cut at max_steps' if perf['cut'] else ''))
    return rows, infos, perfs


def diagnose(spec, row, decider):
    """Replay a lab game's logged moves (zero-core's move_log) until its forced line and rule settled:
    whether the line held, the card on top of the decider's library after the line (what the Bauble
    showed), the rule's verdict, and whether the decider activated a fetch while that card was still on
    top (a crack over the seen card)."""
    from zero import mcts
    E = engine()
    g = E.Game('mtg', spec, 2, 0)
    info = {'cracked_over_seen': None, 'top_after_line': None}
    top, lib_n = None, None
    for (p, a, legal) in row.get('moves') or []:
        if g.over:
            break
        if list(g.legal()) != list(legal) or g.to_move != p:
            raise SystemExit(f'diagnose: the replayed lab game left its move log ({g.legal()} vs {legal})')
        if g.to_move == decider:
            lab = json.loads(g.export_state())['lab']
            if lab['forced_left'] == 0 and top is None:
                libs = lab['libraries'][decider]
                top, lib_n = (libs[0] if libs else None), len(libs)
                info['top_after_line'] = top
            if top is not None:
                t = dict(options(g)).get(a, '')
                if t.startswith('Activate ') and t[9:].split(' [')[0] in FETCHES:
                    info['cracked_over_seen'] = True
                    break
        mcts.transition(g, a)
        if top is not None:
            libs = json.loads(g.export_state())['lab']['libraries'][decider]
            if len(libs) < lib_n or not libs or libs[0] != top:
                info['cracked_over_seen'] = False
                break
    lab = json.loads(g.export_state())['lab']
    info.update(broken=lab['broken'], seen=lab['seen'], seen_bad=lab['seen_bad'], rule_cracked=lab['cracked'])
    return info


def stats(vals):
    n = len(vals)
    if not n:
        return None, None
    m = sum(vals) / n
    var = sum((v - m) ** 2 for v in vals) / (n - 1) if n > 1 else 0.0
    return m, math.sqrt(var / n)


def r4(x):
    return None if x is None else round(x, 4)


def summarize(spot, mode, cands, rows, games, si, here=None):
    """Per candidate (of `here`, default all; None for the others): score +- se, the paired differences to
    the best and to the first (the actual play), the per-deal differences to the first (for pooling)."""
    d = spot['decider']
    turn0 = spot['state'].get('turn') or 0
    here = list(range(len(cands))) if here is None else list(here)
    by = [[rows[f'{si}|{mode}|{ci}|{i}'] for i in range(games)] if ci in here else None for ci in range(len(cands))]
    scores = [[float(r['result'][d]) for r in rs] if rs else None for rs in by]
    means = [stats(s)[0] if s else None for s in scores]
    best = max(here, key=lambda c: means[c])
    out = []
    for ci, cand in enumerate(cands):
        if ci not in here:
            out.append(None)
            continue
        m, se = stats(scores[ci])
        rs = by[ci]
        rec = {'candidate': cand['name'], 'games': len(rs), 'score': r4(m), 'se': r4(se),
               'final_turn': round(sum(r['rounds'] for r in rs) / len(rs), 2),
               'turns_from_spot': round(sum(r['rounds'] - turn0 for r in rs) / len(rs), 2),
               'decisions': round(sum(r['decisions'] for r in rs) / len(rs), 1),
               'unfinished': sum(1 for r in rs if not r['over']), 'best': ci == best}
        for ref, key in ((best, 'vs_best'), (0, 'vs_first')):
            dm, dse = stats([a - b for a, b in zip(scores[ci], scores[ref])])
            rec[key] = {'diff': r4(dm), 'se_paired': r4(dse), 'pairs': games}
        out.append(rec)
    diffs = [[a - b for a, b in zip(scores[ci], scores[0])] if scores[ci] else None for ci in range(len(cands))]
    return out, diffs


def pooled(spots_out, mode, n_cands, pick=None):
    """Over the spots that played `mode` (equal weight): the mean score per candidate, and the paired
    difference to the first candidate - the mean of the spots' mean differences, its se stratified by
    spot (sqrt(sum of the spots' squared paired se) / spots: deals independent across spots). `pick`:
    instead of candidate index ci, `pick(spot report, ci)` names each spot's candidate (None: skip)."""
    out = []
    for ci in range(n_cands):
        sc, means, ses, pairs = [], [], [], 0
        for so in spots_out:
            res = so['modes'].get(mode)
            cj = pick(so, ci) if pick else ci
            if not res or cj is None or cj >= len(res['candidates']) or res['candidates'][cj] is None:
                continue
            sc.append(res['candidates'][cj]['score'])
            m, se = stats(res['_diffs'][cj])
            means.append(m)
            ses.append(se)
            pairs += len(res['_diffs'][cj])
        k = len(means)
        out.append({'candidate': ci, 'spots': len(sc), 'score': r4(sum(sc) / len(sc)) if sc else None,
                    'vs_first': {'diff': r4(sum(means) / k) if k else None,
                                 'se_paired': r4(math.sqrt(sum(x * x for x in ses)) / k) if k else None, 'pairs': pairs}})
    return out


def deep_pick_index(so, setting):
    """The index of the candidate that is a deep setting's pick at a spot (its own (d), (e), ..., or the
    candidate it already is), or None."""
    dv = ((so['view'].get('deep') or {}).get('by_setting') or {}).get(setting)
    if not dv or not dv.get('candidate'):
        return None
    name = dv['candidate']
    for ci, c in enumerate(so['candidates']):
        if c == name or c.startswith(name + ' '):
            return ci
    return None


def deal_summary(spot, si, md, cand, games, top=10):
    """The deals the play-outs use, per mode (and the uniform deal for reference): the share of deals in
    which the decider's opponent holds each card (the `top` most frequent), and the real hand."""
    E = engine()
    o = 1 - spot['decider']
    out = {}
    for mode, kw in list(md) + ([('uniform', {})] if all(m != 'uniform' for m, _ in md) else []):
        c = collections.Counter()
        for i in range(games):
            g = E.Game('mtg', start_spec(spot, start_block(mode, cand, deal_seed(si, i), **kw)), 2, 0)
            c.update(set(x['name'] for x in json.loads(g.export_state())['players'][o]['hand']))
        out[mode] = {k: round(v / games, 3) for k, v in c.most_common(top)}
    return out


def line_summary(infos, scores):
    """A forced line's games: how often it held, what the Bauble showed, the rule's verdicts (and their
    scores), how often the network itself cracked a fetch over the seen card."""
    held = [k for k, i in enumerate(infos) if i['broken'] is None]
    seen = collections.Counter(infos[k].get('seen') or infos[k].get('top_after_line') for k in held)
    out = {'games': len(infos), 'line_held': len(held), 'seen': dict(seen.most_common(8))}
    if any(infos[k]['seen_bad'] is not None for k in held):
        bad = [k for k in held if infos[k]['seen_bad']]
        good = [k for k in held if infos[k]['seen_bad'] is False]
        out['rule'] = {'bad': len(bad), 'good': len(good),
                       'score_bad': r4(sum(scores[k] for k in bad) / len(bad)) if bad else None,
                       'score_good': r4(sum(scores[k] for k in good) / len(good)) if good else None,
                       'rule_cracked': sum(infos[k]['rule_cracked'] for k in held)}
    cr = [infos[k]['cracked_over_seen'] for k in held if infos[k]['cracked_over_seen'] is not None]
    if cr:
        out['network_cracked_over_seen_card'] = r4(sum(cr) / len(cr))
        out['cracked_over_seen_n'] = len(cr)
    return out


def run_lab(spots, cand_fn, model_path, modes, hold, exclude, games, sims, view_sims, view_reps, threads, search_threads,
            procs, conc, seed, log=print, diagnose_lines=True, cache=None, cached_only=False, deep=None, play_cands=None):
    """The lab for every spot: its checks and the network's view, then every play-out."""
    ev, model, check_head = load_net(model_path)
    cfg, keys, c_puct = seat_keys(model_path)
    from zero.mcts import belief_settings
    tilt, clip, mix, _ = belief_settings(cfg, 'eval')
    belief = (tilt or 1.0, clip, mix)
    if keys.get('belief_tilt') or 'belief' in modes or 'pinned' in modes:
        check_head(model, 'mtg', f'the belief deal / belief_tilt of {model_path}')
    out = {'model': model_path, 'seat': keys, 'c_puct': c_puct, 'sims': sims, 'games': games, 'modes': modes,
           'hold': list(hold), 'exclude': list(exclude), 'belief': dict(zip(('tilt', 'clip', 'mix'), belief)),
           'threads': {'procs': procs, 'torch': threads, 'search': search_threads}, 'spots': []}
    jobs = []
    t0 = time.time()
    for spot in spots:
        g = check_spot(spot)
        view, pred = net_view(g, spot, ev, keys, c_puct, view_sims, view_reps)
        if deep and deep.get('sims'):
            view['deep'] = cached_deep_view(cache, len(jobs), g, spot, ev, keys, c_puct, pred, deep, model_path)
            log(f'  deep search {deep["sims"]} x {deep["reps"]}: ' + '; '.join(
                f'{k}: pick {v["pick"]} (crack {v["crack_share"]:.3f} q {v["crack_q"]}, Bauble {v["bauble_share"]:.3f} q {v["bauble_q"]})'
                for k, v in view['deep']['by_setting'].items()) + f' ({view["deep"]["seconds"]} s)')
        cands = cand_fn(spot, g, view)
        for c in cands:           # the engine refuses an ambiguous or unknown first label: say so before any play-out
            try:
                engine().Game('mtg', start_spec(spot, start_block('none', c, 0)), 2, 0)
            except Exception as e:                              # noqa: BLE001
                raise SystemExit(f'{spot["id"]}: candidate {c["name"]!r}: {e}')
        md, skipped = [], {}
        for mode in modes:
            kw = {'pred': [float(x) for x in pred], 'belief': belief} if mode in ('belief', 'pinned') else {}
            if mode == 'pinned':
                kw.update(hold=hold, exclude=exclude)
                try:     # a pin no deal can hold at this spot: the spot skips the mode (said in the report)
                    engine().Game('mtg', start_spec(spot, start_block(mode, cands[0], 0, **kw)), 2, 0)
                except Exception as e:                          # noqa: BLE001
                    skipped[mode] = str(e)
                    continue
            md.append((mode, kw))
        jobs.append((spot, md, cands))
        deals = deal_summary(spot, len(jobs) - 1, md, cands[0], games)
        out['spots'].append({'id': spot['id'], 'source': spot['source'], 'frame': spot['frame'], 'gen': spot.get('gen'),
                             'record': spot.get('record'), 'site_id': spot.get('site_id'), 'view': view,
                             'hand': spot.get('hand'),
                             'candidates': [c['name'] for c in cands], 'lines': cands, 'skipped_modes': skipped,
                             'deals': deals, 'modes': {}})
        log(f'spot {spot["id"]}: checked; network view at {time.time() - t0:.0f} s; candidates {[c["name"] for c in cands]}'
            + (f'; skipped {skipped}' if skipped else ''))
    n = sum(len(m) * len(c) for _, m, c in jobs) * games
    log(f'play-outs: {n} games at {sims} simulations per move, {procs} processes')
    t1 = time.time()
    rows, infos, perfs = play(jobs, model_path, keys, c_puct, sims, games, seed, conc, threads, search_threads, procs,
                              diagnose_lines, cache=cache, log=log, cached_only=cached_only, play_cands=play_cands)
    out['perf'] = {'games': len(rows), 'seconds': round(time.time() - t1, 1), 'jobs': perfs,
                   'cut': sum(p['cut'] for p in perfs)}
    for si, (spot, md, cands) in enumerate(jobs):
        so = out['spots'][si]
        d = spot['decider']
        for mode, _ in md:
            here = [ci for ci, c in enumerate(cands) if mode in c.get('modes', [mode])]
            if not all(f'{si}|{mode}|{ci}|{i}' in rows for ci in here for i in range(games)):
                so.setdefault('incomplete_modes', []).append(mode)    # (--cached-only: not every job played yet)
                continue
            res, diffs = summarize(spot, mode, cands, rows, games, si, here)
            # the decider's per-deal results of each candidate (deal i: the same world for all)
            entry = {'candidates': res, '_diffs': diffs,
                     'results': [[float(rows[f'{si}|{mode}|{ci}|{i}']['result'][d]) for i in range(games)]
                                 if ci in here else None for ci in range(len(cands))]}
            if diagnose_lines:
                entry['lines'] = []
                for ci, cand in enumerate(cands):
                    tags = [f'{si}|{mode}|{ci}|{i}' for i in range(games)]
                    if ci not in here or not all(t in infos for t in tags):
                        entry['lines'].append(None)
                        continue
                    entry['lines'].append(line_summary([infos[t] for t in tags], [float(rows[t]['result'][d]) for t in tags]))
            so['modes'][mode] = entry
    out['seconds'] = round(time.time() - t0, 1)
    return out


# ------------------------------------------------------------------ the turn-1 Bauble position (built from seeds)

ISLANDS = ('Island', 'Volcanic Island')
TAMIYO = 'Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar'
T1_ALLOWED = ('Force of Will', 'Daze', 'Cori-Steel Cutter', 'Murktide Regent', 'Prismari Charm')


def t1_composition(hand):
    """'clean': exactly one Island-type land, one fetchland, one Mishra's Bauble, one Tamiyo and every other card
    from T1_ALLOWED (none castable on our turn 1; Force of Will and Daze reactive only); 'any': at least one
    of each of the four; else None."""
    c = collections.Counter(hand)
    isl, fe = sum(c[x] for x in ISLANDS), sum(c[x] for x in FETCHES)
    if not (isl and fe and c[BAUBLE] and c[TAMIYO]):
        return None
    rest = sum(v for k, v in c.items() if k not in ISLANDS + FETCHES + (BAUBLE, TAMIYO))
    ok_rest = sum(v for k, v in c.items() if k in T1_ALLOWED)
    return 'clean' if (isl, fe, c[BAUBLE], c[TAMIYO]) == (1, 1, 1, 1) and rest == ok_rest else 'any'


def opening_spot(spec, seed):
    """The starting player's first main-phase priority on turn 1 of game `seed` of `spec` (a dict): the
    chooser plays first, both keep their seven, priority passes until then (no other decision on the way);
    None when the starting player gets no turn-1 decision. -> a spot (as `spot_from_replay`)."""
    from zero import mcts
    E = engine()
    sp = dict(spec, log=True)
    g = E.Game('mtg', json.dumps(sp), 2, int(seed))
    acts = []
    for _ in range(40):
        if g.over:
            return None
        st = json.loads(g.export_state())
        d = st['decision']
        if st['turn'] > 1:
            return None
        if d['kind'] == 'Priority' and st['step'] == 'main1' and st['active'] == g.to_move:
            return {'id': f'seed{seed}', 'source': f'seed:{seed}', 'frame': len(acts), 'spec': dict(spec), 'seed': int(seed),
                    'actions': acts, 'decider': int(g.to_move), 'recorded': {'acts': list(g.legal()), 'a': None},
                    'state': st, 'gen': None, 'title': f'opening of seed {seed}',
                    'hand': sorted(c['name'] for c in st['players'][g.to_move]['hand'])}
        want = {'PlayDraw': 'Play first', 'Mulligan': 'Keep', 'Priority': PASS}.get(d['kind'])
        a = next((x for x, t in options(g) if t == want), None)
        if a is None:
            return None
        acts.append(int(a))
        mcts.transition(g, a)
    return None


def opening_spot_draw(spec, seed, ev, keys, c_puct, sims=32):
    """The second player's first main-phase priority (turn 2, its hand with the draw) of game `seed`: the
    chooser plays first, both keep, the starting player's turn 1 played by the network (the evaluation
    seat's search, `sims` simulations, a fixed seed per decision), priority otherwise passing. None when the
    game does not get there."""
    from zero import mcts
    E = engine()
    sp = dict(spec, log=True)
    g = E.Game('mtg', json.dumps(sp), 2, int(seed))
    acts = []
    for k in range(200):
        if g.over:
            return None
        st = json.loads(g.export_state())
        d = st['decision']
        if st['turn'] > 2:
            return None
        if st['turn'] == 2 and d['kind'] == 'Priority' and st['step'] == 'main1' and st['active'] == g.to_move:
            return {'id': f'seed{seed}d', 'source': f'seed:{seed}:draw', 'frame': len(acts), 'spec': dict(spec), 'seed': int(seed),
                    'actions': acts, 'decider': int(g.to_move), 'recorded': {'acts': list(g.legal()), 'a': None},
                    'state': st, 'gen': None, 'title': f'opening of seed {seed}, on the draw',
                    'hand': sorted(c['name'] for c in st['players'][g.to_move]['hand'])}
        if st['turn'] == 1 and g.to_move == st['active'] and d['kind'] not in ('PlayDraw', 'Mulligan'):
            pred = belief_pred(ev, g, g.to_move) if keys.get('belief_tilt') else None
            r = search([g], ev, sims, [9000 + k], keys, c_puct, [pred] if pred is not None else None)[0]
            a = r['acts'][r['rule']]
        else:
            want = {'PlayDraw': 'Play first', 'Mulligan': 'Keep', 'Priority': PASS}.get(d['kind'])
            a = next((x for x, t in options(g) if t == want), None)
            if a is None:
                a = options(g)[0][0]
        acts.append(int(a))
        mcts.transition(g, a)
    return None


def find_t1_spots(spec, seeds, want=8, composition='clean'):
    """Opening spots on the play whose starting hand is `composition` ('clean' or 'any'), the first `want`."""
    out = []
    for s in seeds:
        sp = opening_spot(spec, s)
        if sp and t1_composition(sp['hand']) in ((composition,) if composition == 'clean' else ('clean', 'any')):
            out.append(sp)
            if len(out) >= want:
                break
    return out


def t1_lines(g, d):
    """The turn-1 lines' labels at an opening spot: the Island-type land, the fetchland, Tamiyo's cast."""
    labels = [t for _, t in options(g)]
    isl = next(t for t in labels if t.startswith('Play ') and t[5:] in ISLANDS)
    fe = next(t for t in labels if t.startswith('Play ') and t[5:] in FETCHES)
    crack = [fe, 'Activate ' + fe[5:], 'resolve', 'Cast ' + TAMIYO, 'resolve']
    keep = [isl, 'Cast ' + TAMIYO, 'resolve']
    look = ['Cast ' + BAUBLE, 'resolve', 'Activate ' + BAUBLE, f'Target P{d + 1}', 'resolve']
    return crack, keep, look


def t1_candidates(spot, g, view=None, deep_modes=None):
    """(a) the network's own play; (b) no Bauble: crack the fetch, Tamiyo; (c) no Bauble: the Island-type land
    + Tamiyo, the fetch kept; (d) Mishra's Bauble at the own library first, then the network; (e) the same
    + the branch rule (a land seen: the fetch, cracked, Tamiyo; a spell seen: the land, Tamiyo, the fetch
    kept); with a deep view, each deep setting's pick (forced alone, the network on) unless it is already a
    candidate's line."""
    crack, keep, look = t1_lines(g, spot['decider'])
    cands = [{'name': "(a) the network's own play", 'forced': ['*'], 'follow': None},
             {'name': '(b) no Bauble: crack the fetch, Tamiyo', 'forced': crack, 'follow': None},
             {'name': '(c) no Bauble: Island-type land + Tamiyo, fetch kept', 'forced': keep, 'follow': None},
             {'name': '(d) Bauble own library first, network on', 'forced': look, 'follow': None},
             {'name': '(e) Bauble first + branch (land seen: crack; else land + Tamiyo)', 'forced': look,
              'follow': {'rule': 'bauble_fetch', 'land_min': 0, 'fetches': list(FETCHES), 'bad': crack, 'good': keep}}]
    deep = (view or {}).get('deep')
    if deep:
        tags = iter('fghijk')
        for name, dv in deep['by_setting'].items():
            pick = dv['pick']
            same = next((c['name'] for c in cands[1:] if c['forced'][:1] == [label_item(pick)] and len(c['forced']) == 1), None)
            if same:
                dv['candidate'] = same
                continue
            tag = next(tags)
            dv['candidate'] = f'({tag})'
            c = {'name': f'({tag}) {pick} (the deep {name} search\'s pick), network on', 'forced': [label_item(pick)],
                 'follow': None}
            if deep_modes:
                c['modes'] = list(deep_modes)
            cands.append(c)
    return cands


def t1_groups(label):
    """The turn-1 option classes of the deep view."""
    if label == 'Cast ' + BAUBLE:
        return 'bauble'
    if label.startswith('Play ') and label[5:] in FETCHES:
        return 'fetch'
    if label.startswith('Play ') and label[5:] in ISLANDS:
        return 'island'
    return None


def scripted(g, d, items, finds=('Find Volcanic Island', 'Find Island')):
    """Play forced `items` (labels; 'resolve': pass while anything is on the stack) for the decider `d`, the
    opponent passing (or its lowest option), the decider's other questions its first option (mana
    payments) - but a library search, which takes the first of `finds` offered (None: each option in turn,
    a list of games). -> [(game, the find taken)] once the items are done (at the next decision, whoever's,
    with an empty stack; one decision at a time, single-option ones too), or [] when an item is missing."""
    def step1(x, a):
        x.step(int(a))      # one decision at a time (a single-option one too: the line's "resolve" sees it)
    out = []
    work = [(g, list(items), None)]
    while work:
        h, rest, found = work.pop()
        for _ in range(200):
            if h.over:
                break
            st = json.loads(h.export_state())
            dec = st['decision']
            opts = options(h)
            if rest and rest[0] == 'resolve' and not st.get('stack'):
                rest.pop(0)                    # (the stack is empty, whoever decides: the item is done)
                continue
            if h.to_move != d:
                if not rest:
                    out.append((h, found))     # the line is done; the next decision is the opponent's
                    break
                step1(h, next((a for a, t in opts if t == PASS), opts[0][0]))
                continue
            if dec['kind'] == 'SearchLibrary':
                fl = [(a, t) for a, t in opts if t.startswith('Find ') and t != 'Find nothing']
                for a, t in fl[1:] if finds is None else []:
                    h2 = h.clone()
                    step1(h2, a)
                    work.append((h2, list(rest), t))
                want = next((x for x in (finds or ()) if any(t == x for _, t in fl)), None)
                pick = next(a for a, t in fl if t == want) if want else (fl[0][0] if fl else opts[0][0])
                found = want or (fl[0][1] if fl else None)
                step1(h, pick)
                continue
            if not rest:
                if dec['kind'] == 'Priority' and not st.get('stack'):
                    out.append((h, found))
                    break
                step1(h, opts[0][0])
                continue
            item = rest[0]
            if item == 'resolve':
                if not st.get('stack'):
                    rest.pop(0)
                    continue
                step1(h, next((a for a, t in opts if t == PASS), opts[0][0]) if dec['kind'] == 'Priority' else opts[0][0])
                continue
            if dec['kind'] in ('PayMana', 'PayPool'):
                step1(h, opts[0][0])
                continue
            a = next((x for x, t in opts if t == item), None)
            if a is None:
                break
            rest.pop(0)
            step1(h, a)
    return out


def one_ply_t1(spot, ev, pred, belief, deals=200, log=print):
    """The value network alone after a forced Bauble look (the user's one-ply check): for `deals` belief deals
    of the spot, the look line with the opponent passing, then each branch scripted - 'keep it': the Island-type
    land + Tamiyo, the fetch held; 'crack': the fetch, cracked (the better of its finds), Tamiyo - and the
    network's value (its own view) at the decider's next priority after Tamiyo resolved. Per seen card and per
    class (land / spell): the mean value of each branch and the share of deals where the value prefers keep.
    The crack fetches a Volcanic Island (an Island without one), as the expert line does."""
    import numpy as np
    E = engine()
    d = spot['decider']
    g0 = spot_game(spot)
    crack, keep, look = t1_lines(g0, d)
    rows = []
    for i in range(deals):
        st = start_block('belief', {'forced': []}, deal_seed(0, i), pred=pred, belief=belief)
        g = E.Game('mtg', start_spec(spot, st), 2, 0)
        after = scripted(g, d, look)
        if not after:
            continue
        h = after[0][0]
        lib = json.loads(h.export_state())['lab']['libraries'][d]
        seen = lib[0] if lib else None
        vals = {}
        for name, line in (('keep', keep), ('crack', crack)):
            best = None
            for x, _ in scripted(h.clone(), d, line):
                v = float(evaluate(ev, [x.encode(d)], [list(x.legal())])[1][0][0])
                best = v if best is None else max(best, v)
            vals[name] = best
        if vals['keep'] is None or vals['crack'] is None:
            continue
        rows.append({'seen': seen, 'land': seen in ISLANDS + FETCHES + ('Wasteland', 'Thundering Falls'), **vals})
    # (the spot's own value, before the look: the scale the branches move from)
    out = {'deals': deals, 'measured': len(rows), 'by_class': {}, 'by_card': {}}
    for key, f in (('land', lambda r: r['land']), ('spell', lambda r: not r['land'])):
        rs = [r for r in rows if f(r)]
        if rs:
            out['by_class'][key] = {'n': len(rs), 'keep': r4(float(np.mean([r['keep'] for r in rs]))),
                                    'crack': r4(float(np.mean([r['crack'] for r in rs]))),
                                    'prefers_keep': r4(float(np.mean([r['keep'] > r['crack'] for r in rs])))}
    for card, n in collections.Counter(r['seen'] for r in rows).most_common(6):
        rs = [r for r in rows if r['seen'] == card]
        out['by_card'][card] = {'n': n, 'keep': r4(float(np.mean([r['keep'] for r in rs]))),
                                'crack': r4(float(np.mean([r['crack'] for r in rs]))),
                                'prefers_keep': r4(float(np.mean([r['keep'] > r['crack'] for r in rs])))}
    return out


# ------------------------------------------------------------------ the turn-1 Bauble canary (a fingerprint)

def game_seed(driver_seed, gid):
    """zero-core's driver game seed of game `gid` (worker::init_slot's base stream's first draw)."""
    m64 = (1 << 64) - 1

    def split64(x):
        z = (x + 0x9E3779B97F4A7C15) & m64
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & m64
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & m64
        return z ^ (z >> 31)
    s, x = [], split64(driver_seed ^ split64(gid + 1))
    for _ in range(4):
        x = (x + 0x9E3779B97F4A7C15) & m64
        s.append(split64(x))
    if not any(s):
        s[0] = 1
    rotl = lambda v, k: ((v << k) | (v >> (64 - k))) & m64  # noqa: E731
    return (rotl((s[0] + s[3]) & m64, 23) + s[0]) & m64


def t1_canary_game(spec, seed, moves):
    """One game's turn-1 Bauble fingerprint: for each player, at its first main-phase priority (its first
    turn), the hand's composition (`t1_composition`, with the draw on the draw); and when it has one, what it
    did that turn: whether it activated Mishra's Bauble at its own library before its land drop, the card the
    look showed (the library's top when the ability was targeted), and the land it played (Island-type /
    fetch, cracked that turn or not). `moves`: the game's multi-option decisions (seat, action), in order.
    -> [per player record]."""
    from zero import mcts
    E = engine()
    sp = json.loads(spec) if isinstance(spec, str) else dict(spec)
    sp.pop('log', None)
    g = E.Game('mtg', json.dumps(sp), 2, int(seed))
    out, cur, acts = {}, {}, []
    for mv in moves:
        p, a = mv[0], mv[1]
        if len(mv) > 2 and mv[2] is not None and len(mv[2]) == 1:
            continue            # (a logged single-option move: auto-stepped below, as the driver did)
        while not g.over and len(g.legal()) == 1:
            mcts.transition(g, g.legal()[0])
        if g.over:
            break
        st = json.loads(g.export_state())
        if st['turn'] > 2:
            break
        q = g.to_move
        lab = dict(options(g))
        t = lab.get(int(a), '')
        act = st['active']
        if q == act and st['step'] in ('main1', 'main2') and act not in out:
            hand = sorted(c['name'] for c in st['players'][act]['hand'])
            out[act] = {'player': act, 'turn': st['turn'], 'on_play': st['turn'] == 1, 'hand': hand,
                        'composition': t1_composition(hand), 'looked': False, 'seen': None, 'land': None, 'cracked': False,
                        'actions': []}
            cur = out[act]
        r = out.get(act)
        if r is not None and q == act and st['turn'] == r['turn']:
            r['actions'].append(t)
            if t == f'Target P{act + 1}' and st.get('stack') and st['stack'][-1].get('name') == BAUBLE and r['land'] is None:
                lab_g = E.Game('mtg', json.dumps(dict(sp, start={'seed': int(seed), 'actions': acts, 'deal': 'none'})), 2, 0)
                libs = json.loads(lab_g.export_state())['lab']['libraries'][act]
                r['looked'], r['seen'] = True, (libs[0] if libs else None)
            if t.startswith('Play ') and r['land'] is None:
                r['land'] = 'island' if t[5:] in ISLANDS else ('fetch' if t[5:] in FETCHES else 'other')
            if t.startswith('Activate ') and t[9:] in FETCHES:
                r['cracked'] = True
        acts.append(int(a))
        mcts.transition(g, int(a))
    del cur
    return list(out.values())


def t1_canary(sources, log=print):
    """The turn-1 Bauble canary over game records: `sources` [(spec, seed, moves, tag)]. Per composition
    ('any', 'clean') and side (play / draw): hands, how often the player Baubled its own library before its
    land drop, and with a look the land choice by the seen card's class (land / spell)."""
    recs = []
    for src in sources:
        spec, seed, moves, tag = src[:4]
        gen = src[4] if len(src) > 4 else None
        for r in t1_canary_game(spec, seed, moves):
            if r['composition']:
                recs.append(dict(r, tag=tag, gen=gen))
    out = {'records': len(recs), 'table': {}}
    for comp in ('any', 'clean'):
        for side in ('play', 'draw'):
            rs = [r for r in recs if (comp == 'any' or r['composition'] == 'clean') and r['on_play'] == (side == 'play')]
            looked = [r for r in rs if r['looked']]
            by = collections.Counter()
            for r in looked:
                cls = 'land' if r['seen'] in ISLANDS + FETCHES + ('Wasteland', 'Thundering Falls') else 'spell'
                by[f'{cls} seen -> {r["land"]}{" cracked" if r["cracked"] else ""}'] += 1
            nolook = collections.Counter(f'{r["land"]}{" cracked" if r["cracked"] else ""}' for r in rs if not r['looked'])
            out['table'][f'{comp} {side}'] = {'hands': len(rs), 'bauble_look_before_land': len(looked),
                                              'after_a_look': dict(by), 'without_a_look': dict(nolook)}
    out['examples'] = [{k: r[k] for k in ('tag', 'player', 'on_play', 'hand', 'looked', 'seen', 'land', 'cracked', 'actions')}
                       for r in recs if r['composition'] == 'clean'][:20]
    out['records_list'] = [{k: r[k] for k in ('tag', 'gen', 'player', 'on_play', 'composition', 'looked', 'seen', 'land',
                                              'cracked')} for r in recs]
    # the fingerprint per generation (any composition, both sides): hands, Bauble looks before the land drop
    by = collections.defaultdict(lambda: [0, 0, 0])
    for r in recs:
        b = by[r['gen']]
        b[0] += 1
        b[1] += r['looked']
        b[2] += r['composition'] == 'clean'
    out['by_gen'] = {str(k): {'hands': v[0], 'looked': v[1], 'clean': v[2]} for k, v in sorted(by.items(), key=lambda kv: (kv[0] is None, kv[0] or 0))}
    return out


def canary_sources(replays=REPLAYS, selfplay=(), gen_min=None):
    """The records the canary reads: zero-replay/1 files (`replays`: a directory) and self-play move logs
    (`selfplay`: JSON files of zero.h2h-style rows {gid, moves} with the file's spec and driver seed:
    E:/mtg-zero-scratch/explore/ab/fp_*_241.json)."""
    out = []
    for f in sorted(glob.glob(os.path.join(replays, '*.json.gz'))):
        rep = load_replay(f)
        m = re.search(r'Generation (\d+)', rep.get('title') or '')
        if gen_min is not None and (not m or int(m.group(1)) < gen_min):
            continue
        out.append((rep['spec'], replay_seed(rep), [(fm['p'], fm['a']) for fm in rep['frames']], os.path.basename(f),
                    int(m.group(1)) if m else None))
    for path in selfplay:
        with open(path, encoding='utf-8') as fh:
            d = json.load(fh)
        for r in d['rows']:
            out.append((d['spec'], game_seed(int(d['seed']), int(r['gid'])), [tuple(m) for m in r['moves']],
                        f'{os.path.basename(path)}:{r["gid"]}', d.get('iter') or d.get('gen')))
    return out


# ------------------------------------------------------------------ the API (the CLI's own functions; for a UI)

def build_position(replay=None, frame=None, site_id=None, record=None, scenario=None, opening_seed=None,
                   spec=None, check=True):
    """A position (a spot dict, JSON-serializable: `save_scenario`) from a replay + frame, a site replay id
    + frame, a position record, a saved scenario, or an opening seed (`opening_spot`, `spec` default the
    Delver mirror); checked against the replay's stored state when it has one."""
    if scenario:
        s = load_scenario(scenario) if isinstance(scenario, str) else dict(scenario)
    elif opening_seed is not None:
        s = opening_spot(spec or json.loads(A.SPEC_DELVER), int(opening_seed))
        if s is None:
            raise ValueError(f'seed {opening_seed}: the starting player has no turn-1 decision')
    elif record:
        s = spot_from_record(record)
    elif site_id:
        path = replay_for_site_id(site_id)
        s = spot_from_replay(load_replay(path), int(frame), path)
        s['site_id'] = site_id
    elif replay:
        s = spot_from_replay(load_replay(replay), int(frame), replay)
    else:
        raise ValueError('a replay + frame, a site id + frame, a record, a scenario or an opening seed')
    if check:
        check_spot(s)
    return s


def search_params(model_path, **over):
    """The search settings of a position evaluation: the checkpoint's run's evaluation seat (`seat_keys`) with
    `over` applied - sims, chance_k, c_puct, belief_tilt / belief_clip / belief_mix (tilt 0: no root belief),
    leaf_value_view ('mover' | 'root'), root_min_visits (a floor per option), select / select_min_visits /
    cq_c_visit / cq_c_scale, seeds (a list) -> a dict `evaluate_position` takes."""
    _, keys, c_puct = seat_keys(model_path)
    p = {'keys': dict(keys), 'c_puct': c_puct, 'sims': 64, 'chance_k': 0, 'seeds': [1000, 1001, 1002, 1003], 'floor': 0}
    for k, v in over.items():
        if k in ('sims', 'chance_k', 'c_puct', 'seeds', 'floor'):
            p[k] = v
        elif k == 'root_min_visits':
            p['floor'] = int(v)
        else:
            p['keys'][k] = v
    if not p['keys'].get('belief_tilt'):
        for k in ('belief_tilt', 'belief_clip', 'belief_mix', 'belief_fast'):
            p['keys'].pop(k, None)
    return p


def evaluate_position(spot, ev, params, pred=None):
    """The network at a position: per option (keyed by action id - two options may share a label, e.g. two
    "Target Volcanic Island"; the label is a display field) the prior, and one search per seed with `params`
    (`search_params`): per run the visits and Q per action and the rule's choice (an action id). -> dict."""
    g = spot_game(spot)
    d = spot['decider']
    st = json.loads(g.export_state())
    raw = [(int(o['a']), o['t']) for o in (st.get('decision') or {}).get('options', [])]
    pri, val, _ = evaluate(ev, [g.encode(d)], [[a for a, _ in raw]])[:3]
    if pred is None and params['keys'].get('belief_tilt'):
        pred = belief_pred(ev, g, d)
    n = len(params['seeds'])
    res = search([g] * n, ev, int(params['sims']), params['seeds'], params['keys'], params['c_puct'],
                 [pred] * n if pred is not None else None, chance_k=int(params['chance_k']), floor=[int(params['floor'])] * n)
    return {'decider': d, 'value': float(val[0][0]),
            'options': [{'action': a, 'label': t, 'prior': float(p)} for (a, t), p in zip(raw, pri)],
            'runs': [{'seed': sd, 'visits': {str(a): int(n_) for a, n_ in zip(r['acts'], r['n'])},
                      'q': {str(a): q for a, q in zip(r['acts'], r['q'])}, 'choice': int(r['acts'][r['rule']])}
                     for sd, r in zip(params['seeds'], res)]}


def sweep(spot, ev, model_path, grid, pred=None):
    """`evaluate_position` over every combination of `grid` ({param: [values]}, e.g. {'sims': [64, 800],
    'chance_k': [0, 8]}). -> [(the combination, its evaluation)]."""
    import itertools
    keys = list(grid)
    out = []
    for combo in itertools.product(*(grid[k] for k in keys)):
        over = dict(zip(keys, combo))
        out.append((over, evaluate_position(spot, ev, search_params(model_path, **over), pred)))
    return out


def play_out(spot, candidates, model_path, mode='belief', games=100, sims=32, hold=(), exclude=(), seed=1, ev=None,
             progress=None, cancel=None, si=0):
    """Candidates (dicts {'name', 'forced', 'follow'}: option ids or labels, short forced sequences, a follow
    rule) played out from a position in one deal mode, `games` paired deals each, in this process (one
    driver per candidate; `progress(done, total)` / `cancel()` as `play_one`). -> the per-candidate summary
    (`summarize`) and the raw rows."""
    ev = ev or load_net(model_path)[0]
    cfg, keys, c_puct = seat_keys(model_path)
    from zero.mcts import belief_settings
    tilt, clip, mix, _ = belief_settings(cfg, 'eval')
    kw = {}
    if mode in ('belief', 'pinned'):
        kw = {'pred': [float(x) for x in belief_pred(ev, spot_game(spot), spot['decider'])], 'belief': (tilt or 1.0, clip, mix)}
    if mode == 'pinned':
        kw.update(hold=hold, exclude=exclude)
    rows = {}
    total = len(candidates) * games
    for ci, cand in enumerate(candidates):
        def prog(done, _n, ci=ci):
            if progress is not None:
                progress(ci * games + done, total)
        r, _, _ = play_one(spot, [(mode, kw)], cand, ci, si, ev, keys, c_puct, sims, games, seed, progress=prog, cancel=cancel)
        rows.update(r)
    res, diffs = summarize(spot, mode, candidates, rows, games, si)
    return {'candidates': res, 'diffs_vs_first': diffs, 'rows': rows}


# ------------------------------------------------------------------ text

def table(rep):
    lines = []
    for so in rep['spots']:
        v = so['view']
        lines.append(f'== {so["id"]} (gen {so.get("gen")}, P{v["decider"] + 1}, turn {v["turn"]} {v["step"]}; '
                     f'value {v["value"]})')
        pri = ', '.join(f'{k} {p:.3f}' for k, p in sorted(v['prior'].items(), key=lambda kv: -kv[1])[:5])
        lines.append(f'   prior: {pri}')
        if v.get('search'):
            s = v['search']
            lines.append(f'   search {s["reps"]}x{s["sims"]}: ' + ', '.join(
                f'{k} n={n} q={s["q"][k]}' for k, n in sorted(s['visits'].items(), key=lambda kv: -kv[1])[:4])
                + f'; moves {s["rule_moves"]}')
        for k, dv in ((v.get('deep') or {}).get('by_setting') or {}).items():
            gs = ', '.join(f'{gn} {x["share"]:.3f} q {x["q"]}' for gn, x in (dv.get('groups') or {}).items())
            lines.append(f'   deep {k} ({v["deep"]["reps"]}x{v["deep"]["sims"]}): {gs}; rule moves'
                         f' {dv["rule_moves"]} -> {dv.get("candidate")}')
        lines.append('   opponent hand: ' + ', '.join(f'{k} x{c}' for k, c in v['opponent_hand'].items())
                     + ' | belief: ' + ', '.join(f'{k} {e:.2f}' for k, e in list(v['belief_hand'].items())[:6]))
        for m, d in (so.get('deals') or {}).items():
            lines.append(f'   opponent holds ({m} deals): ' + ', '.join(f'{k} {x:.2f}' for k, x in list(d.items())[:7]))
        modes = list(so['modes'])
        lines.append('   {:<50}'.format('candidate (score +- se; vs best +- paired se)') + ''.join(f'{m:>32}' for m in modes))
        for ci, name in enumerate(so['candidates']):
            row = f'   {name[:50]:<50}'
            for m in modes:
                c = so['modes'][m]['candidates'][ci]
                if c is None:
                    row += '-'.rjust(32)
                    continue
                vb = c['vs_best']
                row += f'{c["score"]:.3f}+-{c["se"]:.3f} ({vb["diff"]:+.3f}+-{vb["se_paired"]:.3f})'.rjust(32)
            lines.append(row)
        lines.append('   games {} per cell; mean turns from the spot: {}'.format(
            so['modes'][modes[0]]['candidates'][0]['games'] if modes else 0,
            ', '.join(f'{m} ' + '/'.join(f'{c["turns_from_spot"]:.1f}' for c in so['modes'][m]['candidates'] if c) for m in modes)))
        for m in modes:
            for ci, li in enumerate(so['modes'][m].get('lines') or []):
                if li:
                    lines.append(f'   [{m}] {so["candidates"][ci][:44]}: line held {li["line_held"]}/{li["games"]}'
                                 + (f', rule {li["rule"]}' if li.get('rule') else '')
                                 + (f', the network cracked over the seen card {li["network_cracked_over_seen_card"]:.2f}'
                                    f' of {li["cracked_over_seen_n"]}' if 'network_cracked_over_seen_card' in li else ''))
        if so.get('one_ply'):
            op = so['one_ply']
            lines.append(f'   one-ply value after a forced look ({op["measured"]} of {op["deals"]} belief deals): ' + '; '.join(
                f'{k} seen (n {x["n"]}): keep {x["keep"]} vs crack {x["crack"]}, keep preferred {x["prefers_keep"]:.2f}'
                for k, x in op['by_class'].items()))
            lines.append('      by card: ' + '; '.join(f'{k} (n {x["n"]}): keep {x["keep"]} / crack {x["crack"]}'
                                                   f' -> keep {x["prefers_keep"]:.2f}' for k, x in op['by_card'].items()))
        if so.get('hand'):
            lines.append(f'   hand: {", ".join(so["hand"])}')
        if so.get('skipped_modes'):
            lines.append(f'   skipped: {so["skipped_modes"]}')
        if so.get('incomplete_modes'):
            lines.append(f'   not played yet (--cached-only): {so["incomplete_modes"]}')
    if rep.get('pooled'):
        lines.append('== pooled over spots (equal weight): score; paired difference to (a) over all deals')
        for m, pc in rep['pooled'].items():
            for x in pc:
                if not x['spots']:
                    continue
                vf = x['vs_first']
                lines.append(f'   [{m}] {rep["candidate_names"][x["candidate"]][:50]:<50} {x["score"]:.3f} over {x["spots"]} spots;'
                             f' vs (a) {vf["diff"]:+.4f} +- {vf["se_paired"]:.4f} ({vf["pairs"]} pairs)')
    if rep.get('perf'):
        p = rep['perf']
        th = rep.get('threads') or {}
        cached = sum(1 for j in p.get('jobs', []) if j.get('cached'))
        job_s = sum(j['seconds'] for j in p.get('jobs', []))
        lines.append(f'   play-outs: {p["games"]} games, wall {p["seconds"]} s, {job_s:.0f} job-seconds'
                     f' ({p["games"] / max(job_s, 1e-9):.3f} games per job-second; {th.get("procs")} processes x'
                     f' {th.get("torch")} torch + {th.get("search")} search threads)'
                     + (f'; {cached} jobs from the cache' if cached else '')
                     + f'; whole lab {rep.get("seconds")} s' + (f'; {p["cut"]} games cut at max_steps' if p.get('cut') else ''))
    return '\n'.join(lines)


def strip_private(rep):
    out = json.loads(json.dumps(rep))
    for so in out['spots']:
        for m in so['modes'].values():
            m.pop('_diffs', None)
    return out


# ------------------------------------------------------------------ commands

def spots_of(a):
    if a.replay:
        if a.frame is None:
            raise SystemExit('--replay needs --frame')
        return [spot_from_replay(load_replay(a.replay), a.frame, a.replay)]
    if a.site_id:
        if a.frame is None:
            raise SystemExit('--site-id needs --frame (the frame index of the spot)')
        path = replay_for_site_id(a.site_id, a.publish_log)
        s = spot_from_replay(load_replay(path), a.frame, path)
        s['site_id'] = a.site_id
        return [s]
    if a.record:
        return [spot_from_record(r, a.replays) for r in read_records(a.record)]
    raise SystemExit('give --replay + --frame, --site-id + --frame or --record')


def common(ap):
    ap.add_argument('--model', required=True, help="a COPY of a checkpoint in a copy of its run dir (models/, config.json)")
    ap.add_argument('--replay')
    ap.add_argument('--frame', type=int)
    ap.add_argument('--site-id')
    ap.add_argument('--publish-log', default=PUBLISH_LOG)
    ap.add_argument('--record')
    ap.add_argument('--replays', default=REPLAYS)
    ap.add_argument('--modes', default='belief,actual,pinned')
    ap.add_argument('--hold', action='append', default=[], help='pinned mode: a card the opponent holds (repeatable)')
    ap.add_argument('--exclude', action='append', default=[], help='pinned mode: a card the opponent does not hold')
    ap.add_argument('--games', type=int, default=300, help='deals per spot (each candidate and mode plays every one)')
    ap.add_argument('--sims', type=int, default=32)
    ap.add_argument('--view-sims', type=int, default=64)
    ap.add_argument('--view-reps', type=int, default=4)
    ap.add_argument('--threads', type=int, default=2, help='torch threads per worker process (2: 4 gains nothing)')
    ap.add_argument('--search-threads', type=int, default=1, help='search threads per worker process')
    ap.add_argument('--procs', type=int, default=4, help='worker processes (one driver per spot, candidate and mode)')
    ap.add_argument('--conc', type=int, default=512)
    ap.add_argument('--seed', type=int, default=1)
    ap.add_argument('--no-diagnose', action='store_true')
    ap.add_argument('--cache', help='a directory for each finished job\'s games (a stopped lab resumes from it)')
    ap.add_argument('--cached-only', action='store_true', help='report the cached jobs only, play nothing')
    ap.add_argument('--out')


def finish(rep, a):
    if a.out:
        with open(a.out, 'w', encoding='utf-8') as f:
            json.dump(strip_private(rep), f, indent=1)
        with open(os.path.splitext(a.out)[0] + '.txt', 'w', encoding='utf-8') as f:
            f.write(table(rep) + '\n')
    print(table(rep), flush=True)


def check_modes(a):
    modes = [m for m in a.modes.split(',') if m]
    for m in modes:
        if m not in MODES:
            raise SystemExit(f'--modes: {m} (one of {MODES})')
    if 'pinned' in modes and not (a.hold or a.exclude):
        raise SystemExit('mode pinned needs --hold and/or --exclude')
    return modes


def cmd_spot(a):
    modes = check_modes(a)
    low_priority(a.threads)
    extra = [parse_cand(c) for c in a.cand]

    def cands(spot, g, view):
        cs = top_k(view, a.top_k) if a.top_k else []
        for c in extra:
            if all(c['forced'] != x['forced'] or c.get('follow') != x.get('follow') for x in cs):
                cs.append(c)
        if not cs:
            raise SystemExit('no candidates (--top-k / --cand)')
        return cs
    rep = run_lab(spots_of(a), cands, a.model, modes, a.hold, a.exclude, a.games, a.sims, a.view_sims, a.view_reps,
                  a.threads, a.search_threads, a.procs, a.conc, a.seed, diagnose_lines=not a.no_diagnose, cache=a.cache,
                  cached_only=a.cached_only)
    finish(rep, a)


def cmd_bauble(a):
    modes = check_modes(a)
    low_priority(a.threads)
    ks = sorted({int(x) for x in a.deep_k.split(',') if x.strip()}, reverse=True)
    deep_modes = [m for m in a.deep_modes.split(',') if m]
    settings = deep_settings(ks, a.deep_floor)
    deep = {'sims': a.deep_sims, 'settings': settings, 'reps': a.deep_reps} if a.deep_sims else None
    rep = run_lab(spots_of(a), lambda s, g, v: bauble_candidates(s, g, a.land_min, v, deep_modes), a.model, modes, a.hold,
                  a.exclude, a.games, a.sims, a.view_sims, a.view_reps, a.threads, a.search_threads, a.procs, a.conc, a.seed,
                  diagnose_lines=not a.no_diagnose, cache=a.cache, cached_only=a.cached_only, deep=deep,
                  play_cands=None if a.play_cands is None else {int(x) for x in a.play_cands.split(',') if x.strip()})
    names = ['(a) the actual play (crack now)', '(b) Bauble first, network on', f'(c) Bauble first + rule (land_min {a.land_min})']
    picks = [None, None, None]
    for st in (settings if deep else []):
        names.append(f'the deep {st["name"]} search\'s pick')
        picks.append(st['name'])
    rep['candidate_names'] = names
    rep['pooled'] = {m: pooled(rep['spots'], m, len(names),
                               pick=lambda so, ci: ci if picks[ci] is None else deep_pick_index(so, picks[ci])) for m in modes}
    finish(rep, a)


def cmd_t1(a):
    """The turn-1 Bauble position (seeds searched for the opening hand), candidates (a)-(e) (+ the deep picks),
    the play-outs and the one-ply value check."""
    modes = check_modes(a)
    low_priority(a.threads)
    if a.scenario:
        spots = [load_scenario(x) for x in a.scenario]
    else:
        lo, hi = (int(x) for x in a.seeds.split('-'))
        if a.draw:
            ev0 = load_net(a.model)[0]
            _, keys0, cp0 = seat_keys(a.model)
            spots = []
            for sd in range(lo, hi):
                # (cheap first: the second player's seven and its draw - its library's top - before its turn)
                o = opening_spot(json.loads(A.SPEC_DELVER), sd)
                if o is None:
                    continue
                lab = json.loads(engine().Game('mtg', start_spec(o, {'deal': 'none'}), 2, 0).export_state())['lab']
                q = 1 - o['decider']
                eight = sorted(c['name'] for c in o['state']['players'][q]['hand']) + lab['libraries'][q][:1]
                if t1_composition(eight) not in ((a.composition,) if a.composition == 'clean' else ('clean', 'any')):
                    continue
                sp = opening_spot_draw(json.loads(A.SPEC_DELVER), sd, ev0, keys0, cp0)
                if sp and t1_composition(sp['hand']) in ((a.composition,) if a.composition == 'clean' else ('clean', 'any')):
                    spots.append(sp)
                    if len(spots) >= a.spots:
                        break
        else:
            spots = find_t1_spots(json.loads(A.SPEC_DELVER), range(lo, hi), a.spots, a.composition)
        print(f'{len(spots)} opening spots ({a.composition}) in seeds {a.seeds}: ' + '; '.join(
            f'{s["id"]} P{s["decider"] + 1}: {", ".join(s["hand"])}' for s in spots), flush=True)
        if a.save_spots:
            os.makedirs(a.save_spots, exist_ok=True)
            for s in spots:
                save_scenario(s, os.path.join(a.save_spots, f'{s["id"]}.json'))
    if not spots:
        raise SystemExit('no opening spot found')
    ks = sorted({int(x) for x in a.deep_k.split(',') if x.strip()}, reverse=True)
    deep_modes = [m for m in a.deep_modes.split(',') if m]
    settings = deep_settings(ks, a.deep_floor)
    deep = {'sims': a.deep_sims, 'settings': settings, 'reps': a.deep_reps} if a.deep_sims else None
    rep = run_lab(spots, lambda s, g, v: t1_candidates(s, g, v, deep_modes), a.model, modes, a.hold, a.exclude, a.games,
                  a.sims, a.view_sims, a.view_reps, a.threads, a.search_threads, a.procs, a.conc, a.seed,
                  diagnose_lines=not a.no_diagnose, cache=a.cache, cached_only=a.cached_only, deep=deep,
                  play_cands=None if a.play_cands is None else {int(x) for x in a.play_cands.split(',') if x.strip()})
    if a.one_ply:
        ev = load_net(a.model)[0]
        cfg, _, _ = seat_keys(a.model)
        from zero.mcts import belief_settings
        tilt, clip, mix, _ = belief_settings(cfg, 'eval')
        for s, so in zip(spots, rep['spots']):
            pred = belief_pred(ev, spot_game(s), s['decider'])
            so['one_ply'] = one_ply_t1(s, ev, [float(x) for x in pred], (tilt or 1.0, clip, mix), a.one_ply)
    names = ["(a) the network's own play", '(b) no Bauble: crack, Tamiyo', '(c) no Bauble: Island + Tamiyo, fetch kept',
             '(d) Bauble first, network on', '(e) Bauble first + branch rule']
    picks = [None] * 5
    for st in (settings if deep else []):
        names.append(f'the deep {st["name"]} search\'s pick')
        picks.append(st['name'])
    rep['candidate_names'] = names
    rep['pooled'] = {m: pooled(rep['spots'], m, len(names),
                               pick=lambda so, ci: ci if picks[ci] is None else deep_pick_index(so, picks[ci])) for m in modes}
    finish(rep, a)


def cmd_canary(a):
    """The turn-1 Bauble canary over game records (a fingerprint: run per generation)."""
    gen_min = a.gen_min
    src = canary_sources(a.replays, a.selfplay or (), gen_min)
    t0 = time.time()
    out = t1_canary(src)
    out.update(sources=len(src), seconds=round(time.time() - t0, 1), replays=a.replays, selfplay=a.selfplay, gen_min=gen_min)
    print(json.dumps({k: v for k, v in out.items() if k not in ('examples', 'records_list')}, indent=1))
    if a.out:
        with open(a.out, 'w', encoding='utf-8') as f:
            json.dump(out, f, indent=1)


def cmd_show(a):
    with open(a.report, encoding='utf-8') as f:
        print(table(json.load(f)))


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest='cmd', required=True)
    s = sub.add_parser('spot', help='candidates at one or more spots')
    common(s)
    s.add_argument('--top-k', type=int, default=3)
    s.add_argument('--cand', action='append', default=[])
    s.set_defaults(fn=cmd_spot)
    b = sub.add_parser('bauble', help="the Bauble-before-crack demo: (a) crack now, (b) Bauble first, (c) + the rule")
    common(b)
    b.add_argument('--land-min', type=int, default=4)
    b.add_argument('--deep-sims', type=int, default=0, help='a deep search at each spot (e.g. 800) whose picks join the candidates')
    b.add_argument('--deep-k', default='0,8', help='its chance_k values (the largest first picks (d), the next (e))')
    b.add_argument('--deep-reps', type=int, default=8)
    b.add_argument('--deep-floor', type=int, default=100, help='also each chance_k with this root floor of visits per '
                   'option (0: none; at most half the budget)')
    b.add_argument('--deep-modes', default='belief', help='the modes the deep picks are played in')
    b.add_argument('--play-cands', help='play only these candidate indices (e.g. 3,4,5: the deep picks; the others '
                   'from the cache, else listed as not played yet)')
    b.set_defaults(fn=cmd_bauble)
    t = sub.add_parser('t1', help='the turn-1 Bauble position: opening hands searched over seeds, candidates (a)-(e)')
    common(t)
    t.add_argument('--seeds', default='1-60000', help='the game seeds searched for the opening hand (lo-hi)')
    t.add_argument('--spots', type=int, default=4, help='opening spots to take')
    t.add_argument('--composition', default='clean', choices=('clean', 'any'))
    t.add_argument('--draw', action='store_true', help='opening spots on the draw (the starting player\'s turn 1 by the network)')
    t.add_argument('--scenario', action='append', help='a saved opening spot (position-lab/1 JSON) instead of the search')
    t.add_argument('--save-spots', help='a directory to save the found spots as scenarios')
    t.add_argument('--one-ply', type=int, default=200, help='belief deals for the one-ply value check (0: none)')
    t.add_argument('--deep-sims', type=int, default=0)
    t.add_argument('--deep-k', default='0,8')
    t.add_argument('--deep-reps', type=int, default=8)
    t.add_argument('--deep-floor', type=int, default=100)
    t.add_argument('--deep-modes', default='belief')
    t.add_argument('--play-cands')
    t.set_defaults(fn=cmd_t1)
    c = sub.add_parser('canary', help='the turn-1 Bauble canary over game records (a per-generation fingerprint)')
    c.add_argument('--replays', default=REPLAYS)
    c.add_argument('--selfplay', action='append', help='a self-play move-log file (E:/mtg-zero-scratch/explore/ab/fp_*.json)')
    c.add_argument('--gen-min', type=int)
    c.add_argument('--out')
    c.set_defaults(fn=cmd_canary)
    w = sub.add_parser('show', help='the text table of a report')
    w.add_argument('--report', required=True)
    w.set_defaults(fn=cmd_show)
    a = ap.parse_args(argv)
    a.fn(a)


if __name__ == '__main__':
    main()
