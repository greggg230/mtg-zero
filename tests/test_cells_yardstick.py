"""The metagame cells yardstick (docs/ENGINE.md 177; mtgz.az.yardsticks, kinds cells / cells_gen / cells_base): the
statistics (the odds-ratio Elo gain over paired seeds, its delta-method se with the pairing's covariance, the bootstrap,
synthetic results with known answers), the games' layout (cells from the two chooser matches of a deck pair), the
summaries (per-deck, overall, anchored), the baseline cache, the reference ladder (since, freezing, chaining, the
promotion flag), the watcher's order and gating, the farm backend's jobs (fakes: no farm) and the settings. No process
is started here; tools/run_yardsticks.py's child is exercised by the smoke (docs/ENGINE.md 177)."""
import json
import math
import os
import random
import sys
import time

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import mtgz.az  # noqa: E402,F401
from mtgz.az import yardsticks as Y  # noqa: E402

D, S, N = Y.CELL_DECKS            # Delver, Doomsday, Sneak and Show
K = 400 / math.log(10)


# ---------------------------------------------------------------- the statistics

def test_identical_games_gain_nothing_with_no_noise_under_the_pairing():
    """The same seeds played identically by C and R: the gain is 0 and so is its se - the CRN pairing - while the
    unpaired se (the baseline taken as independent games) is not."""
    rng = random.Random(1)
    a = [(rng.choice((0.0, 1.0)), rng.choice((0.0, 0.5, 1.0))) for _ in range(200)]
    s = Y.cell_stats(a, list(a), boot=200)
    assert s['elo'] == 0 and s['se'] == 0 and s['boot_se'] == 0
    assert s['se_unpaired'] > 15 and s['corr'] == 1.0


def test_the_delta_method_is_the_gradient_through_the_units_covariance():
    np = pytest.importorskip('numpy')
    a = [(1, 1), (1, 0), (0, 0), (1, 0.5), (0, 1), (1, 1)]
    b = [(1, 0), (0, 0), (0, 0), (1, 1), (0, 0), (1, 0)]
    s = Y.cell_stats(a, b, boot=0)
    u = np.array([(x + y) / 2 for x, y in a])
    v = np.array([(x + y) / 2 for x, y in b])
    pa, pb = u.mean(), v.mean()
    g = np.array([1 / (pa * (1 - pa)), -1 / (pb * (1 - pb))])
    var = g @ np.cov(np.vstack([u, v])) @ g / len(u)
    assert s['elo'] == pytest.approx(K * (math.log(pa / (1 - pa)) - math.log(pb / (1 - pb))), abs=0.06)
    assert s['se'] == pytest.approx(K * math.sqrt(var), abs=0.06)
    assert s['score'] == round(pa, 4) and s['base'] == round(pb, 4) and s['units'] == 6 and s['games'] == 12
    assert s['chose'] == {'score': round(4 / 6, 4), 'base': 0.5} and s['pentanomial'] == [1, 0, 2, 1, 2]


def test_a_mirror_cell_is_the_ordinary_paired_head_to_head():
    """No baseline: 0.5 exactly; elo and se are zero.h2h's paired summary's (the pair scores' se through the logistic
    Elo's derivative)."""
    from zero import h2h
    rng = random.Random(7)
    a = [(float(rng.random() < 0.62), float(rng.random() < 0.55)) for _ in range(150)]
    s = Y.cell_stats(a, None, boot=0)
    rows = [r for g, (x, y) in enumerate(a) for r in ({'gid': 2 * g, 'result': [x, 1 - x]},
                                                     {'gid': 2 * g + 1, 'result': [1 - y, y]})]
    ps = h2h.pair_stats(rows)
    p = ps['score']
    assert s['base'] == 0.5 and s['score'] == pytest.approx(p, abs=1e-4)
    assert s['elo'] == pytest.approx(400 * math.log10(p / (1 - p)), abs=0.1)
    assert s['se'] == pytest.approx(K * math.sqrt(ps['var'] / ps['n']) / (p * (1 - p)), abs=0.1)


