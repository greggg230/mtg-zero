"""Bauble canaries for the heavy-decisions experiment (ENGINE.md 79.6): does the network look at its own
library with Mishra's Bauble before it commits to a fetch / a land, and does what it does next depend on the
card it saw? Over game records (spec, seed, multi-option moves): zero-replay/1 files (eval replays) and
self-play move logs (tools/light_eval.py / tools/heavy_eval.py play files). The games are rebuilt with this
checkout's engine (CPU); the card a look showed is read by passing a clone until the player draws (the
first card its library loses to its hand: measure-play's probe).

(a) Turn-1 hands: at a player's first main-phase priority of its first turn, the hand's composition
    ('clean': exactly one of Island / Volcanic Island, one fetchland, one Mishra's Bauble, one Tamiyo, the
    rest only Force of Will / Daze / Cori-Steel Cutter / Murktide Regent / Prismari Charm - the position
    lab's turn-1 composition; 'any': at least one of each of the four), and what it did that turn: a
    Bauble look at its own library before its land drop, the card seen, the land played (Island-type /
    fetch, cracked that turn or not).
(b) In general: episodes of a turn where the player has priority with an empty stack and both a fetch to
    crack (or play and crack) and a Bauble to cast or activate - did it look at its own library first, crack
    blind, or neither; and after any look at its own library with a fetch available, did it crack before its
    next draw, by the seen card's class (land / spell).

  python tools/heavy_canary.py --replays C:/.../runs/<run>/replays [--gen-min 360] [--play play.json ...] \\
      [--json out.json]
Importable: `game_records`, `canary`, `sources_from_replays`, `sources_from_play`.
"""
import argparse
import collections
import glob
import gzip
import json
import math
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
sys.path.insert(0, os.path.join(ROOT, 'tools'))

BAUBLE = "Mishra's Bauble"
TAMIYO = 'Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar'
ISLANDS = ('Island', 'Volcanic Island')
FETCHES = ('Flooded Strand', 'Misty Rainforest', 'Scalding Tarn', 'Polluted Delta')
T1_ALLOWED = ('Force of Will', 'Daze', 'Cori-Steel Cutter', 'Murktide Regent', 'Prismari Charm')
LANDS = set(ISLANDS) | set(FETCHES) | {'Wasteland', 'Thundering Falls'}


def engine():
    import mtgz.az  # noqa: F401
    from zero.games import engine as e
    return e()


def t1_composition(hand):
    c = collections.Counter(hand)
    isl, fe = sum(c[x] for x in ISLANDS), sum(c[x] for x in FETCHES)
    if not (isl and fe and c[BAUBLE] and c[TAMIYO]):
        return None
    rest = sum(v for k, v in c.items() if k not in ISLANDS + FETCHES + (BAUBLE, TAMIYO))
    ok = sum(v for k, v in c.items() if k in T1_ALLOWED)
    return 'clean' if (isl, fe, c[BAUBLE], c[TAMIYO]) == (1, 1, 1, 1) and rest == ok else 'any'


def state(g):
    s = json.loads(g.export_state())
    return s, {o['a']: o['t'] for o in (s.get('decision') or {}).get('options', [])}


def hand_names(s, p):
    return collections.Counter(c['name'] for c in s['players'][p]['hand'])


def probe_top(g, tp, max_steps=600):
    """The top card of player tp's library: a clone takes the lowest option everywhere (pass, keep on top,
    no attackers, no block, no, done, find nothing) until tp draws; the card its hand gains."""
    h = g.clone()
    s = json.loads(h.export_state())
    for _ in range(max_steps):
        if h.over or h.to_move is None:
            return None
        lib, hand, n0 = s['players'][tp]['library'], hand_names(s, tp), s.get('log_n', 0)
        h.step(min(h.legal()))
        s = json.loads(h.export_state())
        if s['players'][tp]['library'] < lib:
            gain = hand_names(s, tp) - hand
            if lib - s['players'][tp]['library'] == 1 and sum(gain.values()) == 1:
                return next(iter(gain))
            new = s.get('log_n', 0) - n0
            for ln in (s.get('log', [])[-new:] if new > 0 else []):
                m = re.search(r'^T\d+ \w+ (.*) Library -> Hand$', ln)
                if m and gain.get(m.group(1)):
                    return m.group(1)
            return None
    return None


