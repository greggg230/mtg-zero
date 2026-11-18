"""Combo statistics of constructed best-of-one games between fixed decklists: who wins, how, and what the combo
decks did on the way (Doomsday piles, Show and Tell, Sneak Attack, Thassa's Oracle, counters on combo pieces).
Measuring only: no rule, encoding or search change. Read-only for runs (writes only under --out).

  python tools/combo_stats.py play CKPT --decks doomsday_8027957,izzet_delver_8016541 --sims 1024,64 \\
      --games 100 --out DIR [--label L] [--seed 1] [--batch 25] [--threads 4] [--device cpu]
  python tools/combo_stats.py selfplay CKPT --p 0.5 --decks ... (the same flags)   # with the forced find: NOT a measurement
  python tools/combo_stats.py scan DIR/L.b000.games.json.gz [...] [--jobs 4]   # rebuild + per-game stats
  python tools/combo_stats.py report DIR/*.stats.jsonl [--md OUT.md] [--json OUT.json]
  python tools/combo_stats.py rows CKPT --p 1 --decks ... --sims 16 --games 20 [--out ROWS.json]   # the wheel's rows

play. Player 0 plays the first deck's main, player 1 the second's (open decklists: data/decks/<name>.json, the
constructed best-of-one spec of `mtgz.az.decklists.mirror_spec` with a deck per seat, ENGINE.md 120). Both players
are the checkpoint, each with its run's evaluation seat as the standing yardsticks' best of one plays it
(`mtgz.az.yardsticks.bo1_seat`: the run's move rule and root belief minus the deck tilt; `zero.h2h.run_seat_keys`:
margin_w, redet, eval_cache; c_puct 1, chance_k 2) and its own simulations (`--sims S0,S1`: every decision of that
player, mana payments included, as zero.h2h's seats). Who plays first is the engine's die roll and the winner's
play / draw choice (a decision of the network), so the seats are not alternated. One evaluator serves both
players. Games are played in batches of --batch games (driver seed `--seed` + the batch index: the same seeds give
the same deals, so two runs of one pairing with other simulations are paired by deal), each batch written as
<out>/<label>.b<NNN>.games.json.gz (the driver's rows with their move logs, the spec, the seats, wall and CPU
seconds) - a batch already on disk is skipped (resume). Games dropped after a panic go to <out>/game_failures.jsonl
(zero.game_failure's records: `python -m mtgz.az.game_failure replay FILE --line N` reproduces one).

scan. Each game is rebuilt in the engine from its seed and move log with the engine's play-by-play on (the spectator
view at every step, as tools/delver_blunders.py's rebuild) and read into one JSON line (<games file minus
.games.json.gz>.stats.jsonl): the decks, the starting player, the result and the end's reason, the win method
(`oracle`: the winner's Thassa's Oracle trigger; `pact`: the loser's Pact of Negation trigger; `combat` / `damage`:
the loser at 0 life in / outside a combat damage step; `decking`; `turn_cap`; `unfinished`; `aborted`), the turn,
mulligans, decisions, and the tracked cards' story: every cast of a tracked card (TRACKED below; the roles are card
names, so any deck that plays the cards is read the same way) with its outcome (resolved / countered and by what),
each resolved Doomsday's pile (the library after it, top first, and the choices that built it), whether the Oracle
was within reach (in the pile or in the hand), the life left; each Show and Tell's cards each player put onto the
battlefield; Sneak Attack activations; fatties and Omniscience entering the battlefield and how (show / sneak /
cast); the winner's attackers in its last attack.

selfplay. `play` with the Doomsday warm-up's forced find in the spec (docs/ENGINE.md 180, mtgz.az.forced_find: a share
--p of Doomsday's resolutions must put Thassa's Oracle in the pile) - what the self-play of a `legacy-meta3-ddwheel` run
plays at that p, to watch the network's own piles while the wheel is on. Not a measurement: `play` (and every
evaluation) refuses the forced find; the exit gate reads `play`'s games.

The Oracle at each resolved Doomsday (`ff` in a stats line, `oracle` in the report): `voluntary` (the pile holds it and the
player chose it), `forced` (its last pick was forced: the engine's play-by-play line "P<p>'s forced find: <class> (drawn
<bool>)", only with the forced find), `in_hand` (none in the pile, one in the caster's hand), `not_findable` (none in the
library or graveyard when it resolved), `missed` (one could be found, none chosen). With the forced find the class is the
engine's; without it the state's (the same order). A drawn resolution whose Oracle would have been the network's own LAST
pick reads `forced` (the decision had one option): the warm-up's voluntary share is exact on the undrawn resolutions
(`undrawn`); `play`'s games have no draw. The exit gate (the user's): the Oracle in the network's own piles (`voluntary`)
in at least 50% of resolutions, plus Oracle wins against Delver.

rows. The training rows the forced find's self-play makes (docs/ENGINE.md 180.12): `--games` self-play games of the
two decks with the forced find at --p, both players one recording self-play seat of the checkpoint (`--sims`, every
search full: every decision of two or more options recorded), traced as the farm's worker traces them; each game's
record is replayed by the trainer's path (`replay_rows`) and rebuilt with the play-by-play on to count its samples by
class: a pile or order decision of a resolving Doomsday whose resolution the draw applied to (`drawn`: drawn, the pile
holding the Oracle, none in the caster's hand) or not (`undrawn`), any other decision (`other`), another seat's
value-only sample (`view`) - each as policy rows (a policy target) and value-only rows (none). The wheel's rule: the
drawn class has no policy row; every other decision has its policy.

report. Per label: games, player 0's score (+- binomial se), each deck's score on the play and on the draw, the win
methods per winning deck, the turn of the win, mulligans, decisions and seconds per game, unfinished games and
failures, and the combo tables. `--pair A:B` adds the paired difference of player 0's score between two labels
played on the same seeds (per-deal differences; their se).
"""
import argparse
import collections
import glob
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
if ROOT not in sys.path:
    sys.path.insert(0, ROOT)

FORMAT = 'combo-games/1'
ORACLE = "Thassa's Oracle"
DOOMSDAY = 'Doomsday'
SHOW = 'Show and Tell'
SNEAK = 'Sneak Attack'
OMNI = 'Omniscience'
PACT = 'Pact of Negation'
FATTIES = ('Emrakul, the Aeons Torn', 'Griselbrand', 'Atraxa, Grand Unifier')
# the cards whose casts are followed (outcome, counters); counterspells and discard are what answers them
TRACKED = (DOOMSDAY, ORACLE, SHOW, SNEAK, OMNI) + FATTIES
COMBAT_STEPS = ('combat_damage', 'first_strike_damage')
CHOICE_KINDS = ('ChooseGraveyardCard', 'SearchLibrary', 'PutOnTop')
# the Oracle at a resolved Doomsday (module docs; engine/src/forced_find.rs FfClass, in its order)
FF_CLASSES = ('voluntary', 'forced', 'in_hand', 'not_findable', 'missed')


def say(*a):
    print(*a, flush=True)


# ====================================================================================== play
def deck_spec(p0, p1, max_turns=60, mulligans=True):
    """The constructed best-of-one spec: player 0 the main of decklist `p0`, player 1 `p1`'s (JSON)."""
    from mtgz.az import decklists as D
    decks = {}
    for k, name in (('p0', p0), ('p1', p1)):
        main = D.names(D.load(name)['main'])
        if len(main) < 60:
            raise ValueError(f'decklist {name}: {len(main)} maindeck cards (constructed: at least 60)')
        decks[k] = main
    return json.dumps({'max_turns': max_turns, 'mulligans': mulligans, 'constructed': True, 'decks': decks},
                      separators=(',', ':'))


def sha256(path):
    with open(path, 'rb') as f:
        return hashlib.sha256(f.read()).hexdigest()


def head_commit():
    try:
        return subprocess.run(['git', '-C', ROOT, 'rev-parse', 'HEAD'], capture_output=True, text=True,
                              timeout=30).stdout.strip() or None
    except (OSError, subprocess.SubprocessError):
        return None


