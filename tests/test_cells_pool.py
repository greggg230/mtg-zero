"""The cells' reference pool and joint rating fit (docs/ENGINE.md 186; mtgz.az.cells_fit, the pool mode of
mtgz.az.yardsticks): the fit recovers known skills on simulated games with common random numbers (one uniform per
deal and choice, shared by every generation that plays it: the CRN coupling of the real cells, with the two choices of
a deal correlated), and its bootstrap over deals covers them (the inverse Hessian does not); the most informative
reference; the promotion rules (manual and automatic); the transitivity flag on an exploiter; the retroactive fit of
ladder-mode records; the pool watcher end to end on a fake run whose games are simulated; the setting; the commands.
No process is started (the watcher's items are measured in-process, its children's command lines checked)."""
import json
import math
import os
import random
import sys
import time
from statistics import NormalDist

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import mtgz.az  # noqa: E402,F401
from mtgz.az import cells_fit as CF  # noqa: E402
from mtgz.az import yardsticks as Y  # noqa: E402

np = pytest.importorskip('numpy')
D, S, N = Y.CELL_DECKS
K = 400 / math.log(10)
PHI = NormalDist().cdf


def sig(x):
    return 1.0 / (1.0 + math.exp(-x))


# ---------------------------------------------------------------- simulated worlds (game tuples)

class Deals:
    """The deals' uniforms: per deal a shared normal z, per choice its own; U = Phi(rho z + sqrt(1 - rho^2) e) is
    uniform (so P(win) is exactly the model's p for every game) and shared by every generation playing that deal and
    choice (common random numbers), the two choices correlated by rho^2."""

    def __init__(self, salt, rho=0.7):
        self.salt, self.rho, self.u = salt, rho, {}

    def __call__(self, deal, c):
        if deal not in self.u:
            rng = random.Random(f'{self.salt}|{deal}')
            z = rng.gauss(0, 1)
            self.u[deal] = [PHI(self.rho * z + math.sqrt(1 - self.rho ** 2) * rng.gauss(0, 1)) for _ in (0, 1)]
        return self.u[deal][c]


def sim_games(deals, truth, design, m=0.6, c=0.3, mirror=False):
    """design [(C, R, units)]: C against R on deals (0, gid) for gid < 2 units (even: C pilots X), both choices."""
    out = []
    for src, (C, R, units) in enumerate(design):
        for g in range(2 * units):
            gx, gy = (C, R) if g % 2 == 0 else (R, C)
            for ch in (0, 1):
                s = 1 if ch == 0 else -1
                p = sig((0 if mirror else m) + truth[gx][0] - truth[gy][1 if not mirror else 0] + c * s)
                out.append((gx, gy, s, float(deals((0, g), ch) < p), (0, g), src))
    return out


TRUTH = {'g0': (0.0, 0.0), 'g1': (0.3, 0.2), 'g2': (0.6, 0.5), 'g3': (0.9, 0.7), 'g4': (1.2, 1.0)}
# the pool's shape: everything against g0 early, then g2 promoted: main against the newest, continuity the older one
DESIGN = [('g0', 'g0', 200), ('g1', 'g0', 200), ('g2', 'g0', 150), ('g2', 'g1', 100), ('g2', 'g2', 200),
          ('g3', 'g2', 150), ('g3', 'g0', 50), ('g4', 'g2', 150), ('g4', 'g3', 100), ('g3', 'g3', 200)]


def test_the_fit_recovers_known_skills_and_its_bootstrap_covers_them():
    """100 replications of a pool-shaped design on 400 correlated deals: the estimates centre on the truth (mean z
    near 0), the bootstrap's 95% intervals cover ~95% of the 800 (skill, replication) cases and its se matches the
    estimates' spread; the inverse Hessian, which takes the coupled games as independent, overstates the se of a
    rating measured straight against the anchor and understates the chained ones' by about half."""
    est, sb, sm = {}, {}, {}
    cover_b = cover_m = tot = 0
    zs = []
    for rep in range(100):
        f = CF.fit_pair(sim_games(Deals(rep), TRUTH, DESIGN), ('X', 'Y'), 'g0', boot=150, seed=rep)
        for p in ('g1', 'g2', 'g3', 'g4'):
            for role, j in (('X', 0), ('Y', 1)):
                i = f['idx'][(role, p)]
                e, b, m = f['theta'][i], float(np.std(f['reps'][:, i], ddof=1)), math.sqrt(f['cov'][i, i])
                est.setdefault((role, p), []).append(e)
                sb.setdefault((role, p), []).append(b)
                sm.setdefault((role, p), []).append(m)
                tot += 1
                cover_b += abs(e - TRUTH[p][j]) < 1.96 * b
                cover_m += abs(e - TRUTH[p][j]) < 1.96 * m
                zs.append((e - TRUTH[p][j]) / b)
    assert 0.91 <= cover_b / tot <= 0.985, cover_b / tot
    assert abs(float(np.mean(zs))) < 0.2, np.mean(zs)
    for k in est:
        assert 0.8 < np.mean(sb[k]) / np.std(est[k], ddof=1) < 1.25, (k, np.mean(sb[k]), np.std(est[k], ddof=1))
    assert np.mean(sm[('X', 'g1')]) > 1.2 * np.std(est[('X', 'g1')], ddof=1)          # (the pairing it misses)
    assert np.mean(sm[('X', 'g4')]) < 0.7 * np.std(est[('X', 'g4')], ddof=1)          # (the shared deals it misses)
    assert cover_m / tot < 0.85


