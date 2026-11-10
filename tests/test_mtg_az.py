"""MTG's parts of the AlphaZero stack on top of zero-core: the engine module (`mtgz._engine`
registers the toy and `mtg`), the `mtg` game through the bindings (actions, encode, determinize,
force_chance, bots in the Rust driver), the run presets, and the evaluation deck set
(`mtgz.az.evaldecks`). The game-independent tests live in zero-core's tests/; the engine's in
engine/tests (cargo test)."""
import json

import numpy as np
import pytest

import mtgz.az
from mtgz.az import evaldecks
from mtgz.az import games as G
from mtgz.az import protocol, run
from mtgz.az.infer import UniformEvaluator, drive

SPEC = '{"max_turns": 60}'
MATCH = '{"max_turns": 60, "format": "bo3"}'


def nf():
    return G.info('mtg')['num_features']


def test_engine_module_registers_the_games():
    from mtgz import _engine as E
    assert G.engine() is E and E.game_names() == ['toy', 'mtg', 'team_draft', 'mtg_eoe']
    info = G.info('mtg')
    assert info['num_actions'] == 1024 and info['num_features'] == 44212      # layout v6 (ENGINE.md 100.5)
    assert info['min_players'] == info['max_players'] == 2
    assert info['bots'] == ['random', 'heuristic']


def test_game_through_the_bindings():
    E = G.engine()
    rng = np.random.default_rng(1)
    g = E.Game('mtg', SPEC, 2, 7)
    steps = 0
    while not g.over:
        legal = g.legal()
        assert len(legal) >= 1 and list(legal) == sorted(set(legal)) and max(legal) < 1024
        p = g.to_move
        x = np.asarray(g.encode(p))
        assert x.shape == (nf(),) and np.isfinite(x).all()
        if steps % 25 == 0:
            d = g.clone()
            d.determinize(p, 99)
            assert list(d.legal()) == list(legal)
            assert np.array_equal(np.asarray(d.encode(p)), x)          # no hidden information
        g.step(int(legal[rng.integers(len(legal))]))
        steps += 1
        assert steps < 20000
    r = g.result()
    assert abs(sum(r) - 1) < 1e-6 and g.rounds >= 1
    with pytest.raises(Exception):
        E.Game('mtg', '{"bogus": 1}', 2, 1)
    with pytest.raises(Exception):
        E.Game('mtg', SPEC, 3, 1)


def test_bots_in_the_rust_driver():
    """heuristic vs random through the driver (the evaluation path): the heuristic wins."""
    h, r = G.seat_dict('heuristic'), G.seat_dict('random')
    drv = G.Driver('mtg', [[h, r], [r, h]] * 6, spec=SPEC, concurrency=12,
                   threads=2, seed=3, max_steps=20000)
    drive(drv, {})
    res = drv.take_results()
    assert len(res) == 12 and all(r['over'] for r in res)
    score = sum(r['result'][r['gid'] % 2] for r in res) / len(res)
    assert score > 0.7, score


def test_selfplay_records_samples():
    seat = G.nn_seat('m', 4, selfplay=True, sims_fast=2, p_full=0.5, temp_moves=10)
    drv = G.Driver('mtg', [[seat, seat]] * 2, spec=SPEC, concurrency=2, threads=1, seed=5, record=True,
                   max_steps=20000)
    parts = []
    drive(drv, {'m': UniformEvaluator()}, on_batch=lambda lb: parts.append(drv.take_samples()))
    parts.append(drv.take_samples())
    s = protocol.concat_samples(parts)
    protocol.validate_samples(s, nf())
    assert len(s['x']) > 50
    res = drv.take_results()
    assert len(res) == 2 and all(r['over'] for r in res)


def test_match_through_the_bindings():
    """A best-of-three match (docs/ENGINE.md 36) through the bindings: sideboarding decisions
    between games, the match result, determinize keeping the decider's view; the heuristic beats
    random in matches through the driver."""
    E = G.engine()
    rng = np.random.default_rng(3)
    for seed in (1, 2):
        g = E.Game('mtg', MATCH, 2, seed)
        steps = 0
        while not g.over:
            legal = g.legal()
            p = g.to_move
            x = np.asarray(g.encode(p))
            if steps % 40 == 0:
                d = g.clone()
                d.determinize(p, 7)
                assert list(d.legal()) == list(legal) and np.array_equal(np.asarray(d.encode(p)), x)
            g.step(int(legal[rng.integers(len(legal))]))
            steps += 1
            assert steps < 60000
        r = g.result()
        assert abs(sum(r) - 1) < 1e-6 and r[0] in (0.0, 0.5, 1.0)
    h, r = G.seat_dict('heuristic'), G.seat_dict('random')
    drv = G.Driver('mtg', [[h, r], [r, h]] * 3, spec=MATCH, concurrency=6, threads=2, seed=3, max_steps=60000)
    drive(drv, {})
    res = drv.take_results()
    assert len(res) == 6 and all(x['over'] for x in res)
    assert sum(x['result'][x['gid'] % 2] for x in res) / len(res) > 0.7