def seats_for(ckpt, sims, chance_k=2, c_puct=1.0):
    """Each player's seat: the yardsticks' best-of-one seat of the checkpoint's run with that player's simulations
    (one network key 'a': one evaluator serves both)."""
    from zero import games as G, h2h
    from mtgz.az.yardsticks import bo1_seat
    rule = bo1_seat(ckpt)
    keys = h2h.run_seat_keys(ckpt)
    out = []
    for s in sims:
        seat = G.nn_seat('a', int(s), c_puct, chance_k=chance_k, **keys)
        seat.update(rule)
        out.append(seat)
    return out


def play(ckpt, decks, sims, games, out, label=None, seed=1, batch=25, threads=4, groups=2, conc=None, device='cpu',
         max_steps=20000, sparse=True, max_turns=60, log=say, forced_find=None):
    """`forced_find` None: a measurement (the spec refuses the forced find); a share p: `selfplay` (module docs)."""
    from zero import games as G
    from zero.infer import NetEvaluator, drive
    from zero.net import load
    from zero.local_config import portable_path
    from mtgz.az import forced_find as FF
    label = label or f'{decks[0]}-vs-{decks[1]}_s{sims[0]}-{sims[1]}' + ('' if forced_find is None else f'_ff{forced_find:g}')
    os.makedirs(out, exist_ok=True)
    spec = deck_spec(decks[0], decks[1], max_turns=max_turns)
    if forced_find is None:
        FF.refuse(spec, 'combo_stats play')
    else:
        spec = FF.with_forced_find(spec, forced_find)
    seats = seats_for(ckpt, sims)
    model, _ = load(ckpt, device, game='mtg')
    ev = NetEvaluator(model, device)
    meta = {'format': FORMAT, 'label': label, 'ckpt': portable_path(os.path.abspath(ckpt)),
            'ckpt_sha256': sha256(ckpt)[:16], 'decks': list(decks), 'sims': [int(s) for s in sims], 'spec': spec,
            'seats': seats, 'paired': False, 'commit': head_commit(), 'device': device, 'threads': threads,
            'groups': groups}
    if forced_find is not None:
        meta['selfplay'] = True                 # (the forced find on: not a measurement, docs/ENGINE.md 180)
        meta['forced_find'] = json.loads(spec)['forced_find']
    nb = (games + batch - 1) // batch
    for b in range(nb):
        n = min(batch, games - b * batch)
        path = os.path.join(out, f'{label}.b{b:03d}.games.json.gz')
        if os.path.exists(path):
            continue
        dseed = int(seed) + b
        drv = G.Driver('mtg', [list(seats) for _ in range(n)], spec=spec, concurrency=min(conc or n, n), seed=dseed,
                       threads=threads, groups=groups, max_steps=max_steps, move_log=1_000_000, sparse=bool(sparse))
        t0, c0 = time.time(), time.process_time()
        drive(drv, {'a': ev})
        wall, cpu = time.time() - t0, time.process_time() - c0
        rows = sorted(drv.take_results(), key=lambda r: r['gid'])
        fails = drv.take_failures() if hasattr(drv, 'take_failures') else []
        if fails:
            write_failures(out, fails, label, b)
        rec = dict(meta, batch=b, driver_seed=dseed, games=n, wall_s=round(wall, 1), cpu_s=round(cpu, 1),
                   failures=len(fails), created=int(time.time()), rows=rows)
        tmp = path + f'.{os.getpid()}.tmp'
        with gzip.open(tmp, 'wt', encoding='utf-8') as f:
            json.dump(rec, f, separators=(',', ':'))
        os.replace(tmp, path)
        w0 = sum(r['result'][0] for r in rows) / max(1, len(rows))
        log(f'{label} batch {b + 1}/{nb}: {n} games in {wall:.0f} s ({wall / n:.1f} s a game, cpu {cpu / n:.1f} s), '
            f'p0 {w0:.3f}, unfinished {sum(1 for r in rows if not r["over"])}, failures {len(fails)} -> {path}')


def write_failures(out, fails, label, b):
    """Driver failures (panics) -> <out>/game_failures.jsonl as zero.game_failure records (each with its repro)."""
    from zero import game_failure as GF
    path = os.path.join(out, GF.FILE)
    fs = [json.loads(f) if isinstance(f, str) else f for f in fails]
    commit = head_commit()
    GF.append(path, lambda first: [GF.record(f, it=f'{label}.b{b:03d}', source='local', game='mtg',
                                              engine_commit=commit, build=None, path=path, line=first + i,
                                              pkg='mtgz')
                                    for i, f in enumerate(fs)])
    say(f'GAME FAILURES: {len(fs)} game(s) of {label} batch {b} panicked -> {path}')


# ====================================================================================== scan
def names_of(cards):
    return [c.get('name') for c in cards or []]


def library_top(st, p):
    """Player p's library as the spectator view knows it, top first (names; `known[p].library` [pos, n, name])."""
    k = (st.get('known') or [{}, {}])[p] or {}
    return [x[2] for x in sorted(k.get('library') or [], key=lambda x: x[0])]


def stack_entries(st):
    """The stack's objects by their stack id (`stack`; `id` is the source card's, shared by its triggers)."""
    return {e['stack']: e for e in st.get('stack') or []}


