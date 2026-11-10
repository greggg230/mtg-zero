"""The 17lands EoE sealed yardstick in engine terms (mtgz/sealed/yardstick.py, docs/ENGINE.md
101.4) and the engine's sealed builder and spec through the bindings (101.2-101.3). A yardstick
only: nothing here feeds training. Needs `mtgz._engine` (tools/build_py.sh)."""
import collections
import json

import numpy as np
import pytest

from mtgz.sealed import cards as C
from mtgz.sealed import engine as R
from mtgz.sealed import seventeen_lands as L
from mtgz.sealed import yardstick as Y


@pytest.fixture(scope='module')
def sealed():
    pools = L.load('Sealed')
    return pools, Y.lookup(pools)


def test_pools_map_to_engine_ids(sealed):
    pools, table = sealed
    rep = Y.report(pools, table)
    assert rep['pools'] == 9765
    # Paradox Haze has no IR; every other card of the set has an engine id but the ones on other
    # branches (this base: The Dominion Bracelet).
    assert 'Paradox Haze' in rep['unsupported_cards'] and table['Paradox Haze'].startswith('unknown card')
    assert set(rep['unsupported_cards']) <= {'Paradox Haze', 'The Dominion Bracelet'}
    assert rep['pools_fully_supported'] >= 9100
    assert rep['builds_deck_supported_40_cards'] <= rep['builds_deck_supported'] <= rep['builds']
    ids = [v for v in table.values() if isinstance(v, int)]
    assert len(ids) == len(set(ids))                       # one id per name


def _supported_pool(pools, table, size=40):
    for p in pools:
        if not Y.unsupported(p, table) and not p.incomplete:
            for b in p.builds:
                if b.deck_size == size:
                    return p, b
    raise AssertionError('no supported pool')


def test_a_human_build_plays_as_built_against_a_generated_pool(sealed):
    pools, table = sealed
    pool, build = _supported_pool(pools, table)
    spec = Y.spec(pool, build, max_turns=40)
    seats = R.seats(spec, 7)
    assert seats[0]['deck'] == Y.deck(build) and len(seats[0]['deck']) == 40
    assert sorted(seats[0]['deck'] + seats[0]['sideboard']) == sorted(
        Y.deck(build) + Y.sideboard(pool, build))
    assert 'build' not in seats[0] and seats[0]['pool_seed'] is None      # played as given
    assert seats[1]['pool_seed'] is not None and len(seats[1]['deck']) == 40
    # The same pool built by our builder instead: a 40-card deck from it, another per game seed.
    ours = Y.spec(pool, max_turns=40)
    a, b = R.seats(ours, 1)[0], R.seats(ours, 2)[0]
    for s in (a, b):
        assert len(s['deck']) == 40 and s['build']['colors']
        nonbasic = [n for n in s['deck'] if n not in C.BASICS]
        assert collections.Counter(nonbasic + s['sideboard']) == collections.Counter(pool.cards)
    assert R.seats(ours, 1) == R.seats(ours, 1)


def test_a_yardstick_spec_plays_through_the_engine(sealed):
    from mtgz.az import games as G
    pools, table = sealed
    pool, build = _supported_pool(pools, table)
    other, ob = _supported_pool(pools[100:], table)
    E = G.engine()
    g = E.Game('mtg', json.dumps(Y.spec(pool, build, opponent=(other, ob), max_turns=20)), 2, 3)
    rng = np.random.default_rng(0)
    steps = 0
    while not g.over and steps < 20000:
        legal = g.legal()
        g.step(int(legal[rng.integers(len(legal))]))
        steps += 1
    assert g.over


def test_the_builder_through_the_bindings():
    p = R.pool(5, collation='arena')
    names = p['cards']
    if any(not isinstance(v, int) for v in R.lookup(names).values()):
        pytest.skip('pool 5 holds a card unsupported on this checkout')
    b = R.build(names, 1)
    assert len(b['deck']) == 40 and 16 <= b['lands'] <= 18
    assert R.build(names, 1) == b
    assert R.build(names, 1, {'lands': [17, 17], 'splash': 0})['lands'] == 17
    with pytest.raises(ValueError):
        R.build(names + ['Lightning Bolt'], 1)
    with pytest.raises(ValueError):
        R.build(names, 1, {'flavour': 2})
