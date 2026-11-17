"""tools/delverai_meta.py (docs/ENGINE.md 185): a metagame run's parts of the delverai record and recording - the
75s, the wheel's p, the self-play results by ordered pairing read from the shards, the recorder's covering seed, the
combo statistics' entry - and the publisher's record of a metagame run (no local paths). No process is started."""
import json
import os
import sys

import numpy as np
import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'tools'))
sys.path.insert(0, ROOT)

import mtgz.az  # noqa: E402,F401
from mtgz.az import belief, decklists, replays  # noqa: E402
import delverai_meta as M  # noqa: E402

PATH_RE = __import__('re').compile(r'[A-Za-z]:[\\/]|\\|(^|[\s"\'(])/(Users|home|mnt|tmp|root|var)/|\.pt\b')


def cfg(sched=None):
    spec = json.loads(decklists.metagame_spec())
    if sched is not None:
        spec['forced_find'] = {'spell': 'Doomsday', 'card': "Thassa's Oracle", 'p': 1.0, 'unless_in_hand': True}
    return {'spec': json.dumps(spec), 'forced_find_schedule': sched or {}}


def test_the_75s_with_their_labels_and_provenance():
    ds = M.decks(cfg())
    assert [d['label'] for d in ds] == ['Izzet Delver', 'Doomsday', 'Sneak and Show']
    for d in ds:
        assert sum(n for n, _ in d['main']) == 60
        assert sum(n for n, _ in d['sideboard']) == 15          # (the list's sideboard: the spec plays mains only)
        assert d['player'] and d['source'].startswith('https://')
    assert M.weights(cfg()) == [[1] * 3 for _ in range(3)]
    assert M.is_metagame(cfg()) and not M.is_metagame({'spec': decklists.mirror_spec()})


def test_the_wheel_is_the_schedules_p_at_every_iteration():
    w = M.wheel(cfg({'start': 1.0, 'end': 0.0, 'from': 1, 'iters': 60}), 15)
    assert w['spell'] == 'Doomsday' and w['card'] == "Thassa's Oracle"
    assert w['now'] == {'iter': 16, 'p': 0.75}
    assert w['points'][0] == [1, 1.0] and w['points'][-1] == [16, 0.75] and len(w['points']) == 16
    w = M.wheel(cfg({'start': 1.0, 'end': 0.0, 'from': 1, 'iters': 60}), 99)
    assert w['points'][-2] == [61, 0.0] and w['points'][-1] == [100, 0.0] and w['now']['p'] == 0.0
    assert M.wheel(cfg(), 5) is None


def test_the_opponent_pool_bag_names_the_opponents_deck():
    """The bag the shard reader takes the pairing from (encode.rs bag::OPP_POOL at belief.O_BAGS): every game of the
    metagame, from both seats, reads its opponent's deck."""
    from zero.games import engine
    E = engine()
    spec = decklists.metagame_spec()
    reader = M.DeckReader(M.decks(cfg()))
    seen = set()
    for seed in range(120):
        g = E.Game('mtg', spec, 2, seed)
        pair = json.loads(g.export_state())['match']['pairing']['decks']
        x = g.encode(g.to_move)
        cols = np.nonzero(x[reader.lo:reader.hi])[0] + reader.lo
        assert reader.deck(cols) == pair[1 - g.to_move]
        seen.add(tuple(pair))
    assert len(seen) == 9


def _game(pa, pb, r0, rows=6, first_full=True):
    """Rows of one game: players alternate; player 0 sees deck pb, player 1 deck pa; the deck target is the whole
    opponent main at the first decision of each, then less."""
    p, z, opp, dk = [], [], [], []
    for k in range(rows):
        pl = k % 2
        p.append(pl)
        z.append([r0, 1 - r0] if pl == 0 else [1 - r0, r0])
        opp.append(pb if pl == 0 else pa)
        dk.append(60.0 if (k < 2 and first_full) else 55.0)
    return p, z, opp, dk


def _cat(*games):
    cols = list(zip(*games))
    return (np.concatenate([np.asarray(c) for c in cols[0]]), np.asarray([x for c in cols[1] for x in c], float),
            np.concatenate([np.asarray(c) for c in cols[2]]), np.concatenate([np.asarray(c, float) for c in cols[3]]))


