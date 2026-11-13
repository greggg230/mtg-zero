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
  python tools/vast.py run-push NAME RUN [--include X] [--leave Y] [--clear-setting S]   # upload what the run resumes
                                                         # from; desktop copy PAUSEd
  python tools/vast.py run-start NAME RUN [--checkout DIR] [-- extra run args]   # supervise.py on the box
  python tools/vast.py run-sync NAME RUN [--loop SEC] [--no-data]                # box outputs -> desktop mirror
  python tools/vast.py run-ctl NAME RUN stop|pause|resume|tail
  python tools/vast.py run-home NAME RUN [--wait-min M]  # STOP on the box, confirmed stop, full pull, checked, desktop
                                                         # copy runnable again

Box names are short words ('vast' is the one the run setting selfplay_remote 'vast' reaches: ssh alias
mtgz-vast, unless MTGZ_VAST_HOST names another alias). The ledger is ~/.config/vastai/mtgz_boxes.json; the ssh
aliases are a managed block per box in ~/.ssh/config (what zero-core's ssh transport and git use). Costs: the box
bills from creation until it is destroyed (GPU + disk), so `destroy` when done and check `status`.

While a run lives on a box, its desktop dir holds ON_VAST.json (which box, since when, the settings run-start
changed) and a PAUSE file by 'tools/vast.py' (a desktop supervisor started by mistake waits instead of forking the
run). run-home brings the newest state back, restores those settings, removes both and leaves STOP (relaunch on
the desktop as usual: the supervisor moves STOP away).

docs/ENGINE.md 143 (the Fable review's fixes): a file is pulled by content, never by size alone (vast_pulled.json
holds the box's listing entry of what the desktop has; same-size files of another version are compared by sha256);
an ssh failure is "unknown", never "the box run stopped"; run-home confirms the stop (no supervisor, no run process,
supervisor state 'done') before its final pull and checks the optimizer state and best.pt after it; run-push refuses
a run entry or a desktop-path setting it does not know; each box's host key is pinned at its first connection.
"""
import argparse
import ast
import base64
import ctypes
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
# A process without a console window (a `run-sync --loop` whose launcher has exited) would give every ssh / git
# child its own visible terminal window, each one stealing keyboard focus (5 per sync pass, 2026-11-05). Those
# children get a hidden console instead; a process that has a console window keeps sharing it with them.
NO_WINDOW = (subprocess.CREATE_NO_WINDOW
             if os.name == 'nt' and not ctypes.windll.kernel32.GetConsoleWindow() else 0)
sys.path.insert(0, HERE)
import vast_provider as vp  # noqa: E402


def _load_checkout():
    """mtgz/checkout.py by its path (standard library only; no package import, which needs torch)."""
    import importlib.util
    spec = importlib.util.spec_from_file_location('_mtgz_checkout', os.path.join(ROOT, 'mtgz', 'checkout.py'))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


checkout = _load_checkout()

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
PULLED = 'vast_pulled.json'   # the box's (size, mtime) of every file the desktop copy holds as the box's (sync_once)
PAUSE_BY = 'tools/vast.py'
# settings run-start changes for the box (no Mac from there; nothing remote) and run-home restores
BOX_SETTINGS = {'eval_remote': '', 'selfplay_remote': '', 'selfplay_remote_procs': 0, 'selfplay_remote_linger': 0,
                'selfplay_local_linger': 0}
PATH_SETTINGS = ('elo_anchors', 'heldout_shards')
APPEND = ('metrics.jsonl', 'evals.jsonl', 'log.txt', 'stdout.txt', 'supervisor.log', 'supervisor_console.txt')
NO_PULL = ('config.json', 'STOP', 'PAUSE', 'STOP.old', ON_VAST, 'selfplay_remotes', 'supervisor_state.json', PULLED)
# what run-push uploads from a run directory (the run resumes from these), and what stays on the desktop by design
# (control files, this tool's own files, logs, the publishers' output); any other entry is refused until it is named
# with --include (uploaded) or --leave (stays) - docs/ENGINE.md 143
PUSH_FILES = ('run_state.json', 'metrics.jsonl', 'evals.jsonl', 'log.txt', 'stdout.txt', 'supervisor.log',
              'supervisor_console.txt', 'progress.json')
PUSH_DIRS = ('elo', 'data', 'models')
DESKTOP_ONLY = ('STOP', 'STOP.old', 'PAUSE', 'supervisor_state.json', 'vast_sync.json', PULLED, 'selfplay_remotes',
                '.remote_config.json', 'delverai.json', 'replays', 'debug')
DESKTOP_ONLY_RE = re.compile(r'ON_VAST(_\d+)?\.json|.*\.(txt|log|tmp|vasttmp)')
# a Windows absolute path inside a setting's value (a desktop path the box does not have)
DESKTOP_PATH_RE = re.compile(r'(?<![A-Za-z])[A-Za-z]:[\\/]|\\\\[^\\]')
KNOWN_HOSTS_DIR = os.path.expanduser('~/.ssh')


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


def known_hosts(name):
    """The box's own known_hosts file: its host key, pinned at the first connection (StrictHostKeyChecking
    accept-new); a later connection that meets another key is refused. A rented box is third-party hardware."""
    return os.path.join(KNOWN_HOSTS_DIR, f'known_hosts_{alias(name)}')


def write_alias(name, host, port):
    a = alias(name)
    try:
        os.remove(known_hosts(name))                    # a new box under this name: its key is learnt afresh
    except OSError:
        pass
    block = (f'# >>> {a} (tools/vast.py) >>>\nHost {a}\n  HostName {host}\n  Port {port}\n  User root\n'
             f'  IdentityFile ~/.ssh/id_ed25519\n  StrictHostKeyChecking accept-new\n'
             f'  UserKnownHostsFile ~/.ssh/known_hosts_{a}\n'
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
    try:
        os.remove(known_hosts(name))
    except OSError:
        pass


def ssh(name, cmd, input=None, check=True, timeout=600, capture=True):
    """Run `cmd` (a bash script) on box `name` -> CompletedProcess (stdout / stderr decoded). `input` goes as
    UTF-8 bytes: a text-mode pipe on Windows would turn its newlines into CRLF (bash: "pipefail\\r")."""
    r = subprocess.run(['ssh', '-o', 'BatchMode=yes', alias(name), cmd],
                       input=input.encode('utf-8') if input is not None else None,
                       capture_output=capture, timeout=timeout, stdin=None if input is not None else subprocess.DEVNULL,
                       creationflags=NO_WINDOW)
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
    the desktop never reaches destroys itself at `deadline`. It runs at every start of the container: a deadline
    file already there (`extend`'s, raised or lowered) is kept - rewriting the rent's deadline after a restart
    fired the guard at once on a box whose deadline had been extended."""
    with open(os.path.join(HERE, 'vast_guard.py'), encoding='utf-8') as f:
        tree = ast.parse(f.read())
    for node in ast.walk(tree):                          # docstrings out: the start script's size is limited
        body = getattr(node, 'body', None)
        if isinstance(body, list) and body and isinstance(body[0], ast.Expr) \
                and isinstance(getattr(body[0], 'value', None), ast.Constant) and isinstance(body[0].value.value, str):
            node.body = body[1:] or [ast.Pass()]
    b64 = base64.b64encode(zlib.compress(ast.unparse(tree).encode(), 9)).decode()
    unpack = "import zlib,base64,sys;open(sys.argv[1],'wb').write(zlib.decompress(base64.b64decode(sys.argv[2])))"
    return (f'G={GUARD_DIR}; mkdir -p "$G"; [ -s "$G/deadline" ] || echo {int(deadline + GUARD_MARGIN_S)} > "$G/deadline"; '
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
    return dict(FILTERS, max_dph=a.max_dph, min_ram_gb=a.min_ram, min_cores=a.min_cores, num_gpus=a.gpus)


def each_name(fn):
    """A command over several boxes (`rent w1 w2 w3`): `fn` once per name, in turn."""
    def run(a):
        for n in a.names:
            fn(argparse.Namespace(**dict(vars(a), name=n)))
    return run


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
            # (a non-zero exit: a script renting boxes must not carry on as if the box were protected)
            die(f'box {a.name!r} (instance {iid}) is UP and BILLING, but its deadline guard is not confirmed: only '
                f'`destroy` protects against a forgotten box now. `python tools/vast.py destroy {a.name}`, or check '
                f'{GUARD_DIR} on it (`python tools/vast.py ssh {a.name}`) and carry on knowingly')
        return
    die(f'no offer could be rented: {tried}')


def cmd_status(a):
    p = vp.VastCLI()
    led = load_ledger()
    live = {i['id']: i for i in p.list()}
    now = time.time()
    total = 0.0
    late = []
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
        # (the box's guard fires GUARD_MARGIN_S after the ledger's deadline: past that, and 5 minutes more, it failed)
        if e.get('state') != 'destroyed' and inst is not None and left * 3600 < -(GUARD_MARGIN_S + 300):
            late.append(name)
            print(f'  WARNING: {name} is PAST its deadline ({-left:.1f} h ago) and still listed (billing): its guard did '
                  f'not destroy it - `python tools/vast.py destroy {name}`')
    for i in live.values():
        print(f"(not in the ledger) iid {i['id']} {i.get('status')} {i.get('label')} ${i.get('dph', 0):.3f}/h")
    print(f'ledger spend ~${total:.2f}; balance ${p.balance():.2f}')
    if late:
        sys.exit(1)                                       # (a scheduled `status` sees it as a failure)


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
    return subprocess.run(['git', '-C', root, *args], capture_output=True, text=True, check=True,
                          creationflags=NO_WINDOW).stdout.strip()


def box_repo(sha):
    return f'{BOX_HOME}/mtg-zero-wt/run-{sha[:12]}'      # = mtgz.az.mac.eval_checkout(sha, BOX_HOME)


def cmd_prep_many(a):
    """`prep` on several boxes at once: one `vast.py prep <box>` process each (logs ~/.config/vastai/prep-<box>.log),
    waited for together; fails if any failed."""
    if len(a.names) == 1:
        return cmd_prep(argparse.Namespace(**dict(vars(a), name=a.names[0])))
    procs = {}
    for n in a.names:
        box(n)
        args = [sys.executable, os.path.abspath(__file__), 'prep', n] + (['--checkout', a.checkout] if a.checkout else []) \
            + (['--gate', a.gate] if a.gate else [])
        logf = os.path.join(os.path.dirname(LEDGER), f'prep-{n}.log')
        procs[n] = (subprocess.Popen(args, stdout=open(logf, 'w', encoding='utf-8'), stderr=subprocess.STDOUT,
                                     stdin=subprocess.DEVNULL, env=dict(os.environ, PYTHONIOENCODING='utf-8'),
                                     creationflags=NO_WINDOW), logf)
        say(f'prep {n}: started (log {logf})')
    bad = []
    for n, (p, logf) in procs.items():
        rc = p.wait()
        with open(logf, encoding='utf-8', errors='replace') as f:
            last = [x for x in f.read().splitlines() if x.strip()][-1:] or ['']
        say(f'prep {n}: exit {rc}: {last[0][:200]}')
        if rc:
            bad.append(n)
    if bad:
        die(f'prep failed on {bad} (their logs above)')


def cmd_link(a):
    """Let the box TRAINER (whose run reaches its workers over ssh) log in to each WORKER box: a key of the
    trainer's own (made there once; the desktop's key never goes to a rented box) added to the workers'
    authorized_keys, and a `Host mtgz-<worker>` block per worker in the trainer's ~/.ssh/config (the same alias
    the run's remote 'vast:<worker>' uses). Checked by an ssh from the trainer to each worker."""
    box(a.trainer)
    pub = ssh(a.trainer, '[ -f ~/.ssh/id_ed25519 ] || ssh-keygen -q -t ed25519 -N "" -f ~/.ssh/id_ed25519; '
                         'cat ~/.ssh/id_ed25519.pub').stdout.strip().splitlines()[-1]
    if not pub.startswith('ssh-'):
        die(f'no public key from {a.trainer}: {pub[:100]}')
    blocks = []
    p = vp.VastCLI()
    for w in a.workers:
        e = box(w)
        r = e.get('ssh') or {}
        if not r:
            die(f'box {w!r} has no ssh route in the ledger')
        # through Vast, never by editing the worker's authorized_keys: Vast rewrites that file, and a hand edit
        # locked a worker out (even the desktop's key) until its key was re-attached (2026-11-05)
        p.attach_ssh(e['iid'], pub)
        blocks.append(f'# >>> {alias(w)} (tools/vast.py) >>>\nHost {alias(w)}\n  HostName {r["host"]}\n  Port {r["port"]}\n'
                      f'  User root\n  StrictHostKeyChecking accept-new\n  UserKnownHostsFile ~/.ssh/known_hosts_{alias(w)}\n'
                      f'  LogLevel ERROR\n'
                      f'  ServerAliveInterval 15\n  ServerAliveCountMax 4\n  ConnectTimeout 20\n# <<< {alias(w)} <<<\n')
    # the trainer's ssh config: our blocks replaced, the rest kept
    script = ('import re, os, sys\np = os.path.expanduser("~/.ssh/config")\n'
              's = open(p).read() if os.path.exists(p) else ""\n'
              'for a in sys.argv[1:]:\n'
              '    s = re.sub(r"# >>> " + re.escape(a) + r" \\(tools/vast\\.py\\) >>>\\n.*?# <<< " + re.escape(a) + r" <<<\\n", "", s, flags=re.S)\n'
              '    kh = os.path.expanduser("~/.ssh/known_hosts_" + a)\n'
              '    os.path.exists(kh) and os.remove(kh)\n'
              'blocks = sys.stdin.read()\nopen(p, "w").write(blocks + s)\nos.chmod(p, 0o600)\n')
    ssh(a.trainer, f'{BOX_PY} -c {shlex.quote(script)} {" ".join(alias(w) for w in a.workers)}', input=''.join(blocks))
    for w in a.workers:
        for _ in range(12):                                # Vast applies an attached key within seconds
            r = ssh(a.trainer, f'ssh -o BatchMode=yes {alias(w)} hostname', check=False, timeout=90)
            if r.returncode == 0:
                break
            time.sleep(10)
        say(f'{a.trainer} -> {w}: ' + ('ok (' + r.stdout.strip() + ')' if r.returncode == 0 else
                                       f'FAILED: {(r.stderr or r.stdout).strip()[-200:]}'))


def workers_file(run):
    """(where, path): the run's selfplay_remotes file - on its box while the run lives there (ON_VAST.json),
    else in the desktop dir."""
    try:
        with open(os.path.join(run, ON_VAST), encoding='utf-8') as f:
            m = json.load(f)
        return m['box'], m['remote_dir'] + '/selfplay_remotes'
    except (OSError, ValueError, KeyError):
        return None, os.path.join(run, 'selfplay_remotes')


def cmd_workers(a):
    """The run's self-play workers: zero-core's selfplay_remotes file (one remote name per line: 'vast:w1' = box w1,
    'gpu:1' = this machine's GPU 1, 'mac'), re-read by the run at every generation's start (docs/ENGINE.md 97.2).
    show | set NAMES | add NAMES | remove NAMES | clear."""
    run = os.path.abspath(a.run)
    where, path = workers_file(run)
    if where:
        cur = ssh(where, f'cat {shlex.quote(path)} 2>/dev/null || true').stdout
    else:
        cur = open(path, encoding='utf-8').read() if os.path.exists(path) else ''
    names = [x.split('#')[0].strip() for x in cur.splitlines()]
    names = [x for x in names if x]
    if a.what == 'show':
        print(f'{where + ":" if where else ""}{path}: {names or "(none: no file or empty)"}')
        return
    for n in a.names if a.what in ('set', 'add') else []:    # (remove: a destroyed box's name must still go)
        if n.startswith('vast:'):
            box(n.split(':', 1)[1])                      # a box we rented (ledger)
        elif not (n.startswith('gpu:') and n[4:].isdigit()) and n not in ('mac', 'vast', 'local'):
            die(f'{n!r}: a remote name (vast:<box>, gpu:<N>, mac, vast)')
    if a.what == 'set':
        new = list(dict.fromkeys(a.names))
    elif a.what == 'add':
        new = names + [n for n in a.names if n not in names]
    elif a.what == 'remove':
        new = [n for n in names if n not in a.names]
    else:
        new = []
    body = f'# self-play workers of this run (tools/vast.py workers, {time.strftime("%Y-%m-%d %H:%M:%S")})\n' + \
        ''.join(n + '\n' for n in new)
    if where:
        ssh(where, f'cat > {shlex.quote(path)}.tmp && mv {shlex.quote(path)}.tmp {shlex.quote(path)}', input=body)
    else:
        with open(path + '.tmp', 'w', encoding='utf-8', newline='\n') as f:
            f.write(body)
        os.replace(path + '.tmp', path)
    say(f'workers of {os.path.basename(run)}: {names} -> {new} (applies from the next generation)')


def cmd_prep(a):
    box(a.name)
    co = os.path.abspath(a.checkout or ROOT)
    sha = git(co, 'rev-parse', 'HEAD')
    pin = git(co, 'rev-parse', 'HEAD:third_party/zero-core')
    dirty = checkout.changes(co)
    if dirty:
        die(f'{co} is not exactly its commit ({"; ".join(d.strip() for d in dirty[:6])}): the box builds the commit, '
            f'so an uncommitted change or an untracked file the desktop run would import is not there (commit first)')
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
                   check=True, creationflags=NO_WINDOW)
    subprocess.run(['git', '-C', os.path.join(co, 'third_party', 'zero-core'), 'push', '-q', '-f',
                    f'{alias(a.name)}:mtg-zero-core.git', f'{pin}:refs/heads/pin-{pin[:12]}'], check=True,
                   creationflags=NO_WINDOW)
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
                            stdin=subprocess.PIPE, creationflags=NO_WINDOW)
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


