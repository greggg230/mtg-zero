"""17lands-style card statistics from a run's own self-play games (docs/ENGINE.md 107.4): how often the decks holding a card
win, by where the card was in the game.

Per card, over the games of a window and the seats counted (each seat of a game is one deck; a card counts once per seat
and game however many copies its deck holds; a draw is half a win), 17lands' metrics with their definitions
(https://www.17lands.com/metrics_definitions):

* **GP** (games played): the card is in the seat's main deck. **GP WR** = the seats' win rate.
* **OH** (opening hand): a copy is in the seat's final opening hand - the last hand it drew before the first turn, less
  that hand's bottomed cards (`games.kept_hand`).
* **GD** (games drawn): a copy was drawn after the opening hand (the draw step's draws and effects' draws).
* **GIH** (game in hand): OH or GD. **GNS** (game not seen): in the deck, never in hand (GP less GIH).
* **IWD** (improvement when drawn) = GIH WR - GNS WR.

Each win rate carries its count and a Wilson 95% interval; IWD's interval is Newcombe's (method 10: the two Wilson intervals
combined). OH, GD, GIH and GNS need the line's draws (game log version 2, from ENGINE.md 107.3); GP counts every game of the
window, version 1 included, so its count can be larger than GIH + GNS. In self-play both seats are the same network, so
the average seat wins exactly 50%; a card's rate is that of the decks holding it (deck quality, the builder's choices and
game length included), not of the card alone - 17lands' caveat too.

**Filters**: the iteration window (default the latest 10 finished iterations: the network improves), the phase
(self-play by default; evaluation games repeat the run's fixed evaluation decks against bots), the seats (the network's
by default: a seat whose search dealt worlds, `searched`; `all` adds the bots' seats of evaluation games), the seat's
builder mode (`best` / `explore` / ...) and its deck's colour pair.

**Draw credit** (optional, `--credit CKPT`, CPU time): `mtgz.sealed.cardcredit`'s value change across a card's draws, over
the window's version-2 games and the counted seats, with any checkpoint (`latest`: the run's newest).

**The 17lands column**: a yardstick only (DESIGN.md 0: human data never trains anything). From
`data/sealed/eoe_17lands_sealed.json.gz` (17lands' public EOE Premier Sealed games, best of one, CC BY 4.0), computed here:
their GIH WR when the dataset has the games' opening hands and draws (it has), else their GP WR; the record says which.

    python -m mtgz.sealed.cardstats RUN [--last 10 | --iters A-B] [--phase selfplay|eval|all] [--seats network|all]
        [--mode best] [--pair WG] [--sort gih] [--min-games 0] [--top 40] [--credit CKPT|latest [--credit-games N]
        [--threads 4]] [--no-17lands] [--json PATH]
"""
import argparse
import functools
import glob
import json
import math
import os
import re
import sys

from mtgz.sealed import cards as C
from mtgz.sealed import games as GL

Z95 = 1.959963984540054
# A seat's per-card counts: games and doubled wins (a draw = 1, a win = 2: integers) - in the deck (gp), in a game with the
# seat's draws logged (h: version 2), in the opening hand (oh), drawn after it (gd), either (gih). GNS = h - gih.
FIELDS = ('gp_n', 'gp_w2', 'h_n', 'h_w2', 'oh_n', 'oh_w2', 'gd_n', 'gd_w2', 'gih_n', 'gih_w2')
METRICS = ('gp', 'oh', 'gd', 'gih', 'gns')
DEFINITIONS = {
    'gp': 'Games played: the card is in the deck',
    'oh': "Opening hand: a copy is in the final opening hand (after the mulligans' bottomed cards)",
    'gd': 'Games drawn: a copy was drawn after the opening hand',
    'gih': 'Game in hand: in the opening hand or drawn',
    'gns': 'Game not seen: in the deck, never in hand',
    'iwd': 'Improvement when drawn: GIH WR - GNS WR',
}
COLOUR_ORDER = 'WUBRG'


# ---------------------------------------------------------------------------------------------------- intervals

def wilson(p, n, z=Z95):
    """The Wilson score interval of a rate p over n trials -> (lo, hi), (None, None) for n = 0."""
    if not n:
        return None, None
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(max(0.0, p * (1 - p) / n + z * z / (4 * n * n))) / d
    return max(0.0, c - h), min(1.0, c + h)


