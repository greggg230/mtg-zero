"""Vast.ai provider for tools/vast.py (copied from dominion-zero domz/az/vast.py at 6048187, without its
MockProvider): offers, renting, status, destroying through the `vastai` CLI (--raw JSON), key from
~/.config/vastai/vast_api_key or VAST_API_KEY. Every offer is checked again here against the filters after the
provider's own search (`offer_problems`), so a provider-side query mistake cannot rent a machine we did not ask for.
"""
import json
import os
import re
import shutil
import subprocess
import sys
import threading
import time

KEY_FILE = os.path.expanduser('~/.config/vastai/vast_api_key')
LABEL_PREFIX = 'mtgz-'                 # every instance this code creates carries a label starting so

# offer filters (experiment spec [filters] overrides any of them)
FILTERS = dict(
    gpu_name='RTX 5090', num_gpus=1, min_cores=16, min_ram_gb=48, min_reliability=0.98, min_disk_gb=80,
    min_inet_down=200, min_inet_up=100, max_dph=1.5, min_cuda=12.8, verified=True, min_direct_ports=1,
    min_hours_left=12.0, cpu_regex='', exclude_machines=[], exclude_hosts=[], geolocation='',
)


class ProviderError(Exception):
    """A provider call failed. `status`: the HTTP status the API answered (None: no answer)."""

    def __init__(self, msg, status=None):
        super().__init__(msg)
        self.status = status


class NoOffer(ProviderError):
    """No offer could be rented (every one tried failed or was taken): nothing was created.
    `tried`: {offer id: error}."""

    def __init__(self, msg, tried=None):
        super().__init__(msg)
        self.tried = dict(tried or {})


# ------------------------------------------------------------------ offers

def _f(x, d=0.0):
    try:
        return float(x)
    except (TypeError, ValueError):
        return d


def normalize_offer(r):
    """A provider offer -> the fields the harness uses (Vast's raw names in comments)."""
    ram = _f(r.get('cpu_ram'))
    return {
        'id': int(r['id']), 'machine_id': r.get('machine_id'), 'host_id': r.get('host_id'),
        'gpu': r.get('gpu_name', ''), 'num_gpus': int(_f(r.get('num_gpus'), 1)),
        'cores': _f(r.get('cpu_cores_effective')), 'cpu': (r.get('cpu_name') or '').replace('®', ''),
        'ram_gb': ram / 1024 if ram > 2048 else ram,                  # cpu_ram: MB in the raw offer
        'disk_gb': _f(r.get('disk_space')), 'reliability': _f(r.get('reliability2', r.get('reliability'))),
        'dph': _f(r.get('dph_total')), 'dph_base': _f(r.get('dph_base', r.get('dph_total'))),
        'storage_month_gb': _f(r.get('storage_cost')),                # $/GB/month
        'down_cost_gb': _f(r.get('inet_down_cost')), 'up_cost_gb': _f(r.get('inet_up_cost')),    # $/GB
        'inet_down': _f(r.get('inet_down')), 'inet_up': _f(r.get('inet_up')),                    # Mb/s
        'cuda': _f(r.get('cuda_max_good')), 'verified': r.get('verification', 'verified' if r.get('verified') else '') == 'verified'
        or r.get('verified') is True, 'direct_ports': int(_f(r.get('direct_port_count'))),
        'geo': r.get('geolocation') or '', 'end_date': _f(r.get('end_date')) or None,
        'rentable': r.get('rentable', True) and not r.get('rented', False),
        'driver': r.get('driver_version', ''), 'dlperf': _f(r.get('dlperf')),
    }


def offer_query(f, disk_gb=None):
    """The `vastai search offers` query for the filters (the provider pre-filter)."""
    q = ([f'gpu_name={f["gpu_name"].replace(" ", "_")}'] if f.get('gpu_name') else []) + [f'num_gpus={f["num_gpus"]}',
         f'cpu_cores_effective>={f["min_cores"]}', f'cpu_ram>={f["min_ram_gb"]}',
         f'reliability>={f["min_reliability"]}', f'disk_space>={max(f["min_disk_gb"], disk_gb or 0)}',
         f'inet_down>={f["min_inet_down"]}', f'inet_up>={f["min_inet_up"]}', f'dph_total<={f["max_dph"]}',
         f'cuda_max_good>={f["min_cuda"]}', 'rentable=true', f'direct_port_count>={f["min_direct_ports"]}']
    if f.get('verified'):
        q.append('verified=true')
    if f.get('geolocation'):
        q.append(f'geolocation={f["geolocation"]}')
    return ' '.join(q)