def _world(rng, n, gain, base=(0.72, 0.45)):
    """n paired seeds: each game's one uniform U shared by C and R (the same deal), C winning when U < its
    probability (R's logit + gain in Elo)."""
    pc = [1 / (1 + (1 - q) / q * 10 ** (-gain / 400)) for q in base]
    a, b = [], []
    for _ in range(n):
        us = (rng.random(), rng.random())
        a.append(tuple(float(us[j] < pc[j]) for j in (0, 1)))
        b.append(tuple(float(us[j] < base[j]) for j in (0, 1)))
    truth = K * (math.log(sum(pc) / 2 / (1 - sum(pc) / 2)) - math.log(sum(base) / 2 / (1 - sum(base) / 2)))
    return a, b, truth


def test_the_se_is_the_spread_of_the_estimate_over_replications():
    """Synthetic paired worlds with a known gain: the estimates centre on it, the delta-method se matches their
    spread across 400 replications (and the bootstrap's agrees on one), and the unpaired se overstates it."""
    rng = random.Random(2026)
    es, ses, unp = [], [], []
    for _ in range(400):
        a, b, truth = _world(rng, 200, 60.0)
        s = Y.cell_stats(a, b, boot=0)
        es.append(s['elo'])
        ses.append(s['se'])
        unp.append(s['se_unpaired'])
    m = sum(es) / len(es)
    sd = math.sqrt(sum((e - m) ** 2 for e in es) / (len(es) - 1))
    assert abs(m - truth) < 3 * sd / math.sqrt(len(es)) + 2.0, (m, truth, sd)
    assert 0.88 < (sum(ses) / len(ses)) / sd < 1.12, (sum(ses) / len(ses), sd)
    assert (sum(unp) / len(unp)) > 1.3 * sd                 # the pairing's gain
    a, b, _ = _world(random.Random(5), 200, 60.0)
    s = Y.cell_stats(a, b, boot=2000, seed=3)
    assert 0.85 < s['boot_se'] / s['se'] < 1.15 and s['boot_95'][0] < s['elo'] < s['boot_95'][1]


def test_a_cell_at_100_percent_is_clamped_not_infinite():
    s = Y.cell_stats([(1.0, 1.0)] * 20, [(0.0, 1.0)] * 20, boot=50)
    assert s['clamped'] and math.isfinite(s['elo']) and s['score'] == 1.0
    assert s['elo'] == pytest.approx(K * (math.log((1 - 0.5 / 40) / (0.5 / 40)) - 0), abs=0.1)
    with pytest.raises(ValueError):
        Y.cell_stats([], None)
    with pytest.raises(ValueError):
        Y.cell_stats([(1, 1)], [(1, 1), (0, 0)])


def test_the_cells_of_a_pair_come_from_its_two_chooser_matches():
    """A in seat gid % 2 (zero.h2h's and a farm job's convention): even game ids pilot p0's deck (cell X|Y), odd ones
    p1's (Y|X); game g of the chooser-0 match and of the chooser-1 match are one unit, A's side choosing in the one
    whose chooser is A's seat."""
    r0 = [1.0, 0.0, 0.5, 1.0]          # chooser seat 0
    r1 = [0.0, 1.0, 1.0, 0.0]          # chooser seat 1
    u = Y.cell_units((D, S), {0: r0, 1: r1})
    assert u == {f'{D}|{S}': [(1.0, 0.0), (0.5, 1.0)], f'{S}|{D}': [(1.0, 0.0), (0.0, 1.0)]}
    m = Y.cell_units((D, D), {0: r0, 1: r1})
    assert list(m) == [f'{D}|{D}'] and len(m[f'{D}|{D}']) == 4
    assert Y.res_decode(Y.res_code(r0)) == r0 and Y.res_code([0.25]) == [0.25]