def test_a_mirror_has_one_skill_and_no_matchup_offset():
    truth = {'g0': (0.0,), 'g1': (0.4,), 'g2': (0.9,)}
    design = [('g1', 'g0', 300), ('g2', 'g1', 300), ('g2', 'g0', 100)]
    f = CF.fit_pair(sim_games(Deals('m'), truth, design, c=0.5, mirror=True), ('X', 'X'), 'g0', boot=100)
    assert 'm' not in f['idx'] and set(k for k in f['idx'] if isinstance(k, tuple)) == {('X', 'g1'), ('X', 'g2')}
    for p in ('g1', 'g2'):
        i = f['idx'][('X', p)]
        assert abs(f['theta'][i] - truth[p][0]) < 3 * np.std(f['reps'][:, i], ddof=1)
    assert abs(f['theta'][f['idx']['c']] - 0.5) < 0.15


def test_the_deals_of_a_sequential_row_follow_the_batch_schedule():
    assert CF._clusters((D, S), 10, None, 3) == [(10, 0), (10, 1), (10, 2)]
    assert CF._clusters((D, S), 10, 4, 6) == [(10, 0), (10, 1), (10, 2), (10, 3), (10 + Y.SEQ_SEED, 0),
                                              (10 + Y.SEQ_SEED, 1)]
    assert CF._clusters((D, D), 10, 4, 3) == [(10, 0), (10, 1), (10 + Y.SEQ_SEED, 0)]   # (a mirror's half batches)
    g = CF.match_games((D, S), 'a', 'b', {'0': '10', '1': '01'}, 5, None, 7)
    # gid 0: A pilots X; gid 1: B pilots X (A's result flipped to X's); chooser 0: X's side chose
    assert g == [('a', 'b', 1, 1.0, (5, 0), 7), ('b', 'a', 1, 1.0, (5, 1), 7),
                 ('a', 'b', -1, 0.0, (5, 0), 7), ('b', 'a', -1, 0.0, (5, 1), 7)]


# ---------------------------------------------------------------- the pool's choices

def test_the_most_informative_reference_and_the_continuity_one():
    truth = {'g0': (0.0, 0.0), 'g2': (1.0, 1.0), 'g4': (2.0, 2.0)}
    design = [('g0', 'g0', 150), ('g2', 'g0', 150), ('g4', 'g2', 150), ('g2', 'g2', 150), ('g4', 'g4', 150)]
    f = CF.fit_pair(sim_games(Deals('i'), truth, design, m=0.0, c=0.0), (D, S), 'g0')
    fits = {(D, S): f}
    pool = [{'label': 'gen0', 'sha': 'g0'}, {'label': 'g2', 'sha': 'g2'}, {'label': 'g4', 'sha': 'g4'}]
    assert CF.choose(fits, (D, S), 'g4', pool)[0] == 'g4'                       # (itself: 0.5)
    assert CF.choose(fits, (D, S), 'g2', pool)[0] == 'g2'
    lab, seen = CF.choose(fits, (D, S), 'g0', pool)
    assert lab == 'gen0' and seen['g4'][1] < seen['g2'][1] < seen['gen0'][1]
    pred, _ = CF.predict(fits, (D, S), 'g4', 'g0')
    assert pred[Y.cell_id(D, S)] > 0.8 and pred[Y.cell_id(S, D)] > 0.8
    assert CF.continuity(pool, 'g4') == 'g2' and CF.continuity(pool, 'g2') == 'gen0'
    assert CF.continuity(pool, 'gen0') is None
    refs = [{'label': 'gen0', 'since': 0}, {'label': 'a', 'since': 32}, {'label': 'b', 'since': 64},
            {'label': 'c', 'since': 96}, {'label': 'd', 'since': 128}]
    assert [r['label'] for r in CF.active_refs(refs, 200, 3)] == ['gen0', 'b', 'c', 'd']   # (g0 is permanent)
    assert [r['label'] for r in CF.active_refs(refs, 100, 3)] == ['gen0', 'a', 'b', 'c']
    assert [r['label'] for r in CF.active_refs(refs, 10, 3)] == ['gen0']
    ys = Y.normalize({'bo1_games': 0, 'blunders': False, 'cells_games': 400, 'cells_refs': ['iter0'],
                      'cells_mode': 'pool'})
    assert CF.split_games(ys, True) == (300, 100, 0) and CF.split_games(ys, False) == (400, 0, 0)
    seq = dict(ys, cells_target_se=30, cells_batch=40)
    assert CF.split_games(seq, True) == (320, 80, 0)                             # (whole batches)
    assert CF.split_games(dict(ys, cells_main_share=1.0), True) == (400, 0, 0)
    # generation 0's fixed share when it is neither member (cells_gen0_share 0.1): taken first, the main leg's share
    # of the whole, the continuity the rest; at least one batch
    assert CF.split_games(ys, True, gen0=True) == (300, 60, 40)
    assert CF.split_games(seq, True, gen0=True) == (320, 40, 40)
    assert CF.split_games(dict(seq, cells_gen0_share=0.01), True, gen0=True) == (320, 40, 40)
    assert CF.split_games(dict(ys, cells_gen0_share=0.0), True, gen0=True) == (300, 100, 0)
    assert CF.split_games(dict(seq, cells_games=40), True, gen0=True) == (40, 0, 0)     # (one batch: no room)


