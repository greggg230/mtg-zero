"""Build, sign and publish zero-farm builds of this checkout (docs/ENGINE.md 114.3; zero-farm PROTOCOL.md 2).

    python tools/farm_build.py --profile release|fuzz [--runtime-id BUILD_ID] [--publish] [--url URL]
    python tools/farm_build.py --runtime [--ort-dll PATH] [--msvc-dir DIR] [--publish] [--url URL]
    python tools/farm_build.py --runtime --pack winml [--winml-version V | --winml-nupkg F] [--publish] [--url URL]

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

The GPU runtime pack (`--runtime --pack winml`; zero-farm PROTOCOL.md 8, DESIGN.md 8.5): Windows ML's
redistributable files from the NuGet package Microsoft.Windows.AI.MachineLearning (`--winml-version`, default
WINML_VERSION, fetched from nuget.org, or a local `--winml-nupkg`): its ONNX Runtime (CPU and DirectML built in;
static C runtime), DirectML.dll and Microsoft.Windows.AI.MachineLearning.dll (the flat C API of Windows ML's
execution-provider catalog, which on Windows 11 24H2+ brings the GPU vendors' providers: TensorRT-RTX, AMD GPU,
OpenVINO). Every file Microsoft-signed; none imports the MSVC runtime. Windows 10 1903+ runs its CPU and
DirectML providers. With them, unmodified, the package's own license files (WINML_LICENSE_FILES: `license.txt`, the
Microsoft Software License Terms of the Windows ML runtime - DirectML ships in the same package -, and
`ThirdPartyNotices.txt`; zero-farm README "Third-party components"). Checked through a release build (`info`: every DLL from the pack; `probe-providers --providers
cpu` on this PC: the CPU passes its self-test) before it is signed.

Prints the build_id (last line: `build_id <id>`). The release key: `--key` (default
~/.config/zero-farm/release.key, DPAPI: this Windows user); the admin token: the file
~/.config/zero-farm/admin_token (`--token-file`), read into ZEROFARM_ADMIN_TOKEN for farm-release, never
printed. farm-release: `--farm-release` (default E:/cargo-target/zero-farm/main/release/farm-release.exe).
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
# Windows ML (zero-farm PROTOCOL.md 8): the NuGet package and its runtime files
WINML_PACKAGE = 'microsoft.windows.ai.machinelearning'
WINML_VERSION = '2.5.96'
WINML_FILES = ('onnxruntime.dll', 'DirectML.dll', 'Microsoft.Windows.AI.MachineLearning.dll')
# ... and its license files, shipped in the pack byte for byte (the package's root; the nuspec's <license file>)
WINML_LICENSE_FILES = ('license.txt', 'ThirdPartyNotices.txt')
# what a statically linked worker may import: Windows itself
SYSTEM_DLLS = ('kernel32.dll', 'ntdll.dll', 'advapi32.dll', 'psapi.dll', 'bcryptprimitives.dll', 'user32.dll', 'ws2_32.dll',
               'userenv.dll', 'shell32.dll', 'ole32.dll', 'bcrypt.dll', 'secur32.dll', 'crypt32.dll', 'dbghelp.dll',
               'dxgi.dll')  # dxgi: the worker lists the PC's graphics adapters by LUID (zero-farm PROTOCOL.md 8)


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


def cargo_build(profile):
    """mtgz-worker built with `profile` and the static C runtime in the farm-build target dir -> the executable
    (checked: it imports nothing beyond Windows)."""
    tdir = target_dir()
    env = dict(os.environ, CARGO_TARGET_DIR=tdir, RUSTFLAGS='-C target-feature=+crt-static')
    env.setdefault('CARGO_BUILD_JOBS', '8')
    print(f'building mtgz-worker (profile {profile}, +crt-static) at {git("rev-parse", "HEAD")[:12]} in {tdir}', flush=True)
    subprocess.run(['cargo', 'build', '--profile', profile, '-p', 'mtgz_worker'], cwd=ROOT, env=env, check=True)
    exe = os.path.join(tdir, profile, 'mtgz-worker.exe')
    bad = [d for d in pe_imports(exe) if d not in SYSTEM_DLLS and not d.startswith('api-ms-win-core-')]
    if bad:
        die(f'{exe} imports {bad}: a farm worker must import nothing beyond Windows (static CRT)')
    return exe


def build_worker(a):
    ec, cc = clean_tree()
    tdir = target_dir()
    exe = cargo_build(a.profile)
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
    want_kinds = ['selfplay', 'eval', 'fuzz'] if a.profile == 'release' else ['fuzz']
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
    # loaded through a statically linked release worker (one that loaded no MSVC runtime of its own): every DLL from
    # the pack
    worker = a.check_worker or cargo_build('release')
    info = json.loads(subprocess.run([worker, 'info', '--runtime-dir', d], capture_output=True, text=True, check=True).stdout)
    ort = info['onnxruntime']
    if 'error' in ort or any(os.path.normcase(os.path.dirname(p or '')) != os.path.normcase(os.path.abspath(d)) for p in ort['modules'].values()):
        die(f'the pack did not load from itself: {ort}')
    print(f'runtime pack {version}: ONNX Runtime loaded ({ort["version"]}), every DLL from the pack ({", ".join(ort["modules"])})')
    with open(os.path.join(d, 'runtime.json'), encoding='utf-8') as f:
        rj = json.load(f)
    rj['ort_build_info'] = ort['version']
    with open(os.path.join(d, 'runtime.json'), 'w', encoding='utf-8') as f:
        json.dump(rj, f, indent=1)
    bid = sign_publish(a, d, ['--kind', 'runtime', '--version', version, '--min-agent', a.min_agent])
    return bid, d


def build_winml(a):
    """The Windows ML runtime pack (module docs) -> (build_id, directory)."""
    import io
    import urllib.request
    import zipfile
    version = a.winml_version or WINML_VERSION
    if a.winml_nupkg:
        with open(a.winml_nupkg, 'rb') as f:
            nupkg = f.read()
        source = os.path.basename(a.winml_nupkg)
    else:
        url = f'https://api.nuget.org/v3-flatcontainer/{WINML_PACKAGE}/{version}/{WINML_PACKAGE}.{version}.nupkg'
        print(f'fetching {url}', flush=True)
        with urllib.request.urlopen(url, timeout=600) as r:
            nupkg = r.read()
        source = url
    tdir = target_dir()
    with zipfile.ZipFile(io.BytesIO(nupkg)) as z:
        nuspec = next(n for n in z.namelist() if n.endswith('.nuspec'))
        spec = z.read(nuspec).decode('utf-8', 'replace')
        if not a.winml_nupkg and f'<version>{version}</version>' not in spec:
            die(f'{nuspec} is not version {version}')
        if a.winml_nupkg:
            version = spec.split('<version>', 1)[1].split('<', 1)[0]
        readme = z.read('README.md').decode('utf-8', 'replace') if 'README.md' in z.namelist() else ''
        # "This package includes **ONNX Runtime 1.28.3**."
        ort_version = readme.split('ONNX Runtime ', 1)[1].split('*', 1)[0].strip(' .') if 'includes **ONNX Runtime ' in readme else None
        pack = f'winml-{version}-ort-{ort_version or "unknown"}'
        d = os.path.join(a.out or os.path.join(tdir, 'dist'), pack)
        if os.path.exists(d):
            shutil.rmtree(d)
        os.makedirs(d)
        for n in WINML_FILES:
            with open(os.path.join(d, n), 'wb') as f:
                f.write(z.read(f'runtimes/win-x64/native/{n}'))
        if '<license type="file">license.txt</license>' not in spec:
            die(f'{nuspec}: the package no longer names license.txt as its license (look at what it ships now)')
        licenses = {}
        for n in WINML_LICENSE_FILES:
            if n not in z.namelist():
                die(f"the package has no {n} at its root: the pack must ship Microsoft's license files")
            data = z.read(n)
            with open(os.path.join(d, n), 'wb') as f:
                f.write(data)
            licenses[n] = {'sha256': hashlib.sha256(data).hexdigest(), 'size': len(data), 'source': f'{WINML_PACKAGE}.{version}.nupkg/{n}'}
    for n, x in licenses.items():
        with open(os.path.join(d, n), 'rb') as f:
            if hashlib.sha256(f.read()).hexdigest() != x['sha256']:
                die(f"{n}: not the package's bytes")
    desc = {}
    for n in WINML_FILES:
        src = os.path.join(d, n)
        st, subj = sig_of(src)
        if st != 'Valid' or 'O=Microsoft Corporation' not in subj:
            die(f'{n}: signature {st} by {subj!r}: Windows ML\'s files must be Microsoft-signed')
        bad = [m for m in pe_imports(src) if m in MSVC]
        if bad:
            die(f'{n} imports {bad}: the pack carries no MSVC runtime')
        with open(src, 'rb') as f:
            data = f.read()
        desc[n] = {'version': file_version(src), 'sha256': hashlib.sha256(data).hexdigest(), 'size': len(data), 'signature': st, 'signer': subj}
    rj = {'runtime': pack, 'onnxruntime': ort_version or desc['onnxruntime.dll']['version'], 'provider': 'cpu+dml+winml',
          'providers': ['cpu', 'dml', 'tensorrt_rtx', 'amdgpu', 'migraphx', 'openvino'],
          'winml': {'package': WINML_PACKAGE, 'version': version, 'source': source, 'sha256': hashlib.sha256(nupkg).hexdigest()},
          'files': desc, 'licenses': licenses}
    worker = a.check_worker or cargo_build('release')
    info = json.loads(subprocess.run([worker, 'info', '--runtime-dir', d], capture_output=True, text=True, check=True).stdout)
    ort = info['onnxruntime']
    loaded = {n: p for n, p in ort.get('modules', {}).items() if p}
    if 'error' in ort or any(os.path.normcase(os.path.dirname(loaded.get(n) or '')) != os.path.normcase(os.path.abspath(d)) for n in ('onnxruntime.dll', 'DirectML.dll')):
        die(f'the pack did not load from itself: {ort}')
    if 'DmlExecutionProvider' not in ort.get('available', []):
        die(f'the pack\'s ONNX Runtime has no DirectML provider: {ort.get("available")}')
    pr = subprocess.run([worker, 'probe-providers', '--runtime-dir', d, '--providers', 'cpu', '--reps', '0'], capture_output=True, text=True)
    probe = json.loads(pr.stdout) if pr.returncode == 0 and pr.stdout.strip().startswith('{') else None
    if not probe or probe.get('providers') != ['cpu']:
        die(f'probe-providers --providers cpu through the pack: {pr.stdout[-500:]} {pr.stderr[-500:]}')
    rj['ort_build_info'] = ort['version']
    rj['check'] = {'available': ort.get('available'), 'cpu_selftest_max_abs': next(r['selftest']['max_abs'] for r in probe['results'] if r['provider'] == 'cpu')}
    with open(os.path.join(d, 'runtime.json'), 'w', encoding='utf-8') as f:
        json.dump(rj, f, indent=1)
    print(f'runtime pack {pack}: ONNX Runtime {rj["onnxruntime"]} ({", ".join(ort.get("available", []))}), every DLL from the pack')
    bid = sign_publish(a, d, ['--kind', 'runtime', '--version', pack, '--min-agent', a.min_agent])
    return bid, d


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    g = ap.add_mutually_exclusive_group(required=True)
    g.add_argument('--profile', choices=('release', 'fuzz'))
    g.add_argument('--runtime', action='store_true', help='the runtime pack (ONNX Runtime + its MSVC runtime)')
    ap.add_argument('--runtime-id', help='release builds: the runtime pack build_id they load ONNX Runtime from')
    ap.add_argument('--pack', choices=('cpu', 'winml'), default='cpu', help='--runtime: the CPU pack (default) or the Windows ML GPU pack')
    ap.add_argument('--winml-version', help=f'--pack winml: the Microsoft.Windows.AI.MachineLearning version (default {WINML_VERSION})')
    ap.add_argument('--winml-nupkg', help='--pack winml: a local .nupkg instead of nuget.org')
    ap.add_argument('--ort-dll')
    ap.add_argument('--msvc-dir')
    ap.add_argument('--check-worker', help='--runtime: the worker that loads the pack as a check (default: the release farm build, built if needed)')
    ap.add_argument('--min-agent', default=MIN_AGENT)
    ap.add_argument('--out', help='where the build directories go (default <target>/farm-build/dist)')
    ap.add_argument('--publish', action='store_true')
    ap.add_argument('--url', default='https://farm.greggg230.com')
    ap.add_argument('--key', default=os.path.join(HOME, '.config', 'zero-farm', 'release.key'))
    ap.add_argument('--token-file', default=os.path.join(HOME, '.config', 'zero-farm', 'admin_token'))
    ap.add_argument('--farm-release', default='E:/cargo-target/zero-farm/main/release/farm-release.exe')
    a = ap.parse_args()
    bid, d = (build_winml(a) if a.pack == 'winml' else build_runtime(a)) if a.runtime else build_worker(a)
    print(f'build directory {d}')
    if a.publish:
        print(farm_release(a, 'publish', d, '--url', a.url, token=True))
        print(f'published to {a.url}')
    print(f'build_id {bid}')


if __name__ == '__main__':
    main()
