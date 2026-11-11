"""17lands-style card statistics from the game log (docs/ENGINE.md 107.4): the kept opening hand (mulligans' bottoms, the
engine's own single-option answers), the per-card counts and their 17lands definitions, Wilson / Newcombe intervals, the
filters (iteration window, phase, seats, builder mode, colour pair), version-1 lines (GP only), the 17lands yardstick
column, the command line, and the draw credit on real logged games."""
import json
import math
import os

import pytest

from mtgz.sealed import cardcredit as CC
from mtgz.sealed import cardstats as CS
from mtgz.sealed import games as GL

DECK_A = {'Plains': 8, 'Forest': 9, 'Alpha': 2, 'Beta': 1, 'Gamma': 1}
DECK_B = {'Island': 17, 'Delta': 1, 'Epsilon': 1}


def seat(deck, draws=None, bottom=(), mulligans=0, mode='best', colors='WG'):
    s = {'mode': mode, 'pool_seed': 1, 'colors': colors, 'splash': '', 'lands': 17, 'candidate': None, 'deck': deck}
    if draws is not None:
        s.update(draws=draws, bottom=list(bottom), mulligans=mulligans, plays=[])
    return s


def hand(cards, kind=0, turn=0):
    return [[turn, kind, c] for c in cards]


def game(seats, result, t, spec='sp', v=2, searched=(True, True), aborted=False):
    return {'v': v, 'spec': spec, 'seed': 1, 'deck_seed': 1, 'eval': None, 'seats': seats, 'searched': list(searched),
            'result': None if aborted else list(result), 'aborted': aborted, 'turns': 10, 'time': t}


def write_run(path, games, iters=((1, 100.0), (2, 200.0))):
    os.makedirs(os.path.join(path, 'games'), exist_ok=True)
    lines = [{'v': 2, 'spec': 'sp', 'sealed': {'set': 'eoe'}, 'decks': 'random'},
             {'v': 2, 'spec': 'ev', 'sealed': {'set': 'eoe'}, 'decks': 'eval'}] + games
    with open(os.path.join(path, 'games', 'games-1.jsonl'), 'w', encoding='utf-8', newline='') as f:
        f.write(''.join(json.dumps(x) + '\n' for x in lines))
        f.write('{"v": 2, "spec": "sp", "seats"')        # a line being written: left for the next read
    with open(os.path.join(path, 'metrics.jsonl'), 'w', encoding='utf-8') as f:
        f.write(''.join(json.dumps({'iter': i, 'time': t}) + '\n' for i, t in iters))
    return str(path)


# ---------------------------------------------------------------------------------------------------- the opening hand

def test_kept_hand_follows_the_engines_mulligans():
    seven = ['Alpha', 'Plains', 'Plains', 'Forest', 'Beta', 'Forest', 'Plains']
    assert sorted(GL.kept_hand(seat(DECK_A, hand(seven)))) == sorted(seven)
    # one mulligan: a new seven, one of them bottomed (the first hand is gone whole)
    h2 = ['Gamma', 'Forest', 'Forest', 'Forest', 'Plains', 'Plains', 'Alpha']
    s = seat(DECK_A, hand(seven + h2), bottom=['Alpha'], mulligans=1)
    assert sorted(GL.kept_hand(s)) == sorted(h2[:-1])
    # two mulligans: the second hand's one bottom and the third hand's two are all in `bottom`, in order. The second
    # hand's bottomed Gamma is in the kept hand too: only the third hand's bottoms leave it (the bug of reading the
    # last seven less every bottomed name)
    h3 = ['Gamma', 'Beta', 'Plains', 'Forest', 'Forest', 'Plains', 'Alpha']
    s = seat(DECK_A, hand(seven + h2 + h3), bottom=['Gamma', 'Beta', 'Alpha'], mulligans=2)
    kept = GL.kept_hand(s)
    assert sorted(kept) == sorted(['Gamma', 'Plains', 'Forest', 'Forest', 'Plains'])
    assert 'Gamma' in CC.seen_cards(s)
    # cards as their index among the sorted deck names (the log's way)
    names = sorted(DECK_A)
    s_idx = seat(DECK_A, hand([names.index(c) for c in seven + h2]), bottom=[names.index('Alpha')], mulligans=1)
    assert sorted(GL.kept_hand(s_idx)) == sorted(h2[:-1])