def test_presets_and_defaults():
    assert {'toy-smoke', 'toy-1', 'mtg-smoke', 'mtg-1', 'mtg-2', 'mtg-2-mlp', 'mtg-3', 'mtg-4'} <= set(run.PRESETS)
    c3 = run.resolve_config(run.parse(['--run', 'x', '--preset', 'mtg-3']), {})
    c4 = run.resolve_config(run.parse(['--run', 'x', '--preset', 'mtg-4']), {})
    assert json.loads(c4['spec'])['format'] == 'bo3' and json.loads(c4['eval_spec']) == dict(
        json.loads(c4['spec']), decks='eval')                           # evaluation and Elo: matches
    assert {k for k in c3 if c3[k] != c4[k]} == {'spec', 'eval_spec', 'max_steps', 'preset'}
    cfg = run.resolve_config(run.parse(['--run', 'x', '--preset', 'mtg-smoke']), {})
    assert cfg['game'] == 'mtg' and cfg['yardstick'] == 'heuristic'
    assert json.loads(cfg['spec'])['max_turns'] == 60
    assert cfg['eval_spec'] == ''                                      # the package's evaluation spec
    cfg = run.resolve_config(run.parse(['--run', 'x', '--preset', 'mtg-1']), {})
    assert cfg['game'] == 'mtg' and cfg['iters'] >= 4 and cfg['infer_device'] == 'cpu'
    a = run.resolve_config(run.parse(['--run', 'x', '--preset', 'mtg-2']), {})
    b = run.resolve_config(run.parse(['--run', 'x', '--preset', 'mtg-2-mlp']), {})
    assert (a['arch'], a['policy_target'], a['sims_minor']) == ('tokens', 'cq', 4) and b['arch'] == 'mlp'
    assert {k for k in a if a[k] != b[k]} == {'arch', 'hidden', 'preset'}      # the control differs only in the network


def test_eval_spec():
    assert json.loads(evaldecks.eval_spec('')) == {'decks': 'eval'}
    s = json.loads(evaldecks.eval_spec('{"max_turns": 50, "eval_decks": 4}'))
    assert s == {'max_turns': 50, 'eval_decks': 4, 'decks': 'eval'}
    lists = '{"decks": {"p0": ["Forest"], "p1": ["Island"]}}'
    assert json.loads(evaldecks.eval_spec(lists)) == json.loads(lists)
    # the engine accepts it and plays the fixed set: every game is one of eval_decks pairs
    E = G.engine()
    E.Game('mtg', evaldecks.eval_spec(SPEC), 2, 1)


def test_evaluation_drivers_use_the_eval_decks():
    """The game package's evaluation spec (zero-core GamePackage.eval_specs: the evaluation deck
    set of the run's spec): drivers that record nothing (evaluation matches) get it, self-play
    drivers the run's spec; another game of the package (the toy) keeps its spec."""
    import mtgz.az as A
    from zero import package
    assert not hasattr(run.Run.driver, '_mtgz_eval_decks')            # no patch any more
    assert package.active().eval_spec_of('mtg', A.SPEC) == A.EVAL_SPEC == evaldecks.eval_spec(A.SPEC)
    assert package.active().eval_spec_of('toy', '{}') == '{}'
    seen = []
    real = run.G.Driver

    def spy(game, seats, spec=None, **kw):
        seen.append((kw.get('record'), spec))

    class Fake:
        cfg = dict(run.DEFAULTS, game='mtg', spec=SPEC)
    f = Fake()
    try:
        run.G.Driver = spy
        run.Run.driver(f, [['heuristic', 'random']], seed=1)
        run.Run.driver(f, [['heuristic', 'random']], seed=1, record=True)
        f.cfg['eval_spec'] = '{"max_turns": 9}'                        # a run's own setting wins
        run.Run.driver(f, [['heuristic', 'random']], seed=1)
    finally:
        run.G.Driver = real
    assert json.loads(seen[0][1])['decks'] == 'eval' and seen[0][0] is False
    assert seen[1] == (True, SPEC) and seen[2] == (False, '{"max_turns": 9}')