def test_the_pair_specs_put_both_mains_and_the_chooser_and_the_seeds_are_the_pairs():
    ys = Y.normalize({'bo1_games': 0, 'blunders': False, 'cells_games': 8, 'cells_refs': ['iter0']})
    jobs = Y.pair_jobs((D, N), 8, ys)
    assert [j['games'] for j in jobs] == [8, 8] and [j['chooser'] for j in jobs] == [0, 1]
    assert jobs[0]['seed'] == jobs[1]['seed'] == Y.pair_seed(ys, (D, N)) != Y.pair_seed(ys, (D, S))
    sp = [json.loads(j['spec']) for j in jobs]
    from mtgz.az import decklists as DL
    assert sp[0]['decks'] == {'p0': DL.names(DL.load(D)['main']), 'p1': DL.names(DL.load(N)['main'])}
    assert (sp[0]['chooser'], sp[1]['chooser'], sp[0]['constructed'], sp[0]['max_turns']) == (0, 1, True, 60)
    assert [j['games'] for j in Y.pair_jobs((S, S), 8, ys)] == [4, 4]      # (a mirror: every game its cell's)
    assert Y.cell_pairs(ys) == [(D, D), (D, S), (D, N), (S, S), (S, N), (N, N)]
    with pytest.raises(NotImplementedError):
        Y._cell_spec(D, S, 0, dict(ys, cells_format='bo3'))


def test_the_engine_parses_every_cell_spec():
    from mtgz import _engine as E      # noqa: F401
    from zero import games as G
    ys = Y.normalize({'bo1_games': 0, 'blunders': False, 'cells_games': 4, 'cells_refs': ['iter0']})
    for pair in Y.cell_pairs(ys):
        for j in Y.pair_jobs(pair, 4, ys):
            G.Driver('mtg', [[G.nn_seat('m', 1), G.nn_seat('m', 1)]], spec=j['spec'], concurrency=1, seed=0)


def _cells(gains, se=10.0, vs=None):
    return {Y.cell_id(x, y): {'elo': gains[i * 3 + j], 'se': se, 'score': 0.5, 'base': 0.5,
                              'vs_ref': (vs or {}).get((i, j), Y.expected_score(gains[i * 3 + j])), 'games': 400}
            for i, x in enumerate(Y.CELL_DECKS) for j, y in enumerate(Y.CELL_DECKS)}


def test_the_summary_means_cells_per_deck_and_overall_and_anchors_through_the_reference():
    gains = [10, 20, 30, 40, 50, 60, 70, 80, 300]
    s = Y.summarize(_cells(gains), Y.CELL_DECKS, base=(1100.0, 12.0), promote=0.8)
    assert s['complete'] and s['decks'][D] == {'elo': 20.0, 'se': round(math.sqrt(300) / 3, 1)}
    assert s['decks'][N]['elo'] == 150.0
    assert s['overall'] == {'elo': 73.3, 'se': round(math.sqrt(900) / 9, 1)}
    assert s['anchored'] == {'elo': 1173.3, 'se': round(math.sqrt(144 + (30 / 9) ** 2), 1)}
    assert s['promote'] == [Y.cell_id(N, N)]                       # (300 Elo: expected score 0.85)
    part = _cells(gains)
    del part[Y.cell_id(S, N)]
    p = Y.summarize(part, Y.CELL_DECKS, base=(1000.0, 0.0))
    assert not p['complete'] and p['overall'] is None and p['anchored'] is None and S not in p['decks']
    assert p['missing'] == [Y.cell_id(S, N)] and D in p['decks']


# ---------------------------------------------------------------- the baseline cache and the measurements

def _fake_play(calls, c_score=0.75, r_score=0.5):
    """play_pair_local stand-in: A wins with a fixed pattern by seat; counts its calls."""
    def play(a, b, pair, games, ys, seat, log=print):
        calls.append((os.path.basename(a), os.path.basename(b), tuple(pair), games))
        n = Y.pair_jobs(pair, games, ys)[0]['games']
        p = r_score if os.path.basename(a) == os.path.basename(b) else c_score
        res = {str(c): [float((g * 7 + c * 3) % 100 < p * 100) for g in range(n)] for c in (0, 1)}
        return {'results': res, 'unfinished': 0, 'select': 'visits'}
    return play


def _ckpts(tmp_path):
    ck, ref = tmp_path / 'c.pt', tmp_path / 'r.pt'
    ck.write_bytes(b'checkpoint')
    ref.write_bytes(b'reference')
    return str(ck), str(ref)


