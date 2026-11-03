"""Blunder measurement for the Legacy Izzet Delver mirror (docs/blunders.md): specific mistakes per checkpoint,
as rates with standard errors, from a fixed seeded set of evaluation-setting games - next to Elo, before
and during a training fork. Read-only for runs (writes only under --out).

  python tools/delver_blunders.py run CKPT [CKPT ...] --out DIR [--games 2000] [--sims N] [--seed 20261102]
      [--opp self | ANCHOR.pt] [--engine CHECKOUT] [--device cuda] [--conc 1000] [--jobs 6] [--diff 2-1]
  python tools/delver_blunders.py play CKPT [CKPT ...] --out DIR ...     # the games only (move logs)
  python tools/delver_blunders.py scan DIR/x.games.json.gz [...]         # rebuild + count -> <label>.scan.json
  python tools/delver_blunders.py report DIR/*.scan.json [--diff 2-1,4-3] [--json OUT]   # the table
  python tools/delver_blunders.py replays RUN [--min-gen G --max-gen H]  # the same counts over recorded
                                                                         # zero-replay/1 games (no rebuild)

The games. Each checkpoint plays itself (`--opp self`, the default) with its run's evaluation seat on both
sides: zero.h2h's match (its run's chance_k, the 'cq' move rule, root-view leaves, the belief tilt -
h2h.run_rule), unpaired, driver seed `--seed`, so every checkpoint gets the same game seeds (the same
opening deals; the games diverge from there), with the driver's move log. The search budget is `--sims`
(default: the run's eval_sims, 32; every decision gets it, mana payments too, as zero.h2h's and the
Delver Lab Play tab's seats); a scan's label is <run>_<iter>_s<sims>. Both seats are the checkpoint, so
every decision counts. `--opp ANCHOR.pt`: the checkpoint against a fixed anchor (its own run's evaluation
seat), only the checkpoint's seat counted. The games are then rebuilt in Python from their seeds and
move logs (`rebuild`: zero-core's Driver derives a game's engine seed from the driver seed and its gid -
`game_seed`), the spectator view at every engine step, and scanned.

The engine. `--engine CHECKOUT` (default: this checkout) is the mtg-zero checkout whose mtgz (its built
mtgz/_engine.pyd and pinned zero-core) plays and rebuilds the games: e.g. a float-mana build later. Its
git commit is recorded in every output file; a scan rebuilds with the engine given to it (the same one
that played the games - a different build stops at the first move it cannot replay).

The standard errors are cluster (per-game) standard errors of a ratio: games are independent, the
events of one game are not (two seats of one game, the same Daze opportunity at two decisions).

Metrics (a "mana land" = Volcanic Island, Island, Thundering Falls, Wasteland; a "fetch" = the four
fetchlands; the AI = the counted players: both seats with --opp self). Exact definitions in the
functions below; docs/blunders.md has a summary.

  Daze          the AI casts Daze at an opponent's spell; payable = the spell's controller could pay
                {1} at the cast: floating mana in its pool, else an untapped mana land, else only an
                uncracked fetch (crack in response, fetch an untapped land); paid = it was asked at
                Daze's resolution and paid. Daze at its own spell (a Cori-Steel Cutter flurry trigger)
                is counted apart.
  floating      (an engine with mana floating; 0 elsewhere by construction: the current engine never
                leaves mana in a pool outside a payment) a float = mana in a player's pool that outlives
                the action that made it (`floats`); spent = the pool fell again before the step ended;
                pointless = nothing of it spent. Opportunities: Wasteland targeting a player's untapped
                mana land (used = the land tapped for mana before it died, any way: floating or paying
                for a spell; missed spend = not used while an instant the land could help pay -
                Brainstorm, Lightning Bolt, Unholy Heat, Prismari Charm - was a legal cast), and Daze
                returning an untapped Island (floated = the caster floated while casting it; missed spend
                = no float with such an instant in hand); a Daze returning an untapped Island while a
                tapped one was there, without a float, is a mana lost on any engine.
  burn          Lightning Bolt / Unholy Heat resolving at an opponent's creature: the creature
                survived; could not kill (damage dealt < toughness - damage already marked); into an
                already damaged creature that survived (the Murktide double-Bolt pattern).
  land drop     a non-cantrip spell cast in the caster's own main phase with its land drop unused and
                an untapped-entering land in hand that left it with no untapped land (mana land or
                fetch), then that land played later the same turn (the 11-01 scan's definition).
  Tamiyo        the AI's own draw spell or Clue (Brainstorm, Ponder, Prismari Charm, Clue) drew the third
                card of its own turn and flipped its Tamiyo, Inquisitive Student while she could attack
                (not summoning sick; untapped, or attacked this turn; no untapped opposing flier or
                reach creature with power 3+): before her attack = before the declare-attackers step,
                the Clue her attack makes lost (the 11-02 Play-tab report); clean board = no Dragon's
                Rage Channeler, Monk or Cori-Steel Cutter of the AI's (no surveil, prowess or flurry
                upside to a cantrip before combat).
  habits        tools/delver_fingerprints.py's metrics (fetch cracked at once, Brainstorm with a
                shuffle, Murktide delve, FoW pitches, tap-out into Daze, T1-2 Volcanic, ...).
"""
import argparse
import collections
import gzip
import hashlib
import json
import math
import os
import re
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import delver_fingerprints as F  # noqa: E402  (pure Python: the habit metrics, the run lookup)

SUITE_VERSION = 3                          # bump when a metric's definition changes (the baseline records it)
GAMES_FORMAT = 'delver-blunders-games/1'
BASELINE_FORMAT = 'delver-blunder-baseline/1'
BASELINE_PATH = os.path.join(ROOT, 'data', 'delver_blunder_baseline.json')
SCAN_FORMAT = 'delver-blunders-scan/1'
DEFAULT_SEED = 20261102
MANA_LANDS = {'Volcanic Island': 'UR', 'Island': 'U', 'Thundering Falls': 'UR', 'Wasteland': 'C'}
ENTERS_TAPPED = {'Thundering Falls'}
ISLANDS = F.ISLANDS                                       # Daze returns an Island (by type)
FETCH = F.FETCH
INSTANTS = {'Brainstorm': 'U', 'Lightning Bolt': 'R', 'Unholy Heat': 'R', 'Prismari Charm': 'UR'}
INSTANT_COST = {'Brainstorm': 1, 'Lightning Bolt': 1, 'Unholy Heat': 1, 'Prismari Charm': 2}
BURN = {'Lightning Bolt', 'Unholy Heat'}
CANTRIPS = {'Brainstorm', 'Ponder'}
TAMIYO = 'Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar'
TAMIYO_FLIP = f'{TAMIYO} Battlefield -> Exile'            # her transform: exiled, back as the planeswalker
TAMIYO_TRIGGER = f'trigger of {TAMIYO} goes on the stack'  # her attack trigger and her third-draw trigger
LOG_STEP = re.compile(r'^T\d+ (\w+) ')                     # a log line's step: 'T4 Main1 ...'
BEFORE_ATTACKS = ('Untap', 'Upkeep', 'Draw', 'Main1', 'BeginCombat')
TAMIYO_DRAWERS = {'Brainstorm', 'Ponder', 'Prismari Charm', 'Clue'}       # the stack objects that draw
COMBAT_UPSIDE = {"Dragon's Rage Channeler", 'Monk', 'Cori-Steel Cutter'}  # surveil / prowess / flurry
LANDS = set(MANA_LANDS) | FETCH
TARGET_RE = re.compile(r'target (.+?)#(\d+)')
DMG_RE = re.compile(r'(\d+) damage to ')


def say(*a):
    """The tool's log: print, flushed (a watcher's console is a file)."""
    print(*a, flush=True)


# ====================================================================================== the scan
def pool(ps):
    """The number of mana in a player's pool (the view's 'pool': one letter a mana, absent when empty)."""
    return len(ps.get('pool') or '')


def lands_of(ps):
    return [c for c in ps.get('battlefield') or [] if 'land' in c.get('types', ['land'])]


def untapped_mana(ps):
    return [c for c in lands_of(ps) if c.get('name') in MANA_LANDS and not c.get('tapped')]


def untapped_fetch(ps):
    return [c for c in lands_of(ps) if c.get('name') in FETCH and not c.get('tapped')]


def dec_of(s):
    return s.get('decision') or {}


def label_of(s, a):
    if a is None:
        return None
    for o in dec_of(s).get('options') or []:
        if o.get('a') == a:
            return o.get('t')
    return None


class Game:
    """A game as a sequence of states: seq[k] is the state at the k-th recorded step (the last one the
    final state), lab[k] the option chosen there (None at the end). Rebuilt games have a state at every
    engine step; zero-replay/1 games at every frame (forced picks have none: coarser, still usable)."""

    def __init__(self, d):
        st, fr = d['states'], d['frames']
        self.seq = [st[fm.get('s', k)] for k, fm in enumerate(fr)]
        self.lab = [label_of(s, fm.get('a')) for s, fm in zip(self.seq, fr)]
        fin = st[d.get('final', len(st) - 1)]
        if not self.seq or fin is not self.seq[-1]:
            self.seq.append(fin)
            self.lab.append(None)
        self.n = len(self.seq)
        self.spots = []                             # (kind, decision index, watched option): canary candidates

    def new_log(self, k):
        """The log lines that appeared from seq[k - 1] to seq[k]."""
        if k <= 0:
            return list(self.seq[0].get('log') or [])
        a, b = self.seq[k - 1].get('log_n', 0), self.seq[k].get('log_n', 0)
        lines = self.seq[k].get('log') or []
        new = b - a
        if new <= 0:
            return []
        return lines[-new:] if new <= len(lines) else list(lines)

    def segment(self, k):
        s = self.seq[k]
        return (s.get('turn'), s.get('step'))

    def next_priority(self, k):
        """The first index after k whose decision is a Priority one (a cast or an activation is
        complete: costs paid, targets chosen), or None."""
        for j in range(k + 1, self.n):
            if dec_of(self.seq[j]).get('kind') == 'Priority':
                return j
        return None

    def stack_window(self, sid, j0):
        """(first, last) indices from j0 on while stack object `sid` is on the stack."""
        j = j0
        while j + 1 < self.n and any(o.get('stack') == sid for o in self.seq[j + 1].get('stack') or []):
            j += 1
        return j0, j


def obj_on_bf(s, oid):
    for p in s['players']:
        for c in p.get('battlefield') or []:
            if c.get('id') == oid:
                return p['id'], c
    return None, None


PAYMENT = ('PayMana', 'PayPool')            # mid-payment decisions: a pool there may be a payment's
LOG_FLOAT_KINDS = ('Priority', 'BounceCost')  # where a float is a decision of its own (not a cast's payment)