def test_layout_and_token_network_read_the_encoding():
    """layout_v6.json (encode::layout_json) through zero-core's token network: the identities the
    network decodes are the engine's cards, the pointer actions of the hand and permanent slots
    are their action ids, and a real leaf batch gets a valid answer."""
    import torch
    import mtgz.az as A
    from zero.infer import NetEvaluator
    from zero.net import make_net, tokens_layout
    info = G.info('mtg')
    lay = tokens_layout(A.LAYOUT, info['num_features'], info['num_actions'])
    assert A.LAYOUT['features'] == info['num_features'] and lay['vocab'] == 1024 and A.LAYOUT['version'] == 6
    segs = {s['name']: s for s in lay['segments']}
    assert segs['list']['count'] == 64 and [b['count'] for b in lay['bags']] == [23]   # M8b, v6
    assert [g['name'] for g in lay['globals']] == ['globals', 'match', 'globals_v5', 'globals_v6']   # v6 (100.5)
    names = A.LAYOUT['cards']
    torch.manual_seed(0)
    m = make_net(info, 'mtg', 'tokens', 32, 1, heads=2)
    E = G.engine()
    rng = np.random.default_rng(2)
    g = E.Game('mtg', SPEC, 2, 11)
    seen = 0
    while not g.over and seen < 40:
        legal = list(g.legal())
        x = torch.from_numpy(np.asarray(g.encode(g.to_move)))[None]
        with torch.no_grad():
            h, n, full = m.pack(x)
        segs = {s['name']: s for s in lay['segments']}
        hand = segs['hand']
        for s in range(hand['count']):
            o = hand['offset'] + s * hand['width']
            if x[0, o + hand['present']] > 0.5:
                ident = int(m._identity(x[0, o:o + hand['width']][None], hand['id'])[0])
                digits = (int(x[0, o:o + 16].argmax()), int(x[0, o + 16:o + 24].argmax()), int(x[0, o + 24:o + 32].argmax()))
                assert digits[0] * 64 + digits[1] * 8 + digits[2] == ident and names[ident]      # a card of the cube
                seen += 1
        # the slot of every legal HAND / MY_PERM / OPP_PERM id is a token
        start, pos = 1, {}
        for s in lay['segments']:
            pos[s['name']] = start
            start += s['count']
        tokens = set(full[0, :int(n[0])].tolist())
        for a in legal:
            for name, base, cnt in (('hand', 36, 16), ('my_permanents', 100, 32), ('opponent_permanents', 196, 32)):
                if base <= a < base + cnt:
                    assert pos[name] + a - base in tokens, (name, a)
        g.step(int(legal[rng.integers(len(legal))]))
    assert seen > 0
    seat = G.nn_seat('m', 4)
    drv = G.Driver('mtg', [[seat, G.seat_dict('random')]] * 6, spec=SPEC, concurrency=6, threads=1, seed=4,
                   max_steps=20000)
    lb = drv.next_batch()
    pri, val, mar = NetEvaluator(m)(lb)
    protocol.validate_evaluation(lb, pri, val, mar)


