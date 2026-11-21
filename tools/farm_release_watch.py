"""A ready farm build for every main merge (docs/ENGINE.md 196.4): polls main and runs tools/farm_release.py for its
newest head, one release at a time, below normal priority (farm_release.py's heavy steps hold slots of tools/slotd.sh).

    python tools/launch_hidden.py <builds>/release_watch.console.txt <python> tools/farm_release_watch.py   # start
    python tools/farm_release_watch.py --status        # running? its pid, the last head, the last result
    python tools/farm_release_watch.py --stop          # writes <builds>/release_watch.STOP; it exits within a poll
    python tools/farm_release_watch.py --once          # one poll in the foreground (what it would do, then do it)

Every `--every` seconds (300): `git fetch origin main` (`--no-fetch`: local refs only), then the head = the newer of
the local `main` and `origin/main` (the one that contains the other; diverged: origin/main). A head it has not handled
yet goes to `farm_release.py release <head>`, which reuses the build of a commit with the same Rust inputs (no build:
an index entry in seconds) and otherwise builds, signs, re-signs for CUDA, self-tests, gates and publishes - and, as
release.json says (docs/ENGINE.md 216; default on), the macOS part: the macOS build made on the Mac, its farm gates,
published, recorded in the entry (a reused entry without one gets it too). Heads that main passed while a release ran
are skipped: the next poll takes the newest. A failed release is not retried for the same head (the next merge tries
again; `farm_release.py release <head>` by hand). Publishing makes a build available only: nothing is attached to any
run.

A release whose macOS part failed (farm_release.py exit 3: the Mac asleep or offline, its build, gate or publish
failing) is published without it: logged as `released <head> WITHOUT ITS MACOS BUILD`, the state's `mac_pending`
{head, attempts, last_try, line}, and retried - `farm_release.py release <head>` again, which builds only the missing
macOS part - every `--mac-retry-every` seconds (3600), at most `--mac-retries` times (6; then `GIVING UP`, in the log
and `mac_pending.gave_up`), while that head is main's newest. A newer head's release takes over (its entry gets its own
macOS part, or this one's when it reuses the same build).

Files in `<builds>` (mtgz.az.farm_builds.builds_dir(): $ZERO_BIG_DISK/zero-farm-builds): release_watch.log (one line
per event; each release's own output in releases/<sha12>/), release_watch.json (state: pid, last head, last result),
index.json (the builds, farm_release.py's). One watcher per machine (release_watch.lock). Not a web server: no port.
The release uses this checkout's tools/farm_release.py and farm_build.py (start the watcher from a checkout of main,
or a pinned worktree, never from one being edited).
"""
import argparse
import datetime
import json
import os
import subprocess
import sys
import time

TOOLS = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(TOOLS)
sys.path.insert(0, ROOT)
sys.path.insert(0, os.path.join(ROOT, 'third_party', 'zero-core', 'python'))
MAC_FAILED = 3          # farm_release.py's EXIT_MAC_FAILED: published without its macOS part
MAC_RETRY_EVERY = 3600.0
MAC_RETRIES = 6


def FB():
    from mtgz.az import farm_builds
    return farm_builds


def paths():
    d = FB().builds_dir()
    if not d:
        raise SystemExit('farm_release_watch: no builds directory ($MTGZ_FARM_BUILDS, or ZERO_BIG_DISK)')
    os.makedirs(d, exist_ok=True)
    return {k: os.path.join(d, f) for k, f in (('dir', ''), ('log', 'release_watch.log'), ('state', 'release_watch.json'),
                                                ('lock', 'release_watch.lock'), ('stop', 'release_watch.STOP'))}


def git(*a, check=True):
    p = subprocess.run(['git', *a], cwd=ROOT, capture_output=True, text=True, stdin=subprocess.DEVNULL)
    if check and p.returncode:
        raise RuntimeError(f'git {" ".join(a)}: {p.stderr.strip()[:300]}')
    return p.stdout.strip()


def log(P, msg):
    line = f'{datetime.datetime.now().isoformat(timespec="seconds")} {msg}'
    print(line, flush=True)
    with open(P['log'], 'a', encoding='utf-8') as f:
        f.write(line + '\n')


def load_state(P):
    try:
        with open(P['state'], encoding='utf-8') as f:
            return json.load(f)
    except (OSError, ValueError):
        return {}


