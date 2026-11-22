"""Re-record the Delver canary library after a rules change (docs/ENGINE.md 100.8): every spot's path
rebuilt on THIS checkout's engine to the same position, or the spot dropped with its reason - never an
approximate spot.

    python tools/delver_rerecord.py check [--library L] [--stamp]
    python tools/delver_rerecord.py rerecord --old-engine CHECKOUT [--library L] [--out L] [--report R.json]
                                             [--knowledge-change]
    python tools/delver_rerecord.py trace --engine CHECKOUT --library L --out TRACE.json   # rerecord's old side

A spot is a source game's seed and its actions from the deal to the decision (the raw steps, single-option
decisions included: `delver_blunders.spot_game`). A rules change that asks new questions (ENGINE.md
100.4.3: priority with "Activate mana abilities" where the old engine passed for the player, a covering
payment that may go on; 100.3: triggers of different objects of one card ordered) makes the same actions
mean other things: the spots stop replaying (delver_archive's "does not replay on this engine",
library-score's stale ids).

`rerecord`: the old engine (`--old-engine`: a checkout, or a copy of its mtgz/ and
third_party/zero-core/python, whose built engine recorded the library; its engine_sha256 must be the
library's `engine.engine_sha256`, `--any-old-engine` waives it) replays every spot in a subprocess
(`trace`: each step's decider, decision kind, options' words, the action and its words, the exported
state). Then this engine walks the same game. Each old step is matched by the same decider, the same
decision kind, an option with the same words (the same action id first, then any other with those
words) and the whole exported state equal (all but `decision`, `log`, `log_n`, `v`: zones in order with
ids, tapped, counters, damage, the stack, pools, life, the libraries' order) up to the twins' ids: a card's
id (its object index) is no player's information, so the walk carries a pairing of this engine's ids with the
old engine's, and two cards may be paired anew only where no player can tell them apart - the same
definition and owner, at the same place of the same player's hidden zone (a library position, a hand slot),
with the same knowledge (who knows the card: `known`). Everything else is compared under the pairing, exactly
(docs/ENGINE.md 157: a rules change that moves one twin instead of the other - 152.1's pile pick - changes
which object is where, not the game). One more place: while a looked-at pile is put back card by card
(`PutOnTop`, `PutOnBottom`), the decider's not-yet-placed cards wait in transient places (by card index before
152.1, by place after); there the decider's library may differ within the top run of places the decider knows
(the pile), as a rearrangement of the same (definition, knowledge) cards - all else equal, no pairing changed -
and the next step is compared as any (the final library is a function of the definitions picked, 152.1). A
decision the old engine never asked is answered only as the old engine behaved:

* Priority whose options are exactly "Pass priority" and "Activate mana abilities": pass (the old engine
  passed for the player, who had no other option);
* PayMana offering "Pay the rest from the pool": that (the old engine ended the payment when the pool
  covered the rest; 100.4.3, 13.13);
* any other PayMana no old step matches (ENGINE.md 229.2: a payment's sources the game tells apart are
  options of their own, so a payment the old engine finished by itself - one class left - may ask): each
  option is tried, and the one whose later states equal the old engine's is kept (the permanent the old
  engine tapped: permanents are never paired anew, so at most one does);
* CastMana offering "Pay the cost" (ENGINE.md 231: a cast's 601.2g step, which the old engine never asked - it
  went straight to the payment): that;
* OrderTriggers, ChooseReplacement, OrderTimestamps (questions 100.3 added): each option is tried, and
  the one whose later states equal the old engine's is kept. When several do (the order shows in no
  recorded state: nobody acted while those triggers waited), the first is taken and the spot lists the
  decision in `rerecord.order_unseen`.

Anything else - another new question, a state that differs, the watched option not offered at the same
position - fails the spot: it moves to `dropped` with the reason. A re-recorded spot keeps its id, kind,
source, gid, k (the recording engine's decision index, the id's), seed, spec, watch, answer, why, review
and value (the recording network's), and gets the new `actions`, `sig`, `pos` (`delver_blunders.spot_pos`:
the exported state's hash at the spot) and `rerecord` (the epoch, the old path's length, the inserted
decisions' indices by question); an index of the old path (`k_main1`) is mapped onto the new path. The
file gets `rules_epoch` (tools/position_lab.py's RULES_EPOCH), this checkout's `engine`, and `recorded`
(every engine it was recorded on, oldest first).

`--knowledge-change` (a rules change of what the players know and nothing else - ENGINE.md 192: a look at the
opponent's library card its owner doesn't know is remembered): the exported states are compared without who knows
what (`known`'s per-card masks and seen hands; the libraries' order stays), so every action, zone, order, counter
and public fact must still match exactly, and the spot gets this engine's `sig` and `pos` with its knowledge (a spot
whose path is the same keeps its last re-record's record, `rerecord`).

`check`: every spot on this engine (its actions legal, the decider, `sig`, the watched option, `pos`) and
the file's `rules_epoch` against RULES_EPOCH. `--stamp`: when every spot reaches its `pos` exactly (the
rules changed, these positions did not), the file gets the current epoch and engine without an old
engine; when any spot doesn't, it refuses: rerecord with the engine that recorded the library.
"""
import argparse
import collections
import json
import os
import subprocess
import sys
import tempfile
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import delver_blunders as DB  # noqa: E402  (load_engine, engine_info, spot_sig, spot_pos, the library)

