"""Batched paired races (mtgz/sealed/batch.py, docs/ENGINE.md 110): the row-exact evaluator's answers
depend on each row alone; a batched race is the per-process race game for game (the heuristic; a tiny
token network with the row-exact evaluator, and with the usual one evaluated batch by batch); the
pilot's simulation mix; the screening stage's error budget; the CLI. Needs `mtgz._engine`
(tools/build_py.sh)."""
import json
import os

import numpy as np
import pytest

from mtgz.sealed import batch as BT
from mtgz.sealed import compare as K
from mtgz.sealed import engine as R


@pytest.fixture(scope='module')
def builds():
    """Three builder builds of one arena pool (supported on this base) and the pool."""
    for ps in range(3, 50):
        p = R.pool(ps, collation='arena')
        if all(isinstance(v, int) for v in R.lookup(p['cards']).values()):
            break
    bs = [K.builder_build(p['cards'], s, name=f'b{s}') for s in (1, 2, 3)]
    return p, bs


@pytest.fixture(scope='module')
def net(tmp_path_factory):
    """A tiny random token network as runs/<run>/models/iter_0000.pt, with a run config (the mix: 4 / 2 / 1)."""
    import torch
    from zero.net import make_net, save
    from mtgz.az import games as G
    d = tmp_path_factory.mktemp('run')
    os.makedirs(d / 'models')
    torch.manual_seed(0)
    ck = str(d / 'models' / 'iter_0000.pt')
    save(make_net(G.info('mtg_eoe'), 'mtg_eoe', 'tokens', 16, 1, heads=2), ck)
    (d / 'config.json').write_text(json.dumps({'sims': 4, 'sims_fast': 2, 'p_full': 0.25, 'sims_minor': 1}),
                                   encoding='utf-8')
    return ck


def _scores(comp):
    return [dict(s) for s in comp.scores]


def test_rowexact_answers_depend_on_the_row_alone(builds, net):
    """Real leaf batches of a comparison: RowExact's answers for a batch alone, merged with the others (any
    order) and in dense or sparse form are the same bits; the usual forward's are within float rounding."""
    from zero.infer import NetEvaluator
    from zero.protocol import LeafBatch, densify
    p, bs = builds
    pilot = K.Pilot(net, sims=2)
    comp = K.Comparison(bs, K.Field(seed=1), pilot, seed=2, block=8, threads=1, max_turns=8)
    ex = BT.RowExact(pilot.model(), 'cpu', rows=4)
    usual = NetEvaluator(pilot.model(), 'cpu')
    got = []
    for i in range(3):
        drv = comp.driver(i, 0, list(range(8)), sparse=True)
        for _ in range(30):
            lb = drv.next_batch()
            if lb is None:
                break
            got.append({k: (np.array(v) if hasattr(v, '__len__') else v) for k, v in lb.a.items()})
            drv.submit(lb.id, *ex(lb))
        drv.end_phase()
    assert len(got) > 20

    def merged(order):
        md = BT.MultiDrive({'net': ex})
        lbs = [LeafBatch(k, 'net', 'nn', got[k]) for k in order]
        out = md._merged(ex, lbs)
        return {k: o for k, o in zip(order, out)}
    alone = {k: ex.evaluate_sparse(g['xs_off'], g['xs_idx'], g['xs_val'], g['n_features'], g['n_players'],
                                   g['opt_off'], g['opt_action']) for k, g in enumerate(got)}
    fwd = merged(list(range(len(got))))
    rev = merged(list(range(len(got)))[::-1])
    for k, g in enumerate(got):
        for x in range(3):
            assert np.array_equal(alone[k][x], fwd[k][x]) and np.array_equal(alone[k][x], rev[k][x])
        dense = ex.evaluate_dense(densify(g['xs_off'], g['xs_idx'], g['xs_val'], g['n_features']), g['n_players'],
                                  g['opt_off'], g['opt_action'])
        assert all(np.array_equal(alone[k][x], dense[x]) for x in range(3))
        u = usual.evaluate(densify(g['xs_off'], g['xs_idx'], g['xs_val'], g['n_features']), g['n_players'],
                           g['opt_off'], g['opt_action'])
        assert np.allclose(u[0], alone[k][0], atol=1e-5) and np.allclose(u[1], alone[k][1], atol=1e-5)