class Reader:
    """One game read step by step (the state before a move, the move's option text, the log lines it wrote)."""

    def __init__(self, decks):
        self.decks = decks
        # the Oracle at each resolving Doomsday (module docs): its copies in each seat's main; where it was as the
        # resolution began (stack id -> zone); the engine's forced-find line (stack id -> (class, drawn))
        self.oracle_main = [_main_count(d, ORACLE) for d in decks]
        self.where = {}
        self.ff = {}
        self.spells = []                  # tracked casts: {card, p, turn, outcome, by}
        self.live = {}                    # stack id -> index into spells (tracked spells on the stack; still there
        #                                   at the end of a game: outcome None)
        self.doomsday = []
        self.show = []
        self.sneak = []                   # activations {p, turn}
        self.entered = []                 # fatties / Omniscience onto the battlefield {card, p, turn, how}
        self.attacks = {}                 # turn -> (active, [names])
        self.choices = collections.defaultdict(list)   # stack id of a resolving Doomsday -> choice texts
        self.show_before = {}             # stack id of a Show and Tell -> battlefields before it resolved
        self.casts = collections.Counter()   # (p, card) -> casts of every card (all, untracked too)
        self.counters = collections.Counter()  # (p, counterspell, countered card) for tracked targets
        self.recent = collections.deque(maxlen=60)   # the last log lines (the end's evidence)

    def step(self, before, kind, text, lines, after):
        turn = before.get('turn')
        sb, sa = stack_entries(before), stack_entries(after)
        self.recent.extend(lines)
        for ln in lines:
            m = re.match(r'^T\d+ \w+ P(\d) casts (.+) \((\w+)', ln)
            if m:
                self.casts[(int(m.group(1)), m.group(2))] += 1
            m = re.match(r'^T\d+ \w+ attacks with (.+)$', ln)
            if m:
                self.attacks.setdefault(turn, (before.get('active'), []))[1].append(m.group(1))
            m = re.match(r'^T\d+ \w+ P(\d) activates Sneak Attack ', ln)
            if m:
                self.sneak.append({'p': int(m.group(1)), 'turn': turn})
        # a Doomsday resolving: its choices (the decisions while it is the top of the stack)
        top = (before.get('stack') or [None])[-1] if before.get('stack') else None
        if top and top.get('name') == DOOMSDAY and not top.get('ability'):
            # (where the Oracle is until the resolution's first choice: the state it resolves from; its forced-find line)
            if not self.choices.get(top['stack']):
                self.where[top['stack']] = self.oracle_where(before, top.get('controller'))
            for ln in lines:
                m = re.search(r"P(\d)'s forced find: (\w+) \(drawn (true|false)\)", ln)
                if m:
                    self.ff[top['stack']] = (m.group(2), m.group(3) == 'true')
        if top and top.get('name') == DOOMSDAY and not top.get('ability') and kind in CHOICE_KINDS:
            self.choices[top['stack']].append(text)
        # a Show and Tell about to resolve: the battlefields before (its first choice, or the state it leaves)
        for i, e in sb.items():
            if e.get('name') == SHOW and not e.get('ability') and kind == 'Priority':
                self.show_before[i] = [names_of(p.get('battlefield')) for p in before['players']]
        # new tracked spells on the stack
        for i, e in sa.items():
            if i not in sb and not e.get('ability') and e.get('name') in TRACKED:
                self.live[i] = len(self.spells)
                self.spells.append({'card': e['name'], 'p': e.get('controller'), 'turn': after.get('turn'),
                                    'outcome': None, 'by': None})
        gone = [i for i in sb if i not in sa]
        resolved_now = {}
        countered_names = [m.group(1) for m in (re.match(r'^T\d+ \w+ counters (.+)$', ln) for ln in lines) if m]
        for i in gone:
            e = sb[i]
            if not e.get('ability') and e.get('name') in countered_names:
                countered_names.remove(e['name'])
                outcome = 'countered'
            else:
                outcome = 'resolved'
            resolved_now[i] = (e, outcome)
        for i, (e, outcome) in resolved_now.items():
            if i in self.live:
                sp = self.spells[self.live.pop(i)]
                sp['outcome'] = outcome
                if outcome == 'countered':
                    by = [x.get('name') for j, (x, o) in resolved_now.items()
                          if j != i and o == 'resolved' and not x.get('ability') and x.get('controller') != sp['p']]
                    sp['by'] = by[0] if by else None
                    self.counters[(1 - sp['p'], sp['by'], sp['card'])] += 1
                if outcome == 'resolved' and sp['card'] == DOOMSDAY:
                    self.doomsday_resolved(sp, after, i)
                if outcome == 'resolved' and sp['card'] == SHOW:
                    self.show_resolved(sp, after, i)
        # fatties and Omniscience entering the battlefield
        for p in (0, 1):
            nb = collections.Counter(names_of(before['players'][p].get('battlefield')))
            na = collections.Counter(names_of(after['players'][p].get('battlefield')))
            for card in (OMNI,) + FATTIES:
                for _ in range(max(0, na[card] - nb[card])):
                    how = 'other'
                    for i, (e, o) in resolved_now.items():
                        if o != 'resolved':
                            continue
                        if e.get('name') == SHOW and not e.get('ability'):
                            how = 'show'
                        elif e.get('name') == SNEAK and e.get('ability'):
                            how = 'sneak'
                        elif e.get('name') == card and not e.get('ability'):
                            how = 'cast'
                    self.entered.append({'card': card, 'p': p, 'turn': after.get('turn'), 'how': how})

    def oracle_where(self, st, p):
        """Where player p's Oracle is in state `st`: 'hand', 'graveyard', 'library' (a copy of its main not seen in any
        other zone), else 'gone' (exiled, on the battlefield or the stack, or none in the main)."""
        if p is None:
            return None
        pl = st['players'][p]
        if ORACLE in names_of(pl.get('hand')):
            return 'hand'
        if ORACLE in names_of(pl.get('graveyard')):
            return 'graveyard'
        seen = sum(names_of(pl.get(z)).count(ORACLE) for z in ('exile', 'battlefield'))
        seen += sum(1 for e in st.get('stack') or [] if e.get('name') == ORACLE and not e.get('ability')
                    and e.get('controller') == p)
        return 'library' if self.oracle_main[p] > seen else 'gone'

    def doomsday_resolved(self, sp, after, sid):
        p = sp['p']
        pile = library_top(after, p)
        hand = names_of(after['players'][p].get('hand'))
        where = self.where.pop(sid, None)
        ff = self.ff.pop(sid, None)
        if ff:
            cls, drawn = ff
        else:
            # (no forced find: the state's class, in the engine's order - module docs)
            cls = ('voluntary' if ORACLE in pile else 'in_hand' if ORACLE in hand or where == 'hand'
                   else 'missed' if where in ('library', 'graveyard') else 'not_findable')
            drawn = None
        self.doomsday.append({'p': p, 'turn': sp['turn'], 'pile': pile, 'library': after['players'][p].get('library'),
                              'choices': self.choices.pop(sid, []), 'hand': hand,
                              'life': after['players'][p].get('life'),
                              'oracle_in_pile': ORACLE in pile, 'oracle_in_hand': ORACLE in hand,
                              'oracle_reachable': ORACLE in pile or ORACLE in hand,
                              'oracle_where': where, 'ff': cls, 'ff_drawn': drawn})

    def show_resolved(self, sp, after, sid):
        bef = self.show_before.pop(sid, None)
        put = []
        for p in (0, 1):
            na = collections.Counter(names_of(after['players'][p].get('battlefield')))
            nb = collections.Counter(bef[p]) if bef else collections.Counter()
            put.append(sorted((na - nb).elements()) if bef else None)
        self.show.append({'p': sp['p'], 'turn': sp['turn'], 'put': put})


_MAINS = {}


def _main_count(deck, card):
    """Copies of `card` in decklist `deck`'s main (data/decks names; 0 for an unknown name)."""
    if deck not in _MAINS:
        try:
            from mtgz.az import decklists as D
            _MAINS[deck] = D.names(D.load(deck)['main'])
        except (OSError, KeyError, ValueError):
            _MAINS[deck] = []
    return _MAINS[deck].count(card)


def method_of(final, prev, recent=()):
    """How the game ended (module docs); `recent` the last log lines (a draw from an empty library)."""
    reason = final.get('reason')
    w = final.get('winner')
    if not final.get('over'):
        return 'unfinished'
    if reason == 'Aborted':
        return 'aborted'
    if reason == 'TurnLimit':
        return 'turn_cap'
    if reason == 'Concede' or reason == 'Conceded':
        return 'concede'
    if w is None:
        return f'draw_{reason}'
    loser = 1 - w
    if reason == 'Effect':
        # the last of the two triggers in the log (a trigger put and resolved in one step never shows on a stack)
        for ln in reversed(list(recent)):
            if f'trigger of {ORACLE} goes on the stack' in ln:
                return 'oracle'
            if f'trigger of {PACT} goes on the stack' in ln:
                return 'pact'
        for e in reversed((prev or {}).get('stack') or []):
            if e.get('name') == ORACLE and e.get('controller') == w:
                return 'oracle'
            if e.get('name') == PACT and e.get('controller') == loser:
                return 'pact'
        return 'effect'
    lp = final['players'][loser]
    if lp.get('life', 1) <= 0:
        return 'combat' if final.get('step') in COMBAT_STEPS else 'damage'
    if lp.get('library', 1) == 0 or any(f'P{loser} draws from an empty library' in ln for ln in recent):
        return 'decking'
    return f'sba_other'