TRACE_FORMAT = 'delver-canary-trace/1'
PASS, FLOAT, POOL = 'Pass priority', 'Activate mana abilities', 'Pay the rest from the pool'
STEP_PASS = 'Pay the cost'   # a cast's 601.2g step (ENGINE.md 231)
ORDERS = ('OrderTriggers', 'ChooseReplacement', 'OrderTimestamps')
PILE = ('PutOnTop', 'PutOnBottom')        # a looked-at pile put back card by card (docs/ENGINE.md 152.1, 157)
OLD_INDEX_KEYS = ('k_main1',)             # spot fields that index the old path (the Bauble miner's main-1 decision)
NODES = 200000                            # the walk's budget a spot (a loud failure, never a cut path)



def _lay():
    """mtgz.az.layouts (docs/ENGINE.md 236): the game - layout v7 `mtg` or v8 `mtg_v8` - of the checkpoints this tool loads
    (`load_used`), every game it plays and encodes (`current`: layout v8 until a checkpoint is loaded)."""
    from mtgz.az import layouts
    return layouts

def new_game(E, sp):
    sj = json.loads(sp['spec'])
    sj['log'] = True
    return E.Game(_lay().current(), json.dumps(sj), 2, int(sp['seed']))


def options(st):
    return [(o['t'], int(o['a'])) for o in (st.get('decision') or {}).get('options') or []]


def strip(st):
    return {k: v for k, v in st.items() if k != 'log'}


# ------------------------------------------------------------------------------- the old engine's side
def trace_spot(E, sp):
    """The spot replayed on engine E -> {'steps': [{player, kind, labels, a, t, state}], 'final': {...}}
    (each step's state is the one before its action; `final` the spot's)."""
    g = new_game(E, sp)
    steps = []
    for k, a in enumerate(sp['actions']):
        if g.over:
            raise RuntimeError(f'the game is over at step {k}')
        legal = [int(x) for x in g.legal()]
        if int(a) not in legal:
            raise RuntimeError(f'step {k}: action {a} is not legal (legal {legal})')
        st = json.loads(g.export_state())
        d = st.get('decision') or {}
        words = dict((x, t) for t, x in options(st))
        steps.append({'player': d.get('player'), 'kind': d.get('kind'), 'labels': [t for t, _ in options(st)],
                      'a': int(a), 't': words.get(int(a)), 'state': strip(st)})
        g.step(int(a))
    st = json.loads(g.export_state())
    d = st.get('decision') or {}
    return {'steps': steps, 'final': {'player': d.get('player'), 'kind': d.get('kind'),
                                      'labels': [t for t, _ in options(st)], 'state': strip(st),
                                      'to_move': g.to_move, 'sig': DB.spot_sig(st)}}


