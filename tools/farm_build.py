"""Build, sign and publish zero-farm builds of this checkout (docs/ENGINE.md 114.3; zero-farm PROTOCOL.md 2).

    python tools/farm_build.py --profile release|fuzz [--runtime-id BUILD_ID] [--publish] [--url URL]
    python tools/farm_build.py --runtime [--ort-dll PATH] [--msvc-dir DIR] [--publish] [--url URL]

Worker builds (`--profile`): the tree must be clean (tracked files, and the zero-core submodule at its
pin with no changes) - a build names its exact commits. `mtgz-worker` is built at HEAD with that cargo
profile (`release`; `fuzz` = release + debug assertions + overflow checks, ENGINE.md 70) and the C
runtime linked statically (`+crt-static`: the executable imports nothing beyond Windows itself; checked),
in its own target dir (`<target>/farm-build`, so these flags never touch the checkout's normal builds).
The build directory holds `mtgz-worker.exe` only; it is signed with the release key by zero-farm's
`farm-release sign --kind worker --game mtg --engine-commit <HEAD> --core-commit <pin> --profile <p>
--aw 1 --zero-onnx 1 (release; none for fuzz) --min-agent 0.1.3 [--requires-runtime <id>] --entry
mtgz-worker.exe`, checked (the worker reads the signed manifest: its build_id and profile), and with
`--publish` uploaded and registered (`farm-release publish`). A release build plays networks: give
the runtime pack it loads ONNX Runtime from (`--runtime-id`); a fuzz build needs no runtime pack.

The runtime pack (`--runtime`): ONNX Runtime's CPU library (`--ort-dll`, default the onnxruntime
package's of this Python) and the MSVC runtime it imports, app-local (`--msvc-dir`, default
%WINDIR%\\System32: each must be Microsoft-signed and at least the toolset ONNX Runtime was linked
with), plus `runtime.json` (each file's version, signer and sha256); loaded through a release build's
`info --runtime-dir` (every DLL from the pack) before it is signed (`--kind runtime`).

Prints the build_id (last line: `build_id <id>`). The release key: `--key` (default
~/.config/zero-farm/release.key, DPAPI: this Windows user); the admin token: the file
~/.config/zero-farm/admin_token (`--token-file`), read into ZEROFARM_ADMIN_TOKEN for farm-release, never
printed. farm-release: `--farm-release` (default E:/cargo-target/zero-farm/agent/release/farm-release.exe).
"""
import argparse
import hashlib
import json
import os
import shutil
import struct
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
HOME = os.path.expanduser('~')
MIN_AGENT = '0.1.3'
MSVC = ('vcruntime140.dll', 'vcruntime140_1.dll', 'msvcp140.dll', 'msvcp140_1.dll')
# what a statically linked worker may import: Windows itself
SYSTEM_DLLS = ('kernel32.dll', 'ntdll.dll', 'advapi32.dll', 'psapi.dll', 'bcryptprimitives.dll', 'user32.dll', 'ws2_32.dll',
               'userenv.dll', 'shell32.dll', 'ole32.dll', 'bcrypt.dll', 'secur32.dll', 'crypt32.dll', 'dbghelp.dll')


def die(msg):
    print(f'farm_build: {msg}', file=sys.stderr)
    sys.exit(1)


def git(*a, cwd=ROOT):
    return subprocess.run(['git', *a], cwd=cwd, capture_output=True, text=True, check=True).stdout.strip()


def clean_tree():
    """(HEAD sha, the zero-core pin's sha) of a clean checkout, else die."""
    dirty = git('status', '--porcelain', '--untracked-files=no', '--ignore-submodules=none')
    if dirty:
        die(f'the tree is not clean (commit first: a build names its exact commits):\n{dirty}')
    core = os.path.join(ROOT, 'third_party', 'zero-core')
    pin = git('rev-parse', 'HEAD:third_party/zero-core')
    have = git('rev-parse', 'HEAD', cwd=core)
    if have != pin:
        die(f'third_party/zero-core is at {have[:12]}, the pin is {pin[:12]}')
    if git('status', '--porcelain', '--untracked-files=no', cwd=core):
        die('third_party/zero-core has uncommitted changes')
    return git('rev-parse', 'HEAD'), pin


def pe_imports(path):
    """The DLL names a PE file imports (its import directory), lowercased."""
    with open(path, 'rb') as f:
        b = f.read()
    pe = struct.unpack_from('<I', b, 0x3C)[0]
    assert b[pe:pe + 4] == b'PE\0\0', path
    nsec = struct.unpack_from('<H', b, pe + 6)[0]
    opt = pe + 24
    magic = struct.unpack_from('<H', b, opt)[0]
    dd = opt + (112 if magic == 0x20B else 96)
    imp_rva = struct.unpack_from('<I', b, dd + 8)[0]
    secs = [struct.unpack_from('<8sIIII', b, opt + struct.unpack_from('<H', b, pe + 20)[0] + 40 * i) for i in range(nsec)]

    def off(rva):
        for _, vsize, va, rsize, raw in secs:
            if va <= rva < va + max(vsize, rsize):
                return rva - va + raw
        raise ValueError(f'rva {rva:#x}')
    out, p = [], off(imp_rva)
    while True:
        name_rva = struct.unpack_from('<I', b, p + 12)[0]
        if not name_rva:
            break
        n = off(name_rva)
        out.append(b[n:b.index(b'\0', n)].decode('ascii').lower())
        p += 20
    return out


