"""Rented GPU boxes (Vast.ai) for MTG Zero: rent one, prepare it with a run's exact code, then either
(1) let a desktop run self-play on it (run setting selfplay_remote 'vast', mtgz/az/__init__.py) or
(2) move a whole run onto it and keep a desktop mirror of its outputs (run-push / run-start / run-sync /
run-home). The box destroys itself at its deadline (tools/vast_guard.py, started by the instance's start
script before anything else), so a forgotten box costs at most hourly x hours.

  python tools/vast.py offers [--max-dph 1.2] [--min-ram 64]
  python tools/vast.py rent NAME --hours H [--offer ID] [--max-dph D] [--min-ram G]   # + ssh alias mtgz-NAME
  python tools/vast.py prep NAME [--checkout DIR] [--gate DIGEST]   # the checkout's commit built on the box
  python tools/vast.py status                                       # boxes, deadlines, spend, balance
  python tools/vast.py extend NAME --hours H                        # new deadline H hours from now
  python tools/vast.py ssh NAME [CMD...]
  python tools/vast.py destroy NAME | --all

  # (2) a whole run on the box (RUN = the desktop run dir, stopped; the box's copy is /root/runs/<run name>)
  python tools/vast.py run-push NAME RUN                 # upload what the run resumes from; desktop copy PAUSEd
  python tools/vast.py run-start NAME RUN [--checkout DIR] [-- extra run args]   # supervise.py on the box
  python tools/vast.py run-sync NAME RUN [--loop SEC] [--no-data]                # box outputs -> desktop mirror
  python tools/vast.py run-ctl NAME RUN stop|pause|resume|tail
  python tools/vast.py run-home NAME RUN [--wait-min M]  # STOP on the box, full pull, desktop copy runnable again

Box names are short words ('vast' is the one the run setting selfplay_remote 'vast' reaches: ssh alias
mtgz-vast, unless MTGZ_VAST_HOST names another alias). The ledger is ~/.config/vastai/mtgz_boxes.json; the ssh
aliases are a managed block per box in ~/.ssh/config (what zero-core's ssh transport and git use). Costs: the box
bills from creation until it is destroyed (GPU + disk), so `destroy` when done and check `status`.

While a run lives on a box, its desktop dir holds ON_VAST.json (which box, since when, the settings run-start
changed) and a PAUSE file by 'tools/vast.py' (a desktop supervisor started by mistake waits instead of forking the
run). run-home brings the newest state back, restores those settings, removes both and leaves STOP (relaunch on
the desktop as usual: the supervisor moves STOP away).
"""
import argparse
import ast
import base64
import io
import json
import os
import re
import shlex
import subprocess
import sys
import tarfile
import threading
import time
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
sys.path.insert(0, HERE)
import vast_provider as vp  # noqa: E402

LEDGER = os.path.expanduser('~/.config/vastai/mtgz_boxes.json')
SSH_CONFIG = os.path.expanduser('~/.ssh/config')
IMAGE = 'pytorch/pytorch:2.11.0-cuda12.8-cudnn9-runtime'     # = the desktop's torch 2.11.0+cu128
DISK_GB = 120
RUST_TOOLCHAIN = '1.95.0'                                     # = the desktop's rustc
BOX_HOME = '/root'
BOX_RUNS = '/root/runs'
BOX_PY = '/usr/bin/python'
GUARD_DIR = '/root/mtgz/guard'
GUARD_MARGIN_S = 600          # the box's own guard fires this long after the ledger's deadline
# CPUs whose per-core speed is close to the desktop's i9-13900K (search threads): preferred, then price
FAST_CPU = r'9950X|9900X|7950X|7945HX|9955HX|7900X|9800X3D|7800X3D|9700X|7700X|1[34]900|1[34]700|285K|265K|Ultra 9|Ultra 7'
FILTERS = dict(vp.FILTERS, gpu_name='RTX 5090', min_cores=16, min_ram_gb=60, max_dph=1.2, min_disk_gb=DISK_GB)
ON_VAST = 'ON_VAST.json'
PAUSE_BY = 'tools/vast.py'
# settings run-start changes for the box (no Mac from there; nothing remote) and run-home restores
BOX_SETTINGS = {'eval_remote': '', 'selfplay_remote': '', 'selfplay_remote_procs': 0, 'selfplay_remote_linger': 0,
                'selfplay_local_linger': 0}
PATH_SETTINGS = ('elo_anchors', 'heldout_shards')
APPEND = ('metrics.jsonl', 'evals.jsonl', 'log.txt', 'stdout.txt', 'supervisor.log', 'supervisor_console.txt')
NO_PULL = ('config.json', 'STOP', 'PAUSE', 'STOP.old', ON_VAST)


def die(msg):
    raise SystemExit(f'vast.py: {msg}')


def say(msg):
    print(time.strftime('%H:%M:%S ') + msg, flush=True)


# ------------------------------------------------------------------ ledger + ssh aliases

def load_ledger():
    try:
        with open(LEDGER, encoding='utf-8') as f:
            return json.load(f)
    except (OSError, ValueError):
        return {}


def save_ledger(led):
    os.makedirs(os.path.dirname(LEDGER), exist_ok=True)
    tmp = LEDGER + '.tmp'
    with open(tmp, 'w', encoding='utf-8') as f:
        json.dump(led, f, indent=1)
    os.replace(tmp, LEDGER)


def alias(name):
    if name == 'vast' and os.environ.get('MTGZ_VAST_HOST'):
        return os.environ['MTGZ_VAST_HOST']
    return f'mtgz-{name}'


def _block_re(name):
    a = re.escape(alias(name))
    return re.compile(rf'# >>> {a} \(tools/vast\.py\) >>>\n.*?# <<< {a} <<<\n', re.S)


