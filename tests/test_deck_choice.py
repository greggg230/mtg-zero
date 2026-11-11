"""The sealed deck choice (docs/ENGINE.md 107.2): the presets of run 2, deck-choice games through zero-core's driver with
the seats' value choices (zero-core docs/search.md 3.17: valued by a one-ply lookahead, never recorded), the game log
of the chosen decks, the candidates through the bindings, and the value-picked build of a pool with a network pilot
(`mtgz.sealed.compare choice`)."""
import json
import os

import pytest

import mtgz.az as P
from mtgz import _engine as E
from mtgz.az import games as G
from mtgz.az import run
from mtgz.az.infer import UniformEvaluator, drive
from mtgz.sealed import compare as K
from mtgz.sealed import engine as SE
from mtgz.sealed import games as GL

SPEC = json.dumps({'sealed': {'set': 'eoe', 'collation': 'paper', 'best_share': 0.5,
                              'deck_choice': {'k': 6, 'explore': 0.0}}, 'max_turns': 20})


def test_the_presets_of_run_2():
    c = run.resolve_config(run.parse(['--run', 'x', '--preset', 'eoe-2']), {})
    assert json.loads(c['spec'])['sealed']['deck_choice'] == {'k': 32, 'explore': 0.1}
    assert c['eval_spec'] == P.EVAL_SPEC_EOE and 'deck_choice' not in c['eval_spec']
    assert (c['selfplay_value_choice_worlds'], c['selfplay_value_choice_temp'], c['eval_value_choice_worlds']) == (8, 0.03, 0)
    r = run.Run.__new__(run.Run)
    r.cfg = c
    s = run.Run.nn_seat(r, 'm', 64, selfplay=True)
    assert (s['value_choice_worlds'], s['value_choice_temp']) == (8, 0.03)
    assert 'value_choice_worlds' not in run.Run.nn_seat(r, 'm', 32)
    # run 1's presets are as they were
    one = run.resolve_config(run.parse(['--run', 'x', '--preset', 'eoe-1']), {})
    assert 'deck_choice' not in one['spec'] and one['selfplay_value_choice_worlds'] == 0
    smoke = run.resolve_config(run.parse(['--run', 'x', '--preset', 'eoe-choice-smoke']), {})
    assert json.loads(smoke['spec'])['sealed']['deck_choice']['k'] == 8 and smoke['selfplay_value_choice_worlds'] == 4


def test_the_candidates_through_the_bindings():
    c = SE.candidates(SPEC, 7)
    assert len(c) == 2 and all(len(s['candidates']) == 6 and s['explore'] is None for s in c)
    assert all(b['lands'] == 17 and b['splash'] == '' and b['tilt'] == 0 for b in (s['candidates'][0] for s in c))
    with pytest.raises(ValueError, match='deck_choice'):
        SE.candidates(json.dumps({'sealed': {'set': 'eoe', 'collation': 'paper'}}), 7)


def test_value_choices_choose_the_decks_and_the_log_records_them(tmp_path):
    seat = G.nn_seat('m', 4, selfplay=True, sims_fast=2, p_full=0.5, temp_moves=10, sims_minor=2,
                     value_choice_worlds=3, value_choice_temp=0.0)
    E.game_log(str(tmp_path / 'games' / 'games-1.jsonl'))
    try:
        drv = G.Driver('mtg_eoe', [[seat, seat]] * 3, spec=SPEC, concurrency=3, seed=3, threads=2, record=True)
        drive(drv, {'m': UniformEvaluator()})
        res = drv.take_results()
        st = drv.stats()
        smp = drv.take_samples()
    finally:
        E.game_log(None)
    assert len(res) == 3 and all(r['over'] for r in res)
    # every seat chose (explore 0): six value choices, their rows, none recorded
    assert st['value_choices'] == 6 and 0 < st['value_choice_rows'] <= 6 * 6 * 3
    specs, games = GL.load(str(tmp_path))
    assert len(games) == 3 and all(s['mode'] == 'chosen' for g in games for s in g['seats'])
    # a uniform network values every candidate alike: the first best, candidate 0 (the best guess)
    assert all(s['candidate'] == 0 for g in games for s in g['seats'])
    assert GL.check(specs, games)['mismatch'] == []
    out = GL.summary(str(tmp_path), specs, games)
    assert out['selfplay']['chosen'] == {'seats': 6, 'best_guess': 6, 'candidates': {'0': 6}}
    assert smp is not None and len(smp['x']) > 0
    # off (no value choice): the deck choice is searched as any decision and the games go on
    plain = G.nn_seat('m', 4, selfplay=True, sims_fast=2, p_full=0.5, temp_moves=10, sims_minor=2)
    drv = G.Driver('mtg_eoe', [[plain, plain]] * 2, spec=SPEC, concurrency=2, seed=3, threads=2)
    drive(drv, {'m': UniformEvaluator()})
    assert all(r['over'] for r in drv.take_results()) and drv.stats()['value_choices'] == 0
    # the heuristic takes the best guess; a random seat any
    h, r = G.seat_dict('heuristic'), G.seat_dict('random')
    drv = G.Driver('mtg_eoe', [[h, r]] * 2, spec=SPEC, concurrency=2, seed=5, threads=2)
    drive(drv, {})
    assert all(x['over'] for x in drv.take_results())


def test_the_value_picked_build_races_random_builds(tmp_path):
    import torch
    from zero.net import make_net, save
    torch.manual_seed(0)
    os.makedirs(tmp_path / 'r' / 'models')
    ck = str(tmp_path / 'r' / 'models' / 'iter_0000.pt')
    save(make_net(G.info('mtg_eoe'), 'mtg_eoe', 'tokens', 16, 1, heads=2), ck)
    p = SE.pool(11, collation='paper')
    pilot = K.Pilot(ck, sims=2)
    j, cands = K.value_pick(p['cards'], pilot, k=5, worlds=2, seed=11, collation='paper')
    assert 0 <= j < 5 and len(cands) == 5
    j2, _ = K.value_pick(p['cards'], pilot, k=5, worlds=2, seed=11, collation='paper')
    assert j2 == j, 'the same pick from the same seeds'
    assert K.value_pick(p['cards'], K.Pilot('heuristic'), k=5, seed=11, collation='paper')[0] == 0
    jj, cs = K.choice_candidates(p['cards'], pilot, k=5, worlds=2, seed=11, vs=2, collation='paper')
    assert jj == j and cs[0]['name'] == f'picked{j}' and len(cs) == 3 and all(c['name'].startswith('random') for c in cs[1:])
    out_path = tmp_path / 'choice.json'
    rc = K.main(['choice', '--pool-seed', '11', '--collation', 'paper', '--k', '5', '--worlds', '2', '--vs', '1',
                 '--pilot', ck, '--sims', '2', '--games', '4', '--block', '4', '--threads', '1', '--max-turns', '6',
                 '--json', str(out_path), '--quiet'])
    out = json.load(open(out_path))
    assert rc == 0 and out['choice']['picked'] == j and out['candidates'][0]['games'] == 4
