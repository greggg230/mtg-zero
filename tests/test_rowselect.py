"""The row selector of a multi-source distillation (mtgz/az/rowselect.py, docs/ENGINE.md 239.1; zero-core docs/az.md 76):
checked against the engine - every decision of random best-of-one and best-of-three metagame games, both players'
rows - the opponent's deck read from the opponent-pool bag is the pairing's, the acting player's true deck is always
among its candidates, and the candidates are one deck wherever the docstring says they must be (every best-of-one
row; every row of a two-deck or mirror run; all but the sideboarding rows of a three-deck best-of-three); the hook on
shard files (dense and sparse), its keys and its loud refusals; a dealt post-board 60 read by its 75 (a dealt-deck
mirror); and zero-core's `tools/distill.py --sources` through this repo's wrapper on best-of-three self-play shards of
a stew-specialist-like pairing run."""
import json
import os
import sys

import numpy as np
import pytest

import mtgz.az  # noqa: F401
from mtgz.az import belief, decklists as DL, rowselect as RS
from zero import data, games as G
from zero.infer import UniformEvaluator, drive

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
D, DD, SN = DL.IZZET_DELVER, DL.LEGACY_DOOMSDAY, DL.SNEAK_SHOW
LO, HI = belief.O_BAGS, belief.O_BAGS + RS.BAGS * belief.D


def specs():
    return {'bo1-3': DL.metagame_spec(decks=[D, DD, SN]),
            'bo3-3': DL.metagame_spec(decks=[D, DD, SN], fmt='bo3', game_episodes=True, start_game=1),
            'bo3-pair': DL.metagame_spec(decks=[D, DD], fmt='bo3', game_episodes=True, start_game=1,
                                         weights=[[0, 1], [1, 0]]),
            'bo3-mirror': DL.metagame_spec(decks=[D], fmt='bo3', game_episodes=True, start_game=1, weights=[[1]])}


def play(spec, seeds, every=1, keep=0):
    """Random play of `spec`'s games (best-of-three: every episode of the match) -> the bag entries of both players'
    rows at every `every`-th decision ((ptr, idx, val) as a sparse shard stores them), each row's (acting deck, opponent
    deck, sideboarding) from the match's pairing, and up to `keep` whole rows (head features, player) for shard files."""
    E = G.engine()
    rng = np.random.default_rng(1)
    ptr, idx, val, truth, rows = [0], [], [], [], []
    for seed in seeds:
        g = E.Game('mtg', spec, 2, seed)
        m = json.loads(g.export_state())['match']
        pair = m['pairing']['decks'] if 'pairing' in m else [0, 0]      # (a fixed mirror spec: deck 0 both seats)
        k = 0
        while g is not None:
            while not g.over:
                if k % every == 0:
                    sb = json.loads(g.export_state())['match'].get('sideboarding') is not None
                    for v in (0, 1):
                        x = np.asarray(g.encode_row(v)[0], np.float32)
                        nz = np.flatnonzero(x[LO:HI]) + LO
                        idx.append(nz)
                        val.append(x[nz])
                        ptr.append(ptr[-1] + len(nz))
                        truth.append((pair[v], pair[1 - v], sb))
                        if len(rows) < keep:
                            rows.append((x, v))
                k += 1
                legal = g.legal()
                g.step(legal[int(rng.integers(len(legal)))])
            g = g.next_episode(None)[0] if g.chained else None
    bags = RS.ShardBags.from_arrays('<played>', np.array(ptr, np.int64), np.concatenate(idx).astype(np.int64),
                                    np.concatenate(val).astype(np.float16))
    return bags, np.array(truth, dtype=object), rows


@pytest.fixture(scope='module')
def played():
    sp = specs()
    # (seeds of every cell twice: the engine draws a game's pairing from its seed, ENGINE.md 176.1)
    return {'bo1-3': play(sp['bo1-3'], [7, 9, 1, 25, 47, 5, 12, 3, 0, 11, 19, 4, 26, 53, 6, 15, 14, 2], every=5),
            'bo3-3': play(sp['bo3-3'], range(6), every=2),
            'bo3-pair': play(sp['bo3-pair'], range(3), every=3),
            'bo3-mirror': play(sp['bo3-mirror'], range(2), every=3)}