def write_alias(name, host, port):
    a = alias(name)
    block = (f'# >>> {a} (tools/vast.py) >>>\nHost {a}\n  HostName {host}\n  Port {port}\n  User root\n'
             f'  IdentityFile ~/.ssh/id_ed25519\n  StrictHostKeyChecking no\n  UserKnownHostsFile /dev/null\n'
             f'  LogLevel ERROR\n  ServerAliveInterval 15\n  ServerAliveCountMax 4\n  ConnectTimeout 20\n'
             f'# <<< {a} <<<\n')
    try:
        with open(SSH_CONFIG, encoding='utf-8') as f:
            s = f.read()
    except OSError:
        s = ''
    s = _block_re(name).sub('', s)
    # first in the file: ssh takes the first value it finds for each option
    with open(SSH_CONFIG, 'w', encoding='utf-8', newline='\n') as f:
        f.write(block + s)


def remove_alias(name):
    try:
        with open(SSH_CONFIG, encoding='utf-8') as f:
            s = f.read()
    except OSError:
        return
    t = _block_re(name).sub('', s)
    if t != s:
        with open(SSH_CONFIG, 'w', encoding='utf-8', newline='\n') as f:
            f.write(t)


def ssh(name, cmd, input=None, check=True, timeout=600, capture=True):
    """Run `cmd` (a bash script) on box `name` -> CompletedProcess (stdout / stderr decoded). `input` goes as
    UTF-8 bytes: a text-mode pipe on Windows would turn its newlines into CRLF (bash: "pipefail\\r")."""
    r = subprocess.run(['ssh', '-o', 'BatchMode=yes', alias(name), cmd],
                       input=input.encode('utf-8') if input is not None else None,
                       capture_output=capture, timeout=timeout, stdin=None if input is not None else subprocess.DEVNULL)
    if capture:
        r.stdout = r.stdout.decode('utf-8', 'replace')
        r.stderr = r.stderr.decode('utf-8', 'replace')
    if check and r.returncode:
        die(f'ssh {alias(name)} failed ({r.returncode}): {((r.stderr or "") + (r.stdout or "")).strip()[-1500:]}')
    return r


def box(name):
    e = load_ledger().get(name)
    if not e or e.get('state') == 'destroyed':
        die(f'no box {name!r} in the ledger (python tools/vast.py status)')
    return e


# ------------------------------------------------------------------ renting

def onstart_script(deadline):
    """The instance's start script: write and start the deadline guard before anything else, so even a box
    the desktop never reaches destroys itself at `deadline`."""
    with open(os.path.join(HERE, 'vast_guard.py'), encoding='utf-8') as f:
        tree = ast.parse(f.read())
    for node in ast.walk(tree):                          # docstrings out: the start script's size is limited
        body = getattr(node, 'body', None)
        if isinstance(body, list) and body and isinstance(body[0], ast.Expr) \
                and isinstance(getattr(body[0], 'value', None), ast.Constant) and isinstance(body[0].value.value, str):
            node.body = body[1:] or [ast.Pass()]
    b64 = base64.b64encode(zlib.compress(ast.unparse(tree).encode(), 9)).decode()
    unpack = "import zlib,base64,sys;open(sys.argv[1],'wb').write(zlib.decompress(base64.b64decode(sys.argv[2])))"
    return (f'G={GUARD_DIR}; mkdir -p "$G"; echo {int(deadline + GUARD_MARGIN_S)} > "$G/deadline"; '
            f'P=$(command -v python3 || command -v python); "$P" -c "{unpack}" "$G/vast_guard.py" {b64}; '
            f'(nohup "$P" "$G/vast_guard.py" "$G" >> "$G/guard.out" 2>&1 &)')


def ranked_offers(p, f, hours, region='na'):
    """Offers passing the filters, in `region` ('na': the US and Canada - near the desktop; 'any'), fast desktop
    CPUs first, then by price. A far box is slow for everything: Taiwan (160 ms) moved ~0.1 MB/s each way on
    2026-11-05 (a 7 MB checkpoint took 73 s; remote self-play's shards missed their phase), Texas ~9 MB/s."""
    offers = [o for o in p.search(f, DISK_GB) if not vp.offer_problems(o, f, need_hours=hours)]
    if region == 'na':
        offers = [o for o in offers if re.search(r',\s*(US|CA)$', o['geo'] or '')]
    fast = re.compile(FAST_CPU, re.I)
    return sorted(offers, key=lambda o: (0 if fast.search(o['cpu']) else 1, vp.hourly(o, DISK_GB), -o['cores']))


def fmt_offer(o):
    star = '*' if re.search(FAST_CPU, o['cpu'], re.I) else ' '
    return (f"{o['id']:>10} {star} ${vp.hourly(o, DISK_GB):.3f}/h  {o['cores']:3.0f} thr {o['ram_gb']:4.0f} GB  "
            f"rel {o['reliability']:.3f}  {o['inet_down']:5.0f}/{o['inet_up']:5.0f} Mb/s  {o['cpu'][:36]:36}  {o['geo']}")


def filters_from(a):
    return dict(FILTERS, max_dph=a.max_dph, min_ram_gb=a.min_ram, min_cores=a.min_cores)


def cmd_offers(a):
    p = vp.VastCLI()
    offers = ranked_offers(p, filters_from(a), a.hours, a.region)
    for o in offers[:20]:
        print(fmt_offer(o))
    print(f'{len(offers)} offers pass the filters (* = a fast desktop CPU, preferred); balance ${p.balance():.2f}')