def trace(E, spots):
    out = {}
    for sp in spots:
        try:
            tr = trace_spot(E, sp)
        except Exception as e:                                   # noqa: BLE001
            out[sp['id']] = {'error': f'does not replay on its recording engine: {e}'}
            continue
        fin = tr['final']
        if fin['sig'] != sp.get('sig', fin['sig']) or fin['to_move'] != sp['decider'] or sp['watch'] not in fin['labels']:
            out[sp['id']] = {'error': 'replays to another position on its recording engine (fingerprint, decider or '
                                      'watched option differ)'}
            continue
        out[sp['id']] = tr
    return out


# --------------------------------------------------------------------------------- this engine's walk
class Fail(Exception):
    pass


# (`--knowledge-change`: compare the states without who knows what; module docs)
KNOWLEDGE_CHANGE = False


def pos_of(st):
    out = {k: v for k, v in st.items() if k not in DB.POS_DROP}
    if KNOWLEDGE_CHANGE and 'known' in out:
        out['known'] = [{'order': k.get('order') or []} for k in out['known']]
    return out


def first_diff(a, b, path=''):
    """The first difference of two JSON values, worded (new vs old)."""
    if type(a) is not type(b):
        return f'{path or "."}: {json.dumps(a)[:90]} (new) vs {json.dumps(b)[:90]} (old)'
    if isinstance(a, dict):
        for k in sorted(set(a) | set(b)):
            if k not in a or k not in b:
                return f'{path}.{k}: {json.dumps(a.get(k))[:90]} (new) vs {json.dumps(b.get(k))[:90]} (old)'
            d = first_diff(a[k], b[k], f'{path}.{k}')
            if d:
                return d
        return None
    if isinstance(a, list):
        for i, (x, y) in enumerate(zip(a, b)):
            d = first_diff(x, y, f'{path}[{i}]')
            if d:
                return d
        if len(a) != len(b):
            return f'{path}: {len(a)} items (new) vs {len(b)} (old)'
        return None
    return None if a == b else f'{path}: {a!r} (new) vs {b!r} (old)'


# --------------------------------------------------------------------------- the twins' ids (docs/ENGINE.md 157)
def deck_names(sp):
    """The spot's game's card id -> definition name for every deck card: a game's objects are its decks' cards in
    the spec's list order, P0's then P1's (ids past them are tokens, public from their creation)."""
    d = json.loads(sp['spec']).get('decks') or {}
    return list(d.get('p0') or []) + list(d.get('p1') or [])


def renamed(st, sig):
    """The exported state `st` with its card ids renamed by `sig` (this engine's id -> the old engine's; an id
    without an entry keeps its number): every card object (zones, the command zone, the stack's sources), the
    references (`attached`, `blocking`, `attacks`, a target's `id`) and `known` (a hand card the other player knows,
    the libraries' order). Player and stack-object numbers are not card ids and are left alone."""
    if not sig:
        return st
    f = lambda x: sig.get(x, x)                                  # noqa: E731

    def card(o):
        o = dict(o)
        for k in ('id', 'attached', 'blocking', 'attacks'):
            if k in o:
                o[k] = f(o[k])
        return o

    out = dict(st)
    out['players'] = [dict(pl, **{z: [card(o) for o in pl.get(z) or []] for z in ('hand', 'graveyard', 'exile', 'battlefield')
                                  if z in pl}) for pl in st['players']]
    if 'command' in st:
        out['command'] = [card(o) for o in st['command']]
    out['stack'] = [dict(card(s), **({'targets': [card(t) if 'id' in t else t for t in s['targets']]} if 'targets' in s else {}))
                    for s in st.get('stack') or []]
    if 'known' in st:
        out['known'] = [dict(k, hand=[[f(i), n] for i, n in k.get('hand') or []], order=[f(i) for i in k.get('order') or []])
                        for k in st['known']]
    return out


