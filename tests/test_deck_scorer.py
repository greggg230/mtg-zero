"""The deck choice by the evaluator (docs/ENGINE.md 112): the export format and its numpy reference against torch, the
engine's scorer against both, the choice through the bindings and the driver (the game log names the evaluator and
`--check` rebuilds the choice), and the run's side - the versions, the rolling retrain's scheduling and reports, the
deck-choice evaluation's spec, the presets."""
import json
import os

import numpy as np
import pytest

import mtgz.az as P
from mtgz import _engine as E
from mtgz.az import games as G
from mtgz.az import run
from mtgz.az.infer import drive
from mtgz.sealed import engine as SE
from mtgz.sealed import evaluator as EV
from mtgz.sealed import games as GL
from mtgz.sealed import rolling as RO
from mtgz.sealed import scorer as S

FIXTURES = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'engine', 'tests', 'data')
V2 = 'E:/mtg-zero/s4a/v2_time'


@pytest.fixture(scope='module')
def exported(tmp_path_factory):
    """Random evaluators of every kind, saved (.pt) and exported (`Evaluator.export`): {kind: (pt, json, sha256)}."""
    d = tmp_path_factory.mktemp('scorer')
    out = {}
    for kind, kw in (('linear', {}), ('set', {}), ('pair', {'base': 'set', 'dim': 2})):
        e = S.random_evaluator(kind, seed=11, members=3, d=5, k=2, hidden=6, **kw)
        pt, js = str(d / f'evaluator_{kind}.pt'), str(d / f'{kind}.json')
        e.save(pt)
        e.export(js)
        out[kind] = (pt, js, S.sha256_file(js))
    return out


def test_the_engines_fixtures_are_reproducible(tmp_path):
    """engine/tests/data/scorer_*.json: `python -m mtgz.sealed.scorer fixtures` makes them byte for byte."""
    got = S.fixtures(str(tmp_path))
    for name, sha in got.items():
        with open(os.path.join(FIXTURES, name), 'rb') as f:
            assert S.hashlib.sha256(f.read()).hexdigest() == sha, name


def test_the_engine_scores_the_export_as_its_reference_and_torch(exported):
    decks = S.sample_decks(300, seed=4)
    for kind, (pt, js, sha) in exported.items():
        r = S.compare(js, decks, pt)
        assert S.failed(r) is None and r['reference_engine'] < 1e-12 and r['torch_engine'] < 1e-6, r
        assert r['score_sd'] > 0.01, 'random weights spread the scores'
        info = S.engine_info(js)
        assert (info['sha256'], info['kind'], info['members']) == (sha, kind, 3)
        assert info['field_decks'] == (24 if kind == 'pair' else 0)
        # one evaluator, one file
        EV.Evaluator.load(pt).export(js + '.again')
        assert S.sha256_file(js + '.again') == sha
    with pytest.raises(ValueError, match='Paradox Haze|not a card'):
        E.sealed_scores(exported['set'][1], json.dumps([['Paradox Haze'] * 40]))
    with pytest.raises(ValueError, match='No Such Card'):
        E.sealed_scores(exported['set'][1], json.dumps([['No Such Card'] * 40]))


@pytest.mark.skipif(not os.path.exists(os.path.join(V2, 'evaluator_set.json')), reason='the v2 evaluators (E:) are private data')
def test_the_v2_evaluators_score_exactly():
    """eval-v2's trained evaluators (ENGINE.md 111; their exports): the engine against the reference and torch."""
    decks = S.sample_decks(2000, seed=0)
    for kind in S.KINDS:
        r = S.compare(os.path.join(V2, f'evaluator_{kind}.json'), decks, os.path.join(V2, f'evaluator_{kind}.pt'))
        print(kind, json.dumps(r))
        assert S.failed(r) is None, r


def choice_spec(js, k=6, explore=0.25, **dc):
    return json.dumps({'sealed': {'set': 'eoe', 'collation': 'paper', 'best_share': 0.5,
                                  'deck_choice': dict({'k': k, 'explore': explore, 'by': 'evaluator', 'evaluator': js}, **dc)},
                       'max_turns': 20})