def wait_running(p, iid, timeout_s=900):
    t0 = time.time()
    last = None
    while time.time() - t0 < timeout_s:
        inst = p.show(iid)
        if inst is None:
            return None, 'the instance is gone'
        st = inst.get('status')
        if st != last:
            say(f'instance {iid}: {st} {inst.get("status_msg", "")[:120]}')
            last = st
        direct = [r for r in inst.get('ssh_routes') or [] if r['kind'] == 'direct']
        if st == 'running' and direct:
            return inst, None
        if st in ('exited', 'offline') or 'error' in (inst.get('status_msg') or '').lower():
            return None, f'status {st}: {inst.get("status_msg")}'
        time.sleep(10)
    return None, f'not running after {timeout_s} s'


def cmd_rent(a):
    led = load_ledger()
    if a.name in led and led[a.name].get('state') != 'destroyed':
        die(f'box {a.name!r} exists (destroy it first): {led[a.name]}')
    if not re.fullmatch(r'[a-z][a-z0-9]{0,15}', a.name):
        die('NAME: a short lowercase word (letters, digits)')
    p = vp.VastCLI()
    f = filters_from(a)
    offers = ranked_offers(p, f, a.hours, a.region)
    if a.offer:
        offers = [o for o in offers if o['id'] == a.offer]
        if not offers:
            die(f'offer {a.offer} is gone or fails the filters (python tools/vast.py offers)')
    if not offers:
        die('no offer passes the filters')
    tried = {}
    for o in offers[:3]:
        deadline = time.time() + a.hours * 3600
        say(f'renting {fmt_offer(o).strip()}  (deadline in {a.hours:g} h: at most ${vp.hourly(o, DISK_GB) * a.hours:.2f})')
        try:
            iid = p.create(o, IMAGE, DISK_GB, f'mtgz-{a.name}', onstart_script(deadline))
        except vp.ProviderError as e:
            tried[o['id']] = str(e)
            say(f'  failed: {e}')
            continue
        led[a.name] = {'name': a.name, 'iid': iid, 'offer': o['id'], 'hourly': round(vp.hourly(o, DISK_GB), 4),
                       'created': time.time(), 'deadline': deadline, 'cpu': o['cpu'], 'threads': o['cores'],
                       'ram_gb': round(o['ram_gb']), 'geo': o['geo'], 'state': 'starting'}
        save_ledger(led)
        inst, err = wait_running(p, iid)
        if inst is None:
            say(f'  instance {iid} did not come up ({err}): destroying it')
            p.destroy(iid)
            led[a.name].update(state='destroyed', destroyed=time.time(), note=err)
            save_ledger(led)
            led.pop(a.name)
            tried[o['id']] = err
            continue
        r = next(r for r in inst['ssh_routes'] if r['kind'] == 'direct')
        write_alias(a.name, r['host'], r['port'])
        led[a.name].update(state='running', ssh=r)
        save_ledger(led)
        for _ in range(18):                          # sshd may lag the status by a few seconds
            if ssh(a.name, 'true', check=False, timeout=60).returncode == 0:
                break
            time.sleep(10)
        g = ssh(a.name, f'cat {GUARD_DIR}/guard.json 2>/dev/null || echo missing', check=False, timeout=60).stdout.strip()
        ok = '"waiting"' in g and 'CONTAINER_API_KEY' in g
        say(f'box {a.name!r} = instance {iid} up: ssh {alias(a.name)} ({r["host"]}:{r["port"]}); deadline guard '
            + ('running (self-destroys at the deadline)' if ok else f'NOT CONFIRMED: {g[:200]}'))
        if not ok:
            say('  WARNING: without the guard only `destroy` / the ledger deadline protect against a forgotten box')
        return
    die(f'no offer could be rented: {tried}')


def cmd_status(a):
    p = vp.VastCLI()
    led = load_ledger()
    live = {i['id']: i for i in p.list()}
    now = time.time()
    total = 0.0
    for name, e in sorted(led.items()):
        inst = live.pop(e['iid'], None)
        state = (inst or {}).get('status') or ('gone' if e.get('state') != 'destroyed' else 'destroyed')
        end = e.get('destroyed') or now
        spent = e['hourly'] * max(0.0, end - e['created']) / 3600
        total += spent
        left = (e['deadline'] - now) / 3600
        print(f"{name:10} iid {e['iid']:<10} {state:10} ${e['hourly']:.3f}/h  ~${spent:.2f} so far  "
              + (f"deadline in {left:.1f} h  " if e.get('state') != 'destroyed' else '')
              + f"{e.get('cpu', '')[:30]}  ssh {alias(name)}")
    for i in live.values():
        print(f"(not in the ledger) iid {i['id']} {i.get('status')} {i.get('label')} ${i.get('dph', 0):.3f}/h")
    print(f'ledger spend ~${total:.2f}; balance ${p.balance():.2f}')


def cmd_extend(a):
    e = box(a.name)
    dl = time.time() + a.hours * 3600
    ssh(a.name, f'echo {int(dl + GUARD_MARGIN_S)} > {GUARD_DIR}/deadline')
    led = load_ledger()
    led[a.name]['deadline'] = dl
    save_ledger(led)
    say(f'box {a.name!r} (iid {e["iid"]}): deadline now in {a.hours:g} h (at most ${e["hourly"] * a.hours:.2f} more)')


def cmd_destroy(a):
    led = load_ledger()
    names = [n for n, e in led.items() if e.get('state') != 'destroyed'] if a.all else [a.name]
    p = vp.VastCLI()
    for n in names:
        e = led.get(n)
        if not e:
            die(f'no box {n!r}')
        p.destroy(e['iid'])
        gone = p.show(e['iid']) is None
        e.update(state='destroyed', destroyed=time.time())
        remove_alias(n)
        say(f'box {n!r} (iid {e["iid"]}) destroyed' + ('' if gone else ' - STILL LISTED, check `status`'))
    save_ledger(led)
    left = p.list()
    say(f'instances on the account now: {[i["id"] for i in left] or "none"}')