def offer_problems(o, f, now=None, need_hours=0.0):
    """Why an offer does not meet the filters (empty = it does)."""
    now = now or time.time()
    p = []
    if f['gpu_name'].lower() not in o['gpu'].lower():
        p.append(f'gpu {o["gpu"]}')
    if o['num_gpus'] != f['num_gpus']:
        p.append(f'{o["num_gpus"]} gpus')
    if o['cores'] < f['min_cores']:
        p.append(f'{o["cores"]:.1f} cores')
    if o['ram_gb'] < f['min_ram_gb'] * 0.97:
        p.append(f'{o["ram_gb"]:.0f} GB RAM')
    if o['reliability'] < f['min_reliability']:
        p.append(f'reliability {o["reliability"]:.3f}')
    if o['disk_gb'] < f['min_disk_gb']:
        p.append(f'{o["disk_gb"]:.0f} GB disk')
    if o['inet_down'] < f['min_inet_down'] or o['inet_up'] < f['min_inet_up']:
        p.append(f'inet {o["inet_down"]:.0f}/{o["inet_up"]:.0f} Mb/s')
    if o['dph'] > f['max_dph']:
        p.append(f'${o["dph"]:.3f}/h')
    if o['cuda'] and o['cuda'] < f['min_cuda']:
        p.append(f'cuda {o["cuda"]}')
    if f.get('verified') and not o['verified']:
        p.append('unverified')
    if o['direct_ports'] < f['min_direct_ports']:
        p.append('no direct ports')
    if not o['rentable']:
        p.append('not rentable')
    hours = max(f.get('min_hours_left', 0.0), need_hours)
    if o['end_date'] and o['end_date'] - now < hours * 3600:
        p.append(f'offer ends in {(o["end_date"] - now) / 3600:.1f} h')
    if f.get('cpu_regex') and not re.search(f['cpu_regex'], o['cpu'], re.I):
        p.append(f'cpu {o["cpu"]}')
    if o['machine_id'] in (f.get('exclude_machines') or []) or o['host_id'] in (f.get('exclude_hosts') or []):
        p.append('excluded machine / host')
    return p


def hourly(o, disk_gb):
    """$/h of an offer with `disk_gb` of disk: the GPU price + storage (billed from creation)."""
    return o['dph_base'] + o['storage_month_gb'] * disk_gb / (30 * 24)


def cost(o, hours, disk_gb, gb_down=0.0, gb_up=0.0):
    """$ for `hours` of an instance from offer o plus traffic (GB into / out of the box)."""
    return hourly(o, disk_gb) * hours + o['down_cost_gb'] * gb_down + o['up_cost_gb'] * gb_up


def machine_type(o):
    """Offers of the same machine type: same GPU, CPU model and effective core count."""
    return (o['gpu'], o['cpu'], round(o['cores']), o['num_gpus'])


def select_offers(offers, n, prefer_identical=True):
    """n offers to rent, cheapest first. With `prefer_identical`, the cheapest set of n
    offers of ONE machine type (identical machines for an experiment's arms) if any type has
    n; otherwise the n cheapest and a note that the machines differ (their throughputs are
    then measured and reported). -> (offers, note)."""
    offers = sorted(offers, key=lambda o: (o['dph'], -o['cores']))
    if len(offers) < n:
        return offers, f'only {len(offers)} matching offers for {n} instances'
    if prefer_identical:
        groups = {}
        for o in offers:
            groups.setdefault(machine_type(o), []).append(o)
        full = [(sum(o['dph'] for o in g[:n]), t, g[:n]) for t, g in groups.items() if len(g) >= n]
        if full:
            total, t, g = min(full, key=lambda x: x[0])
            return g, f'{n} identical machines: {t[1]} ({t[2]} cores), ${total:.2f}/h in total'
    pick = offers[:n]
    types = sorted({machine_type(o)[1:3] for o in pick})
    return pick, ('no machine type has %d offers: the arms run on %d different machine types %s - throughput is '
                  'measured per box and reported' % (n, len(types), types)) if len(types) > 1 else \
        f'{n} machines of one type'


# ------------------------------------------------------------------ instances