def own_fetch(s, p):
    """p has a fetch to crack (untapped on its battlefield) or to play (in hand)."""
    me = s['players'][p]
    bf = any(c['name'] in FETCHES and c.get('controller', c.get('owner')) == p for c in me['battlefield'])
    return bf or any(c['name'] in FETCHES for c in me['hand'])


def is_crack(t):
    return t.startswith('Activate ') and t[9:] in FETCHES


def game_records(spec, seed, moves, tag='', gen=None):
    """One game's canary records: {'t1': [per player], 'episodes': [(b) episodes], 'looks': [(b) looks]}.
    `moves`: the game's multi-option decisions [(seat, action[, legal])]; single-option ones are stepped."""
    E = engine()
    sp = json.loads(spec) if isinstance(spec, str) else dict(spec)
    sp['log'] = True
    g = E.Game('mtg', json.dumps(sp), 2, int(seed))
    t1, episodes, looks = {}, [], []
    epi = {}                     # player -> the open (b) episode {turn, outcome}
    look_open = {}               # player -> an open look {seen, cls, turn, cracked}
    pending_look = {}            # player -> True when its Bauble ability targets its own library
    first_turn = {}
    def resolved(keys):
        # a look of a player's own library resolved: KEY_LOOK (kind 3) to it after its self-targeted Bauble
        for pl in list(pending_look):
            if any(k >> 56 == 3 and (k >> 48) & 255 == pl for k in keys):
                pending_look.pop(pl)
                seen = probe_top(g, pl)
                s2, _ = state(g)
                lk = {'player': pl, 'turn': s2['turn'], 'seen': seen, 'cls': 'land' if seen in LANDS else 'spell',
                      'fetch': own_fetch(s2, pl), 'cracked': False, 'tag': tag, 'gen': gen}
                if lk['fetch']:
                    looks.append(lk)
                    look_open[pl] = lk
                e = epi.get(pl)
                if e is not None and e['outcome'] == 'neither':
                    e['outcome'] = 'looked first'
                    episodes.append(epi.pop(pl))
                r2 = t1.get(pl)
                if r2 is not None and r2['land'] is None and s2['turn'] == r2['turn']:
                    r2['looked'], r2['seen'] = True, seen

    for mv in moves:
        p, a = int(mv[0]), int(mv[1])
        if len(mv) > 2 and mv[2] is not None and len(mv[2]) == 1:
            continue
        while not g.over and len(g.legal()) == 1:
            resolved(g.step(g.legal()[0]))
        if g.over:
            break
        s, lab = state(g)
        q, t = g.to_move, lab.get(a, '')
        turn, act, step = s['turn'], s['active'], s.get('step')
        if (s.get('decision') or {}).get('kind') not in ('Mulligan', 'PlayDraw', None) and turn >= 1:
            first_turn.setdefault(act, turn)
        # (a) the first main-phase priority of a player's first turn
        if q == act and turn == first_turn.get(act) and step in ('main1', 'main2') and act not in t1:
            hand = sorted(c['name'] for c in s['players'][act]['hand'])
            t1[act] = {'player': act, 'turn': turn, 'on_play': turn == 1, 'composition': t1_composition(hand),
                       'looked': False, 'seen': None, 'land': None, 'cracked': False, 'tag': tag, 'gen': gen}
        r = t1.get(q)
        live_t1 = r is not None and turn == r['turn'] and q == act
        # (b) an opportunity: empty stack, a fetch crack (or a fetch to play) and a Bauble to use
        empty = not s.get('stack')
        if (s.get('decision') or {}).get('kind') == 'Priority' and empty and q is not None:
            can_crack = any(is_crack(x) for x in lab.values()) or any(x.startswith('Play ') and x[5:] in FETCHES for x in lab.values())
            can_bauble = ('Activate ' + BAUBLE) in lab.values() or ('Cast ' + BAUBLE) in lab.values()
            e = epi.get(q)
            if e is not None and e['turn'] != turn:
                episodes.append(epi.pop(q))
                e = None
            if can_crack and can_bauble and e is None:
                epi[q] = e = {'player': q, 'turn': turn, 'own_turn': q == act, 'outcome': 'neither', 'tag': tag, 'gen': gen}
        # the move
        tgt_self = t == f'Target P{q + 1}' and s.get('stack') and s['stack'][-1].get('name') == BAUBLE
        if tgt_self:
            pending_look[q] = True
        if is_crack(t):
            e = epi.get(q)
            if e is not None and e['outcome'] == 'neither':
                e['outcome'] = 'blind crack'
                episodes.append(epi.pop(q))
            lk = look_open.get(q)
            if lk is not None and not lk['cracked']:
                lk['cracked'] = True
            if live_t1:
                r['cracked'] = True
        if live_t1 and t.startswith('Play ') and r['land'] is None:
            r['land'] = 'island' if t[5:] in ISLANDS else ('fetch' if t[5:] in FETCHES else 'other')
        resolved(g.step(a))
        # a look stays open until its player's next draw (its library shrinks in its own draw step)
        if not g.over:
            s3, _ = state(g)
            for pl, lk in list(look_open.items()):
                if s3['turn'] != lk['turn'] and s3['active'] == pl and s3.get('step') not in ('untap', 'upkeep'):
                    look_open.pop(pl)
    episodes.extend(epi.values())
    return {'t1': list(t1.values()), 'episodes': episodes, 'looks': looks}


