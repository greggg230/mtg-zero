"""Constructed decklists of `data/decks/` (docs/ENGINE.md 54) and the `mtg` specs that play them.

A decklist is a JSON file with its provenance (`source`, `event`, `date`, `player`, ...) and `main` /
`sideboard` as [count, card name] with the engine's card names (the MTGO export, verbatim, sits next to
it; the engine's `az::constructed` reads the same file and checks the two agree). A constructed spec
(`"constructed": true`) plays fixed decklists with the card table of constructed play (the cube's
cards and the legacy ones of `engine/src/az/legacy_cards.txt`: Murktide Regent, Prismari Charm, the
Legacy Doomsday deck's ten, ENGINE.md 124); the engine checks CR 100.2a and 100.4a (at least 60 cards, at
most four of a card but basic lands, a sideboard of at most 15). Best-of-one plays the mains; best-of-three
(`"format": "bo3"`, docs/ENGINE.md 120) sideboards between games: each pool is exactly the 75, every deck
exactly 60 of it, game 1 the mains (open) and each later game's 60 hidden from the other player within the
public 75. A metagame (`"metagame"`, docs/ENGINE.md 176: `metagame_spec`) plays every ordered pairing of a
list of named 75s, each game's pairing drawn by the engine from its seed; `pair_spec` is one fixed pairing.

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

# The Legacy Sneak and Show deck (ENGINE.md 123): MTGGoldfish deck 8014220 (data/decks/sneak_show_8014220.json).
SNEAK_SHOW = 'sneak_show_8014220'

# The three-deck Legacy metagame (docs/ENGINE.md 176): Izzet Delver, Doomsday, Sneak and Show.
META3 = (IZZET_DELVER, LEGACY_DOOMSDAY, SNEAK_SHOW)


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


# (The smoke's sideboard of the 75's then-supported cards, DELVER_SMOKE_SIDEBOARD, is gone: since its five
# sideboard cards outside the cube are supported (docs/ENGINE.md 122) the smoke plays the full 75 too, 129.2.)


def mirror_bo3_spec(name=IZZET_DELVER, sideboard=None, max_turns=60, mulligans=True, game_episodes=False,
                    start_game=1, keep_deck=None, keep_known=False, dealt_decks=None):
    """The `mtg` spec of a constructed best-of-three mirror (docs/ENGINE.md 120): both seats the
    decklist's main (exactly 60) and `sideboard` ([[count, name], ...]; None: the decklist's own 15),
    `"format": "bo3"` (JSON); `game_episodes`: each game its own episode, labelled by its own outcome
    (`"game_episodes": true`, ENGINE.md 121); `start_game` 2: post-sideboard games only, each match starting
    at the sideboarding before game 2 with game 1's result drawn (`"start_game": 2`, ENGINE.md 130; the
    engine refuses it without `game_episodes`); `keep_deck`: the seats that keep their deck at every
    sideboarding (`"keep_deck"`, ENGINE.md 130: the yardstick's); `keep_known`: the other seat's worlds know they keep
    (`"keep_known"`, ENGINE.md 141: the yardstick's informed arm); `dealt_decks`: {'p': share, 'swaps': [lo, hi]}
    (`"dealt_decks"`, ENGINE.md 154: in that share of matches one seat's 60s are dealt from its 75 - a self-play
    setting, the evaluation spec drops it). The engine refuses a spec holding a card it
    does not support, naming every one (no silent drop, no substitution)."""
    d = load(name)
    main = names(d['main'])
    side = names(d['sideboard'] if sideboard is None else sideboard)
    if len(main) != 60:
        raise ValueError(f'decklist {name}: {len(main)} maindeck cards (constructed best-of-three: exactly 60)')
    full = names(d['sideboard'])
    for card in set(side):
        if side.count(card) > full.count(card):
            raise ValueError(f'decklist {name}: {side.count(card)} {card} in the sideboard, the 75 has {full.count(card)}')
    spec = {'max_turns': max_turns, 'mulligans': mulligans, 'constructed': True, 'format': 'bo3'}
    if game_episodes:
        spec['game_episodes'] = True
    if start_game != 1:
        spec['start_game'] = start_game
    if keep_deck:
        spec['keep_deck'] = sorted(keep_deck)
    if keep_known:
        spec['keep_known'] = True
    if dealt_decks:
        spec['dealt_decks'] = {'p': dealt_decks['p'], 'swaps': list(dealt_decks['swaps'])}
        if dealt_decks.get('per_seat'):
            spec['dealt_decks']['per_seat'] = True     # (each seat dealt on its own, ENGINE.md 168.1)
    spec['decks'] = {'p0': main, 'p1': main, 'p0_sideboard': side, 'p1_sideboard': side}
    return json.dumps(spec, separators=(',', ':'))


def selfplay_spec(run, it):
    """zero-core's `selfplay_specs` hook for `mtg` (its docs/az.md 40; docs/ENGINE.md 147.4): the run's own spec
    (None), refused when it holds `"keep_deck"`. A keeping seat's only sideboarding option is `PASS` (ENGINE.md
    130.3), an evaluation setting: in self-play every row of that seat's sideboarding would train a forced decision
    and the other seat's worlds would deal it sideboarded 60s it never plays. zero.run calls the hook before every
    self-play phase - local, rolling and remote alike - so such a run stops at its first self-play with exit status
    78 (refused, not restarted)."""
    spec = run.cfg.get('spec') or ''
    if spec and 'keep_deck' in json.loads(spec):
        from zero.run import EncodingMismatch

        class KeepDeckRefused(EncodingMismatch):
            """A self-play spec with `keep_deck` (refused like an EncodingMismatch: exit 78, not restarted)."""

        raise KeepDeckRefused(f'iteration {it}: the self-play spec holds "keep_deck" - an evaluation setting (the '
                              f'yardstick mtgz.az.keep_h2h plays it), never self-play: the keeping seat\'s rows would '
                              f'train forced sideboarding decisions (docs/ENGINE.md 130.3, 147.4)')
    # the matchup model's choice table (docs/ENGINE.md 168.3; matchup_every 0: None, the run's spec)
    from .matchup import selfplay_spec as matchup_spec
    s = matchup_spec(run, it)
    # then the forced find's p of the iteration (docs/ENGINE.md 180; forced_find_schedule {}: None, the spec so far)
    from .forced_find import selfplay_spec as forced_find_spec
    f = forced_find_spec(run, it, s)
    return f if f is not None else s


def _seat_lists(name, fmt):
    """(main, sideboard) names of decklist `name` as a seat of format `fmt` plays it: 'bo1' the main (at least 60; no
    sideboard, as `mirror_spec`), 'bo3' the main (exactly 60) and the 75's sideboard (ENGINE.md 120)."""
    d = load(name)
    main = names(d['main'])
    if fmt == 'bo1':
        if len(main) < 60:
            raise ValueError(f'decklist {name}: {len(main)} maindeck cards (constructed: at least 60)')
        return main, []
    if fmt == 'bo3':
        if len(main) != 60:
            raise ValueError(f'decklist {name}: {len(main)} maindeck cards (constructed best-of-three: exactly 60)')
        return main, names(d['sideboard'])
    raise ValueError(f"format {fmt!r}: 'bo1' or 'bo3'")


def _format_keys(fmt, max_turns, mulligans, game_episodes, start_game):
    spec = {'max_turns': max_turns, 'mulligans': mulligans, 'constructed': True}
    if fmt == 'bo3':
        spec['format'] = 'bo3'
        if game_episodes:
            spec['game_episodes'] = True
        if start_game != 1:
            spec['start_game'] = start_game
    elif game_episodes or start_game != 1:
        raise ValueError('game_episodes and start_game are best-of-three settings (ENGINE.md 121, 130)')
    return spec


def _dealt(dealt_decks):
    """The spec's `"dealt_decks"` of {'p', 'swaps', 'per_seat'?} (docs/ENGINE.md 154, 168.1)."""
    out = {'p': dealt_decks['p'], 'swaps': list(dealt_decks['swaps'])}
    if dealt_decks.get('per_seat'):
        out['per_seat'] = True
    return out


def pair_spec(p0, p1, fmt='bo1', max_turns=60, mulligans=True, game_episodes=False, start_game=1, dealt_decks=None):
    """The `mtg` spec of one fixed pairing (docs/ENGINE.md 176's evaluation cells): seat 0 plays decklist `p0`, seat 1
    `p1` (names of data/decks/; the same name is that deck's mirror). `fmt` 'bo1': the mains, no sideboards (as
    `mirror_spec`); 'bo3': constructed best-of-three, each seat's main (exactly 60) and its 75's sideboard
    (`game_episodes`, `start_game` as `mirror_bo3_spec`; the engine refuses `start_game` 2 for two different 75s,
    ENGINE.md 147.4); `dealt_decks` as `metagame_spec`'s. Its games are exactly the metagame spec's games of that pairing
    (`metagame_spec`: the engine's `Metagame::pair_json`). JSON."""
    spec = _format_keys(fmt, max_turns, mulligans, game_episodes, start_game)
    if dealt_decks:
        spec['dealt_decks'] = _dealt(dealt_decks)
    (m0, s0), (m1, s1) = _seat_lists(p0, fmt), _seat_lists(p1, fmt)
    decks = {'p0': m0, 'p1': m1}
    if s0:
        decks['p0_sideboard'] = s0
    if s1:
        decks['p1_sideboard'] = s1
    spec['decks'] = decks
    return json.dumps(spec, separators=(',', ':'))


def metagame_spec(decks=META3, fmt='bo1', weights=None, max_turns=60, mulligans=True, game_episodes=False,
                  start_game=1, dealt_decks=None):
    """The `mtg` spec of a metagame (docs/ENGINE.md 176): `decks` (names of data/decks/, each its own 75) and every
    game's (best-of-three: every match's) ordered pairing (p0's deck, p1's deck) drawn by the engine from that game's
    seed - so every machine that plays a seed plays its pairing. `weights`: None = uniform over the n x n ordered
    pairs, mirrors included (each cell the same share of games); a {(p0 name, p1 name): integer} dict (unlisted cells
    1, 0 = never played) or an n x n list of integers (row p0's deck, column p1's) - relative integer weights, e.g. 2
    plays a lagging cell twice as often. `fmt` 'bo1': each deck its main (no sideboard, as `mirror_spec`); 'bo3':
    constructed best-of-three, each 75 (`game_episodes`; `start_game` 2 only when every cell of positive weight is a
    mirror, ENGINE.md 147.4). `dealt_decks` ({'p', 'swaps', 'per_seat'}; best-of-three, docs/ENGINE.md 154, 168.1,
    223): each seat's 60s dealt from its own 75. A deck's choice table is its entry's "table" (docs/ENGINE.md 223: the
    matchup model's hook writes them, `matchup.with_tables`). The engine refuses `sideboard_table`, keep_deck and hidden
    decklists with it. The evaluation spec is the same spec without dealt decks, tables and the forced find
    (`evaldecks.eval_spec`). JSON."""
    decks = list(decks)
    if len(set(decks)) != len(decks) or not decks:
        raise ValueError(f'metagame decks {decks}: distinct names, at least one')
    entries = []
    for name in decks:
        main, side = _seat_lists(name, fmt)
        e = {'name': name, 'main': main}
        if side:
            e['sideboard'] = side
        entries.append(e)
    meta = {'decks': entries}
    if weights is not None:
        n = len(decks)
        if isinstance(weights, dict):
            unknown = [k for k in weights if not (isinstance(k, tuple) and len(k) == 2 and set(k) <= set(decks))]
            if unknown:
                raise ValueError(f'metagame weights: cells {unknown} are not (p0 name, p1 name) pairs of {decks}')
            w = [[weights.get((a, b), 1) for b in decks] for a in decks]
        else:
            w = [list(r) for r in weights]
        if len(w) != n or any(len(r) != n for r in w):
            raise ValueError(f'metagame weights: a {n} x {n} matrix (row p0, column p1)')
        if any(not isinstance(x, int) or isinstance(x, bool) or x < 0 for r in w for x in r) or not any(x for r in w for x in r):
            raise ValueError(f'metagame weights {w}: non-negative integers, at least one positive')
        meta['weights'] = w
    spec = _format_keys(fmt, max_turns, mulligans, game_episodes, start_game)
    if dealt_decks:
        spec['dealt_decks'] = _dealt(dealt_decks)
    spec['metagame'] = meta
    return json.dumps(spec, separators=(',', ':'))


def pairing_spec(a, b, fmt='bo3', max_turns=60, mulligans=True, game_episodes=True, start_game=1, dealt_decks=None):
    """A stew specialist's self-play spec (docs/ENGINE.md 223; the stew brief's "the best-of-three metagame restricted
    to its pairing"): `metagame_spec` of decks [a, b] with weights [[0, 1], [1, 0]] - every match a against b, a's seat
    drawn from the match's seed - or, for a mirror (a == b), decks [a] with weights [[1]]. Each match is exactly its
    pairing's fixed spec's match of the seed (`pair_spec`, the engine's `Metagame::pair_json`), the per-deck settings
    (dealt decks from each seat's own 75, a deck's choice table) passed through. JSON."""
    decks, w = ([a], [[1]]) if a == b else ([a, b], [[0, 1], [1, 0]])
    return metagame_spec(decks, fmt=fmt, weights=w, max_turns=max_turns, mulligans=mulligans,
                         game_episodes=game_episodes, start_game=start_game, dealt_decks=dealt_decks)


def mirror_spec(name=IZZET_DELVER, max_turns=60, mulligans=True):
    """The `mtg` spec of a mirror: both seats the decklist's maindeck, no sideboards, best-of-one,
    constructed (JSON)."""
    main = names(load(name)['main'])
    if len(main) < 60:
        raise ValueError(f'decklist {name}: {len(main)} maindeck cards (constructed: at least 60)')
    return json.dumps({'max_turns': max_turns, 'mulligans': mulligans, 'constructed': True,
                       'decks': {'p0': main, 'p1': main}}, separators=(',', ':'))
