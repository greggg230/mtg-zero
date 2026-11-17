"""The yardsticks' cost settings (docs/ENGINE.md 182; mtgz.az.yardsticks): off by default (the measurements of before),
checked; a cadence per kind (bo1, blunders, cells, cells_gen: gen over gen along its own cadence); a sequential best of
one through zero.h2h.sequential (the stop rule, the cap, local games only); sequential cells (batches on
h2h.sequential's seed schedule, the stop rule over every cell of the pair, the baseline on the same batches with its own
cache key, each cell against the baseline's same first units); sparse rows in the local head-to-heads. Fakes for the
games: no network is loaded."""
import json
import os
import random
import sys
import time

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
import mtgz.az  # noqa: E402,F401
from mtgz.az import yardsticks as Y  # noqa: E402

D, S, N = Y.CELL_DECKS


def test_the_cost_settings_are_off_by_default_and_checked():
    d = Y.normalize(Y.DELVER_YARDSTICKS)
    assert all(d[k] is None for k in Y.EVERY.values())
    assert d['bo1_target_se'] is None and d['bo1_sprt'] is None and d['cells_target_se'] is None
    assert not Y.bo1_sequential(d) and not Y.cells_sequential(d) and d['sparse'] is True
    assert Y.normalize({'blunders': True, 'bo1_every': 0})['bo1_every'] is None          # (0 = every one)
    on = Y.normalize({'bo1_games': 400, 'bo1_opponents': ['iter0'], 'bo1_target_se': 0.02, 'bo1_every': 48,
                      'blunders': False})
    assert Y.bo1_sequential(on) and on['bo1_batch'] == 100 and on['bo1_every'] == 48
    cells = {'bo1_games': 0, 'blunders': False, 'cells_games': 400, 'cells_refs': ['iter0'], 'cells_gen_games': 200}
    assert Y.cells_sequential(Y.normalize(dict(cells, cells_target_se=35)))
    for bad, why in (({'blunders': True, 'bo1_every': -16}, 'bo1_every'),
                     ({'bo1_opponents': ['iter0'], 'bo1_target_se': 0, 'blunders': False}, 'bo1_target_se'),
                     ({'bo1_opponents': ['iter0'], 'bo1_sprt': [20, 0], 'blunders': False}, 'bo1_sprt'),
                     ({'bo1_opponents': ['iter0'], 'bo1_sprt': [0, 20], 'bo1_batch': 7}, 'bo1_batch'),
                     ({'bo1_opponents': ['iter0'], 'bo1_target_se': 0.02, 'bo1_batch': 800}, 'bo1_batch'),
                     ({'bo1_opponents': ['iter0'], 'bo1_target_se': 0.02, 'bo1_backend': 'farm'}, 'farm'),
                     (dict(cells, cells_target_se=-1), 'cells_target_se'),
                     (dict(cells, cells_target_se=35, cells_batch=6), 'multiple of 4'),
                     (dict(cells, cells_target_se=35, cells_batch=120), 'not a multiple'),
                     (dict(cells, cells_target_se=35, cells_backend='farm'), 'farm')):
        with pytest.raises(ValueError, match=why):
            Y.normalize(bad)
    with pytest.raises(ValueError, match='unknown'):
        Y.normalize({'blunders': True, 'bo1_evry': 32})


def _run(tmp_path, ys, iters=range(0, 97, 16)):
    run = tmp_path / 'run'
    (run / 'models').mkdir(parents=True)
    cfg = {'preset': 'delver-mirror-bo3-dealt', 'eval_every': 16, 'eval_spec': json.dumps({'max_turns': 60}),
           'yardsticks': ys}
    (run / 'config.json').write_text(json.dumps(cfg))
    for i in iters:
        p = run / 'models' / f'iter_{i:04d}.pt'
        p.write_bytes(f'net {i}'.encode())
        os.utime(p, (time.time() - 3600, time.time() - 3600))
    return str(run), cfg