@pytest.mark.parametrize('old_version, old_features', [(4, 33652), (5, 37620)])
def test_an_older_token_network_widens_onto_layout_v6_exactly(tmp_path, old_version, old_features):
    """ENGINE.md 60.2, 100.5: a token network of layout v4 or v5 (layout_v4.json, layout_v5.json, as
    frozen), widened with zero-core's `net.widen` onto this build's layout v6, computes on any v6
    input what it computed on the old part of it (`token_feature_map`: the v4 or v5 encoding,
    tests/digest.rs DIGEST_ENCODING=v4 / v5); the same tokens. Real encodings of this build (cube
    and Delver mirror positions), their new floats then randomised."""
    import os
    import torch
    import mtgz.az as A
    from mtgz.az import decklists
    from zero.net import TokenNet, token_feature_map, tokens_layout, widen
    info = G.info('mtg')
    with open(os.path.join(os.path.dirname(A.__file__), f'layout_v{old_version}.json'), encoding='utf-8') as f:
        lo = json.load(f)
    assert lo['version'] == old_version and lo['features'] == old_features
    torch.manual_seed(3)
    old = TokenNet(old_features, info['num_actions'], lo, d=32, layers=2, heads=2, ff=64, game='mtg').eval()
    with torch.no_grad():
        for lin in (old.gen, old.ptr):
            lin.weight.normal_(0, 0.3)
            lin.bias.normal_(0, 0.3)
    new = widen(old, info['num_features'], info['num_actions'], layout=A.LAYOUT)
    assert new.n_tok == old.n_tok and new.config['layout']['bags'][0]['count'] == 23
    assert new.config['layout']['version'] == 6
    fmap = token_feature_map(old.config['layout'], old_features,
                             tokens_layout(A.LAYOUT, info['num_features'], info['num_actions']), info['num_features'])
    E = G.engine()
    rng = np.random.default_rng(60)
    xs = []
    for spec in (SPEC, decklists.mirror_spec(decklists.IZZET_DELVER, max_turns=60)):
        for seed in range(3):
            g = E.Game('mtg', spec, 2, seed)
            k = 0
            while not g.over and k < 400:
                if k % 7 == 0:
                    for v in range(2):
                        xs.append(np.asarray(g.encode(v)))
                legal = g.legal()
                g.step(int(legal[rng.integers(len(legal))]))
                k += 1
    x5 = torch.from_numpy(np.stack(xs))
    x4 = x5[:, fmap]
    newcols = torch.ones(x5.shape[1], dtype=torch.bool)
    newcols[fmap] = False
    assert int(newcols.sum()) == info['num_features'] - old_features and (x5[:, newcols] != 0).any()   # new facts present
    n = torch.full((x5.shape[0],), 2)
    with torch.no_grad():
        p, q = old(x4, n), new(x5, n)
        noisy = x5.clone()
        noisy[:, newcols] = torch.rand(x5.shape[0], int(newcols.sum())) * 4 - 2
        r = new(noisy, n)
    for a, b, c in zip(p, q, r):
        assert torch.allclose(a, b, atol=1e-5) and torch.allclose(a, c, atol=1e-5)


@pytest.mark.parametrize('old_version, old_features', [(4, 33652), (5, 37620)])
def test_the_encoding_guard_refuses_an_older_network_and_takes_a_v6_one(tmp_path, old_version, old_features):
    """ENGINE.md 61, 100.5: zero-core's encoding guard (`zero.net.check_encoding`, its docs/az.md
    10.4) on this build (layout v6): a layout v4 or v5 token network is refused wherever it is
    loaded - net.load, a run's evaluator, the replay recorder - naming both sides (33,652 or 37,620
    -> 44,212 features, the segments that moved, layout v6); its fork_run widen is taken and written
    as a v6 network; a new network records layout version 6, and a v6 network recorded as another
    version is refused."""
    import os
    import torch
    import mtgz.az as A
    from mtgz.az import replays
    from zero.net import (EncodingMismatch, TokenNet, check_checkpoint, check_encoding, encoding_mismatch, load,
                          make_net, save, widen)
    info = G.info('mtg')
    with open(os.path.join(os.path.dirname(A.__file__), f'layout_v{old_version}.json'), encoding='utf-8') as f:
        v4 = json.load(f)
    torch.manual_seed(4)
    old = TokenNet(old_features, info['num_actions'], v4, d=16, layers=1, heads=2, ff=32, game='mtg').eval()
    p4 = str(tmp_path / 'old.pt')
    save(old, p4, {'iter': 3})
    with pytest.raises(EncodingMismatch) as e:
        load(p4)
    msg = str(e.value)
    assert f'features {old_features} -> 44212' in msg and f'token layout v{old_version} (11 segments' in msg
    assert 'token layout v6 (11' in msg and 'tools/fork_run.py' in msg
    R = run.Run(str(tmp_path / 'r'), run.resolve_config(run.parse(['--run', 'x', '--preset', 'mtg-smoke']), {}))
    with pytest.raises(EncodingMismatch):
        R.evaluator(p4)
    with pytest.raises(EncodingMismatch):
        replays.check_encoding(p4)
    # fork_run's route: widened, then a network of this build's encoding (recording layout v6)
    new = widen(old, info['num_features'], info['num_actions'], layout=A.LAYOUT)
    assert new.config['layout']['version'] == 6 and encoding_mismatch(new.config) is None
    p5 = str(tmp_path / 'v6.pt')
    save(new, p5)
    check_checkpoint(p5)
    replays.check_encoding(p5)
    load(p5, game='mtg')
    m = make_net(info, 'mtg', 'tokens', 16, 1, heads=2)
    assert m.config['layout']['version'] == 6 and encoding_mismatch(m.config) is None
    with pytest.raises(EncodingMismatch, match='token layout version 7 -> 6'):
        check_encoding(dict(m.config, layout=dict(m.config['layout'], version=7)))
    # a v6 network without a recorded version: judged by its structure
    assert encoding_mismatch(dict(m.config, layout={k: v for k, v in m.config['layout'].items() if k != 'version'})) is None
    # the draft network: its own game and layout (draft_v2)
    d = make_net(G.info('team_draft'), 'team_draft', 'tokens', 16, 1, heads=2)
    assert d.config['layout']['version'] == 2 and encoding_mismatch(d.config) is None
    with pytest.raises(EncodingMismatch, match="game 'team_draft' -> 'mtg'"):
        check_encoding(d.config, 'mtg')