def scan_row(E, meta, row):
    """A game rebuilt from its seed and move log -> its stats line (module docs). Raises when it does not replay."""
    sp = json.loads(meta['spec'])
    sp['log'] = True
    g = E.Game('mtg', json.dumps(sp), 2, E.game_seed_of(int(meta['driver_seed']), int(row['gid']), False))
    R = Reader(meta['decks'])
    st = json.loads(g.export_state())
    prev = None
    steps = 0

    def step(a):
        nonlocal st, prev, steps
        d = st.get('decision') or {}
        text = next((o.get('t') for o in d.get('options') or [] if o.get('a') == a), None)
        g.step(int(a))
        nxt = json.loads(g.export_state())
        k = nxt['log_n'] - st['log_n']
        lines = nxt['log'][-k:] if k > 0 else []
        if k > len(nxt['log']):
            # (never since the engine exports every line a step writes, ENGINE.md 189; a build before it kept a
            # state's newest 40 and a resolving Doomsday's oldest exile lines were lost: kept as a check)
            lines = ['(log lines lost)'] + lines
        R.step(st, d.get('kind'), text, lines, nxt)
        prev, st = st, nxt
        steps += 1

    def auto():
        while g.to_move is not None and len(g.legal()) == 1:
            step(g.legal()[0])
    for mi, (seat, a, legal) in enumerate(row['moves']):
        if not (len(legal) == 1 and g.to_move == seat and list(g.legal()) == list(legal)):
            auto()
        if g.to_move != seat or list(g.legal()) != list(legal):
            raise RuntimeError(f'game {row["gid"]}: the rebuilt game left its move log at move {mi}')
        step(a)
    if row['over']:
        auto()
    if row['over'] and g.to_move is not None:
        raise RuntimeError(f'game {row["gid"]}: the move log ended before the game')
    final = st
    w = final.get('winner') if final.get('over') else None
    last_attack = None
    if w is not None:
        ts = [t for t, (act, _) in R.attacks.items() if act == w]
        if ts:
            last_attack = {'turn': max(ts), 'attackers': R.attacks[max(ts)][1]}
    dec = collections.Counter(s for s, _, _ in row['moves'])
    method = method_of(final, prev, R.recent)
    return {'label': meta['label'], 'batch': meta['batch'], 'gid': row['gid'], 'driver_seed': meta['driver_seed'],
            'decks': meta['decks'], 'sims': meta['sims'], 'over': bool(row['over']),
            'forced_find': meta.get('forced_find'),
            'aborted': bool(row.get('aborted')), 'result': row['result'], 'winner': w,
            'reason': final.get('reason'), 'method': method, 'turn': final.get('turn'),
            'starting': final.get('starting'), 'mulligans': [p.get('mulligans') for p in final['players']],
            'life': [p.get('life') for p in final['players']], 'decisions': row.get('decisions'),
            'moves': [dec[0], dec[1]], 'steps': steps,
            'spells': R.spells, 'doomsday': R.doomsday, 'show': R.show, 'sneak': R.sneak, 'entered': R.entered,
            'last_attack': last_attack,
            'end_lines': list(R.recent)[-12:] if method in ('effect', 'sba_other') or method.startswith('draw_') else None,
            'counters': [[p, by, card, n] for (p, by, card), n in sorted(R.counters.items(), key=str)],
            'casts': [[p, card, n] for (p, card), n in sorted(R.casts.items(), key=str)]}


def read_games(path):
    with gzip.open(path, 'rt', encoding='utf-8') as f:
        return json.load(f)


def _scan_file(path):
    import mtgz.az  # noqa: F401
    from mtgz import _engine as E
    rec = read_games(path)
    meta = {k: v for k, v in rec.items() if k != 'rows'}
    out, errors = [], []
    for row in rec['rows']:
        try:
            s = scan_row(E, meta, row)
        except Exception as e:      # a game that does not replay is reported, never dropped silently
            errors.append(f'{os.path.basename(path)} game {row.get("gid")}: {type(e).__name__}: {e}')
            continue
        s['wall_s_per_game'] = round(rec['wall_s'] / max(1, rec['games']), 2)
        s['cpu_s_per_game'] = round(rec['cpu_s'] / max(1, rec['games']), 2)
        s['batch_failures'] = rec.get('failures', 0)
        out.append(s)
    dst = path[:-len('.games.json.gz')] + '.stats.jsonl'
    with open(dst, 'w', encoding='utf-8', newline='\n') as f:
        for s in out:
            f.write(json.dumps(s, separators=(',', ':')) + '\n')
        for e in errors:
            f.write(json.dumps({'scan_error': e}) + '\n')
    return dst, len(out), errors


def scan(paths, jobs=1, log=say):
    if jobs > 1 and len(paths) > 1:
        from concurrent.futures import ProcessPoolExecutor
        with ProcessPoolExecutor(jobs) as ex:
            res = list(ex.map(_scan_file, paths))
    else:
        res = [_scan_file(p) for p in paths]
    bad = 0
    for dst, n, errors in res:
        log(f'{dst}: {n} games' + (f', {len(errors)} NOT REPLAYED' if errors else ''))
        for e in errors:
            log('  SCAN ERROR ' + e)
        bad += len(errors)
    if bad:
        log(f'{bad} game(s) did not replay (see SCAN ERROR lines)')
    return res


# ====================================================================================== report
def se(p, n):
    return math.sqrt(max(p * (1 - p), 0.0) / n) if n else float('nan')


def pct(x, n):
    return f'{x / n:.3f} +- {se(x / n, n):.3f} ({x}/{n})' if n else '-'


def read_stats(paths):
    rows, errors = [], []
    for p in paths:
        with open(p, encoding='utf-8') as f:
            for line in f:
                if not line.strip():
                    continue
                r = json.loads(line)
                (errors if 'scan_error' in r else rows).append(r)
    return rows, errors


def short(deck):
    return {'doomsday_8027957': 'DD', 'izzet_delver_8016541': 'Delver', 'sneak_show_8014220': 'SnS'}.get(deck, deck)


