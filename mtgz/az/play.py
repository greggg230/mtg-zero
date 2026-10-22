"""zero.play of the pinned zero-core (third_party/zero-core) for MTG Zero: this module replaces
itself with it on import (same object: settings, state and patches are shared)."""
import sys

import mtgz.az  # noqa: F401  (registers the game package)
from zero import play as _core

sys.modules[__name__] = _core
