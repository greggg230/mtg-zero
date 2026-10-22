"""`python -m mtgz.az.targets`: zero.targets of the pinned zero-core (third_party/zero-core) for MTG Zero.
Imported, this module IS zero.targets (same object: settings, state and patches are shared)."""
import sys

import mtgz.az  # noqa: F401  (registers the game package)
from zero import targets as _core

if __name__ == '__main__':
    _core.main()
else:
    sys.modules[__name__] = _core
