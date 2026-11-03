"""Behaviour fingerprints of the Legacy Izzet Delver mirror (ENGINE.md 73.5): how a line's checkpoints
play, per band of generations, from the recorded evaluation replays (runs/<run>/replays/*.json.gz,
zero-replay/1: the Delver AI publisher's games, each a generation's network against itself). Read-only.
Productizes the Fable review's scripts of 2026-10-30 (fable_replays.py, fable_replays2.py,
mull_replays.py), with their definitions fixed where they miscounted (below).

  python tools/delver_fingerprints.py r3-2b-beliefsearch                 # bands of 20 generations
  python tools/delver_fingerprints.py r3-2b-beliefsearch delver2 --band 40 --min-gen 100
  python tools/delver_fingerprints.py runs/x --json out.json             # + the counts as JSON

A run is a name under runs/ (this checkout's, else the main checkout's) or a path. Both seats of a game
are the same network, so every metric counts both players' decisions. Each cell is "rate (n)": n is
the metric's denominator in that band (a few dozen games a band: read the differences of rates
against their binomial noise, sqrt(r (1 - r) / n)).

Metrics (a "fetch" is Flooded Strand, Misty Rainforest, Scalding Tarn or Polluted Delta):

  fetch cracked now     of the fetches played and later cracked: the share cracked at once (the
                        activation is the log line after the land is played, or the one after that -
                        nothing else happened between). The rest is held to later in the turn, the
                        next turn or later (`--json`: 'fetch_crack' counts; 'never' = not cracked by
                        the game's end, not in n).
  brainstorm w/ shuffle of the Brainstorms cast (the chosen 'Cast Brainstorm'): the share with a shuffle
                        to follow - the caster's own fetch ability on the stack (cast in response to
                        it: `bs in resp. to fetch`), an uncracked fetch of its own on the battlefield,
                        or a fetch in hand with its land drop unused in its own turn before the end
                        step (upkeep, draw, main 1 ... main 2: it can still play and crack it).
  murktide delve best   of the Murktide Regents cast: the share whose delve exiled as many instants
                        and sorceries as it could (min(5, those in the caster's graveyard at the cast);
                        the exiled cards: the log's 'exiles X (delve)' lines of that cast). Countered
                        Murktides count (the delve was chosen at the cast). `--json`: 'delve_short'.
  fow pitched fow       of Force of Will's alternative cost payments (the ExileCost decision with the
                        caster's FoW on top of the stack) where the hand offered another blue card than
                        a Force of Will: the share that exiled a Force of Will.
  tap-out into Daze     of the spells cast in the caster's own main phase with an empty stack from
                        turn 3 on (global turns; free spells excluded: Daze, Force of Will, the
                        Baubles) where the opponent - at its first decision after the cast, when Daze
                        could answer - holds a Daze and controls an Island (Volcanic Island, Island,
                        Thundering Falls): the share where the caster had no untapped land left
                        (Wasteland makes the {1}; an uncracked fetch counts, it fetches an untapped
                        land). `tapped out: dazed`: of those, the share the opponent then Dazed.
  T1-2 Volcanic, U only of the fetch searches in the decider's own turns 1-2 (global turns 1-4) with the
                        basic Island in its library ('Find Island' offered) whose turn then cast
                        spells and none red (Lightning Bolt, Dragon's Rage Channeler, Unholy Heat,
                        Cori-Steel Cutter, Prismari Charm): the share that found Volcanic Island
                        instead of the Wasteland-proof basic. `... nothing cast`: the same when
                        nothing was cast for the rest of that turn.
  mulligans             the opening seven: keep rate by lands in hand (0, 1, 2, 3, 4+) and play / draw,
                        the network's mean prior on 'Mulligan', and how often the search visited it.

Fixes against the review's scripts: 'T1-2' was global turns 1-2 (each player's first turn only) and
read the Island's zone AFTER the fetch had found it (a found Island never counted as "in the library":
the Volcanic share was 100% by construction) - now the search's own options say what the library
held; the tap-out spare lands ignored Wasteland and uncracked fetches (both pay Daze's {1}) and were
read one state after the cast (mana payment may not have finished) - now at the opponent's first
decision; Murktide's delve skipped countered casts.
"""
import argparse
import collections
import glob
import gzip
import json
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
MAIN_RUNS = r'C:/Users/Gregg Keithley/mtg-zero/runs'
FETCH = {'Flooded Strand', 'Misty Rainforest', 'Scalding Tarn', 'Polluted Delta'}
ISLANDS = {'Volcanic Island', 'Island', 'Thundering Falls'}            # Daze returns an Island (by type)
LANDS = ISLANDS | FETCH | {'Wasteland'}
BLUE = {'Brainstorm', 'Ponder', 'Daze', 'Force of Will', 'Prismari Charm', 'Murktide Regent',
        'Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar'}