def test_the_candidates_carry_the_evaluators_scores_and_choice(exported):
    pt, js, sha = exported['set']
    ref = EV.ReferenceScorer(js)
    n_chosen = 0
    for seed in range(12):
        for seat in SE.candidates(choice_spec(js), seed):
            s = np.array(seat['scores'])
            assert np.allclose(s, ref.score([c['deck'] for c in seat['candidates']]), atol=1e-12)
            assert seat['evaluator_sha'] == sha
            if seat['explore'] is None:
                n_chosen += 1
                assert seat['chosen'] == int(np.argmax(s))
            else:
                assert seat['chosen'] is None
    assert n_chosen > 8
    # value choice: no scores
    assert 'scores' not in SE.candidates(json.dumps({'sealed': {'set': 'eoe', 'collation': 'paper',
                                                                'deck_choice': {'k': 4}}}), 1)[0]


def test_driver_games_choose_by_the_evaluator_and_the_log_checks(exported, tmp_path):
    pt, js, sha = exported['linear']
    spec = choice_spec(js)
    bot = G.seat_dict('heuristic')
    E.game_log(str(tmp_path / 'games' / 'games-1.jsonl'))
    try:
        drv = G.Driver('mtg_eoe', [[bot, bot]] * 4, spec=spec, concurrency=4, seed=3, threads=2, record=False)
        drive(drv, {})
        res = drv.take_results()
    finally:
        E.game_log(None)
    assert len(res) == 4 and all(r['over'] for r in res)
    specs, games = GL.load(str(tmp_path))
    assert len(games) == 4
    sl = next(iter(specs.values()))['sealed']['deck_choice']
    assert (sl['by'], sl['evaluator_sha']) == ('evaluator', sha)
    modes = [s['mode'] for g in games for s in g['seats']]
    assert 'chosen' in modes
    for g in games:
        for s in g['seats']:
            assert (s.get('evaluator') == sha) == (s['mode'] == 'chosen')
    assert GL.check(specs, games)['mismatch'] == []
    # a tampered choice is found
    g = json.loads(json.dumps(games[0]))
    p = [s['mode'] for s in g['seats']].index('chosen')
    g['seats'][p]['evaluator'] = '0' * 64
    assert any(m[3] == 'evaluator' for m in GL.check(specs, [g])['mismatch'])


class FakeRun:
    def __init__(self, run, cfg):
        self.run, self.cfg, self.lines = str(run), cfg, []

    def log(self, msg):
        self.lines.append(msg)