def cmd_ssh(a):
    box(a.name)
    r = subprocess.run(['ssh', alias(a.name)] + ([' '.join(a.cmd)] if a.cmd else []))
    sys.exit(r.returncode)


# ------------------------------------------------------------------ preparing a checkout

def git(root, *args):
    return subprocess.run(['git', '-C', root, *args], capture_output=True, text=True, check=True).stdout.strip()


def box_repo(sha):
    return f'{BOX_HOME}/mtg-zero-wt/run-{sha[:12]}'      # = mtgz.az.mac.eval_checkout(sha, BOX_HOME)


def cmd_prep(a):
    box(a.name)
    co = os.path.abspath(a.checkout or ROOT)
    sha = git(co, 'rev-parse', 'HEAD')
    pin = git(co, 'rev-parse', 'HEAD:third_party/zero-core')
    if git(co, 'status', '--porcelain', '--untracked-files=no'):
        die(f'{co} has uncommitted changes: the box builds a commit (commit first)')
    t0 = time.time()
    say(f'box {a.name!r}: bootstrap (apt, rust {RUST_TOOLCHAIN}, pip, torch check)')
    with open(os.path.join(HERE, 'vast_bootstrap.sh'), encoding='utf-8') as f:
        r = ssh(a.name, f'RUST_TOOLCHAIN={RUST_TOOLCHAIN} bash -s', input=f.read(), timeout=1200)
    for line in r.stdout.splitlines():
        if line.startswith(('torch', 'device', 'python', 'rustc', '{')):
            print('   ', line[:200])
    ssh(a.name, 'git init -q --bare ~/mtg-zero.git 2>/dev/null; git init -q --bare ~/mtg-zero-core.git 2>/dev/null; true')
    say(f'push {sha[:12]} + zero-core {pin[:12]}')
    subprocess.run(['git', '-C', co, 'push', '-q', '-f', f'{alias(a.name)}:mtg-zero.git', f'{sha}:refs/heads/run-{sha[:12]}'],
                   check=True)
    subprocess.run(['git', '-C', os.path.join(co, 'third_party', 'zero-core'), 'push', '-q', '-f',
                    f'{alias(a.name)}:mtg-zero-core.git', f'{pin}:refs/heads/pin-{pin[:12]}'], check=True)
    repo = box_repo(sha)
    script = f'''set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH" PYTHON={BOX_PY} PYO3_PYTHON={BOX_PY}
[ -d ~/mtg-zero/.git ] || git clone -q --no-checkout ~/mtg-zero.git ~/mtg-zero
cd ~/mtg-zero && git fetch -q origin "+refs/heads/run-{sha[:12]}:refs/remotes/origin/run-{sha[:12]}"
[ -d {repo} ] || git worktree add -q --detach {repo} {sha}
cd {repo} && git checkout -q -f --detach {sha}
git config submodule.third_party/zero-core.url ~/mtg-zero-core.git
git -c protocol.file.allow=always submodule update -q --init third_party/zero-core
STAMP="$(git rev-parse --git-path mtgz-engine-commit)"
if [ "$(cat "$STAMP" 2>/dev/null)" != "{sha}" ] || [ ! -f mtgz/_engine.so ]; then
  rm -f "$STAMP"; bash tools/build_py.sh 2>&1 | tail -2; git rev-parse HEAD > "$STAMP"
else echo "engine already built at {sha[:12]}"; fi
{BOX_PY} -m mtgz.az.remote check {sha} && echo "checkout ready: {repo}"
'''
    say(f'checkout + build in {repo}')
    print(ssh(a.name, 'bash -s', input=script, timeout=1800).stdout.strip())
    if a.gate:
        say(f'gate: tools/gate.py repin --expect-digest {a.gate} (build, digest, 1-config Delver smoke)')
        g = ssh(a.name, f'cd {repo} && export PATH="$HOME/.cargo/bin:$PATH" PYTHON={BOX_PY} PYO3_PYTHON={BOX_PY} && '
                        f'{BOX_PY} tools/gate.py repin --expect-digest {a.gate} --spec delver --preset delver-mirror '
                        f'--no-mac 2>&1 | tail -8', timeout=3600, check=False)
        print(g.stdout.strip())
        if 'GATE PASSED' not in g.stdout:
            die('the gate did not pass on the box (above)')
    led = load_ledger()
    led[a.name].setdefault('checkouts', {})[sha[:12]] = {'sha': sha, 'pin': pin, 'repo': repo, 'gate': a.gate or None,
                                                         'time': time.time()}
    save_ledger(led)
    say(f'box {a.name!r} ready with {sha[:12]} in {time.time() - t0:.0f} s')


# ------------------------------------------------------------------ (2) a whole run on the box

def box_run_dir(run):
    return f'{BOX_RUNS}/{os.path.basename(os.path.normpath(run))}'


def ext_path(p):
    return f'{BOX_RUNS}/_ext/{os.path.basename(p)}'


def supervisor_alive(run):
    try:
        with open(os.path.join(run, 'supervisor_state.json'), encoding='utf-8') as f:
            pid = json.load(f).get('pid')
    except (OSError, ValueError):
        return None
    try:
        import psutil
        return pid if pid and psutil.pid_exists(pid) and 'python' in psutil.Process(pid).name().lower() else None
    except Exception:
        return None


def newest_iter(run):
    its = [int(m.group(1)) for n in os.listdir(os.path.join(run, 'models')) for m in [re.fullmatch(r'iter_(\d+)\.pt', n)] if m]
    return max(its) if its else None