def _fit_with(cells_n, cells_r):
    return {'gens': [{'sha': 'n', 'complete': True, 'cells': {c: {'elo': v} for c, v in cells_n.items()}},
                     {'sha': 'r', 'complete': True, 'cells': {c: {'elo': v} for c, v in cells_r.items()}}]}


def test_the_promotion_rules_cadence_and_threshold_both_ways(tmp_path):
    ref = tmp_path / 'r.pt'
    ref.write_bytes(b'r')
    sha_r = Y.sha12(str(ref))
    refs = [{'label': 'gen0', 'since': 0, 'path': str(tmp_path / 'none.pt')},
            {'label': 'g32', 'since': 32, 'path': str(ref)}]
    ys = {'cells_promote': 0.8, 'cells_promote_every': 64}
    fit = _fit_with({'a|b': 100.0, 'b|a': 50.0}, {'a|b': 0.0, 'b|a': 0.0})
    fit['gens'][1]['sha'] = sha_r
    assert CF.promote_rule(fit, refs, 64, 'n', ys) is None                     # (+100: 0.64; 32 iterations)
    assert CF.promote_rule(fit, refs, 96, 'n', ys)['why'] == 'cadence'
    assert CF.promote_rule(fit, refs, 96, 'n', dict(ys, cells_promote_every=0)) is None
    up = _fit_with({'a|b': 300.0, 'b|a': 0.0}, {'a|b': 0.0, 'b|a': 0.0})
    up['gens'][1]['sha'] = sha_r
    assert CF.promote_rule(up, refs, 48, 'n', ys) == {'iter': 48, 'why': 'threshold', 'cells': ['a|b'], 'vs': 'g32'}
    # below 0.20 against the newest reference: regressed - never promoted, not even on the cadence
    down = _fit_with({'a|b': -300.0, 'b|a': 0.0}, {'a|b': 0.0, 'b|a': 0.0})
    down['gens'][1]['sha'] = sha_r
    assert CF.promote_rule(down, refs, 48, 'n', ys) is None
    assert CF.promote_rule(down, refs, 96, 'n', ys) is None
    assert CF.against_newest(down, refs, 96, 'n', ys) == ('g32', [], ['a|b'])
    mixed = _fit_with({'a|b': -300.0, 'b|a': 300.0}, {'a|b': 0.0, 'b|a': 0.0})
    mixed['gens'][1]['sha'] = sha_r
    assert CF.promote_rule(mixed, refs, 48, 'n', ys) is None                   # (one cell up, one down: regressed)
    inc = _fit_with({'a|b': 300.0}, {'a|b': 0.0})
    inc['gens'][0]['complete'] = False
    inc['gens'][1]['sha'] = sha_r
    assert CF.promote_rule(inc, refs, 200, 'n', ys) is None                    # (only a fully rated checkpoint)
    assert CF.promote_rule(up, refs, 32, 'n', ys) is None                      # (not newer than the newest)


# ---------------------------------------------------------------- a fake run with simulated games

def _gen(path):
    with open(path, 'rb') as f:
        return int(f.read().split()[1])