def test_kept_hand_bottoms_what_the_engine_answered_alone():
    # mulligan to 3: four bottoms from this hand; once only Plains are left the engine puts them on the bottom itself
    # (one option per definition: no action, no logged entry)
    h0 = ['Alpha'] * 2 + ['Plains'] * 5
    rounds = [h0, h0, h0, h0, ['Alpha', 'Plains', 'Plains', 'Plains', 'Plains', 'Plains', 'Plains']]
    # mulligan 1: one Alpha; 2: both; 3: both, then a Plains alone; 4: the Alpha, then three Plains alone
    logged = ['Alpha'] + ['Alpha', 'Alpha'] + ['Alpha', 'Alpha'] + ['Alpha']
    s = seat(DECK_A, hand([c for r in rounds for c in r]), bottom=logged, mulligans=4)
    assert GL.kept_hand(s) == ['Plains'] * 3
    # a record that does not fit is an error, never a guess
    with pytest.raises(ValueError, match='not in that hand'):
        GL.kept_hand(seat(DECK_A, hand(['Alpha'] * 7 + ['Plains'] * 6 + ['Beta']), bottom=['Gamma'], mulligans=1))
    with pytest.raises(ValueError, match='left over'):
        GL.kept_hand(seat(DECK_A, hand(['Alpha'] * 7), bottom=['Alpha'], mulligans=0))
    with pytest.raises(ValueError, match='drawn before the first turn'):
        GL.kept_hand(seat(DECK_A, hand(['Alpha'] * 6), mulligans=0))


# ---------------------------------------------------------------------------------------------------- intervals

def test_wilson_and_newcombe():
    lo, hi = CS.wilson(0.5, 100)
    assert (round(lo, 4), round(hi, 4)) == (0.4038, 0.5962)
    lo, hi = CS.wilson(0.0, 10)
    assert lo == 0.0 and round(hi, 4) == 0.2775
    assert CS.wilson(0.3, 0) == (None, None)
    # Newcombe (1998) method 10, his example: 56/70 vs 48/80 -> 0.2000 (0.0524, 0.3339)
    d, lo, hi = CS.newcombe(56 / 70, 70, 48 / 80, 80)
    assert (round(d, 4), round(lo, 4), round(hi, 4)) == (0.2, 0.0524, 0.3339)
    assert CS.newcombe(0.5, 0, 0.5, 10) == (None, None, None)
    r = CS.rate(10, 13)                               # 10 seats, 6 wins and a draw
    assert r['wr'] == 0.65 and r['lo'] < 0.65 < r['hi'] and CS.rate(0, 0) is None


# ---------------------------------------------------------------------------------------------------- the counts

def games_fixture():
    """Self-play games of iterations 1 and 2, an evaluation game, a version-1 line, an aborted game."""
    opening = ['Alpha', 'Plains', 'Plains', 'Forest', 'Forest', 'Plains', 'Forest']
    a1 = seat(DECK_A, hand(opening) + hand(['Beta', 'Alpha'], kind=1, turn=3), mode='best', colors='WG')
    b1 = seat(DECK_B, hand(['Island'] * 7) + hand(['Delta'], kind=2, turn=2), mode='explore', colors='U')
    a2 = seat(DECK_A, hand(['Gamma'] + ['Plains'] * 6), mode='explore', colors='WG')
    b2 = seat(DECK_B, hand(['Island'] * 6 + ['Epsilon']), mode='best', colors='U')
    return [
        game([a1, b1], [1.0, 0.0], 50.0),                                  # iteration 1: A (best) wins
        game([a2, b2], [0.5, 0.5], 150.0),                                 # iteration 2: a draw
        game([seat(DECK_A, mode='explore'), seat(DECK_B, mode='explore', colors='U')], [0.0, 1.0], 160.0, v=1),  # GP only
        game([a1, b1], [1.0, 0.0], 170.0, aborted=True),                  # aborted: not counted
        game([a2, b2], [0.0, 1.0], 180.0, spec='ev', searched=(True, False)),   # an evaluation game, B a bot
        game([a1, b1], [1.0, 0.0], 250.0),                                 # after the last metrics line: in progress
    ]


