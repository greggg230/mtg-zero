"""How closely the network agrees with the matchup model's choice table (docs/ENGINE.md 168.7): the prior a checkpoint
puts on the table's 60 at a fixed probe set of sideboarding decisions of the run's own matches.

pb3 sideboards its chosen-60 seats by the matchup model's choice table through teacher decisions (168.3): the table's 60
is played without a search and recorded as the policy target, so the network's PRIOR should learn the table's choice.
This tool measures whether it does, checkpoint by checkpoint, on the same positions every time.

**The probe set** (`probe`, made once and saved: every checkpoint is measured on the same positions). Sideboarding
decisions of seats that choose their 60 (not dealt) in the run's own logged self-play matches (the best-of-three game
log, 168.2), rebuilt exactly from the match's seed on the run's spec without a table (the table is never encoded and
never draws from the match's RNG: a position is the same with or without it):

* game 2: the match's start (post-sideboard matches: `start_game` 2); a seat that sideboards second sees the first
  seat's sideboarding, which is driven to the 60 the log says it played (hidden from the second seat anyway);
* game 3: the logged match's game-2 sideboarding as above (each choosing seat driven to its logged game-2 60), then
  game 2 played out by a FIXED policy - the run's first checkpoint's prior, sampled with the probe's seed (the logged
  game 2's moves are not in the log) - kept when it reaches a game 3; a seat that sideboards second there sees the first
  seat keep its 60.

Each position is stored as the action list from the match's start (`-1`: the next game's episode), so a rebuild is the
same game on every machine. Stratified by the table's context (game, sideboards first, lead, other seat dealt):
`--per-context` positions of each reachable context, chosen by a seeded shuffle.

**The measures** (`measure`; forward passes only, CPU, below-normal priority). A sideboarding is a sequence of decisions
(36.2: the changed entries in list order, each with its new count, then PASS - every legal 60 is reached in exactly one
way), so the network's prior over 60s is the product of its priors along that path. Per position:

* `prior` on a table version's 60 for the position's context (the canonical path the teacher plays,
  `sbtable::next_action`); `top1`: the network's highest-prior 60 is the table's. The highest-prior 60 is found
  exactly by a best-first search over the sideboarding tree (the path priors only shrink, so the first complete 60
  taken off the queue is the mode); a position whose search exceeds `--budget` forward passes is reported unresolved
  (`unresolved`) and left out of top1 - never guessed.
* `keep`: the prior on PASS at the first decision (keep the last game's 60; in game 2 that is the main);
  `main`: the prior on the main 60; `rough`: the prior on pb1's old favourite, the main +Rough // Tumble -Prismari Charm.

Compared against: the newest table version at the time of the measurement, the version that was live for the
checkpoint's own iteration (`used.jsonl`: iteration N's self-play spec, which checkpoint N trained on) and the first
version with a table. One row per checkpoint in `<run>/matchup/agreement.jsonl` (overall and per context, which
versions, the probe's id).

    python tools/matchup_agreement.py probe RUN --out PROBE [--per-context 75] [--seed 20261115] [--policy-iter 0]
    python tools/matchup_agreement.py measure RUN --probe PROBE [--iters 0-13] [--force]
    python tools/matchup_agreement.py watch RUN --probe PROBE [--poll 60] [--upload] [--stop FILE]
    python tools/matchup_agreement.py report RUN
"""
import argparse
import collections
import hashlib
import heapq
import json
import math
import os
import random
import re
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
sys.path.insert(0, os.path.join(ROOT, 'tools'))

FORMAT = 'mtgz-matchup-probe/1'
# The engine's action ids at a sideboarding (engine/src/decision.rs `act`): PASS, NUMBER + count, LIBRARY + entry.
PASS, NUMBER, LIBRARY = 0, 4, 340
NEXT_EPISODE = -1
ROUGH = {'Rough // Tumble': 1, 'Prismari Charm': -1}
SB_KINDS = ('Sideboard', 'SideboardCount')
BUDGET = 400
BATCH = 128
MAX_STEPS = 30000
RE_OPT = re.compile(r'^(?:Change (.*) \(now (\d+) of (\d+)\)|Take out (.*)|Bring in (.*))$')


class AgreementError(RuntimeError):
    pass


# ----------------------------------------------------------------------------- the canonical path (36.2, 168.3)


def _lay():
    """mtgz.az.layouts (docs/ENGINE.md 236): the game - layout v8 `mtg` or v7 `mtg_v7` - of the checkpoints this tool loads
    (`load_used`), every game it plays and encodes (`current`: layout v8 until a checkpoint is loaded)."""
    from mtgz.az import layouts
    return layouts