def test_the_baseline_is_played_once_per_key_and_the_cells_read_it(tmp_path, monkeypatch):
    calls = []
    monkeypatch.setattr(Y, 'play_pair_local', _fake_play(calls))
    ck, ref = _ckpts(tmp_path)
    ys = Y.normalize({'bo1_games': 0, 'blunders': False, 'cells_games': 40, 'cells_refs': ['r=' + ref],
                      'cells_boot': 0})
    seat, cdir = {'select': 'cq'}, str(tmp_path / 'cache')
    with pytest.raises(RuntimeError, match='no baseline'):
        Y.measure_cells_core('cells', ck, 16, ref, 'r', (D, S), 40, ys, seat, cdir)
    b1 = Y.measure_base_core(ref, 'r', (D, S), 40, ys, seat, cdir)
    assert not b1['cached'] and len(calls) == 1 and calls[0][:2] == ('r.pt', 'r.pt')
    b2 = Y.measure_base_core(ref, 'r', (D, S), 40, ys, seat, cdir)
    assert b2['cached'] and len(calls) == 1 and b2['base'] == b1['base'] and b2['scores'] == b1['scores']
    # the key moves with the sims, the seat, the seed, the games and the reference's network
    k = Y.base_key(ref, (D, S), 40, ys, seat)[0]
    assert k == b1['base']
    assert len({k, Y.base_key(ref, (D, S), 40, dict(ys, cells_sims=8), seat)[0],
                Y.base_key(ref, (D, S), 40, ys, {'select': 'visits'})[0],
                Y.base_key(ref, (D, S), 40, dict(ys, cells_seed=1), seat)[0],
                Y.base_key(ref, (D, S), 20, ys, seat)[0], Y.base_key(ck, (D, S), 40, ys, seat)[0]}) == 6
    row = Y.measure_cells_core('cells', ck, 16, ref, 'r', (D, S), 40, ys, seat, cdir)
    assert len(calls) == 2 and calls[1][:2] == ('c.pt', 'r.pt')
    assert set(row['cells']) == {Y.cell_id(D, S), Y.cell_id(S, D)} and row['base'] == b1['base']
    assert row['cells'][Y.cell_id(D, S)]['units'] == 20 and row['games'] == 80
    assert row['sha'] == Y.sha12(ck) and row['ref_sha'] == Y.sha12(ref) and row['iter'] == 16
    assert all(c['elo'] > 0 for c in row['cells'].values())          # 0.75 against 0.5 on the same pattern
    mirror = Y.measure_cells_core('cells', ck, 16, ref, 'r', (N, N), 40, ys, seat, cdir)
    assert mirror['base'] is None and mirror['cells'][Y.cell_id(N, N)]['base'] == 0.5 and len(calls) == 3
    with pytest.raises(ValueError, match='mirror'):
        Y.measure_base_core(ref, 'r', (N, N), 40, ys, seat, cdir)


def test_standalone_measures_every_pair_and_summarizes(tmp_path, monkeypatch):
    calls = []
    monkeypatch.setattr(Y, 'play_pair_local', _fake_play(calls))
    ck, ref = _ckpts(tmp_path)
    ys = Y.normalize({'bo1_games': 0, 'blunders': False, 'cells_games': 20, 'cells_refs': ['r=' + ref],
                      'cells_boot': 0})
    out = tmp_path / 'rows.jsonl'
    rows, s = Y.measure_cells_standalone(ck, ref, ys, str(tmp_path / 'cache'), seat={}, rows_out=str(out))
    assert [r['kind'] for r in rows].count('cells_base') == 3 and [r['kind'] for r in rows].count('cells') == 6
    assert s['complete'] and s['overall']['elo'] > 0 and len(out.read_text().splitlines()) == 9
    n = len(calls)
    Y.measure_cells_standalone(ck, ref, ys, str(tmp_path / 'cache'), seat={})
    assert len(calls) == n + 6                                       # (the baselines cached: only C's games)


# ---------------------------------------------------------------- a fake run: wanted, the ladder, the report

def _run(tmp_path, iters=(0, 16, 32, 48), ys=None, pool=False):
    run = tmp_path / 'run'
    (run / 'models').mkdir(parents=True)
    cfg = {'preset': 'delver-mirror-bo3-dealt', 'eval_every': 16,
           'yardsticks': dict({'bo1_games': 0, 'blunders': False, 'cells_games': 8, 'cells_refs': ['iter0'],
                               'cells_device': 'cpu', 'cells_boot': 0}, **(ys or {}))}
    if pool:
        cfg.update(selfplay_remote='pool:farm-x', pool_build='b' * 24, pool_runtime='c' * 24,
                   pool_worker=str(tmp_path / 'w.exe'))
    (run / 'config.json').write_text(json.dumps(cfg))
    for i in iters:
        p = run / 'models' / f'iter_{i:04d}.pt'
        p.write_bytes(f'net {i}'.encode())
        os.utime(p, (time.time() - 3600, time.time() - 3600))
    return str(run), cfg


