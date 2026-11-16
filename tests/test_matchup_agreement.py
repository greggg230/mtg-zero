"""tools/matchup_agreement.py and tools/delverai_matchup.py (docs/ENGINE.md 168.7): the network's agreement with the
matchup model's choice table, and the site's record.

* the canonical sideboarding path (36.2, `sbtable::next_action`) reaches every legal 60 of a small 75 exactly once;
* the best-first mode is the exact highest-prior 60 (brute force on a made-up tree), and a search past its budget is
  reported unresolved, never guessed;
* end to end on the engine with a tiny untrained network: a real game log (post-sideboard matches played at random), the
  probe set (game-2 starts and game-3 positions after game 2 played by the policy), rebuilt exactly; a checkpoint's row
  (against the newest, the live and the first table), its path prior checked against the engine stepped by hand, keep =
  main in game 2;
* the record: the versions, the agreement (against the live table, the first before it), the reachable contexts of the
  newest table, matches per hour (prorated over the trailing hour of run time, stops from the supervisor log as gaps,
  the farm summed without machine ids).
"""
import itertools
import json
import math
import os
import random
import sys

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'tools'))

import matchup_agreement as M  # noqa: E402
import delverai_matchup as DM  # noqa: E402


# ----------------------------------------------------------------------------- the canonical path

def test_the_canonical_path_reaches_every_legal_60_exactly_once():
    copies = [4, 1, 2, 3, 1]
    start = [3, 1, 0, 1, 1]
    size = sum(start)
    targets = [list(t) for t in itertools.product(*[range(c + 1) for c in copies]) if sum(t) == size]
    paths = {}
    for t in targets:
        pa = M.path(copies, start, t)
        assert pa[-1] == M.PASS and M.PASS not in pa[:-1]
        assert M.deck_after(copies, start, pa) == t
        paths[tuple(pa)] = t
    assert len(paths) == len(targets), 'one path per 60'
    keys = list(paths)
    for a in keys:                                 # (no path is a prefix of another: PASS ends each)
        for b in keys:
            assert a == b or a != b[:len(a)]
    assert M.path(copies, start, start) == [M.PASS]
    # a one-copy entry toggles (one answer), a several-copy entry asks its count (two)
    assert M.path(copies, start, [4, 0, 0, 1, 1]) == [M.LIBRARY + 0, M.NUMBER + 4, M.LIBRARY + 1, M.PASS]
    with pytest.raises(M.AgreementError):
        M.path(copies, start, [5, 1, 0, 0, 0])


# ----------------------------------------------------------------------------- the exact mode

class FakeGame:
    """A sideboarding as the engine asks it (counts, last, asking), on a small 75 of deck size `size`."""

    def __init__(self, copies, counts, size, last=None, asking=None, done=False):
        self.copies, self.counts, self.size, self.last, self.asking, self.done = copies, list(counts), size, last, asking, done
        self.to_move = 0

    def clone(self):
        return FakeGame(self.copies, self.counts, self.size, self.last, self.asking, self.done)

    def step(self, a):
        assert a in self.legal()
        self.counts, self.last, self.asking, self.done = M.apply(self.copies, self.counts, self.last, self.asking, a)

    def legal(self):
        if self.asking is not None:
            k = self.asking
            return [M.NUMBER + x for x in range(self.copies[k] + 1) if x != self.counts[k]]
        out = [M.PASS] if sum(self.counts) == self.size else []
        frm = 0 if self.last is None else self.last + 1
        return out + [M.LIBRARY + k for k in range(frm, len(self.copies))]


class FakeNet:
    def __init__(self, seed):
        self.seed, self.n = seed, 0

    def prior_of(self, g):
        acts = g.legal()
        w = [math.exp(3 * random.Random(f'{self.seed}-{g.counts}-{g.last}-{g.asking}-{a}').random()) for a in acts]
        return {a: x / sum(w) for a, x in zip(acts, w)}

    def priors(self, games):
        self.n += len(games)
        return [self.prior_of(g) for g in games]