def ssh_routes(r):
    """Every way to reach the instance's sshd, preferred first: the direct public port
    (public_ipaddr + the host port mapped to 22/tcp: fastest for bulk transfers) and Vast's
    ssh proxy (ssh_host:ssh_port, e.g. ssh5.vast.ai: a TCP forward to the same sshd, so the
    same keys; it works when the host's direct port is firewalled or not mapped yet). The
    proxy port is ssh_port + 1 for jupyter-runtype instances (as `vastai ssh-url`)."""
    out = []
    p22 = (r.get('ports') or {}).get('22/tcp')
    if p22 and r.get('public_ipaddr'):
        try:
            out.append({'host': r['public_ipaddr'].strip(), 'port': int(p22[0]['HostPort']), 'kind': 'direct'})
        except (KeyError, IndexError, ValueError, TypeError, AttributeError):
            pass
    if r.get('ssh_host') and r.get('ssh_port'):
        try:
            port = int(r['ssh_port']) + (1 if 'jupyter' in (r.get('image_runtype') or '') else 0)
            out.append({'host': str(r['ssh_host']).strip(), 'port': port, 'kind': 'proxy'})
        except (ValueError, TypeError):
            pass
    return out


def _ssh_of(r):
    """The preferred route (ssh_routes), or None."""
    rs = ssh_routes(r)
    return rs[0] if rs else None


def normalize_instance(r):
    routes = ssh_routes(r)
    return {'id': int(r['id']), 'status': r.get('actual_status'), 'intended': r.get('intended_status'),
            'label': r.get('label') or '', 'ssh': routes[0] if routes else None, 'ssh_routes': routes,
            'dph': _f(r.get('dph_total')),
            'start_date': _f(r.get('start_date')) or None, 'status_msg': (r.get('status_msg') or '')[:300],
            'machine_id': r.get('machine_id'), 'host_id': r.get('host_id'), 'gpu': r.get('gpu_name', ''),
            'cpu': r.get('cpu_name', ''), 'cores': _f(r.get('cpu_cores_effective')),
            'image': r.get('image_uuid') or r.get('image') or ''}


# ------------------------------------------------------------------ providers

def api_key():
    k = os.environ.get('VAST_API_KEY', '').strip()
    if k:
        return k
    try:
        with open(KEY_FILE, encoding='utf-8') as f:
            return f.read().strip() or None
    except OSError:
        return None


def vastai_exe():
    """The vastai CLI: on PATH, in the user scripts dir of this Python, or the module itself."""
    exe = shutil.which('vastai')
    if exe:
        return [exe]
    import site
    for base in (site.getuserbase(), os.path.dirname(sys.executable)):
        for sub in ('Scripts', 'bin', os.path.join('Python%d%d' % sys.version_info[:2], 'Scripts')):
            for name in ('vastai.exe', 'vastai'):
                p = os.path.join(base, sub, name)
                if os.path.exists(p):
                    return [p]
    return [sys.executable, '-c', 'import sys; from vastai.cli.main import main; sys.exit(main())']


def api_error(err):
    """The CLI's --raw error object on stderr -> (status, msg), else None."""
    for line in reversed((err or '').splitlines()):
        line = line.strip()
        if line.startswith('{'):
            try:
                d = json.loads(line)
            except ValueError:
                continue
            if isinstance(d, dict) and (d.get('error') or 'status_code' in d):
                return int(_f(d.get('status_code'))), str(d.get('msg') or d.get('error'))[:300]
    m = re.search(r'Failed with error (\d+): (.*)', err or '')
    if m:
        return int(m.group(1)), m.group(2)[:300]
    return None


