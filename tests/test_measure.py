"""Strength measurement on the Delver mirror (docs/ENGINE.md 73; zero-core docs/az.md 20): the driver's
paired games deal the same game twice from the MTG engine's seed, the held-out shim is the core's, and
tools/delver_fingerprints.py reads a recorded replay of the mirror."""
import gzip
import importlib.util
import json
import os

import mtgz.az as P
from mtgz.az import games as G
from mtgz.az.infer import drive

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SPEC = P.SPEC_DELVER


def play(seats, paired, seed=3):
    kw = {'paired': True} if paired else {}
    drv = G.Driver('mtg', seats, spec=SPEC, concurrency=len(seats), threads=2, seed=seed, max_steps=20000, move_log=4000,
                   **kw)
    drive(drv, {})
    return sorted(drv.take_results(), key=lambda r: r['gid'])


def test_paired_mirror_games_share_the_deal():
    """Paired (zero-core's DriverCfg::paired): games 2k and 2k + 1 draw the MTG game's seed and every
    driver stream from k - with the same player in both seats the pair is one game twice; unpaired, the
    two games are unrelated. A heuristic against random, seats swapped, still plays each pair's deal."""
    h = G.seat_dict('heuristic')
    key = lambda r: (r['steps'], r['scores'], r['moves'])                    # noqa: E731
    p = play([[h, h]] * 8, True)
    assert all(key(p[2 * k]) == key(p[2 * k + 1]) for k in range(4))
    u = play([[h, h]] * 8, False)
    assert any(key(u[2 * k]) != key(u[2 * k + 1]) for k in range(4))
    r = G.seat_dict('random')
    m = play([[h, r], [r, h]] * 4, True)
    assert len(m) == 8 and all(x['over'] for x in m)


def test_heldout_shim_is_the_core_module():
    import zero.heldout
    from mtgz.az import heldout
    assert heldout is zero.heldout


def test_fingerprints_read_a_recorded_replay(tmp_path):
    import torch
    from zero import replay
    from zero.net import make_net, save
    from mtgz.az.replays import spec_with_log
    torch.manual_seed(0)
    m = make_net(G.info('mtg'), 'mtg', 'tokens', 16, 1, heads=2)
    run = tmp_path / 'fp-run'
    os.makedirs(run / 'models')
    ck = str(run / 'models' / 'iter_0007.pt')
    save(m, ck, {'iter': 7})
    # (against the random bot: a tiny random network alone only passes priority - nothing to count)
    reps = replay.record(ck, opp='random', games=6, sims=2, spec=spec_with_log(SPEC), chance_k=0, seed=4,
                         title='Generation 7', log=lambda *a: None)
    for rep in reps:
        replay.save(rep, str(run / 'replays'))
    spec = importlib.util.spec_from_file_location('delver_fingerprints', os.path.join(ROOT, 'tools', 'delver_fingerprints.py'))
    fp = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(fp)
    bands = fp.collect([str(run)], 20, 0, 10 ** 9)
    assert list(bands) == [('fp-run', 0)]
    B = bands[('fp-run', 0)]
    assert B.games == 6 and (B.lo, B.hi) == (7, 7)
    # every opening seven is a mulligan decision of the table (both seats, every game)
    assert sum(v[1] for v in B.mull.values()) == 12
    # the random bot casts and fetches: the counters are filled and consistent
    c = B.c
    assert c['fetch_cracked'] > 0 and c['bs'] > 0 and c['murk'] > 0 and c['decided'] == 6
    assert c['fetch_now'] + c['fetch_later this turn'] + c['fetch_next turn'] + c['fetch_2+ turns'] == c['fetch_cracked']
    assert c['bs_resp'] <= c['bs_shuffle'] <= c['bs'] and c['murk_best'] <= c['murk'] and c['daze_tapout'] <= c['daze_live']
    txt = fp.table(bands)
    for label, _, _ in fp.METRICS:
        assert label in txt
    out = tmp_path / 'fp.json'
    assert fp.main([str(run), '--json', str(out)]) is None
    j = json.load(open(out))
    assert j[0]['games'] == 6 and set(j[0]['rates']) == {m[0] for m in fp.METRICS}
    # a replay is gzipped zero-replay/1 with the engine's log (the tool reads the log)
    with gzip.open(next((run / 'replays').glob('*.json.gz')), 'rt', encoding='utf-8') as f:
        d = json.load(f)
    assert d['format'] == 'zero-replay/1' and any(s.get('log') for s in d['states'])