def float_cause(G, p, k):
    """Why player p floated mana at the step from seq[k]: 'wasteland' (an opponent's Wasteland ability
    targeting one of p's lands is on the stack), 'daze' (p is returning an Island for its Daze: the
    decision is Daze's alternative cost, or p returns an Island within the next steps while its Daze is
    on the stack), else 'other'."""
    s = G.seq[k]
    for o in s.get('stack') or []:
        if o.get('name') == 'Wasteland' and o.get('ability') and o.get('controller') != p and any(
                obj_on_bf(s, t.get('id'))[0] == p for t in o.get('targets') or []):
            return 'wasteland'
    if dec_of(s).get('kind') in ('BounceCost', 'ChooseAltCost', 'Sacrifice'):
        return 'daze' if any(o.get('name') == 'Daze' and o.get('controller') == p for o in s.get('stack') or []) else 'cost'
    if any(o.get('name') == 'Daze' and o.get('controller') == p for o in s.get('stack') or []):
        for x in range(k + 1, min(G.n, k + 4)):
            if any(f'P{p} returns ' in ln for ln in G.new_log(x)):
                return 'daze'
    return 'other'


FLOAT_LABEL = re.compile(r'^Tap (.+) for mana')   # the float build's options (its ENGINE.md 86.4): at priority
#   "Tap Volcanic Island for mana", in a cost "Tap Island for mana, then return it to hand"


def floats(G, p):
    """Player p's floats: [(index, units, spent units, cause, decision index)], read from the decisions
    (ENGINE.md 86 of the float build: a mana ability taken outside a payment is the option 'Tap <source>
    for mana' at priority, or '... then return it to hand' / '... then sacrifice it' in a cost; a
    Volcanic Island then asks ChooseMana 'Add {U}' / 'Add {R}'). Index: the first state of the step whose
    pool shows it (the decision's next state when none does: the step ended first). Spent: the pool's
    falls after that, before the step ended. Cause: `float_cause` at the decision. A float no option names
    - p's pool larger at a state outside a payment than at the previous such state, with no such decision
    since - comes out with cause 'unlabeled' (none expected: a check of the detector)."""
    out, arrivals = [], set()
    for k in range(G.n - 1):
        d = dec_of(G.seq[k])
        if d.get('player') != p or d.get('kind') in PAYMENT or not FLOAT_LABEL.match(G.lab[k] or ''):
            continue
        seg, base = G.segment(k), pool(G.seq[k]['players'][p])
        arr = None
        for x in range(k + 1, G.n):
            if G.segment(x) != seg:
                break
            if pool(G.seq[x]['players'][p]) > base:
                arr = x
                break
        spent = 0
        if arr is not None:
            prev = pool(G.seq[arr]['players'][p])
            for x in range(arr + 1, G.n):
                if G.segment(x) != seg:
                    break
                v = pool(G.seq[x]['players'][p])
                spent += max(0, prev - v)
                prev = v
            arrivals.add(arr)
        out.append((arr if arr is not None else k + 1, 1, min(1, spent), float_cause(G, p, k), k))
    # pool growth no option explains (a detector check)
    k = 0
    while k < G.n:
        seg = G.segment(k)
        j = k
        while j + 1 < G.n and G.segment(j + 1) == seg:
            j += 1
        prev = 0
        for i in range(k, j + 1):
            if dec_of(G.seq[i]).get('kind') in PAYMENT and i != j:
                continue
            v = pool(G.seq[i]['players'][p])
            if v > prev and i not in arrivals and not any(e[4] < i <= e[0] for e in out):
                out.append((i, v - prev, 0, 'unlabeled', i))
            prev = v
        k = j + 1
    return sorted(out)


def scan_daze(G, C, players, ex):
    """Daze cast at an opponent's spell: can its controller pay {1}, and does it."""
    for k in range(G.n - 1):
        if G.lab[k] != 'Cast Daze':
            continue
        s = G.seq[k]
        p = dec_of(s).get('player')
        if players is not None and p not in players:
            continue
        before = {o.get('stack') for o in s.get('stack') or []}
        daze = jd = None
        for j in range(k + 1, min(G.n, k + 16)):
            new = [o for o in G.seq[j].get('stack') or [] if o.get('name') == 'Daze' and o.get('spell')
                   and o.get('controller') == p and o.get('stack') not in before]
            if new and new[0].get('targets'):
                daze, jd = new[0], j
                break
        if daze is None:
            continue
        # (a spell target is the spell's card: {id, name, live}; the stack object of a spell has its card's id)
        t = daze['targets'][0]
        tobj = next((o for o in G.seq[jd].get('stack') or [] if o.get('spell') and (
            o.get('id') == t.get('id') if 'id' in t else o.get('stack') == t.get('stack'))), None)
        if tobj is None:
            continue
        q = tobj.get('controller')
        if q == p:
            C['daze_own_spell'] += 1
            continue
        them = s['players'][q]
        cls = 'pool' if pool(them) else ('land' if untapped_mana(them) else ('fetch' if untapped_fetch(them) else None))
        C['daze_cast'] += 1
        if cls:
            C['daze_payable'] += 1
            C['daze_pay_' + cls] += 1
            if cls in ('land', 'pool'):
                G.spots.append(('daze_payable_' + cls, k, 'Cast Daze'))
        _, j1 = G.stack_window(daze['stack'], jd)
        asked = paid = False
        for j in range(jd, j1 + 1):
            sj = G.seq[j]
            dj = dec_of(sj)
            stk = sj.get('stack') or []
            if dj.get('kind') == 'YesNo' and dj.get('player') == q and stk and stk[-1].get('stack') == daze['stack']:
                asked = True
                paid = paid or G.lab[j] == 'Yes'
        C['daze_asked'] += asked
        if cls:
            C['daze_paid'] += paid
            C['daze_paid_' + cls] += paid
            if cls == 'pool':
                ex.append(('daze into floating mana', k, s.get('turn'), f'P{p} Daze at {tobj.get("name")}, '
                                                                       f'P{q} pool {them.get("pool")}, paid {paid}'))


def scan_floats(G, C, players, ex):
    """Floats, and the two kinds of opportunities (Wasteland at an untapped mana land; Daze returning an
    untapped Island)."""
    fls = {p: floats(G, p) for p in (0, 1) if players is None or p in players}
    for p, fs in fls.items():
        for i, units, spent, cause, _ in fs:
            C['float'] += 1
            C['float_units'] += units
            C['float_spent'] += spent > 0
            C['float_pointless'] += spent == 0
            C['float_wasted_units'] += max(0, units - spent)
            C['float_' + cause] += 1
            if cause == 'other':
                ex.append(('float outside a Wasteland / Daze', i, G.seq[i].get('turn'), f'P{p} pool {units}'))
    # ---- Wasteland targeting an untapped mana land
    seen = set()
    for k in range(G.n):
        for o in G.seq[k].get('stack') or []:
            if o.get('name') != 'Wasteland' or not o.get('ability') or not o.get('targets') or o.get('stack') in seen:
                continue
            seen.add(o.get('stack'))
            tid = o['targets'][0].get('id')
            q, land = obj_on_bf(G.seq[k], tid)
            if land is None or q == o.get('controller') or land.get('name') not in MANA_LANDS or land.get('tapped'):
                continue
            if players is not None and q not in players:
                continue
            _, k1 = G.stack_window(o['stack'], k)
            C['waste_opp'] += 1
            fl = [f for f in fls.get(q, []) if k <= f[4] <= k1 and f[3] == 'wasteland']
            used = bool(fl) or any((obj_on_bf(G.seq[j], tid)[1] or {}).get('tapped') for j in range(k, k1 + 1))
            C['waste_used'] += used
            C['waste_float'] += bool(fl)
            C['waste_float_spent'] += any(f[2] > 0 for f in fl)
            # value taken from the doomed land: tapped to pay for something (no float of it), or a float of it spent
            if not ((used and not fl) or any(f[2] > 0 for f in fl)):
                colors = set(MANA_LANDS[land['name']])
                can = set()
                for j in range(k, k1 + 1):
                    dj = dec_of(G.seq[j])
                    if dj.get('kind') != 'Priority' or dj.get('player') != q:
                        continue
                    for opt in dj.get('options') or []:
                        t = opt.get('t') or ''
                        if t.startswith('Cast ') and t[5:] in INSTANTS and colors & set(INSTANTS[t[5:]]):
                            can.add(t[5:])
                if can:
                    C['waste_missed_spend'] += 1
                    C['waste_missed_spend_bs'] += 'Brainstorm' in can
                    if 'Brainstorm' in can:
                        j = next(j for j in range(k, k1 + 1) if dec_of(G.seq[j]).get('player') == q and any(
                            o.get('t') == 'Cast Brainstorm' for o in dec_of(G.seq[j]).get('options') or []))
                        G.spots.append(('waste_brainstorm', j, 'Cast Brainstorm'))
                    ex.append(('Wasteland: castable instant, land unused', k, G.seq[k].get('turn'),
                               f'P{q} {land["name"]} doomed, could cast {sorted(can)}'))
    # ---- Daze returning an untapped Island
    for k in range(G.n - 1):
        if G.lab[k] != 'Cast Daze':
            continue
        s = G.seq[k]
        p = dec_of(s).get('player')
        if players is not None and p not in players:
            continue
        me0 = s['players'][p]
        isl = [c for c in lands_of(me0) if c.get('name') in ISLANDS]
        back = jb = None
        for x in range(k + 1, min(G.n, k + 20)):
            if G.segment(x) != G.segment(k):
                break
            ids = {c.get('id') for c in G.seq[x]['players'][p].get('battlefield') or []}
            gone = [c for c in isl if c.get('id') not in ids]
            if gone:
                back, jb = gone[0], x
                break
        if back is None:
            continue                                  # hard cast (paid {1}{U}), or no Island left
        C['daze_bounce'] += 1
        fl = [f for f in fls.get(p, []) if k < f[4] <= jb and f[3] == 'daze']
        # a choice: a tapped and an untapped Island both there (returning the untapped one without a float
        # loses a mana; returning the tapped one does not)
        if any(c.get('tapped') for c in isl) and any(not c.get('tapped') for c in isl):
            C['daze_bounce_choice'] += 1
            C['daze_bounce_untapped_over_tapped'] += not back.get('tapped') and not fl
            # the float rule's version of the same loss: the untapped Island floated, returned, its mana unspent
            C['daze_bounce_floatwaste_over_tapped'] += not back.get('tapped') and bool(fl) and not any(f[2] > 0 for f in fl)
        if back.get('tapped'):
            continue
        C['daze_bounce_untapped'] += 1
        C['daze_bounce_float'] += bool(fl)
        C['daze_bounce_float_spent'] += any(f[2] > 0 for f in fl)
        if not fl:
            me1 = G.seq[jb]['players'][p]
            colors = set(MANA_LANDS.get(back['name'], ''))
            other = len(untapped_mana(me1))
            hand = [c.get('name') for c in me1.get('hand') or []]
            can = [x for x in hand if x in INSTANTS and colors & set(INSTANTS[x]) and INSTANT_COST[x] <= 1 + other]
            if can:
                C['daze_bounce_missed_spend'] += 1
    return fls


