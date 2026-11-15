"""zero-farm fuzz campaigns with signed definitions (docs/ENGINE.md 114.5; zero-farm PROTOCOL.md 4, 5.1, 7).

    python tools/farm_campaign.py create NAME --build ID --mode M [--env K=V ...] --seed-base N [--chunk N|auto]
           [--priority P] [--needs-threads N --needs-ram-mb N] [--paused]
    python tools/farm_campaign.py update NAME [--build ID] [--mode M] [--env K=V ...] [--seed-base N]
           [--chunk N|auto] [--priority P] [--needs-threads N --needs-ram-mb N | --no-needs]
    python tools/farm_campaign.py pause NAME | resume NAME
    python tools/farm_campaign.py --from-existing NAME
  common: [--url URL] [--token-file F] [--key DATA_KEY] [--farm-release EXE] [--worker EXE] [--dist DIR ...]
          [--expires-days 90] [--defs-dir DIR] [--redefine] [--dry-run]

create / update:
1. The campaign's fields: `create` takes them from the flags (env {} unless given, chunk auto, priority 0.2,
   state active, or paused with --paused); `update` reads the campaign (`GET /v1/admin/campaign/NAME`) and puts the
   flags over it (`--env` replaces the whole env).
2. The build's signed worker: its build directory (`manifest.json` whose sha256[:24] is the build id, the executable
   checked against the manifest) under the farm_build dist directories (`--dist`, default every
   E:/cargo-target/mtg-zero/*/farm-build/dist and this checkout's), or `--worker EXE` (its `info` must name the
   build). `info` must list the kind `fuzz`; then `fuzz-check --mode M --env K=V ...` must accept the mode and env
   (PROTOCOL 5.1) and name the manifest's engine commit.
3. The campaign's definition (zero.farm_def, PROTOCOL 7): kind campaign, builds [the build], the build's runtime pack
   (`requires_runtime`, else null), no checkpoints, jobs.fuzz = {mode, env} - exactly what the PUT names. A new
   version (the coordinator's current + 1, or the archive's) signed with the data key and published by
   `farm-release define --publish`, unless the newest kept definition already says the same, is the coordinator's
   current one (when it names one) and lasts another 7 days (`--redefine`: always). It expires after
   `--expires-days` (90; 0 = never): re-run `update NAME` (or `--from-existing NAME`) before that.
4. `PUT /v1/admin/campaign/NAME` with the fields (build_id, mode, env, seed_base, chunk, priority, state, needs).

pause / resume: the campaign put again with state paused / active (no new definition: nothing it allows changes).
--from-existing NAME: the definition of a live campaign as it is (its build, mode and env from GET), checked and
published as in 2-3; nothing else is put.
--dry-run: every read and check, then the definition and the PUT body printed; nothing signed, published or put.

The admin token: the file ~/.config/zero-farm/admin_token (`--token-file`), handed to farm-release in its
environment, never on a command line, never printed. The data key: ~/.config/zero-farm/data.key (`--key`).
farm-release: `--farm-release` (default E:/cargo-target/zero-farm/main/release/farm-release.exe).
"""
import argparse
import glob
import hashlib
import json
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
sys.path.insert(0, ROOT)
import mtgz  # noqa: E402,F401  (this checkout's pinned zero-core on sys.path)
from zero import farm_def as FD  # noqa: E402

HOME = os.path.expanduser('~')
EXPIRES_DAYS = 90
RENEW_S = 7 * 86400                     # a definition that expires sooner than this is renewed by update / --from-existing
DEFAULTS = {'env': {}, 'chunk': 'auto', 'priority': 0.2, 'state': 'active', 'needs': None}
FIELDS = ('build_id', 'mode', 'env', 'seed_base', 'chunk', 'priority', 'state', 'needs')


def die(msg):
    print(f'farm_campaign: {msg}', file=sys.stderr)
    sys.exit(1)


def log(msg):
    print('# ' + msg, file=sys.stderr, flush=True)


def read_token(path):
    try:
        with open(path, encoding='utf-8') as f:
            t = f.read().strip()
    except OSError as e:
        die(f'the admin token ({path}): {e}')
    if not t:
        die(f'{path} is empty')
    return t


def parse_env(items):
    """--env K=V ... (repeatable) -> {K: V}."""
    out = {}
    for it in [x for grp in (items or []) for x in grp]:
        k, sep, v = it.partition('=')
        if not sep or not k:
            die(f'--env {it!r}: K=V')
        out[k] = v
    return out


# ---------------------------------------------------------------- the build's worker