def newcombe(p1, n1, p2, n2, z=Z95):
    """Newcombe's interval (method 10) of the difference p1 - p2 of two independent rates -> (d, lo, hi), Nones when
    either side has no trial."""
    if not n1 or not n2:
        return None, None, None
    l1, u1 = wilson(p1, n1, z)
    l2, u2 = wilson(p2, n2, z)
    d = p1 - p2
    return d, d - math.sqrt((p1 - l1) ** 2 + (u2 - p2) ** 2), d + math.sqrt((u1 - p1) ** 2 + (p2 - l2) ** 2)


def rate(n, w2):
    """{n, wr, lo, hi} of n seats with w2 doubled wins, None for none."""
    if not n:
        return None
    p = w2 / (2 * n)
    lo, hi = wilson(p, n)
    return {'n': n, 'wr': round(p, 4), 'lo': round(lo, 4), 'hi': round(hi, 4)}


# ---------------------------------------------------------------------------------------------------- one seat

def seat_view(seat):
    """(kept opening hand, cards drawn after it) of a version-2 line's seat, as sets of names."""
    return set(GL.kept_hand(seat)), {GL.card_name(seat, d[2]) for d in seat['draws'] if d[1] != 0}


def norm_pair(colours):
    """A colour pair in the log's order (WUBRG): 'gw' -> 'WG'."""
    s = str(colours or '').upper()
    if not s or any(ch not in COLOUR_ORDER for ch in s):
        raise ValueError(f'not colours: {colours!r}')
    return ''.join(ch for ch in COLOUR_ORDER if ch in s)


def seat_filter(seats='network', mode=None, pair=None):
    """g -> the seats of game line g that count: the network's (`searched`) or all, of builder mode `mode`, whose deck's
    colours are `pair`."""
    if seats not in ('network', 'all'):
        raise ValueError(f"seats is 'network' or 'all', not {seats!r}")
    pair = norm_pair(pair) if pair else None

    def of(g):
        return [p for p, s in enumerate(g['seats'])
                if (seats == 'all' or g['searched'][p]) and (mode is None or s['mode'] == mode)
                and (pair is None or s['colors'] == pair)]
    return of


class Tally:
    """The counts (FIELDS) per card name over the seats added, and the games' totals."""

    def __init__(self):
        self.cards = {}
        self.games = self.seats = self.hand_games = self.hand_seats = self.aborted = 0
        self.best = [0, 0]                         # games best guess against explore build: [games, doubled wins of best]

    def add(self, g, seats):
        """Game line g, counting the seats `seats` (indices)."""
        if g['result'] is None:
            self.aborted += 1
            return
        v2 = g.get('v', 1) >= 2
        self.games += 1
        self.hand_games += v2
        modes = [s['mode'] for s in g['seats']]
        if sorted(modes) == ['best', 'explore']:
            self.best[0] += 1
            self.best[1] += round(2 * float(g['result'][modes.index('best')]))
        for p in seats:
            seat = g['seats'][p]
            w2 = round(2 * float(g['result'][p]))
            self.seats += 1
            oh = gd = ()
            if v2:
                self.hand_seats += 1
                oh, gd = seat_view(seat)
            for name in seat['deck']:
                c = self.cards.get(name)
                if c is None:
                    c = self.cards[name] = [0] * len(FIELDS)
                c[0] += 1
                c[1] += w2
                if v2:
                    c[2] += 1
                    c[3] += w2
                    a, b = name in oh, name in gd
                    if a:
                        c[4] += 1
                        c[5] += w2
                    if b:
                        c[6] += 1
                        c[7] += w2
                    if a or b:
                        c[8] += 1
                        c[9] += w2

    def merge(self, other):
        for name, c in other.cards.items():
            mine = self.cards.setdefault(name, [0] * len(FIELDS))
            for i, x in enumerate(c):
                mine[i] += x
        for k in ('games', 'seats', 'hand_games', 'hand_seats', 'aborted'):
            setattr(self, k, getattr(self, k) + getattr(other, k))
        self.best = [self.best[0] + other.best[0], self.best[1] + other.best[1]]
        return self


def metrics(c):
    """A card's counts (FIELDS) -> {gp, oh, gd, gih, gns: rate or None, iwd: {d, lo, hi} or None}."""
    gp_n, gp_w2, h_n, h_w2, oh_n, oh_w2, gd_n, gd_w2, gih_n, gih_w2 = c
    out = {'gp': rate(gp_n, gp_w2), 'oh': rate(oh_n, oh_w2), 'gd': rate(gd_n, gd_w2), 'gih': rate(gih_n, gih_w2),
           'gns': rate(h_n - gih_n, h_w2 - gih_w2)}
    d, lo, hi = newcombe(gih_w2 / (2 * gih_n) if gih_n else 0, gih_n,
                         (h_w2 - gih_w2) / (2 * (h_n - gih_n)) if h_n > gih_n else 0, h_n - gih_n)
    out['iwd'] = None if d is None else {'d': round(d, 4), 'lo': round(lo, 4), 'hi': round(hi, 4)}
    return out