def summarize(rs):
    """One label's numbers (a dict; `table` renders it)."""
    n = len(rs)
    d0, d1 = rs[0]['decks']
    out = {'label': rs[0]['label'], 'decks': [d0, d1], 'sims': rs[0]['sims'], 'games': n,
           'forced_find': rs[0].get('forced_find')}
    s0 = sum(r['result'][0] for r in rs)
    out['p0_score'] = s0 / n
    out['p0_se'] = se(s0 / n, n)
    # each player's score on the play / draw
    pd = {}
    for p in (0, 1):
        for play in (True, False):
            g = [r for r in rs if (r['starting'] == p) == play]
            sc = sum(r['result'][p] for r in g)
            pd[f'p{p}_{"play" if play else "draw"}'] = (sc / len(g), se(sc / len(g), len(g)), len(g)) if g else None
    out['play_draw'] = pd
    meth = collections.Counter()
    for r in rs:
        meth[(r['winner'], r['method'])] += 1
    out['methods'] = {f'{"p" + str(w) if w is not None else "none"}:{m}': c for (w, m), c in sorted(meth.items(), key=str)}
    wins = [r for r in rs if r['winner'] is not None]
    out['win_turn'] = {f'p{p}': (sum(r['turn'] for r in wins if r['winner'] == p) / max(1, sum(1 for r in wins if r['winner'] == p)))
                       for p in (0, 1)}
    out['mulligans'] = [sum((r['mulligans'][p] or 0) for r in rs) / n for p in (0, 1)]
    out['mull_any'] = [sum(1 for r in rs if (r['mulligans'][p] or 0) > 0) / n for p in (0, 1)]
    out['decisions'] = sum(r['decisions'] or 0 for r in rs) / n
    out['moves'] = [sum(r['moves'][p] for r in rs) / n for p in (0, 1)]
    out['steps'] = sum(r['steps'] for r in rs) / n
    out['wall_s'] = sum(r['wall_s_per_game'] for r in rs) / n
    out['cpu_s'] = sum(r['cpu_s_per_game'] for r in rs) / n
    out['unfinished'] = sum(1 for r in rs if not r['over'])
    out['aborted'] = sum(1 for r in rs if r['aborted'])
    out['turn_cap'] = sum(1 for r in rs if r['method'] == 'turn_cap')
    out['failures'] = sum(dict(((r['driver_seed'], r['batch']), r['batch_failures']) for r in rs).values())
    # every cast (per game), the most cast cards of each player
    cc = [collections.Counter(), collections.Counter()]
    for r in rs:
        for p, card, k in r['casts']:
            cc[p][card] += k
    out['casts_per_game'] = [{c: round(k / n, 2) for c, k in cc[p].most_common(12)} for p in (0, 1)]
    # tracked spells
    sp = collections.Counter()
    by = collections.Counter()
    for r in rs:
        for s in r['spells']:
            sp[(s['p'], s['card'], s['outcome'])] += 1
            if s['outcome'] == 'countered':
                by[(s['p'], s['card'], s['by'])] += 1
    out['spells'] = {f'p{p}:{c}:{o}': k for (p, c, o), k in sorted(sp.items(), key=str)}
    out['countered_by'] = {f'p{p}:{c}<-{b}': k for (p, c, b), k in sorted(by.items(), key=str)}
    # Doomsday
    dd = [(r, x) for r in rs for x in r['doomsday']]
    if any(s['card'] == DOOMSDAY for r in rs for s in r['spells']):
        ddp = [p for p in (0, 1) if any(s['card'] == DOOMSDAY and s['p'] == p for r in rs for s in r['spells'])]
        info = {}
        for p in ddp:
            casts = [s for r in rs for s in r['spells'] if s['card'] == DOOMSDAY and s['p'] == p]
            games_cast = [r for r in rs if any(s['card'] == DOOMSDAY and s['p'] == p for s in r['spells'])]
            res = [(r, x) for r, x in dd if x['p'] == p]
            games_res = [r for r in rs if any(x['p'] == p for x in r['doomsday'])]
            oracle_win = [r for r in rs if r['winner'] == p and r['method'] == 'oracle']
            piles = collections.Counter(tuple(x['pile']) for _, x in res)
            cards = collections.Counter(c for _, x in res for c in set(x['pile']))
            after = collections.Counter()
            for r, x in res:
                won = r['winner'] == p
                after[('won' if won else 'lost' if r['winner'] == 1 - p else 'draw',
                       r['method'] if won else 'x')] += 1
            info[f'p{p}'] = {
                'casts': len(casts), 'games_cast': len(games_cast),
                'outcomes': dict(collections.Counter(s['outcome'] for s in casts)),
                'resolved': len(res), 'games_resolved': len(games_res),
                'oracle_reachable': sum(1 for _, x in res if x['oracle_reachable']),
                'oracle_in_pile': sum(1 for _, x in res if x['oracle_in_pile']),
                'oracle_in_hand': sum(1 for _, x in res if x['oracle_in_hand']),
                'pile_size': collections.Counter(len(x['pile']) for _, x in res),
                'life_after': sum((x['life'] or 0) for _, x in res) / max(1, len(res)),
                'score_when_resolved': sum(r['result'][p] for r in games_res) / max(1, len(games_res)),
                'score_when_not_resolved': sum(r['result'][p] for r in rs if r not in games_res) / max(1, n - len(games_res)),
                'after_resolve': {f'{a}:{b}': c for (a, b), c in after.items()},
                'oracle_wins': len(oracle_win),
                'oracle_cast_after': sum(1 for r, x in res if any(s['card'] == ORACLE and s['p'] == p and
                                                                     s['turn'] >= x['turn'] for s in r['spells'])),
                'oracle_win_after': sum(1 for r, x in res if r['winner'] == p and r['method'] == 'oracle'),
                'turns_to_end': sum(r['turn'] - x['turn'] for r, x in res) / max(1, len(res)),
                'cast_turn': sum(s['turn'] for s in casts) / max(1, len(casts)),
                'oracle_casts': sum(1 for r in rs for s in r['spells'] if s['card'] == ORACLE and s['p'] == p),
                'oracle_resolved': sum(1 for r in rs for s in r['spells'] if s['card'] == ORACLE and s['p'] == p
                                       and s['outcome'] == 'resolved'),
                'top_piles': [[list(k), c] for k, c in piles.most_common(8)],
                'pile_cards': dict(cards.most_common(25)),
                # the Oracle at each resolution (module docs): all, and the undrawn ones (the forced find's draw said no)
                'oracle': {k: sum(1 for _, x in res if x.get('ff') == k) for k in FF_CLASSES},
                'undrawn': {k: sum(1 for _, x in res if x.get('ff') == k and x.get('ff_drawn') is False) for k in FF_CLASSES},
                'drawn': sum(1 for _, x in res if x.get('ff_drawn')),
            }
        out['doomsday'] = info
    # Show and Tell / Sneak Attack
    if any(s['card'] in (SHOW, SNEAK) for r in rs for s in r['spells']):
        snp = [p for p in (0, 1) if any(s['card'] in (SHOW, SNEAK) and s['p'] == p for r in rs for s in r['spells'])]
        info = {}
        for p in snp:
            shows = [x for r in rs for x in r['show'] if x['p'] == p]
            put = [collections.Counter(), collections.Counter()]
            for x in shows:
                for q in (0, 1):
                    for c in (x['put'][q] if x['put'][q] is not None else ['?']):
                        put[q][c] += 1
                    if x['put'][q] == []:
                        put[q]['(nothing)'] += 1
            ent = collections.Counter((e['card'], e['how']) for r in rs for e in r['entered'] if e['p'] == p)
            won_combat = [r for r in rs if r['winner'] == p and r['method'] in ('combat', 'damage')]
            fat = collections.Counter()
            for r in won_combat:
                atk = (r['last_attack'] or {}).get('attackers') or []
                fs = sorted(set(a for a in atk if a in FATTIES))
                fat['+'.join(fs) if fs else ('other: ' + '+'.join(sorted(set(atk)))[:60] if atk else 'no attack')] += 1
            info[f'p{p}'] = {
                'show': dict(collections.Counter(s['outcome'] for r in rs for s in r['spells'] if s['card'] == SHOW and s['p'] == p)),
                'games_show_resolved': sum(1 for r in rs if any(x['p'] == p for x in r['show'])),
                'show_put_self': dict(put[p].most_common(10)), 'show_put_opp': dict(put[1 - p].most_common(10)),
                'sneak_cast': dict(collections.Counter(s['outcome'] for r in rs for s in r['spells'] if s['card'] == SNEAK and s['p'] == p)),
                'sneak_activations': sum(1 for r in rs for x in r['sneak'] if x['p'] == p),
                'games_sneak_activated': sum(1 for r in rs if any(x['p'] == p for x in r['sneak'])),
                'entered': {f'{c}:{h}': k for (c, h), k in sorted(ent.items())},
                'games_omniscience': sum(1 for r in rs if any(e['p'] == p and e['card'] == OMNI for e in r['entered'])),
                'games_fatty': sum(1 for r in rs if any(e['p'] == p and e['card'] in FATTIES for e in r['entered'])),
                'won_by_damage': len(won_combat), 'winning_attackers': dict(fat.most_common(8)),
            }
        out['sneak_show'] = info
    return out


def fmt_pd(x):
    return f'{x[0]:.3f} +- {x[1]:.3f} (n {x[2]})' if x else '-'