def rate(k, n):
    return {'n': n, 'k': k, 'rate': round(k / n, 4) if n else None, 'se': round(math.sqrt(k / n * (1 - k / n) / n), 4) if n else None}


def canary(records):
    """Aggregate game_records -> the canary tables (small samples are reported with their n and se)."""
    t1 = [r for g in records for r in g['t1'] if r['composition']]
    out = {'t1': {}, 'b': {}}
    for comp in ('any', 'clean'):
        for side in ('play', 'draw'):
            rs = [r for r in t1 if (comp == 'any' or r['composition'] == 'clean') and r['on_play'] == (side == 'play')]
            looked = [r for r in rs if r['looked']]
            by = collections.Counter(f'{"land" if r["seen"] in LANDS else "spell"} seen -> {r["land"]}'
                                     f'{" cracked" if r["cracked"] else ""}' for r in looked)
            nolook = collections.Counter(f'{r["land"]}{" cracked" if r["cracked"] else ""}' for r in rs if not r['looked'])
            out['t1'][f'{comp} {side}'] = {'hands': len(rs), 'look_before_land': rate(len(looked), len(rs)),
                                           'after_a_look': dict(by), 'without_a_look': dict(nolook)}
    eps = [e for g in records for e in g['episodes']]
    oc = collections.Counter(e['outcome'] for e in eps)
    decided = oc['looked first'] + oc['blind crack']
    out['b']['episodes'] = dict(oc)
    out['b']['look_first'] = rate(oc['looked first'], decided)
    out['b']['look_first_own_turn'] = rate(sum(e['outcome'] == 'looked first' for e in eps if e['own_turn']),
                                           sum(e['outcome'] != 'neither' for e in eps if e['own_turn']))
    lks = [lk for g in records for lk in g['looks']]
    for cls in ('land', 'spell'):
        xs = [lk for lk in lks if lk['cls'] == cls]
        out['b'][f'crack_after_seen_{cls}'] = rate(sum(lk['cracked'] for lk in xs), len(xs))
    out['records'] = {'games': len(records), 't1_hands': len(t1), 'episodes': len(eps), 'looks': len(lks)}
    by = collections.defaultdict(lambda: collections.Counter())
    for g in records:
        for r in g['t1']:
            if r['composition']:
                by[r['gen']]['hands'] += 1
                by[r['gen']]['looked'] += r['looked']
        for e in g['episodes']:
            by[e['gen']]['episodes'] += 1
            by[e['gen']]['looked_first'] += e['outcome'] == 'looked first'
            by[e['gen']]['blind'] += e['outcome'] == 'blind crack'
    out['by_gen'] = {str(k): dict(v) for k, v in sorted(by.items(), key=lambda kv: (kv[0] is None, kv[0] or 0))}
    return out


