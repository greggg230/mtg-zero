"""mtgz.az.sideboard_census (docs/ENGINE.md 132): what a best-of-three checkpoint sideboards before game 2. The
summary's arithmetic on hand-made starts (the splits, the shares and their standard errors, each card's distribution
of copy changes, the checks a start must pass), a tiny untrained network's census end to end (CPU, 4 simulations:
every start stops at game 2's first decision of play with two legal 60s), and the publisher's census pass (the file
beside delverai.json, the record's `sideboard_census`, a generation already done or failed not done again)."""
import argparse
import json
import math
import os
import sys

import pytest

import mtgz.az as A
from mtgz.az import decklists, sideboard_census as C
from mtgz.az import games as G

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def _main():
    return {n: c for n, c in C.replays.main_decks(A.SPEC_DELVER_BO3)[0].items()}


def _deck(changes):
    """The main 60 with {name: copy change} applied, as the spectator view lists it."""
    d = _main()
    for n, c in changes.items():
        d[n] = d.get(n, 0) + c
    return [[n, c] for n, c in d.items() if c]


def _start(loser, chooser_deck, other_deck, play_first=True):
    """A census start's record: game 1's loser sideboards first and chooses."""
    decks = [None, None]
    decks[loser], decks[1 - loser] = _deck(chooser_deck), _deck(other_deck)
    return {'order': [loser, 1 - loser], 'wins': [0, 1] if loser == 0 else [1, 0], 'chooser': loser,
            'choice': 'Play first' if play_first else 'Draw first', 'starting': loser if play_first else 1 - loser,
            'decks': decks, 'results': [2 if loser == 0 else 1, 0]}


def test_the_summary_splits_the_sideboardings_and_counts_each_cards_copy_changes():
    pyro3 = {'Pyroblast': 3, 'Force of Will': -2, 'Daze': -1}
    rough = {'Rough // Tumble': 1, 'Prismari Charm': -1}
    recs = [_start(0, pyro3, rough), _start(1, pyro3, {}), _start(0, {'Pyroblast': 2, 'Daze': -2}, rough),
            _start(1, {}, rough, play_first=False)]
    rec = C.summarize(recs, A.SPEC_DELVER_BO3, {'iter': 7, 'starts': 4})
    assert rec['iter'] == 7 and rec['chose_play'] == [3, 4]
    play, draw, ch, ot = (rec[k] for k in ('play', 'draw', 'chooser', 'other'))
    assert [s['n'] for s in (play, draw, ch, ot)] == [4, 4, 4, 4]
    # the chooser: +3 Pyroblast twice, +2 once, kept once; on the play: the chooser's three and the other's one
    cards = {c['name']: c for c in ch['cards']}
    assert cards['Pyroblast']['d'] == [[3, 2], [2, 1]]
    assert cards['Pyroblast']['mean'][0] == pytest.approx(2.0)
    sd = math.sqrt(sum((x - 2.0) ** 2 for x in (3, 3, 2, 0)) / 3)
    assert cards['Pyroblast']['mean'][1] == pytest.approx(sd / 2, abs=1e-4)
    assert cards['Daze']['d'] == [[-2, 1], [-1, 2]]
    assert ch['kept'] == [0.25, round(math.sqrt(0.25 * 0.75 / 4), 4)]
    assert ch['swapped'][0] == pytest.approx((3 + 3 + 2 + 0) / 4)
    assert ot['kept'][0] == 0.25 and {c['name'] for c in ot['cards']} == {'Rough // Tumble', 'Prismari Charm'}
    # the play split: three choosers who played and the last start's other player (its chooser drew)
    assert {c['name']: c['d'] for c in play['cards']}['Rough // Tumble'] == [[1, 1]]
    assert {c['name']: c['d'] for c in draw['cards']}['Rough // Tumble'] == [[1, 2]]
    # the 75's order: the sideboard's cards first, then the main's
    names = [c['name'] for c in ch['cards']]
    assert names.index('Pyroblast') < names.index('Daze') < names.index('Force of Will')
    text = C.describe(rec)
    assert 'chooser took the play in 3 of 4' in text and '+3 50.0%' in text
    json.dumps(rec)


def test_a_start_that_is_not_a_census_start_fails_loudly():
    ok = _start(0, {}, {})
    with pytest.raises(C.CensusError, match='chooser'):
        C.summarize([dict(ok, chooser=1)], A.SPEC_DELVER_BO3)           # game 1's winner chose
    with pytest.raises(C.CensusError, match='60 cards'):
        C.summarize([dict(ok, decks=[_deck({'Pyroblast': 1}), ok['decks'][1]])], A.SPEC_DELVER_BO3)    # 61
    with pytest.raises(C.CensusError, match='60 cards'):
        C.summarize([dict(ok, decks=[_deck({'Pyroblast': 4, 'Daze': -4}), ok['decks'][1]])], A.SPEC_DELVER_BO3)  # a 4th
    with pytest.raises(C.CensusError, match='starting'):
        C.summarize([dict(ok, starting=None)], A.SPEC_DELVER_BO3)