def test_segment_splits_games_by_result_deck_and_a_fresh_deck_target():
    g = [_game(0, 1, 1.0), _game(0, 1, 1.0), _game(0, 1, 0.0), _game(2, 1, 0.0), _game(1, 1, 0.5)]
    p, z, opp, dk = _cat(*g)
    r0 = np.where(p == 0, z[:, 0], z[:, 1])
    assert M.segment(p, r0, opp, dk) == [(0, 1, 1.0), (0, 1, 1.0), (0, 1, 0.0), (2, 1, 0.0), (1, 1, 0.5)]
    # one player's rows only: its pairing is unread (None on the other seat)
    p1, z1, o1, d1 = _game(0, 2, 1.0)
    keep = [i for i in range(6) if p1[i] == 0]
    out = M.segment(np.asarray(p1)[keep], np.asarray([1.0] * 3), np.asarray(o1)[keep], np.asarray(d1, float)[keep])
    assert out == [(None, 2, 1.0)]


def _shard(path, games, reader, compressed=False):
    p, z, opp, dk = _cat(*games)
    n = len(p)
    xi, xp = [], [0]
    for o in opp:
        d = M.decks(cfg())[o]
        ids = sorted({belief.names().index(c) for _, c in d['main']})
        xi += [reader.lo + i for i in ids]
        xp.append(len(xi))
    aux = np.zeros((n, 2 * belief.D), np.float16)
    aux[:, belief.D] = dk                         # (only the total is read)
    arrs = dict(player=p.astype(np.int8), z=z.astype(np.float16), x_ptr=np.asarray(xp, np.int64),
                x_idx=np.asarray(xi, np.uint16), aux=aux)
    (np.savez_compressed if compressed else np.savez)(path, **arrs)


def test_an_iterations_tally_from_its_shards(tmp_path):
    reader = M.DeckReader(M.decks(cfg()))
    d = tmp_path / 'run' / 'data' / 'iter_0003'
    d.mkdir(parents=True)
    # the local writer's two shards (stored: memory-mapped) and a remote one (compressed)
    _shard(str(d / 'r00_000.npz'), [_game(0, 1, 1.0), _game(1, 0, 0.0)], reader)
    _shard(str(d / 'r00_001.npz'), [_game(2, 2, 0.5), _game(0, 1, 1.0)], reader)
    _shard(str(d / 'm00-abcd_000.npz'), [_game(1, 2, 0.0)], reader, compressed=True)
    (d / 'DONE.json').write_text(json.dumps({'selfplay': {'games': 4, 'remote': {'accepted': 1},
                                                          'resign': {'resigned': 2}}}))
    t = M.iteration_tally(str(tmp_path / 'run'), 3, reader, 3)
    assert t == {'iter': 3, 'games': 5, 'read': 5, 'unread': 0, 'resigned': 2,
                 'cells': {'0|1': [2, 2.0], '1|0': [1, 0.0], '1|2': [1, 0.0], '2|2': [1, 0.5]}}
    assert M.iteration_tally(str(tmp_path / 'run'), 4, reader, 3) is None       # no such iteration
    # the pass: tallied once, kept in the state dir, read back for the record
    run = str(tmp_path / 'run')
    sd = str(tmp_path / 'state')
    assert M.selfplay_pass(run, cfg(), sd, log=lambda m: None) == [3]
    assert M.selfplay_pass(run, cfg(), sd, log=lambda m: None) == []
    assert M.selfplay_record(sd) == [t]


def test_the_covering_seed_gives_each_pairing_once_and_matches_the_recorder():
    from zero import replay as core
    spec = replays.spec_with_log(decklists.metagame_spec())
    seed, ps = M.covering_seed(spec, 9, 9, start=12345)
    assert sorted(ps) == [(i, j) for i in range(3) for j in range(3)]
    drv = core.RecordingDriver('mtg', 9, lambda k: ['random', 'random'], spec=spec, concurrency=3, seed=seed)
    got = []
    while drv.next_gid < 9:
        drv._refill()
        got += [(s.gid, tuple(json.loads(s.game.export_state())['match']['pairing']['decks'])) for s in drv.active]
        drv.active = []
    assert [x for _, x in sorted(got)] == ps
    # fewer games than cells: all different
    _, ps = M.covering_seed(spec, 4, 9, start=7)
    assert len(set(ps)) == 4


def test_a_recorded_games_seats_and_listing_pairing():
    from zero.games import engine
    spec = replays.spec_with_log(decklists.metagame_spec())
    st = json.loads(engine().Game('mtg', spec, 2, 1).export_state())
    rep = {'states': [st], 'final': 0, 'format': 'zero-replay/1', 'title': 't', 'group': None, 'created': 0,
           'game_index': 0, 'seats': [{'name': 'a', 'sims': 4}, {'name': 'a'}], 'selfplay': False, 'frames': [],
           'over': True, 'result': [1, 0]}
    p = st['match']['pairing']
    assert M.seat_labels(rep, M.decks(cfg())) == [M.decks(cfg())[i]['label'] for i in p['decks']]
    assert replays.summary(rep)['pairing'] == {'decks': p['decks'], 'names': p['names']}
    st.pop('match')
    assert 'pairing' not in replays.summary(dict(rep, states=[st]))