def brute_mode(net, g, p=1.0):
    best = (None, 0.0)
    for a, q in net.prior_of(g).items():
        if a == M.PASS:
            cand = (list(g.counts), p * q)
        else:
            h = g.clone()
            h.step(a)
            cand = brute_mode(net, h, p * q)
        if cand[0] is not None and cand[1] > best[1]:
            best = cand
    return best


@pytest.mark.parametrize('seed', range(6))
def test_the_best_first_mode_is_the_exact_highest_prior_60(seed):
    copies = [2, 1, 2, 1]
    start = [2, 1, 0, 0]
    trees = [M.Tree(FakeGame(copies, start, 3), copies, start)]
    net = FakeNet(seed)
    got = M.modes(net, trees, budget=10_000)[0]
    want = brute_mode(net, FakeGame(copies, start, 3))
    assert got[0] == want[0] and got[1] == pytest.approx(want[1])
    # the path prior of that 60 is the same product
    assert M.path_priors(net, trees, [[got[0]]])[0][0] == pytest.approx(want[1])
    # a search past its budget is unresolved, never guessed
    flat = FakeNet(seed)
    def deep(g):                                   # (PASS unlikely everywhere: the mode lies deep in the tree)
        w = {a: (0.001 if a == M.PASS else 1.0) for a in g.legal()}
        return {a: x / sum(w.values()) for a, x in w.items()}
    flat.prior_of = deep
    assert M.modes(flat, [M.Tree(FakeGame(copies, start, 3), copies, start)], budget=2)[0] is None


# ----------------------------------------------------------------------------- end to end on the engine

def _engine():
    os.environ.setdefault('CUDA_VISIBLE_DEVICES', '-1')
    import mtgz.az  # noqa: F401
    from zero.games import engine
    return engine()


def _tiny_run(tmp_path, matches=36):
    """A run directory: a post-sideboard spec with per-seat dealing (p ~ 0: every seat chooses), two tiny checkpoints,
    a real game log of random matches, iteration times, a table version that the second iteration played."""
    import torch
    import mtgz.az as A
    from mtgz import _engine as engine_mod
    from mtgz.az import decklists
    from mtgz.az import games as G
    from zero.net import make_net, save
    E = _engine()
    run = tmp_path / 'delver-bo3-tiny'
    for d in ('models', 'games', 'matchup'):
        (run / d).mkdir(parents=True)
    sp = json.loads(decklists.mirror_bo3_spec(max_turns=12, game_episodes=True, start_game=2))
    sp['dealt_decks'] = {'p': 1e-12, 'swaps': [0, 15], 'per_seat': True}
    spec = json.dumps(sp)
    (run / 'config.json').write_text(json.dumps({'game': 'mtg', 'spec': spec, 'matchup_table_min_gain': 0.005}), encoding='utf-8')
    torch.manual_seed(0)
    for k in (0, 1):
        m = make_net(G.info('mtg'), 'mtg', 'tokens', 16, 1, heads=2, aux=A.PACKAGE.aux_of('mtg'))
        save(m, str(run / 'models' / f'iter_{k:04d}.pt'), {'iter': k})
    rng = random.Random(5)
    engine_mod.game_log(str(run / 'games' / 'games-1.jsonl'))
    try:
        for s in range(matches):
            g = E.Game('mtg', spec, 2, 1000 + s)
            while True:
                if g.to_move is None:
                    g, _ = g.next_episode()
                    if g is None:
                        break
                    continue
                g.step(rng.choice(list(g.legal())))
    finally:
        engine_mod.game_log(None)
    lines = [json.loads(x) for x in (run / 'games' / 'games-1.jsonl').read_text(encoding='utf-8').splitlines()]
    t_end = max(x['time'] for x in lines if 'time' in x) + 1
    (run / 'metrics.jsonl').write_text(json.dumps({'iter': 1, 'time': t_end, 'seconds': 10.0}) + '\n', encoding='utf-8')
    spec_line = next(x for x in lines if x.get('kind') == 'bo3-spec')
    names = [n for n, _ in spec_line['lists'][0]]
    copies = [c for _, c in spec_line['lists'][0]]
    main = spec_line['mains'][0]
    # the table: +1 of the first card the main lacks, -1 of the first it holds four of (every context)
    tgt = list(main)
    tgt[next(i for i in range(len(main)) if main[i] < copies[i] and main[i] == 0)] += 1
    tgt[next(i for i in range(len(main)) if main[i] == 4)] -= 1
    from mtgz.az import matchup as MU
    table = {'version': 'v0001', 'list': [[n, c] for n, c in zip(names, copies)],
             'choices': [[g_, f, l, o, tgt] for (g_, f, l, o) in MU.contexts()]}
    MU.check_table(spec, table)
    (run / 'matchup' / 'v0001.json').write_text(json.dumps({'version': 'v0001', 'iter': 1, 'table': table, 'table_detail': [
        {'context': list(c), 'deck': tgt, 'in': {names[i]: tgt[i] - main[i] for i in range(len(main)) if tgt[i] > main[i]},
         'out': {names[i]: main[i] - tgt[i] for i in range(len(main)) if tgt[i] < main[i]}} for c in MU.contexts()]}), encoding='utf-8')
    (run / 'matchup' / 'used.jsonl').write_text(json.dumps({'iter': 0, 'version': None}) + '\n'
                                                + json.dumps({'iter': 1, 'version': 'v0001.json', 'table': True}) + '\n', encoding='utf-8')
    (run / 'matchup' / 'index.jsonl').write_text(json.dumps({'version': 'v0001', 'iter': 1, 'window': [1, 1], 'games': 50, 'val_games': 10,
                                                             'val_logloss': 0.68, 'val_logloss_pd': 0.69, 'gain_nats': 0.01,
                                                             'table': True}) + '\n', encoding='utf-8')
    return str(run), E, names, copies, main, tgt