class World:
    """Known skills by iteration (logits): a[i, X|Y] = slope[X|Y] x i / 16; matchup offsets and chooser edges per
    pair; an optional exploiter (generation e with a bonus against generation v only). A game uses its deal's shared
    uniform with probability `couple` (the CRN coupling), else its own (the players' decisions diverged)."""

    def __init__(self, salt='w', exploit=None, rho=0.6, couple=0.6):
        self.deals = Deals(salt, rho)
        self.salt, self.couple = salt, couple
        self.drop = {}                          # {generation: logits lost in every cell} (a regression)
        self.slope = {Y.cell_id(x, y): 0.10 + 0.03 * (i * 3 + j) for i, x in enumerate(Y.CELL_DECKS)
                      for j, y in enumerate(Y.CELL_DECKS)}
        self.m = {(D, S): 0.8, (D, N): -0.5, (S, N): 0.3}
        self.c = {(D, D): 0.4, (D, S): 0.2, (D, N): 0.0, (S, S): -0.2, (S, N): 0.3, (N, N): 0.1}
        self.exploit = exploit

    def a(self, i, x, y):
        return self.slope[Y.cell_id(x, y)] * i / 16.0 - self.drop.get(i, 0.0)

    def play(self, a, b, pair, games, ys, seat, log=print, stop=None):
        ga, gb = _gen(a), _gen(b)
        x, y = pair
        out = {'results': {}, 'unfinished': 0, 'select': 'sim'}
        for j in Y.pair_jobs(pair, games, ys):
            s = 1 if j['chooser'] == 0 else -1
            res = []
            for g in range(j['games']):
                gx, gy = (ga, gb) if g % 2 == 0 else (gb, ga)
                lo = self.m.get(tuple(pair), 0.0) + self.a(gx, x, y) - self.a(gy, y, x) + self.c[tuple(pair)] * s
                if self.exploit:
                    e, v, bonus = self.exploit
                    lo += bonus if (gx, gy) == (e, v) else -bonus if (gx, gy) == (v, e) else 0.0
                rng = random.Random(f"{self.salt}|{pair}|{j['seed']}|{g}|{j['chooser']}|{gx}|{gy}")
                u = self.deals((tuple(pair), j['seed'], g), j['chooser']) if rng.random() < self.couple else rng.random()
                yx = float(u < sig(lo))
                res.append(yx if g % 2 == 0 else 1.0 - yx)
            out['results'][str(j['chooser'])] = res
        return out


def _run(tmp_path, iters, ys):
    run = tmp_path / 'run'
    (run / 'models').mkdir(parents=True)
    cfg = {'preset': 'delver-mirror-bo3-dealt', 'eval_every': 16,
           'yardsticks': dict({'bo1_games': 0, 'blunders': False, 'cells_refs': ['iter0'], 'cells_device': 'cpu',
                               'cells_boot': 0, 'cells_fit_boot': 40}, **ys)}
    (run / 'config.json').write_text(json.dumps(cfg))
    for i in iters:
        p = run / 'models' / f'iter_{i:04d}.pt'
        p.write_bytes(f'net {i}'.encode())
        os.utime(p, (time.time() - 3600, time.time() - 3600))
    return str(run), cfg


@pytest.fixture
def sim(monkeypatch):
    """The watcher's measurements in-process on a World's games (no network loaded: the seat and seat keys stubbed)."""
    monkeypatch.setattr(Y, 'cells_seat', lambda run: {'select': 'sim'})
    monkeypatch.setattr(Y, '_seat_keys', lambda ref: {})
    worlds = []

    def use(world):
        worlds.append(world)
        monkeypatch.setattr(Y, 'play_pair_local', world.play)
    return use


def _execute(run, cfg, ys, it):
    if it['kind'] == 'cells_base':
        return Y.measure_cells_base(run, it['ref'], it['pair'], it['games'], cfg, ys, log=lambda m: None)
    if it['kind'] == Y.FIT_KIND:
        return Y.measure_cells_fit(run, it['iter'], cfg, ys, log=lambda m: None)
    return Y.measure_cells(run, it['iter'], it['ref'], it['pair'], cfg, ys, kind=it['kind'], log=lambda m: None,
                           games=it.get('games') if it.get('role') else None, role=it.get('role'))


def drive(run, cfg, ys=None):
    """The watcher's lanes, in-process, until nothing is due -> the items measured, in order."""
    done = []
    while True:
        ys_ = ys or Y.of_run(run)
        moved = False
        for lane in Y.lanes(run, cfg, ys_):
            it = Y.pending(run, cfg, ys_, lane, settle=0)
            if it is None:
                continue
            Y.append_row(run, _execute(run, cfg, ys_, it))
            done.append(it)
            moved = True
        if not moved:
            return done


def _check_recovery(fit, world, its, k=3.5):
    """Every rated cell of the listed generations within k bootstrap se of the truth (in Elo)."""
    worst = 0.0
    for g in fit['gens']:
        if g['iter'] not in its:
            continue
        for cid, c in g['cells'].items():
            x, y = cid.split('|')
            z = abs(c['elo'] - K * world.a(g['iter'], x, y)) / max(c['se'], 1e-9)
            worst = max(worst, z)
    assert worst < k, worst
    return worst