def scan_burn(G, C, players, ex):
    """Lightning Bolt / Unholy Heat leaving the stack aimed at an opponent's creature."""
    seen = set()
    for k in range(G.n):
        for o in G.seq[k].get('stack') or []:
            if o.get('name') not in BURN or not o.get('spell') or o.get('stack') in seen:
                continue
            caster = o.get('controller')
            if players is not None and caster not in players:
                seen.add(o.get('stack'))
                continue
            _, j1 = G.stack_window(o['stack'], k)
            if j1 + 1 >= G.n:
                continue                          # still on the stack at the end
            seen.add(o.get('stack'))
            pend = G.seq[j1]
            obj = next(x for x in pend.get('stack') or [] if x.get('stack') == o['stack'])
            text = ' | '.join(G.new_log(j1 + 1))
            tid = ((obj.get('targets') or [{}])[0]).get('id')
            if tid is None:
                m = TARGET_RE.search(text)
                tid = int(m.group(2)) if m else None
            ctl, c = obj_on_bf(pend, tid)
            if c is None or 'creature' not in c.get('types', []) or ctl == caster:
                continue
            if f'counters {o["name"]}' in text or 'damage to' not in text:
                C['burn_countered'] += 1
                continue
            m = DMG_RE.search(text)
            dealt = int(m.group(1)) if m else 0
            tough = (c.get('pt') or [0, 0])[1]
            dmg = c.get('damage', 0) or 0
            survived = obj_on_bf(G.seq[j1 + 1], tid)[1] is not None
            C['burn_creature'] += 1
            C['burn_survived'] += survived
            C['burn_hopeless'] += dealt < tough - dmg
            cast = next((x for x in range(k - 1, max(-1, k - 12), -1) if G.lab[x] == f'Cast {o["name"]}'
                         and dec_of(G.seq[x]).get('player') == caster), None)
            if cast is not None and survived and (dmg > 0 or dealt < tough):
                G.spots.append(('burn_damaged_survivor' if dmg > 0 else 'burn_could_not_kill', cast, G.lab[cast]))
            if dmg > 0:
                C['burn_damaged'] += 1
                C['burn_damaged_survived'] += survived
            if survived:
                C['burn_survived_' + ('heat' if o['name'] == 'Unholy Heat' else 'bolt')] += 1
                ex.append(('burn survivor' + (' (already damaged)' if dmg else ''), j1, pend.get('turn'),
                           f'P{caster} {o["name"]} for {dealt} at {c.get("name")} {c.get("pt")} damage {dmg}'))


def scan_land_drop(G, C, players, ex):
    """A non-cantrip spell cast with the land drop unused and an untapped-entering land in hand that
    tapped the caster out (no untapped mana land or fetch at the next priority decision), then a land
    from that hand played the same turn."""
    for k in range(G.n - 1):
        lab = G.lab[k]
        if not lab or not lab.startswith('Cast '):
            continue
        card = lab[5:]
        if card in CANTRIPS or card in F.FREE:
            continue
        s = G.seq[k]
        d = dec_of(s)
        p = d.get('player')
        if players is not None and p not in players:
            continue
        if d.get('kind') != 'Priority' or s.get('active') != p or s.get('step') not in ('main1', 'main2') or s.get('stack'):
            continue
        me = s['players'][p]
        if me.get('lands_played', 0) != 0:
            continue
        hand_lands = {c.get('name') for c in me.get('hand') or [] if c.get('name') in LANDS and c.get('name') not in ENTERS_TAPPED}
        if not hand_lands:
            continue
        C['ld_cast'] += 1
        j = G.next_priority(k)
        if j is None:
            continue
        mj = G.seq[j]['players'][p]
        if untapped_mana(mj) or untapped_fetch(mj):
            continue
        C['ld_tapout'] += 1
        played = None
        for x in range(k + 1, G.n):
            stop = False
            for ln in G.new_log(x):
                if 'Untap turn' in ln:
                    stop = True
                    break
                m = re.search(rf'P{p} plays (.*)$', ln)
                if m and m.group(1) in hand_lands:
                    played = m.group(1)
            if stop or played:
                break
        if not played:
            continue
        C['ld_blunder'] += 1
        G.spots.append(('land_drop_tapout', k, lab))
        op = 1 - p
        them = G.seq[j]['players'][op]
        live = any(c.get('name') == 'Daze' for c in them.get('hand') or []) and any(
            c.get('name') in ISLANDS for c in them.get('battlefield') or [])
        C['ld_blunder_live_daze'] += live
        dazed = False
        for x in range(k + 1, G.n):
            lines = G.new_log(x)
            if any(f'P{op} casts Daze' in ln for ln in lines):
                dazed = True
                break
            if any(f'{card} Stack ->' in ln for ln in lines):
                break
        C['ld_blunder_dazed'] += dazed
        ex.append(('tap-out with an unused land drop', k, s.get('turn'), f'P{p} cast {card}, then played {played}'))


def _turn_lines_back(G, k, upto, t):
    """Turn t's log lines from seq[k] back (newest step first) as (index, lines); seq[k]'s own lines cut
    at `upto` (None: all of them)."""
    for j in range(k, 0, -1):
        if G.seq[j - 1].get('turn') != t:
            return
        lines = G.new_log(j)
        yield j, (lines if j != k or upto is None else lines[:upto])


def scan_tamiyo(G, C, players, ex):
    """The AI's own draw spell or Clue flipping its Tamiyo, Inquisitive Student on its own turn while she
    could attack: before her attack (the Clue lost), after it, or after combat without one. The flip's
    cause is the top stack object (the flipper's) where her third-draw trigger went on the stack."""
    for k in range(1, G.n):
        lines = G.new_log(k)
        i = next((x for x, ln in enumerate(lines) if ln.endswith(TAMIYO_FLIP)), None)
        if i is None:
            continue
        prev, nxt = G.seq[k - 1], G.seq[k]
        t = prev.get('turn')
        for ps in prev['players']:
            p = ps['id']
            if prev.get('active') != p or (players is not None and p not in players):
                continue
            for c in ps.get('battlefield') or []:
                after = obj_on_bf(nxt, c['id'])[1]                    # the planeswalker may keep her id
                if c.get('name') != TAMIYO or (after is not None and after.get('name') == TAMIYO):
                    continue
                tj, tline = next(((j, [ln for ln in lj if ln.endswith(TAMIYO_TRIGGER)][-1])
                                  for j, lj in _turn_lines_back(G, k, i, t) if any(ln.endswith(TAMIYO_TRIGGER) for ln in lj)),
                                 (None, None))
                if tj is None:
                    continue
                top = (G.seq[tj - 1].get('stack') or [{}])[-1]
                cause = top.get('name') if top.get('controller') == p else None
                if cause not in TAMIYO_DRAWERS:
                    continue                                          # a draw step, a Bauble, an ultimate
                m = LOG_STEP.match(tline)
                step = m.group(1) if m else None
                cut = G.new_log(tj).index(tline)
                attacked = any('attacks with ' + TAMIYO in ln for _, lj in _turn_lines_back(G, tj, cut, t) for ln in lj)
                if c.get('sick') or (c.get('tapped') and not attacked):
                    continue
                opp = prev['players'][1 - p]
                if any('creature' in x.get('types', []) and not x.get('tapped') and {'flying', 'reach'} & set(x.get('kw') or [])
                       and (x.get('pt') or [0, 0])[0] >= 3 for x in opp.get('battlefield') or []):
                    continue                                          # attacking could lose her
                clean = not ({x.get('name') for x in ps.get('battlefield') or []} & COMBAT_UPSIDE)
                when = 'pre' if step in BEFORE_ATTACKS else ('after' if attacked else 'noattack')
                C['tam_flip'] += 1
                C['tam_flip_' + when] += 1
                if clean:
                    C['tam_flip_clean'] += 1
                    C['tam_flip_clean_' + when] += 1
                if when != 'pre':
                    continue
                want = 'Activate Clue' if cause == 'Clue' else 'Cast ' + cause
                cast = next((j for j in range(tj - 1, -1, -1) if G.seq[j].get('turn') == t and G.lab[j] == want
                             and dec_of(G.seq[j]).get('player') == p), None)
                if clean and cast is not None:
                    G.spots.append(('tamiyo_flip_precombat', cast, want))
                ex.append(('Tamiyo flipped before her attack' + ('' if clean else ' (DRC / Monk / Cutter out)'), tj, t,
                           f'P{p} {cause} in {step}'))


def scan_game(d, players=None, examples=None, spots=None):
    """All counts of one game `d` (zero-replay/1-like: states, frames, result) -> Counter. `players`: the
    counted players (None: both). `examples`: a list that gets (metric, step index, turn, text) rows;
    `spots`: a list that gets the canary candidates (kind, decision index, watched option)."""
    C = collections.Counter()
    ex = examples if examples is not None else []
    G = Game(d)
    C['games'] = 1
    C['pool_states'] = sum(1 for s in G.seq if any(pool(ps) for ps in s.get('players') or []))
    scan_daze(G, C, players, ex)
    scan_floats(G, C, players, ex)
    scan_burn(G, C, players, ex)
    scan_land_drop(G, C, players, ex)
    scan_tamiyo(G, C, players, ex)
    if spots is not None:
        spots.extend(G.spots)
    B = F.Band()
    F.scan_game(d, B, players)
    for key, v in B.c.items():
        C['fp_' + key] += v
    for (lands, pos), (keep, n, prior, vis) in B.mull.items():
        if lands == 1:
            C['mull1_n'] += n
            C['mull1_keep'] += keep
    return C