def apply(copies, counts, last, asking, a):
    """One sideboarding answer `a` (engine `Sideboarding::choose`) -> (counts, last, asking, done)."""
    counts = list(counts)
    if asking is not None:
        if a < NUMBER or a >= LIBRARY:
            raise AgreementError(f'a count answer expected, got {a}')
        counts[asking] = a - NUMBER
        return counts, asking, None, False
    if a == PASS:
        return counts, last, None, True
    k = a - LIBRARY
    if not 0 <= k < len(copies):
        raise AgreementError(f'no entry {k}')
    if copies[k] == 1:
        counts[k] ^= 1
        return counts, k, None, False
    return counts, last, k, False


def next_action(counts, last, asking, target):
    """The canonical path's next answer toward `target` (engine `sbtable::SideboardTable::next_action`)."""
    frm = 0 if last is None else last + 1
    if list(counts[:frm]) != list(target[:frm]):
        raise AgreementError('the sideboarding left the canonical path')
    if asking is not None:
        return NUMBER + target[asking]
    for k in range(frm, len(target)):
        if counts[k] != target[k]:
            return LIBRARY + k
    return PASS


def path(copies, start, target):
    """The canonical answers from deck `start` to `target` (counts per entry of the 75), PASS last."""
    if len(start) != len(copies) or len(target) != len(copies):
        raise AgreementError('a 60 of another length than the 75')
    if any(not 0 <= t <= c for t, c in zip(target, copies)):
        raise AgreementError(f'target {target} outside the 75 {copies}')
    counts, last, asking, out = list(start), None, None, []
    while True:
        a = next_action(counts, last, asking, target)
        out.append(a)
        counts, last, asking, done = apply(copies, counts, last, asking, a)
        if done:
            if counts != list(target):
                raise AgreementError('the path ended off its target')
            return out


def deck_after(copies, start, actions):
    """The deck counts after answers `actions` from `start` (PASS ends it)."""
    counts, last, asking = list(start), None, None
    for a in actions:
        counts, last, asking, done = apply(copies, counts, last, asking, a)
        if done:
            break
    return counts


def changes(main, deck, names):
    """{'in': {name: +n}, 'out': {name: n}} of `deck` against `main`."""
    return {'in': {names[i]: deck[i] - main[i] for i in range(len(deck)) if deck[i] > main[i]},
            'out': {names[i]: main[i] - deck[i] for i in range(len(deck)) if deck[i] < main[i]}}


def with_changes(main, names, d):
    out = list(main)
    for n, c in d.items():
        out[names.index(n)] += c
    return out


def ctx_key(c):
    return ','.join(str(int(x)) for x in c)


# ----------------------------------------------------------------------------- the run's files

def mdir(run):
    return os.path.join(run, 'matchup')


def rows(path):
    if not os.path.exists(path):
        return []
    with open(path, encoding='utf-8') as f:
        return [json.loads(x) for x in f if x.strip()]


def agreement_path(run):
    return os.path.join(mdir(run), 'agreement.jsonl')


def table_versions(run):
    """[(iter, name, table)] of the run's matchup versions that publish a table, oldest first."""
    from mtgz.az import matchup as MU
    out = []
    for b, p in MU.versions(run):
        with open(p, encoding='utf-8') as f:
            v = json.load(f)
        if v.get('table'):
            out.append((b, os.path.basename(p)[:-5], v['table']))
    return out


def live_version(run, it):
    """The table version iteration `it`'s self-play played (`used.jsonl`), None without one."""
    u = [r for r in rows(os.path.join(mdir(run), 'used.jsonl')) if r.get('iter') == it]
    if not u or not u[-1].get('version') or u[-1].get('table') is False:
        return None
    return u[-1]['version'][:-5] if u[-1]['version'].endswith('.json') else u[-1]['version']


def table_start(run):
    """The first iteration whose self-play sideboarded by a table, None before."""
    its = [r['iter'] for r in rows(os.path.join(mdir(run), 'used.jsonl')) if r.get('table')]
    return min(its) if its else None


def checkpoints(run):
    out = []
    d = os.path.join(run, 'models')
    for f in os.listdir(d) if os.path.isdir(d) else []:
        m = re.match(r'^iter_(\d+)\.pt$', f)
        if m:
            out.append((int(m.group(1)), os.path.join(d, f)))
    return sorted(out)