@pytest.mark.parametrize('case', ['bo1-3', 'bo3-3', 'bo3-pair', 'bo3-mirror'])
def test_the_bags_tell_the_opponent_and_hold_the_actors_deck(played, case):
    bags, truth, _ = played[case]
    mg = RS.Metagame(json.loads(specs()[case]))
    o, cand = RS.identify(bags, mg)
    actor = np.array([t[0] for t in truth])
    opp = np.array([t[1] for t in truth])
    sb = np.array([t[2] for t in truth], bool)
    assert len(o) == bags.N > 100
    if case == 'bo1-3':
        assert len(set(zip(actor, opp))) == 9                 # (every cell played)
    assert (o == opp).all()                                   # the opponent's deck, exactly
    assert cand[np.arange(len(o)), actor].all()               # the true deck is always a candidate
    one = cand.sum(1) == 1
    if case == 'bo3-3':
        # three decks, best-of-three: a sideboarding row holds none of its player's cards (no game bags), so the
        # opponent's deck leaves every deck that plays it; every other row is told apart
        assert sb.any() and (one == ~sb).all() and (cand[sb].sum(1) == 3).all()
    else:
        assert one.all()
    if case.startswith('bo3'):
        assert sb.any()                                       # (sideboarding rows were among them)


def test_a_dealt_60_is_read_by_its_75(played):
    """The acting deck is identified by its 75, never by the 60 it plays: a best-of-three Delver mirror whose seats are
    dealt random post-board 60s from their own 75 (`dealt_decks` per seat, ENGINE.md 154 / 168.1; the metagame spec
    refuses dealt decks until per-seat dealing reaches pairings, so a fixed mirror spec, read as a one-deck metagame) -
    the opponent-pool bag is the whole 75 on every row, every row's own cards are within the 75 (one candidate), and
    many rows' own cards are NOT within the registered main 60 (sideboard cards dealt in)."""
    for sg in (1, 2):
        sp = DL.mirror_bo3_spec(D, game_episodes=True, start_game=sg,
                                dealt_decks={'p': 1.0, 'swaps': [0, 15], 'per_seat': True})
        spec = json.loads(sp)
        d = spec['decks']
        mg = RS.Metagame({'format': 'bo3', 'constructed': True,
                          'metagame': {'decks': [{'name': D, 'main': d['p0'], 'sideboard': d['p0_sideboard']}]}})
        main60 = RS.Metagame({'format': 'bo3', 'constructed': True,
                              'metagame': {'decks': [{'name': D, 'main': d['p0']}]}}).full[0, :belief.CUBE_CAP]
        bags, truth, _ = play(sp, range(3), every=2)
        o, cand = RS.identify(bags, mg)                       # (raises on a row whose bag is not the 75)
        assert (o == 0).all() and cand[:, 0].all() and bags.N > 100
        own = np.concatenate([bags.counts(lo, min(bags.N, lo + 512))[0] for lo in range(0, bags.N, 512)])
        assert (own > main60).any(1).sum() > bags.N // 10, sg


def test_the_selector_decides_what_the_opponent_alone_tells(played):
    """Undecidable rows count only where the answer depends on the actor: a pairing the opponent's deck rules out is
    decided on a sideboarding row too."""
    bags, truth, _ = played['bo3-3']
    mg = RS.Metagame(json.loads(specs()['bo3-3']))
    o, cand = RS.identify(bags, mg)
    sb = np.array([t[2] for t in truth], bool)
    actor = np.array([t[0] for t in truth])
    for sel in ({'actor': D}, {'actor': DD, 'not_pairing': [[DD, DD]]}):
        dec, yes = RS.evaluate(RS.parse(sel, mg), o, cand, 3)
        assert not dec[sb].all() and dec[~sb].all()
        want = actor == mg.index(sel['actor'], 'actor')
        if 'not_pairing' in sel:
            want &= ~((actor == 1) & (o == 1))
        assert (yes[dec] == want[dec]).all()
    dec, yes = RS.evaluate(RS.parse({'pairing': [DD, SN]}, mg), o, cand, 3)
    ruled_out = sb & (o == 0)                                 # the opponent plays Delver: not a DD-Sneak game
    assert dec[ruled_out].all() and not yes[ruled_out].any()