# ====================================================================================== the report
# (section, label, numerator, denominator); denominator None = per 100 games
METRICS = [
    ('Daze', 'Daze cast at an opponent spell (/100 g)', 'daze_cast', None),
    ('Daze', 'into payable mana (any)', 'daze_payable', 'daze_cast'),
    ('Daze', '  ... floating mana in the pool', 'daze_pay_pool', 'daze_cast'),
    ('Daze', '  ... an untapped mana land', 'daze_pay_land', 'daze_cast'),
    ('Daze', '  ... only an uncracked fetch', 'daze_pay_fetch', 'daze_cast'),
    ('Daze', 'payable: then paid', 'daze_paid', 'daze_payable'),
    ('Daze', '  floating: then paid', 'daze_paid_pool', 'daze_pay_pool'),
    ('Daze', '  land: then paid', 'daze_paid_land', 'daze_pay_land'),
    ('Daze', 'Daze at its own spell (/100 g)', 'daze_own_spell', None),
    ('float', 'floats (/100 g)', 'float', None),
    ('float', '  spent before the pool emptied', 'float_spent', 'float'),
    ('float', '  pointless (none of it spent)', 'float_pointless', 'float'),
    ('float', '  outside Wasteland / Daze (/100 g)', 'float_other', None),
    ('float', '  pool growth no option explains (/100 g)', 'float_unlabeled', None),
    ('float', 'Wasteland at an untapped mana land (/100 g)', 'waste_opp', None),
    ('float', '  land used before it died', 'waste_used', 'waste_opp'),
    ('float', '  floated', 'waste_float', 'waste_opp'),
    ('float', '  floated and spent', 'waste_float_spent', 'waste_opp'),
    ('float', '  no value taken, a castable instant could use it', 'waste_missed_spend', 'waste_opp'),
    ('float', 'Daze returning an untapped Island (/100 g)', 'daze_bounce_untapped', None),
    ('float', '  floated', 'daze_bounce_float', 'daze_bounce_untapped'),
    ('float', '  floated and spent', 'daze_bounce_float_spent', 'daze_bounce_untapped'),
    ('float', '  no float, an instant in hand could use it', 'daze_bounce_missed_spend', 'daze_bounce_untapped'),
    ('float', 'Daze returned untapped over a tapped Island', 'daze_bounce_untapped_over_tapped', 'daze_bounce_choice'),
    ('float', '  ... floated it, mana unspent', 'daze_bounce_floatwaste_over_tapped', 'daze_bounce_choice'),
    ('burn', 'burn at an opponent creature (/100 g)', 'burn_creature', None),
    ('burn', '  creature survived', 'burn_survived', 'burn_creature'),
    ('burn', '  could not kill (dealt < toughness left)', 'burn_hopeless', 'burn_creature'),
    ('burn', '  into a damaged creature that survived', 'burn_damaged_survived', 'burn_damaged'),
    ('burn', '  damaged survivor (/100 g)', 'burn_damaged_survived', None),
    ('land drop', 'tap-out, then the land played', 'ld_blunder', 'ld_cast'),
    ('land drop', '  (/100 g)', 'ld_blunder', None),
    ('land drop', '  ... into a live Daze', 'ld_blunder_live_daze', 'ld_blunder'),
    ('land drop', '  ... Dazed', 'ld_blunder_dazed', 'ld_blunder'),
    ('Tamiyo', 'own cantrip flipped her, she could attack (/100 g)', 'tam_flip', None),
    ('Tamiyo', '  before her attack (the Clue lost)', 'tam_flip_pre', 'tam_flip'),
    ('Tamiyo', '  after combat without an attack', 'tam_flip_noattack', 'tam_flip'),
    ('Tamiyo', '  clean board: before her attack', 'tam_flip_clean_pre', 'tam_flip_clean'),
    ('Tamiyo', '  clean board: before her attack (/100 g)', 'tam_flip_clean_pre', None),
    ('habits', 'fetch cracked at once', 'fp_fetch_now', 'fp_fetch_cracked'),
    ('habits', 'Brainstorm with a shuffle', 'fp_bs_shuffle', 'fp_bs'),
    ('habits', 'Murktide delve best', 'fp_murk_best', 'fp_murk'),
    ('habits', 'FoW pitched a FoW', 'fp_fow_pitch_fow', 'fp_fow_alt'),
    ('habits', 'tap-out into a live Daze', 'fp_daze_tapout', 'fp_daze_live'),
    ('habits', '  ... then Dazed', 'fp_daze_tapout_dazed', 'fp_daze_tapout'),
    ('habits', 'T1-2 Volcanic, blue only', 'fp_t12_ublue_volc', 'fp_t12_ublue'),
    ('habits', 'kept a 1-land seven', 'mull1_keep', 'mull1_n'),
    ('habits', 'starting player won', 'fp_starter_won', 'fp_decided'),
]


def metric_key(num, den):
    """A metric's stable key (the baseline's, the JSONL's): its numerator, '_per100g' for a per-game count."""
    return num if den else num + '_per100g'


def summarize(per_game):
    """{key: {section, label, value, se, n}} of a scan's per-game counts (rates as fractions)."""
    out = {}
    for sec, lab, num, den in METRICS:
        v, se, n = metric(per_game, num, den)
        out[metric_key(num, den)] = {'section': sec, 'label': lab.strip(), 'num': num, 'den': den, 'value': v, 'se': se, 'n': n}
    return out


def load_baseline(path=BASELINE_PATH):
    if not path or not os.path.exists(path):
        return None
    with open(path, encoding='utf-8') as f:
        b = json.load(f)
    if b.get('format') != BASELINE_FORMAT:
        raise SystemExit(f'{path}: not a {BASELINE_FORMAT} file')
    return b


def versus(value, se, base, n=None):
    """(gap, z, flag) of a measurement against a baseline entry: z = gap / the se of the difference (two
    independent sets of games), the measurement's se floored at what the baseline's rate would give with
    its n (a binomial se for a rate, the baseline's se scaled by its games for a /100 g count: a small set
    with every event alike has se 0); flagged when |z| > 2 and n >= 10."""
    if value is None or not base or base.get('value') is None:
        return None, None, False
    gap = value - base['value']
    se = se or 0.0
    if n:
        if base.get('den'):
            p0 = min(max(base['value'], 0.0), 1.0)
            se = max(se, math.sqrt(p0 * (1 - p0) / n))
        elif base.get('games'):
            se = max(se, (base.get('se') or 0.0) * math.sqrt(base['games'] / n))
    sd = math.hypot(se, base.get('se') or 0.0)
    if sd == 0:
        return gap, None, False
    z = gap / sd
    return gap, z, abs(z) > 2 and (n is None or n >= 10)


def make_baseline(scan_path, out=BASELINE_PATH, games_path=None, replace=False, digest=None, log=say):
    """Add the metrics a baseline file lacks (all of them for a new file) from a scan of the baseline
    checkpoint's fixed games; the entries already there stay (`replace`: overwrite them) - a new category
    joins with the baseline games re-scanned, the old ones keep their numbers."""
    S = load_scan(scan_path)
    meta = S.get('meta') or {}
    b = load_baseline(out) or {
        'format': BASELINE_FORMAT, 'what': 'Delver mirror blunder rates of the fixed baseline checkpoint (tools/delver_blunders.py)',
        'checkpoint': meta.get('label'), 'ckpt': meta.get('ckpt'), 'ckpt_sha256': meta.get('ckpt_sha256'),
        'games': {'n': len(S['games']), 'seed': meta.get('driver_seed'), 'sims': meta.get('sims'), 'chance_k': meta.get('chance_k'),
                  'seat': meta.get('seat_a'), 'opp': meta.get('opp'), 'file': games_path},
        'engine': {k: (meta.get('engine_play') or {}).get(k) for k in ('digest', 'engine_sha256', 'commit', 'branch')},
        'metrics': {}}
    if digest:
        b['engine']['digest'] = digest
    if b.get('ckpt_sha256') and meta.get('ckpt_sha256') and b['ckpt_sha256'] != meta['ckpt_sha256']:
        raise SystemExit(f'{scan_path} measures {meta.get("label")}, the baseline is {b.get("checkpoint")}')
    added = []
    today = time.strftime('%Y-%m-%d')
    for key, m in summarize([g['c'] for g in S['games']]).items():
        if key in b['metrics'] and not replace:
            continue
        b['metrics'][key] = dict(m, suite=SUITE_VERSION, measured=today, games=len(S['games']))
        added.append(key)
    b['suite'] = SUITE_VERSION
    os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
    with open(out, 'w', encoding='utf-8', newline='\n') as f:
        json.dump(b, f, indent=1)
        f.write('\n')
    log(f'{out}: {len(added)} metrics added ({len(b["metrics"])} in all)')
    return b


def ratio_se(xs, ns):
    """The ratio sum(x) / sum(n) of per-game counts and its cluster standard error (games independent)."""
    X, N, G = sum(xs), sum(ns), len(xs)
    if N == 0:
        return None, None, 0
    r = X / N
    if G < 2:
        return r, None, N
    v = sum((x - r * n) ** 2 for x, n in zip(xs, ns)) * G / (G - 1)
    return r, math.sqrt(v) / N, N


def per100_se(xs):
    G = len(xs)
    if G == 0:
        return None, None, 0
    m = sum(xs) / G
    if G < 2:
        return 100 * m, None, G
    v = sum((x - m) ** 2 for x in xs) / (G - 1)
    return 100 * m, 100 * math.sqrt(v / G), G


def metric(per_game, num, den):
    """-> (value, se, n): a rate (den given) or a count per 100 games (den None)."""
    xs = [g.get(num, 0) for g in per_game]
    if den is None:
        return per100_se(xs)
    return ratio_se(xs, [g.get(den, 0) for g in per_game])


def fmt(v, se, n, den):
    if v is None:
        return f'- ({n})'
    if den is None:
        return f'{v:.1f} +-{se:.1f}' if se is not None else f'{v:.1f}'
    return f'{100 * v:.1f} +-{100 * se:.1f} ({n})' if se is not None else f'{100 * v:.1f} ({n})'


def parse_diffs(spec, n):
    """`--diff` '2-1,4-3' -> [(1, 0), (3, 2)] (column j minus column i, 1-based); None: last - first when
    there are two columns."""
    if spec is None:
        return [(n - 1, 0)] if n == 2 else []
    out = []
    for part in filter(None, spec.split(',')):
        j, i = (int(x) for x in part.split('-'))
        if not (1 <= i <= n and 1 <= j <= n):
            raise SystemExit(f'--diff {part}: columns are 1..{n}')
        out.append((j - 1, i - 1))
    return out


def table(scans, diffs=None, baseline=None):
    """The report: one column a scan (a checkpoint at a budget), then the differences `diffs` [(j, i)]:
    column j minus column i, with the se of two independent sets of games. `baseline` (a baseline file's
    dict): its column first, and a '*' on every cell more than 2 se from it."""
    diffs = parse_diffs(None, len(scans)) if diffs is None else diffs
    if baseline:
        return _table_vs(scans, diffs, baseline)
    names = [s['label'] for s in scans]
    w0 = max(len(m[1]) for m in METRICS) + 2
    cols = names + [f'({j + 1}) - ({i + 1})' for j, i in diffs]
    ws = [max(22, len(c) + 2) for c in cols]
    out = [f'({k + 1}) {c}' for k, c in enumerate(names)] + ['']
    out.append('metric'.ljust(w0) + ''.join(f'({k + 1})'.rjust(w) for k, w in enumerate(ws[:len(names)]))
               + ''.join(c.rjust(w) for c, w in zip(cols[len(names):], ws[len(names):])))
    out.append('games'.ljust(w0) + ''.join(str(len(s['games'])).rjust(w) for s, w in zip(scans, ws)))
    sec = None
    for section, label, num, den in METRICS:
        if section != sec:
            sec = section
            out.append(f'-- {section}')
        vals = [metric([g['c'] for g in s['games']], num, den) for s in scans]
        row = label.ljust(w0) + ''.join(fmt(v, se, n, den).rjust(w) for (v, se, n), w in zip(vals, ws))
        for (j, i), w in zip(diffs, ws[len(names):]):
            (a, sa, _), (b, sb, _) = vals[i], vals[j]
            if a is not None and b is not None and sa is not None and sb is not None:
                k = 1 if den is None else 100
                row += f'{k * (b - a):+.1f} +-{k * math.hypot(sa, sb):.1f}'.rjust(w)
            else:
                row += '-'.rjust(w)
        out.append(row)
    fl = sum(g['c'].get('float', 0) for s in scans for g in s['games'])
    ps = sum(g['c'].get('pool_states', 0) for s in scans for g in s['games'])
    out.append('')
    out.append('rates in % +- 1 se (n = the denominator); /100 g = per 100 games +- 1 se; se: per-game clusters')
    out.append(f'mana in a pool at {ps} recorded states, {fl} floats' +
               (' - this engine does not float mana: the float rows are 0 by construction' if ps == 0 else ''))
    return '\n'.join(out)


