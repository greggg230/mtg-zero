"""`python -m mtgz.az.sideboard_census`: what a best-of-three checkpoint sideboards before game 2 - a census of its
sideboarding decisions (docs/ENGINE.md 132), for the Best of 3 page of delverai.greggg230.com.

  python -m mtgz.az.sideboard_census runs/<run>/models/iter_NNNN.pt [--starts 200] [--sims N] [--device cpu]
      [--threads 4] [--seed 1] [--out FILE]

N synthetic post-sideboard starts of the checkpoint's run spec (docs/ENGINE.md 130: `"start_game": 2`, each match
begins at the sideboarding before game 2 with game 1's winner drawn from the match's seed). In each the checkpoint
decides every sideboarding decision of both players - game 1's loser first - and then the loser's play-or-draw
choice for game 2, searching as the recorded matches do (`mtgz.az.replays.record`: the run's evaluation move rule,
no noise, the leaf values' view and root search of `replays.leaf_view` / `root_search`, the run's evaluation
belief and deck tilts; the run's self-play simulations `sims` unless --sims). No game is played: each start stops
at the first decision of game 2 that is neither sideboarding nor the play-or-draw choice (the mulligans).

Each player's 60 is recorded as copy changes against the registered main and split by whether that player is then
on the play or on the draw (the chooser's search decides it after both decks are chosen), and by chooser (game 1's
loser: sideboards first, then chooses) or not. Each split holds one sideboarding of each start: its n is the
number of starts, its samples independent. Per split: the share that kept the main 60 and the mean number of cards
swapped, each with its standard error, and per card the distribution of its copy change (+3 Pyroblast in 50% of
them, -4 Force of Will in 60%) with the mean change and its standard error. Game 3's sideboarding (it depends on
game 2) is not in it.

The census is the publisher's (tools/delverai_publish.py, `--census N`: each recorded generation's), never the
training run's: it reads the checkpoint and its run's config.json only.
"""
import argparse
import json
import math
import os
import random
import time

import mtgz.az  # noqa: F401  (registers the game package)
from mtgz.az import replays
from zero import replay as core
from zero.infer import NetEvaluator, drive
from zero.net import load
from zero.play import NNSeat, PyDriver

STARTS = 200           # the default number of starts (two sideboardings each)
SPLITS = ('play', 'draw', 'chooser', 'other')
PRE_GAME = ('Sideboard', 'PlayDraw')
MAX_STEPS = 400        # a start's decisions before game 2 (a sideboarding is ~5-15): past this it is a failure


class CensusError(RuntimeError):
    """A start that did not reach game 2 as a census start must (never a silent gap in the counts)."""


class _CensusDriver(PyDriver):
    """zero.play's Python driver whose games stop at game 2's first decision of play: each start's record in
    `self.records` (gid -> {winner1, order, chooser, choice, starting, decks})."""

    def __init__(self, *a, **kw):
        self.records = {}
        self._notes = {}
        super().__init__(*a, **kw)

    def _apply(self, slot, a):
        st = json.loads(slot.game.export_state())
        d = st.get('decision') or {}
        note = self._notes.setdefault(slot.gid, {'order': []})
        if d.get('kind') == 'Sideboard':
            if not note['order'] or note['order'][-1] != d.get('player'):
                note['order'].append(d.get('player'))
            note.setdefault('wins', (st.get('match') or {}).get('wins'))
        elif d.get('kind') == 'PlayDraw':
            note['chooser'] = d.get('player')
            note['choice'] = replays._option_text(st, a)
        super()._apply(slot, a)
        g = slot.game
        if g.over:
            raise CensusError(f'start {slot.gid}: the game ended before game 2 was played')
        st = json.loads(g.export_state())
        kind = (st.get('decision') or {}).get('kind')
        if kind in PRE_GAME:
            return
        m = st.get('match') or {}
        if m.get('game') != 2 or 'chooser' not in note:
            raise CensusError(f'start {slot.gid}: left the sideboarding at game {m.get("game")} '
                              f'({kind}) without a play-or-draw choice')
        self.records[slot.gid] = dict(note, starting=st.get('starting'), decks=m.get('decks'),
                                      results=m.get('results'))
        slot.steps = self.max_steps                 # done: the driver finishes the slot


def search_seat(cfg, sims):
    """The network seat the census searches with: the recorded matches' (`replays.record`: evaluation settings,
    the leaf values' view and root search by `replays.leaf_view` / `replays.root_search`)."""
    side = dict(cfg, eval_leaf_value_view=replays.leaf_view(cfg), eval_root_search=replays.root_search(cfg))
    return core._nn('a', sims, side, False, cfg.get('chance_k', 0))


