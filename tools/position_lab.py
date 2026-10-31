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
REPLAYS = r'C:/Users/Gregg Keithley/mtg-zero/runs/r3-2b-beliefsearch/replays'
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


def search(games, ev, sims, seeds, keys, c_puct, preds=None):
    """One Rust tree per game with the evaluation seat's settings (root-view leaves, the root belief,
    the move rule), `sims` simulations each in lockstep (root_view.py's search_all)."""
    import numpy as np
    rv = keys.get('leaf_value_view', 'mover') == 'root'
    trees = []
    for i, (g, s) in enumerate(zip(games, seeds)):
        t = g.search_tree(True, int(s))
        if keys.get('root_min_visits'):
            t.set_root_min(int(keys['root_min_visits']) if len(g.legal()) <= keys.get('root_min_visits_max_actions', 4) else 0)
        if keys.get('belief_tilt'):
            t.set_belief(np.asarray(preds[i], np.float32), g, float(keys['belief_tilt']), float(keys['belief_clip']),
                         float(keys['belief_mix']))
        trees.append(t)
    for _ in range(sims):
        rows, acts, owners = [], [], []
        for j, (g, t) in enumerate(zip(games, trees)):
            r = t.simulate(g, c_puct, root_view=True) if rv else t.simulate(g, c_puct)
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

def parse_cand(text):
    """A --cand: a JSON object {"name", "forced", "follow"}, or a ';'-separated label sequence."""
    text = text.strip()
    if text.startswith('{'):
        c = json.loads(text)
        if not c.get('forced'):
            raise SystemExit(f'--cand {text}: no "forced" line')
        return {'name': c.get('name') or ' ; '.join(map(str, c['forced'])), 'forced': c['forced'], 'follow': c.get('follow')}
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


def bauble_candidates(spot, g, land_min):
    """(a) the actual play (the frame's move), (b) the Bauble line then the network, (c) the line with the
    follow rule (crack iff the seen card is a land and the decider has >= land_min lands)."""
    names = dict(options(g))
    actual = names[spot['recorded']['a']]
    line = bauble_line(g, spot['decider'])
    return [{'name': f'(a) {actual} (the actual play)', 'forced': [label_item(actual)], 'follow': None},
            {'name': '(b) Bauble at own library first, network on', 'forced': line, 'follow': None},
            {'name': f'(c) Bauble first + rule (crack iff land & >= {land_min} lands)', 'forced': line,
             'follow': {'rule': 'bauble_fetch', 'land_min': land_min, 'fetches': list(FETCHES)}}]


def start_block(mode, cand, deal_seed, pred=None, belief=None, hold=(), exclude=()):
    """The start block of one game: deal mode, deal seed, the candidate's line and rule."""
    st = {'deal_seed': int(deal_seed), 'forced': cand['forced']}
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


def play_one(spot, modes, cand, ci, si, ev, keys, c_puct, sims, games, seed=1, conc=512, threads=4, move_log=0):
    """One candidate at one spot: every mode x `games` deals in one zero-core driver, queued setups in the
    order (mode, deal i) - every candidate's driver has the same seed and the same order, so game gid of
    one candidate and of another are the same deal AND the same search streams (they part where the
    games do: zero-core docs/search.md 2.3). -> ({tag: row}, perf, {tag: spec})."""
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
         log=print):
    """Every (spot, candidate, mode) of `jobs` ([(spot, [(mode, start kwargs)], candidates)]) in `procs`
    worker processes (each: `threads` torch threads, `search_threads` search threads, below-normal
    priority), jobs found in `cache` (same key) reused. -> (rows by tag, line infos by tag, perfs)."""
    from concurrent.futures import ProcessPoolExecutor, as_completed
    _WORKER['model_id'] = model_id(model_path)
    work = [(spot, m, cand, ci, si, keys, c_puct, sims, games, seed, conc, search_threads, diag, cache)
            for si, (spot, md, cands) in enumerate(jobs) for ci, cand in enumerate(cands) for m in md]
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


def summarize(spot, mode, cands, rows, games, si):
    """Per candidate: score +- se, the paired differences to the best and to the first (the actual play),
    the per-deal differences to the first (for pooling)."""
    d = spot['decider']
    turn0 = spot['state'].get('turn') or 0
    by = [[rows[f'{si}|{mode}|{ci}|{i}'] for i in range(games)] for ci in range(len(cands))]
    scores = [[float(r['result'][d]) for r in rs] for rs in by]
    means = [stats(s)[0] for s in scores]
    best = max(range(len(cands)), key=lambda c: means[c])
    out = []
    for ci, cand in enumerate(cands):
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
    diffs = [[a - b for a, b in zip(scores[ci], scores[0])] for ci in range(len(cands))]
    return out, diffs