def test_counts_follow_the_17lands_definitions(tmp_path):
    run = write_run(tmp_path / 'r', games_fixture())
    out = CS.stats(run, last=0, yardstick=False)
    assert out['window'] == [1, 2] and out['iterations'] == [1, 2] and out['hand_iterations'] == [1, 2]
    assert (out['games'], out['hand_games'], out['seats_counted'], out['aborted']) == (3, 2, 6, 1)
    by = {r['name']: r for r in out['cards']}
    # Alpha: in all three A decks; in the opening hand of game 1 (and drawn again), not seen in game 2; v1 game: GP only
    a = by['Alpha']
    assert a['gp'] == CS.rate(3, 2 + 1 + 0)                       # won, drew, lost
    assert a['oh']['n'] == 1 and a['oh']['wr'] == 1.0
    assert a['gd']['n'] == 1 and a['gih']['n'] == 1 and a['gih']['wr'] == 1.0
    assert a['gns'] == CS.rate(1, 1)                              # game 2 (a draw): in the deck, never in hand
    assert a['iwd']['d'] == 0.5 and a['iwd']['lo'] < 0.5 < a['iwd']['hi']
    # Beta: drawn at turn 3 of game 1 (GD, not OH); Gamma: in game 2's opening hand only
    assert by['Beta']['oh'] is None and by['Beta']['gd']['n'] == 1 and by['Beta']['gns']['n'] == 1
    assert by['Gamma']['oh'] == CS.rate(1, 1) and by['Gamma']['gd'] is None
    # Delta: an effect's draw is a draw; Epsilon: in B's opening hand of game 2
    assert by['Delta']['gd'] == CS.rate(1, 0) and by['Epsilon']['oh'] == CS.rate(1, 1)
    # every seat counts each deck card once: GIH + GNS = the seats with draws
    for r in out['cards']:
        n_h = (r['gih'] or {}).get('n', 0) + (r['gns'] or {}).get('n', 0)
        assert n_h == r['gp']['n'] - 1, r['name']                 # all but the version-1 game
    # the best guess against an explore build: game 1 (best won) and game 2 (a draw)
    assert out['best_vs_explore'] == CS.rate(2, 3)


def test_the_filters(tmp_path):
    run = write_run(tmp_path / 'r', games_fixture())
    assert CS.stats(run, last=1, yardstick=False)['window'] == [2, 2]
    one = CS.stats(run, iters=(1, 1), yardstick=False)
    assert one['games'] == 1 and {r['name'] for r in one['cards']} == set(DECK_A) | set(DECK_B)
    # the evaluation game: the network's seat only, unless all seats
    ev = CS.stats(run, last=0, phase='eval', yardstick=False)
    assert ev['games'] == 1 and ev['seats_counted'] == 1 and {r['name'] for r in ev['cards']} == set(DECK_A)
    assert CS.stats(run, last=0, phase='eval', seats='all', yardstick=False)['seats_counted'] == 2
    assert CS.stats(run, last=0, phase='all', yardstick=False)['games'] == 4
    # builder mode and colour pair: per seat
    best = CS.stats(run, last=0, mode='best', yardstick=False)
    assert best['seats_counted'] == 2 and best['games'] == 2       # A in game 1, B in game 2
    assert CS.stats(run, last=0, pair='gw', yardstick=False)['pair'] == 'WG'
    assert {r['name'] for r in CS.stats(run, last=0, pair='U', yardstick=False)['cards']} == set(DECK_B)
    with pytest.raises(ValueError):
        CS.norm_pair('WX')
    with pytest.raises(SystemExit, match='no game lines'):
        CS.stats(str(tmp_path / 'empty'), yardstick=False)