def test_a_bigger_network_distilled_on_delver_shards_reads_layout_v6(tmp_path):
    """ENGINE.md 62: zero-core's distillation (tools/distill.py) on Delver mirror self-play shards of
    this build: a token network of another size trained on a small teacher's answers, held out on
    another shard; the student is a layout v6 network (version recorded, the encoding guard accepts
    it), its record in the checkpoint, and it moved toward its teacher."""
    import os
    import sys
    import torch
    from mtgz.az import decklists
    from zero.data import ShardWriter
    from zero.net import load, make_net, save
    info = G.info('mtg')
    spec = decklists.mirror_spec(decklists.IZZET_DELVER, max_turns=60)
    meta = {'game': 'mtg', 'features': info['num_features'], 'actions': info['num_actions']}
    for name, seed in (('iter_0001', 3), ('iter_0002', 4)):
        seat = G.nn_seat('m', 4, selfplay=True, sims_fast=2, p_full=0.5, temp_moves=10)
        drv = G.Driver('mtg', [[seat, seat]] * 2, spec=spec, concurrency=2, threads=1, seed=seed, record=True,
                       max_steps=20000)
        w = ShardWriter(str(tmp_path / 'data' / name), 'c00', rows=4096, meta=meta)
        drive(drv, {'m': UniformEvaluator()}, on_batch=lambda lb: w.add(drv.take_samples()))
        w.add(drv.take_samples())
        assert w.close()['samples'] > 50
    torch.manual_seed(0)
    t = make_net(info, 'mtg', 'tokens', 32, 1, heads=2)
    with torch.no_grad():
        for lin in (t.gen, t.ptr):                  # a teacher with a policy (they start at zero)
            lin.weight.normal_(0, 0.3)
            lin.bias.normal_(0, 0.3)
    tp, out = str(tmp_path / 'teacher.pt'), str(tmp_path / 'student.pt')
    save(t, tp, {'iter': 3})
    sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'tools'))
    import distill
    assert distill.main([tp, out, '--data', str(tmp_path / 'data' / 'iter_0001'), '--heldout',
                         str(tmp_path / 'data' / 'iter_0002'), '--hidden', '48', '--blocks', '2', '--heads', '2',
                         '--steps', '40', '--batch', '64', '--lr', '2e-3', '--warmup', '5', '--eval-every', '0',
                         '--torch-threads', '1', '--normal-priority']) == 0
    s, extra = load(out, game='mtg')
    assert (s.config['d'], s.config['layers'], s.config['layout']['version']) == (48, 2, 6)
    h = extra['distill']['heldout']
    assert [x['step'] for x in h] == [0, 40] and h[-1]['policy_kl'] < h[0]['policy_kl']
    assert extra['teacher_extra'] == {'iter': 3} and extra['distill']['student_net']['params'] == s.n_params()


def test_replays_leaf_view():
    """Recorded games search with root view whenever the run trains with it (ENGINE.md 67), even
    when its evaluation setting is still the default mover; otherwise with the run's own."""
    from mtgz.az import replays
    assert replays.leaf_view({'selfplay_leaf_value_view': 'root', 'eval_leaf_value_view': 'mover'}) == 'root'
    assert replays.leaf_view({'selfplay_leaf_value_view': 'root'}, selfplay=True) == 'root'
    assert replays.leaf_view({'eval_leaf_value_view': 'root'}) == 'root'
    assert replays.leaf_view({'selfplay_leaf_value_view': 'mover'}) == 'mover'
    assert replays.leaf_view({}) == 'mover'

