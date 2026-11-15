"""pytest setup: import mtgz from this checkout (mtgz/_engine.pyd built by tools/build_py.sh)."""
import os
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if ROOT not in sys.path:
    sys.path.insert(0, ROOT)

import torch  # noqa: E402

torch.set_num_threads(1)          # docs/az.md 7: > 1 thread collapses on a saturated CPU


def pytest_configure(config):
    config.addinivalue_line('markers', 'live_run_hazard: starts supervisors or churns processes - skipped while a run '
                                       'is live on this machine (zero.runctl.live_run_fence_reason; docs/ENGINE.md 166)')


def pytest_collection_modifyitems(config, items):
    import mtgz  # noqa: F401 - puts the pinned zero-core on sys.path
    from zero import runctl
    runctl.fence_items(config, items)