def dist_dirs(extra=None):
    if extra:
        return list(extra)
    out = sorted(glob.glob('E:/cargo-target/mtg-zero/*/farm-build/dist'))
    try:
        import farm_build
        mine = os.path.join(farm_build.target_dir(), 'dist')
        if os.path.isdir(mine) and mine not in out:
            out.append(mine)
    except Exception:                                  # noqa: BLE001 - (not a git checkout, ...: the globbed ones)
        pass
    return out


def sha256_file(p):
    h = hashlib.sha256()
    with open(p, 'rb') as f:
        for blk in iter(lambda: f.read(1 << 20), b''):
            h.update(blk)
    return h.hexdigest()


def build_dir_of(build_id, dists):
    """The signed build directory of `build_id` under `dists` (its manifest's sha256[:24] = the id; every listed
    file's size and sha256 checked) -> (dir, manifest dict), or (None, None)."""
    for d in dists:
        for m in sorted(glob.glob(os.path.join(d, '*', 'manifest.json'))):
            with open(m, 'rb') as f:
                b = f.read()
            if hashlib.sha256(b).hexdigest()[:24] != build_id:
                continue
            man = json.loads(b)
            bd = os.path.dirname(m)
            for fe in man.get('files', []):
                p = os.path.join(bd, fe['path'])
                if not os.path.exists(p) or os.path.getsize(p) != fe['size'] or sha256_file(p) != fe['sha256']:
                    die(f'{bd}: {fe["path"]} is not the file its manifest lists (size / sha256)')
            return bd, man
    return None, None


def run_json(argv, what):
    p = subprocess.run(argv, capture_output=True, text=True, encoding='utf-8', errors='replace', timeout=300)
    if p.returncode != 0:
        die(f'{what} refused (exit {p.returncode}): {(p.stderr or p.stdout).strip()[:2000]}')
    try:
        return json.loads(p.stdout)
    except ValueError:
        die(f'{what}: not JSON: {p.stdout[:500]!r}')


def check_worker(a, build_id, mode, env):
    """The build's worker found and checked, `fuzz-check` of the mode and env passed -> (manifest, fuzz-check)."""
    if a.worker:
        exe, man = a.worker, None
        bd = os.path.dirname(os.path.abspath(exe))
        mp = os.path.join(bd, 'manifest.json')
        if os.path.exists(mp):
            with open(mp, 'rb') as f:
                man = json.loads(f.read())
    else:
        dists = dist_dirs(a.dist)
        bd, man = build_dir_of(build_id, dists)
        if bd is None:
            die(f'build {build_id}: no signed build directory under {dists} (give --worker <its mtgz-worker.exe>, '
                f'or --dist DIR)')
        exe = os.path.join(bd, man.get('entry') or 'mtgz-worker.exe')
    info = run_json([exe, 'info', '--ort-dll', 'none'], f'{exe} info')
    if info.get('build_id') != build_id:
        die(f'{exe} is build {info.get("build_id")}, not {build_id}')
    if 'fuzz' not in (info.get('kinds') or []):
        die(f'build {build_id} runs {info.get("kinds")}, no fuzz jobs')
    if man is None:
        die(f'{exe}: no manifest.json next to it (a signed build directory is needed)')
    if man.get('kind') != 'worker':
        die(f'build {build_id} is a {man.get("kind")} build, not a worker')
    argv = [exe, 'fuzz-check', '--mode', mode]
    for k, v in sorted(env.items()):
        argv += ['--env', f'{k}={v}']
    chk = run_json(argv, f'fuzz-check --mode {mode}')
    if man.get('engine_commit') and chk.get('commit') != man['engine_commit']:
        die(f'fuzz-check names commit {chk.get("commit")}, the manifest {man["engine_commit"]}')
    if chk.get('mode') != mode:
        die(f'fuzz-check reads the mode as {chk.get("mode")!r}, not {mode!r}')
    log(f'build {build_id} ({man.get("version")}, profile {man.get("profile")}): fuzz-check {mode} ok - loop '
        f'{chk.get("loop")}, min chunk {chk.get("min_chunk")}, env {chk.get("env")}')
    return man, chk


# ---------------------------------------------------------------- the definition

def definition_fields(man, build_id, mode, env):
    return {'game': man.get('game') or 'mtg', 'builds': [build_id], 'runtime': man.get('requires_runtime'),
            'checkpoints': {}, 'jobs': {'selfplay': None, 'eval': None, 'fuzz': {'mode': mode, 'env': dict(env)}}}