def unknown_entries(run, include=(), leave=()):
    """The run directory's top-level entries that run-push neither uploads nor leaves on the desktop by design (nor
    the caller named with --include / --leave): a co-training run's resolver/, an EoE run's games/ and evaluators/,
    an archive, ... - the box run would start without them, silently."""
    out = []
    for n in sorted(os.listdir(run)):
        if (n == 'config.json' or n in PUSH_FILES or n in PUSH_DIRS or n in DESKTOP_ONLY or DESKTOP_ONLY_RE.fullmatch(n)
                or n in include or n in leave):
            continue
        out.append(n + ('/' if os.path.isdir(os.path.join(run, n)) else ''))
    return out


def desktop_path_settings(cfg, clear=()):
    """The settings whose value holds a desktop path (a drive letter, a UNC path; inside a spec too) that run-push
    neither remaps (PATH_SETTINGS) nor clears (--clear-setting): the box has no such file."""
    return sorted(k for k, v in cfg.items() if k not in PATH_SETTINGS and k not in clear
                  and DESKTOP_PATH_RE.search(v if isinstance(v, str) else json.dumps(v)))


def write_marker(run, m):
    """ON_VAST.json, atomically (a sync loop reads it every pass: a half-written marker is never seen)."""
    path = os.path.join(run, ON_VAST)
    with open(path + '.vasttmp', 'w', encoding='utf-8') as f:
        json.dump(m, f, indent=1)
    os.replace(path + '.vasttmp', path)


