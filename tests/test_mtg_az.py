"""MTG's parts of the AlphaZero stack on top of zero-core: the engine module (`mtgz._engine`
registers the toy and `mtg`), the `mtg` game through the bindings (actions, encode, determinize,
force_chance, bots in the Rust driver), the run presets, and the evaluation deck set
(`mtgz.az.evaldecks`). The game-independent tests live in zero-core's tests/; the engine's in
engine/tests (cargo test)."""
import json

import numpy as np
import pytest

from mtgz.az import evaldecks
from mtgz.az import games as G
from mtgz.az import protocol, run
from mtgz.az.infer import UniformEvaluator, drive

SPEC = '{"max_turns": 60}'


def test_engine_module_registers_the_games():
    from mtgz import _engine as E
    assert G.engine() is E and E.game_names() == ['toy', 'mtg']
    info = G.info('mtg')
    assert info['num_actions'] == 1024 and info['num_features'] == 23388
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
        assert x.shape == (23388,) and np.isfinite(x).all()
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
    protocol.validate_samples(s, 23388)
    assert len(s['x']) > 50
    res = drv.take_results()
    assert len(res) == 2 and all(r['over'] for r in res)


def test_presets_and_defaults():
    assert {'toy-smoke', 'toy-1', 'mtg-smoke', 'mtg-1', 'mtg-2', 'mtg-2-mlp'} <= set(run.PRESETS)
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
    """layout_v3.json (encode::layout_json) through zero-core's token network: the identities the
    network decodes are the engine's cards, the pointer actions of the hand and permanent slots
    are their action ids, and a real leaf batch gets a valid answer."""
    import torch
    import mtgz.az as A
    from zero.infer import NetEvaluator
    from zero.net import make_net, tokens_layout
    info = G.info('mtg')
    lay = tokens_layout(A.LAYOUT, info['num_features'], info['num_actions'])
    assert A.LAYOUT['features'] == info['num_features'] and lay['vocab'] == 1024 and A.LAYOUT['version'] == 3
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
