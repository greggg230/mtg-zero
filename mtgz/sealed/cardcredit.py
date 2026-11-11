"""Per-card draw credit from a run's logged games (docs/ENGINE.md 107.3): the S4 card ranking's first signal.

The game log (version 2, `mtgz.sealed.games`) carries what replays each game exactly - its spec, seed and actions - so the
network's value before and after every draw can be computed offline with ANY checkpoint, later and better ones included.
For each logged game this tool replays the actions (`mtgz._engine` Game objects, CPU) and, at each step in which a player
draws, values the state from that player's own view (`encode(drawer)`, a value-only row: P(drawer wins), draws 0.5) just
before the step - the last decision before the draw - and just after it - the first decision after it (a finished game: its
result). A card's **draw credit** is the mean change over its draws (a step drawing n cards credits each with 1/n of its
change), with its standard error; `relative` subtracts the mean change over every credited draw (the step's other
events - a new turn's untap - shift every card alike). Hands drawn before the first turn (the opening hand, a mulligan's)
are left out unless `--include-opening` (they are judged as a hand, not a draw); their mean change is reported apart.

Beside it, from the log alone, the raw drawn-vs-not-drawn win rates (17lands' "game in hand" and "not seen"): per card and
game where its seat's deck holds it, whether it was seen - in the kept opening hand (the last seven cards drawn before the
first turn, less those put on the bottom) or drawn later - and the seat's result (1 / 0.5 / 0); `iwd` = seen - not seen.
A yardstick of the same games: longer games see more cards, so the raw rates mix card quality with game length.

    python -m mtgz.sealed.cardcredit RUN --checkpoint CKPT [--device cpu] [--phase selfplay|eval|all] [--games N]
        [--include-opening] [--min-draws 1] [--top 40] [--batch 256] [--threads 4] [--json PATH]

Pure self-play: everything here comes from the run's own games and networks (DESIGN.md 0). Every logged game must replay
to its logged result; a mismatch is an error (the log is the record, a game that does not replay is a broken record).
"""
import argparse
import base64
import json
import math
import sys

import numpy as np

from mtgz.sealed import games as GL

KEY_DRAW = 1


def actions_of(g):
    """A version-2 game line's actions (u16, in order)."""
    if g.get('v', 1) < 2 or 'actions' not in g:
        raise ValueError(f'game {g.get("seed")}: a version-1 line (no actions): logged before ENGINE.md 107.3')
    a = np.frombuffer(base64.b64decode(g['actions']), dtype='<u2')
    if len(a) != g['n_actions']:
        raise ValueError(f'game {g["seed"]}: {len(a)} actions decoded, {g["n_actions"]} logged')
    return [int(x) for x in a]


def replay_spec(spec_line):
    """The spec JSON a game line's spec line describes (its `v` and `spec` keys out): `new_game(spec, seed)` and the
    actions replay the game."""
    return json.dumps({k: v for k, v in spec_line.items() if k not in ('v', 'spec')})


def draws_of(keys):
    """(player, card definition) of every draw among a step's chance keys (`KEY_DRAW`: the true card, the replay is the
    live game)."""
    return [((k >> 48) & 0xFF, k & ((1 << 48) - 1)) for k in keys if (k >> 56) == KEY_DRAW]


def card_name(seat, c):
    """A line's card (an index among the seat's sorted deck names, or a name)."""
    return sorted(seat['deck'])[c] if isinstance(c, int) else c


def seen_cards(seat):
    """The names a seat saw in a game (version 2): its kept opening hand - the last seven of the cards it drew before the
    first turn, less those it put on the bottom - and every card it drew later."""
    opening = [card_name(seat, d[2]) for d in seat['draws'] if d[0] == 0][-7:]
    for b in seat['bottom']:
        n = card_name(seat, b)
        if n in opening:
            opening.remove(n)
    return set(opening) | {card_name(seat, d[2]) for d in seat['draws'] if d[0] > 0}


def raw_rates(games):
    """{name: {'gih': [results], 'gns': [results]}} over the games' seats whose deck holds the card."""
    out = {}
    for g in games:
        if g['result'] is None:
            continue
        for p, seat in enumerate(g['seats']):
            seen = seen_cards(seat)
            for n in seat['deck']:
                e = out.setdefault(n, {'gih': [], 'gns': []})
                e['gih' if n in seen else 'gns'].append(float(g['result'][p]))
    return out


def mean_se(xs):
    n = len(xs)
    if not n:
        return None, None
    m = sum(xs) / n
    if n < 2:
        return m, None
    return m, math.sqrt(sum((x - m) ** 2 for x in xs) / (n - 1) / n)


