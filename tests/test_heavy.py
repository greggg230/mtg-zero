"""Heavy decisions on the Delver mirror (docs/ENGINE.md 79; zero-core docs/search.md 3.11, az.md 23): the
game's hooks (the passive option, which keys reveal something to a viewer), the reveal probe at turn-1
spots with Mishra's Bauble in hand (cast, then activate at its own library: a follow-up action - depth 2,
not 1), a Delver self-play driver with heavy decisions (rows' `heavy`, their budgets, light windows
never probed), the canaries' records, and the run settings through `python -m mtgz.az.run`."""
import json
import os
import subprocess
import sys

import numpy as np

import mtgz.az as P
from mtgz.az import games as G

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'tools'))

import heavy_canary as HC  # noqa: E402
from heavy_eval import opening_spot  # noqa: E402

HIDDEN = 0xFFFFFFFF


def key(kind, player, payload):
    return (kind << 56) | (player << 48) | payload


def bauble_opening(max_seeds=400):
    spec = json.loads(P.SPEC_DELVER)
    for s in range(max_seeds):
        r = opening_spot(spec, s)
        if r is None:
            continue
        g, p, hand = r
        _, lab = HC.state(g)
        if ("Cast " + HC.BAUBLE) in lab.values() and any(x.startswith('Play ') for x in lab.values()):
            return g, p, lab
    raise AssertionError('no turn-1 spot with a castable Bauble')


def test_the_hooks_and_the_probe_at_a_turn_one_bauble():
    g, p, lab = bauble_opening()
    acts = list(g.legal())
    before = g.export_state()
    assert g.passive_action() == 0 and lab[0] == 'Pass priority'
    i = acts.index(next(a for a, t in lab.items() if t == 'Cast ' + HC.BAUBLE))
    deep = g.heavy_probe(seed=1, depth=2)
    assert deep['reveal'][i], (deep, lab)
    # passing is the baseline (never probed)
    assert not deep['reveal'][acts.index(0)]
    # depth 1: the look needs a follow-up action (cast, then activate), never found; a land drop reveals
    # nothing on its own (at depth 2 it may: the land, then a cantrip it pays for)
    shallow = g.heavy_probe(seed=1, depth=1)
    assert not shallow['reveal'][i]
    for a, t in lab.items():
        if t.startswith('Play ') and t[5:] in HC.ISLANDS:
            assert not shallow['reveal'][acts.index(a)], t
    # the same answer in other worlds (the probe deals one for the decider)
    assert all(g.heavy_probe(seed=s, depth=2)['reveal'][i] for s in range(2, 6))
    # the probe never changes the game
    assert g.export_state() == before
    # which keys reveal: the viewer's own draws / looks (never masked ones), another player's card made
    # public, never a single-option decision's bookkeeping key or the viewer's own card made public
    q = 1 - p
    assert g.reveals(key(1, p, 55), p) and g.reveals(key(3, p, 55), p)
    assert not g.reveals(key(1, q, HIDDEN), p) and not g.reveals(key(1, q, 55), p) and not g.reveals(key(3, p, HIDDEN), p)
    assert g.reveals(key(2, q, 55), p) and not g.reveals(key(2, p, 55), p)
    assert not g.reveals(key(4, p, 7), p) and not g.reveals(key(7, 0, 7), p)
    assert g.reveals(key(5, q, 1), p) and g.reveals(key(6, q, 3), p)
    # cast the Bauble; its activation's target (required) has no passive option
    g.step(next(a for a, t in lab.items() if t == 'Cast ' + HC.BAUBLE))
    for _ in range(20):
        _, lab = HC.state(g)
        if ('Activate ' + HC.BAUBLE) in lab.values() and g.to_move == p:
            break
        g.step(0 if 0 in g.legal() else min(g.legal()))
    g.step(next(a for a, t in lab.items() if t == 'Activate ' + HC.BAUBLE))
    s, lab = HC.state(g)
    assert s['decision']['kind'] == 'Target' and g.passive_action() is None, (s['decision'], lab)


def uniform(lb):
    a = lb.a
    off = a['opt_off']
    b = len(a['player'])
    pri = np.concatenate([np.full(int(off[r + 1] - off[r]), 1.0 / max(1, int(off[r + 1] - off[r])), np.float32)
                          for r in range(b)]) if off[-1] else np.zeros(0, np.float32)
    v = np.full((b, 8), 0.0, np.float32)
    v[:, :2] = 0.5
    return pri, v, np.zeros((b, 8), np.float32)


def play(extra, games=4, seed=3, sims=16):
    seat = G.nn_seat('m', sims, selfplay=True, sims_fast=sims // 4, p_full=0.25, temp_moves=60, sims_minor=2,
                     policy_target='cq', cq_c_scale=0.3, **extra)
    d = G.Driver('mtg', [[seat, seat]] * games, spec=P.SPEC_DELVER, concurrency=games, seed=seed, record=True, threads=2,
                 move_log=100_000)
    rows = []
    while True:
        lb = d.next_batch()
        if lb is None:
            break
        d.submit(lb.id, *uniform(lb))
        rows += d.take_results()
        d.take_samples()
    rows += d.take_results()
    return sorted(rows, key=lambda r: r['gid']), d.stats()


def test_a_delver_driver_with_heavy_decisions():
    heavy = dict(sims_heavy=32, sims_heavy_fast=8, chance_k_heavy=2, root_min_visits_heavy=2)
    light = dict(sims_light=8, sims_light_fast=2, light_record=True, light_classes=list(range(22)))
    rows, st = play(dict(heavy, **light))
    rows0, st0 = play({})
    assert all('heavy' not in r for r in rows0) and st0['heavy_probed'] == 0
    searched = sum(r['decisions'] for r in rows)
    probed = sum(r['heavy']['probed'] for r in rows)
    lt = sum(r['light']['decisions'] for r in rows)
    # every searched decision is light (a window), minor (a payment: st['minor']) or probed
    assert probed + lt + st['minor'] == searched, (probed, lt, st['minor'], searched)
    hv = [r['heavy'] for r in rows]
    n = sum(h['decisions'] for h in hv)
    assert 0 < n < probed, (n, probed)
    assert all(8 * h['decisions'] <= h['sims'] <= 32 * h['decisions'] for h in hv)
    assert st['heavy'] == n and st['heavy_forced'] > 0 and st['heavy_probe_s'] > 0
    assert st['sims'] == sum(h['sims_all'] for h in hv)
    # the canaries read these rows (their move logs)
    recs = [HC.game_records(P.SPEC_DELVER, s, r['moves'], f'g{r["gid"]}') for r, s in
            zip(rows, [__import__('light_eval').game_seed(3, r['gid']) for r in rows])]
    out = HC.canary(recs)
    assert out['records']['games'] == len(rows) and set(out['b']) >= {'look_first', 'crack_after_seen_land'}


def test_the_run_settings():
    p = subprocess.run([sys.executable, '-m', 'mtgz.az.run', '--help'], capture_output=True, text=True, cwd=ROOT)
    for f in ('--selfplay-sims-heavy', '--selfplay-sims-heavy-fast', '--selfplay-chance-k-heavy',
              '--selfplay-root-min-visits-heavy', '--selfplay-heavy-prior-max', '--selfplay-heavy-depth',
              '--selfplay-heavy-horizon', '--selfplay-heavy-lines'):
        assert f in p.stdout, f
