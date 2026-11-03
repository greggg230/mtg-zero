"""Murktide delve spots: every Murktide Regent cast in a games file, its delve decisions searched exhaustively in
the engine - for each option, the most instants/sorceries the delve can still exile after it (the rest of the
delve played best). The first decision whose chosen option reaches fewer than the best option is a spot
(kind murktide_delve_short; watched = the chosen option, 'No' = stop delving, or a card that skips an
instant). Writes a spots file in delver_blunders' format (library-add takes it) + a summary. The delve: the
engine fixes the number of cards by the generic mana left to pay (or offers 'No' once paid), the picks go in
graveyard order (a pick skips every card before it), the forced remainder is taken without a decision, and
the cards reach exile with the payment - so a path's total is read from the exile zone at the end.
(docs/blunders.md 6a; the user's ask of 11-02.)

usage: python tools/delver_murktide_spots.py GAMES.games.json.gz OUT.json [max_spots] [max_games] [--engine CHECKOUT]
"""
import collections
import gzip
import json
import os
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import delver_blunders as B  # noqa: E402
import delver_fingerprints as F  # noqa: E402

args = [a for a in sys.argv[1:] if not a.startswith('--engine')]
ENG = next((a.split('=', 1)[1] for a in sys.argv[1:] if a.startswith('--engine=')), B.ROOT)
E = B.load_engine(ENG)
INST = F.INST_SORC
path, out_path = args[0], args[1]
max_spots = int(args[2]) if len(args) > 2 else 24
max_games = int(args[3]) if len(args) > 3 else 10 ** 9
rec = json.load(gzip.open(path, 'rt', encoding='utf-8'))
label = rec['label'] if '_s' in rec['label'] else f"{rec['label']}_s{rec['sims']}"


def exiled_inst(g, p):
    st = json.loads(g.export_state())
    return sum(c.get('name') in INST for c in st['players'][p].get('exile') or [])


def is_delve(g, p):
    if g.to_move != p:
        return False
    d = json.loads(g.export_state()).get('decision') or {}
    return d.get('task') == 'Delve'


def decision(g):
    return json.loads(g.export_state()).get('decision') or {}


def best_from(g, p, picks, inst, base, memo):
    """The most instants/sorceries this delve exiles in all (the caster's exile count at the end, minus `base`)
    from game g on. Picks reach exile with the payment, and the engine takes forced picks by itself (no
    decision), so the total is read from the exile zone at the end; `picks` / `inst` (the visible picks of this
    path) only key the memo: (the decision's options, picks made) -> the total minus `inst`."""
    while g.to_move == p and len(g.legal()) == 1:
        d = decision(g)
        if d.get('task') != 'Delve':
            break
        t = d['options'][0]['t']
        g.step(int(g.legal()[0]))
        if t.startswith('Choose '):
            picks += 1
            inst += t[7:] in INST
    d = decision(g)
    if g.to_move != p or d.get('task') != 'Delve':
        for _ in range(12):                                   # the payment: the exiles land with it
            d = decision(g)
            if g.to_move != p or d.get('kind') not in ('PayMana', 'PayPool', 'ChooseMana'):
                break
            g.step(int(g.legal()[0]))
        return exiled_inst(g, p) - base
    key = (json.dumps(d.get('options')), picks)
    if key in memo:
        return inst + memo[key]
    best = -1
    for o in d['options']:
        h = g.clone()
        h.step(int(o['a']))
        ch = o['t'].startswith('Choose ')
        best = max(best, best_from(h, p, picks + ch, inst + (ch and o['t'][7:] in INST), base, memo))
    memo[key] = best - inst
    return best


spots, C, by_type = [], collections.Counter(), collections.Counter()
for gi, row in enumerate(rec['rows'][:max_games]):
    d = B.rebuild(E, rec['spec'], rec['driver_seed'], row, rec.get('paired', False))
    G = B.Game(d)
    acts = [f['a'] for f in d['frames']]
    seed = B.game_seed(rec['driver_seed'], row['gid'], rec.get('paired', False))
    k = 0
    while k < G.n - 1:
        if G.lab[k] != 'Cast Murktide Regent':
            k += 1
            continue
        p = B.dec_of(G.seq[k])['player']
        C['casts'] += 1
        sp0 = {'spec': rec['spec'], 'seed': seed, 'actions': acts[:k]}
        g0 = B.spot_game(E, sp0)
        base = exiled_inst(g0, p)
        g0.step(int(acts[k]))
        total_best = best_from(g0.clone(), p, 0, 0, base, {})
        picks_done, inst_done = 0, 0
        found = None
        j = k + 1
        while j < G.n - 1 and B.dec_of(G.seq[j]).get('task') == 'Delve' and B.dec_of(G.seq[j]).get('player') == p:
            gj = B.spot_game(E, dict(sp0, actions=acts[:j]))
            memo = {}
            here = best_from(gj.clone(), p, picks_done, inst_done, base, memo)
            h = gj.clone()
            h.step(int(acts[j]))
            lab = G.lab[j] or ''
            ch = lab.startswith('Choose ')
            got = best_from(h, p, picks_done + ch, inst_done + int(ch and lab[7:] in INST), base, memo)
            if got < here and found is None:
                found = (j, lab, here, got)
            picks_done += ch
            inst_done += int(ch and lab[7:] in INST)
            j += 1
        if found:
            j, watch, here, got = found
            kind = 'stop' if watch == 'No' else 'skip'
            C['short'] += 1
            by_type[kind] += 1
            st = G.seq[j]
            gy = [c['name'] for c in st['players'][p]['graveyard']]
            if len(spots) < max_spots:
                spots.append({'kind': 'murktide_delve_short', 'source': label, 'gid': row['gid'], 'k': j, 'spec': rec['spec'],
                              'seed': seed, 'actions': acts[:j], 'watch': watch, 'decider': p, 'turn': st['turn'],
                              'step': st['step'], 'delve': {'best': here, 'got': got, 'how': kind, 'cast_best': total_best,
                                                            'graveyard': gy}})
        else:
            C['best'] += 1
        k = j if found else k + 1
    if len(spots) >= max_spots and gi >= 999:
        break
    C['games'] += 1
json.dump({'format': B.SPOTS_FORMAT, 'engine': B.engine_info(ENG), 'spots': spots}, open(out_path, 'w', encoding='utf-8'))
print(dict(C), dict(by_type), len(spots), 'spots ->', out_path)
for s in spots[:12]:
    print(s['gid'], s['k'], 'T', s['turn'], s['watch'], s['delve']['how'], 'best', s['delve']['best'], 'got', s['delve']['got'])