def table(s):
    """A label's summary as Markdown."""
    d0, d1 = (short(d) for d in s['decks'])
    L = [f'### {s["label"]}: {d0} (p0, {s["sims"][0]} sims) vs {d1} (p1, {s["sims"][1]} sims)', '',
         f'* games {s["games"]}; **p0 ({d0}) score {s["p0_score"]:.3f} +- {s["p0_se"]:.3f}**; '
         f'unfinished {s["unfinished"]}, aborted {s["aborted"]}, turn cap {s["turn_cap"]}, failures {s["failures"]}',
         f'* on the play / draw: p0 {fmt_pd(s["play_draw"]["p0_play"])} / {fmt_pd(s["play_draw"]["p0_draw"])}; '
         f'p1 {fmt_pd(s["play_draw"]["p1_play"])} / {fmt_pd(s["play_draw"]["p1_draw"])}',
         f'* win methods: ' + ', '.join(f'{k} {v}' for k, v in s['methods'].items()),
         f'* mean turn of the win (game turns, both players\' turns counted): p0 {s["win_turn"]["p0"]:.1f}, '
         f'p1 {s["win_turn"]["p1"]:.1f}',
         f'* mulligans per game p0 {s["mulligans"][0]:.2f} (any {s["mull_any"][0]:.2f}), p1 {s["mulligans"][1]:.2f} '
         f'(any {s["mull_any"][1]:.2f})',
         f'* per game: {s["decisions"]:.0f} decisions (the driver count), moves p0 {s["moves"][0]:.0f} / p1 {s["moves"][1]:.0f}, '
         f'{s["steps"]:.0f} engine steps; {s["wall_s"]:.1f} s wall a game in its batch, {s["cpu_s"]:.1f} CPU-s a game',
         f'* casts per game p0: ' + ', '.join(f'{k} {v}' for k, v in s['casts_per_game'][0].items()),
         f'* casts per game p1: ' + ', '.join(f'{k} {v}' for k, v in s['casts_per_game'][1].items()),
         f'* tracked casts: ' + ', '.join(f'{k} {v}' for k, v in s['spells'].items())]
    if s['countered_by']:
        L.append('* countered by: ' + ', '.join(f'{k} {v}' for k, v in s['countered_by'].items()))
    if s.get('forced_find'):
        L.append(f'* **SELF-PLAY with the forced find** {s["forced_find"]} (docs/ENGINE.md 180): not a measurement')
    for p, x in (s.get('doomsday') or {}).items():
        L += [f'* **Doomsday ({p})**: cast {x["casts"]} times in {x["games_cast"]} games ({x["games_cast"] / s["games"]:.2f} of games), '
              f'outcomes {x["outcomes"]}; resolved {x["resolved"]} in {x["games_resolved"]} games; '
              f'Oracle reachable after it {x["oracle_reachable"]}/{x["resolved"]} (in pile {x["oracle_in_pile"]}, in hand '
              f'{x["oracle_in_hand"]}); life after {x["life_after"]:.1f}; pile sizes {dict(x["pile_size"])}',
              f'  * score when it resolved {x["score_when_resolved"]:.3f} (n {x["games_resolved"]}), otherwise '
              f'{x["score_when_not_resolved"]:.3f}; after a resolution: {x["after_resolve"]}',
              f'  * Thassa\'s Oracle: cast {x["oracle_casts"]}, resolved {x["oracle_resolved"]}, **Oracle wins {x["oracle_wins"]}**; '
              f'after a resolved Doomsday: Oracle cast in {x["oracle_cast_after"]}, Oracle win in {x["oracle_win_after"]}; '
              f'mean game turn of the cast {x["cast_turn"]:.1f}, game turns from the resolution to the end {x["turns_to_end"]:.1f}',
              f'  * pile cards (games with the card in the pile): {x["pile_cards"]}']
        o, n_res = x.get('oracle') or {}, x['resolved']
        if n_res:
            vol = o.get('voluntary', 0)
            L.append(f'  * **the Oracle at the resolutions**: ' + ', '.join(f'{k} {o.get(k, 0)}' for k in FF_CLASSES)
                     + f'; **in its own piles {vol}/{n_res} = {vol / n_res:.3f}** (exit gate >= 0.5: '
                     f'{"met" if vol / n_res >= 0.5 else "not met"})')
            u = x.get('undrawn') or {}
            nu = sum(u.values())
            if x.get('drawn') or nu:
                L.append(f'  * forced find: drawn {x["drawn"]}, undrawn {nu} (' + ', '.join(f'{k} {u.get(k, 0)}' for k in FF_CLASSES)
                         + (f'; own piles among the undrawn {u.get("voluntary", 0) / nu:.3f}' if nu else '') + ')')
        for pile, c in x['top_piles'][:5]:
            L.append(f'  * pile x{c}: {" / ".join(pile)}')
    for p, x in (s.get('sneak_show') or {}).items():
        L += [f'* **Sneak and Show ({p})**: Show and Tell {x["show"]}, resolved in {x["games_show_resolved"]} games; '
              f'put in by the caster {x["show_put_self"]}, by the opponent {x["show_put_opp"]}',
              f'  * Sneak Attack cast {x["sneak_cast"]}, activations {x["sneak_activations"]} in {x["games_sneak_activated"]} games; '
              f'entered {x["entered"]}; games with Omniscience in play {x["games_omniscience"]}, with a fatty {x["games_fatty"]}',
              f'  * won by damage {x["won_by_damage"]}: winning attackers {x["winning_attackers"]}']
    return '\n'.join(L)


def paired(ra, rb):
    """Player 0's score difference B - A over the deals both played (same driver seed and gid)."""
    a = {(r['driver_seed'], r['gid']): r['result'][0] for r in ra}
    b = {(r['driver_seed'], r['gid']): r['result'][0] for r in rb}
    ks = sorted(set(a) & set(b))
    if len(ks) < 2:
        return None
    d = [b[k] - a[k] for k in ks]
    m = sum(d) / len(d)
    v = sum((x - m) ** 2 for x in d) / (len(d) - 1)
    return {'n': len(d), 'diff': m, 'se': math.sqrt(v / len(d)),
            'a': sum(a[k] for k in ks) / len(ks), 'b': sum(b[k] for k in ks) / len(ks)}


def report(paths, md=None, js=None, pairs=(), merge=(), log=say):
    rows, errors = read_stats(paths)
    alias = {}
    for m in merge:                     # NEW=A,B,...: labels read as one (the same pairing and budgets, other seeds)
        new, _, olds = m.partition('=')
        alias.update({o: new for o in olds.split(',')})
    by = collections.defaultdict(list)
    for r in rows:
        r['label'] = alias.get(r['label'], r['label'])
        by[r['label']].append(r)
    sums = [summarize(by[k]) for k in sorted(by)]
    text = '\n\n'.join(table(s) for s in sums)
    pr = []
    for spec in pairs:
        a, b = spec.split(':')
        x = paired(by.get(a, []), by.get(b, []))
        if x:
            pr.append(dict(x, a_label=a, b_label=b))
            text += (f'\n\n* paired {b} - {a}: p0 score {x["b"]:.3f} vs {x["a"]:.3f}, difference {x["diff"]:+.3f} +- '
                     f'{x["se"]:.3f} over {x["n"]} common deals')
    if errors:
        text += '\n\nSCAN ERRORS:\n' + '\n'.join(e['scan_error'] for e in errors)
    log(text)
    if md:
        with open(md, 'w', encoding='utf-8', newline='\n') as f:
            f.write(text + '\n')
    if js:
        with open(js, 'w', encoding='utf-8', newline='\n') as f:
            json.dump({'labels': sums, 'pairs': pr, 'scan_errors': [e['scan_error'] for e in errors]}, f, indent=1,
                      default=lambda o: dict(o) if isinstance(o, collections.Counter) else str(o))
    return sums, pr


# ====================================================================================== spots
def find_spots(paths, per_file=None):
    """The pile spots of played games: each resolving Doomsday's first choice that offers Thassa's Oracle (a 'Find'
    or graveyard choice while Doomsday is the top of the stack) -> [{source, gid, seed, spec, actions, decider,
    turn, options, played}] (`actions`: every engine step before the spot, forced ones included)."""
    import mtgz.az  # noqa: F401
    from mtgz import _engine as E
    out = []
    for path in paths:
        rec = read_games(path)
        n = 0
        for row in rec['rows']:
            seed = E.game_seed_of(int(rec['driver_seed']), int(row['gid']), False)
            g = E.Game('mtg', rec['spec'], 2, seed)
            acts, seen = [], set()

            def look(a):
                st = json.loads(g.export_state())
                d = st.get('decision') or {}
                top = (st.get('stack') or [None])[-1]
                opts = {o.get('a'): o.get('t') or '' for o in d.get('options') or []}
                if (top and top.get('name') == DOOMSDAY and top.get('spell') and d.get('kind') in CHOICE_KINDS[:2]
                        and top['stack'] not in seen and any(ORACLE in t for t in opts.values())):
                    seen.add(top['stack'])
                    out.append({'source': os.path.basename(path), 'gid': row['gid'], 'seed': seed, 'spec': rec['spec'],
                                'actions': list(acts), 'decider': g.to_move, 'turn': st.get('turn'),
                                'kind': d.get('kind'), 'options': sorted(set(opts.values())),
                                'played': opts.get(a)})

            def stepa(a):
                look(a)
                g.step(int(a))
                acts.append(int(a))
            for seat, a, legal in row['moves']:
                while g.to_move is not None and len(g.legal()) == 1 and not (g.to_move == seat and list(g.legal()) == list(legal)):
                    stepa(g.legal()[0])
                stepa(a)
            n += 1
            if per_file and n >= per_file:
                break
    return out


def resolving(st):
    """A Doomsday (a spell) is the top of the stack and its controller makes one of its choices."""
    top = (st.get('stack') or [None])[-1]
    d = st.get('decision') or {}
    return bool(top and top.get('name') == DOOMSDAY and top.get('spell') and d.get('kind') in CHOICE_KINDS)