def test_probe_and_measure_end_to_end_with_a_tiny_network(tmp_path):
    run, E, names, copies, main, tgt = _tiny_run(tmp_path)
    probe = M.make_probe(run, per_context=3, seed=7, policy_iter=0, log=lambda m: None, E=E)
    pos = probe['positions']
    assert pos and {p['game'] for p in pos} == {2, 3}, 'game-2 starts and game-3 positions'
    assert probe['list'] == [[n, c] for n, c in zip(names, copies)] and probe['main'] == main
    assert all(v <= 3 for v in probe['contexts'].values())
    path = tmp_path / 'probe.json'
    path.write_text(json.dumps(probe), encoding='utf-8')
    probe = M.load_probe(str(path), run)
    spec = M.run_spec(run)
    games = [M.replay(E, spec, p) for p in pos]
    for g, p in zip(games, pos):
        assert g.to_move == p['seat']
        M.check_counts(g, names, copies, p['start'])
        if p['game'] == 2:
            assert p['start'] == main
    # a probe made twice is the same probe
    assert M.make_probe(run, per_context=3, seed=7, policy_iter=0, log=lambda m: None, E=E)['id'] == probe['id']

    rows = M.measure_missing(run, probe, E, games, log=lambda m: None, settle=0)
    assert [r['iter'] for r in rows] == [0, 1]
    r0, r1 = rows
    assert r0['tables'] == {'newest': 'v0001', 'live': None, 'first': 'v0001'}
    assert r1['tables'] == {'newest': 'v0001', 'live': 'v0001', 'first': 'v0001'}
    assert r1['n'] == len(pos) and set(r1['all']['vs']) == {'v0001'}
    for r in rows:
        a = r['all']
        for k in ('keep', 'main', 'rough'):
            assert 0.0 <= a[k] <= 1.0
        assert 0.0 <= a['vs']['v0001']['prior'] <= 1.0
        assert a['unresolved'] + a['vs']['v0001']['resolved'] == r['n']
        g2 = [c for k, c in r['contexts'].items() if k.startswith('2,')]
        assert all(c['keep'] == pytest.approx(c['main']) for c in g2), 'in game 2 keeping is the main'
    # the table's prior at one position by hand: the engine stepped along the canonical path, the net's prior each step
    net = M.Net(M.load_net(dict(M.checkpoints(run))[1]))
    g = games[0].clone()
    p = 1.0
    for a in M.path(copies, pos[0]['start'], tgt):
        p *= net.priors([g])[0][a]
        g.step(a)
    tree = [M.Tree(games[0].clone(), copies, pos[0]['start'])]
    assert M.path_priors(net, tree, [[tgt]])[0][0] == pytest.approx(p, rel=1e-5)
    # measured once: nothing more to do, and the file holds one row per checkpoint
    assert M.measure_missing(run, probe, E, games, log=lambda m: None, settle=0) == []
    assert [x['iter'] for x in M.rows(M.agreement_path(run))] == [0, 1]

    rec = DM.record(run)
    assert rec['run'] == 'matchup.delver-bo3-tiny' and rec['kind'] == 'matchup' and rec['table_from'] == 1
    assert [a['ref'] for a in rec['agreement']] == ['v0001', 'v0001'] and rec['agreement'][0]['live'] is None
    assert rec['table']['version'] == 'v0001' and len(rec['table']['contexts']) == 8, 'the reachable contexts only'
    assert rec['versions'][0]['gain'] == 0.01
    json.dumps(rec)