def test_the_pool_watcher_end_to_end_with_automatic_promotion(tmp_path, sim):
    world = World('pool')
    sim(world)
    run, cfg = _run(tmp_path, range(0, 129, 16), {
        'cells_games': 120, 'cells_gen_games': 40, 'cells_mode': 'pool', 'cells_auto_promote': True,
        'cells_promote_every': 32, 'cells_pool_active': 2, 'cells_main_share': 0.75})
    ys = Y.of_run(run)
    assert Y.lanes(run, cfg, ys) == ['cpu_bo1', 'fit']
    done = drive(run, cfg)
    kinds = [(d['kind'], d.get('iter'), d['ref'] if 'ref' in d else None) for d in done]
    assert kinds[0] == ('cells_base', None, 'gen0')                    # (g0's baselines before anything)
    rows = Y.read_rows(run)
    fits = {r['iter']: r for r in rows if r['kind'] == Y.FIT_KIND}
    assert sorted(fits) == list(range(16, 129, 16))
    lad = json.load(open(Y.ladder_path(run)))['refs']
    assert [(e['label'], e['since'], e['from_ref']) for e in lad] == [('g32', 32, 'pool-fit'), ('g64', 64, 'pool-fit'),
                                                                     ('g96', 96, 'pool-fit'), ('g128', 128, 'pool-fit')]
    assert fits[32]['promote'] == {'iter': 32, 'why': 'cadence', 'cells': fits[32]['promote']['cells'], 'vs': 'gen0',
                                   'auto': True, 'label': 'g32', 'elo': lad[0]['elo'], 'elo_se': lad[0]['elo_se']}
    assert fits[48]['promote'] is None
    # each promoted reference's baselines, once, before the first checkpoint that has it in its pool
    bases = [(d['ref'], tuple(d['pair'])) for d in done if d['kind'] == 'cells_base']
    assert len(bases) == len(set(bases)) and {r for r, _ in bases} >= {'gen0', 'g32', 'g64', 'g96'}
    first = {r: min(i for i, d in enumerate(done) if d['kind'] == 'cells' and d['ref'] == r) for r in ('g32', 'g64')}
    for r, i in first.items():
        assert all(any(d['kind'] == 'cells_base' and d['ref'] == r and tuple(d['pair']) == p for d in done[:i])
                   for p in [(D, S), (D, N), (S, N)])
    # later checkpoints: a main leg (90 games) against an active reference, a continuity leg against the next older,
    # and generation 0's fixed share (12) when it is neither (cells_gen0_share 0.1)
    legs = [r for r in rows if r['kind'] == 'cells' and r['iter'] == 112]
    assert {r['base_games'] for r in legs} == {120}
    pool112 = ['gen0', 'g64', 'g96']                                  # (pool_active 2 + the anchor)
    assert fits[112]['pool'] == pool112
    for r in legs:
        assert r['ref'] in pool112
    for pair, p in fits[112]['plan'].items():
        g = p['games']
        if p['main'] == 'gen0':
            assert p['cont'] is None and p['gen0'] is None and g == {'gen0': 120}
        elif p['cont'] == 'gen0':
            assert p['gen0'] is None and (g[p['main']], g['gen0']) == (90, 30)
        else:
            assert (p['main'], p['cont'], p['gen0']) == ('g96', 'g64', 'gen0')
            assert (g['g96'], g['g64'], g['gen0']) == (90, 18, 12)
    assert any(p['gen0'] for p in fits[112]['plan'].values())         # (the world's steady gains: g96 is the main)
    # the record: every cell rated, the anchored number, the whole curve in gens, the flags quiet (no exploiter)
    f = fits[128]
    assert f['complete'] and set(f['cells']) == {Y.cell_id(x, y) for x in Y.CELL_DECKS for y in Y.CELL_DECKS}
    assert f['anchored']['elo'] == pytest.approx(1000 + f['overall']['elo'], abs=0.11)
    assert [g['iter'] for g in f['gens']] == list(range(0, 129, 16))
    assert f['flags']['transitivity'] == [] and f['method']['boot'] == 40
    fit = CF.fit_run(run, cfg, ys, boot=100)
    _check_recovery(fit, world, range(16, 129, 16))
    assert sum(t['flag'] for t in fit['transitivity']) <= 1, [t for t in fit['transitivity'] if t['flag']]
    st = Y.status(run, cfg, ys)
    assert st['cells']['fit']['iter'] == 128 and st['cells']['missing'] == [] and st['cells']['failed'] == []