def remap_paths(cfg):
    """{setting: (desktop value, box value)} for the settings holding desktop paths; the files to upload."""
    out, files = {}, []
    if cfg.get('elo_anchors'):
        parts = []
        for item in str(cfg['elo_anchors']).split(','):
            tag, _, path = item.partition('=')
            files.append(path)
            parts.append(f'{tag}={ext_path(path)}')
        out['elo_anchors'] = (cfg['elo_anchors'], ','.join(parts))
    if cfg.get('heldout_shards'):
        paths = [x for x in str(cfg['heldout_shards']).split(',') if x]
        files += paths
        out['heldout_shards'] = (cfg['heldout_shards'], ','.join(ext_path(x) for x in paths))
    return out, files


def tar_upload(name, dest, members):
    """members: [(arcname, local path | bytes)] -> extracted under `dest` on the box (one ssh stream)."""
    proc = subprocess.Popen(['ssh', '-o', 'BatchMode=yes', alias(name), f'mkdir -p {shlex.quote(dest)} && tar xf - -C {shlex.quote(dest)}'],
                            stdin=subprocess.PIPE)
    total = 0
    with tarfile.open(fileobj=proc.stdin, mode='w|') as tf:
        for arc, src in members:
            if isinstance(src, bytes):
                ti = tarfile.TarInfo(arc)
                ti.size = len(src)
                ti.mtime = time.time()
                tf.addfile(ti, io.BytesIO(src))
                total += len(src)
            else:
                tf.add(src, arcname=arc, recursive=False)
                total += os.path.getsize(src)
    proc.stdin.close()
    if proc.wait():
        die(f'upload to {alias(name)}:{dest} failed')
    return total


def cmd_run_push(a):
    e = box(a.name)
    run = os.path.abspath(a.run)
    if os.path.exists(os.path.join(run, ON_VAST)):
        die(f'{run} is already on a box: {open(os.path.join(run, ON_VAST)).read()}')
    pid = supervisor_alive(run)
    if pid:
        die(f'the run\'s supervisor (pid {pid}) is running: stop the run first (STOP file, wait for its clean exit)')
    with open(os.path.join(run, 'config.json'), encoding='utf-8') as f:
        cfg = json.load(f)
    k = newest_iter(run)
    paths, ext = remap_paths(cfg)
    for p in ext:
        if not os.path.exists(p):
            die(f'{p} (a setting of the run) does not exist')
    box_cfg = dict(cfg, **{s: v[1] for s, v in paths.items()}, **BOX_SETTINGS)
    members = [('config.json', json.dumps(box_cfg, indent=1).encode())]
    for n in ('run_state.json', 'metrics.jsonl', 'evals.jsonl', 'log.txt', 'stdout.txt', 'supervisor.log',
              'supervisor_console.txt', 'progress.json'):
        if os.path.exists(os.path.join(run, n)):
            members.append((n, os.path.join(run, n)))
    for sub in ('elo', 'data'):
        for dp, _dn, fns in os.walk(os.path.join(run, sub)):
            for fn in fns:
                full = os.path.join(dp, fn)
                members.append((os.path.relpath(full, run).replace('\\', '/'), full))
    mdir = os.path.join(run, 'models')
    want = {f'iter_{i:04d}.pt' for i in range(max(0, k - 32), k + 1)} | {'opt_latest.pt', 'index.json'}
    try:
        with open(os.path.join(mdir, 'index.json'), encoding='utf-8') as f:
            best = (json.load(f).get('best') or {}).get('file')
        if best:
            want.add(best)
    except (OSError, ValueError):
        pass
    for fn in sorted(os.listdir(mdir)):
        if fn in want:
            members.append((f'models/{fn}', os.path.join(mdir, fn)))
    size = sum(len(s) if isinstance(s, bytes) else os.path.getsize(s) for _, s in members)
    say(f'run-push {run} (newest checkpoint {k}) -> {alias(a.name)}:{box_run_dir(run)}: {len(members)} files, '
        f'{size / 1e9:.2f} GB (+ {len(ext)} anchor / held-out files)')
    marker = {'box': a.name, 'iid': e['iid'], 'alias': alias(a.name), 'remote_dir': box_run_dir(run),
              'since': time.time(), 'iter_at_push': k, 'restore': {**{s: v[0] for s, v in paths.items()},
                                                                   **{s: cfg.get(s) for s in BOX_SETTINGS}}}
    # the desktop copy is PAUSEd before the upload: nothing may start it while the box owns the run
    with open(os.path.join(run, 'PAUSE'), 'w', encoding='utf-8') as f:
        json.dump({'time': time.time(), 'by': PAUSE_BY}, f)
    with open(os.path.join(run, ON_VAST), 'w', encoding='utf-8') as f:
        json.dump(marker, f, indent=1)
    t0 = time.time()
    # the desktop copy is the run now: an older copy on the box (an earlier push) could hold newer checkpoints
    # or data the run would resume from
    rd = box_run_dir(run)
    assert rd.startswith(BOX_RUNS + '/') and len(rd) > len(BOX_RUNS) + 1, rd
    ssh(a.name, f'rm -rf {shlex.quote(rd)}')
    tar_upload(a.name, f'{BOX_RUNS}/_ext', [(os.path.basename(p), p) for p in ext])
    tar_upload(a.name, box_run_dir(run), members)
    say(f'uploaded in {time.time() - t0:.0f} s; desktop copy PAUSEd + {ON_VAST}; next: run-start')


def run_args(run, marker):
    """The desktop supervisor's run arguments for the box: remote settings off, desktop paths remapped."""
    with open(os.path.join(run, 'supervisor_state.json'), encoding='utf-8') as f:
        args = list(json.load(f).get('args') or [])
    drop = {'--' + s.replace('_', '-') for s in BOX_SETTINGS}
    remap = {'--' + s.replace('_', '-'): s for s in PATH_SETTINGS}
    out, i = [], 0
    while i < len(args):
        x = args[i]
        if x in drop:
            i += 2
            continue
        if x in remap and i + 1 < len(args):
            box_cfg = remap_paths({remap[x]: args[i + 1]})[0]
            out += [x, box_cfg[remap[x]][1]]
            i += 2
            continue
        out.append(x)
        i += 1
    return out


