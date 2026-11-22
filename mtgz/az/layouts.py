"""Which MTG layout a run, a checkpoint, a record or a farm job is (docs/ENGINE.md 235).

Layout v8 (`layout_v8.json`, the game `mtg_v8`) is the default from now on: every preset, every new run, the game
package's default game, every tool that starts fresh. Layout v7 (`layout_v7.json`) keeps the game name every run,
checkpoint, farm run, campaign and record made before layout v8 records - `mtg` - as their compatibility layout.
zero-core chooses a token layout and an engine game by the game's NAME, and its encoding guard compares a checkpoint's
recorded game with the game it is loaded as, so a name is a layout and nothing changes layout under a run:

* **A run** plays its saved config's game: a run made before layout v8 says `mtg` and resumes, re-pinned on a new main or
  not, as layout v7, byte for byte (its rows, its checkpoints, its shards, its farm jobs). A new run says `mtg_v8`.
* **A checkpoint** records its game; `load` loads it as that game (zero.net.load's default) and `model_game` names it, so a
  tool plays a network's games in that network's layout. Loading it as the other game is refused by zero-core's
  encoding guard (exit 78: "game 'mtg' -> 'mtg_v8'", the token layouts differ), never encoded in silence.
* **The farm**: a job carries its run's settings' game (zero-core's farm pool), so a v7 run's leases are `mtg` and a v8
  run's `mtg_v8` on a worker built from now on (the release worker registers both); a worker refuses a graph of another
  layout than its job's game ("another layout").
* **A run moves** to layout v8 only by `tools/fork_run.py <its checkpoint> <new run> --game mtg_v8` (zero-core's widen:
  v8's six segments gated at exactly 0).
"""
GAME = 'mtg_v8'      # layout v8: the default, every new run and every tool that starts fresh
GAME_V7 = 'mtg'      # layout v7: the compatibility layout of the runs, checkpoints and records made before layout v8
GAMES = (GAME, GAME_V7)


def checkpoint_game(path):
    """The game checkpoint `path` records (its network config's)."""
    import torch
    blob = torch.load(path, map_location='cpu', weights_only=False)
    cfg = blob.get('config') if isinstance(blob, dict) else None
    if not isinstance(cfg, dict) or not cfg.get('game'):
        raise ValueError(f'{path}: a checkpoint that records no game (docs/ENGINE.md 235: a layout is never guessed)')
    return cfg['game']


def model_game(model):
    """The game a loaded network records (its config's): the layout its games are played and encoded in."""
    g = (getattr(model, 'config', None) or {}).get('game')
    if not g:
        raise ValueError('a network that records no game (docs/ENGINE.md 235: a layout is never guessed)')
    return g


def load(path, device='cpu', **kw):
    """zero.net.load of checkpoint `path` as the game it records (layout v7 `mtg` or v8 `mtg_v8`): (model, extra)."""
    from zero.net import load as _load
    return _load(path, device, **kw)


def same_game(*games, where='the networks'):
    """The one game of several networks (a match, a pair): refused when they record different layouts."""
    gs = sorted(set(games))
    if len(gs) != 1:
        raise ValueError(f'{where} record different games {gs}: layouts v7 and v8 cannot play one match (docs/ENGINE.md '
                         f'232; fork the v7 one onto mtg_v8 with tools/fork_run.py)')
    return gs[0]


_USED = None


def use(game):
    """This process plays and encodes MTG games of `game` from now on (a tool, after loading a checkpoint: its layout);
    a second MTG layout in one process is refused (a v7 and a v8 network cannot share games). Returns `game`."""
    global _USED
    if game in GAMES:
        if _USED not in (None, game):
            raise ValueError(f'this process plays game {_USED!r} (a checkpoint loaded before), and now {game!r}: layouts v7 '
                             f'and v8 cannot share its games (docs/ENGINE.md 235)')
        _USED = game
    return game


def current():
    """The MTG game this process plays: the one its checkpoints record (`use`, `load_used`), else layout v8's (a tool
    that starts fresh)."""
    return _USED or GAME


def load_used(path, device='cpu', **kw):
    """`load` of checkpoint `path`, and `use` of the game it records: (model, extra)."""
    m, extra = load(path, device, **kw)
    use(model_game(m))
    return m, extra


def record_game(rec):
    """The game a game record or a stored spot names (its `game` key; one written before layout v8 names `mtg` or none:
    layout v7's)."""
    return (rec or {}).get('game') or GAME_V7