def _fake_measure(run, cfg, ys, item, gain=40.0):
    """An in-process measurement: a base item writes its cache file; a cells item writes its row (a fixed gain)."""
    if item['kind'] == 'cells_base':
        os.makedirs(Y.cache_dir(run, ys), exist_ok=True)
        with open(Y.base_file(Y.cache_dir(run, ys), item['base']), 'w') as f:
            json.dump({'results': {}}, f)
        Y.append_row(run, {'kind': 'cells_base', 'ref': item['ref'], 'pair': item['pair'], 'base': item['base'],
                           'scores': {}, 'time': 1})
        return
    x, y = item['pair']
    cids = {Y.cell_id(x, y), Y.cell_id(y, x)}
    Y.append_row(run, {'kind': item['kind'], 'iter': item['iter'], 'ref': item['ref'], 'pair': item['pair'],
                       'sha': Y.sha12(Y.model_path(run, item['iter'])), 'base': item['base'], 'time': time.time(),
                       'cells': {c: {'elo': gain, 'se': 12.0, 'score': 0.6, 'base': 0.5, 'games': 8,
                                     'vs_ref': Y.expected_score(gain)} for c in cids}})


def _drain(run, cfg, ys, lane='cpu_bo1', gain=40.0):
    done = []
    while True:
        it = Y.pending(run, cfg, ys, lane, settle=0)
        if it is None:
            return done
        _fake_measure(run, cfg, ys, it, gain)
        done.append((it['kind'], it.get('iter'), it['ref'], tuple(it['pair'])))


def test_each_pairs_baseline_comes_first_once_and_gates_its_cells(tmp_path):
    run, cfg = _run(tmp_path)
    ys = Y.of_run(run)
    w = Y.wanted(run, cfg, ys, settle=0)
    assert {x['lane'] for x in w} == {'cpu_bo1'} and Y.lanes(run, cfg, ys) == ['cpu_bo1']
    kinds = [(x['kind'], x.get('iter')) for x in w]
    assert kinds.count(('cells_base', None)) == 3                     # once per non-mirror pair, before the first
    assert kinds[0] == ('cells', 16) and kinds[1] == ('cells_base', None)
    assert not any(x.get('iter') == 0 for x in w)                    # (generation 0 is the reference)
    first = Y.pending(run, cfg, ys, 'cpu_bo1', settle=0)
    assert first['kind'] == 'cells' and tuple(first['pair']) == (D, D)
    # a failing baseline holds its cells back (they wait: missing), not the other pairs'
    _fake_measure(run, cfg, ys, first)
    b = Y.pending(run, cfg, ys, 'cpu_bo1', settle=0)
    assert b['kind'] == 'cells_base'
    Y.append_row(run, {'kind': 'cells_base', 'ref': b['ref'], 'pair': b['pair'], 'base': b['base'], 'error': 'x',
                       'attempt': 1, 'retry_at': time.time() + 600, 'time': 1})
    nxt = Y.pending(run, cfg, ys, 'cpu_bo1', settle=0)
    assert nxt['kind'] == 'cells_base' and nxt['base'] != b['base']
    st = Y.status(run, cfg, ys)
    assert st['missing'] == [] and [f['kind'] for f in st['cells']['failed']] == ['cells_base']
    assert any(m['kind'] == 'cells' and m['pair'] == b['pair'] for m in st['cells']['missing'])