# ---------------------------------------------------------------------------------------------------- the cards

@functools.lru_cache(None)
def card_info():
    """The set's cards for the tables, in collector order: [{name, colors, ci, rarity ('basic' for the basic lands), mv,
    type, sheet}] (data/sealed/eoe_set.json)."""
    sheet_order = {'eoe': 0, 'basic': 1, 'eos': 2, 'spg': 3}
    rows = sorted(C.eoe_set()['cards'], key=lambda c: (sheet_order.get(c.get('sheet'), 9),
                                                       int(re.sub(r'\D', '', str(c.get('collector_number'))) or 0)))
    return [{'name': c['name'], 'colors': c.get('colors') or '', 'ci': c.get('color_identity') or '',
             'rarity': 'basic' if c.get('sheet') == 'basic' else c.get('rarity'), 'mv': c.get('mana_value'),
             'type': c.get('type_line') or '', 'sheet': c.get('sheet')} for c in rows]


@functools.lru_cache(4)
def seventeen_lands(event='Sealed', path=None):
    """The 17lands yardstick per card (a comparison only, never a training signal: DESIGN.md 0): their GIH WR from the
    event's games when the dataset has the games' opening hands and draws, else their GP WR. -> {metric, key, event,
    games, win_rate, license, source, draft_time, rule, cards: {name: {n, wr}}}."""
    from mtgz.sealed import seventeen_lands as SL
    path = path or SL.DATASET
    doc = SL._doc(path)
    gp, gih, games, wins, draws_seen = {}, {}, 0, 0, False
    for pool in SL.load(event, path):
        for b in pool.builds:
            for gm in b.games:
                games += 1
                wins += gm.won
                seen = set(gm.opening_hand) | set(gm.drawn)
                draws_seen = draws_seen or bool(seen)
                for name in b.deck:
                    e = gp.setdefault(name, [0, 0])
                    e[0] += 1
                    e[1] += gm.won
                    if name in seen:
                        e = gih.setdefault(name, [0, 0])
                        e[0] += 1
                        e[1] += gm.won
    key = 'gih' if draws_seen else 'gp'
    src = doc['sources'][event]
    return {'metric': 'GIH WR' if draws_seen else 'GP WR', 'key': key, 'event': event,
            'event_name': {'Sealed': 'Premier Sealed (best of one)', 'TradSealed': 'Traditional Sealed (best of three)'}.get(event, event),
            'games': games, 'win_rate': round(wins / games, 4) if games else None, 'license': doc['license'],
            'source': src['url'], 'draft_time': src.get('draft_time'), 'rule': doc.get('rule'),
            'cards': {n: {'n': k, 'wr': round(w / k, 4)} for n, (k, w) in sorted((gih if draws_seen else gp).items())}}


# ---------------------------------------------------------------------------------------------------- a run's numbers

def checkpoint_of(run, which):
    """`which` (a .pt path, or 'latest': the run's newest models/iter_NNNN.pt) -> (path, iteration or None)."""
    if which != 'latest':
        m = re.search(r'iter_(\d+)\.pt$', str(which))
        return which, int(m.group(1)) if m else None
    best = None
    for f in glob.glob(os.path.join(run, 'models', 'iter_*.pt')):
        m = re.search(r'iter_(\d+)\.pt$', f)
        if m and (best is None or int(m.group(1)) > best[1]):
            best = (f, int(m.group(1)))
    if best is None:
        raise SystemExit(f'no checkpoint in {run}/models')
    return best


def window_of(times, last=10, iters=None):
    """The iteration window [lo, hi] (inclusive) of the run's finished iterations (`times`: games.iteration_times):
    `iters` (lo, hi) as given, else the latest `last` (0 = all); None when the run has no metrics (every game counts)."""
    if iters is not None:
        return [int(iters[0]), int(iters[1])]
    done = sorted({it for _, it in times})
    if not done:
        return None
    return [done[0] if not last else done[max(0, len(done) - int(last))], done[-1]]


def parse_iters(s):
    """'41-44' -> (41, 44), '44' -> (44, 44)."""
    a, _, b = str(s).partition('-')
    return int(a), int(b or a)