def run_starts(ckpt, starts=STARTS, sims=None, device='cpu', seed=1, conc=None, log=print):
    """Play `starts` census starts of `ckpt` -> (records in start order, info). `conc`: starts searched side by
    side (default all: one network batch carries a leaf of each)."""
    replays.check_encoding(ckpt)
    run, it = core.run_of(ckpt)
    cfg = core.run_config(run)
    if not cfg.get('spec'):
        raise SystemExit(f'{ckpt}: no run config with a spec next to it ({run or "no run directory"})')
    spec = json.loads(cfg['spec'])
    if not (spec.get('constructed') and spec.get('format') == 'bo3' and int(spec.get('start_game') or 1) == 2):
        raise SystemExit(f'{ckpt}: a sideboard census needs a constructed best-of-three spec with "start_game": 2')
    sims = int(sims or cfg.get('sims', 64))
    model, _ = load(ckpt, device)
    game = model.config.get('game', 'mtg')
    ev = NetEvaluator(model, device)
    seat = search_seat(cfg, sims)
    t0 = time.time()
    drv = _CensusDriver(game, starts, lambda k: [seat, seat], spec=cfg['spec'], concurrency=min(conc or starts, starts),
                        seed=seed, max_steps=MAX_STEPS)
    drive(drv, {'a': ev})
    secs = time.time() - t0
    missing = [k for k in range(starts) if k not in drv.records]
    if missing:
        raise CensusError(f'{len(missing)} of {starts} starts reached no game 2 (first: {missing[:5]})')
    info = {'iter': it, 'run': os.path.basename(run) if run else None, 'starts': starts, 'sims': sims,
            'select': core._with_rule({}, seat).get('select', 'visits'), 'seed': seed, 'seconds': round(secs, 1),
            'device': device, 'simulations': drv.sims}
    log(f'sideboard census of {os.path.basename(ckpt)}: {starts} starts in {secs:.0f}s ({drv.sims} simulations)')
    return [drv.records[k] for k in range(starts)], info


def sideboardings(records, spec):
    """Each start's two sideboardings: [{start, seat, chooser, play, deck: {name: copies}, delta: {name: change}}].
    Checks what a census start must be (CensusError otherwise): game 1's loser sideboarded first and chose, each
    deck is 60 cards of the seat's 75, and exactly one player is on the play."""
    mains, pools = replays.main_decks(spec), _pool_counts(spec)
    out = []
    for k, r in enumerate(records):
        wins = r.get('wins') or [0, 0]
        loser = 0 if wins == [0, 1] else 1 if wins == [1, 0] else None
        if loser is None or r['order'][:1] != [loser] or r['chooser'] != loser or sorted(r['order']) != [0, 1]:
            raise CensusError(f'start {k}: game 1 {wins}, sideboarding order {r["order"]}, chooser {r["chooser"]}')
        if r.get('starting') not in (0, 1):
            raise CensusError(f'start {k}: no starting player in game 2 ({r.get("starting")})')
        for p in (0, 1):
            deck = replays._tally(r['decks'][p])
            if sum(deck.values()) != 60 or any(c > pools[p].get(n, 0) for n, c in deck.items()):
                raise CensusError(f'start {k}: seat {p} deck is not 60 cards of its 75: {deck}')
            main = mains[p]
            delta = {n: deck.get(n, 0) - main.get(n, 0) for n in set(deck) | set(main)}
            out.append({'start': k, 'seat': p, 'chooser': p == r['chooser'], 'play': p == r['starting'],
                        'choice': r.get('choice'), 'deck': deck, 'delta': {n: d for n, d in delta.items() if d}})
    return out


def _pool_counts(spec):
    s = json.loads(spec) if isinstance(spec, str) else spec
    d = s.get('decks') or {}
    return {p: replays._tally(list(d[f'p{p}']) + list(d.get(f'p{p}_sideboard') or [])) for p in (0, 1)}


def _share(k, n):
    p = k / n if n else 0.0
    return [round(p, 4), round(math.sqrt(p * (1 - p) / n), 4) if n else None]


def _mean(xs):
    n = len(xs)
    if not n:
        return [None, None]
    m = sum(xs) / n
    var = sum((x - m) ** 2 for x in xs) / (n - 1) if n > 1 else 0.0
    return [round(m, 4), round(math.sqrt(var / n), 4)]