def pooled(spots_out, mode, n_cands):
    """Over the spots that played `mode` (equal weight: the same games per spot): the mean score per
    candidate, and the paired difference to the first candidate over every deal of every spot, with
    its se (deals independent across spots)."""
    out = []
    for ci in range(n_cands):
        sc, dif = [], []
        for so in spots_out:
            res = so['modes'].get(mode)
            if not res:
                continue
            sc.append(res['candidates'][ci]['score'])
            dif.extend(res['_diffs'][ci])
        m, se = stats(dif)
        out.append({'candidate': ci, 'spots': len(sc), 'score': r4(sum(sc) / len(sc)) if sc else None,
                    'vs_first': {'diff': r4(m), 'se_paired': r4(se), 'pairs': len(dif)}})
    return out


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
            procs, conc, seed, log=print, diagnose_lines=True, cache=None):
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
        cands = cand_fn(spot, g, view)
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
                             'candidates': [c['name'] for c in cands], 'lines': cands, 'skipped_modes': skipped,
                             'deals': deals, 'modes': {}})
        log(f'spot {spot["id"]}: checked; network view at {time.time() - t0:.0f} s; candidates {[c["name"] for c in cands]}'
            + (f'; skipped {skipped}' if skipped else ''))
    n = sum(len(m) * len(c) for _, m, c in jobs) * games
    log(f'play-outs: {n} games at {sims} simulations per move, {procs} processes')
    t1 = time.time()
    rows, infos, perfs = play(jobs, model_path, keys, c_puct, sims, games, seed, conc, threads, search_threads, procs,
                              diagnose_lines, cache=cache, log=log)
    out['perf'] = {'games': len(rows), 'seconds': round(time.time() - t1, 1), 'jobs': perfs,
                   'cut': sum(p['cut'] for p in perfs)}
    for si, (spot, md, cands) in enumerate(jobs):
        so = out['spots'][si]
        d = spot['decider']
        for mode, _ in md:
            res, diffs = summarize(spot, mode, cands, rows, games, si)
            # the decider's per-deal results of each candidate (deal i: the same world for all)
            entry = {'candidates': res, '_diffs': diffs,
                     'results': [[float(rows[f'{si}|{mode}|{ci}|{i}']['result'][d]) for i in range(games)]
                                 for ci in range(len(cands))]}
            if diagnose_lines:
                entry['lines'] = []
                for ci, cand in enumerate(cands):
                    tags = [f'{si}|{mode}|{ci}|{i}' for i in range(games)]
                    if not all(t in infos for t in tags):
                        entry['lines'].append(None)
                        continue
                    entry['lines'].append(line_summary([infos[t] for t in tags], [float(rows[t]['result'][d]) for t in tags]))
            so['modes'][mode] = entry
    out['seconds'] = round(time.time() - t0, 1)
    return out


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
                vb = c['vs_best']
                row += f'{c["score"]:.3f}+-{c["se"]:.3f} ({vb["diff"]:+.3f}+-{vb["se_paired"]:.3f})'.rjust(32)
            lines.append(row)
        lines.append('   games {} per cell; mean turns from the spot: {}'.format(
            so['modes'][modes[0]]['candidates'][0]['games'] if modes else 0,
            ', '.join(f'{m} ' + '/'.join(f'{c["turns_from_spot"]:.1f}' for c in so['modes'][m]['candidates']) for m in modes)))
        for m in modes:
            for ci, li in enumerate(so['modes'][m].get('lines') or []):
                if li:
                    lines.append(f'   [{m}] {so["candidates"][ci][:44]}: line held {li["line_held"]}/{li["games"]}'
                                 + (f', rule {li["rule"]}' if li.get('rule') else '')
                                 + (f', the network cracked over the seen card {li["network_cracked_over_seen_card"]:.2f}'
                                    f' of {li["cracked_over_seen_n"]}' if 'network_cracked_over_seen_card' in li else ''))
        if so.get('skipped_modes'):
            lines.append(f'   skipped: {so["skipped_modes"]}')
    if rep.get('pooled'):
        lines.append('== pooled over spots (equal weight): score; paired difference to (a) over all deals')
        for m, pc in rep['pooled'].items():
            for x in pc:
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
                  a.threads, a.search_threads, a.procs, a.conc, a.seed, diagnose_lines=not a.no_diagnose, cache=a.cache)
    finish(rep, a)


def cmd_bauble(a):
    modes = check_modes(a)
    low_priority(a.threads)
    rep = run_lab(spots_of(a), lambda s, g, v: bauble_candidates(s, g, a.land_min), a.model, modes, a.hold, a.exclude,
                  a.games, a.sims, a.view_sims, a.view_reps, a.threads, a.search_threads, a.procs, a.conc, a.seed,
                  diagnose_lines=not a.no_diagnose, cache=a.cache)
    rep['candidate_names'] = ['(a) the actual play (crack now)', '(b) Bauble first, network on',
                              f'(c) Bauble first + rule (land_min {a.land_min})']
    rep['pooled'] = {m: pooled(rep['spots'], m, 3) for m in modes}
    finish(rep, a)


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
    b.set_defaults(fn=cmd_bauble)
    w = sub.add_parser('show', help='the text table of a report')
    w.add_argument('--report', required=True)
    w.set_defaults(fn=cmd_show)
    a = ap.parse_args(argv)
    a.fn(a)


if __name__ == '__main__':
    main()