def save_state(P, st):
    tmp = P['state'] + '.tmp'
    with open(tmp, 'w', encoding='utf-8', newline='\n') as f:
        json.dump(st, f, indent=1)
    os.replace(tmp, P['state'])


def newest(a, b):
    """The newer of two commits: the one that contains the other; diverged (or either missing): `b` if it exists."""
    if not a or a == b:
        return b
    if not b:
        return a
    anc = lambda x, y: subprocess.run(['git', 'merge-base', '--is-ancestor', x, y], cwd=ROOT,  # noqa: E731
                                      stdin=subprocess.DEVNULL).returncode == 0
    if anc(b, a):
        return a
    return b


def head(fetch=True, P=None):
    if fetch:
        p = subprocess.run(['git', 'fetch', '-q', 'origin', 'main'], cwd=ROOT, capture_output=True, text=True,
                           stdin=subprocess.DEVNULL, timeout=300)
        if p.returncode and P:
            log(P, f'git fetch origin main failed: {p.stderr.strip()[:200]} (local refs only this poll)')
    sha = lambda r: git('rev-parse', '--verify', '-q', f'{r}^{{commit}}', check=False) or None  # noqa: E731
    return newest(sha('main'), sha('origin/main'))


def release(P, sha, python):
    """farm_release.py release <sha> -> (exit status, its last lines)."""
    out = os.path.join(P['dir'], 'releases', sha[:12])
    os.makedirs(out, exist_ok=True)
    kw = {'creationflags': 0x00004000 | 0x08000000} if os.name == 'nt' else {}     # BELOW_NORMAL | CREATE_NO_WINDOW
    with open(os.path.join(out, 'watch_release.log'), 'a', encoding='utf-8') as f:
        p = subprocess.run([python, os.path.join(TOOLS, 'farm_release.py'), 'release', sha], cwd=ROOT, stdout=f,
                           stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL, **kw)
    tail = open(os.path.join(out, 'watch_release.log'), encoding='utf-8', errors='replace').read().strip().splitlines()[-3:]
    return p.returncode, tail


def mac_retry(P, st, a, h):
    """Head `h` was released without its macOS part (module docs): `farm_release.py release <h>` again - the macOS part
    only - once `--mac-retry-every` seconds have passed since the last try, at most `--mac-retries` times."""
    pend = st.get('mac_pending')
    every, most = getattr(a, 'mac_retry_every', MAC_RETRY_EVERY), getattr(a, 'mac_retries', MAC_RETRIES)
    if not pend or pend.get('head') != h or pend.get('gave_up') or time.time() - pend.get('last_try', 0) < every:
        return
    n = pend.get('attempts', 1)            # (tries so far: the release itself and the retries)
    log(P, f'{h[:12]}: retrying its macOS part (retry {n} of {most}): farm_release.py release {h[:12]}')
    t0 = time.time()
    rc, tail = release(P, h, a.python)
    pend.update(attempts=n + 1, last_try=time.time(), line=' | '.join(tail))
    if rc == 0:
        st.pop('mac_pending', None)
        if h in (st.get('done') or {}):
            st['done'][h]['mac_failed'] = False
        st['last'] = {'head': h, 'ok': True, 'line': tail[-1] if tail else ''}
        log(P, f'macOS part of {h[:12]} backfilled in {time.time() - t0:.0f} s: {tail[-1] if tail else ""}')
        return
    what = 'FAILED again' if rc == MAC_FAILED else f'FAILED (exit {rc})'
    if n >= most:
        pend['gave_up'] = True
        log(P, f'macOS part of {h[:12]} {what}: {" | ".join(tail)} - GIVING UP after {n} retries (farm_release.py release '
               f'{h[:12]} by hand; the next merge tries again)')
    else:
        log(P, f'macOS part of {h[:12]} {what}: {" | ".join(tail)} (retried in {every:.0f} s)')


