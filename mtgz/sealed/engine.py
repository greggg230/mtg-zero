"""The engine's sealed functions (docs/ENGINE.md 101), from the engine module `mtgz._engine`
(rebuild it with `tools/build_py.sh` after a Rust change):

    pool(seed, collation='arena')       # the Rust port of collation.generate: boosters, promo, cards
    build(pool_names, seed, builder)    # the randomised deck builder on a pool of names
    seats(spec, seed)                   # a sealed `mtg` spec's two seats in the game `seed`
    lookup(names)                       # name -> engine card id, or why it has none
    game_log(path)                      # the EoE sealed game log on (a file) or off (None): docs/ENGINE.md 107.1
    candidates(spec, seed)              # a deck-choice spec's candidates of a game: docs/ENGINE.md 107.2

`pool` reproduces `mtgz.sealed.collation.generate` bit for bit (tests/test_sealed_rust.py); the
builder uses no human data (DESIGN.md 0, ENGINE.md 101.2).
"""
import json

from mtgz import _engine as E


def pool(seed, *, collation, boosters=6, prerelease_promo=False, paradox_haze=False):
    """{'seed', 'collation', 'boosters': [[(slot, name), ...], ...], 'promo', 'cards'}: the Rust
    port of `collation.generate(seed, collation=...)`."""
    r = json.loads(E.sealed_pool(seed, collation, boosters, prerelease_promo, paradox_haze))
    r['boosters'] = [[tuple(x) for x in b] for b in r['boosters']]
    return r


def build(pool_names, seed, builder=None):
    """The builder's deck from a pool of card names (supported EoE cards; seeded):
    {'deck', 'sideboard', 'colors', 'splash', 'lands', 'spells', 'splash_spells', 'tilt'}."""
    return json.loads(E.sealed_build(list(pool_names), seed, json.dumps(builder) if builder else ''))


def seats(spec, seed):
    """The two seats of a sealed `mtg` spec's game `seed`: [{'pool', 'deck', 'sideboard',
    'pool_seed', 'build'}, ...]."""
    return json.loads(E.sealed_seats(spec if isinstance(spec, str) else json.dumps(spec), seed))


def lookup(names):
    """{name: engine card id (int) or the reason it has none (str)} in the EoE card table."""
    return json.loads(E.eoe_lookup(list(names)))


def candidates(spec, seed):
    """A deck-choice spec's candidates of the game `seed` (docs/ENGINE.md 107.2): per seat {'pool_seed', 'explore': j or
    None, 'candidates': [build, ...]} (candidate 0 the builder's best guess), None for a given deck."""
    return json.loads(E.sealed_candidates(spec if isinstance(spec, str) else json.dumps(spec), seed))


def game_log(path):
    """Turn the EoE sealed game log on (every finished `mtg_eoe` game started after it appends a line to `path`)
    or off (None); docs/ENGINE.md 107.1, `mtgz.sealed.games`."""
    E.game_log(None if path is None else str(path))
