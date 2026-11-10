"""The 17lands EoE sealed pools and human builds in engine terms (docs/ENGINE.md 101.4,
docs/sealed_data.md 5).

**A yardstick only (DESIGN.md 0): never a training input, target, reward or prior.** It maps the
17lands pools (`seventeen_lands.load`) and their human builds to the engine's EoE card table
(`CardDb::eoe`, through the engine module: an id, or why a card has none), reports how many are
playable on this checkout, and turns a pool (and optionally one of its human builds) into an `mtg`
sealed spec that plays it (ENGINE.md 101.3; the zero-core game `mtg_eoe`, ENGINE.md 105):

    pools = seventeen_lands.load('Sealed')
    report(pools)                                    # fully supported pools / builds, blockers
    spec(pool, pool.builds[0])                       # p0 plays the human build, p1 a generated pool
    spec(pool)                                       # p0 our builder's build of the pool (per game)
    spec(pool, opponent=(other, other.builds[0]))    # against another pool's human build

    python -m mtgz.sealed.yardstick report [--event Sealed|TradSealed]

Needs `mtgz._engine` (tools/build_py.sh).
"""
import argparse
import collections
import json
import sys

from . import cards as C
from . import engine as R
from . import seventeen_lands as L


def _expand(counts):
    """{name: count} -> sorted names with repeats."""
    return sorted(n for n, k in counts.items() for _ in range(k))


def lookup(pools):
    """{name: engine id | reason} for every card of the pools and their builds."""
    names = set()
    for p in pools:
        names.update(p.cards)
        for b in p.builds:
            names.update(b.deck)
    return R.lookup(sorted(names))


def unsupported(pool, table):
    """The pool's cards the engine has no id for (sorted), from a `lookup` table."""
    return sorted(n for n in pool.cards if not isinstance(table[n], int))


def deck(build):
    """A human build's deck as engine names (basics included), sorted."""
    return _expand(build.deck)


def sideboard(pool, build):
    """A human build's sideboard: the pool minus the deck's nonbasic cards (a launch-day card
    17lands' rows missed, `Build.short`, is in it: the player had it)."""
    sb = collections.Counter(pool.cards)
    sb.subtract({n: k for n, k in build.deck.items() if n not in C.BASICS})
    if any(k < 0 for k in sb.values()):
        raise ValueError(f'pool {pool.id}: build {build.index} holds a card its pool does not')
    return _expand(+sb)


def report(pools, table=None):
    """How much of the yardstick this checkout's engine can play: pools whose every card has an
    engine id, human builds whose deck does (and of those, the 40-card ones), the unsupported cards
    by pools holding them."""
    table = lookup(pools) if table is None else table
    blocked = collections.Counter()
    full = 0
    builds = builds_ok = builds_40 = 0
    for p in pools:
        u = unsupported(p, table)
        blocked.update(u)
        full += not u
        for b in p.builds:
            builds += 1
            if all(isinstance(table[n], int) for n in b.deck):
                builds_ok += 1
                builds_40 += b.deck_size == 40
    return {
        'pools': len(pools),
        'pools_fully_supported': full,
        'builds': builds,
        'builds_deck_supported': builds_ok,
        'builds_deck_supported_40_cards': builds_40,
        'unsupported_cards': dict(blocked.most_common()),
        'reasons': {n: table[n] for n in blocked},
    }


def _seat(x):
    """A spec seat from a Pool (our builder builds it every game) or a (Pool, Build) (the human
    build as it was played)."""
    if isinstance(x, tuple):
        pool, build = x
        return {'deck': deck(build), 'sideboard': sideboard(pool, build)}
    return {'pool': _expand(x.cards)}


def spec(pool, build=None, *, opponent=None, collation='arena', builder=None, **top):
    """An `mtg` sealed spec (dict) whose seat 0 plays `pool`: its human `build` if given, else our
    builder's build of the pool (made again every game from the game seed). Seat 1 (`opponent`):
    None, a generated pool (`collation`, from the game seed: the field our self-play sees); a
    Pool, our builder's build of it; a (Pool, Build), that human build. `builder`: the builder's
    settings; `top`: other top-level spec keys (`max_turns`, ...). Every card must be supported
    (the spec fails loudly otherwise: `report` says which pools are)."""
    s = {'set': 'eoe', 'p0': _seat((pool, build) if build is not None else pool)}
    if opponent is not None:
        s['p1'] = _seat(opponent)
    if opponent is None or collation is not None:
        s['collation'] = collation
    if builder:
        s['builder'] = builder
    return {'sealed': s, **top}


def main(argv=None):
    ap = argparse.ArgumentParser(description='the 17lands EoE sealed yardstick in engine terms (ENGINE.md 101.4)')
    sub = ap.add_subparsers(dest='cmd', required=True)
    r = sub.add_parser('report', help='fully supported pools and builds on this checkout')
    r.add_argument('--event', choices=L.EVENTS, action='append', help='default: both')
    a = ap.parse_args(argv)
    for e in a.event or L.EVENTS:
        rep = report(L.load(e))
        print(e, json.dumps(rep, indent=1))
    return 0


if __name__ == '__main__':
    sys.exit(main())