def ensure_definition(a, cl, name, fields):
    """The campaign's definition (module docs, 3) -> its info (version, path, ...), published unless --dry-run."""
    root = a.defs_dir
    cur, _ = FD.remote_current(cl, 'campaign', name)
    cv = cur.get('version') if cur else None
    newest, path = FD.load_local('campaign', name, root=root)
    if (newest is not None and not a.redefine and FD.same_content(newest, dict(newest, **fields))
            and (newest['expires'] is None or newest['expires'] - time.time() > RENEW_S)):
        info = {'kind': 'campaign', 'id': name, 'version': newest['version'], 'path': path, 'sha256': FD.file_sha256(path),
                'key': FD.r2_key('campaign', name, newest['version']), 'created': newest['created'], 'expires': newest['expires']}
        if FD.published_to(path, cl.url) and cv in (None, newest['version']):
            log(f'definition campaign/{name} version {newest["version"]} already says this ({path}): not re-signed')
            return dict(info, reused=True)
        if not FD.published_to(path, cl.url) and (cv is None or cv < newest['version']):
            # (made here but never published - e.g. farm-release failed: the same version, published now)
            if a.dry_run:
                log(f'dry run: definition campaign/{name} version {newest["version"]} ({path}) would be published')
                return dict(info, dry_run=True)
            log(f'definition campaign/{name} version {newest["version"]} was never published: publishing it ({path})')
            return FD.publish(cl, info, key=a.key, farm_release=a.farm_release, log=log)
    if a.dry_run:
        v = FD.next_version(cl, 'campaign', name, root)
        now = int(time.time())
        doc = FD.definition('campaign', name, v, created=now, expires=FD.expires_at(now, a.expires_days), **fields)
        print(FD.encode(doc).decode('ascii'), end='')
        log(f'dry run: definition campaign/{name} version {v} not written, signed or published')
        return {'version': v, 'path': None, 'dry_run': True}
    info = FD.prepare(cl, 'campaign', name, expires_days=a.expires_days, root=root, **fields)
    log(f'definition campaign/{name} version {info["version"]}: {info["path"]}')
    return FD.publish(cl, info, key=a.key, farm_release=a.farm_release, log=log)


# ---------------------------------------------------------------- the campaign

def campaign_body(c):
    return {k: c[k] for k in FIELDS}


def current_fields(ans):
    """A campaign's fields as GET /v1/admin/campaign answers them."""
    chunk = ans.get('chunk')
    if isinstance(chunk, str) and chunk.isdigit():
        chunk = int(chunk)
    return {'build_id': ans['build_id'], 'mode': ans['mode'], 'env': ans.get('env') or {}, 'seed_base': ans['seed_base'],
            'chunk': chunk if chunk is not None else 'auto', 'priority': ans.get('priority', 0.2),
            'state': ans.get('state', 'active'), 'needs': ans.get('needs')}


def put(a, cl, name, c):
    body = campaign_body(c)
    if a.dry_run:
        print(json.dumps({'PUT': f'/v1/admin/campaign/{name}', 'body': body}, indent=1))
        log('dry run: the campaign was not put')
        return None
    ans = cl.call('PUT', f'/v1/admin/campaign/{name}', body=body)
    log(f'campaign {name} put: {json.dumps(ans)}')
    return ans


def needs_of(a, old=None):
    if getattr(a, 'no_needs', False):
        return None
    if a.needs_threads is None and a.needs_ram_mb is None:
        return old
    n = dict(old or {})
    if a.needs_threads is not None:
        n['threads'] = a.needs_threads
    if a.needs_ram_mb is not None:
        n['ram_mb'] = a.needs_ram_mb
    return n


def chunk_of(v):
    if v is None:
        return None
    if v == 'auto':
        return 'auto'
    try:
        n = int(v)
    except ValueError:
        die(f'--chunk {v!r}: a number or auto')
    if n < 1:
        die('--chunk: >= 1')
    return n


