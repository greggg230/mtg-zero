"""The EoE network's input and run setup (docs/ENGINE.md 105): the game `mtg_eoe` (`mtg` on the EoE card table in
its own identity space) through the bindings and zero-core's driver, its layout `layout_eoe.json` and the encoding
guard between it and the cube's `mtg`, the EoE space's names (the belief target's and the replays'), and the
presets."""
import collections
import json

import numpy as np
import pytest

import mtgz.az as P
from mtgz.az import belief as B
from mtgz.az import games as G
from mtgz.az import run
from mtgz.az.infer import UniformEvaluator, drive

SPEC = P.SPEC_EOE


def positions(n=30, seed=0, spec=SPEC):
    """Mid-game states of random play of `mtg_eoe`."""
    rng = np.random.default_rng(seed)
    out = []
    s = 0
    while len(out) < n:
        g = G.engine().Game('mtg_eoe', spec, 2, s)
        s += 1
        for _ in range(int(rng.integers(1, 120))):
            if g.over:
                break
            legal = g.legal()
            g.step(legal[int(rng.integers(len(legal)))])
        if not g.over:
            out.append(g)
    return out


def test_mtg_eoe_is_registered_with_its_layout_and_targets():
    E = G.engine()
    assert 'mtg_eoe' in E.game_names()
    a, b = G.info('mtg'), G.info('mtg_eoe')
    for k in ('num_features', 'num_actions', 'min_players', 'max_players', 'bots', 'aux_layout'):
        assert a[k] == b[k], k
    assert P.PACKAGE.aux_of('mtg_eoe') == P.PACKAGE.aux_of('mtg')
    lay = P.PACKAGE.tokens_of('mtg_eoe')
    assert lay is P.LAYOUT_EOE and lay['name'] == 'eoe' and lay['version'] == 6
    # layout v6 but its name and card names
    strip = lambda d: {k: v for k, v in d.items() if k not in ('name', 'cards')}  # noqa: E731
    assert strip(lay) == strip(P.LAYOUT) and 'name' not in P.LAYOUT
    assert lay['cards'][:3] == ['Anticausal Vestige', 'Tezzeret, Cruel Captain', 'All-Fates Stalker']
    assert P.PACKAGE.spec_of('mtg_eoe') == SPEC and json.loads(SPEC)['sealed']['collation'] == 'arena'
    assert json.loads(P.PACKAGE.eval_spec_of('mtg_eoe', SPEC))['decks'] == 'eval'


def test_each_game_refuses_the_others_specs():
    E = G.engine()
    with pytest.raises(Exception, match='mtg_eoe'):
        E.Game('mtg', SPEC, 2, 1)
    for spec in ('{"max_turns": 60}', P.SPEC_DELVER):
        with pytest.raises(Exception, match='sealed'):
            E.Game('mtg_eoe', spec, 2, 1)
        E.Game('mtg', spec, 2, 1)
    E.Game('mtg_eoe', P.EVAL_SPEC_EOE, 2, 1)


def test_the_guard_refuses_a_cube_network_on_eoe_and_the_reverse():
    """ENGINE.md 61, 105: a network of `mtg` (the cube's and the Delver mirror's space) is refused by `mtg_eoe` and
    an EoE network by `mtg` (by the game: zero-core's guard is per game); each is taken by its own game, and an EoE
    network records its layout's name."""
    from zero.net import EncodingMismatch, check_encoding, encoding_mismatch, make_net
    cube = make_net(G.info('mtg'), 'mtg', 'tokens', 16, 1, heads=2)
    eoe = make_net(G.info('mtg_eoe'), 'mtg_eoe', 'tokens', 16, 1, heads=2)
    assert eoe.config['layout']['name'] == 'eoe' and eoe.config['layout']['version'] == 6
    assert 'name' not in cube.config['layout']
    assert encoding_mismatch(cube.config) is None and encoding_mismatch(eoe.config) is None
    with pytest.raises(EncodingMismatch, match="game 'mtg' -> 'mtg_eoe'"):
        check_encoding(cube.config, 'mtg_eoe')
    with pytest.raises(EncodingMismatch, match="game 'mtg_eoe' -> 'mtg'"):
        check_encoding(eoe.config, 'mtg')


