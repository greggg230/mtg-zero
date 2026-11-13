"""Run a command with a timeout that bounds it, its children and its grandchildren (docs/ENGINE.md 143).

`subprocess.run(..., timeout=T)` kills only the direct child at T, then reads the output pipe to its end - and on
Windows a grandchild (cargo's rustc, pytest's worker, a Python process a bash script started) still holds that pipe,
so the call returns only when the grandchild exits: a gate step asked to stop after 2 s returned after 12, and a hung
build held the gate (and the CPU) until someone killed the tree by hand. Here:

* the output is read by threads, never by the waiting thread;
* every descendant seen while the command runs (polled each second) is remembered with its creation time;
* at the timeout the whole tree is killed (the descendants first, then the command), and a descendant that still
  holds the pipe after the command exited is killed when the deadline passes;
* descendants still alive when the command ends are killed too (a finished step leaves no orphan pool behind).

Used by tools/gate.py (every step) and tools/search_smoke.py (each configuration's process). Needs psutil (a
dependency of the repo: AGENTS.md).
"""
import subprocess
import threading
import time


def _reader(pipe, sink):
    try:
        while True:
            b = pipe.read1(1 << 16)
            if not b:
                break
            sink.append(b)
    except (OSError, ValueError):
        pass


def _note_descendants(psutil, pid, seen):
    try:
        for c in psutil.Process(pid).children(recursive=True):
            try:
                seen.setdefault(c.pid, c.create_time())
            except psutil.Error:
                pass
    except psutil.Error:
        pass


def _kill(psutil, seen, root=None):
    """Kill every remembered descendant still alive with its creation time (never a reused pid), then `root`."""
    if root is not None:
        _note_descendants(psutil, root.pid, seen)
    killed = 0
    for pid, created in list(seen.items()):
        try:
            p = psutil.Process(pid)
            if abs(p.create_time() - created) < 1e-3:
                p.kill()
                killed += 1
        except psutil.Error:
            pass
    if root is not None and root.poll() is None:
        try:
            root.kill()
        except OSError:
            pass
    return killed


def _text(chunks):
    # (universal newlines, as text mode reads them)
    return b''.join(chunks).decode('utf-8', 'replace').replace('\r\n', '\n').replace('\r', '\n')


def run(argv, cwd=None, env=None, timeout=None, merge=True, creationflags=0, poll_s=1.0):
    """Run `argv` (stdin closed) -> (stdout, stderr, exit code or 'timeout'); with `merge` stderr goes to stdout
    and the second element is ''. `timeout` (seconds, None: none) bounds the command and every descendant."""
    import psutil
    p = subprocess.Popen(argv, cwd=cwd, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                         stderr=subprocess.STDOUT if merge else subprocess.PIPE, creationflags=creationflags)
    outs, errs = [], []
    readers = [threading.Thread(target=_reader, args=(p.stdout, outs), daemon=True)]
    if not merge:
        readers.append(threading.Thread(target=_reader, args=(p.stderr, errs), daemon=True))
    for t in readers:
        t.start()
    seen = {}
    deadline = None if timeout is None else time.time() + timeout
    rc = None
    while rc is None:
        try:
            rc = p.wait(timeout=poll_s if deadline is None else max(0.0, min(poll_s, deadline - time.time())))
        except subprocess.TimeoutExpired:
            _note_descendants(psutil, p.pid, seen)
            if deadline is not None and time.time() >= deadline:
                _kill(psutil, seen, p)
                p.wait()
                rc = 'timeout'
    # the command is gone; a descendant may still hold its output: wait for the readers until the deadline
    for t in readers:
        while t.is_alive():
            t.join(poll_s)
            if t.is_alive() and deadline is not None and time.time() >= deadline:
                _kill(psutil, seen)
                t.join(10)
                rc = 'timeout'
                break
    _kill(psutil, seen)                    # what is left of the tree (a step leaves no orphans behind)
    for t, pipe in zip(readers, (p.stdout, p.stderr)):
        # (a reader still blocked holds the pipe's lock: closing it would block this thread too - left to exit)
        if not t.is_alive():
            try:
                pipe.close()
            except OSError:
                pass
    return _text(outs), _text(errs), rc