def twin_match(P, Q, sig, names):
    """Whether this engine's state P is the old engine's Q up to the twins' ids -> the pairing (`sig`, extended), or
    None. P renamed by `sig` must equal Q, but for cards that `sig` may pair anew: two cards no player can tell apart
    - at the same place of the same player's hidden zone (its library's position, its hand's slot) in P and in Q,
    of the same definition (`names`: the decks' cards by id) and owner, with the same knowledge (a hand card: whether
    the other player knows it; a library card: the position's `known` mask, compared with the rest). A card of a
    public zone is never paired anew: it keeps the pairing it had when it was hidden (or its own number), so a twin
    told apart by anything - its place on the battlefield, a reference to it, a mark - stays told apart."""
    R = renamed(P, sig)
    if R == Q:
        return sig
    if len(R.get('known') or []) != len(Q.get('known') or []) or len(R['players']) != len(Q['players']):
        return None
    name = lambda x: names[x] if 0 <= x < len(names) else None   # noqa: E731
    new = {}                                                     # a renamed id of R -> the old id at its place
    for p, (rk, qk) in enumerate(zip(R['known'], Q['known'])):
        ro, qo = rk.get('order') or [], qk.get('order') or []
        rh, qh = R['players'][p].get('hand') or [], Q['players'][p].get('hand') or []
        if len(ro) != len(qo) or len(rh) != len(qh):
            return None
        r_seen = {i for i, _ in rk.get('hand') or []}
        q_seen = {i for i, _ in qk.get('hand') or []}
        pairs = [(x, y) for x, y in zip(ro, qo) if x != y]
        pairs += [(a['id'], b['id']) for a, b in zip(rh, qh)
                  if a['id'] != b['id'] and a.get('owner') == b.get('owner') and (a['id'] in r_seen) == (b['id'] in q_seen)]
        for x, y in pairs:
            if name(x) is None or name(x) != name(y) or new.get(x, y) != y:
                return None
            new[x] = y
    if not new or len(set(new.values())) != len(new):
        return None
    # the renamed ids that are re-paired: x (now meaning old y) - the old y's own holder in R must go to x's old id
    inv = {v: k for k, v in (sig or {}).items()}
    sig2 = dict(sig or {})
    for x, y in new.items():
        orig = inv.get(x, x)                                     # this engine's id that R shows as x
        sig2[orig] = y
    if len(set(sig2.values())) != len(sig2):
        return None
    return sig2 if renamed(P, sig2) == Q else None


def pile_match(P, Q, sig, p):
    """At player p's pile decision (PILE): whether P renamed by `sig` equals Q but for p's library within its top run
    of places p knows in both (the pile being put back), whose cards may wait in other places - the same multiset of
    (definition, knowledge mask) there, and the same places known. Nothing else may differ; the pairing is unchanged."""
    R = renamed(P, sig)
    rk, qk = R['known'][p], Q['known'][p]
    rl, ql = {i: (m, n) for i, m, n in rk['library']}, {i: (m, n) for i, m, n in qk['library']}
    top = 0
    while top in rl and top in ql and rl[top][0] & (1 << p) and ql[top][0] & (1 << p):
        top += 1
    if top < 2 or len(rk['order']) != len(qk['order']):
        return False
    if sorted(rl[i] for i in range(top)) != sorted(ql[i] for i in range(top)):
        return False

    def masked(st, k):
        return dict(st, known=[dict(x, library=[e for e in x['library'] if e[0] >= top], order=[None] * top + x['order'][top:])
                               if j == p else x for j, x in enumerate(st['known'])])
    return masked(R, rk) == masked(Q, qk)


