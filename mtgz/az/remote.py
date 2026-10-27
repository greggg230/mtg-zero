"""`python -m mtgz.az.remote eval MIRROR ITER`: zero.remote of the pinned zero-core (third_party/zero-core)
for MTG Zero - a run's checkpoint evaluation on a remote machine (run setting eval_remote = 'mac': the
MacBook; mtgz/az/mac.py, docs/ENGINE.md 44). As a command it first checks that this checkout is exactly
the run's code (`mtgz.az.mac.verify_checkout`: HEAD = $MTGZ_REMOTE_COMMIT, no tracked file or submodule
changed, the engine built at that commit by tools/mac.sh --eval-checkout) and refuses otherwise.
`python -m mtgz.az.remote check [SHA]`: that check alone. Imported, this module IS zero.remote (same
object)."""
import sys

import mtgz.az  # noqa: F401  (registers the game package)
from zero import remote as _core

if __name__ == '__main__':
    from mtgz.az.mac import ROOT, checkout_state, verify_checkout
    if sys.argv[1:2] == ['check'] and len(sys.argv) <= 3:
        verify_checkout(ROOT, sys.argv[2] if len(sys.argv) == 3 else None)
        print(f'mtgz remote: {ROOT} is {checkout_state(ROOT)[0][:12]}, unchanged, its engine built there')
    else:
        verify_checkout(ROOT)
        _core.main()
else:
    sys.modules[__name__] = _core