def stats(run, last=10, iters=None, phase='selfplay', seats='network', mode=None, pair=None, credit=None,
          credit_games=None, threads=4, yardstick=True, log=None):
    """The card statistics of a run's logged games (module docs) -> the summary dict."""
    if phase not in ('selfplay', 'eval', 'all'):
        raise ValueError(f"phase is 'selfplay', 'eval' or 'all', not {phase!r}")
    times = GL.iteration_times(run)
    window = window_of(times, last, iters)
    of = seat_filter(seats, mode, pair)
    specs, tally, keep, seen_iters, hand_iters = {}, Tally(), [], set(), set()
    n_lines = 0
    for g in GL.stream(run, specs):
        n_lines += 1
        if phase != 'all' and g['phase'] != phase:
            continue
        it = GL.game_iteration(g, times)
        if window is not None and (it is None or not window[0] <= it <= window[1]):
            continue
        ss = of(g)
        if not ss:
            continue
        tally.add(g, ss)
        seen_iters.add(it)
        if g.get('v', 1) >= 2 and g['result'] is not None:
            hand_iters.add(it)
            if credit:
                keep.append(g)
    if not n_lines:
        raise SystemExit(f'no game lines in {run} ({len(GL.files(run))} log files)')
    info = {c['name']: c for c in card_info()}
    rows = []
    for name, c in tally.cards.items():
        ci = info.get(name, {})
        rows.append({'name': name, 'colors': ci.get('colors', ''), 'rarity': ci.get('rarity'), 'mv': ci.get('mv'),
                     'type': ci.get('type', ''), **metrics(c)})
    out = {'run': os.path.basename(os.path.normpath(run)), 'phase': phase, 'seats': seats, 'mode': mode,
           'pair': norm_pair(pair) if pair else None, 'window': window,
           'iterations': sorted(x for x in seen_iters if x is not None),
           'hand_iterations': sorted(x for x in hand_iters if x is not None),
           'games': tally.games, 'seats_counted': tally.seats, 'hand_games': tally.hand_games,
           'hand_seats': tally.hand_seats, 'aborted': tally.aborted,
           'best_vs_explore': rate(tally.best[0], tally.best[1]), 'definitions': DEFINITIONS, 'credit': None,
           'seventeen': None, 'cards': rows}
    if credit:
        out['credit'] = draw_credit(specs, keep, run, credit, of, credit_games, threads, log)
        by = out['credit'].pop('by_card')
        for r in rows:
            r['credit'] = by.get(r['name'])
    if yardstick:
        y = seventeen_lands()
        out['seventeen'] = {k: v for k, v in y.items() if k != 'cards'}
        for r in rows:
            r['seventeen'] = y['cards'].get(r['name'])
    rows.sort(key=lambda r: (r['gih'] is None, -((r['gih'] or {}).get('wr') or 0), r['name']))
    return out


def draw_credit(specs, games, run, which, seats_of, max_games=None, threads=4, log=None):
    """cardcredit's draw credit of the given version-2 games (the newest `max_games`) with checkpoint `which` -> {checkpoint
    iter, games, draws, mean_change, ..., by_card: {name: {credit, se, relative, draws}}}."""
    from mtgz.sealed import cardcredit as CC
    path, it = checkpoint_of(run, which)
    if max_games:
        games = games[-int(max_games):]
    if not games:
        raise SystemExit('draw credit: no version-2 game in the window (the draws are logged from ENGINE.md 107.3 on)')
    out = CC.credit_games(specs, games, path, threads=threads, log=log, seats_of=seats_of)
    by = {c['name']: {'credit': c['credit'], 'se': c['se'], 'relative': c['relative'], 'draws': c['draws']}
          for c in out['cards'] if c['draws']}
    return {'checkpoint': os.path.basename(path), 'checkpoint_iter': it, 'games': out['games'],
            'draws': out['draws_credited'], 'mean_change': out['mean_change'], 'mean_change_se': out['mean_change_se'],
            'opening': out['opening'], 'by_card': by}


# ---------------------------------------------------------------------------------------------------- the command line

def _pct(r, key='wr'):
    return '     -' if not r else f'{100 * r[key]:5.1f}%'