def search_spots(ckpt, spots, sims_list, repeats=8, device='cpu', log=say):
    """Each spot searched `repeats` times (seeds apart) at each budget by the checkpoint's best-of-one seat
    (tools/delver_blunders.search_many: the run's move rule, leaf view and root belief, chance_k 2) ->
    {sims: [per spot {option: searches}]}."""
    import mtgz.az  # noqa: F401
    from mtgz import _engine as E
    sys.path.insert(0, os.path.join(ROOT, 'tools'))
    import delver_blunders as DB
    from zero.infer import NetEvaluator
    from zero.net import load
    from mtgz.az.yardsticks import bo1_seat
    model, _ = load(ckpt, device, game='mtg')
    ev = NetEvaluator(model, device)
    seat = bo1_seat(ckpt)
    base = []
    for sp in spots:
        g = E.Game('mtg', sp['spec'], 2, int(sp['seed']))
        for a in sp['actions']:
            g.step(int(a))
        st = json.loads(g.export_state())
        sp['_labels'] = {o['a']: o['t'] for o in (st.get('decision') or {}).get('options') or []}
        if g.to_move != sp['decider'] or sorted(set(sp['_labels'].values())) != sp['options']:
            raise RuntimeError(f'spot {sp["source"]} {sp["gid"]} does not replay')
        base.append(g)
    res = {}
    for sims in sims_list:
        t0 = time.time()
        games, seeds, owner = [], [], []
        for i, g in enumerate(base):
            for r in range(repeats):
                games.append(g.clone())
                seeds.append(7919 * (i + 1) + r)
                owner.append(i)
        chosen = DB.search_many(ev, games, seeds, int(sims), seat, 2)
        per = [collections.Counter() for _ in spots]
        for i, a in zip(owner, chosen):
            per[i][spots[i]['_labels'].get(a, str(a))] += 1
        # the whole resolution searched: every later choice of the resolving Doomsday at the same budget
        live = list(range(len(games)))
        first = True
        nsearch = len(games)
        while live:
            for k, a in zip(live, chosen):
                games[k].step(int(a))
            nxt = []
            for k in live:
                g = games[k]
                while g.to_move is not None and len(g.legal()) == 1:
                    st = json.loads(g.export_state())
                    if not resolving(st):
                        break
                    g.step(g.legal()[0])
                if g.to_move is not None and len(g.legal()) > 1 and resolving(json.loads(g.export_state())):
                    nxt.append(k)
            live = nxt
            if live:
                chosen = DB.search_many(ev, [games[k] for k in live], [seeds[k] + 104729 for k in live], int(sims), seat, 2)
                nsearch += len(live)
        piles = [collections.Counter() for _ in spots]
        for k, i in enumerate(owner):
            st = json.loads(games[k].export_state())
            pile = library_top(st, spots[i]['decider'])
            piles[i]['oracle' if ORACLE in pile else 'no oracle'] += 1
        res[str(sims)] = {'first': [dict(c) for c in per], 'pile': [dict(c) for c in piles]}
        hit = sum(sum(k for t, k in c.items() if ORACLE in t) for c in per)
        ph = sum(c['oracle'] for c in piles)
        log(f'{sims} sims: {nsearch} searches in {time.time() - t0:.0f} s; the Oracle chosen first in {hit} of '
            f'{len(games)}, in the finished pile in {ph} of {len(games)}')
    for sp in spots:
        sp.pop('_labels', None)
    return res


# ====================================================================================== rows
ROW_CLASSES = ('drawn', 'undrawn', 'other', 'view')


def selfplay_records(ckpt, decks, sims, games, p, seed=1, threads=4, groups=2, max_turns=60, device='cpu',
                     max_steps=20000, log=say):
    """Traced self-play of `games` games of decks[0] vs decks[1] with the forced find at share `p`, both players one
    RECORDING self-play seat (`sims` simulations, full searches: every decision of two or more options is recorded;
    the seat's other settings zero's defaults) of checkpoint `ckpt` ('uniform': uniform priors and values, for
    tests) -> (the spec, the games' records as the farm's worker writes them: zero_search::record::game_json)."""
    from zero import games as G
    from zero.infer import NetEvaluator, UniformEvaluator, drive
    from mtgz.az import forced_find as FF
    spec = FF.with_forced_find(deck_spec(decks[0], decks[1], max_turns=max_turns), p)
    if ckpt == 'uniform':
        ev = UniformEvaluator()
    else:
        from zero.net import load
        model, _ = load(ckpt, device, game='mtg')
        ev = NetEvaluator(model, device)
    seat = G.nn_seat('a', int(sims), selfplay=True)
    drv = G.Driver('mtg', [[seat, seat]], spec=spec, concurrency=min(games, 16), seed=int(seed), record=True,
                   threads=threads, groups=groups, rolling=True, max_steps=max_steps)
    drv.set_trace(True)
    recs = []
    while len(recs) < games:
        drv.resume(games - len(recs), 0)
        drive(drv, {'a': ev})
        _, texts, _ = drv.take_game_records()
        recs += [json.loads(t) for t in texts if t]
        log(f'rows: {len(recs)}/{games} self-play games recorded')
    return spec, recs


def policy_rows(E, spec, rec):
    """One self-play game record -> its samples counted by class and kind (module docs, docs/ENGINE.md 180.12):
    {class: {'policy': n, 'value_only': n}} over ROW_CLASSES - 'drawn' / 'undrawn': a decision of the seat to move
    while a Doomsday resolves (one of its pile or order choices: `resolving`) whose resolution the forced find's draw
    applied to (drawn, the pile holding the Oracle, none in the caster's hand with `unless_in_hand`: the engine's
    play-by-play line "P<p>'s forced find: <class> (drawn <bool>)" and the state) or not; 'other': any other decision
    of the seat to move; 'view': another seat's value-only sample. Read from the game rebuilt from its seed and moves
    with the play-by-play on - not from the hook that chose the rows. Raises when it does not replay."""
    sp = json.loads(spec)
    unless = (sp.get('forced_find') or {}).get('unless_in_hand', True)
    sp['log'] = True
    g = E.Game('mtg', json.dumps(sp), 2, int(rec['seed']))
    st = json.loads(g.export_state())
    at, to_move, res, lines = {}, [], {}, {}
    for k, a in enumerate(rec['steps']):
        if not isinstance(a, int):
            raise ValueError(f'game {rec["gid"]}: step {k} is not an action ({a!r}): constructed best-of-one only')
        to_move.append(g.to_move)
        top = (st.get('stack') or [None])[-1]
        dd = top if top and top.get('name') == DOOMSDAY and top.get('spell') else None
        if resolving(st):
            res.setdefault(dd['stack'], {'hand': ORACLE in names_of(st['players'][dd['controller']].get('hand'))})
            at[k] = dd['stack']
        g.step(int(a))
        nxt = json.loads(g.export_state())
        n = nxt['log_n'] - st['log_n']
        # (the line comes with the pile's last pick: the resolving Doomsday is the stack's top after the step while its
        # order is asked - the resolution's first decisions when the engine answered every pick, in a step that began
        # with another object on top or a priority pass - else the top before it)
        ntop = (nxt.get('stack') or [None])[-1]
        sid = (ntop['stack'] if ntop and ntop.get('name') == DOOMSDAY and ntop.get('spell')
               else dd['stack'] if dd is not None else None)
        if sid is not None and n > 0:
            for ln in nxt['log'][-n:]:
                m = re.search(r"P(\d)'s forced find: (\w+) \(drawn (true|false)\)", ln)
                if m:
                    lines[sid] = (m.group(2), m.group(3) == 'true')
        st = nxt
    to_move.append(g.to_move)
    for sid, r in res.items():
        if sid not in lines:
            raise ValueError(f'game {rec["gid"]}: Doomsday {sid} made choices but has no forced-find line')
        r['ff'], r['drawn'] = lines[sid]
    out = {c: {'policy': 0, 'value_only': 0} for c in ROW_CLASSES}
    s = rec['samples']
    for k, p, kind in zip(s['step'], s['player'], s['kind']):
        if to_move[k] != p:
            cls = 'view'
        elif k in at:
            r = res[at[k]]
            applied = r['drawn'] and r['ff'] in ('forced', 'voluntary') and not (unless and r['hand'])
            cls = 'drawn' if applied else 'undrawn'
        else:
            cls = 'other'
        out[cls]['value_only' if kind else 'policy'] += 1
    return out