def test_a_batched_race_is_the_per_process_race(builds):
    """The heuristic pilot: a race and a fixed comparison played batched give the per-process result."""
    p, bs = builds
    kw = dict(seed=3, block=16, threads=1)
    a = K.Comparison(bs, K.Field(seed=2), K.Pilot('heuristic'), **kw)
    want = K.race(a, delta=0.05, round_games=32, max_games=128, min_games=32)
    b = K.Comparison(bs, K.Field(seed=2), K.Pilot('heuristic'), **kw)
    st = K.RaceState(b, delta=0.05, round_games=32, max_games=128, min_games=32)
    outs, stats = BT.run_races([('x', b, st)], {}, threads=1)
    got = outs['x']
    assert _scores(a) == _scores(b)
    for k in ('winner', 'stop', 'games', 'survivors', 'candidates', 'vs_winner', 'test', 'game_scores'):
        assert got[k] == want[k], k
    assert [dict(r, seconds=0) for r in got['rounds']] == [dict(r, seconds=0) for r in want['rounds']]
    assert stats['games'] == sum(c['games'] for c in want['candidates'])
    # a fixed number of games
    c = K.Comparison(bs[:2], K.Field(seed=2), K.Pilot('heuristic'), **kw)
    want = K.compare(c, 48)
    d = K.Comparison(bs[:2], K.Field(seed=2), K.Pilot('heuristic'), **kw)
    got = BT.run_races([('y', d, K.FixedState(d, 48))], {}, threads=2)[0]['y']
    assert got['pairs'] == want['pairs'] and got['candidates'] == want['candidates']
    assert got['game_scores'] == want['game_scores'] and len(got['game_scores']['b1']) == 48


def test_a_network_race_batched_is_the_per_process_race(builds, net):
    """A tiny token network: two pools' comparisons raced at once in one batched driver are the per-process
    comparisons, game for game - with RowExact both ways, and with the usual evaluator per batch."""
    p, bs = builds
    q = R.pool(7, collation='arena')
    bq = [K.builder_build(q['cards'], s, name=f'q{s}') for s in (1, 2)]
    if not all(isinstance(v, int) for v in R.lookup(q['cards']).values()):
        bq = [dict(b, name='q' + b['name']) for b in bs[:2]]

    def comps(ev):
        pilot = K.Pilot(net, sims=3, evaluator=ev)
        return [K.Comparison(bs, K.Field(seed=4), pilot, seed=5, block=6, threads=2, max_turns=8),
                K.Comparison(bq, K.Field(seed=4), pilot, seed=6, block=6, threads=2, max_turns=8)]
    for ev, mode in (('rowexact', 'merged'), ('default', 'each')):
        serial = comps(ev)
        for c in serial:
            K.compare(c, 12)
        batched = comps(ev)
        evs = batched[0]._evaluators()
        outs, st = BT.run_races([(str(n), c, K.FixedState(c, 12)) for n, c in enumerate(batched)], evs, mode=mode,
                                threads=1)
        for s, b in zip(serial, batched):
            assert _scores(s) == _scores(b) and s.unfinished == b.unfinished, (ev, mode)
        assert st['games'] == 12 * 5 and st['rows'] > 0
        if mode == 'merged':
            assert st['rows_per_call'] > max(len(c.cands) for c in batched)   # batches of several drivers


def test_the_mix_pilot(builds, net):
    """--pilot-search mix: the run's sims / sims_fast / p_full / sims_minor on a self-play-flagged evaluation seat;
    without a fast budget it is the flat seat's games bit for bit (eps 0, no temperature, nothing recorded)."""
    p, bs = builds
    mix = K.Pilot(net, sims=4, search='mix')
    s = mix.seat()
    assert (s['sims'], s['sims_fast'], s['p_full'], s['sims_minor']) == (4, 2, 0.25, 1)
    assert s['selfplay'] and s['eps'] == 0.0 and s['temp_moves'] == 0 and s['record'] is False
    assert mix.describe()['search']['kind'] == 'mix' and K.Pilot(net, sims=4).describe()['search'] == 'flat'
    assert mix.seat('flat') == K.Pilot(net, sims=4).seat()

    def play(pilot):
        c = K.Comparison(bs[:2], K.Field(seed=8), pilot, seed=9, block=6, threads=1, max_turns=8)
        K.compare(c, 6)
        return _scores(c)
    flat = K.Pilot(net, sims=4)
    nofast = K.Pilot(net, sims=4, search='mix')
    nofast.mix = dict(nofast.mix, sims_fast=None, sims_minor=None)
    assert play(nofast) == play(flat)
    play(mix)                                                     # (the mix plays: smaller searches)
    with pytest.raises(ValueError):
        K.Pilot(net.replace('iter_0000.pt', 'nowhere/iter_0000.pt'), search='mix')