def write_shard(path, rows, sparse=True):
    info = G.info('mtg')
    n = len(rows)
    x = np.stack([r[0] for r in rows])
    meta = {'game': 'mtg', 'features': info['num_features'], 'actions': info['num_actions']}
    if sparse:
        meta['x_format'] = 'sparse'
    s = {'x': x, 'player': np.array([r[1] for r in rows]), 'n_players': np.full(n, 2), 'opt_off': np.arange(n + 1),
         'opt_action': np.zeros(n, np.int64), 'pi': np.ones(n, np.float32), 'z': np.full((n, 2), 0.5, np.float32),
         'q': np.full((n, 2), 0.5, np.float32), 'sc': np.zeros((n, 2), np.float32), 'net': np.zeros(n, np.int32)}
    os.makedirs(os.path.dirname(path), exist_ok=True)
    data.write_shard(path, s, meta)
    return path


def test_the_hook_on_shard_files(tmp_path):
    sp = specs()['bo1-3']
    _, truth, rows = play(sp, range(3), every=5, keep=10 ** 6)
    actor = np.array([t[0] for t in truth])
    opp = np.array([t[1] for t in truth])
    ctx = {'config': {'spec': sp}, 'meta': {'game': 'mtg'}}
    for sparse in (True, False):
        p = write_shard(str(tmp_path / f'{sparse}' / 'c00_000.npz'), rows, sparse)
        assert np.array_equal(RS.row_select({'actor': D}, p, ctx), actor == 0)
        assert np.array_equal(RS.row_select({'pairing': [DD, D]}, p, ctx),
                              ((actor == 0) & (opp == 1)) | ((actor == 1) & (opp == 0)))
        both = RS.row_select({'actor': D, 'not_pairing': [[D, D]]}, p, ctx)
        assert np.array_equal(both, (actor == 0) & (opp != 0))
    cells, und = RS.tally(p, {'spec': sp})
    assert und == 0 and sum(cells.values()) == len(rows)
    for bad, msg in (({'deck': D}, 'unknown selector keys'), ({'actor': 'nope'}, 'not one of the run'),
                     ({'pairing': [D]}, 'two deck names'), ({'not_pairing': [D, DD]}, 'two deck names'),
                     ({'not_pairing': []}, 'non-empty list')):
        with pytest.raises(ValueError, match=msg):
            RS.row_select(bad, p, ctx)
    with pytest.raises(ValueError, match='needs the shard'):
        RS.row_select({'actor': D}, p, {'config': None})
    with pytest.raises(ValueError, match='metagame runs only'):
        RS.row_select({'actor': D}, p, {'config': {'spec': DL.pair_spec(D, DD)}})
    # a shard of another metagame: an opponent's pool is no deck of the run's
    others = [x for k, x in enumerate((D, DD, SN)) if k != opp[0]]
    with pytest.raises(ValueError, match='opponent-pool bag'):
        RS.row_select({'actor': others[0]}, p, {'config': {'spec': DL.metagame_spec(decks=others)}})


def test_an_undecidable_row_fails_the_selection(tmp_path):
    sp = specs()['bo3-3']
    _, truth, rows = play(sp, range(2), every=1, keep=10 ** 6)
    sb = [k for k, t in enumerate(truth) if t[2]]
    keep = [rows[k] for k in sb[:4]] + [rows[k] for k, t in enumerate(truth) if not t[2]][:20]
    p = write_shard(str(tmp_path / 'iter_0001' / 'c00_000.npz'), keep)
    with pytest.raises(ValueError, match='4 of 24 rows the selector .* cannot decide'):
        RS.row_select({'actor': D}, p, {'config': {'spec': sp}, 'meta': {'game': 'mtg'}})


