"""Deadline guard of a rented box (tools/vast.py; copied from dominion-zero domz/az/box_guard.py, docs/az.md 10.5 there). Standard library only: the instance's
start script writes this file and runs it before any of our code is on the box.

    python box_guard.py <guard dir>        # <guard dir>/deadline holds a Unix time

Until the deadline it writes <guard dir>/guard.json every 30 s (the desktop may lower or
raise the deadline by rewriting the file). At the deadline it stops our processes (the
run's supervisor and run, benchmarks, comparisons: TERM, then KILL) and asks Vast.ai to
destroy this instance with the per-instance key the platform puts into the container
(CONTAINER_API_KEY / CONTAINER_ID), falling back to stopping it (GPU billing ends, disk
billing continues until the desktop destroys it). Without that key it can only stop our
processes - the desktop's watchdog destroys the instance. guard.json records what it did.
"""
import json
import os
import signal
import sys
import time
import urllib.request

API = 'https://console.vast.ai/api/v0'
OURS = ('mtgz.az.', 'tools/supervise.py', 'bench_threads.py')


def _write(d, obj):
    tmp = os.path.join(d, 'guard.json.tmp')
    with open(tmp, 'w') as f:
        json.dump(obj, f)
    os.replace(tmp, os.path.join(d, 'guard.json'))


def _log(d, msg):
    with open(os.path.join(d, 'guard.log'), 'a') as f:
        f.write(time.strftime('%Y-%m-%d %H:%M:%S ') + msg + '\n')


def our_pids():
    me = os.getpid()
    out = []
    for p in os.listdir('/proc') if os.path.isdir('/proc') else []:
        if not p.isdigit() or int(p) == me:
            continue
        try:
            with open(f'/proc/{p}/cmdline', 'rb') as f:
                cmd = f.read().replace(b'\0', b' ').decode('utf-8', 'replace')
        except OSError:
            continue
        if any(x in cmd for x in OURS) and 'vast_guard' not in cmd:
            out.append(int(p))
    return out


def stop_ours(d, grace=20):
    pids = our_pids()
    for p in pids:
        try:
            os.kill(p, signal.SIGTERM)
        except OSError:
            pass
    t0 = time.time()
    while time.time() - t0 < grace and our_pids():
        time.sleep(1)
    left = our_pids()
    for p in left:
        try:
            os.kill(p, signal.SIGKILL)
        except OSError:
            pass
    return {'terminated': len(pids), 'killed': len(left)}


def api(method, path, body=None):
    key = os.environ.get('CONTAINER_API_KEY')
    if not key:
        return {'ok': False, 'error': 'no CONTAINER_API_KEY in the environment'}
    data = json.dumps(body or {}).encode()
    req = urllib.request.Request(API + path, data=data, method=method,
                                 headers={'Authorization': f'Bearer {key}', 'Content-Type': 'application/json'})
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            return {'ok': 200 <= r.status < 300, 'status': r.status, 'body': r.read(500).decode('utf-8', 'replace')}
    except Exception as e:                     # noqa: BLE001
        return {'ok': False, 'error': f'{type(e).__name__}: {e}'[:300]}


def main():
    d = sys.argv[1]
    os.makedirs(d, exist_ok=True)
    env_names = sorted(k for k in os.environ if 'CONTAINER' in k or 'VAST' in k)     # names only, no values
    _log(d, f'guard {os.getpid()} up; environment names {env_names}')
    st = {'pid': os.getpid(), 'started': time.time(), 'env_names': env_names, 'state': 'waiting'}
    while True:
        try:
            with open(os.path.join(d, 'deadline')) as f:
                dl = float(f.read().split()[0])
        except (OSError, ValueError, IndexError):
            dl = None
        now = time.time()
        st.update(time=now, deadline=dl)
        _write(d, st)
        if dl is not None and now >= dl:
            break
        time.sleep(30 if dl is None else min(30.0, max(1.0, dl - now)))
    _log(d, 'DEADLINE PASSED: stopping our processes and destroying this instance')
    st.update(state='fired', fired=time.time(), stopped=stop_ours(d))
    iid = os.environ.get('CONTAINER_ID')
    if not iid:
        try:
            with open(os.path.join(d, 'instance_id')) as f:
                iid = f.read().strip()
        except OSError:
            iid = None
    res = {'ok': False, 'error': 'no instance id'}
    if iid:
        res = api('DELETE', f'/instances/{iid}/')
        st['destroy'] = res
        if not res.get('ok'):
            st['stop'] = api('PUT', f'/instances/{iid}/', {'state': 'stopped'})
    _write(d, st)
    _log(d, f'fired: {json.dumps(st)[:800]}')


if __name__ == '__main__':
    main()