def table(out, sort='gih', top=40, min_games=0):
    """The summary as text: a header and the cards sorted by `sort` (a metric, 'iwd', 'credit' or '17l'), those with at
    least `min_games` GIH games (GP games for 'gp')."""
    w = out['window']
    lines = [f"{out['run']}: {out['phase']} games" + (f' of iterations {w[0]}-{w[1]}' if w else '')
             + f" ({out['games']} games, {out['hand_games']} with draws; {out['seats_counted']} {out['seats']} seats)"]
    if out['mode'] or out['pair']:
        lines[0] += f" - mode {out['mode'] or 'any'}, pair {out['pair'] or 'any'}"
    if out['best_vs_explore']:
        b = out['best_vs_explore']
        lines.append(f"best guess vs explore build: {100 * b['wr']:.1f}% over {b['n']} games")
    if out['credit']:
        c = out['credit']
        lines.append(f"draw credit: {c['checkpoint']} over {c['games']} games, {c['draws']} draws, mean change {c['mean_change']}")
    if out['seventeen']:
        y = out['seventeen']
        lines.append(f"17L = 17lands {y['metric']} ({y['event_name']}, {y['games']} human games; {y['license']}): a yardstick only")
    key = {'17l': lambda r: (r.get('seventeen') or {}).get('wr'),
           'credit': lambda r: (r.get('credit') or {}).get('credit'),
           'iwd': lambda r: (r['iwd'] or {}).get('d')}.get(sort, lambda r: (r.get(sort) or {}).get('wr'))
    need = 'gp' if sort == 'gp' else 'gih'
    rows = [r for r in out['cards'] if ((r.get(need) or {}).get('n') or 0) >= min_games and key(r) is not None]
    rows.sort(key=lambda r: -key(r))
    lines.append(f"{'card':32s} {'#GP':>5s} {'GP WR':>6s} {'OH WR':>6s} {'GD WR':>6s} {'#GIH':>5s} {'GIH WR':>6s} "
                 f"{'95%':>11s} {'GNS WR':>6s} {'IWD':>6s} {'credit':>7s} {'17L':>6s}")
    for r in rows[:top]:
        g = r['gih']
        ci = f"{100 * g['lo']:4.1f}-{100 * g['hi']:4.1f}" if g else ''
        iwd = f"{100 * r['iwd']['d']:+5.1f}" if r['iwd'] else '     -'
        cr = (r.get('credit') or {}).get('credit')
        y = r.get('seventeen')
        lines.append(f"{r['name'][:32]:32s} {(r['gp'] or {}).get('n', 0):5d} {_pct(r['gp'])} {_pct(r['oh'])} {_pct(r['gd'])} "
                     f"{(g or {}).get('n', 0):5d} {_pct(g)} {ci:>11s} {_pct(r['gns'])} {iwd:>6s} "
                     f"{f'{cr:+.4f}' if cr is not None else '      -':>7s} {_pct(y)}")
    return '\n'.join(lines)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0], formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('run', help='a run directory (its games/*.jsonl and metrics.jsonl), or a directory of game log files')
    w = ap.add_mutually_exclusive_group()
    w.add_argument('--last', type=int, default=10, help='the latest N finished iterations (0: all; default 10)')
    w.add_argument('--iters', default=None, help='an iteration window, A-B (inclusive) or one iteration')
    ap.add_argument('--phase', default='selfplay', choices=['selfplay', 'eval', 'all'])
    ap.add_argument('--seats', default='network', choices=['network', 'all'], help="the network's seats (default) or all")
    ap.add_argument('--mode', default=None, help="only seats of this builder mode (best, explore, chosen, given)")
    ap.add_argument('--pair', default=None, help='only seats whose deck is this colour pair (e.g. WG)')
    ap.add_argument('--sort', default='gih', choices=list(METRICS) + ['iwd', 'credit', '17l'])
    ap.add_argument('--min-games', type=int, default=0, help='the table: cards with at least this many GIH games')
    ap.add_argument('--top', type=int, default=40)
    ap.add_argument('--credit', default=None, metavar='CKPT', help="also the draw credit with this checkpoint ('latest': the run's newest)")
    ap.add_argument('--credit-games', type=int, default=None, help='draw credit over the newest N games of the window')
    ap.add_argument('--threads', type=int, default=4, help='torch threads for the draw credit')
    ap.add_argument('--no-17lands', action='store_true', help='leave the 17lands yardstick column out')
    ap.add_argument('--json', default=None, help='write the whole summary here')
    a = ap.parse_args(argv)
    log = lambda s: print(s, file=sys.stderr, flush=True)  # noqa: E731
    out = stats(a.run, a.last, parse_iters(a.iters) if a.iters else None, a.phase, a.seats, a.mode, a.pair, a.credit,
                a.credit_games, a.threads, not a.no_17lands, log)
    if a.json:
        with open(a.json, 'w', encoding='utf-8') as fh:
            json.dump(out, fh, indent=1)
    print(table(out, a.sort, a.top, a.min_games))
    return out


if __name__ == '__main__':
    main(sys.argv[1:])