def main(argv=None, client=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('action', nargs='?', choices=('create', 'update', 'pause', 'resume'))
    ap.add_argument('name', nargs='?')
    ap.add_argument('--from-existing', metavar='NAME', help='sign and publish the definition of a live campaign as it is')
    ap.add_argument('--build')
    ap.add_argument('--mode')
    ap.add_argument('--env', action='append', nargs='+', metavar='K=V')
    ap.add_argument('--seed-base', type=int)
    ap.add_argument('--chunk')
    ap.add_argument('--priority', type=float)
    ap.add_argument('--needs-threads', type=int)
    ap.add_argument('--needs-ram-mb', type=int)
    ap.add_argument('--no-needs', action='store_true')
    ap.add_argument('--paused', action='store_true', help='create: the campaign starts paused')
    ap.add_argument('--url', default=os.environ.get('FARM_URL', FD.URL))
    ap.add_argument('--token-file', default=os.path.join(HOME, '.config', 'zero-farm', 'admin_token'))
    ap.add_argument('--key', default=FD.data_key_path(), help='the data key (default ~/.config/zero-farm/data.key)')
    ap.add_argument('--farm-release', default=FD.farm_release_path())
    ap.add_argument('--worker', help="the build's mtgz-worker.exe (default: found in the farm_build dist directories)")
    ap.add_argument('--dist', action='append', help='a farm_build dist directory to look for the build in (repeatable)')
    ap.add_argument('--expires-days', type=float, default=EXPIRES_DAYS)
    ap.add_argument('--defs-dir', help='the definitions archive (default ~/.zero-farm/definitions)')
    ap.add_argument('--redefine', action='store_true', help='sign a new definition version even if the newest says the same')
    ap.add_argument('--dry-run', action='store_true')
    a = ap.parse_args(argv)
    if bool(a.from_existing) == bool(a.action):
        ap.error('an action (create | update | pause | resume) NAME, or --from-existing NAME')
    name = a.from_existing or a.name
    if not name or not FD.SLUG_RE.match(name):
        ap.error(f'campaign name {name!r}: [a-z0-9][a-z0-9-]{{0,62}}')
    if a.build is not None and not FD.BUILD_RE.match(a.build):
        ap.error(f'--build {a.build!r}: a build_id (24 hex)')
    cl = client or FD.Client(a.url, token=read_token(a.token_file))
    ans = cl.get_or_none(f'/v1/admin/campaign/{name}')
    if a.from_existing or a.action in ('update', 'pause', 'resume'):
        if ans is None:
            die(f'no campaign {name} on {a.url}')
        cur = current_fields(ans)
        if a.action in ('pause', 'resume'):
            given = [k for k in ('build', 'mode', 'env', 'seed_base', 'chunk', 'priority', 'needs_threads', 'needs_ram_mb')
                     if getattr(a, k) is not None]
            if given:
                ap.error(f'{a.action}: only the state changes (use update for {given})')
            put(a, cl, name, dict(cur, state='paused' if a.action == 'pause' else 'active'))
            return 0
        if a.from_existing:
            given = [k for k in ('build', 'mode', 'env', 'seed_base', 'chunk', 'priority') if getattr(a, k) is not None]
            if given:
                ap.error(f'--from-existing: the campaign as it is (use update for {given})')
            man, _ = check_worker(a, cur['build_id'], cur['mode'], cur['env'])
            info = ensure_definition(a, cl, name, definition_fields(man, cur['build_id'], cur['mode'], cur['env']))
            print(json.dumps({'campaign': name, 'definition': {k: info.get(k) for k in ('version', 'path', 'sha256')},
                              'build_id': cur['build_id'], 'mode': cur['mode'], 'env': cur['env']}))
            return 0
        c = dict(cur)
        if a.build:
            c['build_id'] = a.build
        if a.mode:
            c['mode'] = a.mode
        if a.env is not None:
            c['env'] = parse_env(a.env)
        if a.seed_base is not None:
            c['seed_base'] = a.seed_base
        if a.chunk is not None:
            c['chunk'] = chunk_of(a.chunk)
        if a.priority is not None:
            c['priority'] = a.priority
        c['needs'] = needs_of(a, cur['needs'])
    else:
        if ans is not None:
            die(f'campaign {name} exists on {a.url}: use update')
        if not (a.build and a.mode and a.seed_base is not None):
            ap.error('create: --build, --mode and --seed-base')
        c = dict(DEFAULTS, build_id=a.build, mode=a.mode, env=parse_env(a.env), seed_base=a.seed_base,
                 chunk=chunk_of(a.chunk) or 'auto', priority=a.priority if a.priority is not None else DEFAULTS['priority'],
                 state='paused' if a.paused else 'active', needs=needs_of(a))
    man, chk = check_worker(a, c['build_id'], c['mode'], c['env'])
    if isinstance(c['chunk'], int) and chk.get('min_chunk') and c['chunk'] < chk['min_chunk']:
        log(f'note: chunk {c["chunk"]} is below the mode\'s minimum chunk {chk["min_chunk"]} (fuzz-check)')
    info = ensure_definition(a, cl, name, definition_fields(man, c['build_id'], c['mode'], c['env']))
    put(a, cl, name, c)
    print(json.dumps({'campaign': name, 'action': a.action, 'definition': {k: info.get(k) for k in ('version', 'path', 'sha256')},
                      **campaign_body(c)}))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