def _dd(casts, resolved, wins, oracle):
    return {'casts': casts, 'resolved': resolved, 'oracle_wins': wins, 'oracle': oracle}


def test_the_combo_entry_and_its_gate():
    z = {k: 0 for k in M.FF_CLASSES}
    sums = [
        {'decks': [M.DOOMSDAY, M.DELVER], 'games': 24, 'p0_score': 0.25,
         'doomsday': {'p0': _dd(10, 8, 1, dict(z, voluntary=5, missed=3))}},
        {'decks': [M.DOOMSDAY, M.DOOMSDAY], 'games': 24, 'p0_score': 0.5,
         'doomsday': {'p0': _dd(6, 4, 2, dict(z, voluntary=1, in_hand=3)), 'p1': _dd(5, 4, 1, dict(z, not_findable=4))}},
        {'decks': [M.DOOMSDAY, M.SNEAK], 'games': 24, 'p0_score': 0.5,
         'doomsday': {'p0': _dd(3, 2, 0, dict(z, voluntary=2))},
         'sneak_show': {'p1': {'show': {'resolved': 4, 'countered': 1}, 'games_show_resolved': 4,
                               'games_sneak_activated': 2, 'games_fatty': 5, 'games_omniscience': 0, 'won_by_damage': 6}}},
    ]
    e = M.combo_entry(32, sums, 64, 24, 100.0)
    dd = e['doomsday']
    assert dd['resolved'] == 18 and dd['oracle']['voluntary'] == 8 and dd['oracle']['not_findable'] == 4
    assert dd['vs'][M.DOOMSDAY]['games'] == 48 and dd['vs'][M.DELVER]['oracle_wins'] == 1
    assert dd['own_piles'] == pytest.approx(8 / 18) and not dd['gate']['piles_met'] and not dd['gate']['met']
    assert dd['gate']['oracle_wins_vs_delver'] == 1
    s = e['sneak_show']['vs'][M.DOOMSDAY]
    assert s == {'games': 24, 'points': 12.0, 'show_cast': 5, 'show_resolved': 4, 'sneak_games': 2, 'fatty_games': 5,
                 'omniscience_games': 0, 'won_by_damage': 6}
    assert M.combo_pairings([M.DELVER, M.DOOMSDAY, M.SNEAK]) == [
        (M.DOOMSDAY, M.DELVER), (M.DOOMSDAY, M.DOOMSDAY), (M.DOOMSDAY, M.SNEAK), (M.SNEAK, M.DELVER), (M.SNEAK, M.SNEAK)]


def test_the_publishers_record_of_a_metagame_run(tmp_path):
    import delverai_publish as P
    run = tmp_path / 'meta-run'
    (run / 'models').mkdir(parents=True)
    c = dict(cfg({'start': 1.0, 'end': 0.0, 'from': 1, 'iters': 60}), preset='legacy-meta3-ddwheel', yardsticks={})
    (run / 'config.json').write_text(json.dumps(c))
    (run / 'metrics.jsonl').write_text(''.join(json.dumps({'iter': i, 'time': 1.0 + i, 'selfplay': {'games': 10},
                                                           'train': {'policy': 1.0}}) + '\n' for i in (1, 2)))
    p = P.progress(str(run), 8, {}, {'title': 'Three decks'}, str(tmp_path / 'state'))
    assert p['run_kind'] == 'metagame' and p['deck'] is None and p['format'] == 'bo1'
    m = p['metagame']
    assert [d['name'] for d in m['decks']] == list(decklists.META3)
    assert m['wheel']['now'] == {'iter': 3, 'p': 0.966667}
    assert m['selfplay'] == [] and m['combo'] == []
    assert 'cells' not in m                                   # (the run measures no cells)
    body = P.wire_progress(p).decode()
    assert all(d['source'].startswith('https://') for d in json.loads(body)['metagame']['decks'])
    assert not PATH_RE.search(json.dumps({k: v for k, v in m.items() if k != 'decks'}))
    # a Delver run's record: no kind, no block
    (run / 'config.json').write_text(json.dumps({'spec': decklists.mirror_spec()}))
    q = P.progress(str(run), 8, {}, {}, str(tmp_path / 'state'))
    assert 'run_kind' not in q and 'metagame' not in q and q['deck']['main']
