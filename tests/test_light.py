"""Light decisions on the Delver mirror (docs/ENGINE.md 77; zero-core docs/search.md 3.10, az.md 22): the
game's 22 window classes in its info, a Delver self-play driver searching its timing windows light
(every light decision a window of the classes asked for, by the move log and the engine's own
classification of each rebuilt decision; never recorded unless asked; fewer leaves), and the run
settings through `python -m mtgz.az.run` (refused where they would do nothing; on the Delver smoke
preset: the selfplay metrics' `light`, config.json)."""
import json
import os
import subprocess
import sys

import numpy as np

import mtgz.az as P
from mtgz.az import games as G
from mtgz.az.run import read_metrics
from zero import light as LT

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CLASSES = [f'own_{s}' for s in ('upkeep', 'draw', 'begin_combat', 'declare_attackers', 'declare_blockers',
                                'first_strike_damage', 'combat_damage', 'end_combat', 'end', 'cleanup')] + \
          [f'opp_{s}' for s in ('upkeep', 'draw', 'main1', 'begin_combat', 'declare_attackers', 'declare_blockers',
                                'first_strike_damage', 'combat_damage', 'end_combat', 'main2', 'end', 'cleanup')]


def uniform(lb):
    a = lb.a
    off = a['opt_off']
    b = len(a['player'])
    pri = np.concatenate([np.full(int(off[r + 1] - off[r]), 1.0 / max(1, int(off[r + 1] - off[r])), np.float32)
                          for r in range(b)]) if off[-1] else np.zeros(0, np.float32)
    v = np.full((b, 8), 0.0, np.float32)
    v[:, :2] = 0.5
    return pri, v, np.zeros((b, 8), np.float32)


def play(light, games=6, seed=3, sims=16):
    seat = G.nn_seat('m', sims, selfplay=True, sims_fast=sims // 4, p_full=0.25, temp_moves=60, sims_minor=2,
                     policy_target='cq', cq_c_scale=0.3, view_samples=0.5, **light)
    d = G.Driver('mtg', [[seat, seat]] * games, spec=P.SPEC_DELVER, concurrency=games, seed=seed, record=True, threads=2,
                 move_log=100_000)
    rows, recs = [], []
    while True:
        lb = d.next_batch()
        if lb is None:
            break
        d.submit(lb.id, *uniform(lb))
        rows += d.take_results()
        s = d.take_samples()
        if s is not None:
            recs.append(s)
    rows += d.take_results()
    s = d.take_samples()
    if s is not None:
        recs.append(s)
    return sorted(rows, key=lambda r: r['gid']), recs, d.stats()


M64 = (1 << 64) - 1


def split64(x):
    z = (x + 0x9E3779B97F4A7C15) & M64
    z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & M64
    z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & M64
    return z ^ (z >> 31)


def game_seed(driver_seed, gid):
    """The game seed of game `gid` of a driver with seed `driver_seed` (zero-core's init_slot: the first
    draw of Rng(split64(seed ^ split64(gid + 1))), xoshiro256++ seeded by splitmix64)."""
    x, s = split64(driver_seed ^ split64(gid + 1)), []
    for _ in range(4):
        x = (x + 0x9E3779B97F4A7C15) & M64
        s.append(split64(x))
    if not any(s):
        s[0] = 1
    r = (s[0] + s[3]) & M64
    return ((((r << 23) | (r >> 41)) & M64) + s[0]) & M64


def classes_of_moves(row, seed):
    """Each searched decision of a game (its move log) on the game rebuilt with the engine: its light class
    by the engine's own reading of the position (`Game.light_class`)."""
    g = G.engine().Game('mtg', spec=P.SPEC_DELVER, players=2, seed=game_seed(seed, row['gid']))
    out = []
    for seat, a, legal in row['moves']:
        assert g.to_move == seat and list(g.legal()) == list(legal), row['gid']
        if len(legal) >= 2:
            out.append(g.light_class())
        g.step(a)
    assert g.over
    return out