def load_pulled(run):
    """{path: [size, mtime]}: the box's listing entry of each file the desktop copy holds as the box's (written by
    run-push and every sync). A missing or unreadable manifest is empty: content hashes decide then (sync_once)."""
    try:
        with open(os.path.join(run, PULLED), encoding='utf-8') as f:
            d = json.load(f)
        return d if isinstance(d, dict) else {}
    except (OSError, ValueError):
        return {}


def save_pulled(run, d):
    path = os.path.join(run, PULLED)
    with open(path + '.vasttmp', 'w', encoding='utf-8') as f:
        json.dump(d, f)
    os.replace(path + '.vasttmp', path)


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
    include, leave, clear = list(a.include or []), list(a.leave or []), list(a.clear_setting or [])
    unknown = unknown_entries(run, include, leave)
    if unknown:
        die(f'{run} holds entries run-push does not know: {", ".join(unknown)}. Name each one: --include NAME (uploaded '
            f'as it is: the box run needs it) or --leave NAME (stays on the desktop: the box run starts without it) - '
            f'docs/ENGINE.md 143')
    gone = [n for n in include if not os.path.exists(os.path.join(run, n))]
    if gone:
        die(f'--include {", ".join(gone)}: not in {run}')
    for s in clear:
        if s not in cfg:
            die(f'--clear-setting {s}: the run has no such setting')
    bad = desktop_path_settings(cfg, clear)
    if bad:
        die(f'settings holding a desktop path the box does not have: {", ".join(f"{s}={str(cfg[s])[:120]!r}" for s in bad)}. '
            f'--clear-setting NAME pushes it as \'\' (run-home restores it); anchors and held-out shards '
            f'({", ".join(PATH_SETTINGS)}) are uploaded and remapped')
    k = newest_iter(run)
    paths, ext = remap_paths(cfg)
    for p in ext:
        if not os.path.exists(p):
            die(f'{p} (a setting of the run) does not exist')
    box_cfg = dict(cfg, **{s: v[1] for s, v in paths.items()}, **BOX_SETTINGS, **{s: '' for s in clear})
    members = [('config.json', json.dumps(box_cfg, indent=1).encode())]
    for n in PUSH_FILES:
        if os.path.exists(os.path.join(run, n)):
            members.append((n, os.path.join(run, n)))
    for sub in ('elo', 'data') + tuple(include):
        full = os.path.join(run, sub)
        if os.path.isfile(full):
            members.append((sub, full))
            continue
        for dp, _dn, fns in os.walk(full):
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
        f'{size / 1e9:.2f} GB (+ {len(ext)} anchor / held-out files)' + (f'; left on the desktop: {", ".join(leave)}'
                                                                          if leave else ''))
    restore = {**{s: v[0] for s, v in paths.items()}, **{s: cfg.get(s) for s in BOX_SETTINGS},
               **{s: cfg[s] for s in clear}}
    marker = {'box': a.name, 'iid': e['iid'], 'alias': alias(a.name), 'remote_dir': box_run_dir(run),
              'since': time.time(), 'iter_at_push': k, 'restore': restore, 'absent': []}
    # the desktop copy is PAUSEd before the upload: nothing may start it while the box owns the run; a failed upload
    # puts both files back as they were (the desktop copy is still the run)
    pause = os.path.join(run, 'PAUSE')
    try:
        with open(pause, 'rb') as f:
            pause_before = f.read()
    except OSError:
        pause_before = None
    with open(pause, 'w', encoding='utf-8') as f:
        json.dump({'time': time.time(), 'by': PAUSE_BY}, f)
    write_marker(run, marker)
    t0 = time.time()
    try:
        # the desktop copy is the run now: an older copy on the box (an earlier push) could hold newer checkpoints
        # or data the run would resume from
        rd = box_run_dir(run)
        assert rd.startswith(BOX_RUNS + '/') and len(rd) > len(BOX_RUNS) + 1, rd
        ssh(a.name, f'rm -rf {shlex.quote(rd)}')
        tar_upload(a.name, f'{BOX_RUNS}/_ext', [(os.path.basename(p), p) for p in ext])
        tar_upload(a.name, box_run_dir(run), members)
        # the box's copy now equals the desktop's: its listing is what the desktop holds (sync_once fetches a file
        # only when the box's size or mtime moves away from it, or a content hash says so)
        save_pulled(run, {p: list(v) for p, v in remote_listing(a.name, rd).items()})
    except BaseException:
        os.remove(os.path.join(run, ON_VAST))
        if pause_before is None:
            os.remove(pause)
        else:
            with open(pause, 'wb') as f:
                f.write(pause_before)
        say(f'run-push failed: {ON_VAST} removed and PAUSE as it was - the desktop copy is still the run')
        raise
    say(f'uploaded in {time.time() - t0:.0f} s; desktop copy PAUSEd + {ON_VAST}; next: run-start')