def test_the_ladder_freezes_on_command_and_chains_the_elo(tmp_path):
    run, cfg = _run(tmp_path)
    ys = Y.of_run(run)
    done = _drain(run, cfg, ys, gain=300.0)
    assert [d[1] for d in done if d[0] == 'cells'] == [16] * 6 + [32] * 6 + [48] * 6
    rep = Y.cells_report(run, cfg, ys)
    assert rep['refs'][0]['label'] == 'gen0' and rep['refs'][0]['elo'] == 1000.0
    assert [g['iter'] for g in rep['cells']] == [16, 32, 48]
    assert rep['cells'][0]['summary']['anchored'] == {'elo': 1300.0, 'se': 4.0}
    assert rep['promote']['iter'] == 48 and len(rep['promote']['cells']) == 9       # (300 Elo > 0.80)
    assert not os.path.exists(Y.ladder_path(run))                     # (suggested, never written by the watcher)
    with pytest.raises(ValueError, match='complete'):
        Y.freeze_ref(run, 64, cfg=cfg, ys=ys)
    e = Y.freeze_ref(run, 32, cfg=cfg, ys=ys)
    assert e['label'] == 'g32' and e['since'] == 32 and e['elo'] == 1300.0 and e['from_ref'] == 'gen0'
    assert Y.sha12(e['path']) == Y.sha12(Y.model_path(run, 32))
    with pytest.raises(ValueError, match='exists'):
        Y.freeze_ref(run, 32, label='g32', cfg=cfg, ys=ys)
    refs = Y.cell_refs(run, cfg, ys)
    assert [r['label'] for r in refs] == ['gen0', 'g32']
    assert Y.ref_for(refs, 16)['label'] == 'gen0' and Y.ref_for(refs, 48)['label'] == 'g32'
    # from 32 on: against g32 (32 is g32 itself: nothing); 48 is wanted again against g32
    w = [(x['kind'], x.get('iter'), x['ref']) for x in Y.wanted(run, cfg, ys, settle=0) if x['kind'] == 'cells']
    assert {(i, r) for _, i, r in w} == {(16, 'gen0'), (48, 'g32')}
    done = _drain(run, cfg, ys, gain=50.0)
    assert {(d[1], d[2]) for d in done if d[0] == 'cells'} == {(48, 'g32')}
    rep = Y.cells_report(run, cfg, ys)
    assert rep['refs'][1]['elo'] == 1300.0 and rep['refs'][1]['how'] == 'frozen'
    g48 = [g for g in rep['cells'] if (g['iter'], g['ref']) == (48, 'g32')][0]['summary']
    assert g48['anchored']['elo'] == 1350.0
    assert g48['anchored']['se'] == pytest.approx(math.sqrt(4.0 ** 2 + 4.0 ** 2), abs=0.1)
    assert rep['promote'] is None                                    # (the newest complete one: +50)


def test_a_ladder_reference_without_a_frozen_elo_chains_through_its_measurement(tmp_path):
    run, cfg = _run(tmp_path)
    ys = Y.of_run(run)
    _drain(run, cfg, ys, gain=100.0)
    copy = tmp_path / 'g16.pt'
    copy.write_bytes((tmp_path / 'run' / 'models' / 'iter_0016.pt').read_bytes())
    ys2 = dict(ys, cells_refs=['iter0', {'label': 'g16', 'path': str(copy), 'since': 40}])
    lad = Y.ladder(run, cfg, ys2)
    assert lad[1]['elo'] == 1100.0 and lad[1]['how'] == 'iteration 16 against gen0'
    ys3 = dict(ys, cells_refs=['iter0', {'label': 'x', 'path': str(tmp_path / 'other.pt')}])
    assert Y.ladder(run, cfg, ys3)[1]['elo'] is None
    with pytest.raises(ValueError, match='since'):
        Y.cell_refs(run, cfg, dict(ys, cells_refs=['a=x.pt', {'label': 'b', 'path': 'y', 'since': 5}, 'c=z.pt']))
    with pytest.raises(ValueError, match='two'):
        Y.cell_refs(run, cfg, dict(ys, cells_refs=['a=x.pt', 'a=y.pt']))


