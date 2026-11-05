"""The MacBook as a run's remote evaluator (run setting eval_remote = 'mac'; zero.remote,
docs/ENGINE.md 44) and remote self-play worker (run setting selfplay_remote = 'mac', a co-training
run's resolver_cfg selfplay_remote; zero.remoteplay, docs/ENGINE.md 52): the remote spec of MTG Zero's
game package, and the checks that the Mac evaluates and self-plays with exactly the run's code.

The run's code is the commit of its checkout (where the run process imports mtgz from: a pinned
run worktree, or main). The Mac's checkout of it is ~/mtg-zero-wt/run-<sha12>, made by `bash
tools/mac.sh --eval-checkout` in the run's checkout: that commit checked out, zero-core at its pin,
the engine built there and stamped with the commit (the file `mtgz-engine-commit` in the
checkout's git dir). `RunCommitRemote` is the spec; it resolves when the run starts its RemoteEval
(zero.remote.remote_spec reads it), not when the package is imported:

* the run's checkout must be clean (tracked files and the zero-core submodule): else SystemExit;
* `repo` is the Mac's checkout of that commit, and `env` carries the commit (MTGZ_REMOTE_COMMIT);
* the Mac's checkout is checked over ssh (`python -m mtgz.az.remote check` there): missing, at another
  commit, changed, or its engine not built at the commit -> SystemExit, with the command that
  prepares it; the Mac unreachable -> a warning only (zero.remote retries an evaluation every 5
  minutes, and the check below runs before each one).

On the Mac, `python -m mtgz.az.remote eval ...` and `... selfplay ...` check the checkout
(`verify_checkout`) before anything else and refuse with the reason (zero.remote logs it in the
run's log and retries; zero.remoteplay logs the worker's failure and starts it again later): an
evaluation or a self-play worker never runs with other code than the run's. `python -m
mtgz.az.remote check [SHA]`: the check alone (SHA default $MTGZ_REMOTE_COMMIT).

The spec's `selfplay` dict (zero.remoteplay): the self-play workers' workdir, processes, niceness,
the other games' evaluation locks they back off for, and their run-setting overrides; it resolves
with the rest (the same checkout, commit and environment).
"""
import os
import shlex
import subprocess
import sys
from collections.abc import Mapping

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ENV_COMMIT = 'MTGZ_REMOTE_COMMIT'       # the run's commit, in the remote's environment
STAMP = 'mtgz-engine-commit'            # in the checkout's git dir: the commit its engine was built at


def _git(root, *args):
    return subprocess.run(['git', '-C', root, *args], capture_output=True, text=True, check=True).stdout.strip()


def checkout_state(root):
    """-> (HEAD of the checkout `root`, its changed tracked files and submodules as status lines)."""
    return _git(root, 'rev-parse', 'HEAD'), _git(root, 'status', '--porcelain', '--untracked-files=no').splitlines()


def eval_checkout(sha, home):
    """The Mac's checkout of commit `sha` for remote evaluation (tools/mac.sh --eval-checkout)."""
    return f'{home}/mtg-zero-wt/run-{sha[:12]}'


def problems(root, expect):
    """Why the checkout `root` is not exactly commit `expect` with its engine built at it ([]: it is)."""
    if not expect:
        return [f'{ENV_COMMIT} is not set (the commit of the run\'s code)']
    try:
        head, dirty = checkout_state(root)
        stamp = os.path.join(root, _git(root, 'rev-parse', '--git-path', STAMP))
    except (OSError, subprocess.CalledProcessError) as ex:
        return [f'not a git checkout ({type(ex).__name__}: {str(getattr(ex, "stderr", "") or ex).strip()[-200:]})']
    out = []
    if head != expect:
        out.append(f'HEAD is {head[:12]}, the run\'s code is {expect[:12]}')
    if dirty:
        out.append('changed: ' + '; '.join(d.strip() for d in dirty[:5]))
    try:
        with open(stamp, encoding='utf-8') as f:
            built = f.read().strip()
    except OSError:
        built = ''
    if built != head:
        out.append(f'its engine was not built at {head[:12]} by tools/mac.sh --eval-checkout'
                   + (f' (stamp {built[:12]})' if built else ' (no stamp)'))
    return out