def test_manual_promotion_suggests_and_cells_freeze_does_it(tmp_path, sim, capsys):
    import run_yardsticks as RY
    sim(World('manual'))
    run, cfg = _run(tmp_path, (0, 16, 32, 48), {'cells_games': 40, 'cells_mode': 'pool', 'cells_promote_every': 32})
    ys = Y.of_run(run)
    drive(run, cfg)
    assert not os.path.exists(Y.ladder_path(run))                     # (auto off: nothing written)
    rep = Y.cells_report(run, cfg, ys)
    assert rep['promote'] == {'iter': 48, 'ref': 'gen0', 'cells': rep['promote']['cells'], 'why': 'cadence'}
    assert RY.main(['cells', run, '--boot', '20']) == 0
    out = capsys.readouterr().out
    assert 'SUGGESTED: a new reference - iteration 48 (cadence' in out and 'joint fit (v1)' in out
    assert 'generation 48' in out and 'anchored' in out
    assert RY.main(['cells-freeze', run, '--iter', '48']) == 0
    e = json.load(open(Y.ladder_path(run)))['refs'][0]
    assert (e['label'], e['since'], e['from_ref']) == ('g48', 48, 'pool-fit') and e['elo'] > 1000
    with pytest.raises(SystemExit, match='starts at'):
        RY.main(['cells-freeze', run, '--iter', '32'])
    assert Y.cells_report(run, cfg, ys)['promote'] is None


def test_the_transitivity_flag_catches_an_exploiter_of_a_reference(tmp_path, sim):
    """Generation 48 gains 1.5 logits against generation 0 only (it exploits the reference); its ratings come from
    its games against generation 0, so they predict a far higher score against generation 32 than the direct
    gen-over-gen games show: flagged in every cell, with the largest |z| of the run (its inflated rating also leans on
    32's through their direct games, so 32's own check can trip: the flag says where the ratings stop being
    transitive). The same world without the exploiter flags nothing."""
    sim(World('exploit', exploit=(48, 0, 1.5)))
    run, cfg = _run(tmp_path / 'x', (0, 16, 32, 48), {'cells_games': 200, 'cells_gen_games': 200})
    drive(run, cfg)                                                  # (the ladder mode: everything against gen0)
    fit = CF.fit_run(run, cfg, Y.of_run(run), boot=60)
    ex = [t for t in fit['transitivity'] if (t['iter'], t['prev']) == (48, 32)]
    rest = [t for t in fit['transitivity'] if t not in ex]
    assert len(ex) == 9 and all(t['flag'] and t['pred'] > t['obs'] for t in ex), ex
    assert min(abs(t['z']) for t in ex) > max(abs(t['z']) for t in rest)
    g48 = CF.gen_of(fit, Y.sha12(Y.model_path(run, 48)))
    assert len(g48['flags']['transitivity']) == 9
    sim(World('exploit'))
    run, cfg = _run(tmp_path / 'h', (0, 16, 32, 48), {'cells_games': 200, 'cells_gen_games': 200})
    drive(run, cfg)
    honest = CF.fit_run(run, cfg, Y.of_run(run), boot=60)['transitivity']
    assert len(honest) == 27 and not any(t['flag'] for t in honest) and max(abs(t['z']) for t in honest) < 2.5


def test_the_fit_works_retroactively_on_ladder_mode_records_and_the_switch_remeasures_nothing(tmp_path, sim):
    world = World('ladder')
    sim(world)
    run, cfg = _run(tmp_path, (0, 16, 32, 48, 64), {'cells_games': 160, 'cells_gen_games': 80})
    drive(run, cfg)
    ys = Y.of_run(run)
    rows = Y.read_rows(run)
    assert not any(r['kind'] == Y.FIT_KIND or r.get('role') for r in rows)       # (ladder rows, untouched)
    fit = CF.fit_run(run, cfg, ys, boot=100)
    assert fit['missing_bases'] == [] and fit['anchor']['sha'] == Y.sha12(Y.model_path(run, 0))
    _check_recovery(fit, world, (16, 32, 48, 64))
    # the matchup offsets and chooser edges come out too
    assert abs(fit['pairs'][f'{D},{S}']['m'][0] - K * 0.8) < 3.5 * fit['pairs'][f'{D},{S}']['m'][1]
    # switched to the pool: the measured checkpoints keep their rows; only their fits are wanted (and a new
    # checkpoint is planned against the pool)
    pool = dict(ys, cells_mode='pool')
    w = Y.wanted(run, cfg, pool, settle=0)
    assert {x['kind'] for x in w if Y.key_of(x) not in Y.tally(rows)[0]} == {Y.FIT_KIND}
    done = drive(run, cfg, pool)
    assert [(d['kind'], d['iter']) for d in done] == [(Y.FIT_KIND, i) for i in (16, 32, 48, 64)]
    f = [r for r in Y.read_rows(run) if r['kind'] == Y.FIT_KIND][-1]
    assert f['plan'][f'{D},{S}'] == {'main': 'gen0', 'cont': None, 'gen0': None, 'games': {'gen0': 160}}