class Valuer:
    """The checkpoint's value of value-only rows, batched: `add(row)` -> an id, `run()` fills `values[id]` (P(the row's
    viewer wins))."""

    def __init__(self, checkpoint, device='cpu', batch=256):
        from zero.infer import NetEvaluator
        from zero.net import load
        m, _ = load(checkpoint, device, game='mtg_eoe')
        self.ev = NetEvaluator(m, device)
        self.batch, self.rows, self.values = int(batch), [], {}

    def add(self, row):
        self.rows.append(np.asarray(row, np.float32))
        return len(self.values) + len(self.rows) - 1

    def run(self):
        base = len(self.values)
        for i in range(0, len(self.rows), self.batch):
            x = np.stack(self.rows[i:i + self.batch])
            b = x.shape[0]
            _, val, _ = self.ev.evaluate(x, np.full(b, 2, np.int32), np.zeros(b + 1, np.int32), np.zeros(0, np.int32))
            for j in range(b):
                self.values[base + i + j] = float(val[j][0])
        self.rows = []


def credit_game(G, spec_json, g, valuer, include_opening=False):
    """Replay one game twice: find its draw steps, then value the drawer's view before and after each. -> (events, ok):
    events [(kind, n_cards, [names], before id, after id or None, terminal value or None)], ok = the replay ended at the
    logged result."""
    acts = actions_of(g)
    names = {}
    for seat in g['seats']:
        names.update(seat['deck'])
    ids = {v: k for k, v in GL_lookup(list(names)).items() if isinstance(v, int)}
    # pass 1: the draw steps (step index, drawer, cards, before the first turn?)
    game = G.engine().Game('mtg_eoe', spec_json, 2, g['seed'])
    steps = {}
    for i, a in enumerate(acts):
        t0 = game.rounds
        for p, d in draws_of(game.step(a)):
            e = steps.setdefault((i, p), {'cards': [], 'opening': game.rounds == 0 and t0 == 0})
            e['cards'].append(ids.get(int(d), f'def#{int(d)}'))
    ok = bool(game.over) and g['result'] is not None and [round(float(x), 4) for x in game.result()[:2]] == \
        [round(float(x), 4) for x in g['result']]
    # pass 2: the drawer's view before and after each draw step
    game = G.engine().Game('mtg_eoe', spec_json, 2, g['seed'])
    by_step = {}
    for (i, p), e in steps.items():
        by_step.setdefault(i, []).append((p, e))
    events = []
    for i, a in enumerate(acts):
        # (every draw step, the hands before the first turn included: their change is reported apart)
        todo = by_step.get(i, [])
        before = {p: valuer.add(game.encode(p)) for p, _ in todo}
        game.step(a)
        for p, e in todo:
            if game.over:
                after, term = None, float(game.result()[p])
            else:
                after, term = valuer.add(game.encode(p)), None
            events.append(('opening' if e['opening'] else 'draw', len(e['cards']), e['cards'], before[p], after, term))
    return events, ok


_LOOKUP = {}


def GL_lookup(names):
    """name -> engine card id (cached)."""
    from mtgz.sealed import engine as SE
    miss = [n for n in names if n not in _LOOKUP]
    if miss:
        _LOOKUP.update(SE.lookup(miss))
    return {n: _LOOKUP[n] for n in names}