def _table_vs(scans, diffs, base):
    bm = base['metrics']
    bcol = f'baseline {base.get("checkpoint")}'
    w0 = max(len(m[1]) for m in METRICS) + 2
    names = [s['label'] for s in scans]
    ws = [max(22, len(bcol) + 2)] + [max(24, len(c) + 2) for c in names]
    out = [f'(B) {bcol} ({base["games"]["n"]} games, {base["games"]["sims"]} sims, engine {base["engine"].get("digest")})']
    out += [f'({k + 1}) {c}' for k, c in enumerate(names)] + ['']
    out.append('metric'.ljust(w0) + '(B)'.rjust(ws[0]) + ''.join(f'({k + 1})'.rjust(w) for k, w in enumerate(ws[1:])))
    out.append('games'.ljust(w0) + str(base['games']['n']).rjust(ws[0]) + ''.join(str(len(s['games'])).rjust(w) for s, w in zip(scans, ws[1:])))
    sec, flagged = None, 0
    for section, label, num, den in METRICS:
        if section != sec:
            sec = section
            out.append(f'-- {section}')
        key = metric_key(num, den)
        b = bm.get(key)
        row = label.ljust(w0) + (fmt(b['value'], b['se'], b['n'], den) if b else 'new').rjust(ws[0])
        for s, w in zip(scans, ws[1:]):
            v, se, n = metric([g['c'] for g in s['games']], num, den)
            _, z, flag = versus(v, se, b, n)
            flagged += flag
            row += (fmt(v, se, n, den) + (' *' if flag else '  ')).rjust(w)
        out.append(row)
    out.append('')
    out.append(f'* = more than 2 se from the baseline (the se of the difference): {flagged} cells; rates in % +- 1 se (n)')
    return '\n'.join(out)


def as_json(scans):
    res = []
    for s in scans:
        pg = [g['c'] for g in s['games']]
        res.append({'label': s['label'], 'meta': s.get('meta'), 'games': len(pg),
                    'metrics': {f'{sec}: {lab.strip()}': dict(zip(('value', 'se', 'n'), metric(pg, num, den)),
                                                             num=num, den=den) for sec, lab, num, den in METRICS},
                    'totals': dict(sum((collections.Counter(c) for c in pg), collections.Counter()))})
    return res


# ====================================================================================== the games
def load_engine(path):
    """Import mtgz (and its pinned zero-core) from the checkout `path`; -> the engine module."""
    path = os.path.abspath(path)
    if not os.path.isfile(os.path.join(path, 'mtgz', '__init__.py')):
        raise SystemExit(f'--engine {path}: not an mtg-zero checkout (no mtgz/)')
    if path not in sys.path:
        sys.path.insert(0, path)
    import mtgz  # noqa: F401
    import mtgz.az  # noqa: F401
    real = os.path.normcase(os.path.realpath(mtgz.__file__))
    if not real.startswith(os.path.normcase(os.path.realpath(path))):
        raise SystemExit(f'mtgz was imported from {mtgz.__file__}, not --engine {path} (import order)')
    from zero.games import engine
    return engine()


def engine_info(path):
    path = os.path.abspath(path)

    def git(*a):
        try:
            return subprocess.run(['git', '-C', path, *a], capture_output=True, text=True, timeout=30).stdout.strip()
        except (OSError, subprocess.SubprocessError):
            return None
    pyd = os.path.join(path, 'mtgz', '_engine.pyd')
    if not os.path.exists(pyd):
        pyd = next((os.path.join(path, 'mtgz', f) for f in os.listdir(os.path.join(path, 'mtgz'))
                    if f.startswith('_engine') and f.endswith(('.so', '.pyd'))), None)
    h = None
    if pyd and os.path.exists(pyd):
        with open(pyd, 'rb') as f:
            h = hashlib.sha256(f.read()).hexdigest()[:16]
    commit = git('rev-parse', 'HEAD')
    digest = None
    gate = os.path.join(path, 'gates', f'{(commit or "")[:12]}.json')
    if commit and os.path.exists(gate):
        try:
            with open(gate, encoding='utf-8') as f:
                digest = (json.load(f).get('digests') or {}).get('full')
        except (OSError, ValueError):
            digest = None
    return {'checkout': path, 'commit': commit, 'branch': git('rev-parse', '--abbrev-ref', 'HEAD'),
            'dirty': bool(git('status', '--porcelain', '--untracked-files=no')), 'engine_sha256': h, 'digest': digest}


def sha256(path):
    with open(path, 'rb') as f:
        return hashlib.sha256(f.read()).hexdigest()


def label_for(ckpt):
    run = os.path.basename(os.path.dirname(os.path.dirname(os.path.abspath(ckpt))))
    m = re.search(r'iter_(\d+)\.pt$', ckpt)
    return f'{run}_{m.group(1)}' if m else os.path.splitext(os.path.basename(ckpt))[0]


RULE_KEYS = ('select', 'select_min_visits', 'cq_c_visit', 'cq_c_scale')


def eval_seat(ckpt, select=None, c_scale=None):
    """The checkpoint's run's evaluation seat (h2h.run_rule) with its move rule overridden: `select` 'visits'
    (the most visited option; the 'cq' keys dropped, as zero.h2h's --select visits) or 'cq' (the run's
    guard and transform, or min-visits 4 / c_visit 50 when the run has none), `c_scale` the 'cq' rule's
    scale -> (seat dict, label suffix: '' for the run's own rule)."""
    from zero import h2h
    seat = dict(h2h.run_rule(ckpt))
    if select is None and c_scale is None:
        return seat, ''
    if select == 'visits':
        if c_scale is not None:
            raise SystemExit("--c-scale is the 'cq' rule's (not with --select visits)")
        for k in RULE_KEYS:
            seat.pop(k, None)
        return seat, '_visits'
    if seat.get('select') != 'cq':
        seat.update(select='cq', select_min_visits=4, cq_c_visit=50.0, cq_c_scale=1.0)
    if c_scale is not None:
        seat['cq_c_scale'] = float(c_scale)
    return seat, f'_cq{seat["cq_c_scale"]:g}'


def mirror(ckpt, seat, games, sims, chance_k, seed, threads, groups, device, spec, conc, graphs, sparse):
    """The checkpoint against itself: zero.h2h's match, but both seats on ONE evaluator (h2h gives each side
    its own, so a mirror's leaves went out in two half batches) - the same driver, seeds and game seeds
    (game k: `game_seed(seed, k)`), unpaired, with the move log -> h2h-like {summary keys, rows}."""
    from zero import games as G
    from zero.infer import NetEvaluator, drive
    from zero.net import load
    model, _ = load(ckpt, device, game='mtg')
    sa = G.nn_seat('a', int(sims), 1.0, chance_k=int(chance_k))
    sa.update(seat)
    drv = G.Driver('mtg', [[sa, sa] for _ in range(games)], spec=spec, concurrency=conc, seed=seed, threads=threads,
                   groups=groups, max_steps=5000, move_log=1_000_000, sparse=bool(sparse))
    ev = NetEvaluator(model, device, graphs=bool(graphs) or None)
    drive(drv, {'a': ev})
    rows = sorted(drv.take_results(), key=lambda r: r['gid'])
    w0 = sum(r['result'][0] for r in rows) / max(1, len(rows))
    return {'a': ckpt, 'b': ckpt, 'game': 'mtg', 'sims': int(sims), 'chance_k': int(chance_k), 'games': len(rows),
            'seat0_score': round(w0, 4), 'unfinished': sum(1 for r in rows if not r['over']), 'one_evaluator': True,
            'rows': rows}


def play(ckpt, out, games=1000, seed=DEFAULT_SEED, opp='self', sims=None, device='cuda', threads=12, groups=4,
         conc=512, sparse=True, graphs=None, engine_path=ROOT, log=say, select=None, c_scale=None):
    """`games` games of `ckpt` (its run's evaluation seat; against itself or the anchor `opp`), with move
    logs -> the path of <out>/<label>.games.json.gz."""
    from zero import h2h
    cfg = h2h.run_config(ckpt)
    if not cfg.get('spec'):
        raise SystemExit(f'{ckpt}: no run config.json with a spec next to its models/')
    spec = cfg.get('eval_spec') or cfg['spec']
    sims = int(sims or cfg.get('eval_sims', 32))
    chance_k = int(cfg.get('chance_k', 0))
    sa, rule = eval_seat(ckpt, select, c_scale)
    b = ckpt if opp == 'self' else opp
    sb = dict(sa) if opp == 'self' else dict(h2h.run_rule(opp))
    if graphs is None:
        graphs = device.startswith('cuda')
    t0 = time.time()
    if opp == 'self':
        r = mirror(ckpt, sa, games, sims, chance_k, seed, threads, groups, device, spec, min(conc, games), graphs, sparse)
    else:
        r = h2h.match(ckpt, b, games=games, sims=sims, chance_k=chance_k, seed=seed, threads=threads, groups=groups,
                      device=device, spec=spec, conc=min(conc, games), seat_a=sa, seat_b=sb, max_steps=5000,
                      move_log=1_000_000, rows=True, paired=False, graphs=graphs or None, sparse=sparse)
    wall = time.time() - t0
    rec = {'format': GAMES_FORMAT, 'label': label_for(ckpt) + f'_s{sims}{rule}' + ('' if opp == 'self' else '_vs_' + label_for(opp)),
           'ckpt': os.path.abspath(ckpt), 'ckpt_sha256': sha256(ckpt)[:16], 'opp': 'self' if opp == 'self' else os.path.abspath(opp),
           'spec': spec, 'driver_seed': seed, 'paired': False, 'sims': sims, 'chance_k': chance_k, 'seat_a': sa,
           'seat_b': sb, 'engine': engine_info(engine_path), 'device': device, 'created': int(time.time()),
           'wall_s': round(wall, 1), 'summary': {k: v for k, v in r.items() if k != 'rows'}, 'rows': r['rows']}
    os.makedirs(out, exist_ok=True)
    path = os.path.join(out, rec['label'] + '.games.json.gz')
    with gzip.open(path, 'wt', encoding='utf-8') as f:
        json.dump(rec, f, separators=(',', ':'))
    log(f'{rec["label"]}: {games} games in {wall:.0f} s ({wall / games:.2f} s a game) -> {path}')
    return path


M64 = (1 << 64) - 1


def split64(x):
    z = (x + 0x9E3779B97F4A7C15) & M64
    z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & M64
    z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & M64
    return z ^ (z >> 31)


def _rotl(x, k):
    return ((x << k) | (x >> (64 - k))) & M64