def test_replays_root_search():
    """Recorded games search with the Gumbel root whenever the run trains with it (ENGINE.md 68), as
    with the leaf view; otherwise with the run's own; a Gumbel seat's minor decisions get the minor
    budget's schedule (zero-core docs/search.md 3.7)."""
    from mtgz.az import replays
    assert replays.root_search({'selfplay_root_search': 'gumbel', 'eval_root_search': 'puct'}) == 'gumbel'
    assert replays.root_search({'selfplay_root_search': 'gumbel'}, selfplay=True) == 'gumbel'
    assert replays.root_search({'eval_root_search': 'gumbel'}) == 'gumbel'
    assert replays.root_search({'eval_root_search': 'gumbel'}, selfplay=True) == 'puct'
    assert replays.root_search({}) == 'puct'


def test_a_belief_teacher_distils_into_a_student_with_its_head_on_delver_view_rows(tmp_path):
    """ENGINE.md 62.9: the main line's shape on Delver shards of this build - the belief target recorded
    (aux) and value-only rows (view samples 0.5) - and a teacher with the belief head (a random last
    layer: a head that predicts something); zero-core's distillation gives the student the head (the
    game's targets: the belief search's check passes), distils it and reports both kinds of row; the
    student's record and log carry the head and the value-only rows."""
    import os
    import sys
    import torch
    from mtgz.az import decklists
    from zero.data import ShardWriter
    from zero.net import check_belief_head, load, make_net, save
    info = G.info('mtg')
    spec = decklists.mirror_spec(decklists.IZZET_DELVER, max_turns=60)
    meta = {'game': 'mtg', 'features': info['num_features'], 'actions': info['num_actions']}
    for name, seed in (('iter_0001', 3), ('iter_0002', 4)):
        seat = G.nn_seat('m', 4, selfplay=True, sims_fast=2, p_full=0.5, temp_moves=10, view_samples=0.5)
        drv = G.Driver('mtg', [[seat, seat]] * 2, spec=spec, concurrency=2, threads=1, seed=seed, record=True,
                       max_steps=20000, aux=True)
        w = ShardWriter(str(tmp_path / 'data' / name), 'c00', rows=4096, meta=meta)
        drive(drv, {'m': UniformEvaluator()}, on_batch=lambda lb: w.add(drv.take_samples()))
        w.add(drv.take_samples())
        assert w.close()['samples'] > 50
    torch.manual_seed(0)
    belief = mtgz.az.PACKAGE.aux_of('mtg')   # (the hand's and the deck's targets, docs/ENGINE.md 102.4)
    t = make_net(info, 'mtg', 'tokens', 32, 1, heads=2, aux=belief)
    with torch.no_grad():
        for lin in (t.gen, t.ptr, t.aux_head[2]):
            lin.weight.normal_(0, 0.3)
            lin.bias.normal_(0, 0.3)
    tp, out = str(tmp_path / 'teacher.pt'), str(tmp_path / 'student.pt')
    save(t, tp, {'iter': 3})
    sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'tools'))
    import distill
    assert distill.main([tp, out, '--data', str(tmp_path / 'data' / 'iter_0001'), '--heldout',
                         str(tmp_path / 'data' / 'iter_0002'), '--hidden', '48', '--blocks', '2', '--heads', '2',
                         '--steps', '40', '--batch', '64', '--lr', '2e-3', '--warmup', '5', '--eval-every', '0',
                         '--torch-threads', '1', '--normal-priority']) == 0
    s, extra = load(out, game='mtg')
    check_belief_head(s, 'mtg')                        # what selfplay_ / eval_belief_tilt ask of a checkpoint
    assert s.config['aux'] == belief and s.config['layout']['version'] == 6
    h = extra['distill']['heldout']
    assert h[-1]['aux']['belief']['kl'] < h[0]['aux']['belief']['kl'] and h[-1]['policy_kl'] < h[0]['policy_kl']
    assert h[-1]['view']['rows'] > 0 and h[-1]['decision']['rows'] > 0
    start = json.loads(open(out + '.log.jsonl').readline())
    assert start['aux'] == belief and start['train_view_rows'] > 0 and start['train_aux_rows'] == start['train_rows']