def run_spec(run):
    with open(os.path.join(run, 'config.json'), encoding='utf-8') as f:
        spec = json.load(f)['spec']
    sp = json.loads(spec)
    if not (sp.get('constructed') and sp.get('format') == 'bo3' and sp.get('start_game') == 2):
        raise AgreementError('a constructed best-of-three run of post-sideboard matches (start_game 2) only')
    if 'sideboard_table' in sp:
        raise AgreementError("the run's spec carries a table of its own")
    return spec


def spec_hash(spec):
    return hashlib.sha256(spec.encode()).hexdigest()[:16]


def file_hash(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for b in iter(lambda: f.read(1 << 20), b''):
            h.update(b)
    return h.hexdigest()[:16]


# ----------------------------------------------------------------------------- the engine and the network

def low_priority():
    try:
        import psutil
        p = psutil.Process()
        p.nice(psutil.BELOW_NORMAL_PRIORITY_CLASS if sys.platform == 'win32' else 10)
    except Exception:  # noqa: BLE001  (best effort: the measure runs either way)
        pass


def setup(threads=4):
    os.environ.setdefault('CUDA_VISIBLE_DEVICES', '-1')
    low_priority()
    import mtgz.az  # noqa: F401  (the pinned core and the game package)
    import torch
    torch.set_num_threads(threads)
    from zero.games import engine
    return engine()


def load_net(path):
    from zero.infer import NetEvaluator
    from zero.net import load
    model, _ = _lay().load_used(path, 'cpu')
    model.eval()
    return NetEvaluator(model, 'cpu')


class Net:
    """A checkpoint's priors at decisions (the decider's own row), batched."""

    def __init__(self, ev):
        import rowkit
        self.ev, self.R, self.rk, self.n = ev, rowkit, rowkit.kind(_lay().current()), 0

    def priors(self, games):
        out = []
        for a in range(0, len(games), BATCH):
            part = games[a:a + BATCH]
            xs, acts, rrs = [], [], []
            for g in part:
                x, rr = self.R.row(g, g.to_move, self.rk, opts=True)
                xs.append(x)
                acts.append([int(v) for v in g.legal()])
                rrs.append(rr)
            pri = self.R.evaluate(self.ev, xs, acts, self.rk, rrs)[0]
            o = 0
            for ac in acts:
                out.append({x: float(p) for x, p in zip(ac, pri[o:o + len(ac)])})
                o += len(ac)
            self.n += len(part)
        return out


def state(g):
    return json.loads(g.export_state())


def decision(g):
    return state(g)['decision']


def check_counts(g, names, copies, start):
    """The position's first sideboarding decision offers what `start` says (each option's label: its entry's name and
    current count) - the stored deck and the 75's order are the engine's."""
    d = decision(g)
    if d['kind'] != 'Sideboard':
        raise AgreementError(f'not a sideboarding decision: {d["kind"]}')
    seen = 0
    for o in d['options']:
        if o['a'] == PASS:
            continue
        k = o['a'] - LIBRARY
        m = RE_OPT.match(o['t'])
        if not m or not 0 <= k < len(names):
            raise AgreementError(f'unexpected option {o}')
        if m.group(1) is not None:
            name, now, of = m.group(1), int(m.group(2)), int(m.group(3))
        elif m.group(4) is not None:
            name, now, of = m.group(4), 1, 1
        else:
            name, now, of = m.group(5), 0, 1
        if name != names[k] or now != start[k] or of != copies[k]:
            raise AgreementError(f'option {o["t"]!r} (entry {k}) against the stored deck: {names[k]} {start[k]} of {copies[k]}')
        seen += 1
    if not seen:
        raise AgreementError('a sideboarding decision without entries')


def replay(E, spec, pos):
    """The probe position's game: its match rebuilt from the seed, its stored actions replayed (each one legal)."""
    g = E.Game(_lay().current(), spec, 2, int(pos['seed']))
    for i, a in enumerate(pos['moves']):
        if a == NEXT_EPISODE:
            if g.to_move is not None:
                raise AgreementError(f'position {pos["id"]}: move {i}: next episode while a decision is pending')
            g, _ = g.next_episode()
            if g is None:
                raise AgreementError(f'position {pos["id"]}: move {i}: the match ended')
            continue
        if g.to_move is None or a not in g.legal():
            raise AgreementError(f'position {pos["id"]}: move {i} ({a}) is not legal')
        g.step(a)
    if g.to_move != pos['seat']:
        raise AgreementError(f'position {pos["id"]}: seat {g.to_move} to move, not {pos["seat"]}')
    return g


# ----------------------------------------------------------------------------- the probe set

def load_log(run):
    """The run's logged self-play games grouped by match: {seed: {'dealt', 'games': {n: line}}}, the 75 and the main."""
    from mtgz.az import matchup as MU
    games, specs = MU.load_games(run)
    if not games:
        raise AgreementError(f'{run}: no logged self-play games')
    lists = {json.dumps(s['lists']) for s in specs.values()}
    mains = {json.dumps(s['mains']) for s in specs.values()}
    if len(lists) != 1 or len(mains) != 1:
        raise AgreementError('the logged specs disagree about the 75 or the main')
    lst = json.loads(lists.pop())
    if lst[0] != lst[1]:
        raise AgreementError('a mirror only (one 75 both seats)')
    names = [n for n, _ in lst[0]]
    copies = [c for _, c in lst[0]]
    main = json.loads(mains.pop())[0]
    by = {}
    for x in games:
        m = by.setdefault(int(x['seed']), {'dealt': x['dealt'], 'games': {}})
        if m['dealt'] != x['dealt']:
            raise AgreementError(f'match {x["seed"]}: the dealt seats changed between games')
        m['games'][x['game']] = x
    return by, names, copies, main


def context_of(st, seat, dealt):
    """(game, first, lead, opp_dealt) of `seat`'s sideboarding from the engine's state and the match's dealt seats."""
    m = st['match']
    o = 1 - seat
    order = m['sideboarding']['order']
    return (m['game'] + 1, int(order[0] == seat), m['wins'][seat] - m['wins'][o], int(bool(dealt[o])))


def drive(g, moves, copies, start, target):
    """Play the decider's canonical sideboarding from `start` to `target`, appending to `moves`."""
    for a in path(copies, start, target):
        if a not in g.legal():
            raise AgreementError(f'canonical answer {a} not legal (target {target})')
        g.step(a)
        moves.append(a)


def sideboard_phase(g, moves, decks, take, dealt, copies, positions, base):
    """Run the pending sideboarding: each deciding seat p is recorded as a position when take(p, context) says so,
    then sideboards from decks[p][0] (its last game's 60) to decks[p][1]. Returns when the next game's play begins."""
    while g.to_move is not None:
        d = decision(g)
        if d['kind'] not in SB_KINDS:
            return
        p = g.to_move
        if d['kind'] != 'Sideboard':
            raise AgreementError('a sideboarding entered mid-way')
        st = state(g)
        c = context_of(st, p, dealt)
        if take(p, c):
            positions.append(dict(base, seat=p, context=list(c), start=list(decks[p][0]), moves=list(moves)))
        drive(g, moves, copies, decks[p][0], decks[p][1])


def play_out(games, net, rng_of, moves_of, max_steps=MAX_STEPS):
    """Play each game's current game to its end with the policy's prior (sampled), lockstep in batches. Returns the
    indices of the games whose match goes on (the next episode stepped into, `-1` appended to their moves)."""
    active = list(range(len(games)))
    steps = collections.Counter()
    on = []
    while active:
        need, nxt = [], []
        for i in active:
            g = games[i]
            if g.to_move is None:
                ng, _ = g.next_episode()
                if ng is not None:
                    games[i] = ng
                    moves_of[i].append(NEXT_EPISODE)
                    on.append(i)
                continue
            legal = [int(a) for a in g.legal()]
            if len(legal) == 1:
                g.step(legal[0])
                moves_of[i].append(legal[0])
                steps[i] += 1
                nxt.append(i)
                continue
            if steps[i] > max_steps:
                continue                      # (dropped: never stored)
            need.append(i)
        if need:
            pri = net.priors([games[i] for i in need])
            for i, pr in zip(need, pri):
                acts = sorted(pr)
                a = rng_of[i].choices(acts, weights=[max(pr[x], 0.0) for x in acts])[0]
                games[i].step(a)
                moves_of[i].append(a)
                steps[i] += 1
                nxt.append(i)
        active = nxt
    return sorted(on)


def make_probe(run, per_context=75, seed=20261115, policy_iter=0, log=print, E=None, net=None, batch=192):
    """The fixed probe set (module docs) -> the probe dict."""
    E = E or setup()
    spec = run_spec(run)
    by, names, copies, main = load_log(run)
    rng = random.Random(seed)
    seeds = sorted(by)
    rng.shuffle(seeds)
    positions = []
    got = collections.Counter()

    def want(c):
        return got[c] < per_context

    # game 2: the match's start
    for s in seeds:
        m = by[s]
        if 2 not in m['games'] or all(m['dealt']):
            continue
        line = m['games'][2]
        decks = {p: (main, line['decks'][p]) for p in (0, 1)}
        g = E.Game(_lay().current(), spec, 2, s)
        moves, found = [], []
        sideboard_phase(g, moves, decks, lambda p, c: not m['dealt'][p] and want(tuple(c)), m['dealt'], copies, found,
                        {'seed': str(s), 'game': 2})
        for x in found:
            c = tuple(x['context'])
            if want(c):
                x['id'] = f'{s}-2-{x["seat"]}'
                positions.append(x)
                got[c] += 1
    log(f'game 2: {sum(v for c, v in got.items() if c[0] == 2)} positions: ' +
        ', '.join(f'{ctx_key(c)} {v}' for c, v in sorted(got.items())))

    # game 3: the logged game 2's sideboarding, game 2 played out by the fixed policy
    ck = dict(checkpoints(run)).get(policy_iter)
    if ck is None:
        raise AgreementError(f'{run}: no checkpoint iter_{policy_iter:04d}.pt for the probe\'s game-2 policy')
    net = net or Net(load_net(ck))
    g3 = [(3, f, 0, o) for f in (0, 1) for o in (0, 1)]
    cand = [s for s in seeds if 2 in by[s]['games'] and not all(by[s]['dealt'])]
    k = 0
    while any(want(c) for c in g3) and k < len(cand):
        part = cand[k:k + batch]
        k += len(part)
        # keep only matches that could fill a missing game-3 context (the other seat's dealt flag decides)
        part = [s for s in part if any(want((3, f, 0, int(bool(by[s]['dealt'][1 - p])))) for p in (0, 1)
                                       if not by[s]['dealt'][p] for f in (0, 1))]
        games, moves_of, rngs = [], [], []
        for s in part:
            m = by[s]
            line = m['games'][2]
            g = E.Game(_lay().current(), spec, 2, s)
            mv = []
            sideboard_phase(g, mv, {p: (main, line['decks'][p]) for p in (0, 1)}, lambda p, c: False, m['dealt'],
                            copies, [], {})
            games.append(g)
            moves_of.append(mv)
            rngs.append(random.Random(f'{seed}-{s}'))
        on = play_out(games, net, rngs, moves_of)
        for i in on:
            s = part[i]
            m = by[s]
            g = games[i]
            line = m['games'][2]
            found = []
            decks = {p: (line['decks'][p], line['decks'][p]) for p in (0, 1)}      # (a seat keeps its game-2 60)
            sideboard_phase(g, moves_of[i], decks, lambda p, c: not m['dealt'][p] and want(tuple(c)) and c[0] == 3,
                            m['dealt'], copies, found, {'seed': str(s), 'game': 3})
            for x in found:
                c = tuple(x['context'])
                if want(c):
                    x['id'] = f'{s}-3-{x["seat"]}'
                    positions.append(x)
                    got[c] += 1
        log(f'game 3: {k} of {len(cand)} matches tried, {len(on)} of {len(part)} reached a game 3; ' +
            ', '.join(f'{ctx_key(c)} {got[c]}' for c in g3))
    positions.sort(key=lambda x: (x['context'][0], x['id']))
    body = {'format': FORMAT, 'run': os.path.basename(os.path.normpath(run)), 'spec': spec_hash(spec), 'seed': seed,
            'per_context': per_context, 'list': [[n, c] for n, c in zip(names, copies)], 'main': main,
            'policy': {'iter': policy_iter, 'sha': file_hash(ck), 'rule': 'the prior, sampled (game 2 of a game-3 position)'},
            'created': int(time.time()), 'contexts': {ctx_key(c): v for c, v in sorted(got.items())},
            'positions': positions}
    body['id'] = hashlib.sha256(json.dumps(positions, sort_keys=True).encode()).hexdigest()[:12]
    return body


def load_probe(path, run=None):
    with open(path, encoding='utf-8') as f:
        p = json.load(f)
    if p.get('format') != FORMAT:
        raise AgreementError(f'{path}: not a probe set ({p.get("format")})')
    if run is not None and p['spec'] != spec_hash(run_spec(run)):
        raise AgreementError(f"{path}: made on another spec than the run's")
    if hashlib.sha256(json.dumps(p['positions'], sort_keys=True).encode()).hexdigest()[:12] != p['id']:
        raise AgreementError(f'{path}: its positions do not match its id')
    return p


# ----------------------------------------------------------------------------- the measures

class Tree:
    """One position's sideboarding tree: the game and the network's priors at each answer prefix, filled in batches."""

    def __init__(self, g, copies, start):
        self.root, self.copies, self.start = g, copies, start
        self.games = {(): g}
        self.pri = {}

    def game(self, prefix):
        if prefix not in self.games:
            g = self.game(prefix[:-1]).clone()
            g.step(prefix[-1])
            self.games[prefix] = g
        return self.games[prefix]


def fill(net, items):
    """Evaluate the priors of every (tree, prefix) not evaluated yet."""
    todo, seen = [], set()
    for t, pre in items:
        if pre not in t.pri and (id(t), pre) not in seen:
            seen.add((id(t), pre))
            todo.append((t, pre))
    if todo:
        for (t, pre), pr in zip(todo, net.priors([t.game(pre) for t, pre in todo])):
            t.pri[pre] = pr


def path_priors(net, trees, targets):
    """Per tree, the prior product along the canonical path to each of its targets -> [[p, ...], ...]."""
    paths = [[tuple(path(t.copies, t.start, tg)) for tg in tgs] for t, tgs in zip(trees, targets)]
    need = [(t, pa[:i]) for t, pas in zip(trees, paths) for pa in pas for i in range(len(pa))]
    fill(net, need)
    out = []
    for t, pas in zip(trees, paths):
        r = []
        for pa in pas:
            p = 1.0
            for i, a in enumerate(pa):
                pr = t.pri[pa[:i]]
                if a not in pr:
                    raise AgreementError(f'canonical answer {a} not among the legal answers {sorted(pr)}')
                p *= pr[a]
            r.append(p)
        out.append(r)
    return out


def modes(net, trees, budget=BUDGET):
    """Each tree's highest-prior 60, exactly (best-first: path priors only shrink, so the first complete 60 off the
    queue is the mode) -> [(deck, prior) or None when the search needed more than `budget` evaluations]."""
    heaps = [[(-1.0, 0, (), False)] for _ in trees]
    done = [None] * len(trees)
    evals = [0] * len(trees)
    tick = 1
    live = set(range(len(trees)))
    while live:
        need = []
        for i in sorted(live):
            t, h = trees[i], heaps[i]
            while True:
                negp, _, pre, term = h[0]
                if term:
                    done[i] = (deck_after(t.copies, t.start, pre), -negp)
                    live.discard(i)
                    break
                if pre not in t.pri:
                    if evals[i] >= budget:
                        live.discard(i)              # (unresolved: never guessed)
                    else:
                        need.append((t, pre))
                        evals[i] += 1
                    break
                heapq.heappop(h)
                for a, p in t.pri[pre].items():
                    # (PASS is an answer of a Sideboard decision only: a count's answers are NUMBER + n >= 4)
                    heapq.heappush(h, (negp * p, tick, pre + (a,), a == PASS))
                    tick += 1
        fill(net, need)
    return done


def measure(run, ck, probe, E, positions=None, versions=None, it=None, budget=BUDGET, log=print):
    """One checkpoint's agreement row (module docs)."""
    t0 = time.time()
    spec = run_spec(run)
    names = [n for n, _ in probe['list']]
    copies = [c for _, c in probe['list']]
    main = probe['main']
    rough = with_changes(main, names, ROUGH)
    if positions is None:
        positions = [replay(E, spec, p) for p in probe['positions']]
    versions = versions if versions is not None else table_versions(run)
    if not versions:
        raise AgreementError(f'{run}: no matchup version with a table yet')
    for _, v, tb in versions:
        if [[n, c] for n, c in tb['list']] != probe['list']:
            raise AgreementError(f'table {v}: its 75 is not the probe\'s')
    byname = {v: tb for _, v, tb in versions}
    live = live_version(run, it) if it is not None else None
    cmp_ = []
    for v in (versions[-1][1], live, versions[0][1]):
        if v and v not in cmp_:
            if v not in byname:
                raise AgreementError(f'table version {v} (live for iteration {it}) not found')
            cmp_.append(v)
    choice = {v: {ctx_key(c[:4]): c[4] for c in byname[v]['choices']} for v in cmp_}
    net = Net(load_net(ck))
    trees = [Tree(g.clone(), copies, p['start']) for g, p in zip(positions, probe['positions'])]
    for t, p in zip(trees, probe['positions']):
        if t.root.to_move != p['seat']:
            raise AgreementError(f'position {p["id"]}: not its seat to move')
    targets = [[choice[v][ctx_key(p['context'])] for v in cmp_] + [main, rough] for p in probe['positions']]
    pp = path_priors(net, trees, targets)
    fill(net, [(t, ()) for t in trees])
    mo = modes(net, trees, budget)
    per = []
    for p, t, r, m in zip(probe['positions'], trees, pp, mo):
        x = {'ctx': ctx_key(p['context']), 'keep': t.pri[()].get(PASS, 0.0), 'main': r[len(cmp_)], 'rough': r[len(cmp_) + 1],
             'mode': None if m is None else m[0], 'vs': {}}
        for j, v in enumerate(cmp_):
            x['vs'][v] = {'prior': r[j], 'top1': None if m is None else m[0] == list(choice[v][x['ctx']])}
        per.append(x)
    row = summarize(per, cmp_, main, names)
    row.update({'iter': it, 'checkpoint': os.path.basename(ck), 'probe': probe['id'], 'n': len(per),
                'tables': {'newest': versions[-1][1], 'live': live, 'first': versions[0][1]},
                'evals': net.n, 'seconds': round(time.time() - t0, 1), 'device': 'cpu', 'time': round(time.time(), 1)})
    return row


def _mean(xs):
    return round(sum(xs) / len(xs), 5) if xs else None


def _agg(items, cmp_):
    out = {'n': len(items), 'keep': _mean([x['keep'] for x in items]), 'main': _mean([x['main'] for x in items]),
           'rough': _mean([x['rough'] for x in items]), 'unresolved': sum(x['mode'] is None for x in items), 'vs': {}}
    for v in cmp_:
        res = [x['vs'][v]['top1'] for x in items if x['vs'][v]['top1'] is not None]
        out['vs'][v] = {'prior': _mean([x['vs'][v]['prior'] for x in items]), 'top1': _mean([float(b) for b in res]),
                        'resolved': len(res)}
    return out


def summarize(per, cmp_, main, names):
    by = collections.defaultdict(list)
    for x in per:
        by[x['ctx']].append(x)
    ctx = {}
    for k in sorted(by, key=lambda s: [int(v) for v in s.split(',')]):
        a = _agg(by[k], cmp_)
        decks = collections.Counter(tuple(x['mode']) for x in by[k] if x['mode'] is not None)
        if decks:
            d, n = decks.most_common(1)[0]
            a['favourite'] = dict(changes(main, list(d), names), share=round(n / len(by[k]), 4))
        ctx[k] = a
    return {'all': _agg(per, cmp_), 'contexts': ctx}


# ----------------------------------------------------------------------------- the jobs

def append_row(run, row):
    p = agreement_path(run)
    os.makedirs(os.path.dirname(p), exist_ok=True)
    with open(p, 'a', encoding='utf-8') as f:
        f.write(json.dumps(row, separators=(',', ':')) + '\n')


def measured(run, probe_id):
    return {r['iter'] for r in rows(agreement_path(run)) if r.get('probe') == probe_id}


def describe(row):
    a = row['all']
    v = row['tables']['live'] or row['tables']['newest']
    s = a['vs'][v]
    return (f"iter {row['iter']}: prior on table {v}'s 60 {s['prior']:.3f}, top-1 {s['top1'] if s['top1'] is None else round(s['top1'], 3)}"
            f" ({s['resolved']} resolved); keep {a['keep']:.3f}, main {a['main']:.3f}, +Rough -Charm {a['rough']:.3f}"
            f" [{row['n']} positions, {row['evals']} evaluations, {row['seconds']} s]")


def measure_missing(run, probe, E, positions, iters=None, force=False, log=print, budget=BUDGET, settle=20.0):
    """Measure every checkpoint (in `iters`, if given) without a row for this probe set; append each row. A checkpoint
    written less than `settle` seconds ago waits for the next call (it may still be being written)."""
    done = set() if force else measured(run, probe['id'])
    out = []
    for it, ck in checkpoints(run):
        if (iters is not None and it not in iters) or it in done:
            continue
        if time.time() - os.path.getmtime(ck) < settle:
            continue                                   # (being written)
        row = measure(run, ck, probe, E, positions, it=it, budget=budget, log=log)
        append_row(run, row)
        log(describe(row))
        out.append(row)
    # a table published since the newest checkpoint was measured: that checkpoint again, against it (a row of its own;
    # readers take each checkpoint's newest row - the live table, and so its line, stay the same)
    vs = table_versions(run)
    mine = [r for r in rows(agreement_path(run)) if r.get('probe') == probe['id']]
    if iters is None and vs and mine:
        last = max(mine, key=lambda r: (r['iter'], r['time']))
        ck = dict(checkpoints(run)).get(last['iter'])
        if ck and last['tables']['newest'] != vs[-1][1]:
            row = measure(run, ck, probe, E, positions, versions=vs, it=last['iter'], budget=budget, log=log)
            append_row(run, row)
            log(f'(again, against the new table {vs[-1][1]}) ' + describe(row))
            out.append(row)
    return out


def parse_iters(s):
    if not s:
        return None
    out = set()
    for part in s.split(','):
        if '-' in part:
            a, b = part.split('-')
            out.update(range(int(a), int(b) + 1))
        else:
            out.add(int(part))
    return out


def say(msg):
    print(time.strftime('%H:%M:%S ') + msg, flush=True)


def watch(run, probe_path, poll=60, upload=False, stop=None, url=None, budget=BUDGET, threads=4):
    """Measure each new checkpoint as it lands (and upload the site's record after each)."""
    E = setup(threads)
    probe = load_probe(probe_path, run)
    spec = run_spec(run)
    positions = [replay(E, spec, p) for p in probe['positions']]
    say(f'watching {run}: probe {probe["id"]} ({len(positions)} positions); stop file {stop}')
    last_upload = None
    while True:
        if stop and os.path.exists(stop):
            say('stop file found: exiting')
            return 0
        try:
            new = measure_missing(run, probe, E, positions, log=say, budget=budget)
            if upload:
                import delverai_matchup as DM
                sig = DM.signature(run)
                if new or sig != last_upload:
                    DM.sync(run, **({'url': url} if url else {}), log=say)
                    last_upload = sig
        except Exception as e:  # noqa: BLE001  (logged loudly, retried at the next poll)
            say(f'AGREEMENT WATCH FAILED: {type(e).__name__}: {e}')
        time.sleep(poll)


def report(run):
    for r in rows(agreement_path(run)):
        print(describe(r))


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest='cmd', required=True)
    p = sub.add_parser('probe')
    p.add_argument('run')
    p.add_argument('--out', required=True)
    p.add_argument('--per-context', type=int, default=75)
    p.add_argument('--seed', type=int, default=20261115)
    p.add_argument('--policy-iter', type=int, default=0)
    p.add_argument('--threads', type=int, default=4)
    m = sub.add_parser('measure')
    m.add_argument('run')
    m.add_argument('--probe', required=True)
    m.add_argument('--iters', default='')
    m.add_argument('--force', action='store_true')
    m.add_argument('--budget', type=int, default=BUDGET)
    m.add_argument('--threads', type=int, default=4)
    w = sub.add_parser('watch')
    w.add_argument('run')
    w.add_argument('--probe', required=True)
    w.add_argument('--poll', type=float, default=60)
    w.add_argument('--upload', action='store_true')
    w.add_argument('--url', default=None)
    w.add_argument('--stop', default=None)
    w.add_argument('--budget', type=int, default=BUDGET)
    w.add_argument('--threads', type=int, default=4)
    r = sub.add_parser('report')
    r.add_argument('run')
    a = ap.parse_args(argv)
    if a.cmd == 'probe':
        if os.path.exists(a.out):
            raise SystemExit(f'{a.out} exists: a probe set is made once (delete it by hand to remake it)')
        E = setup(a.threads)
        body = make_probe(a.run, a.per_context, a.seed, a.policy_iter, log=say, E=E)
        os.makedirs(os.path.dirname(os.path.abspath(a.out)), exist_ok=True)
        tmp = a.out + '.tmp'
        with open(tmp, 'w', encoding='utf-8') as f:
            json.dump(body, f, separators=(',', ':'))
        os.replace(tmp, a.out)
        say(f'wrote {a.out}: probe {body["id"]}, {len(body["positions"])} positions: {body["contexts"]}')
        return 0
    if a.cmd == 'measure':
        E = setup(a.threads)
        probe = load_probe(a.probe, a.run)
        positions = [replay(E, run_spec(a.run), p) for p in probe['positions']]
        new = measure_missing(a.run, probe, E, positions, parse_iters(a.iters), a.force, log=say, budget=a.budget)
        say(f'{len(new)} checkpoints measured')
        return 0
    if a.cmd == 'watch':
        return watch(a.run, a.probe, a.poll, a.upload, a.stop, a.url, a.budget, a.threads)
    report(a.run)
    return 0


if __name__ == '__main__':
    sys.exit(main())