def split_flags(args):
    """Command-line arguments with each `--name=value` split into `--name value` (both forms reach a run)."""
    out = []
    for x in args:
        if x.startswith('--') and '=' in x:
            out += x.split('=', 1)
        else:
            out.append(x)
    return out


def run_args(run, marker):
    """The desktop supervisor's run arguments for the box: remote settings off, desktop paths remapped."""
    with open(os.path.join(run, 'supervisor_state.json'), encoding='utf-8') as f:
        args = split_flags(json.load(f).get('args') or [])
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


def flag_settings(args):
    """The run settings named by command-line flags (`--threads 32`, `--games=2000` -> threads, games)."""
    return [x[2:].split('=', 1)[0].replace('-', '_') for x in args if x.startswith('--') and len(x) > 2]


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
    # the settings the extra flags change are saved into the box's config.json and come home with it (box thread
    # counts on the desktop): run-home restores the desktop's values (or drops a setting the desktop did not have)
    with open(os.path.join(run, 'config.json'), encoding='utf-8') as f:
        desk = json.load(f)
    restore, absent = marker.setdefault('restore', {}), marker.setdefault('absent', [])
    for s in flag_settings(a.extra or []):
        if s in restore or s in absent:
            continue
        if s in desk:
            restore[s] = desk[s]
        else:
            absent.append(s)
    marker.update(sha=sha, started=time.time(), args=args)
    write_marker(run, marker)
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


