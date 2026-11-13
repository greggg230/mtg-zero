"""Is a checkout exactly its commit? Its tracked files (and the zero-core submodule at the pin) must match HEAD, and no
untracked file may sit where a build, a test or a run would pick it up (docs/ENGINE.md 143: an untracked
`tests/test_*.py`, `engine/tests/*.rs` or module under `mtgz/` is collected, compiled or imported and is in no commit -
a gate could pass on it here and fail everywhere else, and a remote checkout of the commit would lack it).

Standard library only: tools/gate.py and tools/vast.py load this file by its path, without importing the package
(`import mtgz` needs the zero-core submodule, which a gate may not have initialised yet); mtgz/az/mac.py imports it.
"""
import os
import subprocess

# untracked files that count: anything under these directories, and the root's own build / test configuration
# (a root conftest.py, pytest.ini, Cargo.toml, ...); a stray file elsewhere (a crash dump, notes) does not
CODE_DIRS = ('engine/', 'mtgz/', 'tests/', 'tools/', 'card_ir/', 'py/', 'data/', 'third_party/', '.cargo/')
ROOT_SUFFIXES = ('.py', '.rs', '.toml', '.cfg', '.ini', '.lock')
CORE = 'third_party/zero-core'


def is_code(path):
    """Whether an untracked file at `path` (relative, '/'-separated) can change what the checkout builds or runs."""
    path = path.replace('\\', '/')
    return path.startswith(CODE_DIRS) or ('/' not in path and path.endswith(ROOT_SUFFIXES))


def _git(root, *args):
    return subprocess.run(['git', '-C', root, *args], capture_output=True, text=True, check=True).stdout


def tracked_changes(root):
    """`git status --porcelain --untracked-files=no`: the tracked files (and the submodule) that differ from HEAD."""
    return [line for line in _git(root, 'status', '--porcelain', '--untracked-files=no').splitlines() if line.strip()]


def untracked_code(root):
    """The untracked, not ignored files of the checkout `root` that `is_code`, and those of its zero-core submodule
    (when initialised), as paths relative to `root`."""
    out = [p for p in _git(root, 'ls-files', '--others', '--exclude-standard', '-z').split('\0') if p and is_code(p)]
    core = os.path.join(root, *CORE.split('/'))
    if os.path.exists(os.path.join(core, '.git')):
        out += [f'{CORE}/{p}' for p in _git(core, 'ls-files', '--others', '--exclude-standard', '-z').split('\0') if p]
    return sorted(out)


def changes(root):
    """Everything that keeps the checkout `root` from being exactly its commit, as `git status --porcelain` lines:
    the tracked changes, then '?? <path>' for each untracked file that counts. [] for a clean checkout."""
    return tracked_changes(root) + [f'?? {p}' for p in untracked_code(root)]
