"""python tools/fork_run.py ...: zero-core's zero.tools.fork_run (docstring below) for MTG Zero
(this checkout's pinned core, third_party/zero-core)."""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import mtgz.az  # noqa: E402,F401  (the pinned core on sys.path + MTG Zero's game package)
from zero.tools.fork_run import *  # noqa: E402,F401,F403
from zero.tools.fork_run import __doc__, cli, main  # noqa: E402,F401

if __name__ == '__main__':
    cli()