def member_path(run, name, asked):
    """Where the box's tar member `name` goes under the desktop dir `run` - refused (OSError: the pass fails) unless
    it is one of the relative paths `asked` for, with no absolute, drive or '..' part. A rented box is third-party
    hardware: its tar stream must not write outside the run, or anything the desktop did not ask for."""
    parts = name.split('/')
    if (name not in asked or name.startswith('/') or '\\' in name or ':' in name
            or any(x in ('', '.', '..') for x in parts)):
        raise OSError(f'the box sent a tar member {name!r} that was not asked for or leaves the run directory')
    dst = os.path.join(run, *parts)
    if os.path.commonpath([os.path.abspath(dst), os.path.abspath(run)]) != os.path.abspath(run):
        raise OSError(f'the box sent a tar member {name!r} outside the run directory')
    return dst


def tar_fetch(name, rd, run, paths, timeout_s=1800):
    """Fetch `paths` (relative) from the box's `rd` into `run`, each file written to a temporary name and
    renamed (readers never see half a file). The stream is read to its end: tarfile stops at the end-of-archive
    marker, and the record padding after it, left unread, kept ssh blocked on a full pipe forever (the main line's
    sync loop hung 2026-11-05 14:40). A transfer still running after `timeout_s` is killed (the pass fails, the
    next one retries)."""
    if not paths:
        return 0
    proc = subprocess.Popen(['ssh', '-o', 'BatchMode=yes', alias(name), f'cd {rd} && tar cf - --files-from=-'],
                            stdin=subprocess.PIPE, stdout=subprocess.PIPE, creationflags=NO_WINDOW)
    killer = threading.Timer(timeout_s, proc.kill)
    killer.daemon = True
    killer.start()
    n = 0
    asked = set(paths)
    try:
        proc.stdin.write(('\n'.join(paths) + '\n').encode())
        proc.stdin.close()
        with tarfile.open(fileobj=proc.stdout, mode='r|') as tf:
            for m in tf:
                if not m.isfile():
                    continue
                dst = member_path(run, m.name, asked)
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