def test_a_pool_leg_is_judged_against_the_first_games_of_the_references_baseline(tmp_path, sim):
    sim(World('prefix'))
    run, cfg = _run(tmp_path, (0, 16), {'cells_games': 40, 'cells_mode': 'pool'})
    ys = Y.of_run(run)
    Y.append_row(run, Y.measure_cells_base(run, 'gen0', (D, S), 40, cfg, ys, log=lambda m: None))
    full = Y.measure_cells(run, 16, 'gen0', (D, S), cfg, ys, log=lambda m: None)
    leg = Y.measure_cells(run, 16, 'gen0', (D, S), cfg, ys, log=lambda m: None, games=20, role='main')
    assert leg['base'] == full['base'] and leg['games'] == 40 and full['games'] == 80
    assert leg['results']['0'] == full['results']['0'][:20]          # (the same deals: the same first games)
    base = Y.load_base(Y.cache_dir(run, ys), full['base'])
    bu = Y.cell_units((D, S), {int(c): Y.res_decode(r)[:20] for c, r in base['results'].items()})
    assert leg['cells'][Y.cell_id(D, S)]['base'] == round(sum((x + y) / 2 for x, y in bu[Y.cell_id(D, S)]) / 10, 4)
    with pytest.raises(ValueError, match='baseline of'):
        Y.measure_cells(run, 16, 'gen0', (D, S), cfg, ys, games=60, role='main')


# ---------------------------------------------------------------- the setting, the commands, the take-over

def test_the_pool_settings_are_checked_and_off_by_default():
    d = Y.normalize({'bo1_games': 0, 'blunders': False, 'cells_games': 40, 'cells_refs': ['iter0']})
    assert (d['cells_mode'], d['cells_pool_active'], d['cells_promote_every'], d['cells_auto_promote'],
            d['cells_main_share'], d['cells_fit_boot']) == ('ladder', 3, 64, False, 0.75, 200)
    assert not Y.cells_pool(d) and Y.cells_pool(dict(d, cells_mode='pool'))
    base = {'bo1_games': 0, 'blunders': False, 'cells_games': 40, 'cells_refs': ['iter0']}
    for bad, why in (({'cells_mode': 'tree'}, 'cells_mode'),
                     ({'cells_pool_active': 0}, 'cells_pool_active'),
                     ({'cells_main_share': 0}, 'cells_main_share'),
                     ({'cells_main_share': 1.5}, 'cells_main_share'),
                     ({'cells_fit_boot': -1}, 'cells_fit_boot'),
                     ({'cells_auto_promote': 1}, 'right type')):
        with pytest.raises(ValueError, match=why):
            Y.normalize(dict(base, **bad))
    with pytest.raises(ValueError, match='pool needs cells_games'):
        Y.normalize({'bo1_games': 0, 'blunders': False, 'cells_gen_games': 4, 'cells_mode': 'pool'})


def test_cells_fit_refits_from_the_records_only(tmp_path, sim, capsys, monkeypatch):
    import run_yardsticks as RY
    sim(World('cli'))
    run, cfg = _run(tmp_path, (0, 16, 32), {'cells_games': 40, 'cells_gen_games': 20})
    drive(run, cfg)
    monkeypatch.setattr(Y, 'play_pair_local', lambda *a, **k: pytest.fail('cells-fit plays no game'))
    n = len(Y.read_rows(run))
    assert RY.main(['cells-fit', run, '--json', '--boot', '20']) == 0
    d = json.loads(capsys.readouterr().out)
    assert d['fit_v'] == CF.FIT_V and [g['iter'] for g in d['gens']] == [0, 16, 32]
    assert d['method']['se'].startswith('bootstrap') and d['method']['boot'] == 20
    assert len(Y.read_rows(run)) == n                                   # (nothing written)
    assert RY.main(['cells', run, '--fit', '--boot', '0']) == 0
    out = capsys.readouterr().out
    assert 'iteration 32 against gen0' in out and 'joint fit (v1)' in out and 'inverse Hessian' in out


