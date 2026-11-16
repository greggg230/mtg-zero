"""Combo statistics of constructed best-of-one games between fixed decklists: who wins, how, and what the combo
decks did on the way (Doomsday piles, Show and Tell, Sneak Attack, Thassa's Oracle, counters on combo pieces).
Measuring only: no rule, encoding or search change. Read-only for runs (writes only under --out).

  python tools/combo_stats.py play CKPT --decks doomsday_8027957,izzet_delver_8016541 --sims 1024,64 \\
      --games 100 --out DIR [--label L] [--seed 1] [--batch 25] [--threads 4] [--device cpu]
  python tools/combo_stats.py scan DIR/L.b000.games.json.gz [...] [--jobs 4]   # rebuild + per-game stats
  python tools/combo_stats.py report DIR/*.stats.jsonl [--md OUT.md] [--json OUT.json]

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
         max_steps=20000, sparse=True, max_turns=60, log=say):
    from zero import games as G
    from zero.infer import NetEvaluator, drive
    from zero.net import load
    from zero.local_config import portable_path
    label = label or f'{decks[0]}-vs-{decks[1]}_s{sims[0]}-{sims[1]}'
    os.makedirs(out, exist_ok=True)
    spec = deck_spec(decks[0], decks[1], max_turns=max_turns)
    seats = seats_for(ckpt, sims)
    model, _ = load(ckpt, device, game='mtg')
    ev = NetEvaluator(model, device)
    meta = {'format': FORMAT, 'label': label, 'ckpt': portable_path(os.path.abspath(ckpt)),
            'ckpt_sha256': sha256(ckpt)[:16], 'decks': list(decks), 'sims': [int(s) for s in sims], 'spec': spec,
            'seats': seats, 'paired': False, 'commit': head_commit(), 'device': device, 'threads': threads,
            'groups': groups}
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
    return {e['id']: e for e in st.get('stack') or []}


class Reader:
    """One game read step by step (the state before a move, the move's option text, the log lines it wrote)."""

    def __init__(self, decks):
        self.decks = decks
        self.spells = []                  # tracked casts: {card, p, turn, outcome, by}
        self.live = {}                    # stack id -> index into spells (tracked spells on the stack)
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
        if top and top.get('name') == DOOMSDAY and not top.get('ability') and kind in CHOICE_KINDS:
            self.choices[top['id']].append(text)
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

    def doomsday_resolved(self, sp, after, sid):
        p = sp['p']
        pile = library_top(after, p)
        hand = names_of(after['players'][p].get('hand'))
        self.doomsday.append({'p': p, 'turn': sp['turn'], 'pile': pile, 'library': after['players'][p].get('library'),
                              'choices': self.choices.pop(sid, []), 'hand': hand,
                              'life': after['players'][p].get('life'),
                              'oracle_in_pile': ORACLE in pile, 'oracle_in_hand': ORACLE in hand,
                              'oracle_reachable': ORACLE in pile or ORACLE in hand})

    def show_resolved(self, sp, after, sid):
        bef = self.show_before.pop(sid, None)
        put = []
        for p in (0, 1):
            na = collections.Counter(names_of(after['players'][p].get('battlefield')))
            nb = collections.Counter(bef[p]) if bef else collections.Counter()
            put.append(sorted((na - nb).elements()) if bef else None)
        self.show.append({'p': sp['p'], 'turn': sp['turn'], 'put': put})


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
    out = {'label': rs[0]['label'], 'decks': [d0, d1], 'sims': rs[0]['sims'], 'games': n}
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
                'oracle_casts': sum(1 for r in rs for s in r['spells'] if s['card'] == ORACLE and s['p'] == p),
                'oracle_resolved': sum(1 for r in rs for s in r['spells'] if s['card'] == ORACLE and s['p'] == p
                                       and s['outcome'] == 'resolved'),
                'top_piles': [[list(k), c] for k, c in piles.most_common(8)],
                'pile_cards': dict(cards.most_common(25)),
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
         f'* per game: {s["decisions"]:.0f} searched decisions, moves p0 {s["moves"][0]:.0f} / p1 {s["moves"][1]:.0f}, '
         f'{s["steps"]:.0f} engine steps; {s["wall_s"]:.1f} s wall a game in its batch, {s["cpu_s"]:.1f} CPU-s a game',
         f'* tracked casts: ' + ', '.join(f'{k} {v}' for k, v in s['spells'].items())]
    if s['countered_by']:
        L.append('* countered by: ' + ', '.join(f'{k} {v}' for k, v in s['countered_by'].items()))
    for p, x in (s.get('doomsday') or {}).items():
        L += [f'* **Doomsday ({p})**: cast {x["casts"]} times in {x["games_cast"]} games ({x["games_cast"] / s["games"]:.2f} of games), '
              f'outcomes {x["outcomes"]}; resolved {x["resolved"]} in {x["games_resolved"]} games; '
              f'Oracle reachable after it {x["oracle_reachable"]}/{x["resolved"]} (in pile {x["oracle_in_pile"]}, in hand '
              f'{x["oracle_in_hand"]}); life after {x["life_after"]:.1f}; pile sizes {dict(x["pile_size"])}',
              f'  * score when it resolved {x["score_when_resolved"]:.3f} (n {x["games_resolved"]}), otherwise '
              f'{x["score_when_not_resolved"]:.3f}; after a resolution: {x["after_resolve"]}',
              f'  * Thassa\'s Oracle: cast {x["oracle_casts"]}, resolved {x["oracle_resolved"]}, **Oracle wins {x["oracle_wins"]}**',
              f'  * pile cards (games with the card in the pile): {x["pile_cards"]}']
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


def report(paths, md=None, js=None, pairs=(), log=say):
    rows, errors = read_stats(paths)
    by = collections.defaultdict(list)
    for r in rows:
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


# ====================================================================================== main
def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest='cmd', required=True)
    p = sub.add_parser('play')
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
    s = sub.add_parser('scan')
    s.add_argument('files', nargs='+')
    s.add_argument('--jobs', type=int, default=1)
    r = sub.add_parser('report')
    r.add_argument('files', nargs='+')
    r.add_argument('--md', default=None)
    r.add_argument('--json', default=None)
    r.add_argument('--pair', action='append', default=[], help='LABEL_A:LABEL_B (same seeds): paired difference')
    a = ap.parse_args(argv)
    import mtgz.az  # noqa: F401
    if a.cmd == 'play':
        if a.torch_threads:
            import torch
            torch.set_num_threads(a.torch_threads)
        decks = a.decks.split(',')
        sims = [int(x) for x in a.sims.split(',')]
        if len(decks) != 2 or len(sims) != 2:
            ap.error('--decks and --sims take two values')
        play(a.ckpt, decks, sims, a.games, a.out, a.label, a.seed, a.batch, a.threads, a.groups, a.conc, a.device,
             a.max_steps, max_turns=a.max_turns)
    elif a.cmd == 'scan':
        files = [f for x in a.files for f in (sorted(glob.glob(x)) or [x])]
        res = scan(files, a.jobs)
        if any(e for _, _, e in res):
            sys.exit(1)
    else:
        files = [f for x in a.files for f in (sorted(glob.glob(x)) or [x])]
        report(files, a.md, a.json, a.pair)


if __name__ == '__main__':
    main()