def sha256_file(path):
    import hashlib
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()


def remote_hashes(name, rd, paths):
    """{path: sha256} of the box's files `paths` (relative to `rd`), one ssh session (paths on stdin)."""
    if not paths:
        return {}
    r = ssh(name, f"cd {rd} && tr '\\n' '\\0' | xargs -0 -r sha256sum --", input='\n'.join(paths) + '\n', timeout=900)
    out = {}
    for line in r.stdout.splitlines():
        h, _, p = line.partition('  ')
        if len(h) == 64 and p:
            out[p] = h
    missing = [p for p in paths if p not in out]
    if missing:
        die(f'sha256sum on {alias(name)}:{rd} gave no hash for {", ".join(missing[:5])}')
    return out


def append_tail(name, rd, p, local, have, size, k=64):
    """The bytes the box's append-only log `p` holds past the desktop's `have` -> bytes, or None when the desktop's
    last `k` bytes are not the box's at that offset (the desktop wrote into its copy too, e.g. a supervisor started by
    mistake logged "PAUSE file present": appending would splice two logs) or the read failed: fetch it whole then."""
    k = min(k, have)
    r = subprocess.run(['ssh', '-o', 'BatchMode=yes', alias(name), f'tail -c +{have - k + 1} {rd}/{p} | head -c {size - have + k}'],
                       capture_output=True, timeout=600, creationflags=NO_WINDOW)
    if r.returncode or len(r.stdout) != size - have + k:
        return None
    with open(local, 'rb') as f:
        f.seek(have - k)
        mine = f.read(k)
    return r.stdout[k:] if r.stdout[:k] == mine else None


def sync_once(name, run, rd, data=True, final=False):
    """Pull the box run's outputs into the desktop dir: new / changed files, append-only logs by their new
    bytes. Not pulled while the run lives on the box: config.json (box paths; run-home restores it), the
    control files; without `data` (or `final`), data/ and models/opt_latest.pt.

    "Changed" is decided by content, never by size alone (docs/ENGINE.md 143: the optimizer state and models/best.pt
    are rewritten at the same size every iteration, and run-home's full pull left the desktop's stale ones): a file is
    fetched when the desktop has none, or of another size, or when the box's (size, mtime) is not the one the
    manifest (vast_pulled.json, seeded by run-push from the box's listing) records for what the desktop holds and
    its sha256 differs from the desktop's. JSON files are always fetched (small, rewritten in place)."""
    remote = remote_listing(name, rd)
    pulled = load_pulled(run)
    now = time.time()
    fetch, appended, same_size = [], 0, []
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
            new = append_tail(name, rd, p, local, have, size)
            if new is not None:
                with open(local, 'ab') as f:
                    f.write(new)
                appended += len(new)
                pulled[p] = [size, mtime]
                continue
            fetch.append(p)
            continue
        if have != size or p.endswith('.json'):
            fetch.append(p)
        elif pulled.get(p) != [size, mtime]:
            same_size.append(p)                              # same size, a version the desktop may not hold
    if same_size:
        theirs = remote_hashes(name, rd, same_size)
        for p in same_size:
            if theirs[p] == sha256_file(os.path.join(run, *p.split('/'))):
                pulled[p] = list(remote[p])                  # the same content: nothing to fetch
            else:
                fetch.append(p)
    got = tar_fetch(name, rd, run, fetch)
    for p in fetch:
        pulled[p] = list(remote[p])
    save_pulled(run, {p: v for p, v in pulled.items() if p in remote})
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
    warned = False
    while True:
        t0 = time.time()
        if a.loop and homing(run):
            say(f'run-home has the run ({ON_VAST} gone or homing): sync loop ends')
            return
        data = not a.no_data
        # cost and data safety: the box destroys itself at its deadline with everything since the last pass on it
        dl = (load_ledger().get(a.name) or {}).get('deadline')
        if dl is not None and dl - time.time() < 3600:
            if not warned:
                say(f'WARNING: box {a.name!r} reaches its deadline in {(dl - time.time()) / 60:.0f} min (then it destroys '
                    f'itself): `python tools/vast.py extend {a.name} --hours H`, or run-home now; pulling the data '
                    f'window too from now on')
                warned = True
            data = True
        try:
            nf, nb, nr = sync_once(a.name, run, rd, data=data)
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