def split_summary(sbs, order):
    """One split's sideboardings -> {n, kept: [share, se], swapped: [mean cards brought in, se], cards: [{name,
    mean: [change, se], d: [[change, count], ...] (the copy changes seen, most first; the rest of the n
    unchanged)}]} - the cards it changed, in `order` (the 75's: sideboard cards then the main's)."""
    n = len(sbs)
    swapped = [sum(d for d in s['delta'].values() if d > 0) for s in sbs]
    cards = []
    for name in order:
        ch = [s['delta'].get(name, 0) for s in sbs]
        tally = {}
        for d in ch:
            if d:
                tally[d] = tally.get(d, 0) + 1
        if not tally:
            continue
        cards.append({'name': name, 'mean': _mean(ch),
                      'd': sorted(([d, c] for d, c in tally.items()), key=lambda x: (-abs(x[0]), -x[1]))})
    return {'n': n, 'kept': _share(sum(1 for x in swapped if x == 0), n), 'swapped': _mean(swapped), 'cards': cards}


def summarize(records, spec, info=None):
    """The census record (the progress record's `sideboard_census`): `info` (iter, starts, sims, select, seed,
    seconds, ...), `chose_play` ([starts where the chooser took the play, starts]), and the splits `play`, `draw`
    (by who is on the play in game 2), `chooser`, `other` (game 1's loser, who sideboarded first and chose, or
    not) - each split_summary's."""
    s = json.loads(spec) if isinstance(spec, str) else spec
    sbs = sideboardings(records, s)
    d = s['decks']
    order = list(dict.fromkeys(list(d.get('p0_sideboard') or []) + list(d['p0'])))
    order += [n for n in dict.fromkeys(list(d.get('p1_sideboard') or []) + list(d['p1'])) if n not in order]
    out = dict(info or {})
    out['chose_play'] = [sum(1 for r in records if r['chooser'] == r['starting']), len(records)]
    for key, keep in (('play', lambda x: x['play']), ('draw', lambda x: not x['play']),
                      ('chooser', lambda x: x['chooser']), ('other', lambda x: not x['chooser'])):
        out[key] = split_summary([x for x in sbs if keep(x)], order)
    return out


def census(ckpt, starts=STARTS, sims=None, device='cpu', seed=1, conc=None, log=print):
    """run_starts + summarize -> the census record (with `time`: when it was made)."""
    records, info = run_starts(ckpt, starts=starts, sims=sims, device=device, seed=seed, conc=conc, log=log)
    run, _ = core.run_of(ckpt)
    rec = summarize(records, core.run_config(run)['spec'], info)
    rec['time'] = int(time.time())
    return rec


def describe(rec, top=8):
    """A census record in a few lines of text (the CLI's output)."""
    lines = [f"generation {rec.get('iter')}: {rec.get('starts')} starts, {rec.get('sims')} sims ({rec.get('select')}), "
             f"{rec.get('seconds')}s; the chooser took the play in {rec['chose_play'][0]} of {rec['chose_play'][1]}"]
    for key in SPLITS:
        sp = rec[key]
        lines.append(f"  {key:7s} n={sp['n']}  kept the main {sp['kept'][0]:.0%} +-{sp['kept'][1]:.0%}  "
                     f"swapped {sp['swapped'][0]:.2f} +-{sp['swapped'][1]:.2f}")
        for c in sorted(sp['cards'], key=lambda c: -abs(c['mean'][0]))[:top]:
            dist = ', '.join(f"{d:+d} {k / sp['n']:.1%}" for d, k in c['d'])
            lines.append(f"      {c['name'][:28]:28s} mean {c['mean'][0]:+.2f} +-{c['mean'][1]:.2f}   {dist}")
    return '\n'.join(lines)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('ckpt')
    ap.add_argument('--starts', type=int, default=STARTS, help=f'synthetic post-sideboard starts (default {STARTS})')
    ap.add_argument('--sims', type=int, default=None, help="default: the run's self-play sims (the recorded matches')")
    ap.add_argument('--device', default='cpu')
    ap.add_argument('--threads', type=int, default=4, help='torch threads on the CPU')
    ap.add_argument('--seed', type=int, default=1)
    ap.add_argument('--conc', type=int, default=None, help='starts searched side by side (default all)')
    ap.add_argument('--out', default=None, help='write the census record (JSON) here')
    ap.add_argument('--low-priority', action='store_true', help='run below normal priority')
    a = ap.parse_args(argv)
    if a.device == 'cpu':
        import torch
        torch.set_num_threads(a.threads)
    if a.low_priority:
        import psutil
        psutil.Process().nice(psutil.BELOW_NORMAL_PRIORITY_CLASS if os.name == 'nt' else 10)
    rec = census(a.ckpt, starts=a.starts, sims=a.sims, device=a.device, seed=a.seed, conc=a.conc)
    print(describe(rec))
    if a.out:
        os.makedirs(os.path.dirname(os.path.abspath(a.out)), exist_ok=True)
        with open(a.out, 'w', encoding='utf-8') as f:
            json.dump(rec, f, indent=1)
        print(f'wrote {a.out}')


if __name__ == '__main__':
    main()
