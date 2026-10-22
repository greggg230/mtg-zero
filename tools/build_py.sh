#!/usr/bin/env bash
# Build the PyO3 module (crate py/ -> Python module mtgz._engine: zero-core's driver + the toy and
# MTG) for THIS checkout and install it as mtgz/_engine.pyd (gitignored), so
# `import mtgz._engine` from this checkout uses this checkout's build.
#
#   bash tools/build_py.sh            # release build (default)
#   PYTHON=py bash tools/build_py.sh  # other interpreter
#
# zero-core is pinned (git submodule third_party/zero-core, zero-core docs/pinning.md): a fresh
# checkout or worktree gets it initialised here at the pinned commit; a checked-out core that
# differs from the pin is an error (ZERO_CORE_UNPINNED=1 builds it anyway: core development).
# Target dir: $CARGO_TARGET_DIR, else on Windows E:/cargo-target/mtg-zero/<branch> (C: is
# full), elsewhere <repo>/target.
# Windows locks a loaded .pyd: if the copy fails, the old file is renamed out of the way
# (running processes keep using it; new ones get the new build).
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CORE="$ROOT/third_party/zero-core"
PIN="$(git -C "$ROOT" rev-parse ":third_party/zero-core")"        # the pinned commit (index = HEAD unless staged)
if [ ! -e "$CORE/.git" ]; then
  echo "zero-core: initialising third_party/zero-core at the pinned ${PIN:0:12}"
  git -C "$ROOT" submodule update --init third_party/zero-core
fi
HAVE="$(git -C "$CORE" rev-parse HEAD)"
if [ "$HAVE" != "$PIN" ]; then
  if [ "${ZERO_CORE_UNPINNED:-0}" = 1 ]; then
    echo "WARNING: zero-core is at ${HAVE:0:12}, the pin is ${PIN:0:12} (ZERO_CORE_UNPINNED=1)"
  else
    echo "ERROR: third_party/zero-core is at ${HAVE:0:12} but this checkout pins ${PIN:0:12}." >&2
    echo "  git submodule update third_party/zero-core   # go to the pin" >&2
    echo "  (developing the core: commit + stage the new pin, or ZERO_CORE_UNPINNED=1)" >&2
    exit 1
  fi
fi
case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*|Windows_NT) TDEF=E:/cargo-target/mtg-zero ;;   # the desktop: C: is full
  *) TDEF="" ;;                                                                     # elsewhere: <repo>/target
esac
exec bash "$CORE/tools/build_engine.sh" "$ROOT" mtgz mtgz_py $TDEF