def test_the_screening_stage(builds):
    """The two stages: the alpha split and its bound in the output; a bad build dropped in the screening; without
    a screening delta the race of before."""
    import collections
    from mtgz.sealed import cards as C
    p, bs = builds
    good = bs[0]
    side = sorted(good['sideboard'], key=lambda n: -C.cards()[n]['mana_value'])
    lands = [j for j, n in enumerate(good['deck']) if n in C.BASICS][:8]
    deck = list(good['deck'])
    for j, n in zip(lands, side):
        deck[j] = n
    bad = {'name': 'bad', 'deck': deck, 'sideboard': []}
    twin = dict(good, name='twin')
    comp = K.Comparison([good, twin, bad], K.Field(seed=4), K.Pilot('heuristic'), seed=1, block=64, threads=2)
    out = K.race(comp, delta=0.03, round_games=64, max_games=1024, min_games=64, screen_delta=0.05)
    t = out['test']
    assert t['screen']['alpha'] == pytest.approx(0.025) and t['final_alpha'] == pytest.approx(0.025)
    assert t['screen']['alpha_per_test'] == pytest.approx(0.0125) and t['alpha_per_test'] == pytest.approx(0.0125)
    assert t['screen']['upper'] > K.sprt_bounds(0.025, 0.05)[1]       # stricter than one stage's alpha / (N - 1)
    by = {c['name']: c for c in out['candidates']}
    assert by['bad']['dropped']['stage'] == 'screen'
    assert t['screen']['ended'] is not None and set(t['screen']['ended']['survivors']) == {'b1', 'twin'}
    assert {r['stage'] for r in out['rounds']} <= {'screen', 'final'} and out['rounds'][-1]['stage'] == 'final'
    assert collections.Counter(out['survivors']) == collections.Counter(['b1', 'twin'])
    # the screening's own first test (--screen-min-games), its stages' minimums in the output
    st = K.RaceState(comp, delta=0.03, min_games=256, screen_delta=0.05, screen_min_games=64, screen_alpha=0.01)
    assert [s['min_games'] for s in st.stages] == [64, 256] and st.stages[1]['alpha'] == pytest.approx(0.04)
    with pytest.raises(ValueError):
        K.RaceState(comp, delta=0.05, screen_delta=0.03)
    with pytest.raises(ValueError):
        K.RaceState(comp, alpha=0.05, screen_delta=0.06, screen_alpha=0.05)
    # no screening: the one-stage race's output keys, as before
    comp = K.Comparison([good, bad], K.Field(seed=4), K.Pilot('heuristic'), seed=1, block=64, threads=2)
    out = K.race(comp, delta=0.05, round_games=64, max_games=2048, min_games=64)
    assert 'screen' not in out['test'] and 'stage' not in out['rounds'][0]
    assert out['test']['alpha_per_test'] == pytest.approx(0.05)


def test_the_cli_batched(builds, net, tmp_path):
    p, bs = builds
    paths = []
    for b in bs:
        path = tmp_path / f'{b["name"]}.json'
        path.write_text(json.dumps(b), encoding='utf-8')
        paths.append(str(path))
    out = tmp_path / 'out.json'
    args = ['race', *paths, '--race', '--games', '16', '--block', '16', '--max-games', '32', '--min-games', '16',
            '--quiet', '--json', str(out), '--pilot', net, '--sims', '2', '--max-turns', '6']
    assert K.main(args + ['--eval', 'rowexact']) == 0
    want = json.loads(out.read_text(encoding='utf-8'))
    assert K.main(args + ['--batched', '--progress', '0']) == 0
    got = json.loads(out.read_text(encoding='utf-8'))
    assert got['batched']['evaluator'] == 'rowexact' and got['setup']['pilot']['evaluator']['evaluator'] == 'rowexact'
    assert [c['score'] for c in got['candidates']] == [c['score'] for c in want['candidates']]
    assert got['vs_winner'] == want['vs_winner'] and got['game_scores'] == want['game_scores']
    with pytest.raises(SystemExit):
        K.main(args + ['--eval', 'batchwise'])                      # a mode of --batched only


