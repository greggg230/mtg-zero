"""The deck target and the deck reader's declared inputs (docs/ENGINE.md 102.4), through the engine
module: the package declares the target, the declaration reads layout v6's bags and global blocks only,
and the target is the opponent's unseen cards with hidden decklists, never in the features."""
import numpy as np

import mtgz.az as P
from mtgz.az import deck_reader as R
from mtgz.az import games as G


def test_the_package_declares_the_deck_target():
    assert P.PACKAGE.aux_of('mtg')[1] == ['deck', 1024, 'multinomial']
    from mtgz.az.run import DEFAULTS
    assert DEFAULTS['aux_deck_w'] == 0.0


def test_the_declared_inputs_are_bags_and_globals_of_layout_v6():
    d = R.declaration()
    assert d['target'] == 'deck' and d['layout'] == 6 and d['features'] == G.info('mtg')['num_features']
    end = 0
    for o, n in d['inputs']:
        assert n > 0 and o >= end
        end = o + n
    assert end <= d['features'] and len(d['names']) == len(d['inputs'])
    r = R.READERS['mtg'][0]
    assert r['target'] == 'deck' and r['inputs'] == [list(x) for x in d['inputs']] and r['enter'] == 'trunk'


def test_the_deck_target_is_the_unseen_deck_with_hidden_decklists():
    spec = '{"hidden_decklists": true}'
    differ = 0
    for seed in range(4):
        g = G.engine().Game('mtg', spec, 2, seed)
        me = g.to_move
        t = np.asarray(g.aux_target(me))[1024:]
        # (before any play nothing of the opponent's 40 cards is known)
        assert t.sum() == 40
        x = np.asarray(g.encode(me))
        for k in range(3):
            d = g.clone()
            d.determinize(me, 100 + k)
            assert np.array_equal(np.asarray(d.encode(me)), x)
            td = np.asarray(d.aux_target(me))[1024:]
            assert td.sum() == 40
            differ += not np.array_equal(td, t)
    assert differ >= 10