def translate(E, sp, tr, budget=NODES, prefix=(), i0=0, check_orders=True):
    """The spot's path on engine E reaching the old trace's position -> {'actions', 'inserted' [(index, kind,
    words)], 'order_unseen' [index], 'matched' [new index of each old step], 'twins' [(new index, {this engine's
    id: the old one's}) where the pairing of the twins' ids changed], 'pile' [new index of each pile decision whose
    not-yet-placed cards waited elsewhere]}; raises Fail(reason)."""
    steps, fin = tr['steps'], tr['final']
    names = deck_names(sp)
    nodes = [0]
    far = {'i': -1, 'why': 'no decision reached'}

    def note(i, why):
        if i > far['i']:
            far['i'], far['why'] = i, why

    def rec(g, i, path, ins, matched, unseen, sig, twins, piles):
        nodes[0] += 1
        if nodes[0] > budget:
            raise Fail(f'more than {budget} decisions walked (the furthest old step {far["i"]}: {far["why"]})')
        if g.over:
            note(i, 'the game ended before the spot')
            return None
        st = json.loads(g.export_state())
        d = st.get('decision') or {}
        opts = options(st)
        labels = [t for t, _ in opts]
        tgt = steps[i] if i < len(steps) else fin
        cands = []
        if d.get('player') == tgt['player'] and d.get('kind') == tgt['kind']:
            P = pos_of(st)
            Q = pos_of(tgt['state'])
            sig2 = twin_match(P, Q, sig, names)
            if sig2 is None and d.get('kind') in PILE and i < len(steps) and pile_match(P, Q, sig, d['player']):
                sig2 = sig
                piles = piles + [len(path)]
            if sig2 is not None:
                if sig2 != sig:
                    twins = twins + [(len(path), {x: y for x, y in sig2.items() if sig.get(x, x) != y})]
                    sig = sig2
                if i == len(steps):
                    if sp['watch'] in labels:
                        return {'actions': path, 'inserted': ins, 'order_unseen': unseen, 'matched': matched,
                                'labels': labels, 'twins': twins, 'pile': piles}
                    raise Fail(f'the position is reached, but the watched option {sp["watch"]!r} is not offered at '
                               f'the spot ({labels})')
                else:
                    same = sorted((x for t, x in opts if t == tgt['t']), key=lambda x: x != tgt['a'])
                    cands += [('match', x) for x in same]
                    if not same:
                        note(i, f'old step {i} chose {tgt["t"]!r}, not offered ({labels})')
            else:
                note(i, f'old step {i} ({tgt["kind"]} P{tgt["player"]}): the state differs: {first_diff(renamed(P, sig), Q)}')
        else:
            note(i, f'old step {i} is {tgt["kind"]} P{tgt["player"]} ({tgt.get("t")!r}); this engine asks '
                    f'{d.get("kind")} P{d.get("player")} {labels}')
        kind = d.get('kind')
        if kind == 'Priority' and set(labels) == {PASS, FLOAT}:
            cands.append(('ins', dict(opts)[PASS]))
        elif kind == 'PayMana' and POOL in labels:
            cands.append(('ins', dict(opts)[POOL]))
        elif kind == 'CastMana' and STEP_PASS in labels:
            cands.append(('ins', dict(opts)[STEP_PASS]))
        elif kind == 'PayMana' and not any(how == 'match' for how, _ in cands):
            cands += [('ins', x) for _, x in opts]
        elif kind in ORDERS:
            cands += [('order', x) for _, x in opts]
        words = dict((x, t) for t, x in opts)
        found = None
        for how, a in cands:
            h = g.clone()
            h.step(int(a))
            if how == 'match':
                r = rec(h, i + 1, path + [int(a)], ins, matched + [len(path)], unseen, sig, twins, piles)
            else:
                r = rec(h, i, path + [int(a)], ins + [(len(path), kind, words[a])], matched, unseen, sig, twins, piles)
            if r is None:
                continue
            if how != 'order' or not check_orders:
                return r
            if found is None:
                found = r
                continue
            found['order_unseen'] = sorted(set(found['order_unseen']) | {len(path)})   # another order reaches it too
            return found
        if found is not None:
            return found
        return None

    g = new_game(E, sp)
    for a in prefix:
        g.step(int(a))
    lim = sys.getrecursionlimit()
    sys.setrecursionlimit(max(lim, 20000))
    try:
        r = rec(g, i0, list(prefix), [], [], [], {}, [], [])
    finally:
        sys.setrecursionlimit(lim)
    if r is None:
        raise Fail(f'no path on this engine reaches the position (the furthest old step {far["i"]} of '
                   f'{len(steps)}: {far["why"]})')
    return r


