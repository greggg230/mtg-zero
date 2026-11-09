"""Re-record the Delver canary library after a rules change (docs/ENGINE.md 100.8): every spot's path
rebuilt on THIS checkout's engine to the same position, or the spot dropped with its reason - never an
approximate spot.

    python tools/delver_rerecord.py check [--library L] [--stamp]
    python tools/delver_rerecord.py rerecord --old-engine CHECKOUT [--library L] [--out L] [--report R.json]
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
ids, tapped, counters, damage, the stack, pools, life, the libraries' order). A decision the old engine
never asked is answered only as the old engine behaved:

* Priority whose options are exactly "Pass priority" and "Activate mana abilities": pass (the old engine
  passed for the player, who had no other option);
* PayMana offering "Pay the rest from the pool": that (the old engine ended the payment when the pool
  covered the rest; 100.4.3, 13.13);
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
ORDERS = ('OrderTriggers', 'ChooseReplacement', 'OrderTimestamps')
OLD_INDEX_KEYS = ('k_main1',)             # spot fields that index the old path (the Bauble miner's main-1 decision)
NODES = 200000                            # the walk's budget a spot (a loud failure, never a cut path)


def new_game(E, sp):
    sj = json.loads(sp['spec'])
    sj['log'] = True
    return E.Game('mtg', json.dumps(sj), 2, int(sp['seed']))


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


def pos_of(st):
    return {k: v for k, v in st.items() if k not in DB.POS_DROP}


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


def translate(E, sp, tr, budget=NODES, prefix=(), i0=0, check_orders=True):
    """The spot's path on engine E reaching the old trace's position -> {'actions', 'inserted' [(index, kind,
    words)], 'order_unseen' [index], 'matched' [new index of each old step]}; raises Fail(reason)."""
    steps, fin = tr['steps'], tr['final']
    nodes = [0]
    far = {'i': -1, 'why': 'no decision reached'}

    def note(i, why):
        if i > far['i']:
            far['i'], far['why'] = i, why

    def rec(g, i, path, ins, matched, unseen):
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
            if P == Q:
                if i == len(steps):
                    if sp['watch'] in labels:
                        return {'actions': path, 'inserted': ins, 'order_unseen': unseen, 'matched': matched,
                                'labels': labels}
                    raise Fail(f'the position is reached, but the watched option {sp["watch"]!r} is not offered at '
                               f'the spot ({labels})')
                else:
                    same = sorted((x for t, x in opts if t == tgt['t']), key=lambda x: x != tgt['a'])
                    cands += [('match', x) for x in same]
                    if not same:
                        note(i, f'old step {i} chose {tgt["t"]!r}, not offered ({labels})')
            else:
                note(i, f'old step {i} ({tgt["kind"]} P{tgt["player"]}): the state differs: {first_diff(P, Q)}')
        else:
            note(i, f'old step {i} is {tgt["kind"]} P{tgt["player"]} ({tgt.get("t")!r}); this engine asks '
                    f'{d.get("kind")} P{d.get("player")} {labels}')
        kind = d.get('kind')
        if kind == 'Priority' and set(labels) == {PASS, FLOAT}:
            cands.append(('ins', dict(opts)[PASS]))
        elif kind == 'PayMana' and POOL in labels:
            cands.append(('ins', dict(opts)[POOL]))
        elif kind in ORDERS:
            cands += [('order', x) for _, x in opts]
        words = dict((x, t) for t, x in opts)
        found = None
        for how, a in cands:
            h = g.clone()
            h.step(int(a))
            if how == 'match':
                r = rec(h, i + 1, path + [int(a)], ins, matched + [len(path)], unseen)
            else:
                r = rec(h, i, path + [int(a)], ins + [(len(path), kind, words[a])], matched, unseen)
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
        r = rec(g, i0, list(prefix), [], [], [])
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