INST_SORC = {'Brainstorm', 'Ponder', 'Daze', 'Force of Will', 'Lightning Bolt', 'Unholy Heat', 'Prismari Charm'}
RED = {'Lightning Bolt', "Dragon's Rage Channeler", 'Unholy Heat', 'Cori-Steel Cutter', 'Prismari Charm'}
FREE = {'Daze', 'Force of Will', "Mishra's Bauble", "Urza's Bauble"}
LINE = re.compile(r'T(\d+) (\w+) (.*)')


def run_dir(name):
    for base in (os.path.join(ROOT, 'runs'), MAIN_RUNS):
        d = os.path.join(base, name)
        if os.path.isdir(d):
            return d
    if os.path.isdir(name):
        return name
    raise SystemExit(f'no run {name!r} (runs/ of this checkout or {MAIN_RUNS}, or a path)')


def gen_of(d):
    m = re.search(r'Generation (\d+)', d.get('title') or '')
    return int(m.group(1)) if m else None


def full_log(d):
    """[(state index, line)]: every log line, at the state where it first appears."""
    out, seen = [], 0
    for i, s in enumerate(d['states']):
        n, lines = s.get('log_n', 0), s.get('log', [])
        new = n - seen
        if new > 0:
            out.extend((i, ln) for ln in (lines[-new:] if new <= len(lines) else lines))
            seen = n
    return out


def body(line):
    m = LINE.match(line)
    return (int(m.group(1)), m.group(2), m.group(3)) if m else (None, None, line)


def chosen(s, fm):
    opts = {o['a']: o['t'] for o in (s.get('decision') or {}).get('options', [])}
    return opts.get(fm.get('a')), opts


class Band:
    def __init__(self):
        self.games = 0
        self.c = collections.Counter()
        self.mull = collections.defaultdict(lambda: [0, 0, 0.0, 0])      # (lands, pos) -> keeps, n, prior, searched

    def rate(self, num, den):
        n = self.c[den]
        return (self.c[num] / n if n else None), n


