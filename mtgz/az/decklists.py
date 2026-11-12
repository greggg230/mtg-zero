"""Constructed decklists of `data/decks/` (docs/ENGINE.md 54) and the `mtg` specs that play them.

A decklist is a JSON file with its provenance (`source`, `event`, `date`, `player`, ...) and `main` /
`sideboard` as [count, card name] with the engine's card names (the MTGO export, verbatim, sits next to
it; the engine's `az::constructed` reads the same file and checks the two agree). A constructed spec
(`"constructed": true`) plays fixed decklists with the card table of constructed play (the cube's
cards and the legacy ones of `engine/src/az/legacy_cards.txt`: Murktide Regent, Prismari Charm, the
Legacy Doomsday deck's ten, ENGINE.md 124), best-of-one; the engine checks CR 100.2a and 100.4a (at
least 60 cards, at most four of a card but basic lands, a sideboard of at most 15).

Kept light (no torch / numpy): `json` only.
"""
import json
import os

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DECKS = os.path.join(ROOT, 'data', 'decks')

# The Delver mirror's pinned deck (ENGINE.md 54): "Izzet Delver" by i_b_TRUE, Legacy Challenge 32,
# 2026-10-23, 1st place (8-1), MTGO; https://www.mtggoldfish.com/deck/8016541
IZZET_DELVER = 'izzet_delver_8016541'

# The Legacy Doomsday deck (ENGINE.md 124.4): MTGGoldfish's archetype "Legacy Doomsday" (paper), deck
# 8027957, "Sultai Doomsday" by wonderPreaux, North American Eternal Weekend - Legacy Championship,
# 2026-10-31, 7th place (11-2-2); https://www.mtggoldfish.com/deck/8027957. Its Cavern of Souls' floating mana
# is encoded by layout v7 (ENGINE.md 124.2, 125): the `mtg` game plays it.
LEGACY_DOOMSDAY = 'doomsday_8027957'


def load(name):
    """The decklist `data/decks/<name>.json` (a dict)."""
    with open(os.path.join(DECKS, name + '.json'), encoding='utf-8') as f:
        d = json.load(f)
    for k in ('name', 'player', 'event', 'date', 'source', 'main', 'sideboard'):
        if k not in d:
            raise ValueError(f'decklist {name}: no {k}')
    return d


def names(entries):
    """[[count, name], ...] -> the names, expanded."""
    out = []
    for n, card in entries:
        if not isinstance(n, int) or n < 1:
            raise ValueError(f'decklist entry {n!r} {card!r}')
        out += [card] * n
    return out


def mirror_spec(name=IZZET_DELVER, max_turns=60, mulligans=True):
    """The `mtg` spec of a mirror: both seats the decklist's maindeck, no sideboards, best-of-one,
    constructed (JSON)."""
    main = names(load(name)['main'])
    if len(main) < 60:
        raise ValueError(f'decklist {name}: {len(main)} maindeck cards (constructed: at least 60)')
    return json.dumps({'max_turns': max_turns, 'mulligans': mulligans, 'constructed': True,
                       'decks': {'p0': main, 'p1': main}}, separators=(',', ':'))