def test_the_classes_in_the_game_info():
    assert G.info('mtg')['light_classes'] == CLASSES
    assert LT.select_classes('all,-opp_end', CLASSES) == [i for i, c in enumerate(CLASSES) if c != 'opp_end']
    assert [CLASSES[i] for i in LT.select_classes('opp_*', CLASSES)] == CLASSES[10:]
    assert G.info('team_draft')['light_classes'] == []


def test_a_delver_driver_searching_its_windows_light():
    classes = list(range(len(CLASSES)))
    on, recs_on, st_on = play({'sims_light': 2, 'light_classes': classes})
    off, recs_off, st_off = play({})
    assert all('light' not in r for r in off) and st_off['light'] == 0
    n_light = sum(r['light']['decisions'] for r in on)
    searched = sum(r['decisions'] for r in on)
    assert n_light == st_on['light'] > 0 and 0.05 < n_light / searched < 0.8, (n_light, searched)
    assert sum(r['light']['recorded'] for r in on) == 0 and st_on['light_recorded'] == 0
    # fewer leaves per searched decision (the games themselves differ: other moves at the windows)
    assert st_on['leaves'] / searched < st_off['leaves'] / sum(r['decisions'] for r in off)
    assert sum(r['light']['saved'] for r in on) == st_on['light_saved'] > 0
    # every window of a game (the engine's own reading of each rebuilt decision) is a light decision
    # (all classes asked for) and nothing else is: count them per game against the rows
    by = {}
    for r in on:
        cl = classes_of_moves(r, 3)
        assert r['light']['decisions'] == sum(c is not None for c in cl) and r['decisions'] == len(cl), r['gid']
        for c in cl:
            by[c] = by.get(c, 0) + 1
    assert by.get(None) and any(k and k.startswith('own_') for k in by) and any(k and k.startswith('opp_') for k in by)
    # recorded light decisions: some full draws are recorded
    rec, _, st = play({'sims_light': 8, 'sims_light_fast': 2, 'light_record': True, 'light_classes': classes})
    assert 0 < st['light_recorded'] < st['light']
    # only the opponent's turn
    opp, _, _ = play({'sims_light': 2, 'light_classes': [i for i, c in enumerate(CLASSES) if c.startswith('opp_')]})
    assert 0 < sum(r['light']['decisions'] for r in opp) < n_light
    for r in opp:
        cl = classes_of_moves(r, 3)
        assert r['light']['decisions'] == sum(c is not None and c.startswith('opp_') for c in cl), r['gid']


def test_run_settings_through_mtgz_run(tmp_path):
    def run(name, *extra):
        e = dict(os.environ, PYTHONIOENCODING='utf-8', CUDA_VISIBLE_DEVICES='-1')
        return subprocess.run([sys.executable, '-m', 'mtgz.az.run', '--run', str(tmp_path / name), '--preset', 'delver-smoke',
                               '--iters', '1', '--games', '8', '--conc', '8', '--sims', '8', '--sims-fast', '4', *extra],
                              cwd=ROOT, capture_output=True, text=True, timeout=900, env=e)
    p = run('bad', '--selfplay-sims-light', '2', '--selfplay-light-classes', 'own_main1')
    assert p.returncode != 0 and "'own_main1' matches none of the game's light classes" in p.stdout + p.stderr
    p = run('noop', '--selfplay-light-record', '1')
    assert p.returncode != 0 and 'without selfplay_sims_light' in p.stdout + p.stderr
    p = run('x', '--selfplay-sims-light', '2', '--selfplay-light-classes', 'all,-opp_end')
    assert p.returncode == 0, p.stdout[-3000:] + p.stderr[-3000:]
    m = [r for r in read_metrics(str(tmp_path / 'x')) if 'selfplay' in r][0]['selfplay']
    lt = m['light']
    assert lt['games'] == m['games'] == 8 and lt['per_game'] > 0 and lt['recorded_per_game'] == 0
    assert 0 < lt['saved_share'] < 1
    cfg = json.load(open(tmp_path / 'x' / 'config.json'))
    assert (cfg['selfplay_sims_light'], cfg['selfplay_light_classes']) == (2, 'all,-opp_end')