def rows(ckpt, decks, sims, games, p, out=None, seed=1, threads=4, groups=2, device='cpu', max_turns=60, log=say):
    """`rows` (module docs): self-play records, each replayed by the trainer's path (`replay_rows`: the record
    accepted, its rows rebuilt - those without options counted) and counted by `policy_rows` -> the totals, with
    'ok' = no drawn resolution's decision holds a policy target, every other decision of the seat to move does,
    and the replayed rows without options are the records' value-only samples."""
    import mtgz.az  # noqa: F401
    from mtgz import _engine as E
    spec, recs = selfplay_records(ckpt, decks, sims, games, p, seed, threads, groups, max_turns, device, log=log)
    tot = {c: {'policy': 0, 'value_only': 0} for c in ROW_CLASSES}
    replayed = {'rows': 0, 'without_options': 0}
    for rec in recs:
        text = json.dumps(rec, separators=(',', ':'))
        smp, info = E.replay_rows('mtg', spec, text, False)
        replayed['rows'] += info['rows']
        if smp is not None:
            import numpy as np
            replayed['without_options'] += int(np.count_nonzero(np.diff(np.asarray(smp['opt_off'])) == 0))
        for c, v in policy_rows(E, spec, rec).items():
            for k in v:
                tot[c][k] += v[k]
    value_only = sum(v['value_only'] for v in tot.values())
    ok = (tot['drawn']['policy'] == 0 and tot['undrawn']['value_only'] == 0 and tot['other']['value_only'] == 0
          and replayed['without_options'] == value_only)
    res = {'decks': list(decks), 'p': p, 'sims': int(sims), 'games': len(recs), 'seed': seed, 'rows': tot,
           'replayed': replayed, 'ok': ok}
    log(f'rows ({decks[0]} vs {decks[1]}, forced find p {p:g}, {len(recs)} self-play games, {sims} simulations): '
        f'Doomsday pile/order decisions of drawn resolutions: policy rows {tot["drawn"]["policy"]}, value-only rows '
        f'{tot["drawn"]["value_only"]}; of undrawn resolutions: policy rows {tot["undrawn"]["policy"]}, value-only '
        f'{tot["undrawn"]["value_only"]}; other decisions: policy {tot["other"]["policy"]}, value-only '
        f'{tot["other"]["value_only"]}; other seats\' value-only samples {tot["view"]["value_only"]}; replayed by the '
        f'trainer\'s path: {replayed["rows"]} rows, {replayed["without_options"]} without options - '
        + ('OK' if ok else 'NOT OK'))
    if out:
        os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
        with open(out, 'w', encoding='utf-8', newline='\n') as f:
            json.dump(res, f, indent=1)
    return res


# ====================================================================================== main
def _play_args(p):
    p.add_argument('ckpt')
    p.add_argument('--decks', required=True, help='DECK0,DECK1 (data/decks names)')
    p.add_argument('--sims', default='64,64', help='S0,S1: each player\'s simulations')
    p.add_argument('--games', type=int, default=100)
    p.add_argument('--out', required=True)
    p.add_argument('--label', default=None)
    p.add_argument('--seed', type=int, default=1)
    p.add_argument('--batch', type=int, default=25)
    p.add_argument('--threads', type=int, default=4)
    p.add_argument('--groups', type=int, default=2)
    p.add_argument('--conc', type=int, default=None)
    p.add_argument('--device', default='cpu')
    p.add_argument('--max-steps', type=int, default=20000)
    p.add_argument('--max-turns', type=int, default=60)
    p.add_argument('--torch-threads', type=int, default=None)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest='cmd', required=True)
    p = sub.add_parser('play')
    sp = sub.add_parser('selfplay', help='play with the forced find (docs/ENGINE.md 180): not a measurement')
    sp.add_argument('--p', type=float, required=True, help='the forced find\'s share of Doomsday\'s resolutions')
    _play_args(sp)
    _play_args(p)
    s = sub.add_parser('scan')
    s.add_argument('files', nargs='+')
    s.add_argument('--jobs', type=int, default=1)
    r = sub.add_parser('report')
    r.add_argument('files', nargs='+')
    r.add_argument('--md', default=None)
    r.add_argument('--json', default=None)
    r.add_argument('--pair', action='append', default=[], help='LABEL_A:LABEL_B (same seeds): paired difference')
    r.add_argument('--merge', action='append', default=[], help='NEW=LABEL,LABEL,...: labels read as one')
    q = sub.add_parser('spots', help='the pile spots of played games searched at several budgets')
    q.add_argument('ckpt')
    q.add_argument('files', nargs='+')
    q.add_argument('--sims', default='64,1024')
    q.add_argument('--repeats', type=int, default=8)
    q.add_argument('--max-spots', type=int, default=None)
    q.add_argument('--device', default='cpu')
    q.add_argument('--torch-threads', type=int, default=None)
    q.add_argument('--out', required=True, help='JSON: the spots and the searches\' choices')
    w = sub.add_parser('rows', help='the forced find\'s self-play rows by class (docs/ENGINE.md 180.12)')
    w.add_argument('ckpt', help='a checkpoint, or "uniform" (uniform priors and values)')
    w.add_argument('--p', type=float, required=True, help='the forced find\'s share of Doomsday\'s resolutions')
    w.add_argument('--decks', required=True, help='DECK0,DECK1 (data/decks names)')
    w.add_argument('--sims', type=int, default=16)
    w.add_argument('--games', type=int, default=20)
    w.add_argument('--seed', type=int, default=1)
    w.add_argument('--threads', type=int, default=4)
    w.add_argument('--groups', type=int, default=2)
    w.add_argument('--device', default='cpu')
    w.add_argument('--max-turns', type=int, default=60)
    w.add_argument('--torch-threads', type=int, default=None)
    w.add_argument('--out', default=None, help='JSON: the counts')
    a = ap.parse_args(argv)
    import mtgz.az  # noqa: F401
    if a.cmd == 'spots':
        if a.torch_threads:
            import torch
            torch.set_num_threads(a.torch_threads)
        files = [f for x in a.files for f in (sorted(glob.glob(x)) or [x])]
        spots = find_spots(files)
        if a.max_spots:
            spots = spots[:a.max_spots]
        say(f'{len(spots)} pile spots offering {ORACLE} in {len(files)} files')
        res = search_spots(a.ckpt, spots, [int(x) for x in a.sims.split(',')], a.repeats, a.device)
        with open(a.out, 'w', encoding='utf-8', newline='\n') as f:
            json.dump({'spots': [{k: v for k, v in sp.items() if k not in ('spec', 'actions')} for sp in spots],
                       'repeats': a.repeats, 'results': res}, f, indent=1)
        return
    if a.cmd == 'rows':
        if a.torch_threads:
            import torch
            torch.set_num_threads(a.torch_threads)
        decks = a.decks.split(',')
        if len(decks) != 2:
            ap.error('--decks takes two values')
        r = rows(a.ckpt, decks, a.sims, a.games, a.p, a.out, a.seed, a.threads, a.groups, a.device, a.max_turns)
        if not r['ok']:
            sys.exit(1)
        return
    if a.cmd in ('play', 'selfplay'):
        if a.torch_threads:
            import torch
            torch.set_num_threads(a.torch_threads)
        decks = a.decks.split(',')
        sims = [int(x) for x in a.sims.split(',')]
        if len(decks) != 2 or len(sims) != 2:
            ap.error('--decks and --sims take two values')
        play(a.ckpt, decks, sims, a.games, a.out, a.label, a.seed, a.batch, a.threads, a.groups, a.conc, a.device,
             a.max_steps, max_turns=a.max_turns, forced_find=a.p if a.cmd == 'selfplay' else None)
    elif a.cmd == 'scan':
        files = [f for x in a.files for f in (sorted(glob.glob(x)) or [x])]
        res = scan(files, a.jobs)
        if any(e for _, _, e in res):
            sys.exit(1)
    else:
        files = [f for x in a.files for f in (sorted(glob.glob(x)) or [x])]
        report(files, a.md, a.json, a.pair, a.merge)


if __name__ == '__main__':
    main()