def cmd_run_start(a):
    e = box(a.name)
    run = os.path.abspath(a.run)
    marker = read_marker(run, a.name)
    co = os.path.abspath(a.checkout or ROOT)
    sha = git(co, 'rev-parse', 'HEAD')
    if sha[:12] not in (e.get('checkouts') or {}):
        die(f'box {a.name!r} has no prepared checkout of {sha[:12]} ({co}): python tools/vast.py prep {a.name} --checkout {co}')
    repo = box_repo(sha)
    args = run_args(run, marker) + list(a.extra or [])
    rd = box_run_dir(run)
    envs = 'ZERO_TOKEN_GRAPHS=1 MTGZ_SUPERVISE_STALL=7200'
    # (the braces: `a && b && c &` would background the whole list in a subshell that keeps the ssh channel
    # open until the run ends; only the supervisor goes to the background, its own session, no ssh fds)
    cmd = (f'cd {repo} && {BOX_PY} -m mtgz.az.remote check {sha} >/dev/null && rm -f {rd}/PAUSE && '
           f'{{ {envs} setsid nohup {BOX_PY} tools/supervise.py {rd} {" ".join(shlex.quote(x) for x in args)} '
           f'> {rd}/supervisor_console.txt 2>&1 < /dev/null & }} && sleep 2 && echo started')
    say(f'run-start on {alias(a.name)} from {repo}:\n  {envs} python tools/supervise.py {rd} {" ".join(args)}')
    print(ssh(a.name, cmd).stdout.strip())
    led = load_ledger()
    led[a.name]['run'] = {'run': run, 'remote_dir': rd, 'sha': sha, 'args': args, 'started': time.time()}
    save_ledger(led)
    marker.update(sha=sha, started=time.time(), args=args)
    with open(os.path.join(run, ON_VAST), 'w', encoding='utf-8') as f:
        json.dump(marker, f, indent=1)
    time.sleep(45)
    print(ssh(a.name, f'tail -3 {rd}/supervisor.log; tail -c 600 {rd}/stdout.txt', check=False).stdout)


def read_marker(run, name):
    try:
        with open(os.path.join(run, ON_VAST), encoding='utf-8') as f:
            m = json.load(f)
    except (OSError, ValueError):
        die(f'{run} is not on a box (no {ON_VAST}: run-push first)')
    if m['box'] != name:
        die(f'{run} is on box {m["box"]!r}, not {name!r}')
    return m


def remote_listing(name, rd):
    r = ssh(name, f'cd {rd} && find . -type f -printf "%P\\t%s\\t%T@\\n"', timeout=300)
    out = {}
    for line in r.stdout.splitlines():
        p, s, t = line.split('\t')
        out[p] = (int(s), float(t))
    return out


def tar_fetch(name, rd, run, paths, timeout_s=1800):
    """Fetch `paths` (relative) from the box's `rd` into `run`, each file written to a temporary name and
    renamed (readers never see half a file). The stream is read to its end: tarfile stops at the end-of-archive
    marker, and the record padding after it, left unread, kept ssh blocked on a full pipe forever (the main line's
    sync loop hung 2026-11-05 14:40). A transfer still running after `timeout_s` is killed (the pass fails, the
    next one retries)."""
    if not paths:
        return 0
    proc = subprocess.Popen(['ssh', '-o', 'BatchMode=yes', alias(name), f'cd {rd} && tar cf - --files-from=-'],
                            stdin=subprocess.PIPE, stdout=subprocess.PIPE)
    killer = threading.Timer(timeout_s, proc.kill)
    killer.daemon = True
    killer.start()
    n = 0
    try:
        proc.stdin.write(('\n'.join(paths) + '\n').encode())
        proc.stdin.close()
        with tarfile.open(fileobj=proc.stdout, mode='r|') as tf:
            for m in tf:
                if not m.isfile():
                    continue
                dst = os.path.join(run, *m.name.split('/'))
                os.makedirs(os.path.dirname(dst), exist_ok=True)
                src = tf.extractfile(m)
                with open(dst + '.vasttmp', 'wb') as f:
                    while True:
                        b = src.read(1 << 20)
                        if not b:
                            break
                        f.write(b)
                os.replace(dst + '.vasttmp', dst)
                n += m.size
        while proc.stdout.read(1 << 16):                   # the archive's trailing padding (see above)
            pass
    except (tarfile.TarError, OSError) as ex:
        proc.kill()
        die(f'fetch from {alias(name)}:{rd} failed: {type(ex).__name__}: {ex}')
    finally:
        killer.cancel()
    if proc.wait(timeout=120):
        die(f'fetch from {alias(name)}:{rd} failed (ssh exit {proc.returncode})')
    return n


