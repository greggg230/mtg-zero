"""`python -m mtgz.az.heldout`: zero.heldout of the pinned zero-core (third_party/zero-core) for MTG Zero
(held-out losses on a frozen set of sample rows; zero-core docs/az.md 20.2, ENGINE.md 73).
Imported, this module IS zero.heldout (same object)."""
import sys

import mtgz.az  # noqa: F401  (registers the game package)
from zero import heldout as _core

if __name__ == '__main__':
    _core.main(sys.argv[1:])
else:
    sys.modules[__name__] = _core
