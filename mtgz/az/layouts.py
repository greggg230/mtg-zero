"""Which MTG layout a run, a checkpoint, a record or a farm job is (docs/ENGINE.md 236, 236.9).

The game `mtg` is layout v8 (`layout_v8.json`) from now on: every preset, every new run, the game package's default
game, every tool that starts fresh. The game `mtg_v7` is layout v7 (`layout_v7.json`): the compatibility layout of every
run, checkpoint, farm run, campaign and record made before layout v8. Those all name `mtg` WITHOUT a layout record, and
zero-core's game alias (`mtgz.az.GAME_ALIASES` = {'mtg': {'layout': 8, 'before': 'mtg_v7'}}, zero-core docs/az.md 73)
reads every such name as `mtg_v7`; what is made from now on names `mtg` with `game_layout` 8. Inside a process every game
name is the resolved one, and a name is a layout - nothing changes layout under a run:

* **A run** plays its saved config's game: a run made before layout v8 saved `mtg` and no `game_layout`, so zero.run
  resolves it to `mtg_v7` and it resumes, re-pinned on a new main or not, as layout v7, byte for byte (its rows, its
  checkpoints, its shards, its farm jobs). A new run records `mtg` and `game_layout` 8 at its first start.
* **A checkpoint** records its game (its network config's `game`, and `game_layout` for layout v8): `load` loads it as
  that game (zero.net.load's default) and `model_game` names it, so a tool plays a network's games in that network's
  layout. Loading it as the other game is refused by zero-core's encoding guard (exit 78: "game 'mtg_v7' -> 'mtg'", the
  token layouts differ), never encoded in silence.
* **The farm**: a job carries its run's settings' game and `game_layout`; a worker built from now on plays `mtg` +
  `game_layout` 8 as layout v8 and `mtg` without one (or `mtg_v7`) as layout v7, and refuses a graph of another layout
  than its job's game. A worker of an earlier release refuses a job with `game_layout` 8 (by the run's declared setting
  classes: python_only) instead of playing layout v7 under the name.
* **A run moves** to layout v8 only by `tools/fork_run.py <its checkpoint> <new run> --game mtg` (zero-core's widen:
  v8's six segments gated at exactly 0).
"""
GAME = 'mtg'         # layout v8: the default, every new run and every tool that starts fresh (game_layout 8)
GAME_V7 = 'mtg_v7'   # layout v7: the compatibility layout of the runs, checkpoints and records made before layout v8
GAMES = (GAME, GAME_V7)


def checkpoint_game(path):
    """The game checkpoint `path` records (its network config's `game` with its `game_layout`, resolved)."""
    import torch
    from zero import package
    blob = torch.load(path, map_location='cpu', weights_only=False)
    cfg = blob.get('config') if isinstance(blob, dict) else None
    if not isinstance(cfg, dict) or not cfg.get('game'):
        raise ValueError(f'{path}: a checkpoint that records no game (docs/ENGINE.md 236: a layout is never guessed)')
    return package.record_game(cfg, str(path))


def model_game(model):
    """The game a loaded network records (its config's, resolved by zero.net.build): the layout its games are played
    and encoded in."""
    g = (getattr(model, 'config', None) or {}).get('game')
    if not g:
        raise ValueError('a network that records no game (docs/ENGINE.md 236: a layout is never guessed)')
    return g


def load(path, device='cpu', **kw):
    """zero.net.load of checkpoint `path` as the game it records (layout v7 `mtg_v7` or v8 `mtg`): (model, extra)."""
    from zero.net import load as _load
    return _load(path, device, **kw)


def same_game(*games, where='the networks'):
    """The one game of several networks (a match, a pair): refused when they record different layouts."""
    gs = sorted(set(games))
    if len(gs) != 1:
        raise ValueError(f'{where} record different games {gs}: layouts v7 and v8 cannot play one match (docs/ENGINE.md '
                         f'236; fork the v7 one onto mtg with tools/fork_run.py)')
    return gs[0]


_USED = None


def use(game):
    """This process plays and encodes MTG games of `game` from now on (a tool, after loading a checkpoint: its layout);
    a second MTG layout in one process is refused (a v7 and a v8 network cannot share games). Returns `game`."""
    global _USED
    if game in GAMES:
        if _USED not in (None, game):
            raise ValueError(f'this process plays game {_USED!r} (a checkpoint loaded before), and now {game!r}: layouts v7 '
                             f'and v8 cannot share its games (docs/ENGINE.md 236)')
        _USED = game
    return game


def current():
    """The MTG game this process plays: the one its checkpoints record (`use`, `load_used`), else layout v8's `mtg` (a
    tool that starts fresh)."""
    return _USED or GAME


def load_used(path, device='cpu', **kw):
    """`load` of checkpoint `path`, and `use` of the game it records: (model, extra)."""
    m, extra = load(path, device, **kw)
    use(model_game(m))
    return m, extra


def record_game(rec):
    """The game a game record or a stored spot names: its `game` with its `game_layout`, resolved (zero-core's game
    alias: one written before layout v8 names `mtg` and no layout record, or no game at all - layout v7's `mtg_v7`)."""
    from zero import package
    return package.record_game(rec or {}, default='mtg')


def stamp(game):
    """What a record written now names its game with: {'game': game} plus `game_layout` for layout v8 (zero-core's
    `layout_stamp`), so it reads back as the same game."""
    from zero import package
    return {'game': game, **package.layout_stamp(game)}
