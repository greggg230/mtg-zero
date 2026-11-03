"""Murktide delve spots by the user's rule (11-02). A delve is short when, after all its picks:
  1) fewer than 5 instants/sorceries were exiled, and
  2) the caster's graveyard still holds 2+ instants or 2+ sorceries (one of each left can be right: delirium).
A cast counts when its actual delve is short and exiled fewer instants/sorceries than the graveyard allowed (the
Murktide lost a counter: a delve that kept its size but left two of one type, e.g. a sorcery exiled where an
instant would have left one of each, is not a spot - the user, 11-02, spot 55). The engine searches every delve
choice on the actual path; the spot is the first pick that lowers the most instants/sorceries the delve can
still exile (the pick that cost the counter; watched = that pick, 'No' = stop delving).

The delve: the engine fixes the number of cards by the generic mana left to pay (or offers 'No' once paid), the
picks go in graveyard order (a pick skips every card before it), the forced remainder is taken without a
decision, and the cards reach exile with the payment - so a delve's outcome is read from the zones at the end.

(docs/blunders.md 6a; replaces the 11-02 first rule, which flagged picks that still reached every instant.)

usage: python tools/delver_murktide_spots.py GAMES.games.json.gz OUT.json [max_spots] [max_games] [--engine=CHECKOUT]
"""
import collections
import gzip
import json
import os
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import delver_blunders as B  # noqa: E402

INSTANTS = {'Brainstorm', 'Daze', 'Force of Will', 'Lightning Bolt', 'Unholy Heat', 'Prismari Charm', 'Consign to Memory',
            'Force of Negation', 'Hydroblast', 'Pyroblast', 'Price of Progress'}
SORCERIES = {'Ponder', 'Meltdown', 'Rough // Tumble'}
PAYMENT = ('PayMana', 'PayPool', 'ChooseMana')

args = [a for a in sys.argv[1:] if not a.startswith('--engine=')]
ENG = next((a.split('=', 1)[1] for a in sys.argv[1:] if a.startswith('--engine=')), B.ROOT)
E = B.load_engine(ENG)
path, out_path = args[0], args[1]
max_spots = int(args[2]) if len(args) > 2 else 12
max_games = int(args[3]) if len(args) > 3 else 10 ** 9


def decision(g):
    return json.loads(g.export_state()).get('decision') or {}


def kinds_in(cards):
    names = [c.get('name') for c in cards]
    return sum(n in INSTANTS for n in names), sum(n in SORCERIES for n in names)


def outcome(st, p, base):
    """(short, exiled instants+sorceries, instants left, sorceries left) of a finished delve."""
    pl = st['players'][p]
    ei, es = kinds_in(pl.get('exile') or [])
    gi, gs = kinds_in(pl.get('graveyard') or [])
    exiled = ei + es - base
    return exiled < 5 and (gi >= 2 or gs >= 2), exiled, gi, gs


def finish(g, p):
    for _ in range(12):                                   # the payment: the exiles land with it
        d = decision(g)
        if g.to_move != p or d.get('kind') not in PAYMENT:
            break
        g.step(int(g.legal()[0]))
    return json.loads(g.export_state())


def search(g, p, key0, base, memo):
    """(can a not-short delve still be reached, the most instants/sorceries it can exile) from game g on. key0:
    (picks, instants picked, sorceries picked) of the visible picks so far - with the decision's options it
    fixes the rest (the forced remainder is taken by the engine)."""
    picks, pi, ps = key0
    while g.to_move == p and len(g.legal()) == 1:
        d = decision(g)
        if d.get('task') != 'Delve':
            break
        t = d['options'][0]['t']
        g.step(int(g.legal()[0]))
        if t.startswith('Choose '):
            picks, pi, ps = picks + 1, pi + (t[7:] in INSTANTS), ps + (t[7:] in SORCERIES)
    d = decision(g)
    if g.to_move != p or d.get('task') != 'Delve':
        short, exiled, _, _ = outcome(finish(g, p), p, base)
        return (not short), exiled
    key = (json.dumps(d.get('options')), picks, pi, ps)
    if key not in memo:
        ok, best = False, -1
        for o in d['options']:
            h = g.clone()
            h.step(int(o['a']))
            ch = o['t'].startswith('Choose ')
            n = o['t'][7:] if ch else ''
            a, b = search(h, p, (picks + ch, pi + (n in INSTANTS), ps + (n in SORCERIES)), base, memo)
            ok, best = ok or a, max(best, b)
        memo[key] = (ok, best)
    return memo[key]