def test_the_pool_items_childrens_command_lines(tmp_path, sim):
    import run_yardsticks as RY
    sim(World('cmd'))
    run, cfg = _run(tmp_path, (0, 16, 32), {'cells_games': 40, 'cells_mode': 'pool'})
    ys = Y.of_run(run)
    w = Y.wanted(run, cfg, ys, settle=0)
    f32 = next(x for x in w if x['kind'] == Y.FIT_KIND and x['iter'] == 32)
    assert (Y.FIT_KIND, 16) in f32['after']                              # (fits in order: promotions in order)
    wt = RY.Watcher(run, once=True, log=lambda m: None)
    cell = next(x for x in w if x['kind'] == 'cells')
    assert cell['role'] == 'main' and cell['games'] == 40                # (only the anchor in the pool: every game)
    assert wt.one_cmd(cell)[-4:] == ['--games', '40', '--role', 'main']
    fit = next(x for x in w if x['kind'] == Y.FIT_KIND)
    assert fit['lane'] == 'fit' and len(fit['after']) == 6             # (the first checkpoint: no earlier fit)
    assert wt.one_cmd(fit)[3:] == [run, '--kind', 'cells_fit', '--iter', '16']
    assert RY.item_key(fit)[0] == 'cells_fit_0016'
    assert Y.pending(run, cfg, ys, 'fit', settle=0) is None              # (waits for its measurements)
    st = Y.status(run, cfg, ys)
    assert any(m['kind'] == Y.FIT_KIND for m in st['cells']['missing']) and st['missing'] == []


def test_take_over_stops_only_the_runs_own_companion_watcher(tmp_path, monkeypatch):
    import run_yardsticks as RY
    from zero import runctl
    run = str(tmp_path)
    killed, held = [], {'pid': 4242}
    monkeypatch.setattr(RY, 'take_lock', lambda r: held['pid'] is None)
    monkeypatch.setattr(RY, 'lock_owner', lambda r: held['pid'])

    def kill(pid):
        killed.append(pid)
        held['pid'] = None
    monkeypatch.setattr(RY, 'kill_pid_tree', kill)
    state = {'companion': {'pid': 4242, 'cmd': ['python', 'x/tools/run_yardsticks.py', 'watch', run, '--supervised']}}
    monkeypatch.setattr(runctl, 'read_state', lambda r, name: state)
    w = RY.Watcher(run, take_over=True, log=lambda m: None)
    assert w.seize(wait_s=0.2) and killed == [4242]
    held['pid'], killed[:] = 5151, []                                    # (a hand watcher, not the companion)
    assert not w.seize(wait_s=0.2) and killed == []
    state['companion'] = {'pid': 5151, 'cmd': ['python', 'publisher.py']}
    assert not w.seize(wait_s=0.2) and killed == []
    with pytest.raises(SystemExit):
        RY.main(['watch', run, '--supervised', '--take-over'])


def test_the_fit_lane_outlives_the_other_lanes_when_finishing(tmp_path, monkeypatch):
    """--once / the run's end: a lane with nothing due ends, but the fit lane's items wait on the other lanes'
    measurements, so it ends only after them."""
    import threading
    import run_yardsticks as RY
    ys = Y.normalize({'bo1_games': 0, 'blunders': False, 'cells_games': 8, 'cells_refs': ['iter0'],
                      'cells_mode': 'pool'})
    w = RY.Watcher(str(tmp_path), once=True, log=lambda m: None, poll=0.01)
    w.tick = 0.01
    monkeypatch.setattr(w, 'settings', lambda: ({}, ys))
    monkeypatch.setattr(Y, 'pending', lambda *a, **k: None)
    w.alive = {'fit', 'cpu_bo1'}
    t = threading.Thread(target=w.lane, args=('fit',), daemon=True)
    t.start()
    t.join(0.3)
    assert t.is_alive()
    w.alive.discard('cpu_bo1')
    t.join(2)
    assert not t.is_alive() and w.alive == set()


def test_a_checkpoint_that_regressed_against_the_newest_reference_is_flagged_and_never_promoted(tmp_path, sim):
    """Generation 64 loses 3 logits in every cell: below 0.20 against g32 (the newest reference), so it is flagged
    (`below_ref`, its cells in `regressed`) and not promoted although the cadence is due; the next checkpoint is."""
    world = World('regress')
    world.drop = {64: 3.0}
    sim(world)
    run, cfg = _run(tmp_path, range(0, 81, 16), {
        'cells_games': 40, 'cells_mode': 'pool', 'cells_auto_promote': True, 'cells_promote_every': 32,
        'cells_pool_active': 2})
    drive(run, cfg)
    fits = {r['iter']: r for r in Y.read_rows(run) if r['kind'] == Y.FIT_KIND}
    f = fits[64]
    assert f['promote'] is None and f['flags']['below_ref']['vs'] == 'g32'
    assert len(f['flags']['below_ref']['cells']) >= 6
    assert set(f['flags']['below_ref']['cells']) <= set(f['flags']['regressed'])
    assert fits[80]['promote']['why'] == 'cadence' and fits[80]['promote']['label'] == 'g80'
    assert [e['label'] for e in json.load(open(Y.ladder_path(run)))['refs']] == ['g32', 'g80']