def test_the_versions_and_the_retrains_schedule(exported, tmp_path, monkeypatch):
    pt, js, sha = exported['set']
    spec = choice_spec(js, k=32, explore=0.1)
    cfg = dict(run.DEFAULTS, game='mtg_eoe', spec=spec, evaluator_every=4, evaluator_window=40, evaluator_min_window=8,
               evaluator_val_iters=2)
    R = FakeRun(tmp_path / 'r', cfg)
    os.makedirs(R.run)
    started = []

    class Proc:
        pid = 4242

        def __init__(self, cmd, **kw):
            started.append(cmd)

        def poll(self):
            return 0
    monkeypatch.setattr(RO.subprocess, 'Popen', Proc)
    s = json.loads(RO.selfplay_spec(R, 79))
    assert os.path.exists(os.path.join(R.run, 'evaluators', 'v0078.json')), 'the spec\'s file is the first version'
    dc = s['sealed']['deck_choice']
    assert dc['evaluator'].endswith('/evaluators/v0078.json') and dc['evaluator_sha'] == sha
    for it in range(80, 89):                       # 79..87 played: no window inside the run yet
        RO.selfplay_spec(R, it)
    assert not started
    RO.selfplay_spec(R, 89)                        # b = 88: the window 81-88, after v0078
    assert len(started) == 1 and started[0][3:9] == ['retrain', R.run, '--iter', '88', '--from-iter', '79']
    assert started[0][started[0].index('--kind') + 1] == 'set'
    RO.selfplay_spec(R, 90)                        # the job ended without a status: reported failed once
    assert sum('EVALUATOR RETRAIN FAILED v0088' in x for x in R.lines) == 1
    RO.selfplay_spec(R, 91)
    assert sum('EVALUATOR RETRAIN FAILED' in x for x in R.lines) == 1
    # a published version: used from the next phase, recorded per iteration
    d = os.path.join(R.run, 'evaluators', 'work', 'v0092')
    os.makedirs(d)
    import shutil
    shutil.copyfile(exported['linear'][1], os.path.join(R.run, 'evaluators', 'v0092.json'))
    with open(os.path.join(d, 'status.json'), 'w') as f:
        json.dump({'ok': True, 'sha256': exported['linear'][2], 'games': 10, 'window': [85, 92], 'val': 0.69,
                   'seconds': 1}, f)
    s = json.loads(RO.selfplay_spec(R, 93))
    assert s['sealed']['deck_choice']['evaluator_sha'] == exported['linear'][2]
    assert any('deck evaluator v0092 published' in x for x in R.lines)
    used = RO._rows(os.path.join(R.run, 'evaluators', 'used.jsonl'))
    assert [u['iter'] for u in used] == list(range(79, 92)) + [93] and used[-1]['version'] == 'v0092.json'
    assert len(started) == 1, 'v0092 exists: no retrain of 92'
    # the deck-choice evaluation's evaluator and its remote inputs
    assert RO.evaluator_of(R, 93) == (os.path.join(R.run, 'evaluators', 'v0092.json'), exported['linear'][2])
    assert RO.evaluator_of(R, 80)[1] == sha
    R.cfg = dict(cfg, deckchoice_eval_games=8)
    assert RO.remote_eval_inputs(R, 93) == ['evaluators/used.jsonl', 'evaluators/v0092.json']
    assert RO.remote_eval_inputs(R, 500) == []
    # without the rolling retrain: no version, the spec as given
    R0 = FakeRun(tmp_path / 'r0', dict(cfg, evaluator_every=0))
    assert RO.selfplay_spec(R0, 5) is None and RO.evaluator_of(R0, 5) == (js, sha)
    with pytest.raises(ValueError, match='deck choice by an evaluator'):
        RO.selfplay_spec(FakeRun(tmp_path / 'r1', dict(cfg, spec=P.SPEC_EOE)), 3)
    assert RO.selfplay_spec(FakeRun(tmp_path / 'r2', dict(cfg, spec=P.SPEC_EOE, evaluator_every=0)), 3) is None


def test_the_snapshot_holds_the_windows_games(tmp_path):
    """A retrain reads the games of its iterations only (a remote evaluation's file by its name, the rest by time)."""
    src = tmp_path / 'r'
    (src / 'games').mkdir(parents=True)
    with open(src / 'metrics.jsonl', 'w') as f:
        for it in range(1, 6):
            f.write(json.dumps({'iter': it, 'time': 100.0 * it}) + '\n')
    spec = {'v': 2, 'spec': 'aa', 'sealed': {}}
    with open(src / 'games' / 'games-1.jsonl', 'w') as f:
        f.write(json.dumps(spec) + '\n')
        for t in (50, 150, 250, 350, 450, 550):
            f.write(json.dumps({'v': 2, 'spec': 'aa', 'seed': t, 'time': float(t), 'seats': []}) + '\n')
    with open(src / 'games' / 'eval-iter_0004-9.jsonl', 'w') as f:
        f.write(json.dumps(spec) + '\n' + json.dumps({'v': 2, 'spec': 'aa', 'seed': 1, 'time': 999.0, 'seats': []}) + '\n')
    n, seen = RO.snapshot(str(src), 3, 4, str(tmp_path / 'snap'))
    assert (n, seen) == (3, [3, 4])
    rows = [json.loads(x) for x in open(tmp_path / 'snap' / 'games' / 'games-1.jsonl')]
    assert [r.get('seed') for r in rows] == [None, 250, 350]
    assert len(open(tmp_path / 'snap' / 'metrics.jsonl').readlines()) == 4


