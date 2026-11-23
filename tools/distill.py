"""python tools/distill.py ...: zero-core's zero.tools.distill (docstring below) for MTG Zero
(this checkout's pinned core, third_party/zero-core): a bigger network warm-started from a run's
checkpoint on its shards (docs/ENGINE.md 62); `--sources FILE OUT.pt` distils several teachers, each on its rows
(zero-core docs/az.md 76), with MTG Zero's row selector (`mtgz.az.rowselect`: {"actor"}, {"pairing"}, {"not_pairing"}
of a metagame run's decks; docs/ENGINE.md 239.1)."""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import mtgz.az  # noqa: E402,F401  (the pinned core on sys.path + MTG Zero's game package)
from zero.tools.distill import *  # noqa: E402,F401,F403
from zero.tools.distill import __doc__, cli, main  # noqa: E402,F401

if __name__ == '__main__':
    cli()