def target_dir():
    t = os.environ.get('CARGO_TARGET_DIR')
    if not t:
        branch = git('rev-parse', '--abbrev-ref', 'HEAD')
        t = f'E:/cargo-target/mtg-zero/{branch}' if os.name == 'nt' else os.path.join(ROOT, 'target')
    return os.path.join(t, 'farm-build')


def farm_release(a, *args, token=False):
    env = dict(os.environ)
    if token:
        try:
            with open(a.token_file, encoding='utf-8') as f:
                env['ZEROFARM_ADMIN_TOKEN'] = f.read().strip()
        except OSError as e:
            die(f'the admin token ({a.token_file}): {e}')
    p = subprocess.run([a.farm_release, *args, '--key', a.key], capture_output=True, text=True, env=env)
    if p.returncode:
        die(f'farm-release {args[0]}: {p.stderr.strip() or p.stdout.strip()}')
    return p.stdout.strip()


def sign_publish(a, d, sign_args):
    out = farm_release(a, 'sign', d, *sign_args)
    print(out)
    bid = out.rsplit('build_id ', 1)[1].split()[0]
    with open(os.path.join(d, 'manifest.json'), 'rb') as f:
        if hashlib.sha256(f.read()).hexdigest()[:24] != bid:
            die('the build_id is not sha256(manifest.json)[:24]')
    print(farm_release(a, 'verify', d))
    return bid


def build_worker(a):
    ec, cc = clean_tree()
    tdir = target_dir()
    env = dict(os.environ, CARGO_TARGET_DIR=tdir, RUSTFLAGS='-C target-feature=+crt-static')
    env.setdefault('CARGO_BUILD_JOBS', '8')
    print(f'building mtgz-worker (profile {a.profile}, +crt-static) at {ec[:12]} (zero-core {cc[:12]}) in {tdir}', flush=True)
    subprocess.run(['cargo', 'build', '--profile', a.profile, '-p', 'mtgz_worker'], cwd=ROOT, env=env, check=True)
    exe = os.path.join(tdir, a.profile, 'mtgz-worker.exe')
    bad = [d for d in pe_imports(exe) if d not in SYSTEM_DLLS and not d.startswith('api-ms-win-core-')]
    if bad:
        die(f'{exe} imports {bad}: a farm worker must import nothing beyond Windows (static CRT)')
    version = f'mtg-{ec[:7]}-{a.profile}'
    d = os.path.join(a.out or os.path.join(tdir, 'dist'), version)
    if os.path.exists(d):
        shutil.rmtree(d)
    os.makedirs(d)
    shutil.copy2(exe, os.path.join(d, 'mtgz-worker.exe'))
    sign = ['--kind', 'worker', '--version', version, '--game', 'mtg', '--engine-commit', ec, '--core-commit', cc,
            '--profile', a.profile, '--aw', '1', '--zero-onnx', '1' if a.profile == 'release' else '',
            '--min-agent', a.min_agent, '--entry', 'mtgz-worker.exe']
    if a.profile == 'release':
        if not a.runtime_id:
            die('a release build plays networks: give --runtime-id (the runtime pack it loads ONNX Runtime from)')
        sign += ['--requires-runtime', a.runtime_id]
    bid = sign_publish(a, d, sign)
    info = json.loads(subprocess.run([os.path.join(d, 'mtgz-worker.exe'), 'info', '--ort-dll', 'none'], capture_output=True, text=True,
                                     check=True).stdout)
    want_kinds = ['selfplay', 'fuzz'] if a.profile == 'release' else ['fuzz']
    if info['build_id'] != bid or info['kinds'] != want_kinds:
        die(f'the signed worker says build {info["build_id"]}, kinds {info["kinds"]} (want {bid}, {want_kinds})')
    chk = subprocess.run([os.path.join(d, 'mtgz-worker.exe'), 'fuzz-check', '--mode', 'games'], capture_output=True, text=True)
    if chk.returncode or json.loads(chk.stdout)['commit'] != ec or json.loads(chk.stdout)['profile'] != a.profile:
        die(f'fuzz-check of the signed worker: {chk.stdout} {chk.stderr}')
    return bid, d


def sig_of(path):
    """(Authenticode status, signer subject) of a file (PowerShell)."""
    ps = (f"$s = Get-AuthenticodeSignature -LiteralPath '{path}'; "
          "'{0}|{1}' -f $s.Status, $s.SignerCertificate.Subject")
    out = subprocess.run(['powershell.exe', '-NoProfile', '-NonInteractive', '-Command', ps], capture_output=True, text=True).stdout.strip()
    st, _, subj = out.partition('|')
    return st, subj


def file_version(path):
    ps = f"(Get-Item -LiteralPath '{path}').VersionInfo.FileVersion"
    return subprocess.run(['powershell.exe', '-NoProfile', '-NonInteractive', '-Command', ps], capture_output=True, text=True).stdout.strip()