def replay_seed(rep):
    """zero.replay's game seed of game `game_index` (play.Slot: rng.getrandbits(63) after two draws)."""
    import random
    rng = random.Random(rep['seed'])
    for _ in range(rep['game_index'] + 1):
        r = random.Random(rng.randrange(1 << 30))
    r.randrange(1 << 30)
    r.randrange(1 << 30)
    return r.getrandbits(63)


def sources_from_replays(d, gen_min=None):
    out = []
    for f in sorted(glob.glob(os.path.join(d, '*.json.gz'))):
        with gzip.open(f, 'rt', encoding='utf-8') as fh:
            rep = json.load(fh)
        m = re.search(r'Generation (\d+)', rep.get('title') or '')
        gen = int(m.group(1)) if m else None
        if gen_min is not None and (gen is None or gen < gen_min):
            continue
        out.append((rep['spec'], replay_seed(rep), [(fm['p'], fm['a']) for fm in rep['frames']], os.path.basename(f), gen))
    return out


def sources_from_play(path):
    """A play file (tools/light_eval.py / heavy_eval.py `play`): rows with move logs, the file's spec and seed."""
    from light_eval import game_seed
    d = json.load(open(path, encoding='utf-8'))
    gen = d.get('gen')
    return [(d['spec'], game_seed(int(d['seed']), int(r['gid'])), r['moves'], f'{os.path.basename(path)}:{r["gid"]}', gen)
            for r in d['rows']]


def sources_from_h2h(path, spec, seed, gen=None):
    """A zero.h2h output with `--rows --move-log N` (unpaired: each game's seed from the driver seed and its
    gid), the match's spec (the run's config.json 'spec') and `--seed`."""
    from light_eval import game_seed
    d = json.load(open(path, encoding='utf-8'))
    return [(spec, game_seed(int(seed), int(r['gid'])), r['moves'], f'{os.path.basename(path)}:{r["gid"]}', gen)
            for r in d['rows'] if r.get('moves')]


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--replays', default=None)
    ap.add_argument('--gen-min', type=int, default=None)
    ap.add_argument('--play', nargs='*', default=[])
    ap.add_argument('--h2h', nargs='*', default=[], help='zero.h2h outputs (--rows --move-log, unpaired)')
    ap.add_argument('--h2h-seed', type=int, default=None)
    ap.add_argument('--spec-config', default=None, help="a run's config.json (its 'spec': the h2h's games)")
    ap.add_argument('--gen', type=int, default=None, help="the h2h files' generation (the by_gen table)")
    ap.add_argument('--json', default=None)
    a = ap.parse_args(argv)
    if a.h2h and (a.h2h_seed is None or not a.spec_config):
        raise SystemExit('--h2h needs --h2h-seed and --spec-config (the games are rebuilt from their seeds)')
    os.environ.setdefault('CUDA_VISIBLE_DEVICES', '-1')
    src = (sources_from_replays(a.replays, a.gen_min) if a.replays else [])
    for p in a.play:
        src += sources_from_play(p)
    if a.h2h:
        spec = json.load(open(a.spec_config, encoding='utf-8'))['spec']
        for p in a.h2h:
            src += sources_from_h2h(p, spec, a.h2h_seed, a.gen)
    if not src:
        raise SystemExit('no game records: give --replays, --play or --h2h')
    recs = [game_records(*s) for s in src]
    out = canary(recs)
    print(json.dumps(out, indent=1))
    if a.json:
        with open(a.json, 'w', encoding='utf-8') as f:
            json.dump(out, f, indent=1)


if __name__ == '__main__':
    main()
