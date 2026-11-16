"""`python -m mtgz.az.game_failure`: zero.game_failure of the pinned zero-core (third_party/zero-core) for MTG Zero -
the run's dropped self-play games (`<run>/game_failures.jsonl`, docs/ENGINE.md 172) and their replay:

    python -m mtgz.az.game_failure replay <run>/game_failures.jsonl --line N    # exit 101: the panic reproduced
    python -m mtgz.az.game_failure list <run>/game_failures.jsonl

Imported, this module IS zero.game_failure."""
import sys

import mtgz.az  # noqa: F401  (registers the game package)
from zero import game_failure as _core

if __name__ == '__main__':
    raise SystemExit(_core.main())
else:
    sys.modules[__name__] = _core