def test_gen_over_gen_measures_each_checkpoint_against_the_previous_one(tmp_path):
    run, cfg = _run(tmp_path, ys={'cells_games': 0, 'cells_gen_games': 4, 'cells_refs': []})
    ys = Y.of_run(run)
    w = Y.wanted(run, cfg, ys, settle=0)
    assert {(x['iter'], x['ref']) for x in w if x['kind'] == 'cells_gen'} == {
        (16, 'iter_0000'), (32, 'iter_0016'), (48, 'iter_0032')}
    assert sum(x['kind'] == 'cells_base' for x in w) == 9             # (each previous checkpoint's own baselines)
    assert Y.ref_path(run, cfg, ys, 'iter_0016') == Y.model_path(run, 16)
    _drain(run, cfg, ys, gain=25.0)
    rep = Y.cells_report(run, cfg, ys)
    assert [(g['iter'], g['prev']) for g in rep['gen']] == [(16, 0), (32, 16), (48, 32)]
    assert rep['gen'][0]['summary']['overall']['elo'] == 25.0 and rep['gen'][0]['summary']['anchored'] is None


def test_the_watchers_child_command_and_its_result_line(tmp_path):
    import run_yardsticks as RY
    run, cfg = _run(tmp_path)
    ys = Y.of_run(run)
    w = Y.wanted(run, cfg, ys, settle=0)
    base = next(x for x in w if x['kind'] == 'cells_base')
    cell = next(x for x in w if x['kind'] == 'cells' and x['pair'] == base['pair'])
    wt = RY.Watcher(run, once=True, log=lambda m: None)
    cmd = wt.one_cmd(base)
    assert cmd[3:] == [run, '--kind', 'cells_base', '--ref', 'gen0', '--pair', ','.join(base['pair']), '--games', '8']
    assert wt.one_cmd(cell)[3:] == [run, '--kind', 'cells', '--iter', '16', '--ref', 'gen0', '--pair',
                                    ','.join(cell['pair'])]
    key, what = RY.item_key(cell)
    assert key == 'cells_0016_gen0_izzet-doomsday' and 'iteration 16 vs gen0' in what
    assert RY.item_key(base)[0].startswith('cells_base_gen0_izzet-doomsday_')
    row = {'kind': 'cells', 'iter': 16, 'ref': 'gen0', 'pair': cell['pair'], 'seconds': 3.0,
           'cells': {'a|b': {'elo': 12.3, 'se': 20.0}}}
    assert RY.same_measurement(row, cell) and not RY.same_measurement(dict(row, iter=32), cell)
    assert RY.result_line(row) == 'a|b +12 +- 20, 3 s'
    assert RY.same_measurement({'kind': 'bo1', 'iter': 16, 'opp': 'gen0'}, {'kind': 'bo1', 'iter': 16, 'opp': 'gen0'})


# ---------------------------------------------------------------- the farm backend (fakes)

