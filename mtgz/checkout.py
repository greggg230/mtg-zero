"""Is a checkout exactly its commit? Its tracked files (and the zero-core submodule at the pin) must match HEAD, and no
untracked file may sit where a build, a test or a run would pick it up (docs/ENGINE.md 143: an untracked
`tests/test_*.py`, `engine/tests/*.rs` or module under `mtgz/` is collected, compiled or imported and is in no commit -
a gate could pass on it here and fail everywhere else, and a remote checkout of the commit would lack it).

Standard library only: tools/gate.py and tools/vast.py load this file by its path, without importing the package
(`import mtgz` needs the zero-core submodule, which a gate may not have initialised yet); mtgz/az/mac.py imports it.
"""
import os
import re
import subprocess

# a git child of a console-less process (a run the hub started) gets no window of its own (memory: terminal popups)
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0

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
    return subprocess.run(['git', '-C', root, *args], capture_output=True, text=True, check=True,
                          creationflags=NO_WINDOW).stdout


def read_head(root):
    """The commit HEAD of the checkout `root` points at, read from git's files (no git process) - a worktree's
    `.git` file and `commondir`, a detached HEAD, a loose or a packed ref -, or None when they cannot tell (not a
    checkout, another ref storage): ask git then."""
    try:
        g = os.path.join(root, '.git')
        if os.path.isfile(g):
            with open(g, encoding='utf-8') as f:
                line = f.read().strip()
            if not line.startswith('gitdir:'):
                return None
            g = os.path.normpath(os.path.join(root, line[len('gitdir:'):].strip()))
        with open(os.path.join(g, 'HEAD'), encoding='utf-8') as f:
            head = f.read().strip()
        if re.fullmatch(r'[0-9a-f]{40}', head):
            return head
        if not head.startswith('ref: '):
            return None
        ref = head[5:].strip()
        bases = [g]
        if os.path.exists(os.path.join(g, 'commondir')):
            with open(os.path.join(g, 'commondir'), encoding='utf-8') as f:
                bases.append(os.path.normpath(os.path.join(g, f.read().strip())))
        for b in bases:
            p = os.path.join(b, *ref.split('/'))
            if os.path.isfile(p):
                with open(p, encoding='utf-8') as f:
                    sha = f.read().strip()
                return sha if re.fullmatch(r'[0-9a-f]{40}', sha) else None
        for b in bases:
            p = os.path.join(b, 'packed-refs')
            if os.path.isfile(p):
                with open(p, encoding='utf-8') as f:
                    for line in f:
                        parts = line.split()
                        if len(parts) == 2 and parts[1] == ref and re.fullmatch(r'[0-9a-f]{40}', parts[0]):
                            return parts[0]
    except OSError:
        return None
    return None


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