def test_the_deck_choice_evaluations_spec(exported):
    pt, js, sha = exported['set']
    cfg = dict(run.DEFAULTS, game='mtg_eoe', spec=choice_spec(js, k=32), eval_spec=P.EVAL_SPEC_EOE)
    R = FakeRun('.', cfg)
    for p in (0, 1):
        sp = json.loads(RO.deckchoice_spec(R, js, sha, p))
        assert sp['decks'] == 'eval' and sp['sealed']['best_share'] == 1.0
        assert sp['sealed']['deck_choice'] == {'k': 32, 'explore': 0.0, 'by': 'evaluator',
                                               'evaluator': os.path.abspath(js).replace(os.sep, '/'), 'evaluator_sha': sha,
                                               'seats': [p]}
        seats = SE.seats(json.dumps(sp), 3)
        assert seats[1 - p]['mode'] == 'best'
        c = SE.candidates(json.dumps(sp), 3)
        assert c[1 - p] is None and c[p]['chosen'] is not None
    base = json.loads(RO.deckchoice_spec(R, js, sha, 0, choose=False))
    assert 'deck_choice' not in base['sealed'] and base['sealed']['best_share'] == 1.0


def test_the_presets_of_run_2e():
    c = run.resolve_config(run.parse(['--run', 'x', '--preset', 'eoe-2e']), {})
    dc = json.loads(c['spec'])['sealed']['deck_choice']
    assert (dc['k'], dc['explore'], dc['by']) == (32, 0.1, 'evaluator')
    assert (c['evaluator_every'], c['evaluator_window'], c['evaluator_min_window'], c['evaluator_kind'],
            c['evaluator_half_life'], c['deckchoice_eval_games']) == (4, 40, 8, 'set', 40.0, 64)
    assert c['eval_spec'] == P.EVAL_SPEC_EOE and c['selfplay_value_choice_worlds'] == 0
    one = run.resolve_config(run.parse(['--run', 'x', '--preset', 'eoe-1']), {})
    assert (one['evaluator_every'], one['deckchoice_eval_games']) == (0, 0), 'off by default'
    assert {k: one[k] for k in RO.SETTINGS} == RO.SETTINGS
    smoke = run.resolve_config(run.parse(['--run', 'x', '--preset', 'eoe-2e-smoke']), {})
    assert smoke['evaluator_every'] == 1 and json.loads(smoke['spec'])['sealed']['deck_choice']['k'] == 8


def test_a_rolling_driver_switches_evaluators_between_phases(exported, tmp_path):
    """zero-core's Driver.set_spec on an mtg_eoe rolling driver (run 2's self-play, docs/ENGINE.md 112.4): the games
    started after the switch choose by the new evaluator file, the earlier ones by the old (the game log's sha256)."""
    a, b = exported['linear'], exported['set']
    bot = G.seat_dict('heuristic')
    E.game_log(str(tmp_path / 'games' / 'games-1.jsonl'))
    try:
        drv = G.Driver('mtg_eoe', [[bot, bot]], spec=choice_spec(a[1]), concurrency=3, seed=5, threads=2, record=False,
                       rolling=True)
        drv.resume(3, 0)
        drive(drv, {})
        first = drv.take_results()
        drv.set_spec(choice_spec(b[1]))
        drv.resume(6, 0)
        drive(drv, {})
        second = drv.take_results()
        drv.end_phase()
    finally:
        E.game_log(None)
    assert len(first) >= 3 and len(second) >= 6
    specs, games = GL.load(str(tmp_path))
    shas = {g['seed']: {s.get('evaluator') for s in g['seats']} - {None} for g in games}
    assert {sha for s in shas.values() for sha in s} == {a[2], b[2]}, 'both versions chose decks'
    assert len({json.dumps(v['sealed']['deck_choice'], sort_keys=True) for v in specs.values()}) == 2
    assert GL.check(specs, games)['mismatch'] == []