def test_the_17lands_yardstick_and_the_command_line(tmp_path, capsys):
    y = CS.seventeen_lands()
    assert y['metric'] == 'GIH WR' and y['key'] == 'gih' and y['event'] == 'Sealed'
    assert 'CC BY 4.0' in y['license'] and y['source'].startswith('https://') and 'never' in y['rule']
    assert y['games'] > 50000 and 0.4 < y['win_rate'] < 0.7 and len(y['cards']) > 250
    some = y['cards']['Icecave Crasher']
    assert some['n'] > 1000 and 0 < some['wr'] < 1
    info = CS.card_info()
    assert len(info) == 321 and info[0]['name'] == 'Anticausal Vestige'
    assert {c['rarity'] for c in info} == {'common', 'uncommon', 'rare', 'mythic', 'basic'}
    # the command line: the JSON and the table
    run = write_run(tmp_path / 'r', games_fixture())
    path = tmp_path / 'cs.json'
    CS.main([run, '--last', '0', '--json', str(path), '--sort', 'gp'])
    out = json.load(open(path, encoding='utf-8'))
    assert out['seventeen']['metric'] == 'GIH WR' and out['games'] == 3
    text = capsys.readouterr().out
    assert 'GIH WR' in text and 'a yardstick only' in text and 'Alpha' in text


# ---------------------------------------------------------------------------------------------------- draw credit

def test_draw_credit_on_logged_games(tmp_path):
    import torch
    from mtgz import _engine as E
    from mtgz.az import games as G
    from mtgz.az.infer import UniformEvaluator, drive
    from zero.net import make_net, save
    run = tmp_path / 'run'
    spec = json.dumps({'sealed': {'set': 'eoe', 'collation': 'paper', 'best_share': 0.5}, 'max_turns': 14})
    seat_ = G.nn_seat('m', 4, selfplay=True, sims_fast=2, p_full=0.5, temp_moves=10, sims_minor=2)
    E.game_log(str(run / 'games' / 'games-1.jsonl'))
    try:
        drv = G.Driver('mtg_eoe', [[seat_, seat_]] * 3, spec=spec, concurrency=3, seed=4, threads=2)
        drive(drv, {'m': UniformEvaluator()})
        drv.take_results()
    finally:
        E.game_log(None)
    torch.manual_seed(0)
    os.makedirs(run / 'models')
    save(make_net(G.info('mtg_eoe'), 'mtg_eoe', 'tokens', 16, 1, heads=2), str(run / 'models' / 'iter_0003.pt'))
    out = CS.stats(str(run), credit='latest', threads=2, yardstick=False)
    assert out['window'] is None and out['games'] == out['hand_games'] == 3      # no metrics: every game counts
    c = out['credit']
    assert c['checkpoint'] == 'iter_0003.pt' and c['checkpoint_iter'] == 3 and c['games'] == 3 and c['draws'] > 0
    credited = [r for r in out['cards'] if r.get('credit')]
    assert credited and all(abs(r['credit']['credit']) <= 1 and r['credit']['draws'] >= 1 for r in credited)
    # the same numbers as cardcredit's own run over the whole log
    whole = {x['name']: x for x in CC.credit(str(run), str(run / 'models' / 'iter_0003.pt'), threads=2)['cards']}
    for r in credited:
        assert math.isclose(r['credit']['credit'], whole[r['name']]['credit'], abs_tol=1e-9)
    # every seat's kept hand reconstructs (the log's draws and bottoms fit the engine's mulligans)
    specs, games = GL.load(str(run))
    assert all(len(GL.kept_hand(s)) == 7 - s['mulligans'] for g in games for s in g['seats'])
