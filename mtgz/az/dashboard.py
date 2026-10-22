"""`python -m mtgz.az.dashboard`: zero.dashboard of the pinned zero-core (third_party/zero-core) for MTG Zero.
Imported, this module IS zero.dashboard (same object: settings, state and patches are shared)."""
import sys

import mtgz.az  # noqa: F401  (registers the game package)
from zero import dashboard as _core

if __name__ == '__main__':
    _core.main()
else:
    sys.modules[__name__] = _core