def test_the_driver_stops_every_start_at_game_2_however_it_sideboarded():
    # random players: every kind of sideboarding (counts asked, many swaps, a 60 kept), then the play-or-draw choice
    from zero.infer import drive
    spec = decklists.mirror_bo3_spec(max_turns=12, game_episodes=True, start_game=2)
    drv = C._CensusDriver('mtg', 60, lambda k: ['random', 'random'], spec=spec, concurrency=60, seed=5,
                          max_steps=C.MAX_STEPS)
    drive(drv, {})
    assert sorted(drv.records) == list(range(60))
    recs = [drv.records[k] for k in range(60)]
    assert all(r['results'] in ([1, 0], [2, 0]) for r in recs)
    rec = C.summarize(recs, spec)                    # (its checks: the loser first and choosing, legal 60s)
    assert 10 < rec['chose_play'][0] < 50            # a coin flip each
    assert rec['play']['swapped'][0] > 2 and rec['draw']['swapped'][0] > 2
    assert any(abs(d) > 1 for sp in C.SPLITS for c in rec[sp]['cards'] for d, _ in c['d'])
    assert sum(C.replays._tally(r['decks'][0]) != _main() for r in recs) > 30


def _tiny_run(tmp_path):
    import torch
    from zero.net import make_net, save
    run = tmp_path / 'delver-bo3-tiny'
    (run / 'models').mkdir(parents=True)
    spec = json.loads(decklists.mirror_bo3_spec(max_turns=12, game_episodes=True, start_game=2))
    cfg = {'game': 'mtg', 'spec': json.dumps(spec), 'sims': 4, 'sims_minor': 2, 'chance_k': 0, 'aux_belief_w': 0.25,
           'aux_deck_w': 0.5, 'eval_select': 'visits'}
    (run / 'config.json').write_text(json.dumps(cfg), encoding='utf-8')
    torch.manual_seed(0)
    m = make_net(G.info('mtg'), 'mtg', 'tokens', 16, 1, heads=2, aux=A.PACKAGE.aux_of('mtg'))
    for k in (0, 8):
        save(m, str(run / 'models' / f'iter_{k:04d}.pt'), {'iter': k})
    return str(run)


def test_a_tiny_networks_census_end_to_end(tmp_path):
    run = _tiny_run(tmp_path)
    ck = os.path.join(run, 'models', 'iter_0008.pt')
    records, info = C.run_starts(ck, starts=6, device='cpu', seed=3)
    assert (info['iter'], info['starts'], info['sims'], info['select']) == (8, 6, 4, 'visits')
    for r in records:
        assert r['results'] in ([1, 0], [2, 0]) and r['chooser'] == r['order'][0] and r['choice'] in ('Play first', 'Draw first')
        assert [sum(n for _, n in d) for d in r['decks']] == [60, 60]
    rec = C.summarize(records, json.load(open(os.path.join(run, 'config.json')))['spec'], info)
    assert all(rec[k]['n'] == 6 for k in C.SPLITS) and rec['chose_play'][1] == 6
    for k in C.SPLITS:          # (this tiny network keeps its 60: its 4 simulations' visits tie, PASS first)
        assert (rec[k]['kept'][0] == 1.0) == (rec[k]['cards'] == []) and rec[k]['swapped'][0] >= 0
    # the same seed, the same census
    again, _ = C.run_starts(ck, starts=6, device='cpu', seed=3)
    assert again == records

    # the publisher: the newest recorded generation's census beside delverai.json, then in the record
    sys.path.insert(0, os.path.join(ROOT, 'tools'))
    import delverai_publish as P
    sd = str(tmp_path / 'state')
    os.makedirs(sd)
    state = {'replayed': [0, 8]}
    a = argparse.Namespace(census=4, state_dir=sd, device='cpu')
    logs = []
    assert P.census_pass(run, a, state, os.path.join(sd, 'delverai.json'), log=logs.append)
    assert os.path.exists(P.census_path(sd, 8)) and len(state['census_s']) == 1
    assert not P.census_pass(run, a, state, os.path.join(sd, 'delverai.json'), log=logs.append)   # done already
    got = P.newest_census(sd)
    assert got['iter'] == 8 and got['starts'] == 4 and got['play']['n'] == 4
    assert set(got) <= set(P.CENSUS_KEYS) and 'E:' not in json.dumps(got) and 'run' not in got
    rec = P.progress(run, state_dir=sd)
    assert rec['sideboard_census']['iter'] == 8 and rec['now']['est']['census_s'] == state['census_s'][-1]
    # a failure: logged, recorded, not tried again
    state = {'replayed': [0]}
    real = P.sideboard_census.census
    P.sideboard_census.census = lambda *x, **k: (_ for _ in ()).throw(C.CensusError('boom'))
    try:
        assert not P.census_pass(run, a, state, os.path.join(sd, 'delverai.json'), log=logs.append)
    finally:
        P.sideboard_census.census = real
    assert state['census_failed'] == [0] and any('FAILED' in x and 'boom' in x for x in logs)
    assert not P.census_pass(run, a, state, os.path.join(sd, 'delverai.json'), log=logs.append)
    assert not P.census_pass(run, argparse.Namespace(census=0, state_dir=sd, device='cpu'), {'replayed': [8, 16]},
                             os.path.join(sd, 'delverai.json'))                     # --census 0