def selfplay_shards(run, spec, iters=2, matches=2):
    """Best-of-three self-play shards of `spec` (uniform priors, value-only samples on) in `run`/data, its config.json."""
    info = G.info('mtg')
    from zero import package          # (`mtg` = layout v8: written with its game_layout, docs/ENGINE.md 236.9)
    meta = {'game': 'mtg', **package.layout_stamp('mtg'), 'features': info['num_features'], 'actions': info['num_actions'],
            **data.shard_format({'shard_sparse': 1}, info)}
    for k in range(iters):
        seat = G.nn_seat('m', 4, selfplay=True, sims_fast=2, p_full=0.5, temp_moves=10, leaf_value_view='root',
                         view_samples=0.5)
        drv = G.Driver('mtg', [[seat, seat]] * matches, spec=spec, concurrency=matches, threads=1, seed=11 + k,
                       record=True, max_steps=40000)
        w = data.ShardWriter(str(run / 'data' / f'iter_{k + 1:04d}'), 'c00', rows=4096, meta=meta)
        drive(drv, {'m': UniformEvaluator()}, on_batch=lambda lb: w.add(drv.take_samples()))
        w.add(drv.take_samples())
        w.close()
    (run / 'config.json').write_text(json.dumps({'game': 'mtg', **package.layout_stamp('mtg'), 'spec': spec}))


def test_distill_sources_on_a_specialists_best_of_three_rows(tmp_path):
    """tools/distill.py --sources (this repo's wrapper, zero-core's tool) on a Delver|Doomsday best-of-three pairing
    run: a deck stew of each deck from its own teacher, sources `actor` Delver and `actor` Doomsday - every row taken
    by exactly one of them (each row's actor known), each source's agreement in the record."""
    import torch
    from zero.net import load, make_net, save
    sp = specs()['bo3-pair']
    run = tmp_path / 'spec'
    selfplay_shards(run, sp)
    info = G.info('mtg')
    paths = []
    for name, seed in (('ta', 1), ('tb', 2), ('init', 3)):
        torch.manual_seed(seed)
        t = make_net(info, 'mtg', 'tokens', 32, 1, heads=2)
        if name != 'init':
            with torch.no_grad():
                for lin in (t.gen, t.ptr):
                    lin.weight.normal_(0, 0.3)
                    lin.bias.normal_(0, 0.3)
        paths.append(str(tmp_path / f'{name}.pt'))
        save(t, paths[-1], {'iter': seed})
    d = str(run / 'data')
    src = {'student': {'init': paths[2]},
           'sources': [{'name': 'delver', 'teacher': paths[0], 'data': [d], 'select': {'actor': D}, 'weight': 1},
                       {'name': 'doomsday', 'teacher': paths[1], 'data': [d], 'select': {'actor': DD}, 'weight': 1}],
           'train': {'steps': 30, 'batch': 32, 'lr': 2e-3, 'warmup': 5, 'eval_every': 0, 'save_every': 0, 'seed': 1}}
    sf = tmp_path / 'sources.json'
    sf.write_text(json.dumps(src))
    sys.path.insert(0, os.path.join(ROOT, 'tools'))
    import distill
    out = str(tmp_path / 'stew.pt')
    assert distill.main(['--sources', str(sf), out, '--torch-threads', '1', '--normal-priority']) == 0
    s, extra = load(out, game='mtg')
    rec = extra['distill']
    a, b = rec['sources']
    assert a['train_shards'] == b['train_shards'] and a['rows'] == b['rows']
    assert a['selected'] + b['selected'] == a['rows'] and a['selected'] > 0 and b['selected'] > 0
    assert a['heldout_shards'] == b['heldout_shards'] and len(a['heldout_shards']) == 1
    h = rec['heldout']
    assert [x['step'] for x in h] == [0, 30] and set(h[-1]['sources']) == {'delver', 'doomsday'}
    for name in ('delver', 'doomsday'):
        assert h[-1]['sources'][name]['policy_kl'] < h[0]['sources'][name]['policy_kl']