def rerecorded(sp, tr, r, epoch, sig, pos):
    """The spot with its new path (fields kept, `actions`, `sig`, `pos`, `rerecord` new, old-path indices mapped)."""
    by = collections.defaultdict(list)
    for j, kind, words in r['inserted']:
        by[f'{kind}: {words}'].append(j)
    new = dict(sp, actions=r['actions'], sig=sig, pos=pos)
    moved = {}
    for key in OLD_INDEX_KEYS:
        if isinstance(sp.get(key), int):
            k = sp[key]
            new[key] = r['matched'][k] if k < len(r['matched']) else len(r['actions'])
            if new[key] != k:
                moved[key] = k
    rr = {'epoch': epoch, 'old_steps': len(tr['steps']), 'inserted': dict(by)}
    if r['order_unseen']:
        rr['order_unseen'] = r['order_unseen']
    if r.get('twins'):
        # (where the twins' ids were paired anew: the new path's index, this engine's id -> the old engine's)
        rr['twins'] = [[j, {str(x): y for x, y in sorted(m.items())}] for j, m in r['twins']]
    if r.get('pile'):
        rr['pile'] = r['pile']        # (pile decisions whose not-yet-placed cards waited in other places)
    if moved:
        rr['old_indices'] = moved
    new['rerecord'] = rr
    return new


def run_trace(old, library, any_old):
    """`trace` in a subprocess on the old engine (two engine builds don't load in one process)."""
    with tempfile.TemporaryDirectory() as tmp:
        out = os.path.join(tmp, 'trace.json')
        env = dict(os.environ, PYTHONDONTWRITEBYTECODE='1')      # read-only use of the old checkout
        cmd = [sys.executable, os.path.abspath(__file__), 'trace', '--engine', old, '--library', library, '--out', out]
        p = subprocess.run(cmd, env=env, cwd=tmp)
        if p.returncode:
            raise SystemExit(f'the old engine\'s trace failed (exit {p.returncode}): {" ".join(cmd)}')
        with open(out, encoding='utf-8') as f:
            T = json.load(f)
    if T.get('format') != TRACE_FORMAT:
        raise SystemExit('the trace is not a ' + TRACE_FORMAT + ' file')
    return T


def git_ref(path):
    """'<commit>:<path>' when the file is tracked and unchanged since this checkout's HEAD (the old file stays
    readable there: `git show`), else None."""
    rel = os.path.relpath(os.path.abspath(path), ROOT).replace(os.sep, '/')
    if rel.startswith('..'):
        return None
    head = subprocess.run(['git', '-C', ROOT, 'rev-parse', 'HEAD'], capture_output=True, text=True).stdout.strip()
    same = subprocess.run(['git', '-C', ROOT, 'diff', '--quiet', 'HEAD', '--', rel]).returncode == 0
    tracked = subprocess.run(['git', '-C', ROOT, 'ls-files', '--error-unmatch', rel], capture_output=True).returncode == 0
    return f'{head[:12]}:{rel}' if head and same and tracked else None