def poll(P, st, a):
    h = head(not a.no_fetch, P)
    if not h:
        log(P, 'no main head found')
        return
    st['seen'] = h
    if h in (st.get('done') or {}) or h == st.get('failed'):
        mac_retry(P, st, a, h)
        return
    pend = st.pop('mac_pending', None)
    if pend and not pend.get('gave_up'):
        log(P, f'{pend["head"][:12]}\'s macOS part (failed) is left to {h[:12]}\'s release')
    log(P, f'main is at {h[:12]}: farm_release.py release {h[:12]}')
    t0 = time.time()
    rc, tail = release(P, h, a.python)
    st.setdefault('done', {})
    if rc == 0:
        st['done'][h] = {'at': time.time(), 'seconds': round(time.time() - t0)}
        st['done'] = dict(list(st['done'].items())[-50:])
        st['last'] = {'head': h, 'ok': True, 'line': tail[-1] if tail else ''}
        log(P, f'released {h[:12]} in {time.time() - t0:.0f} s: {tail[-1] if tail else ""}')
    elif rc == MAC_FAILED:
        st['done'][h] = {'at': time.time(), 'seconds': round(time.time() - t0), 'mac_failed': True}
        st['done'] = dict(list(st['done'].items())[-50:])
        st['mac_pending'] = {'head': h, 'attempts': 1, 'last_try': time.time(), 'line': ' | '.join(tail)}
        st['last'] = {'head': h, 'ok': True, 'mac': False, 'line': ' | '.join(tail)}
        log(P, f'released {h[:12]} WITHOUT ITS MACOS BUILD (exit {rc}, {time.time() - t0:.0f} s): {" | ".join(tail)} - '
               f'its macOS part is retried every {getattr(a, "mac_retry_every", MAC_RETRY_EVERY):.0f} s, at most '
               f'{getattr(a, "mac_retries", MAC_RETRIES)} times (releases/{h[:12]}/)')
    else:
        st['failed'] = h
        st['last'] = {'head': h, 'ok': False, 'line': ' | '.join(tail)}
        log(P, f'FAILED {h[:12]} (exit {rc}, {time.time() - t0:.0f} s): {" | ".join(tail)} '
               f'(releases/{h[:12]}/; not retried for this head)')


def status(P):
    st = load_state(P)
    pid = st.get('pid')
    alive = FB()._alive(pid) and os.path.exists(P['lock'])
    print(f'watcher: {"RUNNING pid " + str(pid) if alive else "not running"}; seen {str(st.get("seen"))[:12]}; '
          f'last {json.dumps(st.get("last"))}')
    if st.get('mac_pending'):
        print(f'macOS part pending: {json.dumps(st["mac_pending"])}')
    if os.path.exists(P['log']):
        print(''.join(open(P['log'], encoding='utf-8').readlines()[-5:]).rstrip())
    return 0


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('--every', type=float, default=300.0)
    ap.add_argument('--python', default=sys.executable, help='the interpreter farm_release.py runs with (torch, onnx)')
    ap.add_argument('--mac-retry-every', type=float, default=MAC_RETRY_EVERY,
                    help='seconds between retries of a released head\'s failed macOS part')
    ap.add_argument('--mac-retries', type=int, default=MAC_RETRIES, help='retries of a failed macOS part per head')
    ap.add_argument('--no-fetch', action='store_true')
    ap.add_argument('--once', action='store_true')
    ap.add_argument('--status', action='store_true')
    ap.add_argument('--stop', action='store_true')
    a = ap.parse_args(argv)
    P = paths()
    if a.status:
        return status(P)
    if a.stop:
        open(P['stop'], 'w').close()
        print(f'{P["stop"]} written: the watcher exits at its next poll')
        return 0
    if os.name == 'nt':
        import ctypes
        k = ctypes.windll.kernel32
        k.SetPriorityClass(k.GetCurrentProcess(), 0x00004000)
    try:
        lock = FB().Lock(P['lock'], wait=0)
        lock.__enter__()
    except TimeoutError as e:
        raise SystemExit(f'farm_release_watch: another watcher runs ({e})')
    try:
        if os.path.exists(P['stop']):
            os.remove(P['stop'])
        st = load_state(P)
        st.update(pid=os.getpid(), started=time.time(), root=ROOT)
        save_state(P, st)
        log(P, f'watcher started (pid {os.getpid()}, every {a.every:.0f} s, {ROOT})')
        while True:
            try:
                poll(P, st, a)
            except Exception as e:          # (a poll's failure is logged; the watcher goes on)
                log(P, f'poll error: {type(e).__name__}: {e}')
            save_state(P, st)
            if a.once:
                return 0
            t_end = time.time() + a.every
            while time.time() < t_end:
                if os.path.exists(P['stop']):
                    os.remove(P['stop'])
                    log(P, 'STOP: watcher exits')
                    return 0
                time.sleep(5)
    finally:
        lock.__exit__()


if __name__ == '__main__':
    sys.exit(main())