def homing(run, tries=5):
    """True once run-home has taken the run (its marker says so, or is gone): a sync loop stops pulling. A marker
    that is there but cannot be read (being replaced: Windows) is read again; still unreadable, it is not a reason
    to end the mirror (False)."""
    path = os.path.join(run, ON_VAST)
    for k in range(tries):
        if not os.path.exists(path):
            return True
        try:
            with open(path, encoding='utf-8') as f:
                return bool(json.load(f).get('homing'))
        except (OSError, ValueError):
            if k + 1 < tries:
                time.sleep(0.2)
    return False


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


def box_run_status(name, rd):
    """The box run, from one ssh session -> {'supervisor': its supervise.py is running, 'run': how many mtgz.az.run
    processes run there, 'state': its supervisor_state.json's state (None: no such file, or unreadable)} - or None
    when that cannot be told (ssh failed, or the answer is not this script's). Unknown is never "stopped": reading a
    dropped link as a stopped run made run-home pull a live run and unpause the desktop copy (two copies training;
    docs/ENGINE.md 143)."""
    # the patterns are anchored to the processes' own command lines (python tools/supervise.py ..., python -m
    # mtgz.az.run ...): an unanchored one also matches the `bash -c` that runs pgrep, whose command line holds it
    sh = ("S=no; pgrep -f '^[^ ]*python[0-9.]* tools/supervise[.]py ' >/dev/null && S=yes; "
          "R=$(pgrep -fc '^[^ ]*python[0-9.]* -m mtgz[.]az[.]run ' || true); "
          f'echo "@@status $S ${{R:-0}}"; cat {rd}/supervisor_state.json 2>/dev/null || true')
    try:
        r = ssh(name, sh, check=False, timeout=60)
    except subprocess.TimeoutExpired:
        return None
    if r.returncode:
        return None
    lines = r.stdout.splitlines()
    head = [i for i, x in enumerate(lines) if x.startswith('@@status ')]
    if len(head) != 1:
        return None
    parts = lines[head[0]].split()
    if len(parts) != 3 or parts[1] not in ('yes', 'no') or not parts[2].isdigit():
        return None
    try:
        state = json.loads('\n'.join(lines[head[0] + 1:]) or 'null')
    except ValueError:
        state = None
    return {'supervisor': parts[1] == 'yes', 'run': int(parts[2]),
            'state': state.get('state') if isinstance(state, dict) else None}


def known_status(name, rd, tries, wait_s=15):
    """box_run_status, asked up to `tries` times while it is unknown -> the status, or None."""
    for k in range(tries):
        st = box_run_status(name, rd)
        if st is not None:
            return st
        say(f'cannot tell the box run\'s state (ssh to {alias(name)} failed: {k + 1}/{tries})')
        if k + 1 < tries:
            time.sleep(wait_s)
    return None


def verify_home(name, run, rd, allow_opt_mismatch=False):
    """After run-home's final pull: the desktop's optimizer state and models/best.pt are the box's (sha256 on both
    sides), the optimizer state is of the newest checkpoint, and best.pt is the checkpoint index.json names - or
    SystemExit (the run is not declared home). -> what was checked, for the log."""
    mdir = os.path.join(run, 'models')
    listing = remote_listing(name, rd)
    names = [p for p in ('models/opt_latest.pt', 'models/best.pt')
             if p in listing and os.path.exists(os.path.join(run, *p.split('/')))]
    theirs = remote_hashes(name, rd, names)
    for p in names:
        if theirs[p] != sha256_file(os.path.join(run, *p.split('/'))):
            die(f'after the final pull the desktop\'s {p} is not the box\'s (sha256 differs): not declared home')
    k = newest_iter(run)
    out = [f'{p} = the box\'s' for p in names]
    opt = os.path.join(mdir, 'opt_latest.pt')
    if os.path.exists(opt):
        import torch
        it = torch.load(opt, map_location='cpu', weights_only=False).get('iter')
        if it != k and not allow_opt_mismatch:
            die(f'the optimizer state is of iteration {it}, the newest checkpoint is {k}: the desktop would resume with a '
                f'fresh optimizer. Not declared home (--allow-opt-mismatch: knowingly)')
        out.append(f'optimizer state of iteration {it}' + ('' if it == k else f' (newest checkpoint {k}: ALLOWED)'))
    try:
        with open(os.path.join(mdir, 'index.json'), encoding='utf-8') as f:
            best = (json.load(f).get('best') or {}).get('file')
    except (OSError, ValueError):
        best = None
    if best and os.path.exists(os.path.join(mdir, 'best.pt')) and os.path.exists(os.path.join(mdir, best)):
        if sha256_file(os.path.join(mdir, 'best.pt')) != sha256_file(os.path.join(mdir, best)):
            die(f'models/best.pt is not {best}, the checkpoint models/index.json names best: not declared home')
        out.append(f'best.pt = {best}')
    return out