def scan_game(d, B, players=None):
    """Count game `d`'s metrics into `B`; `players` (a set of 0 / 1): only those players' decisions and
    log lines (a game of two different players, e.g. a lookahead seat against an MCTS one, ENGINE.md
    80.10); None: both (the mirror). The game-level counts (decided, starter won) are the game's."""
    st, fr = d['states'], d['frames']
    res = d['result']
    B.games += 1
    if res[0] != res[1]:
        B.c['decided'] += 1
        if (0 if res[0] > res[1] else 1) == st[0].get('starting'):
            B.c['starter_won'] += 1
    log = full_log(d)
    # ---------------------------------------------------------------- decision-driven
    # (a frame's state is states[frame['s']]: zero-replay/1 stores a state once per change, so frames
    # and states are not 1:1 in ~5% of recorded games; `i` is the state index, as the log's)
    for fi, fm in enumerate(fr):
        i = fm.get('s', fi)
        s = st[i]
        dec = s.get('decision') or {}
        if not dec:
            continue
        p = dec.get('player')
        if p is None or (players is not None and p not in players):
            continue
        op = 1 - p
        me, them = s['players'][p], s['players'][op]
        ch, opts = chosen(s, fm)
        kind = dec.get('kind')
        stack = s.get('stack') or []
        own_main = kind == 'Priority' and s.get('active') == p and s.get('step') in ('main1', 'main2') and not stack
        if kind == 'Mulligan' and me.get('mulligans', 0) == 0 and len(me['hand']) == 7:
            lands = min(4, sum(1 for c in me['hand'] if c.get('name') in LANDS))
            pos = 'play' if p == s.get('starting', s.get('active')) else 'draw'
            r = B.mull[(lands, pos)]
            r[0] += ch != 'Mulligan'
            r[1] += 1
            mi = [k for k, a in enumerate(fm.get('acts', [])) if opts.get(a) == 'Mulligan']
            if mi and fm.get('p0'):
                r[2] += fm['p0'][mi[0]]
                r[3] += (fm.get('n') or [0] * (mi[0] + 1))[mi[0]] > 0
        if ch == 'Cast Brainstorm':
            resp = any(x.get('ability') == 'activated' and x.get('name') in FETCH and x.get('controller') == p for x in stack)
            bf = any(c['name'] in FETCH and c.get('controller', c.get('owner')) == p for c in me['battlefield'])
            # (a fetch in hand, the land drop unused, the caster's own turn before its end step: it can
            # still play the fetch and crack it this turn - from upkeep / draw through main 2)
            hand = (any(c['name'] in FETCH for c in me['hand']) and me.get('lands_played', 0) == 0
                    and s.get('active') == p and s.get('step') not in ('end', 'cleanup'))
            B.c['bs'] += 1
            B.c['bs_shuffle'] += resp or bf or hand
            B.c['bs_resp'] += resp
        if kind == 'ExileCost':
            top = [x for x in stack if x.get('spell')][-1:]
            if top and top[0]['name'] == 'Force of Will' and top[0].get('controller') == p:
                names = {t[len('Exile '):] for t in opts.values() if t.startswith('Exile ')}
                if names - {'Force of Will'} and 'Force of Will' in names:
                    B.c['fow_alt'] += 1
                    B.c['fow_pitch_fow'] += ch == 'Exile Force of Will'
                elif names - {'Force of Will'}:
                    B.c['fow_alt_no_fow'] += 1
        if kind == 'SearchLibrary' and s.get('active') == p and (s.get('turn') or 99) <= 4:
            if any(x.get('ability') == 'activated' and x.get('name') in FETCH and x.get('controller') == p for x in stack) \
                    and 'Find Island' in opts.values() and ch:
                casts, red = 0, False
                rest = [ln for j, ln in log if j > i]
                for ln in rest:
                    if 'Untap turn' in ln:
                        break
                    m = re.search(rf'P{p} casts (.*) \(Hand\)', ln)
                    if m:
                        casts += 1
                        red |= m.group(1) in RED
                if not red:
                    key = 't12_ublue' if casts else 't12_nothing'
                    B.c[key] += 1
                    B.c[key + '_volc'] += ch == 'Find Volcanic Island'
        if own_main and ch and ch.startswith('Cast ') and (s.get('turn') or 0) >= 3 and ch[5:] not in FREE:
            j = next((j for j in range(i + 1, len(st)) if (st[j].get('decision') or {}).get('player') == op), None)
            if j is not None:
                sj = st[j]
                them_j, me_j = sj['players'][op], sj['players'][p]
                live = any(c['name'] == 'Daze' for c in them_j['hand']) and any(
                    c['name'] in ISLANDS for c in them_j['battlefield'])
                if live:
                    spare = sum(1 for c in me_j['battlefield'] if 'land' in c.get('types', ['land'])
                                and c.get('name') in LANDS and not c.get('tapped'))
                    B.c['daze_live'] += 1
                    if spare == 0:
                        B.c['daze_tapout'] += 1
                        # the opponent's Daze before the spell left the stack
                        card = ch[5:]
                        dazed = False
                        for k, ln in log:
                            if k <= i:
                                continue
                            if f'P{op} casts Daze' in ln:
                                dazed = True
                                break
                            if f'{card} Stack ->' in ln:
                                break
                        B.c['daze_tapout_dazed'] += dazed
    # ---------------------------------------------------------------- log-driven
    played = collections.defaultdict(list)
    for k, (i, ln) in enumerate(log):
        turn, step, b = body(ln)
        if turn is None:
            continue
        m = re.match(r'P([01]) (?:plays|activates|casts) ', b)
        if players is not None and m and int(m.group(1)) not in players:
            continue
        m = re.match(r'P([01]) plays (.*)$', b)
        if m and m.group(2) in FETCH:
            played[(int(m.group(1)), m.group(2))].append(k)
        m = re.match(r'P([01]) activates (Flooded Strand|Misty Rainforest|Scalding Tarn|Polluted Delta)', b)
        if m:
            key = (int(m.group(1)), m.group(2))
            if played[key]:
                k0 = played[key].pop(0)
                turns = sum(1 for _, l2 in log[k0 + 1:k] if 'Untap turn' in l2)
                how = 'now' if k - k0 <= 2 else ('later this turn' if turns == 0 else ('next turn' if turns == 1 else
                                                                                       '2+ turns'))
                B.c['fetch_cracked'] += 1
                B.c['fetch_' + how] += 1
        m = re.match(r'P([01]) casts Murktide Regent', b)
        if m:
            p = int(m.group(1))
            s0 = st[i - 1] if i > 0 else st[i]
            avail = sum(1 for c in s0['players'][p]['graveyard'] if c['name'] in INST_SORC)
            got = 0
            for _, l2 in log[k + 1:k + 60]:
                _, _, b2 = body(l2)
                m2 = re.match(rf'P{p} exiles (.*) \(delve\)', b2)
                if m2:
                    got += m2.group(1) in INST_SORC
                if b2.startswith('Murktide Regent Stack ->') or 'counters Murktide' in b2:
                    break
            best = min(5, avail)
            B.c['murk'] += 1
            B.c['murk_best'] += got >= best
            B.c['murk_short_cards'] += max(0, best - got)
    for key, ks in played.items():
        B.c['fetch_never'] += len(ks)