def build_runtime(a):
    ort_dll = a.ort_dll
    if not ort_dll:
        import onnxruntime
        ort_dll = os.path.join(os.path.dirname(onnxruntime.__file__), 'capi', 'onnxruntime.dll')
        ort_version = onnxruntime.__version__
    else:
        ort_version = file_version(ort_dll).split()[0]
    msvc_dir = a.msvc_dir or os.path.join(os.environ.get('WINDIR', 'C:/Windows'), 'System32')
    need = sorted(set(pe_imports(ort_dll)) & set(MSVC))
    tdir = target_dir()
    files = {'onnxruntime.dll': ort_dll}
    for n in need:
        files[n] = os.path.join(msvc_dir, n)
    desc = {}
    for n, src in files.items():
        st, subj = sig_of(src)
        if n in MSVC and (st != 'Valid' or 'O=Microsoft Corporation' not in subj):
            die(f'{src}: signature {st} by {subj!r}: the MSVC runtime must be Microsoft-signed')
        with open(src, 'rb') as f:
            data = f.read()
        desc[n] = {'version': file_version(src), 'sha256': hashlib.sha256(data).hexdigest(), 'size': len(data), 'signature': st,
                   'signer': subj, 'source': os.path.abspath(src)}
    # the toolset ONNX Runtime was linked with: the MSVC runtime must be at least as new
    with open(ort_dll, 'rb') as f:
        b = f.read()
    pe = struct.unpack_from('<I', b, 0x3C)[0]
    lmaj, lmin = b[pe + 24 + 2], b[pe + 24 + 3]
    for n in need:
        v = [int(x) for x in desc[n]['version'].split('.')[:2]]
        if v < [lmaj, lmin]:
            die(f'{files[n]} is {desc[n]["version"]}, older than the toolset {lmaj}.{lmin} ONNX Runtime was linked with')
    version = f'ort-{ort_version}-cpu+msvc-{desc[need[0]]["version"]}' if need else f'ort-{ort_version}-cpu'
    d = os.path.join(a.out or os.path.join(tdir, 'dist'), version)
    if os.path.exists(d):
        shutil.rmtree(d)
    os.makedirs(d)
    for n, src in files.items():
        shutil.copy2(src, os.path.join(d, n))
    with open(os.path.join(d, 'runtime.json'), 'w', encoding='utf-8') as f:
        json.dump({'runtime': version, 'onnxruntime': ort_version, 'provider': 'cpu', 'linker': f'{lmaj}.{lmin}', 'files': desc}, f, indent=1)
    # loaded through a worker: every DLL from the pack
    worker = a.check_worker or os.path.join(tdir, 'release', 'mtgz-worker.exe')
    if not os.path.exists(worker):
        die(f'no worker to check the pack with ({worker}): build one (--profile release) or give --check-worker')
    info = json.loads(subprocess.run([worker, 'info', '--runtime-dir', d], capture_output=True, text=True, check=True).stdout)
    ort = info['onnxruntime']
    if ort.get('version') != ort_version or any(os.path.normcase(os.path.dirname(p or '')) != os.path.normcase(os.path.abspath(d))
                                               for p in ort['modules'].values()):
        die(f'the pack did not load from itself: {ort}')
    print(f'runtime pack {version}: ONNX Runtime {ort["version"]}, every DLL from the pack ({", ".join(ort["modules"])})')
    bid = sign_publish(a, d, ['--kind', 'runtime', '--version', version, '--min-agent', a.min_agent])
    return bid, d


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    g = ap.add_mutually_exclusive_group(required=True)
    g.add_argument('--profile', choices=('release', 'fuzz'))
    g.add_argument('--runtime', action='store_true', help='the runtime pack (ONNX Runtime + its MSVC runtime)')
    ap.add_argument('--runtime-id', help='release builds: the runtime pack build_id they load ONNX Runtime from')
    ap.add_argument('--ort-dll')
    ap.add_argument('--msvc-dir')
    ap.add_argument('--check-worker', help='--runtime: the worker that loads the pack as a check (default the release farm build)')
    ap.add_argument('--min-agent', default=MIN_AGENT)
    ap.add_argument('--out', help='where the build directories go (default <target>/farm-build/dist)')
    ap.add_argument('--publish', action='store_true')
    ap.add_argument('--url', default='https://farm.greggg230.com')
    ap.add_argument('--key', default=os.path.join(HOME, '.config', 'zero-farm', 'release.key'))
    ap.add_argument('--token-file', default=os.path.join(HOME, '.config', 'zero-farm', 'admin_token'))
    ap.add_argument('--farm-release', default='E:/cargo-target/zero-farm/agent/release/farm-release.exe')
    a = ap.parse_args()
    bid, d = build_runtime(a) if a.runtime else build_worker(a)
    print(f'build directory {d}')
    if a.publish:
        print(farm_release(a, 'publish', d, '--url', a.url, token=True))
        print(f'published to {a.url}')
    print(f'build_id {bid}')


if __name__ == '__main__':
    main()