def cmd_run_home(a):
    box(a.name)
    run = os.path.abspath(a.run)
    m = read_marker(run, a.name)
    rd = m['remote_dir']
    st = known_status(a.name, rd, a.tries)
    if st is None:
        die(f'cannot reach {alias(a.name)}: whether the box run has stopped is unknown, and unknown is not stopped. '
            f'Nothing changed on the desktop (a sync loop keeps mirroring); run-home again when the box answers')
    if st['supervisor']:
        ssh(a.name, f'echo \'{{"time": {time.time()}, "by": "{PAUSE_BY}"}}\' > {rd}/STOP')
        say(f'STOP on the box; waiting up to {a.wait_min} min for the run to end its iteration (a sync loop of the run '
            f'keeps mirroring meanwhile)')
        t0 = time.time()
        while True:
            st = box_run_status(a.name, rd)
            if st is None:
                say('cannot tell the box run\'s state (ssh failed): waiting on (unknown is not stopped)')
            elif not st['supervisor']:
                break
            if time.time() - t0 > a.wait_min * 60:
                die(f'the box run did not stop in time: nothing changed on the desktop and a sync loop keeps mirroring; '
                    f'the box has STOP (its run ends after its iteration) - run-home again later')
            time.sleep(15)
    # the stop, positively: no supervisor, no run process, and the supervisor's own last word 'done' (a clean exit:
    # finished or STOP) or 'refused'
    if st['run']:
        die(f'the box\'s supervisor is gone but {st["run"]} mtgz.az.run process(es) still run there: not pulled '
            f'(`python tools/vast.py run-ctl {a.name} {a.run} tail`)')
    if st['state'] not in ('done', 'refused') and not a.assume_stopped:
        die(f'the box\'s supervisor_state.json says {st["state"]!r}, not \'done\': its run did not end cleanly (a crash, a '
            f'kill, never started?). Look first (`python tools/vast.py run-ctl {a.name} {a.run} tail`); --assume-stopped '
            f'pulls it as it is (no supervisor and no run process run there now)')
    m['homing'] = time.time()                              # a sync loop of the run ends at its next pass
    write_marker(run, m)
    try:
        wait_sync_loop(run)
        say(f'box run stopped (supervisor state {st["state"]!r}): full pull (data window, optimizer, checkpoints, logs)')
        t0 = time.time()
        nf, nb, nr = sync_once(a.name, run, rd, data=True, final=True)
        say(f'pulled {nf} files + logs, {nb / 1e9:.2f} GB in {time.time() - t0:.0f} s')
        say('checked: ' + '; '.join(verify_home(a.name, run, rd, a.allow_opt_mismatch)))
    except BaseException:
        m.pop('homing', None)
        write_marker(run, m)
        say(f'run-home failed after the sync loop was told to end: {ON_VAST} no longer says homing - the run is still on '
            f'the box (its STOP is there); restart `run-sync --loop` to keep mirroring, or run-home again')
        raise
    cfg_path = os.path.join(run, 'config.json')
    with open(cfg_path, encoding='utf-8') as f:
        cfg = json.load(f)
    cfg.update(m['restore'])                                # the desktop's paths and remote settings
    for s in m.get('absent') or []:                         # (settings run-start's flags added: the desktop had none)
        cfg.pop(s, None)
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
    if os.path.exists(os.path.join(run, PULLED)):
        os.remove(os.path.join(run, PULLED))              # (the box's listing: nothing once the run is home)
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
        p.add_argument('--gpus', type=int, default=1, help='GPUs per box (2: one box, trainer on GPU 0 + gpu:1)')

    p = sub.add_parser('offers')
    filt(p)
    p.set_defaults(fn=cmd_offers)
    p = sub.add_parser('rent')
    p.add_argument('names', nargs='+', metavar='name')
    p.add_argument('--offer', type=int)
    filt(p)
    p.set_defaults(fn=each_name(cmd_rent))
    p = sub.add_parser('prep')
    p.add_argument('names', nargs='+', metavar='name')
    p.add_argument('--checkout')
    p.add_argument('--gate', metavar='DIGEST', help='run tools/gate.py repin on the box, expecting this engine digest')
    p.set_defaults(fn=cmd_prep_many)
    p = sub.add_parser('link', help='let box TRAINER ssh to the WORKER boxes (its run reaches them as vast:<worker>)')
    p.add_argument('trainer')
    p.add_argument('workers', nargs='+')
    p.set_defaults(fn=cmd_link)
    p = sub.add_parser('workers', help="a run's self-play workers (its selfplay_remotes file, re-read every generation)")
    p.add_argument('run')
    p.add_argument('what', choices=('show', 'set', 'add', 'remove', 'clear'))
    p.add_argument('names', nargs='*', help='remote names: vast:<box>, gpu:<N>, mac, vast')
    p.set_defaults(fn=cmd_workers)
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
    p.add_argument('--include', action='append', default=[], metavar='NAME',
                   help='upload this entry of the run directory as it is (one the push does not know); may repeat')
    p.add_argument('--leave', action='append', default=[], metavar='NAME',
                   help='leave this entry on the desktop (the box run starts without it); may repeat')
    p.add_argument('--clear-setting', action='append', default=[], metavar='SETTING',
                   help="push this setting (one holding a desktop path) as '' (run-home restores it); may repeat")
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
    p.add_argument('--tries', type=int, default=4, help='ssh attempts (15 s apart) before an unreachable box ends it')
    p.add_argument('--assume-stopped', action='store_true', help="pull although the box's supervisor_state.json is not "
                   "'done' (still only with no supervisor and no run process on the box)")
    p.add_argument('--allow-opt-mismatch', action='store_true', help='declare the run home although its optimizer '
                   'state is not of its newest checkpoint')
    p.set_defaults(fn=cmd_run_home)
    a = ap.parse_args()
    if a.cmd == 'destroy' and not (a.name or a.all):
        die('destroy NAME | --all')
    a.fn(a)


if __name__ == '__main__':
    main()