# ----------------------------------------------------------------------------- matches per hour

def _metrics_line(it, t, seconds, desk, mac, farm, eps):
    return {'iter': it, 'time': t, 'seconds': seconds, 'selfplay': {
        'games': desk, 'episodes': {'games': eps, 'contests': desk},
        'resign': {'remote': {'games': int(1.5 * (mac + farm))}},
        'remote': {'games': mac + farm, 'pools': {'pool:x': {'games': farm, 'machines': {'m_abcdefghijklmnop': farm}}}}}}


def test_matches_per_hour_over_the_trailing_hour_of_run_time(tmp_path):
    import time as T
    run = tmp_path / 'r'
    run.mkdir()
    t0 = T.mktime(T.strptime('2026-11-15 20:00:00', '%Y-%m-%d %H:%M:%S'))
    # 20:00-20:30 three 10-minute iterations (600 matches each: 400 desktop, 150 Mac, 50 farm), a 30-minute stop, then
    # 21:00-21:40 four more of 1,200 (the farm grew)
    lines = [_metrics_line(i + 1, t0 + 600 * (i + 1), 600, 400, 150, 50, 600) for i in range(3)]
    lines += [_metrics_line(4 + i, t0 + 3600 + 600 * (i + 1), 600, 700, 200, 300, 1200) for i in range(4)]
    lines.insert(0, {'iter': 0, 'time': t0, 'seconds': 0.0})
    (run / 'metrics.jsonl').write_text(''.join(json.dumps(x) + '\n' for x in lines), encoding='utf-8')
    (run / 'supervisor.log').write_text('2026-11-15 20:30:05 mtgz.az.run exited with code 75; killed 0 orphaned workers\n'
                                        '2026-11-15 20:59:55 started mtgz.az.run pid 1\n', encoding='utf-8')
    r = DM.rate_block(str(run))
    assert r['unit'] == 'matches' and r['columns'] == ['t', 'total', 'desktop', 'mac', 'farm', 'games']
    pts = {p[0]: p[1:] for p in r['points']}
    assert pts[t0 + 600] == [3600.0, 2400.0, 900.0, 300.0, 3600 * 1.0 + 1.5 * 200 * 6]   # the first iteration's rate
    assert pts[t0 + 1800][0] == pytest.approx(3600.0)
    # 21:20: the trailing hour holds 20 min of the new pace and 10 min of the old (the stop is no run time)
    assert pts[t0 + 3600 + 1200][0] == pytest.approx((2 * 1200 + 600) / 1800 * 3600)
    # 21:40: a whole hour of run time back at the new pace
    assert pts[t0 + 3600 + 2400][:4] == pytest.approx([7200.0, 4200.0, 1200.0, 1800.0])
    assert r['gaps'] == [[round(t0 + 1800), round(t0 + 3600)]], 'the stop and the missing run time, merged'
    assert 'm_abcdefghijklmnop' not in json.dumps(r), 'never a farm machine id'