def verify_checkout(root=ROOT, expect=None):
    """SystemExit unless the checkout `root` is exactly the run's commit (`expect`, default
    $MTGZ_REMOTE_COMMIT) with its engine built at it."""
    expect = expect or os.environ.get(ENV_COMMIT, '')
    p = problems(root, expect)
    if p:
        raise SystemExit(f'mtgz remote: {root} is not the run\'s code: {"; ".join(p)}. In the run\'s checkout '
                         f'(at {expect[:12] or "its commit"}): bash tools/mac.sh --eval-checkout (the Mac) or '
                         f'python tools/vast.py prep <box> (a rented box)')


class RunCommitRemote(Mapping):
    """A zero.remote spec ({host, repo, python, workdir, env, overrides}) whose `repo` and `env` are
    those of the commit of the run's checkout `root`, resolved and checked on first use (module
    docstring). `home`: the remote's home directory (absolute: zero.remote quotes the paths)."""

    KEYS = ('host', 'repo', 'python', 'workdir', 'env', 'overrides', 'selfplay')

    def __init__(self, root, host, home, python, workdir, env=None, overrides=None, selfplay=None,
                 prepare='bash tools/mac.sh --eval-checkout'):
        self.root = root
        self.prepare = prepare          # how to prepare the remote's checkout (the preflight's message)
        self.fixed = {'host': host, 'python': python, 'workdir': workdir}
        self.home = home
        self.env = dict(env or {})
        self.overrides = dict(overrides or {})
        self.selfplay = dict(selfplay or {})
        self._spec = None

    def __iter__(self):
        return iter(self.KEYS)

    def __len__(self):
        return len(self.KEYS)

    def __getitem__(self, k):
        if k not in self.KEYS:
            raise KeyError(k)
        if self._spec is None:
            self._spec = self.resolve()
        return self._spec[k]

    def __repr__(self):
        return f'RunCommitRemote({self.fixed["host"]!r}, root={self.root!r}, resolved={self._spec is not None})'

    def resolve(self, preflight=True):
        """-> the spec for the run's checkout as it is now (SystemExit: see the module docstring)."""
        head, dirty = checkout_state(self.root)
        if dirty:
            raise SystemExit(f'eval_remote / selfplay_remote: the run\'s checkout {self.root} has uncommitted '
                             f'changes, so no remote checkout can hold its code: '
                             f'{"; ".join(d.strip() for d in dirty[:5])} (commit them, or run without the remote)')
        spec = dict(self.fixed, repo=eval_checkout(head, self.home), env=dict(self.env, **{ENV_COMMIT: head}),
                    overrides=dict(self.overrides), selfplay=dict(self.selfplay))
        if preflight:
            self.preflight(spec, head)
        return spec

    def preflight(self, spec, head):
        """Check the remote's checkout over ssh; SystemExit if it is not the run's code."""
        exports = ' '.join(f'{k}={shlex.quote(str(v))}' for k, v in spec['env'].items())
        sh = f'cd {shlex.quote(spec["repo"])} && {exports} {shlex.quote(spec["python"])} -m mtgz.az.remote check'
        try:
            r = subprocess.run(['ssh', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=20', spec['host'], sh],
                               capture_output=True, text=True, timeout=180)
        except (OSError, subprocess.TimeoutExpired) as ex:
            print(f'eval_remote: WARNING: cannot check the checkout on {spec["host"]} ({type(ex).__name__}); '
                  f'each evaluation checks it there', file=sys.stderr, flush=True)
            return
        if r.returncode == 255:                        # ssh itself failed: the Mac asleep or offline
            print(f'eval_remote: WARNING: {spec["host"]} unreachable ({r.stderr.strip()[-200:]}); each evaluation '
                  f'checks its checkout there', file=sys.stderr, flush=True)
            return
        if r.returncode:
            raise SystemExit(f'eval_remote: {spec["host"]}:{spec["repo"]} is not the run\'s code ({head[:12]}): '
                             f'{(r.stderr or r.stdout).strip()[-600:]}\n  prepare it: in {self.root}: '
                             f'{self.prepare}')

