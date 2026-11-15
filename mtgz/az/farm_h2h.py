"""`python -m mtgz.az.farm_h2h`: zero.farm_h2h of the pinned zero-core (third_party/zero-core) for MTG Zero - a
head-to-head of two checkpoints played on zero-farm (docs/ENGINE.md 114.4). Imported, this module IS zero.farm_h2h."""
import sys

import mtgz.az  # noqa: F401  (registers the game package)
from zero import farm_h2h as _core

if __name__ == '__main__':
    raise SystemExit(_core.main())
else:
    sys.modules[__name__] = _core