def test_the_belief_target_names_eoe_cards_and_hides_nothing():
    """The belief target (ENGINE.md 66) on `mtg_eoe` is the opponent's hand by EoE identity: `belief.eoe_names()`
    names it as the spectator view does; worlds determinized for the decider encode alike."""
    nm = B.eoe_names()
    assert nm[0] == 'Anticausal Vestige' and nm[B.EOE_CAP - 1] is None and nm[1023] is None
    differ = 0
    for g in positions(20, 2):
        me = g.to_move
        st = json.loads(g.export_state())
        opp = [p for p in st['players'] if p['id'] == 1 - me][0]
        t = np.asarray(g.aux_target(me))
        assert t[1023] == 0
        assert collections.Counter({nm[i]: int(t[i]) for i in np.flatnonzero(t)}) == \
            collections.Counter(c['name'] for c in opp['hand'])
        x = np.asarray(g.encode(me))
        for k in range(2):
            d = g.clone()
            d.determinize(me, 1000 + k)
            assert np.array_equal(np.asarray(d.encode(me)), x)
            differ += not np.array_equal(np.asarray(d.aux_target(me)), t)
    assert differ > 10
    out = B.render_eoe({'belief': np.full(1024, 1 / 1024)}, {'players': [{'id': 1, 'hand': [{}] * 4}]}, 0, {})
    assert out['belief']['n'] == 4 and len(out['belief']['cards']) == 12


def test_selfplay_and_bots_play_mtg_eoe():
    """zero-core's driver: search seats (a uniform evaluator) and the heuristic and random bots play `mtg_eoe` games
    to the end, recording samples of the layout's width; the belief target with them when asked."""
    seat = G.nn_seat('m', 4, selfplay=True, sims_fast=2, p_full=0.5, temp_moves=10, sims_minor=2)
    drv = G.Driver('mtg_eoe', [[seat, seat]] * 2, spec=SPEC, concurrency=2, seed=3, record=True, threads=2, aux=True)
    drive(drv, {'m': UniformEvaluator()})
    res = drv.take_results()
    s = drv.take_samples()
    assert len(res) == 2 and all(r['over'] for r in res)
    assert s['x'].shape[1] == G.info('mtg_eoe')['num_features'] and s['aux'].shape[1] == 1024
    assert s['aux'][:, 1023].sum() == 0
    h, r = G.seat_dict('heuristic'), G.seat_dict('random')
    bots = G.Driver('mtg_eoe', [[h, r]] * 4, spec=P.EVAL_SPEC_EOE, concurrency=4, seed=5, threads=2)
    drive(bots, {})
    r = bots.take_results()
    assert len(r) == 4 and all(x['over'] for x in r)


def test_the_eoe_presets():
    for name in ('eoe-smoke', 'eoe-1'):
        c = run.resolve_config(run.parse(['--run', 'x', '--preset', name]), {})
        assert c['game'] == 'mtg_eoe' and c['spec'] == SPEC and c['eval_spec'] == P.EVAL_SPEC_EOE
        assert c['yardstick'] == 'heuristic' and c['eval_random_games'] > 0 and c['arch'] == 'tokens'
        # no human data in a run: the spec's pools are generated, the builder has no ratings
        assert '17lands' not in json.dumps(c) and 'seventeen' not in json.dumps(c)
    big = P.PRESETS['eoe-1']
    assert (big['hidden'], big['blocks'], big['heads'], big['ff']) == (128, 4, 4, 256)
    assert big['infer_device'] == 'cuda' and big['train_device'] == 'cuda'
