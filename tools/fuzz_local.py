"""One fuzz job on this machine, run exactly as a zero-farm PC runs it (docs/ENGINE.md 171; zero-farm PROTOCOL.md 5,
5.1): a worker executable's `serve` gets one `job` line and its events are read - the same chunk processes, failure
handling, reproduction strings and summary as on the farm. The fuzz steward (tools/fuzz_steward.py) uses it on the
desktop (the build's signed fuzz worker) and on the Mac (the commit's `mtgz-worker` built there with `--profile fuzz`;
the steward ships this file's text, so it must stay standard library only and not import anything of the checkout).

    python tools/fuzz_local.py --worker EXE --mode M [--env K=V ...] --first F --count N [--threads 2]
           [--chunk C] [--work DIR] [--stop-file PATH] [--max-s SECONDS] [--idle]

Output (stdout, one line each, flushed as they happen):
    FUZZ_LOCAL_HELLO {"build_id", "kinds", ...}                    the worker's hello
    FUZZ_LOCAL_FAILURE {"seed", "mode", "site", "panic", "repro", "output_tail"}   each failing seed
    FUZZ_LOCAL_SUMMARY {"mode", "env", "first", "count", "done", "covered": [[lo, hi], ...], "failures": n,
                        "unfinished", "reason", "seconds", "build_id", "engine_commit"}
`done` and `covered` come from the worker's own summary (`out/fuzz-final.json`): the seeds that ran to their end,
failing ones included. `--stop-file` / `--max-s` / SIGTERM / SIGINT: the job is asked to `finish` (each chunk ends
the seed it is playing; the summary lists what is unfinished), and killed 60 s later if it has not ended. `--idle`:
the worker (and its chunk processes, which inherit it) at the lowest CPU priority (Windows IDLE_PRIORITY_CLASS,
POSIX nice 19), no console window. `--chunk C`: the job's chunks of exactly C seeds (the job's `chunk`; default the
worker's own cut, at least the loop's least chunk) - the steward's replay of a failure of a chunk's own checks is that
chunk, cut as the farm cut it (docs/ENGINE.md 209).
Exit: 0 the job ended (failures are results, not errors), 2 the worker refused the job or broke.
"""
import argparse
import json
import os
import signal
import subprocess
import sys
import threading
import time

WIN = sys.platform == 'win32'


def emit(tag, obj):
    sys.stdout.write(f'{tag} {json.dumps(obj, separators=(",", ":"))}\n')
    sys.stdout.flush()


def parse_env(items):
    env = {}
    for it in items or []:
        k, sep, v = it.partition('=')
        if not sep or not k:
            raise SystemExit(f'fuzz_local: --env {it!r}: K=V')
        env[k] = v
    return env