def test_the_farm_backend_plays_each_chooser_match_as_an_unpaired_job(tmp_path, monkeypatch):
    from mtgz.az import farm_campaign as C
    run, cfg = _run(tmp_path, pool=True)
    ys = Y.of_run(run)
    subs = []
    monkeypatch.setattr(C, 'match_settings', lambda ck, sims, spec, tilts=None, ckpt_b=None:
                        ({'eval_spec': spec, 'rule_of': ck, 'b': ckpt_b}, {}))
    monkeypatch.setattr(C, 'match_seat', lambda *a, **k: {})

    def submit(name, a, games, settings, seats, seed, build, worker, runtime, b=None, paired=True, **kw):
        subs.append({'name': name, 'a': a, 'b': b, 'games': games, 'seats': seats, 'seed': seed, 'paired': paired,
                     'chooser': json.loads(settings['eval_spec'])['chooser'], 'rule_of': settings['rule_of']})
        return name
    monkeypatch.setattr(C, 'submit', submit)
    monkeypatch.setattr(C, 'collect', lambda d, **kw: {'run': d, 'digest': 'd' * 16, 'seconds': 1.0,
                                                      'spot': {'checked': 1, 'ok': 1, 'of': 4},
                                                      'coverage': {'local': 0}})
    monkeypatch.setattr(C, 'games', lambda d: [
        {'gid': g, 'seats': [{'net': 1}, {'net': 2}][::1 if g % 2 == 0 else -1],
         'result': [1.0, 0.0] if g % 2 == 0 else [0.0, 1.0], 'over': True}
        for g in range(next(s['games'] for s in subs if s['name'] == d))])
    assert Y.cells_backend(run, cfg, ys, Y.model_path(run, 0)) == 'farm'
    assert Y.lanes(run, cfg, ys) == ['farm']
    assert Y.cells_backend(run, cfg, dict(ys, cells_backend='local'), Y.model_path(run, 0)) == 'local'
    with pytest.raises(RuntimeError, match='no baseline'):
        Y.measure_cells(run, 16, 'gen0', (D, S), cfg, ys)
    key = Y.base_key(Y.model_path(run, 0), (D, S), 8, ys, Y.cells_seat(run))[0]
    os.makedirs(Y.cache_dir(run, ys))
    with open(Y.base_file(Y.cache_dir(run, ys), key), 'w') as f:
        json.dump({'results': {'0': '01' * 4, '1': '01' * 4}}, f)
    row = Y.measure_cells(run, 16, 'gen0', (D, S), cfg, ys)
    assert [(s['chooser'], s['paired'], s['seats'], os.path.basename(s['a']), os.path.basename(s['b'])) for s in subs] \
        == [(c, False, [{'net': 1}, {'net': 2}], 'iter_0016.pt', 'iter_0000.pt') for c in (0, 1)]
    assert subs[0]['seed'] == subs[1]['seed'] == Y.pair_seed(ys, (D, S)) and subs[0]['games'] == 8
    assert row['backend'] == 'farm' and row['farm']['0']['digest'] == 'd' * 16
    assert row['results'] == {'0': '1' * 8, '1': '1' * 8}             # (A, in seat gid % 2, won every game)
    subs.clear()
    monkeypatch.setattr(C, 'games', lambda d: [
        {'gid': g, 'seats': [{'net': 1}, {'net': 1}], 'result': [0.0, 1.0], 'over': True}
        for g in range(next(s['games'] for s in subs if s['name'] == d))])
    os.remove(Y.base_file(Y.cache_dir(run, ys), row['base']))
    b = Y.measure_cells_base(run, 'gen0', (D, S), 8, cfg, ys)
    assert [(s['seats'], s['b'], os.path.basename(s['a']), os.path.basename(s['rule_of'])) for s in subs] == \
        [([{'net': 1}, {'net': 1}], None, 'iter_0000.pt', 'iter_0000.pt')] * 2
    assert b['scores'] == {Y.cell_id(D, S): 0.0, Y.cell_id(S, D): 1.0} and b['backend'] == 'farm'
    # no pool: local
    nopool, ncfg = _run(tmp_path / 'np')
    assert Y.cells_backend(nopool, ncfg, Y.of_run(nopool), Y.model_path(nopool, 0)) == 'local'


# ---------------------------------------------------------------- the setting

def test_the_cells_setting_is_checked_and_off_by_default():
    d = Y.normalize({'blunders': True})
    assert d['cells_games'] == 0 and d['cells_gen_games'] == 0 and d['cells_decks'] == Y.CELL_DECKS
    on = Y.normalize({'bo1_games': 0, 'blunders': False, 'cells_games': 400, 'cells_refs': ['iter0']})
    assert (on['cells_sims'], on['cells_device'], on['cells_promote'], on['cells_base_elo']) == (64, 'cuda', 0.8, 1000.0)
    for bad, why in (({'cells_games': 3, 'cells_refs': ['iter0']}, 'even'),
                     ({'cells_games': 4}, 'cells_refs'),
                     ({'cells_games': 4, 'cells_refs': ['x']}, 'LABEL=PATH'),
                     ({'cells_games': 4, 'cells_refs': ['iter_3=a.pt']}, 'iter_'),
                     ({'cells_games': 4, 'cells_refs': ['iter0'], 'cells_format': 'bo3'}, 'not built'),
                     ({'cells_games': 4, 'cells_refs': ['iter0'], 'cells_decks': ['a', 'a']}, 'distinct'),
                     ({'cells_games': 4, 'cells_refs': ['iter0'], 'cells_promote': 1.2}, 'promote'),
                     ({'cells_games': 4, 'cells_refs': ['iter0'], 'cells_backend': 'mac'}, 'cells_backend'),
                     ({'bo1_games': 0, 'blunders': False}, 'nothing')):
        with pytest.raises(ValueError, match=why):
            Y.normalize(bad)
    assert Y.normalize({'bo1_games': 0, 'blunders': False, 'cells_gen_games': 4})['cells_refs'] == []
