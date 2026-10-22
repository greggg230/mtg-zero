"""`python -m mtgz.az.run`: zero.run of the pinned zero-core (third_party/zero-core) for MTG Zero
(evaluation matches play the fixed deck set through the run setting `eval_spec`, the game
package's default: `mtgz.az.evaldecks`). Imported, this module IS zero.run (same object:
settings and state are shared)."""
import sys

import mtgz.az  # noqa: F401  (registers the game package)
from zero import run as _core

if __name__ == '__main__':
    _core.cli()
else:
    sys.modules[__name__] = _core
