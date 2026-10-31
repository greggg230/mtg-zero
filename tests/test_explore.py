"""Self-play exploration on the Delver mirror (docs/ENGINE.md 75; zero-core docs/search.md 3.9, az.md 21):
the game's six habit tags in its info, a Delver self-play driver exploring them (episodes, masked rows,
the games up to their deviation those of the driver without exploration), the run settings through
`python -m mtgz.az.run` (refused for a name the game does not declare; on the Delver smoke preset:
the selfplay metrics' `explore`, the shards' masked rows) and the evidence tool's games rebuilt from
their move logs (tools/explore_eval.py)."""
import importlib.util
import json
import os
import subprocess
import sys

import numpy as np

import mtgz.az as P
from mtgz.az import games as G
from mtgz.az.run import read_metrics

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TAGS = ['fetch_crack', 'daze_tapout', 'fow_pitch_fow', 'delve_skip_spell', 'volcanic_fetch', 'keep_one_lander']


def tool():
    spec = importlib.util.spec_from_file_location('explore_eval', os.path.join(ROOT, 'tools', 'explore_eval.py'))
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


def uniform(lb):
    a = lb.a
    off = a['opt_off']
    b = len(a['player'])
    pri = np.concatenate([np.full(int(off[r + 1] - off[r]), 1.0 / max(1, int(off[r + 1] - off[r])), np.float32)
                          for r in range(b)]) if off[-1] else np.zeros(0, np.float32)
    v = np.full((b, 8), 0.0, np.float32)
    v[:, :2] = 0.5
    return pri, v, np.zeros((b, 8), np.float32)


def play(explore, games=8, seed=3):
    seat = G.nn_seat('m', 8, selfplay=True, sims_fast=4, p_full=0.25, temp_moves=60, sims_minor=2, policy_target='cq',
                     cq_c_scale=0.3, view_samples=0.5)
    d = G.Driver('mtg', [[seat, seat]] * games, spec=P.SPEC_DELVER, concurrency=games, seed=seed, record=True, threads=2,
                 move_log=100_000, **({'explore': explore} if explore else {}))
    rows, masked = [], 0
    while True:
        lb = d.next_batch()
        if lb is None:
            break
        d.submit(lb.id, *uniform(lb))
        rows += d.take_results()
        s = d.take_samples()
        if s is not None and 'masked' in s:
            masked += int(s['masked'].sum())
    rows += d.take_results()
    return sorted(rows, key=lambda r: r['gid']), masked


def test_the_tags_and_a_delver_driver_exploring_them():
    assert [t[0] for t in G.info('mtg')['explore_tags']] == TAGS
    x = {'games': 1.0, 'q_margin': 1.0, 'mask_z': True, 'tags': [{'tag': t} for t in TAGS]}
    on, masked = play(x)
    off, _ = play(None)
    eps = [(r, r['explore']['episode']) for r in on if r['explore'].get('episode')]
    assert len(eps) >= 4 and masked == sum(r['explore']['masked'] for r in on) > 0
    for r, ep in eps:
        tw = off[r['gid']]
        k = ep['step']
        assert r['moves'][:k] == tw['moves'][:k] and r['moves'][k][1] != tw['moves'][k][1], r['gid']
    # the evidence tool rebuilds each game from its move log (the engine's view at every step)
    T = tool()
    for r in on[:3]:
        rep = T.rebuild(P.SPEC_DELVER, 3, r)
        assert len(rep['states']) == len(rep['frames']) + 1 and rep['states'][-1].get('decision') in (None, {})


def test_run_settings_through_mtgz_run(tmp_path):
    def run(name, *extra):
        e = dict(os.environ, PYTHONIOENCODING='utf-8', CUDA_VISIBLE_DEVICES='-1')
        return subprocess.run([sys.executable, '-m', 'mtgz.az.run', '--run', str(tmp_path / name), '--preset', 'delver-smoke',
                               '--iters', '1', '--games', '8', '--conc', '8', '--sims', '8', '--sims-fast', '4', *extra],
                              cwd=ROOT, capture_output=True, text=True, timeout=900, env=e)
    p = run('bad', '--selfplay-explore', 'fetch_hold')
    assert p.returncode != 0 and "declares no habit tag ['fetch_hold']" in p.stdout + p.stderr
    p = run('x', '--selfplay-explore', 'all', '--selfplay-explore-games', '1', '--selfplay-explore-p', 'fetch_crack=1')
    assert p.returncode == 0, p.stdout[-3000:] + p.stderr[-3000:]
    m = [r for r in read_metrics(str(tmp_path / 'x')) if 'selfplay' in r][0]['selfplay']
    e = m['explore']
    assert e['games'] == m['games'] == 8 and e['eligible'] == 8 and set(e['tags']) == set(TAGS)
    assert e['episodes'] > 0
    cfg = json.load(open(tmp_path / 'x' / 'config.json'))
    assert cfg['selfplay_explore'] == 'all' and cfg['selfplay_explore_p'] == 'fetch_crack=1'
