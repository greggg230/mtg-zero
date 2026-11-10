"""`python -m mtgz.az.run`: zero.run of the pinned zero-core (third_party/zero-core) for MTG Zero
(evaluation matches play the fixed deck set through the run setting `eval_spec`, the game
package's default: `mtgz.az.evaldecks`). Imported, this module IS zero.run (same object:
settings and state are shared).

Run as a command it first turns on the EoE sealed game log (docs/ENGINE.md 107.1): every finished
`mtg_eoe` game this process plays - self-play and evaluation - appends its decks, their builder's
mode and the result to `<run>/games/games-<pid>.jsonl` (`mtgz.sealed.games` reads them). Only sealed
games write, so other runs make no file; `MTGZ_GAME_LOG=0` turns it off."""
import sys

import mtgz.az  # noqa: F401  (registers the game package)
from zero import run as _core

if __name__ == '__main__':
    from mtgz.sealed.games import log_path
    _path = log_path(sys.argv[1:])
    if _path:
        from mtgz import _engine
        _engine.game_log(_path)
    _core.cli()
else:
    sys.modules[__name__] = _core