def sync_once(name, run, rd, data=True, final=False):
    """Pull the box run's outputs into the desktop dir: new / changed files, append-only logs by their new
    bytes. Not pulled while the run lives on the box: config.json (box paths; run-home restores it), the
    control files; without `data` (or `final`), data/ and models/opt_latest.pt."""
    remote = remote_listing(name, rd)
    now = time.time()
    fetch, appended = [], 0
    for p, (size, mtime) in remote.items():
        base = p.split('/')[-1]
        if p in NO_PULL and not (final and p in ('config.json', 'STOP')):
            continue
        if not final and ((not data and p.startswith('data/')) or p == 'models/opt_latest.pt'):
            continue
        if base.endswith(('.tmp', '.vasttmp')) or base.startswith('.'):
            continue
        # a file written in the last 20 s may be half written: next pass - except the append-only logs, which take
        # only their new bytes (a partial last line is completed by the next pass); the box's log.txt is written
        # every few seconds and was never pulled under this rule (the main line's mirror, 2026-11-05)
        if not final and now - mtime < 20 and not p.endswith('.json') and p not in APPEND:
            continue
        local = os.path.join(run, *p.split('/'))
        have = os.path.getsize(local) if os.path.exists(local) else -1
        if p in APPEND and 0 <= have < size:
            r = subprocess.run(['ssh', '-o', 'BatchMode=yes', alias(name), f'tail -c +{have + 1} {rd}/{p} | head -c {size - have}'],
                               capture_output=True, timeout=600)
            if r.returncode == 0 and len(r.stdout) == size - have:
                with open(local, 'ab') as f:
                    f.write(r.stdout)
                appended += len(r.stdout)
                continue
        if have != size or p.endswith('.json'):
            fetch.append(p)
    got = tar_fetch(name, rd, run, fetch)
    removed = 0
    if data or final:                                        # the window's pruning, mirrored
        rdirs = {p.split('/')[1] for p in remote if p.startswith('data/') and p.count('/') >= 2}
        ddir = os.path.join(run, 'data')
        if rdirs and os.path.isdir(ddir):
            import shutil
            for d in os.listdir(ddir):
                if re.fullmatch(r'iter_\d+', d) and d not in rdirs and d < max(rdirs):
                    shutil.rmtree(os.path.join(ddir, d), ignore_errors=True)
                    removed += 1
    return len(fetch), got + appended, removed


def cmd_run_sync(a):
    box(a.name)
    run = os.path.abspath(a.run)
    m = read_marker(run, a.name)
    rd = m['remote_dir']
    while True:
        t0 = time.time()
        if a.loop and homing(run):
            say(f'run-home has the run ({ON_VAST} gone or homing): sync loop ends')
            return
        try:
            nf, nb, nr = sync_once(a.name, run, rd, data=not a.no_data)
            prog = ssh(a.name, f'cat {rd}/progress.json 2>/dev/null; echo; tail -1 {rd}/supervisor.log', check=False,
                       timeout=60).stdout.strip().replace('\n', ' | ')
            say(f'sync: {nf} files + logs, {nb / 1e6:.1f} MB in {time.time() - t0:.0f} s'
                + (f', {nr} old data dirs pruned' if nr else '') + f'; box: {prog[:200]}')
            with open(os.path.join(run, 'vast_sync.json'), 'w', encoding='utf-8') as f:
                json.dump({'time': time.time(), 'files': nf, 'bytes': nb, 'box': a.name,
                           'pid': os.getpid() if a.loop else None}, f)
        except (SystemExit, Exception) as ex:                  # never let one bad pass end the mirror
            say(f'sync failed (retrying): {type(ex).__name__}: {ex}')
        if not a.loop:
            return
        time.sleep(max(5.0, a.loop - (time.time() - t0)))


def homing(run):
    """True once run-home has taken the run (its marker says so, or is gone): a sync loop stops pulling."""
    try:
        with open(os.path.join(run, ON_VAST), encoding='utf-8') as f:
            return bool(json.load(f).get('homing'))
    except (OSError, ValueError):
        return True


def wait_sync_loop(run, timeout_s=300):
    """Wait for a sync loop of this run (its pid in vast_sync.json) to see the homing marker and exit, so that
    no pass of it overwrites what run-home pulls and writes."""
    try:
        with open(os.path.join(run, 'vast_sync.json'), encoding='utf-8') as f:
            pid = json.load(f).get('pid')
    except (OSError, ValueError):
        return
    if not pid:
        return
    import psutil
    t0 = time.time()
    while time.time() - t0 < timeout_s:
        try:
            p = psutil.Process(pid)
            if 'run-sync' not in ' '.join(p.cmdline()):
                return
        except (psutil.NoSuchProcess, psutil.AccessDenied):
            return
        time.sleep(2)
    die(f'the sync loop (pid {pid}) did not end within {timeout_s} s (stop it, then run-home again)')


def cmd_run_ctl(a):
    box(a.name)
    run = os.path.abspath(a.run)
    rd = read_marker(run, a.name)['remote_dir']
    if a.what == 'stop':
        ssh(a.name, f'echo \'{{"time": {time.time()}, "by": "{PAUSE_BY}"}}\' > {rd}/STOP')
        say('STOP written on the box: the run ends after its current iteration')
    elif a.what == 'pause':
        ssh(a.name, f'echo \'{{"time": {time.time()}, "by": "{PAUSE_BY}"}}\' > {rd}/PAUSE')
        say('PAUSE written on the box (the supervisor waits until resume)')
    elif a.what == 'resume':
        ssh(a.name, f'rm -f {rd}/PAUSE')
        say('PAUSE removed on the box')
    else:
        print(ssh(a.name, f'tail -5 {rd}/supervisor.log; echo; tail -c 1500 {rd}/stdout.txt; echo; cat {rd}/progress.json; '
                          f'nvidia-smi --query-gpu=utilization.gpu,power.draw,memory.used --format=csv,noheader; free -g | sed -n 2p',
                  check=False).stdout)


def box_supervisor_running(name):
    # anchored to the supervisor's own command line (python tools/supervise.py ...): an unanchored pattern also
    # matches the `bash -c` that runs pgrep, whose command line contains it
    r = ssh(name, "pgrep -f '^[^ ]*python[0-9.]* tools/supervise[.]py ' >/dev/null && echo yes || echo no", check=False,
            timeout=60)
    return r.stdout.strip() == 'yes'