def test_cadences_pick_each_kinds_checkpoints(tmp_path):
    """bo1 every 32, blunders every 64, cells every 32, cells_gen every 48 (eval_every 16, checkpoints 0-96): each kind
    on its own multiples; gen over gen chains along its cadence (48 against 0, 96 against 48); without cadences every
    evaluated checkpoint, as before."""
    ys = {'bo1_games': 4, 'bo1_opponents': ['iter0'], 'bo1_device': 'cpu', 'bo1_every': 32, 'blunders': True,
          'blunders_every': 64, 'cells_games': 8, 'cells_refs': ['iter0'], 'cells_device': 'cpu', 'cells_every': 32,
          'cells_gen_games': 4, 'cells_gen_every': 48, 'cells_boot': 0}
    run, cfg = _run(tmp_path, ys)
    w = Y.wanted(run, cfg, Y.of_run(run), lib='L', settle=0)
    its = lambda kind: sorted({x['iter'] for x in w if x['kind'] == kind})      # noqa: E731
    assert its('bo1') == [32, 64, 96]                         # (0: gen0 against itself measures nothing)
    assert its('blunders') == [0, 64]
    assert its('cells') == [32, 64, 96]
    assert sorted({(x['iter'], x['ref']) for x in w if x['kind'] == 'cells_gen'}) == [(48, 'iter_0000'),
                                                                                    (96, 'iter_0048')]
    plain = {k: v for k, v in ys.items() if not k.endswith('_every')}
    w2 = Y.wanted(run, cfg, Y.normalize(plain), lib='L', settle=0)
    assert sorted({x['iter'] for x in w2 if x['kind'] == 'bo1'}) == [16, 32, 48, 64, 80, 96]
    assert sorted({x['iter'] for x in w2 if x['kind'] == 'blunders'}) == [0, 16, 32, 48, 64, 80, 96]
    assert sorted({x['iter'] for x in w2 if x['kind'] == 'cells_gen'}) == [16, 32, 48, 64, 80, 96]


