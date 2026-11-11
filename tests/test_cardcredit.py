"""The game log's version 2 and per-card draw credit (docs/ENGINE.md 107.3): games of zero-core's driver logged with their
actions, draws and plays; every game replayed exactly from its spec line, seed and actions; `mtgz.sealed.cardcredit` with a
tiny random network (the drawer's value before and after each draw, the raw drawn-vs-not-drawn rates)."""
import json
import os

import pytest

from mtgz import _engine as E
from mtgz.az import games as G
from mtgz.az.infer import UniformEvaluator, drive
from mtgz.sealed import cardcredit as CC
from mtgz.sealed import games as GL

SPEC = json.dumps({'sealed': {'set': 'eoe', 'collation': 'paper', 'best_share': 0.5,
                              'deck_choice': {'k': 4, 'explore': 0.25}}, 'max_turns': 16})


@pytest.fixture(scope='module')
def logged(tmp_path_factory):
    d = tmp_path_factory.mktemp('run')
    seat = G.nn_seat('m', 4, selfplay=True, sims_fast=2, p_full=0.5, temp_moves=10, sims_minor=2,
                     value_choice_worlds=2, value_choice_temp=0.05)
    E.game_log(str(d / 'games' / 'games-1.jsonl'))
    try:
        drv = G.Driver('mtg_eoe', [[seat, seat]] * 4, spec=SPEC, concurrency=4, seed=9, threads=2)
        drive(drv, {'m': UniformEvaluator()})
        res = drv.take_results()
    finally:
        E.game_log(None)
    return str(d), res


def test_version_2_lines_replay_exactly(logged):
    run, res = logged
    specs, games = GL.load(run)
    assert len(games) == len(res) == 4
    for g in games:
        assert g['v'] == 2
        acts = CC.actions_of(g)
        game = G.engine().Game('mtg_eoe', CC.replay_spec(specs[g['spec']]), 2, g['seed'])
        drew = {0: [], 1: []}
        for a in acts:
            for p, d in CC.draws_of(game.step(a)):
                drew[p].append(d)
        assert game.over and [round(x, 4) for x in game.result()[:2]] == [round(x, 4) for x in g['result']]
        for p, seat in enumerate(g['seats']):
            assert len(seat['draws']) == len(drew[p]) >= 7
            names = [CC.card_name(seat, d[2]) for d in seat['draws']]
            ids = CC.GL_lookup(sorted(set(names)))
            assert [ids[n] for n in names] == drew[p], 'the logged draws are the replay\'s'
            assert all(d[1] == 0 for d in seat['draws'] if d[0] == 0) and all(d[1] in (1, 2) for d in seat['draws'] if d[0] > 0)
            assert all(CC.card_name(seat, x[2]) in seat['deck'] for x in seat['plays'])
            assert CC.seen_cards(seat) <= set(seat['deck'])
    # the deck choice's actions come first and replay the choices
    assert all(s['mode'] in ('chosen', 'explore') for g in games for s in g['seats'])
    assert GL.check(specs, games)['mismatch'] == []


def test_card_credit_with_a_network(logged, tmp_path):
    import torch
    from zero.net import make_net, save
    run, _ = logged
    torch.manual_seed(0)
    os.makedirs(tmp_path / 'r' / 'models')
    ck = str(tmp_path / 'r' / 'models' / 'iter_0000.pt')
    save(make_net(G.info('mtg_eoe'), 'mtg_eoe', 'tokens', 16, 1, heads=2), ck)
    out = CC.credit(run, ck, threads=2)
    assert out['games'] == 4 and out['draws_credited'] > 0 and out['opening']['steps'] >= 8
    cards = {c['name']: c for c in out['cards']}
    credited = [c for c in cards.values() if c['draws']]
    assert credited and all(abs(c['credit']) <= 1.0 for c in credited)
    assert any(c['se'] is not None for c in credited)
    assert all(c['gih_n'] + c['gns_n'] > 0 for c in cards.values())
    # the same numbers again (replays and network deterministic); the table and the command line
    again = CC.credit(run, ck, threads=2)
    assert again['cards'] == out['cards']
    assert 'draws credited' in CC.table(out)
    path = tmp_path / 'cc.json'
    CC.main([run, '--checkpoint', ck, '--json', str(path), '--quiet', '--threads', '2'])
    assert json.load(open(path))['draws_credited'] == out['draws_credited']
    # a version-1 line (no actions) is refused, never skipped in silence
    with pytest.raises(ValueError, match='version-1'):
        CC.actions_of({'v': 1, 'seed': 3})
