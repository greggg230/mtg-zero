"""MTG Zero.

Importing `mtgz` puts this checkout's pinned zero-core (the git submodule `third_party/zero-core`,
docs/pinning.md there) on sys.path: `mtgz.az.*` are zero-core's modules for MTG. Never a
global install - each checkout (main, a pinned run worktree) runs its own pinned core.
"""
import os as _os
import sys as _sys

_ROOT = _os.path.dirname(_os.path.dirname(_os.path.abspath(__file__)))
_CORE = _os.path.join(_ROOT, 'third_party', 'zero-core', 'python')
if not _os.path.isfile(_os.path.join(_CORE, 'zero', '__init__.py')):
    raise ImportError(f'zero-core is missing at {_CORE}: run `git submodule update --init third_party/zero-core` '
                      f'(or `bash tools/build_py.sh`, which does it) in {_ROOT}')
_have = _sys.modules.get('zero')
if _have is not None:
    _want = _os.path.normcase(_os.path.realpath(_os.path.join(_CORE, 'zero')))
    _got = _os.path.normcase(_os.path.realpath(_os.path.dirname(getattr(_have, '__file__', '') or '.')))
    if _got != _want:
        raise ImportError(f'another zero-core is already imported from {_got}; MTG Zero needs its pinned '
                          f'core {_want}')
if _CORE not in _sys.path:
    _sys.path.insert(0, _CORE)
