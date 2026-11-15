"""The warm worktree pool (docs/ENGINE.md 165): a few fixed worktrees, each with its own fixed cargo target dir, that a
gate, a lane check or a run build checks a commit out into - instead of a new worktree (and a cold 30-minute build) per
job. The same path and the same target dir every time, so cargo rebuilds only what the commit changed, and the
one-target-dir-per-checkout rule (AGENTS.md Pitfalls: cargo's dep-info paths are workspace-relative) holds: a warm
worktree's target dir is never another checkout's.

    python tools/warm_pool.py run --sha <rev> [--for <label>] -- python tools/gate.py python
    python tools/warm_pool.py acquire --sha <rev> [--for <label>]     # -> JSON {"path", "target_dir", "slot", "sha"}
    python tools/warm_pool.py release <slot or path>
    python tools/warm_pool.py status
    python tools/warm_pool.py init [--slots 4]                          # create the worktrees (at main), nothing else

`run` acquires a free worktree (waiting up to --wait seconds), checks <rev> out there (detached, forced: tracked files
reset, untracked files removed - ignored build products such as mtgz/_engine.pyd and gates/ stay; the zero-core
submodule at the pin, clean), runs the command in it with CARGO_TARGET_DIR set to its own target dir, and releases it
however the command ends. A warm worktree holds nobody's work: never edit in one. A lease is a file
<pool>/.leases/<slot>.json created exclusively (pid, the process's creation time, label, sha); a lease whose process is
gone (or whose pid now names another process: Windows reuses pids) is taken over.

Pool: worktrees <pool>/warm-1 .. warm-N, <pool> = $MTGZ_WARM_ROOT, else the main checkout's sibling mtg-zero-wt/;
target dirs $MTGZ_WARM_TARGETS/warm-K, else E:/cargo-target/mtg-zero/warm-K on Windows, <worktree>/target elsewhere.
N = --slots, else $MTGZ_WARM_SLOTS, else 4.
"""
import argparse
import json
import os
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
WIN = sys.platform == 'win32'


def git(args, cwd, check=True):
    p = subprocess.run(['git', *args], cwd=cwd, capture_output=True, text=True)
    if check and p.returncode:
        raise RuntimeError(f'git {" ".join(args)} ({cwd}): {p.stderr.strip()[:500]}')
    return p.stdout.strip()


def main_checkout():
    common = git(['rev-parse', '--path-format=absolute', '--git-common-dir'], ROOT)
    return os.path.dirname(os.path.normpath(common))


def pool_dir():
    return os.environ.get('MTGZ_WARM_ROOT') or os.path.join(os.path.dirname(main_checkout()), 'mtg-zero-wt')


def n_slots(arg=None):
    return int(arg or os.environ.get('MTGZ_WARM_SLOTS') or 4)


def slot_path(k):
    return os.path.join(pool_dir(), f'warm-{k}')


def target_of(k):
    base = os.environ.get('MTGZ_WARM_TARGETS')
    if base:
        return os.path.join(base, f'warm-{k}')
    return f'E:/cargo-target/mtg-zero/warm-{k}' if WIN else os.path.join(slot_path(k), 'target')


def lease_path(k):
    return os.path.join(pool_dir(), '.leases', f'warm-{k}.json')


def proc_started(pid):
    """The process's creation time (psutil), or None when no such process."""
    try:
        import psutil
        return psutil.Process(pid).create_time()
    except Exception:      # noqa: BLE001  (gone, no access, no psutil: treated as gone only when it is gone)
        try:
            import psutil  # noqa: F401
        except ImportError:
            return 0.0      # (no psutil: a live pid can't be told from a reused one; never take its lease)
        return None


def me():
    return {'pid': os.getpid(), 'started': proc_started(os.getpid())}


def lease_live(info):
    started = proc_started(int(info.get('pid', -1)))
    if started is None:
        return False
    want = info.get('started')
    return not want or not started or abs(started - want) < 2.0


def try_lease(k, label, sha, holder=None):
    """Take slot k's lease (exclusive create; a stale one is removed first) -> True when held."""
    path = lease_path(k)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    for _ in range(2):
        try:
            fd = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
        except FileExistsError:
            try:
                with open(path, encoding='utf-8') as f:
                    info = json.load(f)
            except (OSError, ValueError):
                info = {}
            if info and lease_live(info):
                return False
            try:
                os.remove(path)                       # its holder is gone: take it over
            except OSError:
                return False
            continue
        with os.fdopen(fd, 'w', encoding='utf-8') as f:
            json.dump({**(holder or me()), 'label': label, 'sha': sha, 'since': time.time()}, f)
        return True
    return False


def release(k, path=None):
    """Remove slot k's lease (`path`: its file, as acquire named it - no git call, so a release in a dying process
    still works)."""
    try:
        os.remove(path or lease_path(k))
    except FileNotFoundError:
        pass


