"""The mixed field and the EoE sealed game log (docs/ENGINE.md 107.1): the presets' `best_share`, the seats' modes
through the bindings, every finished `mtg_eoe` game of zero-core's driver logged once with its decks, modes and
result, the decks rebuilt exactly from the log (`mtgz.sealed.games --check`), and the run shim's log path."""
import json
import os

import pytest

import mtgz.az as P
from mtgz import _engine as E
from mtgz.az import games as G
from mtgz.az.infer import UniformEvaluator, drive
from mtgz.sealed import engine as SE
from mtgz.sealed import games as GL

SPEC = json.dumps({'sealed': {'set': 'eoe', 'collation': 'paper', 'best_share': 0.5}, 'max_turns': 24, 'eval_decks': 4})


def test_the_presets_play_the_mixed_field():
    spec = json.loads(P.SPEC_EOE)
    assert spec['sealed']['best_share'] == 0.5
    assert json.loads(P.EVAL_SPEC_EOE)['sealed']['best_share'] == 0.5
    for name in ('eoe-smoke', 'eoe-1'):
        assert P.PRESETS[name]['spec'] == P.SPEC_EOE


def test_the_seats_name_their_builder():
    modes = [s['mode'] for seed in range(40) for s in SE.seats(SPEC, seed)]
    assert set(modes) == {'best', 'explore'} and 20 <= modes.count('best') <= 60
    plain = json.dumps({'sealed': {'set': 'eoe', 'collation': 'paper'}})
    assert {s['mode'] for seed in range(10) for s in SE.seats(plain, seed)} == {'explore'}
    for seed in range(10):
        for a, b in zip(SE.seats(SPEC, seed), SE.seats(plain, seed)):
            assert a['pool'] == b['pool']
            if a['mode'] == 'explore':
                assert a['deck'] == b['deck']                 # the randomised build unchanged
            else:
                assert a['build']['lands'] == 17 and a['build']['splash'] == '' and a['build']['tilt'] == 0


def test_the_run_shim_logs_to_the_run(monkeypatch):
    assert GL.log_path(['--run', 'runs/x', '--preset', 'eoe-1'], pid=7) == os.path.join('runs/x', 'games', 'games-7.jsonl')
    assert GL.log_path(['--preset', 'eoe-1']) is None
    monkeypatch.setenv('MTGZ_GAME_LOG', '0')
    assert GL.log_path(['--run', 'runs/x']) is None


def test_every_finished_game_is_logged_with_its_decks_modes_and_result(tmp_path):
    seat = G.nn_seat('m', 4, selfplay=True, sims_fast=2, p_full=0.5, temp_moves=10, sims_minor=2)
    h = G.seat_dict('heuristic')
    E.game_log(str(tmp_path / 'games' / 'games-1.jsonl'))
    assert E.game_log_path().endswith('games-1.jsonl')
    try:
        drv = G.Driver('mtg_eoe', [[seat, seat]] * 3, spec=SPEC, concurrency=3, seed=3, threads=2)
        drive(drv, {'m': UniformEvaluator()})
        selfplay = drv.take_results()
        ev = P.eval_spec(SPEC)
        bots = G.Driver('mtg_eoe', [[seat, h], [h, seat]], spec=ev, concurrency=2, seed=5, threads=2)
        drive(bots, {'m': UniformEvaluator()})
        evals = bots.take_results()
    finally:
        E.game_log(None)
    assert E.game_log_path() is None
    specs, games = GL.load(str(tmp_path))
    assert len(specs) == 2 and len(games) == len(selfplay) + len(evals) == 5
    by = {'selfplay': [g for g in games if g['phase'] == 'selfplay'], 'eval': [g for g in games if g['phase'] == 'eval']}
    assert len(by['selfplay']) == 3 and len(by['eval']) == 2
    # the results and turns are the driver's rows'
    for phase, rows in (('selfplay', selfplay), ('eval', evals)):
        assert sorted((r['rounds'], tuple(r['result'])) for r in rows if r['over']) == \
            sorted((g['turns'], tuple(g['result'])) for g in by[phase] if g['result'] is not None)
    # a network seat's search deals worlds, a bot's never does
    assert all(g['searched'] == [True, True] for g in by['selfplay'])
    assert sorted(tuple(g['searched']) for g in by['eval']) == [(False, True), (True, False)]
    assert all(g['eval'] is not None for g in by['eval'])
    for g in games:
        assert all(sum(s['deck'].values()) == 40 and s['mode'] in ('best', 'explore') for s in g['seats'])
    # the decks are rebuilt exactly from the spec and the deck seed
    c = GL.check(specs, games)
    assert c['checked'] == 5 and c['mismatch'] == []
    out = GL.main([str(tmp_path), '--check'])
    assert out['games'] == 5 and out['check']['mismatches'] == 0 and out['selfplay']['searched']['nn'] == 3
    # a tampered deck is a mismatch
    games[0]['seats'][0]['deck'] = {'Plains': 40}
    assert GL.check(specs, games)['mismatch'][0][2:] == (0, 'deck')
    # no games: a loud failure
    with pytest.raises(SystemExit):
        GL.main([str(tmp_path / 'nothing')])