def credit(run, checkpoint, device='cpu', phase='selfplay', max_games=None, include_opening=False, batch=256,
           threads=4, log=None):
    """The run's per-card draw credit (module docs) -> the summary dict."""
    import torch
    torch.set_num_threads(max(1, int(threads)))
    import mtgz.az  # noqa: F401  (the game package)
    from zero import games as G
    specs, games = GL.load(run)
    games = [g for g in games if (phase == 'all' or g['phase'] == phase) and g['result'] is not None]
    old = [g for g in games if g.get('v', 1) < 2]
    if old:
        raise SystemExit(f'{len(old)} version-1 game lines (no actions: logged before ENGINE.md 107.3) in {run}')
    if max_games:
        games = games[:int(max_games)]
    if not games:
        raise SystemExit(f'no finished {phase} games with actions in {run}')
    valuer = Valuer(checkpoint, device, batch)
    per_game, bad = [], []
    for k, g in enumerate(games):
        ev, ok = credit_game(G, replay_spec(specs[g['spec']]), g, valuer, include_opening)
        if not ok:
            bad.append((g['file'], g['seed']))
        per_game.append(ev)
        if log and (k + 1) % 25 == 0:
            log(f'# {k + 1} of {len(games)} games replayed, {len(valuer.rows)} rows waiting')
    valuer.run()
    if bad:
        raise SystemExit(f'{len(bad)} games did not replay to their logged result (first: {bad[:3]}): a broken record')
    vals = valuer.values
    per_card, deltas, opening = {}, [], []
    for ev in per_game:
        for kind, n, cards, b, a, term in ev:
            d = (term if a is None else vals[a]) - vals[b]
            if kind == 'opening':
                opening.append(d)
                if not include_opening:
                    continue
            deltas.append(d)
            for c in cards:
                per_card.setdefault(c, []).append(d / n)
    base, base_se = mean_se(deltas)
    raw = raw_rates(games)
    rows = []
    for name in sorted(set(per_card) | set(raw)):
        xs = per_card.get(name, [])
        m, se = mean_se(xs)
        r = raw.get(name, {'gih': [], 'gns': []})
        gih, _ = mean_se(r['gih'])
        gns, _ = mean_se(r['gns'])
        rows.append({'name': name, 'draws': len(xs), 'credit': None if m is None else round(m, 5),
                     'se': None if se is None else round(se, 5),
                     'relative': None if m is None or base is None else round(m - base, 5),
                     'gih': None if gih is None else round(gih, 4), 'gih_n': len(r['gih']),
                     'gns': None if gns is None else round(gns, 4), 'gns_n': len(r['gns']),
                     'iwd': None if gih is None or gns is None else round(gih - gns, 4)})
    rows.sort(key=lambda x: (x['credit'] is None, -(x['credit'] or 0.0)))
    om, ose = mean_se(opening)
    return {'run': run, 'checkpoint': checkpoint, 'phase': phase, 'games': len(games), 'replayed': len(games),
            'draws_credited': len(deltas), 'mean_change': None if base is None else round(base, 5),
            'mean_change_se': None if base_se is None else round(base_se, 5),
            'opening': {'steps': len(opening), 'mean_change': None if om is None else round(om, 5),
                        'se': None if ose is None else round(ose, 5), 'credited': bool(include_opening)},
            'rows': len(vals), 'cards': rows}


def table(out, top=40, min_draws=1):
    lines = [f"{out['games']} games replayed ({out['phase']}), {out['draws_credited']} draws credited, mean change "
             f"{out['mean_change']} +- {out['mean_change_se']} (opening hands: {out['opening']['steps']} steps, "
             f"{out['opening']['mean_change']})",
             f"{'card':34s} {'draws':>5s} {'credit':>8s} {'se':>7s} {'rel':>8s} {'GIH':>6s} {'n':>4s} {'GNS':>6s} {'n':>4s} {'IWD':>7s}"]
    shown = [c for c in out['cards'] if c['draws'] >= min_draws][:top]
    f = lambda x, w, p: (f'{x:{w}.{p}f}' if x is not None else ' ' * (w - 1) + '-')  # noqa: E731
    for c in shown:
        lines.append(f"{c['name'][:34]:34s} {c['draws']:5d} {f(c['credit'], 8, 4)} {f(c['se'], 7, 4)} "
                     f"{f(c['relative'], 8, 4)} {f(c['gih'], 6, 3)} {c['gih_n']:4d} {f(c['gns'], 6, 3)} {c['gns_n']:4d} "
                     f"{f(c['iwd'], 7, 3)}")
    return '\n'.join(lines)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0], formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('run', help='a run directory (its games/*.jsonl), or a directory of game log files')
    ap.add_argument('--checkpoint', required=True, help='any mtg_eoe checkpoint (.pt)')
    ap.add_argument('--device', default='cpu')
    ap.add_argument('--phase', default='selfplay', choices=['selfplay', 'eval', 'all'])
    ap.add_argument('--games', type=int, default=None, help='at most this many games (the log\'s order)')
    ap.add_argument('--include-opening', action='store_true', help='credit the hands drawn before the first turn too')
    ap.add_argument('--min-draws', type=int, default=1, help='the table: cards credited at least this often')
    ap.add_argument('--top', type=int, default=40, help='the table: this many cards')
    ap.add_argument('--batch', type=int, default=256, help='rows per network batch')
    ap.add_argument('--threads', type=int, default=4, help='torch threads')
    ap.add_argument('--json', default=None, help='write the whole summary here')
    ap.add_argument('--quiet', action='store_true')
    a = ap.parse_args(argv)
    log = None if a.quiet else (lambda s: print(s, file=sys.stderr, flush=True))
    out = credit(a.run, a.checkpoint, a.device, a.phase, a.games, a.include_opening, a.batch, a.threads, log)
    if a.json:
        with open(a.json, 'w', encoding='utf-8') as fh:
            json.dump(out, fh, indent=1)
    print(table(out, a.top, a.min_draws))
    return out


if __name__ == '__main__':
    main(sys.argv[1:])