METRICS = [
    ('fetch cracked now', 'fetch_now', 'fetch_cracked'),
    ('brainstorm w/ shuffle', 'bs_shuffle', 'bs'),
    ('bs in resp. to fetch', 'bs_resp', 'bs'),
    ('murktide delve best', 'murk_best', 'murk'),
    ('fow pitched fow', 'fow_pitch_fow', 'fow_alt'),
    ('tap-out into Daze', 'daze_tapout', 'daze_live'),
    ('tapped out: dazed', 'daze_tapout_dazed', 'daze_tapout'),
    ('T1-2 Volcanic, U only', 't12_ublue_volc', 't12_ublue'),
    ('T1-2 Volcanic, nothing cast', 't12_nothing_volc', 't12_nothing'),
    ('starting player won', 'starter_won', 'decided'),
]


def collect(runs, band, lo, hi):
    bands = collections.OrderedDict()
    for run in runs:
        d0 = run_dir(run)
        name = os.path.basename(os.path.normpath(d0))
        files = sorted(glob.glob(os.path.join(d0, 'replays', '*.json.gz')))
        per = {}
        for f in files:
            with gzip.open(f, 'rt', encoding='utf-8') as fh:
                d = json.load(fh)
            if d.get('format') != 'zero-replay/1' or d.get('selfplay'):
                continue
            g = gen_of(d)
            if g is None or g < lo or g > hi:
                continue
            b0 = g // band * band
            key = (name, b0)
            per.setdefault(key, Band())
            per[key].lo = min(getattr(per[key], 'lo', g), g)
            per[key].hi = max(getattr(per[key], 'hi', g), g)
            scan_game(d, per[key])
        for key in sorted(per):
            bands[key] = per[key]
    return bands


def cell(B, num, den):
    r, n = B.rate(num, den)
    return f'{"-" if r is None else f"{100 * r:.0f}%"} ({n})'


def table(bands):
    names = sorted({name for name, _ in bands})
    cols = [f'{B.lo}-{B.hi}' if len(names) == 1 else f'{name} {B.lo}-{B.hi}' for (name, _), B in bands.items()]
    w0 = max(len(m[0]) for m in METRICS) + 2
    ws = [max(len(c), 10) + 2 for c in cols]
    head = 'generations' if len(names) == 1 else 'run, generations'
    out = ([f'run {names[0]}'] if len(names) == 1 else []) + [head.ljust(w0) + ''.join(c.rjust(w) for c, w in zip(cols, ws))]
    out.append('games'.ljust(w0) + ''.join(str(B.games).rjust(w) for B, w in zip(bands.values(), ws)))
    for label, num, den in METRICS:
        out.append(label.ljust(w0) + ''.join(cell(B, num, den).rjust(w) for B, w in zip(bands.values(), ws)))
    out.append('')
    out.append('mulligans (opening 7): keep rate (n) | mean Mulligan prior | searched')
    keys = sorted({k for B in bands.values() for k in B.mull})
    for k in keys:
        lab = f'  {k[0] if k[0] < 4 else "4+"} lands, {k[1]}'
        row = lab.ljust(w0)
        for B, w in zip(bands.values(), ws):
            keep, n, ps, vis = B.mull.get(k, [0, 0, 0.0, 0])
            row += (f'{100 * keep / n:.0f}% ({n})' if n else '-').rjust(w)
        out.append(row)
    out.append('  prior / searched:'.ljust(w0) + ''.join(
        f'{sum(v[2] for v in B.mull.values()) / max(1, sum(v[1] for v in B.mull.values())):.3f} / '
        f'{sum(v[3] for v in B.mull.values())}'.rjust(w) for B, w in zip(bands.values(), ws)))
    return '\n'.join(out)


def as_json(bands):
    out = []
    for (name, b0), B in bands.items():
        out.append({'run': name, 'band': [B.lo, B.hi], 'games': B.games, 'counts': dict(B.c),
                    'rates': {label: B.rate(num, den)[0] for label, num, den in METRICS},
                    'n': {label: B.rate(num, den)[1] for label, num, den in METRICS},
                    'mulligans': {f'{k[0]}|{k[1]}': {'keep': v[0], 'n': v[1], 'prior_sum': round(v[2], 4), 'searched': v[3]}
                                  for k, v in sorted(B.mull.items())}})
    return out


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('runs', nargs='+')
    ap.add_argument('--band', type=int, default=20, help='generations per band (default 20)')
    ap.add_argument('--min-gen', type=int, default=0)
    ap.add_argument('--max-gen', type=int, default=10 ** 9)
    ap.add_argument('--json', default=None, help='also write the counts and rates here')
    a = ap.parse_args(argv)
    if a.band < 1:
        ap.error('--band >= 1')
    bands = collect(a.runs, a.band, a.min_gen, a.max_gen)
    if not bands:
        raise SystemExit('no replays in range')
    print(table(bands))
    if a.json:
        with open(a.json, 'w', encoding='utf-8') as f:
            json.dump(as_json(bands), f, indent=1)


if __name__ == '__main__':
    sys.exit(main())