def cmd_run_home(a):
    box(a.name)
    run = os.path.abspath(a.run)
    m = read_marker(run, a.name)
    rd = m['remote_dir']
    m['homing'] = time.time()                              # a sync loop of the run ends at its next pass
    with open(os.path.join(run, ON_VAST), 'w', encoding='utf-8') as f:
        json.dump(m, f, indent=1)
    if box_supervisor_running(a.name):
        ssh(a.name, f'echo \'{{"time": {time.time()}, "by": "{PAUSE_BY}"}}\' > {rd}/STOP')
        say(f'STOP on the box; waiting up to {a.wait_min} min for the run to end its iteration')
        t0 = time.time()
        while box_supervisor_running(a.name):
            if time.time() - t0 > a.wait_min * 60:
                die('the box run did not stop in time (nothing changed on the desktop; run-home again later)')
            time.sleep(15)
    wait_sync_loop(run)
    say('box run stopped: full pull (data window, optimizer, checkpoints, logs)')
    t0 = time.time()
    nf, nb, nr = sync_once(a.name, run, rd, data=True, final=True)
    say(f'pulled {nf} files + logs, {nb / 1e9:.2f} GB in {time.time() - t0:.0f} s')
    cfg_path = os.path.join(run, 'config.json')
    with open(cfg_path, encoding='utf-8') as f:
        cfg = json.load(f)
    cfg.update(m['restore'])                                # the desktop's paths and remote settings
    with open(cfg_path + '.vasttmp', 'w', encoding='utf-8') as f:
        json.dump(cfg, f, indent=1)
    os.replace(cfg_path + '.vasttmp', cfg_path)
    k = newest_iter(run)
    with open(os.path.join(run, 'supervisor.log'), 'a', encoding='utf-8') as f:
        f.write(time.strftime('%Y-%m-%d %H:%M:%S ') + f'tools/vast.py: run back from box {a.name!r} (iid {m["iid"]}) at '
                f'iteration {k} (pushed at {m["iter_at_push"]})\n')
    if not os.path.exists(os.path.join(run, 'STOP')):
        with open(os.path.join(run, 'STOP'), 'w', encoding='utf-8') as f:
            json.dump({'time': time.time(), 'by': PAUSE_BY}, f)
    pause = os.path.join(run, 'PAUSE')
    try:
        with open(pause, encoding='utf-8') as f:
            ours = PAUSE_BY in f.read()
    except OSError:
        ours = False                                       # (none: nothing to remove)
    if ours:                                               # (closed first: Windows cannot delete an open file)
        os.remove(pause)
    elif os.path.exists(pause):
        say(f'WARNING: {pause} is not tools/vast.py\'s (someone paused the run): left in place')
    os.replace(os.path.join(run, ON_VAST), os.path.join(run, ON_VAST.replace('.json', f'_{int(time.time())}.json')))
    say(f'{run} is home at iteration {k} (STOP left in place: relaunch on the desktop as usual); the box keeps its '
        f'copy until `destroy {a.name}`')


# ------------------------------------------------------------------ CLI

def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    sub = ap.add_subparsers(dest='cmd', required=True)

    def filt(p):
        p.add_argument('--max-dph', type=float, default=FILTERS['max_dph'])
        p.add_argument('--min-ram', type=int, default=FILTERS['min_ram_gb'])
        p.add_argument('--min-cores', type=int, default=FILTERS['min_cores'])
        p.add_argument('--hours', type=float, default=2.0)
        p.add_argument('--region', choices=('na', 'any'), default='na', help='na: US + Canada (default)')

    p = sub.add_parser('offers')
    filt(p)
    p.set_defaults(fn=cmd_offers)
    p = sub.add_parser('rent')
    p.add_argument('name')
    p.add_argument('--offer', type=int)
    filt(p)
    p.set_defaults(fn=cmd_rent)
    p = sub.add_parser('prep')
    p.add_argument('name')
    p.add_argument('--checkout')
    p.add_argument('--gate', metavar='DIGEST', help='run tools/gate.py repin on the box, expecting this engine digest')
    p.set_defaults(fn=cmd_prep)
    p = sub.add_parser('status')
    p.set_defaults(fn=cmd_status)
    p = sub.add_parser('extend')
    p.add_argument('name')
    p.add_argument('--hours', type=float, required=True)
    p.set_defaults(fn=cmd_extend)
    p = sub.add_parser('destroy')
    p.add_argument('name', nargs='?')
    p.add_argument('--all', action='store_true')
    p.set_defaults(fn=cmd_destroy)
    p = sub.add_parser('ssh')
    p.add_argument('name')
    p.add_argument('cmd', nargs=argparse.REMAINDER)
    p.set_defaults(fn=cmd_ssh)
    p = sub.add_parser('run-push')
    p.add_argument('name')
    p.add_argument('run')
    p.set_defaults(fn=cmd_run_push)
    p = sub.add_parser('run-start')
    p.add_argument('name')
    p.add_argument('run')
    p.add_argument('--checkout')
    p.add_argument('extra', nargs='*', help='after --: more run arguments')
    p.set_defaults(fn=cmd_run_start)
    p = sub.add_parser('run-sync')
    p.add_argument('name')
    p.add_argument('run')
    p.add_argument('--loop', type=float, default=0, help='repeat every SEC seconds until run-home')
    p.add_argument('--no-data', action='store_true', help='leave data/ (the window) on the box until run-home')
    p.set_defaults(fn=cmd_run_sync)
    p = sub.add_parser('run-ctl')
    p.add_argument('name')
    p.add_argument('run')
    p.add_argument('what', choices=('stop', 'pause', 'resume', 'tail'))
    p.set_defaults(fn=cmd_run_ctl)
    p = sub.add_parser('run-home')
    p.add_argument('name')
    p.add_argument('run')
    p.add_argument('--wait-min', type=float, default=30)
    p.set_defaults(fn=cmd_run_home)
    a = ap.parse_args()
    if a.cmd == 'destroy' and not (a.name or a.all):
        die('destroy NAME | --all')
    a.fn(a)


if __name__ == '__main__':
    main()
