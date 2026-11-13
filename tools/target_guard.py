"""python tools/target_guard.py <cargo target dir> <checkout root>: refuse (exit 2) a cargo target dir another
checkout built in, mark a fresh one for this checkout - tools/gate.py's target guard (its `.gate_checkout.json`
marker), for every build: tools/build_py.sh runs it before it builds (docs/ENGINE.md 143). Two checkouts sharing one
target dir can silently reuse each other's build: cargo's dep-info paths are workspace-relative."""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from gate import GateError, target_guard  # noqa: E402


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    if len(argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    try:
        target_guard(argv[0], argv[1])
    except GateError as e:
        print(f'target_guard: {e}', file=sys.stderr)
        return 2
    return 0


if __name__ == '__main__':
    sys.exit(main())