class VastCLI:
    """The real provider: the `vastai` CLI with --raw (JSON) output."""
    name = 'vast'
    dry = False

    def __init__(self, key=None, timeout=120):
        self.key = key or api_key()
        self.exe = vastai_exe()
        self.timeout = timeout

    RETRY_STATUS = (429, 500, 502, 503, 504)

    def _run(self, args, need_key=True, timeout=None, retry_status=RETRY_STATUS, attempts=4, backoff_s=3.0):
        """The CLI with --raw -> parsed JSON (None for no output). An API error (the CLI prints
        {"error": true, "status_code": ..., "msg": ...} to stderr and exits 0 - it used to read
        as "no output", e.g. "create ... failed: None") raises ProviderError with its status;
        rate limiting (429) and gateway errors are retried with backoff (`retry_status`: (429,)
        where a retry could repeat an action the API may already have done)."""
        if need_key and not self.key:
            raise ProviderError(f'no Vast.ai API key ({KEY_FILE} or VAST_API_KEY)')
        env = dict(os.environ)
        if self.key:
            env['VAST_API_KEY'] = self.key
        what = f'vastai {" ".join(map(str, args[:2]))}'
        for k in range(attempts):
            try:
                r = subprocess.run(self.exe + list(args) + ['--raw'], capture_output=True, timeout=timeout or self.timeout,
                                   env=env, stdin=subprocess.DEVNULL, creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
            except subprocess.TimeoutExpired:
                raise ProviderError(f'{what}: timed out')
            out = r.stdout.decode('utf-8', 'replace').strip()
            err = r.stderr.decode('utf-8', 'replace').strip()
            if r.returncode != 0:
                raise ProviderError(f'{what}: exit {r.returncode}: {(err or out)[-400:]}')
            api = api_error(err) if not out else None
            if api is not None:
                status, msg = api
                if status in retry_status and k < attempts - 1:
                    time.sleep(backoff_s * (2 ** k))
                    continue
                raise ProviderError(f'{what}: HTTP {status}: {msg}', status=status)
            try:
                return json.loads(out) if out else None
            except ValueError:
                m = re.search(r'(\{.*\}|\[.*\])\s*$', out, re.S)     # a warning printed before the JSON
                if m:
                    return json.loads(m.group(1))
                raise ProviderError(f'{what}: not JSON: {out[-300:]}')

    def search(self, filters, disk_gb=None, limit=200):
        q = offer_query(filters, disk_gb)
        args = ['search', 'offers', q, '--type', 'on-demand', '-o', 'dph_total', '--limit', str(limit)]
        if disk_gb:
            args += ['--storage', str(int(disk_gb))]
        rows = self._run(args, need_key=False) or []
        return [normalize_offer(r) for r in rows]

    def create(self, offer, image, disk_gb, label, onstart, env=None):
        args = ['create', 'instance', str(offer['id']), '--image', image, '--disk', str(int(disk_gb)), '--ssh',
                '--direct', '--label', label, '--onstart-cmd', onstart, '--cancel-unavail']
        if env:
            args += ['--env', env]
        try:
            r = self._run(args, timeout=180, retry_status=(429,))     # a 5xx may have created it: no blind retry
        except ProviderError as e:
            raise ProviderError(f'create instance from offer {offer["id"]} failed: {e}', status=e.status)
        if not isinstance(r, dict) or not r.get('success') or not r.get('new_contract'):
            raise ProviderError(f'create instance from offer {offer["id"]} failed: {r}')
        return int(r['new_contract'])

    def show(self, iid):
        try:
            r = self._run(['show', 'instance', str(iid)])
        except ProviderError as e:
            if re.search(r'\b(404|not found|no instance)', str(e), re.I):
                return None
            raise
        if isinstance(r, dict) and 'id' not in r:           # {"instances": None}: not found; or wrapped
            r = r.get('instances')
        if isinstance(r, list):
            r = next((x for x in r if int(x.get('id', -1)) == int(iid)), None)
        if not r or not isinstance(r, dict) or r.get('error') or 'id' not in r:
            return None
        return normalize_instance(r)

    def list(self):
        rows = self._run(['show', 'instances']) or []
        if isinstance(rows, dict):
            rows = rows.get('instances') or []
        return [normalize_instance(r) for r in rows]

    def attach_ssh(self, iid, public_key):
        """Let `public_key` log in to instance `iid`: Vast manages the instance's authorized_keys (a key appended
        to it by hand is dropped when Vast rewrites the file, and the 2026-11-05 attempt locked the instance out
        until this re-attached the desktop's key)."""
        # (the CLI prints a Python dict here, not JSON even with --raw: read its text)
        env = dict(os.environ, VAST_API_KEY=self.key or '')
        r = subprocess.run(self.exe + ['attach', 'ssh', str(iid), public_key.strip()], capture_output=True, env=env,
                           timeout=self.timeout, stdin=subprocess.DEVNULL,
                           creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
        out = (r.stdout + r.stderr).decode('utf-8', 'replace')
        if r.returncode or "'success': True" not in out:
            raise ProviderError(f'attach ssh to {iid} failed (exit {r.returncode}): {out.strip()[-300:]}')
        return out.strip()

    def destroy(self, iid):
        try:
            self._run(['destroy', 'instance', str(iid), '-y'], timeout=120)
        except ProviderError as e:
            if not re.search(r'\b(404|not found|no instance|already)', str(e), re.I):
                if self.show(iid) is not None:
                    raise
        return True

    def balance(self):
        r = self._run(['show', 'user'])
        if isinstance(r, dict):
            for k in ('credit', 'balance'):
                if k in r:
                    return _f(r[k])
        return None