def game_seed(driver_seed, gid, paired=False):
    """The engine seed zero-core's Driver gives game `gid` (its rng: split64 of the driver seed and the
    game's stream - the pair's with `paired` -, the first draw of a xoshiro256++ seeded by splitmix)."""
    x = split64(int(driver_seed) ^ split64((gid // 2 if paired else gid) + 1))
    s = []
    for _ in range(4):
        x = (x + 0x9E3779B97F4A7C15) & M64
        s.append(split64(x))
    if not any(s):
        s[0] = 1
    return (_rotl((s[0] + s[3]) & M64, 23) + s[0]) & M64


def rebuild(E, spec, driver_seed, row, paired=False):
    """A game of the driver's move log rebuilt in the engine `E` -> a zero-replay/1-like dict (a state at
    every engine step; frames {s, p, k 'nn' | 'forced', a}). Raises when the log does not replay."""
    sp = json.loads(spec)
    sp['log'] = True
    g = E.Game('mtg', json.dumps(sp), 2, game_seed(driver_seed, int(row['gid']), paired))
    states, frames = [], []

    def step(a, kind):
        states.append(json.loads(g.export_state()))
        frames.append({'s': len(states) - 1, 'p': g.to_move, 'k': kind, 'a': int(a)})
        g.step(int(a))

    def auto():
        while g.to_move is not None and len(g.legal()) == 1:
            step(g.legal()[0], 'forced')
    for mi, (seat, a, legal) in enumerate(row['moves']):
        if not (len(legal) == 1 and g.to_move == seat and list(g.legal()) == list(legal)):
            auto()
        if g.to_move != seat or list(g.legal()) != list(legal):
            raise RuntimeError(f'game {row["gid"]}: the rebuilt game left its move log at move {mi} (another engine?)')
        step(a, 'nn')
    auto()
    if g.to_move is not None:
        raise RuntimeError(f'game {row["gid"]}: the move log ended before the game (move_log too small?)')
    states.append(json.loads(g.export_state()))
    return {'format': 'zero-replay/1', 'states': states, 'frames': frames, 'result': row['result'],
            'rounds': row['rounds'], 'gid': row['gid'], 'final': len(states) - 1}


_W = {}


def _init_worker(engine_path):
    import psutil
    try:
        psutil.Process().nice(psutil.BELOW_NORMAL_PRIORITY_CLASS if os.name == 'nt' else 10)
    except (psutil.Error, AttributeError):
        pass
    _W['E'] = load_engine(engine_path)


def _scan_row(args):
    spec, seed, row, players, paired = args
    d = rebuild(_W['E'], spec, seed, row, paired)
    ex, sp = [], []
    c = scan_game(d, players, ex, sp)
    return {'gid': row['gid'], 'result': row['result'], 'rounds': row['rounds'], 'c': dict(c),
            'ex': [list(e) for e in ex[:20]], 'spots': [list(x) for x in sp[:20]]}


def scan_file(path, engine_path=ROOT, jobs=4, log=say):
    """Rebuild and count every game of a .games.json.gz -> the path of its .scan.json."""
    with gzip.open(path, 'rt', encoding='utf-8') as f:
        rec = json.load(f)
    if rec.get('format') != GAMES_FORMAT:
        raise SystemExit(f'{path}: not a {GAMES_FORMAT} file')
    eng = engine_info(engine_path)
    if rec['engine'].get('engine_sha256') != eng.get('engine_sha256'):
        log(f'note: games played by the engine build {rec["engine"].get("engine_sha256")} (commit '
            f'{(rec["engine"].get("commit") or "?")[:12]}), rebuilt by {eng.get("engine_sha256")} ({(eng.get("commit") or "?")[:12]})')
    t0 = time.time()
    rows = rec['rows']
    args = [(rec['spec'], rec['driver_seed'], row, None if rec['opp'] == 'self' else {row['gid'] % 2}, rec.get('paired', False))
            for row in rows]
    if jobs > 1:
        from concurrent.futures import ProcessPoolExecutor
        with ProcessPoolExecutor(max_workers=jobs, initializer=_init_worker, initargs=(engine_path,)) as ex:
            games = list(ex.map(_scan_row, args, chunksize=8))
    else:
        _init_worker(engine_path)
        games = [_scan_row(a) for a in args]
    if not re.search(r'_s\d+', rec['label']):
        rec['label'] = f"{rec['label']}_s{rec['sims']}"
    meta = {k: rec.get(k) for k in ('label', 'ckpt', 'ckpt_sha256', 'opp', 'driver_seed', 'sims', 'chance_k', 'seat_a',
                                    'device', 'wall_s', 'created')}
    meta['engine_play'] = rec['engine']
    meta['engine_scan'] = eng
    meta['scan_s'] = round(time.time() - t0, 1)
    meta['score'] = (rec.get('summary') or {}).get('a_score')
    out = os.path.join(os.path.dirname(path), rec['label'] + '.scan.json')
    with open(out, 'w', encoding='utf-8') as f:
        json.dump({'format': SCAN_FORMAT, 'label': rec['label'], 'meta': meta, 'games': games}, f, separators=(',', ':'))
    log(f'{rec["label"]}: scanned {len(games)} games in {meta["scan_s"]} s -> {out}')
    return out


def load_scan(path):
    with open(path, encoding='utf-8') as f:
        s = json.load(f)
    if s.get('format') != SCAN_FORMAT:
        raise SystemExit(f'{path}: not a {SCAN_FORMAT} file')
    return s


def scan_replays(run, lo=0, hi=10 ** 9):
    """The counts over a run's recorded evaluation replays (runs/<run>/replays, zero-replay/1) -> a scan
    dict (games of generations lo..hi; frames, not every engine step: coarser than a rebuilt game)."""
    import glob
    d0 = F.run_dir(run)
    games = []
    for f in sorted(glob.glob(os.path.join(d0, 'replays', '*.json.gz'))):
        with gzip.open(f, 'rt', encoding='utf-8') as fh:
            d = json.load(fh)
        if d.get('format') != 'zero-replay/1' or d.get('selfplay'):
            continue
        g = F.gen_of(d)
        if g is None or g < lo or g > hi:
            continue
        ex = []
        c = scan_game(d, None, ex)
        games.append({'gid': os.path.basename(f), 'gen': g, 'result': d.get('result'), 'c': dict(c),
                      'ex': [list(e) for e in ex[:20]]})
    return {'format': SCAN_FORMAT, 'label': f'{os.path.basename(os.path.normpath(d0))} {lo}-{hi} replays',
            'meta': {'replays': d0, 'min_gen': lo, 'max_gen': hi}, 'games': games}


# ====================================================================================== canaries
SPOTS_FORMAT = 'delver-blunders-spots/1'
CANARY_KINDS = {
    'daze_payable_land': 'Daze while the spell\'s controller has an untapped mana land (watched: Cast Daze)',
    'daze_payable_pool': 'Daze while the spell\'s controller has mana floating (watched: Cast Daze)',
    'waste_brainstorm': 'Wasteland at an untapped land, Brainstorm castable from it (watched: Cast Brainstorm, the good line)',
    'burn_could_not_kill': 'burn cast at an undamaged creature it could not kill (watched: the cast)',
    'burn_damaged_survivor': 'burn cast into a damaged creature that survived (watched: the cast)',
    'land_drop_tapout': 'tap-out with the land drop unused, the land played after (watched: the cast)',
    'tamiyo_flip_precombat': 'own cantrip / Clue flipping an able Tamiyo before her attack, clean board (watched: the cast)',
    'play': 'a Delver Lab Play-tab spot (watched: the AI\'s move there)',
}


def build_spots(games_paths, per_kind=12, play=(), engine_path=ROOT, log=say):
    """A fixed canary set: the first `per_kind` candidates of each kind (CANARY_KINDS) in the games' scans
    (game order), and the Play-tab spots `play` ('<game.json>:<action index>'), each as a game seed +
    the actions up to the decision + the watched option -> a spots dict. The spots replay only on an
    engine whose rules agree up to them (build them from games of the engine that will search them)."""
    E = load_engine(engine_path)
    spots = []
    for path in games_paths:
        with gzip.open(path, 'rt', encoding='utf-8') as f:
            rec = json.load(f)
        label = rec['label'] if re.search(r'_s\d+', rec['label']) else f"{rec['label']}_s{rec['sims']}"
        scan = load_scan(os.path.join(os.path.dirname(path), label + '.scan.json'))
        rows = {r['gid']: r for r in rec['rows']}
        taken = collections.Counter()
        for g in sorted(scan['games'], key=lambda g: g['gid']):
            for kind, k, watch in g.get('spots') or []:
                if taken[kind] >= per_kind or any(x['source'] == label and x['gid'] == g['gid'] and x['k'] == k for x in spots):
                    continue
                d = rebuild(E, rec['spec'], rec['driver_seed'], rows[g['gid']], rec.get('paired', False))
                st = d['states'][k]
                spots.append({'kind': kind, 'source': label, 'gid': g['gid'], 'k': k, 'spec': rec['spec'],
                              'seed': game_seed(rec['driver_seed'], g['gid'], rec.get('paired', False)),
                              'actions': [f['a'] for f in d['frames'][:k]], 'watch': watch,
                              'decider': st['decision']['player'], 'turn': st['turn'], 'step': st['step']})
                taken[kind] += 1
    for item in play:
        path, idx = item.rsplit(':', 1)
        idx = int(idx)
        with open(path, encoding='utf-8') as f:
            rec = json.load(f)
        from zero import replay as core
        import mtgz.az.replays as R
        run, _ = core.run_of(rec['model']['path'])
        spec = R.spec_with_log(core.run_config(run)['spec'])
        g = E.Game('mtg', spec, 2, int(rec['seed']))
        for a in rec['actions'][:idx]:
            g.step(int(a))
        st = json.loads(g.export_state())
        a = rec['actions'][idx]
        watch = next(o['t'] for o in st['decision']['options'] if o['a'] == a)
        spots.append({'kind': 'play', 'source': rec.get('id', os.path.basename(os.path.dirname(path))), 'gid': None, 'k': idx,
                      'spec': spec, 'seed': int(rec['seed']), 'actions': [int(x) for x in rec['actions'][:idx]], 'watch': watch,
                      'decider': st['decision']['player'], 'turn': st['turn'], 'step': st['step']})
    log(f'{len(spots)} spots: {dict(collections.Counter(x["kind"] for x in spots))}')
    return {'format': SPOTS_FORMAT, 'engine': engine_info(engine_path), 'created': int(time.time()), 'spots': spots}


def spot_game(E, sp):
    sj = json.loads(sp['spec'])
    sj['log'] = True
    g = E.Game('mtg', json.dumps(sj), 2, int(sp['seed']))
    for a in sp['actions']:
        g.step(int(a))
    return g


def search_many(ev, games, seeds, sims, seat, chance_k=0):
    """The evaluation seat's search (the run's rule and transform, leaf view, root belief: `seat` =
    h2h.run_rule's dict) of every game's pending decision, in lockstep batches -> [chosen action]."""
    import numpy as np
    from zero.protocol import ROW_BELIEF
    trees = [g.search_tree(True, int(sd)) for g, sd in zip(games, seeds)]
    if seat.get('belief_tilt'):
        B = len(games)
        x = np.stack([np.asarray(g.encode(g.to_move), np.float32) for g in games])
        pred = ev.evaluate(x, np.full(B, 2, np.int32), np.zeros(B + 1, np.int32), np.zeros(0, np.int32),
                           np.full(B, ROW_BELIEF, np.uint8))[3]
        for t, g, pr in zip(trees, games, pred):
            t.set_belief(np.asarray(pr, np.float32), g, float(seat['belief_tilt']), float(seat.get('belief_clip', 20.0)),
                         float(seat.get('belief_mix', 0.0)))
    root_view = seat.get('leaf_value_view') == 'root'
    for _ in range(sims):
        rows, back = [], []
        for k, (t, g) in enumerate(zip(trees, games)):
            leaf = t.simulate(g, float(seat.get('c_puct', 1.0)), int(chance_k), 0.0, root_view=root_view)
            if leaf is None:
                continue
            _, x, acts, vx = leaf
            rows.append((np.asarray(x, np.float32), list(acts)))
            vi = None
            if vx is not None:
                rows.append((np.asarray(vx, np.float32), []))
                vi = len(rows) - 1
            back.append((k, len(rows) - (2 if vx is not None else 1), vi))
        if not rows:
            break
        res_p, res_v = [], []
        for a in range(0, len(rows), 1024):
            ch = rows[a:a + 1024]
            off = np.zeros(len(ch) + 1, np.int32)
            off[1:] = np.cumsum([len(r[1]) for r in ch])
            acts = np.asarray([x for r in ch for x in r[1]], np.int32)
            pri, val, mar = ev.evaluate(np.stack([r[0] for r in ch]), np.full(len(ch), 2, np.int32), off, acts)[:3]
            for i in range(len(ch)):
                res_p.append(pri[off[i]:off[i + 1]])
                res_v.append((val[i], mar[i]))
        for k, li, vi in back:
            v, m = res_v[vi if vi is not None else li]
            trees[k].expand([float(x) for x in res_p[li]], [float(x) for x in v], [float(x) for x in m], 0.0)
    rule = seat.get('select', 'visits')
    args = (rule, int(seat.get('select_min_visits', 0)), float(seat.get('cq_c_visit', 50.0)), float(seat.get('cq_c_scale', 1.0)))
    out = []
    for t in trees:
        acts = list(t.root_acts)
        if rule == 'visits':
            nv, p0 = list(t.root_visits), list(t.root_priors0)
            i = max(range(len(acts)), key=lambda j: (nv[j], p0[j]))
        else:
            i = t.root_select(*args)
        out.append(int(acts[i]))
    return out


def canary(spots_path, ckpts, sims_list=(32,), repeats=8, device='cuda', engine_path=ROOT, log=say, select=None,
           c_scale=None, rules=None):
    """Each checkpoint's evaluation seat searches every spot `repeats` times (seeds apart) at each budget ->
    {label: {sims: [per spot: {option label: searches}]}}. `rules`: [(select, c_scale), ...] move rules to
    search with in turn (None: [(select, c_scale)]; each rule's label suffix as `eval_seat`'s)."""
    E = load_engine(engine_path)
    from zero import h2h
    from zero.infer import NetEvaluator
    from zero.net import load
    with open(spots_path, encoding='utf-8') as f:
        S = json.load(f)
    spots = S['spots']
    base = [spot_game(E, sp) for sp in spots]
    for sp, g in zip(spots, base):
        st = json.loads(g.export_state())
        sp['_labels'] = {o['a']: o['t'] for o in (st.get('decision') or {}).get('options') or []}
        if sp['watch'] not in sp['_labels'].values() or g.to_move != sp['decider']:
            raise SystemExit(f'spot {sp["kind"]} {sp["source"]} {sp["gid"]}:{sp["k"]} does not replay on this engine')
    res = {}
    if device == 'cpu':
        import torch
        torch.set_num_threads(min(8, os.cpu_count() or 8))
    for ck, (sel, cs) in [(c, r) for c in ckpts for r in (rules or [(select, c_scale)])]:
        model, _ = load(ck, device, game='mtg')
        ev = NetEvaluator(model, device, graphs=True if device.startswith('cuda') else None)
        seat, rule = eval_seat(ck, sel, cs)
        chance_k = int(h2h.run_config(ck).get('chance_k', 0))
        lab = label_for(ck) + rule
        res[lab] = {}
        for sims in sims_list:
            t0 = time.time()
            games, seeds, owner = [], [], []
            for i, g in enumerate(base):
                for r in range(repeats):
                    games.append(g.clone())
                    seeds.append(7919 * (i + 1) + r)
                    owner.append(i)
            chosen = search_many(ev, games, seeds, int(sims), seat, chance_k)
            per = [collections.Counter() for _ in spots]
            for i, a in zip(owner, chosen):
                per[i][spots[i]['_labels'].get(a, str(a))] += 1
            res[lab][str(sims)] = [dict(c) for c in per]
            log(f'{lab} at {sims} sims: {len(games)} searches in {time.time() - t0:.0f} s')
    for sp in spots:
        sp.pop('_labels', None)
    return {'spots': spots, 'repeats': repeats, 'results': res}


def canary_table(C):
    """One row a checkpoint x rule x budget, one column a spot kind (the mean share over its spots of the
    searches choosing the watched option, +- se over spots) or a Play-tab spot (its share)."""
    spots, rep = C['spots'], C['repeats']
    rows_ = [(lab, sims) for lab, by in C['results'].items() for sims in by]
    cols = []
    short = {'daze_payable_land': 'daze_pay', 'daze_payable_pool': 'daze_float', 'waste_brainstorm': 'waste_bs', 'burn_could_not_kill': 'burn_cnk',
             'burn_damaged_survivor': 'burn_dmg', 'land_drop_tapout': 'ld_tapout', 'tamiyo_flip_precombat': 'tam_flip'}
    for kind in CANARY_KINDS:
        idx = [i for i, sp in enumerate(spots) if sp['kind'] == kind]
        if kind == 'play':
            cols += [(f'#{spots[i]["k"]} {spots[i]["source"][-6:]}', [i]) for i in idx]
        elif idx:
            cols.append((f'{short.get(kind, kind)} ({len(idx)})', idx))
    ws = [max(12, len(c) + 2) for c, _ in cols]
    w0 = max([len(f'{lab} s{sims}') for lab, sims in rows_] + [10]) + 2
    out = [f'share of {rep} searches a spot choosing the watched option (mean over the spots of a kind +- se over spots)']
    for kind, desc in CANARY_KINDS.items():
        out.append(f'  {short.get(kind, kind)}: {desc}')
    for i, sp in enumerate(spots):
        if sp['kind'] == 'play':
            out.append(f'  #{sp["k"]} {sp["source"]}: turn {sp.get("turn")} {sp.get("step")}, watched {sp["watch"]}')
    out.append('')
    out.append('checkpoint, rule, budget'.ljust(w0) + ''.join(c.rjust(w) for (c, _), w in zip(cols, ws)))
    for lab, sims in rows_:
        cells = []
        for _, ii in cols:
            sh = [C['results'][lab][sims][i].get(spots[i]['watch'], 0) / rep for i in ii]
            m = sum(sh) / len(sh)
            se = math.sqrt(sum((x - m) ** 2 for x in sh) / (len(sh) - 1) / len(sh)) if len(sh) > 1 else None
            cells.append(f'{100 * m:.0f}%' + (f' +-{100 * se:.0f}' if se is not None else ''))
        out.append(f'{lab} s{sims}'.ljust(w0) + ''.join(c.rjust(w) for c, w in zip(cells, ws)))
    return '\n'.join(out)


# ====================================================================================== the sidecar
def watch(run, engine_path=ROOT, games=1000, seed=DEFAULT_SEED, sims=None, every=None, loop=300, min_iter=None,
          backfill=False, once=False, baseline=BASELINE_PATH, device='cuda', conc=1000, threads=16, groups=4, jobs=4,
          settle=60, log=say):
    """Follow a run's evaluated checkpoints (models/iter_N.pt, N a multiple of its eval_every) and measure
    each one: <run>/blunders/iter_NNNN.json (every metric, value / se / n, against the baseline) and a
    line of <run>/blunders.jsonl (flat, one per measurement: the plot's rows). Newest first: when it falls
    behind, older checkpoints are skipped (`backfill`: measured when nothing newer waits). Resumable (a
    checkpoint with its iter_NNNN.json is done; the move logs stay in blunders/work/). min_iter None: the
    newest evaluated checkpoint at launch onward. Stops on <run>/blunders/STOP; one watcher a run
    (blunders/watch.pid)."""
    import glob
    import psutil
    d = F.run_dir(run)
    out = os.path.join(d, 'blunders')
    work = os.path.join(out, 'work')
    os.makedirs(work, exist_ok=True)
    pidf = os.path.join(out, 'watch.pid')
    if os.path.exists(pidf):
        try:
            other = int(open(pidf).read().strip() or 0)
        except ValueError:
            other = 0
        if other and other != os.getpid() and psutil.pid_exists(other):
            raise SystemExit(f'{d}: another watcher (pid {other}) follows this run ({pidf})')
    with open(pidf, 'w') as f:
        f.write(str(os.getpid()))
    base = load_baseline(baseline)
    load_engine(engine_path)
    eng = engine_info(engine_path)
    log(f'watching {d} (engine {eng.get("commit", "?")[:12]} digest {eng.get("digest")}, {games} games at '
        f'{sims or "eval_sims"} sims, baseline {base.get("checkpoint") if base else None})')
    failed = set()
    try:
        while True:
            if os.path.exists(os.path.join(out, 'STOP')):
                log('STOP file: exiting')
                return 0
            with open(os.path.join(d, 'config.json'), encoding='utf-8') as f:
                ev_every = int(every or json.load(f).get('eval_every', 8))
            ck = {}
            for f in glob.glob(os.path.join(d, 'models', 'iter_*.pt')):
                m = re.search(r'iter_(\d+)\.pt$', f)
                if m and int(m.group(1)) % ev_every == 0 and time.time() - os.path.getmtime(f) > settle:
                    ck[int(m.group(1))] = f
            if min_iter is None:
                min_iter = max(ck) if ck else 0
                log(f'measuring evaluated checkpoints from iteration {min_iter} on (every {ev_every})')
            done = {int(m.group(1)) for m in (re.search(r'iter_(\d+)\.json$', x) for x in os.listdir(out)) if m}
            pending = sorted(i for i in ck if i >= min_iter and i not in done and i not in failed)
            target = None
            if pending:
                newest = pending[-1]
                target = newest
                if len(pending) > 1 and not backfill:
                    log(f'behind: skipping {pending[:-1]} for {newest}')
                    min_iter = newest
            if target is not None:
                try:
                    measure_one(d, target, ck[target], out, work, engine_path, eng, base, games, seed, sims, device, conc,
                                threads, groups, jobs, log)
                except Exception as e:                      # noqa: BLE001  (log it, go on with the next one)
                    failed.add(target)
                    with open(os.path.join(out, f'iter_{target:04d}.error.txt'), 'w', encoding='utf-8') as f:
                        f.write(f'{type(e).__name__}: {e}\n')
                    log(f'iteration {target}: failed ({type(e).__name__}: {e})')
                continue
            if once:
                return 0
            time.sleep(loop)
    finally:
        try:
            if open(pidf).read().strip() == str(os.getpid()):
                os.remove(pidf)
        except OSError:
            pass


def measure_one(d, it, ckpt, out, work, engine_path, eng, base, games, seed, sims, device, conc, threads, groups, jobs, log):
    """One evaluated checkpoint: the games (kept in work/), the scan, <out>/iter_NNNN.json, a JSONL line."""
    t0 = time.time()
    gp = play(ckpt, work, games, seed, 'self', sims, device, threads, groups, conc, True, engine_path=engine_path, log=log)
    sp = scan_file(gp, engine_path, jobs, log=log)
    S = load_scan(sp)
    summ = summarize([g['c'] for g in S['games']])
    flags = []
    for key, m in summ.items():
        b = (base or {}).get('metrics', {}).get(key)
        gap, z, flag = versus(m['value'], m['se'], b, m['n'])
        m.update(base=None if not b else b.get('value'), base_se=None if not b else b.get('se'), gap=gap, z=z, flag=flag)
        if flag:
            flags.append(key)
    meta = S['meta']
    rec = {'iter': it, 'run': os.path.basename(os.path.normpath(d)), 'time': time.strftime('%Y-%m-%dT%H:%M:%S'),
           'suite': SUITE_VERSION, 'games': len(S['games']), 'sims': meta.get('sims'), 'seed': meta.get('driver_seed'),
           'seat': meta.get('seat_a'), 'engine': {k: eng.get(k) for k in ('commit', 'digest', 'engine_sha256')},
           'baseline': None if not base else {'checkpoint': base.get('checkpoint'), 'engine': base.get('engine'),
                                               'suite': base.get('suite')},
           'seconds': round(time.time() - t0, 1), 'flags': flags, 'metrics': summ}
    with open(os.path.join(out, f'iter_{it:04d}.json'), 'w', encoding='utf-8') as f:
        json.dump(rec, f, indent=1)
    line = {'iter': it, 'time': rec['time'], 'games': rec['games'], 'sims': rec['sims'], 'suite': SUITE_VERSION,
            'engine': eng.get('digest') or eng.get('engine_sha256'), 'flags': flags}
    for key, m in summ.items():
        line[key] = None if m['value'] is None else round(m['value'], 5)
        line[key + '_se'] = None if m['se'] is None else round(m['se'], 5)
    with open(os.path.join(os.path.dirname(out), 'blunders.jsonl'), 'a', encoding='utf-8') as f:
        f.write(json.dumps(line) + '\n')
    log(f'iteration {it}: {rec["games"]} games in {rec["seconds"]:.0f} s; {len(flags)} flagged vs the baseline: {flags}')
    return rec


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    sub = ap.add_subparsers(dest='cmd', required=True)

    def game_args(p):
        p.add_argument('ckpts', nargs='+')
        p.add_argument('--out', required=True)
        p.add_argument('--games', type=int, default=2000)
        p.add_argument('--seed', type=int, default=DEFAULT_SEED, help=f'the driver seed (default {DEFAULT_SEED})')
        p.add_argument('--opp', default='self', help="'self' (default) or a fixed anchor checkpoint")
        p.add_argument('--sims', type=int, default=None, help="default: the run's eval_sims")
        p.add_argument('--device', default='cuda')
        p.add_argument('--threads', type=int, default=16)
        p.add_argument('--groups', type=int, default=4)
        p.add_argument('--conc', type=int, default=1000)
        p.add_argument('--sparse', type=int, default=1)
        p.add_argument('--select', default=None, choices=['visits', 'cq'],
                       help="the move rule (default: the run's eval_select; the label gets _visits / _cq<scale>)")
        p.add_argument('--c-scale', type=float, default=None, help="the 'cq' rule's scale (default: the run's)")
    for name in ('run', 'play'):
        p = sub.add_parser(name)
        game_args(p)
        p.add_argument('--engine', default=ROOT, help='the mtg-zero checkout that plays and rebuilds (default: this one)')
        p.add_argument('--jobs', type=int, default=6)
        p.add_argument('--json', default=None)
        p.add_argument('--diff', default=None, help="difference columns, e.g. '2-1,4-3' (default: 2-1 for two columns)")
    p = sub.add_parser('scan')
    p.add_argument('files', nargs='+')
    p.add_argument('--engine', default=ROOT)
    p.add_argument('--jobs', type=int, default=6)
    p = sub.add_parser('report')
    p.add_argument('files', nargs='+')
    p.add_argument('--json', default=None)
    p.add_argument('--baseline', default=None, help=f'compare with a baseline file (e.g. {os.path.relpath(BASELINE_PATH, ROOT)})')
    p.add_argument('--diff', default=None, help="difference columns, e.g. '2-1,4-3' (default: 2-1 for two columns)")
    p = sub.add_parser('spots', help='freeze a canary set from games (+ their scans) and Play-tab spots')
    p.add_argument('games', nargs='*')
    p.add_argument('--out', required=True)
    p.add_argument('--per-kind', type=int, default=12)
    p.add_argument('--play', action='append', default=[], help='<Delver Lab game.json>:<action index>')
    p.add_argument('--engine', default=ROOT)
    p = sub.add_parser('canary', help="each checkpoint's evaluation search at the frozen spots")
    p.add_argument('spots')
    p.add_argument('ckpts', nargs='+')
    p.add_argument('--sims', default='32', help="budgets, e.g. '32,1024'")
    p.add_argument('--repeats', type=int, default=8)
    p.add_argument('--device', default='cuda')
    p.add_argument('--engine', default=ROOT)
    p.add_argument('--out', default=None)
    p.add_argument('--select', default=None, choices=['visits', 'cq'])
    p.add_argument('--c-scale', type=float, default=None)
    p.add_argument('--rules', default=None, help="several move rules in turn, e.g. 'run,visits,cq:0.1' (run = the run's own)")
    p = sub.add_parser('canary-report', help='print saved canary results (several files are merged)')
    p.add_argument('files', nargs='+')
    p = sub.add_parser('baseline', help='add the metrics a baseline file lacks, from a scan of the baseline checkpoint')
    p.add_argument('scan')
    p.add_argument('--out', default=BASELINE_PATH)
    p.add_argument('--games-file', default=None, help='the baseline games (recorded in the file)')
    p.add_argument('--replace', action='store_true', help='overwrite the entries already there')
    p.add_argument('--digest', default=None, help="the engine's behaviour digest (its gate report's 'full'), when the "
                                                   "checkout has no gates/<commit>.json")
    p = sub.add_parser('watch', help="a sidecar: measure every evaluated checkpoint of a run as it appears")
    p.add_argument('run')
    p.add_argument('--engine', default=ROOT, help="the run's engine checkout (its pinned worktree)")
    p.add_argument('--games', type=int, default=1000)
    p.add_argument('--seed', type=int, default=DEFAULT_SEED)
    p.add_argument('--sims', type=int, default=None)
    p.add_argument('--every', type=int, default=None, help="default: the run's eval_every")
    p.add_argument('--loop', type=int, default=300, help='seconds between looks')
    p.add_argument('--min-iter', type=int, default=None, help='default: the newest evaluated checkpoint at launch')
    p.add_argument('--backfill', action='store_true', help='measure skipped checkpoints when nothing newer waits')
    p.add_argument('--once', action='store_true', help='measure what is pending, then exit')
    p.add_argument('--baseline', default=BASELINE_PATH)
    p.add_argument('--device', default='cuda')
    p.add_argument('--conc', type=int, default=1000)
    p.add_argument('--threads', type=int, default=16)
    p.add_argument('--jobs', type=int, default=4)
    p = sub.add_parser('replays')
    p.add_argument('run')
    p.add_argument('--min-gen', type=int, default=0)
    p.add_argument('--max-gen', type=int, default=10 ** 9)
    p.add_argument('--json', default=None)
    a = ap.parse_args(argv)
    if a.cmd in ('run', 'play'):
        os.environ.setdefault('ZERO_TOKEN_GRAPHS', '1')
        import psutil
        try:
            psutil.Process().nice(psutil.BELOW_NORMAL_PRIORITY_CLASS if os.name == 'nt' else 10)
        except (psutil.Error, AttributeError):
            pass
        load_engine(a.engine)
        paths = [play(c, a.out, a.games, a.seed, a.opp, a.sims, a.device, a.threads, a.groups, a.conc, bool(a.sparse),
                      engine_path=a.engine, select=a.select, c_scale=a.c_scale) for c in a.ckpts]
        if a.cmd == 'play':
            return 0
        scans = [load_scan(scan_file(p, a.engine, a.jobs)) for p in paths]
    elif a.cmd == 'scan':
        for f in a.files:
            scan_file(f, a.engine, a.jobs)
        return 0
    elif a.cmd == 'canary-report':
        C = None
        for f in a.files:
            with open(f, encoding='utf-8') as fh:
                X = json.load(fh)
            if C is None:
                C = X
            else:
                C['results'].update(X['results'])
        print(canary_table(C))
        return 0
    elif a.cmd == 'baseline':
        make_baseline(a.scan, a.out, a.games_file, a.replace, a.digest)
        return 0
    elif a.cmd == 'watch':
        import psutil
        try:
            psutil.Process().nice(psutil.BELOW_NORMAL_PRIORITY_CLASS if os.name == 'nt' else 10)
        except (psutil.Error, AttributeError):
            pass
        os.environ.setdefault('ZERO_TOKEN_GRAPHS', '1')
        return watch(a.run, a.engine, a.games, a.seed, a.sims, a.every, a.loop, a.min_iter, a.backfill, a.once,
                     a.baseline, a.device, a.conc, a.threads, 4, a.jobs)
    elif a.cmd == 'spots':
        S = build_spots(a.games, a.per_kind, a.play, a.engine)
        with open(a.out, 'w', encoding='utf-8') as f:
            json.dump(S, f, separators=(',', ':'))
        return 0
    elif a.cmd == 'canary':
        import psutil
        try:
            psutil.Process().nice(psutil.BELOW_NORMAL_PRIORITY_CLASS if os.name == 'nt' else 10)
        except (psutil.Error, AttributeError):
            pass
        os.environ.setdefault('ZERO_TOKEN_GRAPHS', '1')
        rules = None
        if a.rules:
            rules = []
            for r in a.rules.split(','):
                name, _, sc = r.partition(':')
                rules.append((None if name == 'run' else name, float(sc) if sc else None))
        C = canary(a.spots, a.ckpts, [int(x) for x in a.sims.split(',')], a.repeats, a.device, a.engine,
                   select=a.select, c_scale=a.c_scale, rules=rules)
        print(canary_table(C))
        if a.out:
            with open(a.out, 'w', encoding='utf-8') as f:
                json.dump(C, f, separators=(',', ':'))
        return 0
    elif a.cmd == 'replays':
        scans = [scan_replays(a.run, a.min_gen, a.max_gen)]
    else:
        scans = [load_scan(f) for f in a.files]
    print(table(scans, parse_diffs(getattr(a, 'diff', None), len(scans)),
                load_baseline(a.baseline) if getattr(a, 'baseline', None) else None))
    if getattr(a, 'json', None):
        with open(a.json, 'w', encoding='utf-8') as f:
            json.dump(as_json(scans), f, indent=1)
    return 0


if __name__ == '__main__':
    sys.exit(main())