def checkout(k, sha):
    """Slot k's worktree at `sha` (created if missing), exactly the commit: forced checkout, untracked files removed,
    the submodule at the pin and clean -> (path, full sha)."""
    path = slot_path(k)
    main = main_checkout()
    full = git(['rev-parse', '--verify', f'{sha}^{{commit}}'], main)
    if not os.path.exists(os.path.join(path, '.git')):
        git(['worktree', 'add', '--detach', path, full], main)
    else:
        git(['checkout', '--detach', '--force', full], path)
        git(['clean', '-fdq'], path)
    git(['-c', 'protocol.file.allow=always', 'submodule', 'update', '--init', '--force', 'third_party/zero-core'], path)
    git(['clean', '-fdq'], os.path.join(path, 'third_party', 'zero-core'))
    head = git(['rev-parse', 'HEAD'], path)
    dirty = git(['status', '--porcelain', '--untracked-files=no'], path)
    if head != full or dirty:
        raise RuntimeError(f'{path}: not exactly {full[:12]} after the checkout (HEAD {head[:12]}; {dirty[:200]})')
    return path, full


def acquire(sha, label, wait, slots, holder=None, only=None):
    """A free slot (`only`: that slot alone) checked out at `sha`, its lease held -> its description."""
    t0 = time.time()
    while True:
        for k in ([only] if only else range(1, slots + 1)):
            if try_lease(k, label, sha, holder):
                try:
                    path, full = checkout(k, sha)
                except Exception:
                    release(k)
                    raise
                return {'slot': f'warm-{k}', 'k': k, 'path': path, 'target_dir': target_of(k), 'sha': full,
                        'lease': lease_path(k)}
        if time.time() - t0 > wait:
            raise TimeoutError(f'no free warm worktree in {wait:.0f}s ({slots} slots; `python tools/warm_pool.py status`)')
        time.sleep(5)


def slot_k(name):
    name = os.path.basename(os.path.normpath(str(name)))
    if name.isdigit():
        return int(name)
    if not name.startswith('warm-') or not name[5:].isdigit():
        raise SystemExit(f'not a warm slot: {name}')
    return int(name[5:])


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0], formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--slots', type=int, default=None)
    sub = ap.add_subparsers(dest='cmd', required=True)
    for name in ('run', 'acquire'):
        s = sub.add_parser(name)
        s.add_argument('--sha', required=True)
        s.add_argument('--for', dest='label', default='')
        s.add_argument('--wait', type=float, default=3600.0)
        s.add_argument('--slot', default=None, help='this slot only (warm-K or K), e.g. for a cold-vs-warm measurement')
        if name == 'run':
            s.add_argument('command', nargs=argparse.REMAINDER)
    s = sub.add_parser('release')
    s.add_argument('slot')
    sub.add_parser('status')
    sub.add_parser('init')
    a = ap.parse_args(argv)
    slots = n_slots(a.slots)
    if a.cmd == 'status':
        for k in range(1, slots + 1):
            p = slot_path(k)
            head = git(['rev-parse', '--short=12', 'HEAD'], p, check=False) if os.path.exists(p) else '(not created)'
            try:
                with open(lease_path(k), encoding='utf-8') as f:
                    info = json.load(f)
                held = f'held by pid {info.get("pid")} ({info.get("label") or "-"}, since ' \
                       f'{time.strftime("%H:%M", time.localtime(info.get("since", 0)))})' + ('' if lease_live(info) else ' [stale]')
            except (OSError, ValueError):
                held = 'free'
            print(f'warm-{k}: {p} @ {head}, target {target_of(k)}: {held}')
        return 0
    if a.cmd == 'init':
        main = main_checkout()
        for k in range(1, slots + 1):
            if not os.path.exists(os.path.join(slot_path(k), '.git')):
                if try_lease(k, 'init', 'main'):
                    try:
                        checkout(k, git(['rev-parse', 'main'], main))
                    finally:
                        release(k)
            print(f'warm-{k}: {slot_path(k)}')
        return 0
    if a.cmd == 'release':
        release(slot_k(a.slot))
        return 0
    if a.cmd == 'acquire':
        # (the lease names the caller's parent - the shell that will use the worktree and release it - not this
        # short-lived process)
        holder = {'pid': os.getppid(), 'started': proc_started(os.getppid())}
        print(json.dumps(acquire(a.sha, a.label, a.wait, slots, holder, a.slot and slot_k(a.slot))))
        return 0
    cmd = a.command[1:] if a.command[:1] == ['--'] else a.command
    if not cmd:
        raise SystemExit('run: no command (after --)')
    lease = acquire(a.sha, a.label or ' '.join(cmd)[:80], a.wait, slots, only=a.slot and slot_k(a.slot))
    print(f'[warm_pool] {lease["slot"]} at {lease["sha"][:12]}: {lease["path"]} (target {lease["target_dir"]})', flush=True)
    try:
        env = dict(os.environ, CARGO_TARGET_DIR=lease['target_dir'])
        return subprocess.run(cmd, cwd=lease['path'], env=env).returncode
    finally:
        release(lease['k'], lease['lease'])


if __name__ == '__main__':
    sys.exit(main())