def rerecord(L, T, E, epoch, info, log=DB.say, replaces=None):
    """Every spot of library L re-recorded on engine E from the old trace T -> (new library, report)."""
    spots, dropped, report = [], {}, []
    today = time.strftime('%Y-%m-%d')
    for sp in L['spots']:
        tr = T['spots'].get(sp['id'])
        t0 = time.time()
        if tr is None or 'error' in tr:
            why = (tr or {}).get('error', 'not in the trace')
        else:
            try:
                r = translate(E, sp, tr)
                why = None
            except Fail as e:
                why = str(e)
        if why:
            dropped[sp['id']] = f'rerecord {today} (rules epoch {epoch}): {why}'
            report.append({'id': sp['id'], 'ok': False, 'why': why})
            log(f'DROPPED {sp["id"]}: {why}')
            continue
        g = new_game(E, sp)
        for a in r['actions']:
            g.step(int(a))
        st = json.loads(g.export_state())
        if KNOWLEDGE_CHANGE and r['actions'] == sp['actions'] and not r['inserted'] and not r.get('twins') and not r.get('pile'):
            # (a knowledge change and the same path: the spot as it was - its last re-record's record kept - with this
            # engine's `sig` and `pos`)
            new = dict(sp, sig=DB.spot_sig(st), pos=DB.spot_pos(st))
        else:
            new = rerecorded(sp, tr, r, epoch, DB.spot_sig(st), DB.spot_pos(st))
        spots.append(new)
        old_l, new_l = set(tr['final']['labels']), set(r['labels'])
        rep = {'id': sp['id'], 'ok': True, 'old_steps': len(tr['steps']), 'steps': len(r['actions']),
               'inserted': collections.Counter(f'{k}: {w}' for _, k, w in r['inserted']),
               'options_added': sorted(new_l - old_l), 'options_removed': sorted(old_l - new_l),
               'order_unseen': r['order_unseen'], 'seconds': round(time.time() - t0, 2)}
        report.append(rep)
        log(f'{sp["id"]}: {len(tr["steps"])} -> {len(r["actions"])} steps {dict(rep["inserted"])}'
            + (f'; options added at the spot {rep["options_added"]}' if rep['options_added'] else '')
            + (f'; REMOVED {rep["options_removed"]}' if rep['options_removed'] else '')
            + (f'; order not in any recorded state at {r["order_unseen"]}' if r['order_unseen'] else ''))
    recorded = list(L.get('recorded') or [{'engine': L.get('engine'), 'rules_epoch': L.get('rules_epoch'),
                                           'note': 'the engine the spots were mined, added, reviewed and valued on (their '
                                                   '`value`, the `reference` scores)'}])
    recorded.append({'engine': info, 'rules_epoch': epoch, 'date': today,
                     'how': 'tools/delver_rerecord.py rerecord (docs/ENGINE.md 100.8)',
                     'spots': len(spots), 'dropped': sorted(dropped),
                     **({'replaces': replaces} if replaces else {})})
    out = dict(L, engine=info, rules_epoch=epoch, recorded=recorded, spots=spots,
               dropped=dict(L.get('dropped') or {}, **dropped))
    return out, report