def start_worker(exe, work, idle):
    kw = {}
    if WIN:
        flags = subprocess.CREATE_NO_WINDOW
        if idle:
            flags |= subprocess.IDLE_PRIORITY_CLASS
        kw['creationflags'] = flags
    elif idle:
        kw['preexec_fn'] = lambda: os.nice(19)
    return subprocess.Popen([exe, 'serve'], cwd=work, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                            stderr=open(os.path.join(work, 'worker.log'), 'ab'), **kw)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('--worker', required=True)
    ap.add_argument('--mode', required=True)
    ap.add_argument('--env', action='append', default=[], metavar='K=V')
    ap.add_argument('--first', type=int, required=True)
    ap.add_argument('--count', type=int, required=True)
    ap.add_argument('--threads', type=int, default=2)
    ap.add_argument('--chunk', type=int, default=None, help="chunks of exactly this many seeds (default: the worker's cut)")
    ap.add_argument('--work', default=None, help='the job\'s working directory (default: a new temp dir)')
    ap.add_argument('--stop-file', default=None)
    ap.add_argument('--max-s', type=float, default=None)
    ap.add_argument('--idle', action='store_true')
    ap.add_argument('--hello-timeout', type=float, default=120.0)
    a = ap.parse_args(argv)
    env = parse_env(a.env)
    work = a.work
    if not work:
        import tempfile
        work = tempfile.mkdtemp(prefix='fuzz-local-')
    os.makedirs(work, exist_ok=True)
    t0 = time.time()
    p = start_worker(os.path.abspath(a.worker), work, a.idle)
    events = []
    lock = threading.Condition()

    def reader():
        for raw in p.stdout:
            try:
                ev = json.loads(raw.decode('utf-8', 'replace'))
            except ValueError:
                continue
            with lock:
                events.append(ev)
                lock.notify_all()
        with lock:
            events.append({'ev': '_eof'})
            lock.notify_all()

    threading.Thread(target=reader, daemon=True).start()

    def send(obj):
        try:
            p.stdin.write((json.dumps(obj) + '\n').encode())
            p.stdin.flush()
        except OSError:
            pass

    stop = {'asked': None}

    def ask_stop(*_):
        stop['asked'] = stop['asked'] or time.time()

    signal.signal(signal.SIGINT, ask_stop)
    if hasattr(signal, 'SIGTERM'):
        signal.signal(signal.SIGTERM, ask_stop)

    hello, end, failures, error = None, None, [], None
    job_sent = False
    seen = 0
    while True:
        with lock:
            if seen >= len(events):
                lock.wait(timeout=1.0)
            new, seen = events[seen:], len(events)
        for ev in new:
            kind = ev.get('ev')
            if kind == 'hello':
                hello = ev
                emit('FUZZ_LOCAL_HELLO', {k: ev.get(k) for k in ('build_id', 'game', 'kinds', 'aw')})
                if 'fuzz' not in (ev.get('kinds') or []):
                    error = f'the worker runs {ev.get("kinds")}, no fuzz jobs'
                    send({'op': 'quit'})
                else:
                    job = {'op': 'job', 'lease_id': f'local-{a.first}', 'kind': 'fuzz', 'mode': a.mode, 'env': env,
                           'first': a.first, 'count': a.count, 'threads': max(1, a.threads), 'out_dir': 'out'}
                    if a.chunk:
                        job['chunk'] = a.chunk
                    send(job)
                    job_sent = True
            elif kind == 'failure':
                f = {k: ev.get(k) for k in ('seed', 'mode', 'site', 'panic', 'repro', 'output_tail')}
                failures.append(f)
                emit('FUZZ_LOCAL_FAILURE', f)
            elif kind == 'error':
                if ev.get('fatal'):
                    error = ev.get('message') or 'fatal error'
            elif kind == 'end':
                end = ev
            elif kind == '_eof':
                end = end or {'reason': 'eof'}
        if end is not None:
            break
        now = time.time()
        if hello is None and now - t0 > a.hello_timeout:
            error = f'no hello from the worker in {a.hello_timeout:.0f} s'
            break
        if job_sent and stop['asked'] is None:
            if (a.stop_file and os.path.exists(a.stop_file)) or (a.max_s and now - t0 > a.max_s):
                ask_stop()
        if stop['asked'] is not None and not stop.get('sent'):
            send({'op': 'finish'})
            stop['sent'] = True
        if stop['asked'] is not None and now - stop['asked'] > 60:
            send({'op': 'quit'})
            if now - stop['asked'] > 90:
                break
    try:
        p.stdin.close()
    except OSError:
        pass
    try:
        p.wait(timeout=30)
    except subprocess.TimeoutExpired:
        p.kill()
        p.wait()
    summary = {}
    try:
        with open(os.path.join(work, 'out', 'fuzz-final.json'), encoding='utf-8') as f:
            summary = json.load(f)
    except (OSError, ValueError):
        pass
    covered = summary.get('covered') or summary.get('seeds') or None
    done = summary.get('done')
    if covered is None and done is not None and not (end or {}).get('unfinished'):
        covered = [[a.first, a.first + int(done)]] if done else []
    out = {'mode': a.mode, 'env': env, 'first': a.first, 'count': a.count, 'done': done, 'covered': covered,
           'failures': len(failures), 'unfinished': (end or {}).get('unfinished'), 'reason': (end or {}).get('reason'),
           'seconds': round(time.time() - t0, 1), 'build_id': (hello or {}).get('build_id') or summary.get('build_id'),
           'engine_commit': summary.get('engine_commit'), 'error': error, 'work': work}
    emit('FUZZ_LOCAL_SUMMARY', out)
    return 2 if error or done is None else 0


if __name__ == '__main__':
    sys.exit(main())