rec = json.load(gzip.open(path, 'rt', encoding='utf-8'))
label = rec['label'] if '_s' in rec['label'] else f"{rec['label']}_s{rec['sims']}"
spots, C = [], collections.Counter()
for row in rec['rows'][:max_games]:
    d = B.rebuild(E, rec['spec'], rec['driver_seed'], row, rec.get('paired', False))
    G = B.Game(d)
    acts = [f['a'] for f in d['frames']]
    seed = B.game_seed(rec['driver_seed'], row['gid'], rec.get('paired', False))
    C['games'] += 1
    for k in range(G.n - 1):
        if G.lab[k] != 'Cast Murktide Regent':
            continue
        p = B.dec_of(G.seq[k])['player']
        C['casts'] += 1
        sp0 = {'spec': rec['spec'], 'seed': seed, 'actions': acts[:k]}
        base = sum(kinds_in(G.seq[k]['players'][p].get('exile') or []))
        # the actual delve: the steps after the cast while the caster delves or pays
        m = k + 1
        while m < G.n - 1 and B.dec_of(G.seq[m]).get('player') == p and (
                B.dec_of(G.seq[m]).get('task') == 'Delve' or B.dec_of(G.seq[m]).get('kind') in PAYMENT):
            m += 1
        short, exiled, gi, gs = outcome(G.seq[m], p, base)
        if not short:
            C['ok'] += 1
            continue
        C['short'] += 1
        g1 = B.spot_game(E, dict(sp0, actions=acts[:k + 1]))
        memo = {}
        avoidable, best_total = search(g1, p, (0, 0, 0), base, memo)
        if exiled >= best_total:
            C['short, the most counters it could get'] += 1      # only what was left behind differs: not a spot
            continue
        C['short, a counter lost'] += 1
        picks, pi, ps, found = 0, 0, 0, None
        for j in range(k + 1, m):
            dj = B.dec_of(G.seq[j])
            if dj.get('task') != 'Delve':
                continue
            gj = B.spot_game(E, dict(sp0, actions=acts[:j]))
            here = search(gj.clone(), p, (picks, pi, ps), base, memo)
            lab = G.lab[j] or ''
            ch = lab.startswith('Choose ')
            n = lab[7:] if ch else ''
            nxt = (picks + ch, pi + (n in INSTANTS), ps + (n in SORCERIES))
            h = gj.clone()
            h.step(int(acts[j]))
            after = search(h, p, nxt, base, memo)
            if after[1] < here[1]:
                found = (j, lab, here[1], after[1])
                break
            picks, pi, ps = nxt
        if found is None:
            C['short, no single pick found'] += 1
            continue
        j, watch, best_here, best_after = found
        if len(spots) < max_spots:
            st = G.seq[j]
            spots.append({'kind': 'murktide_delve_short', 'source': label, 'gid': row['gid'], 'k': j, 'spec': rec['spec'],
                          'seed': seed, 'actions': acts[:j], 'watch': watch, 'decider': p, 'turn': st['turn'], 'step': st['step'],
                          'delve': {'rule': 'user-11-02', 'exiled': exiled, 'left_instants': gi, 'left_sorceries': gs,
                                    'best': best_here, 'got': best_after,
                                    'graveyard': [c['name'] for c in st['players'][p]['graveyard']]}})
    if len(spots) >= max_spots and C['games'] >= 1000:
        break
json.dump({'format': B.SPOTS_FORMAT, 'engine': B.engine_info(ENG), 'spots': spots}, open(out_path, 'w', encoding='utf-8'))
print(dict(C), len(spots), 'spots ->', out_path)
for s in spots:
    dv = s['delve']
    print(s['gid'], s['k'], 'T', s['turn'], s['watch'], f"| exiled {dv['exiled']}, left {dv['left_instants']} inst {dv['left_sorceries']} sorc",
          f"| best reachable {dv['best']} -> after {dv['got']}")