# ------------------------------------------------------------------------------------------- check
def check_spots(E, L):
    """[(spot id, problem)] of the library's spots on engine E: actions legal, the decider, the fingerprint,
    the watched option, the position (`pos`, when stored)."""
    bad = []
    for sp in L['spots']:
        g = new_game(E, sp)
        why = None
        for k, a in enumerate(sp['actions']):
            if g.over or int(a) not in [int(x) for x in g.legal()]:
                why = f'step {k}: action {a} is not legal on this engine'
                break
            g.step(int(a))
        if why is None:
            st = json.loads(g.export_state())
            labels = [t for t, _ in options(st)]
            if g.to_move != sp['decider']:
                why = f'the decider is P{g.to_move}, the library says P{sp["decider"]}'
            elif DB.spot_sig(st) != sp['sig']:
                why = 'its fingerprint differs (sig)'
            elif sp['watch'] not in labels:
                why = f'the watched option {sp["watch"]!r} is not offered'
            elif 'pos' not in sp:
                why = 'no recorded position (pos): rerecord it'
            elif DB.spot_pos(st) != sp['pos']:
                why = 'another position (pos: the exported state differs)'
        if why:
            bad.append((sp['id'], why))
    return bad


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0], formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest='cmd', required=True)
    p = sub.add_parser('check', help="every spot on this checkout's engine, and the rules epoch")
    p.add_argument('--library', default=DB.LIBRARY_PATH)
    p.add_argument('--stamp', action='store_true', help='every spot reaches its position: record the current epoch')
    p = sub.add_parser('rerecord', help="every spot's path rebuilt on this engine from the old engine's trace")
    p.add_argument('--old-engine', required=True, help='the checkout (or a copy of its mtgz/ and zero-core python) '
                                                        'whose engine recorded the library')
    p.add_argument('--any-old-engine', action='store_true', help="don't require the library's engine_sha256")
    p.add_argument('--library', default=DB.LIBRARY_PATH)
    p.add_argument('--out', default=None, help='default: --library (rewritten)')
    p.add_argument('--report', default=None, help='the per-spot report (JSON)')
    p.add_argument('--knowledge-change', action='store_true',
                   help='a rules change of what the players know only: compare the states without who knows what')
    p = sub.add_parser('trace', help="(rerecord's subprocess) the spots on the old engine")
    p.add_argument('--engine', required=True)
    p.add_argument('--library', required=True)
    p.add_argument('--out', required=True)
    a = ap.parse_args(argv)
    L = DB.load_library(a.library)
    if L is None:
        raise SystemExit(f'{a.library}: no library')
    if a.cmd == 'trace':
        E = DB.load_engine(a.engine)
        T = {'format': TRACE_FORMAT, 'engine': DB.engine_info(a.engine), 'spots': trace(E, L['spots'])}
        with open(a.out, 'w', encoding='utf-8', newline='\n') as f:
            json.dump(T, f)
        bad = {k: v['error'] for k, v in T['spots'].items() if 'error' in v}
        DB.say(f'traced {len(T["spots"]) - len(bad)} of {len(T["spots"])} spots on {a.engine}' +
               ''.join(f'\n  {k}: {v}' for k, v in bad.items()))
        return T
    epoch = DB.rules_epoch(ROOT)
    E = DB.load_engine(ROOT)
    if a.cmd == 'check':
        bad = check_spots(E, L)
        for sid, why in bad:
            DB.say(f'  {sid}: {why}')
        DB.say(f'{a.library}: {len(L["spots"]) - len(bad)} of {len(L["spots"])} spots reach their position on this '
               f'engine; rules epoch {L.get("rules_epoch")} (this checkout: {epoch})')
        if a.stamp:
            if bad:
                raise SystemExit(f'--stamp refused: {len(bad)} spots do not reach their position; rerecord them '
                                 f'(--old-engine: a checkout of {(L.get("engine") or {}).get("commit")})')
            if L.get('rules_epoch') != epoch:
                info = DB.engine_info(ROOT)
                rec = list(L.get('recorded') or [])
                rec.append({'engine': info, 'rules_epoch': epoch, 'date': time.strftime('%Y-%m-%d'),
                            'how': 'tools/delver_rerecord.py check --stamp: every spot reached its position unchanged'})
                DB.save_library(dict(L, engine=info, rules_epoch=epoch, recorded=rec), a.library)
                DB.say(f'{a.library}: stamped with rules epoch {epoch}')
        elif bad or L.get('rules_epoch') != epoch:
            raise SystemExit(1)
        return bad
    # rerecord
    global KNOWLEDGE_CHANGE
    KNOWLEDGE_CHANGE = a.knowledge_change
    T = run_trace(a.old_engine, a.library, a.any_old_engine)
    want = (L.get('engine') or {}).get('engine_sha256')
    if not a.any_old_engine and T['engine'].get('engine_sha256') != want:
        raise SystemExit(f'--old-engine {a.old_engine}: its engine_sha256 is {T["engine"].get("engine_sha256")}, the '
                         f'library was recorded on {want} (--any-old-engine to waive)')
    info = DB.engine_info(ROOT)
    out, report = rerecord(L, T, E, epoch, info, replaces=git_ref(a.library))
    DB.save_library(out, a.out or a.library)
    if a.report:
        with open(a.report, 'w', encoding='utf-8', newline='\n') as f:
            json.dump({'old_engine': T['engine'], 'engine': info, 'rules_epoch': epoch, 'spots': report}, f, indent=1)
    n_ok = sum(1 for r in report if r['ok'])
    DB.say(f'{a.out or a.library}: {n_ok} of {len(report)} spots re-recorded on {info.get("commit")} '
           f'(engine {info.get("engine_sha256")}, rules epoch {epoch}); dropped {len(report) - n_ok}')
    return out


if __name__ == '__main__':
    main()