def test_progress_lines(builds, net):
    """A per-process race's progress lines (the candidate, its block's games, the evaluator's rows) and the batched
    driver's (games, rows, each pool's stage and survivors)."""
    p, bs = builds
    got = []
    c = K.Comparison(bs[:2], K.Field(seed=8), K.Pilot(net, sims=2), seed=9, block=4, threads=1, max_turns=6,
                     log=got.append, progress=1e-9)
    K.race(c, round_games=4, max_games=4, min_games=4)
    pr = [json.loads(x[len('# progress '):]) for x in got if x.startswith('# progress ')]
    assert pr and pr[-1]['leaves'] > 0 and pr[-1]['of'] == 4 and {x['candidate'] for x in pr} == {'b1', 'b2'}
    assert any(x.startswith('# block ') for x in got) and any(x.startswith('# round ') for x in got)
    got = []
    c = K.Comparison(bs[:2], K.Field(seed=8), K.Pilot(net, sims=2, evaluator='rowexact'), seed=9, block=4, threads=1,
                     max_turns=6)
    BT.run_races([('p0', c, K.RaceState(c, round_games=4, max_games=4, min_games=4))], c._evaluators(),
                 log=got.append, progress=1e-9)
    pr = [json.loads(x[len('# progress '):]) for x in got if x.startswith('# progress ')]
    assert pr and pr[-1]['rows'] > 0 and pr[0]['pools']['p0']['alive'] == 2
    assert any(x.startswith('# round {"pool": "p0"') for x in got) and any(x.startswith('# race ') for x in got)


def test_evalpick_batched_over_pools(net, tmp_path):
    """`compare evalpick --pool-seed a,b --batched` races both pools at once: per pool the same picks, games and verdicts as
    the per-process command with the same evaluator (`--eval rowexact`), the summary over the pools, the batched run's
    statistics; a pool named twice is refused."""
    from mtgz.sealed import evaluator as EV
    decks = []
    for i in range(10):
        q = R.pool(5000 + i, collation='arena')
        decks += [R.build(q['cards'], s)['deck'] for s in range(4)]
    X = EV.counts_matrix(decks)
    y = (np.arange(len(decks)) % 2).astype(np.float32)
    sc = EV.Scaler(EV.deck_features(X))
    ep = str(tmp_path / 'lin.pt')
    EV.Evaluator('linear', [EV.fit('linear', X, sc(EV.deck_features(X)), y)], sc).save(ep)
    out = tmp_path / 'e.json'
    args = ['evalpick', '--evaluator', ep, '--pool-seed', '11,12', '--collation', 'paper', '--k', '5', '--worlds', '2',
            '--vs', '1', '--pilot', net, '--sims', '2', '--race', '--games', '4', '--block', '4', '--min-games', '4',
            '--max-games', '8', '--max-turns', '6', '--quiet', '--json', str(out)]
    assert K.main(args + ['--eval', 'rowexact', '--threads', '1']) == 0
    want = json.loads(out.read_text(encoding='utf-8'))
    assert K.main(args + ['--batched']) == 0
    got = json.loads(out.read_text(encoding='utf-8'))
    assert [p['evalpick']['pool_seed'] for p in got['pools']] == [11, 12] and got['batched']['games'] > 0
    for g, w in zip(got['pools'], want['pools']):
        assert g['evalpick']['picks'] == w['evalpick']['picks'] and g['game_scores'] == w['game_scores']
        assert (g['winner'], g['stop'], g['candidates']) == (w['winner'], w['stop'], w['candidates'])
    assert got['summary'] == want['summary']
    with pytest.raises(SystemExit):
        K.main(args[:4] + ['11,11'] + args[5:] + ['--batched'])


def test_the_eval_cache_with_rowexact(builds, net):
    """--eval-cache with RowExact: the same games bit for bit, fewer rows sent to the network (zero-core docs/search.md
    3.12); refused with the default evaluator; the value pick's seat has no cache."""
    p, bs = builds

    def play(cache):
        pilot = K.Pilot(net, sims=4, evaluator='rowexact', eval_cache=cache)
        c = K.Comparison(bs[:2], K.Field(seed=8), pilot, seed=9, block=6, threads=1, max_turns=8)
        K.compare(c, 6)
        return _scores(c), pilot.evaluators()['net'].leaves, pilot
    a, rows_a, _ = play(0)
    b, rows_b, pilot = play(256)
    assert a == b and rows_b < rows_a
    assert pilot.seat()['eval_cache'] == 256 and 'eval_cache' not in pilot.seat('flat', cache=False)
    assert pilot.describe()['eval_cache'] == 256
    with pytest.raises(ValueError):
        K.Pilot(net, eval_cache=64)
