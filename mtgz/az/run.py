"""`python -m mtgz.az.run`: zero.run of the pinned zero-core (third_party/zero-core) for MTG Zero,
plus the evaluation deck set (`mtgz.az.evaldecks`: evaluation matches play the spec's fixed deck
pairs, self-play new random decks). Imported, this module IS zero.run (same object: settings,
state and patches are shared)."""
import sys

import mtgz.az  # noqa: F401  (registers the game package)
from mtgz.az import evaldecks
from zero import run as _core

evaldecks.patch(_core)

if __name__ == '__main__':
    _core.cli()
else:
    sys.modules[__name__] = _core