def _bo1_fakes(monkeypatch, calls):
    from zero import h2h

    def out(games):
        return {'games': games, 'a_score': 0.7, 'se': 0.021, 'elo_diff': 147.0, 'elo_se': 16.0, 'unfinished': 0,
                'pairs': {'n': games // 2, 'pentanomial': [1, 2, 3, 4, 5]}, 'select_a': 'cq'}

    def match(a, b, games, sims, **kw):
        calls.append(('match', games, kw))
        return out(games)

    def sequential(a, b, batch_games, max_games, sprt=None, target_se=None, sims=64, **kw):
        calls.append(('sequential', (batch_games, max_games, sprt, target_se), kw))
        return dict(out(2 * batch_games), batches=[{}, {}], stop='target_se')
    monkeypatch.setattr(h2h, 'match', match)
    monkeypatch.setattr(h2h, 'sequential', sequential)
    monkeypatch.setattr(Y, 'bo1_seat', lambda ck: {'select': 'cq'})


def test_a_sequential_best_of_one_stops_by_its_rule_and_the_plain_one_is_as_before(tmp_path, monkeypatch):
    calls = []
    _bo1_fakes(monkeypatch, calls)
    base = {'bo1_games': 400, 'bo1_opponents': ['iter0'], 'bo1_device': 'cpu', 'blunders': False}
    run, cfg = _run(tmp_path, dict(base, bo1_target_se=0.025, bo1_batch=100))
    row = Y.measure_bo1(run, 16, 'gen0')
    kind, args, kw = calls[-1]
    assert kind == 'sequential' and args == (100, 400, None, 0.025), calls[-1]
    assert kw['paired'] is True and kw['sparse'] is True and kw['seed'] == 0
    assert row['games'] == 200 and row['stop'] == 'target_se' and row['batches'] == 2 and row['max_games'] == 400
    assert Y.bo1_backend(run, cfg, Y.of_run(run), {'name': 'gen0', 'path': Y.model_path(run, 0)}) == 'local'
    run2, _ = _run(tmp_path / 's', dict(base, bo1_sprt=[0, 20]))
    Y.measure_bo1(run2, 16, 'gen0')
    assert calls[-1][0] == 'sequential' and calls[-1][1] == (100, 400, (0, 20), None)
    # off: one plain match of bo1_games, the row's keys as before (sparse: the same answers)
    run3, _ = _run(tmp_path / 'p', dict(base))
    row = Y.measure_bo1(run3, 16, 'gen0')
    assert calls[-1][0] == 'match' and calls[-1][1] == 400 and calls[-1][2]['sparse'] is True
    assert 'stop' not in row and 'batches' not in row and row['games'] == 400
    run4, _ = _run(tmp_path / 'd', dict(base, sparse=False))
    Y.measure_bo1(run4, 16, 'gen0')
    assert calls[-1][2]['sparse'] is False


def test_a_best_of_one_plays_both_sides_at_the_runs_infer_precision(tmp_path, monkeypatch):
    """infer_precision (zero-core docs/az.md 57; ENGINE.md 183): both sides of a local best of one at the run's, named
    in the row; unset or 'fp32': no keyword, the row of before."""
    calls = []
    _bo1_fakes(monkeypatch, calls)
    base = {'bo1_games': 400, 'bo1_opponents': ['iter0'], 'bo1_device': 'cpu', 'blunders': False}
    run, _ = _run(tmp_path, dict(base))
    row = Y.measure_bo1(run, 16, 'gen0')
    assert 'precision' not in calls[-1][2] and 'precision' not in row
    for p in ('fp32', 'bf16'):
        rd = tmp_path / p
        r, cfg = _run(rd, dict(base))
        cfg['infer_precision'] = p
        (rd / 'run' / 'config.json').write_text(json.dumps(cfg))
        row = Y.measure_bo1(r, 16, 'gen0')
        assert calls[-1][2].get('precision') == (None if p == 'fp32' else p) and row.get('precision') == calls[-1][2].get('precision')


def _cells_fake(monkeypatch, calls, c_p=0.62, r_p=0.5):
    """zero.h2h.match stand-in for the cells: A's result per game from (seed, game id, spec) - the same deal, the same
    random number for C and for R (common random numbers) - C winning below c_p, R (a == b) below r_p."""
    from zero import h2h

    def match(a, b, games, sims, seed=0, spec=None, rows=False, sparse=None, **kw):
        calls.append({'a': os.path.basename(a), 'b': os.path.basename(b), 'games': games, 'seed': seed,
                      'sparse': sparse})
        p = r_p if a == b else c_p
        out = []
        for g in range(games):
            u = random.Random(f'{seed}|{g}|{spec}').random()
            x = float(u < p)
            res = [x, 1 - x] if g % 2 == 0 else [1 - x, x]
            out.append({'gid': g, 'result': res, 'over': True})
        return {'rows': out, 'a_score': 0.5, 'unfinished': 0, 'select_a': 'cq'}
    monkeypatch.setattr(h2h, 'match', match)


def _ckpts(tmp_path):
    ck, ref = tmp_path / 'c.pt', tmp_path / 'r.pt'
    ck.write_bytes(b'checkpoint')
    ref.write_bytes(b'reference')
    return str(ck), str(ref)


def test_sequential_cells_stop_by_the_cells_se_against_the_baselines_same_units(tmp_path, monkeypatch):
    calls = []
    _cells_fake(monkeypatch, calls)
    ck, ref = _ckpts(tmp_path)
    plain = {'bo1_games': 0, 'blunders': False, 'cells_games': 400, 'cells_refs': ['r=' + ref], 'cells_boot': 0,
             'cells_device': 'cpu'}
    seat, cdir = {'select': 'cq'}, str(tmp_path / 'cache')
    loose = Y.normalize(dict(plain, cells_target_se=1000.0, cells_batch=100))
    tight = Y.normalize(dict(plain, cells_target_se=0.01, cells_batch=100))
    # the baseline: every batch of both chooser specs, seeds seed + 7919 k; a key of its own (the batch)
    b = Y.measure_base_core(ref, 'r', (D, S), 400, loose, seat, cdir)
    seed = Y.pair_seed(loose, (D, S))
    assert [(c['games'], c['seed'] - seed) for c in calls] == [(100, 7919 * k) for k in range(4) for _ in (0, 1)]
    assert all(c['sparse'] is True for c in calls)
    assert b['base'] != Y.base_key(ref, (D, S), 400, Y.normalize(plain), seat)[0]
    assert b['base'] == Y.base_key(ref, (D, S), 400, tight, seat)[0]        # (the target is not in the key)
    calls.clear()
    row = Y.measure_cells_core('cells', ck, 16, ref, 'r', (D, S), 400, loose, seat, cdir)
    assert len(calls) == 4 and row['stop'] == 'target_se' and row['batches'] == 2 and row['games'] == 400
    assert all(c['units'] == 100 for c in row['cells'].values()) and row['max_games'] == 400
    # its cells are the plain statistics of those units against the baseline's first units (the CRN pairing kept)
    data = Y.load_base(cdir, b['base'])
    bres = {int(c): Y.res_decode(r)[:200] for c, r in data['results'].items()}
    bu = Y.cell_units((D, S), bres)
    cu = Y.cell_units((D, S), {int(c): Y.res_decode(r) for c, r in row['results'].items()})
    for cid, st in row['cells'].items():
        assert st == Y.cell_stats(cu[cid], bu[cid], boot=0, seed=Y._cell_seed(cid))
        assert st['corr'] is not None and st['corr'] > 0.5          # (the same deals: correlated)
    calls.clear()
    row = Y.measure_cells_core('cells', ck, 16, ref, 'r', (D, S), 400, tight, seat, cdir)
    assert len(calls) == 8 and row['stop'] == 'max_games' and row['games'] == 800
    # a mirror: half the games a spec per batch, no baseline
    calls.clear()
    m = Y.measure_cells_core('cells', ck, 16, ref, 'r', (N, N), 400, loose, seat, cdir)
    assert m['base'] is None and calls[0]['games'] == 50 and m['batches'] == 2


def test_sequential_cells_play_here(tmp_path):
    ys = Y.normalize({'bo1_games': 0, 'blunders': False, 'cells_games': 400, 'cells_refs': ['iter0'],
                      'cells_target_se': 35})
    cfg = {'selfplay_remote': 'pool:farm-x', 'pool_build': 'b' * 24, 'pool_runtime': 'c' * 24, 'pool_worker': 'w'}
    assert Y.cells_backend(str(tmp_path), cfg, ys, str(tmp_path / 'r.pt')) == 'local'
