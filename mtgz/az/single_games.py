"""How many single games ONE game id of an `mtg` spec plays (zero-core docs/az.md 82, its package hook `id_games`;
docs/ENGINE.md 256): throughput is counted in single games, and a best-of-three id is a whole match of two or three.

The format is asked of the engine, never read off the spec's text here: a game is made from the spec
(`MtgSpec::parse` - its default format, its spellings, its refusals) and its exported state names the match's format
(`match.format`, `view.rs`: the parsed `Format`) and whether it chains its episodes (`Game.chained`: the spec's
`game_episodes`). The answer:

* best-of-one: 1 - an id is its game;
* best-of-three with `game_episodes`: None here - zero-core never asks the hook for a chained spec; each row and
  record lists the match's episodes, one per game played, and their number is the count;
* best-of-three without `game_episodes`: None - one episode is the whole match, and nothing an id, a row or a record
  carries says whether it took two games or three. Not counted, never estimated.

`team_draft` declares nothing (an id is a whole draft and its matches): not counted."""
import json

_FORMATS = {}


def spec_format(game, spec):
    """What the engine says of `spec` for `game`: ('bo1' | 'bo3', chained) from a game made of it (cached)."""
    from zero import games as G
    key = (G.engine().__name__, game, spec)
    got = _FORMATS.get(key)
    if got is None:
        g = G.engine().Game(game, spec, 2, 0)
        got = _FORMATS[key] = (json.loads(g.export_state())['match']['format'], bool(g.chained))
    return got


def id_games(game, spec):
    """The single games of one id of `game` with `spec`: 1 for the engine's best-of-one, else None (the module's
    docstring)."""
    fmt, chained = spec_format(game, spec)
    return 1 if fmt == 'bo1' and not chained else None


def mtg(spec):
    return id_games('mtg', spec)


def mtg_v7(spec):
    return id_games('mtg_v7', spec)


def mtg_eoe(spec):
    return id_games('mtg_eoe', spec)


# (`GamePackage(id_games=...)`: every game of `mtg`'s rules; `team_draft` is left out on purpose)
ID_GAMES = {'mtg': 'mtgz.az.single_games:mtg', 'mtg_v7': 'mtgz.az.single_games:mtg_v7',
            'mtg_eoe': 'mtgz.az.single_games:mtg_eoe'}
