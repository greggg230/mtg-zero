"""Build, sign and publish zero-farm builds of this checkout (docs/ENGINE.md 114.3; zero-farm PROTOCOL.md 2).

    python tools/farm_build.py --profile release|fuzz [--runtime-id BUILD_ID] [--root CHECKOUT] [--publish] [--url URL]
    python tools/farm_build.py --runtime [--ort-dll PATH] [--msvc-dir DIR] [--publish] [--url URL]
    python tools/farm_build.py --runtime --pack winml [--winml-version V | --winml-nupkg F] [--publish] [--url URL]
    python tools/farm_build.py --runtime --pack cuda --msvc-dir DIR [--check-net EXPORT_DIR] [--publish] [--url URL]
    python tools/farm_build.py --resign BUILD_DIR --runtime-id ID [--min-agent V] [--publish] [--url URL]
    python tools/farm_build.py --publish-dir BUILD_DIR [--url URL]

`--root CHECKOUT` builds another clean checkout (a worktree at the commit to release) with this file's code: the build
names that checkout's commits (tools/farm_release.py, docs/ENGINE.md 196). `--resign` signs a signed worker build's
executable again for another runtime pack (zero-farm PROTOCOL.md 9.1: the CUDA alternate), `--publish-dir` publishes
a build directory signed earlier (verified first), so a release can sign, check, then publish.

Worker builds (`--profile`): the tree must be clean (tracked files, and the zero-core submodule at its
pin with no changes) - a build names its exact commits. `mtgz-worker` is built at HEAD with that cargo
profile (`release`; `fuzz` = release + debug assertions + overflow checks, ENGINE.md 70) and the C
runtime linked statically (`+crt-static`: the executable imports nothing beyond Windows itself; checked),
in its own target dir (`<target>/farm-build`, so these flags never touch the checkout's normal builds).
The build directory holds `mtgz-worker.exe` only; it is signed with the release key by zero-farm's
`farm-release sign --kind worker --game mtg --engine-commit <HEAD> --core-commit <pin> --profile <p>
--aw 1 --zero-onnx <the worker's own info: 1,2> (release; none for fuzz) --min-agent 0.1.3 [--requires-runtime <id>] --entry
mtgz-worker.exe`, checked (the worker reads the signed manifest: its build_id and profile), and with
`--publish` uploaded and registered (`farm-release publish`). A release build plays networks: give
the runtime pack it loads ONNX Runtime from (`--runtime-id`); a fuzz build needs no runtime pack.
`ZERO_PROBE_VARIANTS` (zero-core crates/worker/build.rs: the probe's graph variants, which may hold real networks, embed
into the worker) is refused for every build except `--diagnostic` (a release worker for canary test runs, version
`mtg-<sha>-release-diag`), which requires it.

The runtime pack (`--runtime`): ONNX Runtime's CPU library (`--ort-dll`, default the onnxruntime
package's of this Python) and the MSVC runtime it imports, app-local (`--msvc-dir`: a Visual Studio
redist folder, below - the default %WINDIR%\\System32 ships no license and is refused; each must be
Microsoft-signed and at least the toolset ONNX Runtime was linked with), plus `runtime.json` (each file's version, signer and sha256); loaded through a release build's
`info --runtime-dir` (every DLL from the pack) before it is signed (`--kind runtime`). With them, unmodified
(zero-farm README "Third-party components"; runtime.json 'licenses': each file's sha256, component and source):
* ONNX Runtime's `LICENSE` and `ThirdPartyNotices.txt` from the onnxruntime package the DLL comes from (ORT_LICENSE_FILES:
  the DLL and both files checked against the package's RECORD, i.e. the wheel's own bytes);
* when the pack carries the MSVC runtime, the Visual C++ runtime's license from the Visual Studio redist folder
  the DLLs come from (`--msvc-dir` must then be `<VS>\\VC\\Redist\\MSVC\\<v>\\x64\\Microsoft.VC143.CRT`): VS ships no
  license file next to the DLLs, so `license.rtf` - the "Microsoft Visual C++ 2015 - 2022 Runtime" license terms -
  is taken byte for byte out of the folder's `vc_redist.x64.exe` (Microsoft-signed, the DLLs' own version; the
  payload checked against the bundle manifest's SHA-1), and `Redist.txt` - Visual Studio 2022's "Distributable Code"
  file, the redist list its license terms name - from `<VS>\\Licenses\\1033` (the VS 2022 license terms themselves
  are not a file in the install: the installer only links them).

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
printed. farm-release: `--farm-release` (default $ZERO_BIG_DISK/cargo-target/zero-farm/main/release/farm-release.exe
on a machine that names a big disk in ~/.config/zero/local.env, else farm-release on PATH).
"""
import argparse
import hashlib
import json
import os
import shutil
import struct
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import local_config  # noqa: E402  (the machine's own settings: ~/.config/zero/local.env, ZERO_BIG_DISK)

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
# The CUDA runtime pack (zero-farm PROTOCOL.md 9, rev 11): official redistributables from PyPI, pinned. ONNX Runtime
# 1.24.4 is the last CUDA 12 build (1.27+ need CUDA 13 = driver 580+); it is built with CUDA 12.8 and needs 12.8+.
# (package, version, wheel tag, {member in the wheel: file in the pack}, {license member: file in the pack})
CUDA_WHEELS = (
    ('onnxruntime-gpu', '1.24.4', 'cp313-cp313-win_amd64',
     {'onnxruntime/capi/onnxruntime.dll': 'onnxruntime.dll',
      'onnxruntime/capi/onnxruntime_providers_cuda.dll': 'onnxruntime_providers_cuda.dll',
      'onnxruntime/capi/onnxruntime_providers_shared.dll': 'onnxruntime_providers_shared.dll'},
     {'onnxruntime/LICENSE': 'LICENSE', 'onnxruntime/ThirdPartyNotices.txt': 'ThirdPartyNotices.txt'}),
    ('nvidia-cuda-runtime-cu12', '12.8.90', 'py3-none-win_amd64',
     {'nvidia/cuda_runtime/bin/cudart64_12.dll': 'cudart64_12.dll'},
     {'nvidia_cuda_runtime_cu12-12.8.90.dist-info/License.txt': 'NVIDIA_CUDA_EULA.txt'}),
    ('nvidia-cublas-cu12', '12.8.4.1', 'py3-none-win_amd64',
     {'nvidia/cublas/bin/cublas64_12.dll': 'cublas64_12.dll', 'nvidia/cublas/bin/cublasLt64_12.dll': 'cublasLt64_12.dll'},
     {'nvidia_cublas_cu12-12.8.4.1.dist-info/License.txt': 'NVIDIA_CUDA_EULA.txt'}),
    # cuFFT: a load-time import of onnxruntime_providers_cuda.dll (Windows loads it whatever the graph does)
    ('nvidia-cufft-cu12', '11.3.3.83', 'py3-none-win_amd64',
     {'nvidia/cufft/bin/cufft64_11.dll': 'cufft64_11.dll'},
     {'nvidia_cufft_cu12-11.3.3.83.dist-info/License.txt': 'NVIDIA_CUDA_EULA.txt'}),
    # cuDNN: only the three libraries our graphs load (cudnn64_9 loads the others by name, on demand: 2.3 GB -> 0.2 GB)
    ('nvidia-cudnn-cu12', '9.28.1.4', 'py3-none-win_amd64',
     {'nvidia/cudnn/bin/cudnn64_9.dll': 'cudnn64_9.dll', 'nvidia/cudnn/bin/cudnn_graph64_9.dll': 'cudnn_graph64_9.dll',
      'nvidia/cudnn/bin/cudnn_ops64_9.dll': 'cudnn_ops64_9.dll'},
     {'nvidia_cudnn_cu12-9.28.1.4.dist-info/licenses/LICENSE': 'NVIDIA_cuDNN_LICENSE.txt'}),
)
# The pack's hard hardware requirement (PROTOCOL.md 9.2): cuDNN 9.28's Windows minimum for CUDA 12.x (CUDA 12's own
# minor-version minimum is 525; Blackwell cards need 570.65+, which every driver supporting them is)
CUDA_GPU = ('nvidia', '527.41', 'cuda')
# Agents that read a runtime manifest's `gpu` and `zst` (PROTOCOL.md 9)
ALT_MIN_AGENT = '0.2.10'
# the CPU pack's license files (module docs): ONNX Runtime's, from its package's root ...
ORT_LICENSE_FILES = ('LICENSE', 'ThirdPartyNotices.txt')
# ... and the Visual C++ runtime's: the license its redistributable shows (a payload of vc_redist.x64.exe's UX
# container) and Visual Studio's distributable-code list
VCREDIST_LICENSE = 'license.rtf'
VCREDIST_LICENSE_TITLE = b'MICROSOFT VISUAL C++ 2015 - 2022 RUNTIME'
VS_REDIST_TXT = ('Licenses', '1033', 'Redist.txt')
VS_REDIST_TXT_TITLE = b'Distributable Code for Microsoft Visual Studio 2022'
# what a statically linked worker may import: Windows itself
SYSTEM_DLLS = ('kernel32.dll', 'ntdll.dll', 'advapi32.dll', 'psapi.dll', 'bcryptprimitives.dll', 'user32.dll', 'ws2_32.dll',
               'userenv.dll', 'shell32.dll', 'ole32.dll', 'bcrypt.dll', 'secur32.dll', 'crypt32.dll', 'dbghelp.dll',
               'dxgi.dll')  # dxgi: the worker lists the PC's graphics adapters by LUID (zero-farm PROTOCOL.md 8)


def die(msg):
    print(f'farm_build: {msg}', file=sys.stderr)
    sys.exit(1)


def git(*a, cwd=None):
    return subprocess.run(['git', *a], cwd=cwd or ROOT, capture_output=True, text=True, check=True).stdout.strip()


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


FARM_RELEASE = local_config.big_disk_path('cargo-target/zero-farm/main/release/farm-release.exe',
                                         default='farm-release')


def target_dir():
    t = os.environ.get('CARGO_TARGET_DIR')
    if not t:
        branch = git('rev-parse', '--abbrev-ref', 'HEAD')
        big = local_config.big_disk_path('cargo-target', 'mtg-zero', branch) if os.name == 'nt' else None
        t = big or os.path.join(ROOT, 'target')
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


def check_variants_env(a):
    """zero-core's crates/worker/build.rs embeds ZERO_PROBE_VARIANTS (the probe's graph variants, which may hold real
    networks) into every worker built while it is set: a normal build with it set - a stray shell variable - would ship
    them in a public release. Only --diagnostic (a release worker for canary test runs) builds with it, and only with it."""
    v = os.environ.get('ZERO_PROBE_VARIANTS')
    if a.diagnostic:
        if a.runtime or a.profile != 'release':
            die('--diagnostic is for a release worker build (--profile release)')
        if not v or not os.path.isfile(v):
            die(f'--diagnostic needs ZERO_PROBE_VARIANTS = an existing variants archive (got {v!r})')
    elif v:
        die(f'ZERO_PROBE_VARIANTS is set ({v}): zero-core would embed those probe variants (possibly real networks) in this '
            'build. Unset it for a normal build, or pass --diagnostic for a canary-only diagnostic worker.')


def build_worker(a):
    ec, cc = clean_tree()
    tdir = target_dir()
    exe = cargo_build(a.profile)
    version = f'mtg-{ec[:7]}-{a.profile}' + ('-diag' if a.diagnostic else '')
    d = os.path.join(a.out or os.path.join(tdir, 'dist'), version)
    if os.path.exists(d):
        shutil.rmtree(d)
    os.makedirs(d)
    shutil.copy2(exe, os.path.join(d, 'mtgz-worker.exe'))
    # (the network file versions the worker loads, as it reports them - zero-onnx/1 and /2 since layout v7's tails,
    # zero-core docs/az.md 46 - signed into its manifest; a fuzz build plays no network)
    zo = json.loads(subprocess.run([exe, 'info', '--ort-dll', 'none'], capture_output=True, text=True, check=True).stdout)['zero_onnx']
    sign = ['--kind', 'worker', '--version', version, '--game', 'mtg', '--engine-commit', ec, '--core-commit', cc,
            '--profile', a.profile, '--aw', '1', '--zero-onnx', ','.join(map(str, zo)) if a.profile == 'release' else '',
            '--min-agent', a.min_agent, '--entry', 'mtgz-worker.exe']
    if a.profile == 'release':
        if not a.runtime_id:
            die('a release build plays networks: give --runtime-id (the runtime pack it loads ONNX Runtime from)')
        sign += ['--requires-runtime', a.runtime_id]
    bid = sign_publish(a, d, sign)
    info = json.loads(subprocess.run([os.path.join(d, 'mtgz-worker.exe'), 'info', '--ort-dll', 'none'], capture_output=True, text=True,
                                     check=True).stdout)
    want_kinds = ['selfplay', 'eval', 'fuzz'] if a.profile == 'release' else ['fuzz']
    if info['build_id'] != bid or info['kinds'] != want_kinds or info.get('zero_onnx') != zo:
        die(f'the signed worker says build {info["build_id"]}, kinds {info["kinds"]}, zero_onnx {info.get("zero_onnx")} '
            f'(want {bid}, {want_kinds}, {zo})')
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


def ort_licenses(ort_dll):
    """ONNX Runtime's license files (ORT_LICENSE_FILES) from the package `ort_dll` comes from -> {name: (bytes,
    component, source)}: the DLL and each file checked against the package's RECORD (the sha256s the wheel installed)."""
    import base64
    import csv
    import glob
    dll = os.path.abspath(ort_dll)
    pkg = os.path.dirname(os.path.dirname(dll))
    site, top = os.path.dirname(pkg), os.path.basename(pkg)
    dll_rel = f'{top}/capi/onnxruntime.dll'
    if os.path.normcase(dll) != os.path.normcase(os.path.join(site, *dll_rel.split('/'))):
        die(f'{ort_dll}: not an onnxruntime package\'s capi/onnxruntime.dll (the pack ships the license files of the package its DLL comes from)')
    for rec in sorted(glob.glob(os.path.join(site, '*.dist-info', 'RECORD'))):
        with open(rec, newline='', encoding='utf-8') as f:
            rows = {r[0]: r[1] for r in csv.reader(f) if len(r) >= 2}
        if dll_rel in rows:
            break
    else:
        die(f'{ort_dll}: no installed package lists it in its RECORD (the pack ships the license files of the package its DLL comes from)')
    dist = os.path.basename(os.path.dirname(rec))[:-len('.dist-info')]

    def package_bytes(rel):
        want = rows.get(rel, '')
        if not want.startswith('sha256='):
            die(f'{dist}: its RECORD has no sha256 for {rel}')
        with open(os.path.join(site, *rel.split('/')), 'rb') as f:
            data = f.read()
        if base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b'=').decode() != want[len('sha256='):]:
            die(f'{rel}: not the bytes the {dist} package installed')
        return data
    package_bytes(dll_rel)
    return {n: (package_bytes(f'{top}/{n}'), 'ONNX Runtime', f'{dist} (pip package): {top}/{n}') for n in ORT_LICENSE_FILES}


def burn_ux_payload(exe, name):
    """The payload `name` of a WiX Burn bundle's UX container (`exe`'s first attached container: a cabinet), byte for
    byte: extracted with Windows' expand.exe and checked against the bundle manifest's size and SHA-1."""
    import re
    import tempfile
    import xml.etree.ElementTree as ET
    with open(exe, 'rb') as f:
        b = f.read()
    pe = struct.unpack_from('<I', b, 0x3C)[0]
    nsec = struct.unpack_from('<H', b, pe + 6)[0]
    sec0 = pe + 24 + struct.unpack_from('<H', b, pe + 20)[0]
    for i in range(nsec):
        sname, _, _, _, raw = struct.unpack_from('<8sIIII', b, sec0 + 40 * i)
        if sname.rstrip(b'\0') == b'.wixburn':
            break
    else:
        die(f'{exe}: not a WiX Burn bundle (no .wixburn section)')
    # BURN_SECTION_HEADER: magic, version, bundle GUID, stub size (= where the UX container starts), original
    # checksum, original signature offset + size, container format (1 = cabinet), container count, container sizes
    magic = struct.unpack_from('<I', b, raw)[0]
    stub, _, _, _, fmt, count = struct.unpack_from('<6I', b, raw + 24)
    if magic != 0x00F14300 or fmt != 1 or count < 1:
        die(f'{exe}: unexpected Burn section (magic {magic:#x}, format {fmt}, {count} containers)')
    ux = struct.unpack_from('<I', b, raw + 48)[0]
    if b[stub:stub + 4] != b'MSCF':
        die(f'{exe}: the UX container is not a cabinet')
    with tempfile.TemporaryDirectory() as t:
        cab, out = os.path.join(t, 'ux.cab'), os.path.join(t, 'ux')
        os.makedirs(out)
        with open(cab, 'wb') as f:
            f.write(b[stub:stub + ux])
        expand = os.path.join(os.environ.get('WINDIR', 'C:/Windows'), 'System32', 'expand.exe')
        subprocess.run([expand, cab, '-F:*', out], capture_output=True, check=True)
        # the UX container's manifest ("0") names each payload's file, its name in the cabinet, size and SHA-1
        found = [e for e in ET.parse(os.path.join(out, '0')).getroot().iter()
                 if e.tag.rsplit('}', 1)[-1] == 'Payload' and e.get('FilePath') == name and e.get('Packaging') == 'embedded']
        if len(found) != 1 or not re.fullmatch(r'u\d+', found[0].get('SourcePath') or ''):
            die(f'{exe}: its UX container has no single embedded payload {name}')
        p = found[0]
        with open(os.path.join(out, p.get('SourcePath')), 'rb') as f:
            data = f.read()
    if len(data) != int(p.get('FileSize')) or hashlib.sha1(data).hexdigest() != (p.get('Hash') or '').lower():
        die(f'{exe}: {name} does not match the bundle manifest\'s size and SHA-1')
    return data


def msvc_licenses(msvc_dir, versions):
    """The Visual C++ runtime's license files (module docs) for the MSVC DLLs taken from `msvc_dir` (their file
    `versions`) -> {name: (bytes, component, source)}."""
    parts = os.path.normpath(os.path.abspath(msvc_dir)).split(os.sep)
    low = [p.lower() for p in parts]
    i = next((k for k in range(len(low) - 3) if low[k:k + 3] == ['vc', 'redist', 'msvc']), None)
    if i is None or len(parts) < i + 5:
        die(f'--msvc-dir {msvc_dir}: the MSVC runtime must come from a Visual Studio redist folder '
            '(<VS>\\VC\\Redist\\MSVC\\<v>\\x64\\Microsoft.VC143.CRT): its license comes from there')
    vs, redist = os.sep.join(parts[:i]), os.sep.join(parts[:i + 4])
    exe = os.path.join(redist, 'vc_redist.x64.exe')
    st, subj = sig_of(exe)
    if st != 'Valid' or 'O=Microsoft Corporation' not in subj:
        die(f'{exe}: signature {st} by {subj!r}: the redistributable must be Microsoft-signed')
    v = file_version(exe)
    if any(x != v for x in versions):
        die(f'{exe} is {v}, the DLLs are {sorted(set(versions))}: the license must be the DLLs\' own redistributable\'s')
    lic = burn_ux_payload(exe, VCREDIST_LICENSE)
    if VCREDIST_LICENSE_TITLE not in lic:
        die(f'{exe}: its {VCREDIST_LICENSE} is not the {VCREDIST_LICENSE_TITLE.decode()} license terms (look at what it ships now)')
    with open(os.path.join(vs, *VS_REDIST_TXT), 'rb') as f:
        txt = f.read()
    if VS_REDIST_TXT_TITLE not in txt:
        die(f'{os.path.join(vs, *VS_REDIST_TXT)} is not Visual Studio 2022\'s distributable-code file (look at what it ships now)')
    vswhere = os.path.join(os.environ.get('ProgramFiles(x86)', 'C:/Program Files (x86)'), 'Microsoft Visual Studio', 'Installer', 'vswhere.exe')
    q = lambda prop: subprocess.run([vswhere, '-path', vs, '-property', prop], capture_output=True, text=True).stdout.strip()
    name = f'{q("displayName")} {q("catalog_productDisplayVersion")}'.strip() if os.path.exists(vswhere) else ''
    name = name or 'Visual Studio'
    rel = os.path.relpath(exe, vs).replace(os.sep, '/')
    return {VCREDIST_LICENSE: (lic, 'Microsoft Visual C++ runtime', f'{name}: {rel} ({v}), its bundle payload {VCREDIST_LICENSE}'),
            VS_REDIST_TXT[-1]: (txt, 'Microsoft Visual C++ runtime', f'{name}: {"/".join(VS_REDIST_TXT)}')}


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
    # the license files, unmodified (module docs)
    lic = ort_licenses(ort_dll)
    if need:
        lic.update(msvc_licenses(msvc_dir, [desc[n]['version'] for n in need]))
    taken = [n.lower() for n in [*files, 'runtime.json', *lic]]
    if len(set(taken)) != len(taken):
        die(f'two files of the pack share a name: {sorted(taken)}')
    licenses = {n: {'sha256': hashlib.sha256(data).hexdigest(), 'size': len(data), 'component': comp, 'source': source}
                for n, (data, comp, source) in lic.items()}
    d = os.path.join(a.out or os.path.join(tdir, 'dist'), version)
    if os.path.exists(d):
        shutil.rmtree(d)
    os.makedirs(d)
    for n, src in files.items():
        shutil.copy2(src, os.path.join(d, n))
    for n, (data, _, _) in lic.items():
        with open(os.path.join(d, n), 'wb') as f:
            f.write(data)
    for n, x in licenses.items():
        with open(os.path.join(d, n), 'rb') as f:
            if hashlib.sha256(f.read()).hexdigest() != x['sha256']:
                die(f'{n}: not its source\'s bytes')
    with open(os.path.join(d, 'runtime.json'), 'w', encoding='utf-8') as f:
        json.dump({'runtime': version, 'onnxruntime': ort_version, 'provider': 'cpu', 'linker': f'{lmaj}.{lmin}', 'files': desc,
                   'licenses': licenses}, f, indent=1)
    # loaded through a statically linked release worker (one that loaded no MSVC runtime of its own): every DLL from
    # the pack
    worker = a.check_worker or cargo_build('release')
    info = json.loads(subprocess.run([worker, 'info', '--runtime-dir', d], capture_output=True, text=True, check=True).stdout)
    ort = info['onnxruntime']
    # newer workers also list the modules they look for but did not load (DirectML.dll: None)
    loaded = {n: p for n, p in ort.get('modules', {}).items() if p}
    here = os.path.normcase(os.path.abspath(d))
    if 'error' in ort or any(os.path.normcase(os.path.dirname(p)) != here for p in loaded.values()) or set(files) - set(loaded):
        die(f'the pack did not load from itself: {ort}')
    print(f'runtime pack {version}: ONNX Runtime loaded ({ort["version"]}), every DLL from the pack ({", ".join(sorted(loaded))})')
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


def pypi_wheel(name, version, tag, wheel_dir):
    """The wheel `name`-`version`-`tag` from PyPI (its JSON API names the file, its URL and sha256), cached in
    `wheel_dir` -> (path, {filename, url, sha256})."""
    import urllib.request
    with urllib.request.urlopen(f'https://pypi.org/pypi/{name}/{version}/json', timeout=60) as r:
        info = json.load(r)
    want = f'{name.replace("-", "_")}-{version}-{tag}.whl'
    hit = [u for u in info['urls'] if u['filename'] == want]
    if len(hit) != 1:
        die(f'PyPI has no {want} for {name} {version} (files: {[u["filename"] for u in info["urls"]][:8]})')
    u = hit[0]
    src = {'filename': u['filename'], 'url': u['url'], 'sha256': u['digests']['sha256']}
    os.makedirs(wheel_dir, exist_ok=True)
    path = os.path.join(wheel_dir, want)
    if os.path.exists(path) and file_sha256(path) == src['sha256']:
        return path, src
    print(f'fetching {src["url"]}', flush=True)
    tmp = path + '.part'
    with urllib.request.urlopen(src['url'], timeout=3600) as r, open(tmp, 'wb') as f:
        shutil.copyfileobj(r, f, 1 << 20)
    if file_sha256(tmp) != src['sha256']:
        die(f'{want}: the download does not match PyPI\'s sha256')
    os.replace(tmp, path)
    return path, src


def file_sha256(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for b in iter(lambda: f.read(1 << 20), b''):
            h.update(b)
    return h.hexdigest()


def wheel_member(z, rows, member, what):
    """A wheel member's bytes, checked against the wheel's RECORD (its own sha256 of the file)."""
    import base64
    want = rows.get(member, '')
    if not want.startswith('sha256='):
        die(f'{what}: its RECORD has no sha256 for {member}')
    data = z.read(member)
    if base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b'=').decode() != want[len('sha256='):]:
        die(f'{what}: {member} is not the bytes its RECORD names')
    return data


def build_cuda(a):
    """The CUDA runtime pack (module docs, zero-farm PROTOCOL.md 9) -> (build_id, directory)."""
    import csv
    import io
    import zipfile
    tdir = target_dir()
    wheel_dir = a.wheel_dir or os.path.join(tdir, 'wheels')
    files, desc, licenses, wheels = {}, {}, {}, []
    ort_version = None
    for name, version, tag, members, lic in CUDA_WHEELS:
        path, src = pypi_wheel(name, version, tag, wheel_dir)
        wheels.append({'package': name, 'version': version, **src})
        what = f'{name} {version}'
        with zipfile.ZipFile(path) as z:
            rec = next(n for n in z.namelist() if n.endswith('.dist-info/RECORD'))
            rows = {r[0]: r[1] for r in csv.reader(io.TextIOWrapper(z.open(rec), encoding='utf-8')) if len(r) >= 2}
            for member, n in members.items():
                files[n] = (wheel_member(z, rows, member, what), f'{src["filename"]}/{member}')
            for member, n in lic.items():
                data = wheel_member(z, rows, member, what)
                if n in licenses:
                    # the CUDA wheels share one EULA: the same bytes, one file, every source named
                    if licenses[n]['sha256'] != hashlib.sha256(data).hexdigest():
                        die(f'{n}: {what} ships a different {member} than {licenses[n]["source"][0]}')
                    licenses[n]['source'].append(f'{src["filename"]}/{member}')
                    continue
                licenses[n] = {'sha256': hashlib.sha256(data).hexdigest(), 'size': len(data), 'component': name, 'source': [f'{src["filename"]}/{member}'],
                               'data': data}
        if name == 'onnxruntime-gpu':
            ort_version = version
    pack = f'cuda-12.8-cudnn-9.28-ort-{ort_version}'
    d = os.path.join(a.out or os.path.join(tdir, 'dist'), pack)
    if os.path.exists(d):
        shutil.rmtree(d)
    os.makedirs(d)
    for n, (data, _) in files.items():
        with open(os.path.join(d, n), 'wb') as f:
            f.write(data)
    # every DLL signed by its maker; the MSVC runtime the DLLs import, app-local, from a VS redist folder
    for n, (data, source) in files.items():
        st, subj = sig_of(os.path.join(d, n))
        maker = 'O=Microsoft Corporation' if n.startswith('onnxruntime') else 'O=NVIDIA Corporation'
        # NVIDIA Authenticode-signs cuDNN but not the CUDA toolkit's libraries (cudart, cuBLAS, cuFFT: unsigned in its
        # installers and wheels alike); those rest on PyPI's sha256 of the wheel and the wheel's RECORD (checked above)
        unsigned_ok = maker == 'O=NVIDIA Corporation' and not n.startswith('cudnn') and st == 'NotSigned'
        if not unsigned_ok and (st != 'Valid' or maker not in subj):
            die(f'{n}: signature {st} by {subj!r}: expected one by {maker[2:]}')
        desc[n] = {'version': file_version(os.path.join(d, n)), 'sha256': hashlib.sha256(data).hexdigest(), 'size': len(data), 'signature': st,
                   'signer': subj, 'source': source}
    need = sorted({m for n in files for m in pe_imports(os.path.join(d, n))} & set(MSVC))
    if not a.msvc_dir:
        die(f'--msvc-dir: the pack carries the MSVC runtime its DLLs import ({need}): give the VS redist folder')
    for n in need:
        src = os.path.join(a.msvc_dir, n)
        st, subj = sig_of(src)
        if st != 'Valid' or 'O=Microsoft Corporation' not in subj:
            die(f'{src}: signature {st} by {subj!r}: the MSVC runtime must be Microsoft-signed')
        shutil.copy2(src, os.path.join(d, n))
        with open(src, 'rb') as f:
            data = f.read()
        desc[n] = {'version': file_version(src), 'sha256': hashlib.sha256(data).hexdigest(), 'size': len(data), 'signature': st, 'signer': subj,
                   'source': os.path.abspath(src)}
    # the MSVC runtime must be at least the toolset the ONNX Runtime DLLs were linked with
    for n in ('onnxruntime.dll', 'onnxruntime_providers_cuda.dll'):
        b = files[n][0]
        pe = struct.unpack_from('<I', b, 0x3C)[0]
        link = [b[pe + 24 + 2], b[pe + 24 + 3]]
        for m in need:
            if [int(x) for x in desc[m]['version'].split('.')[:2]] < link:
                die(f'{m} is {desc[m]["version"]}, older than the toolset {link[0]}.{link[1]} {n} was linked with')
    for n, (data, comp, source) in msvc_licenses(a.msvc_dir, [desc[m]['version'] for m in need]).items():
        licenses[n] = {'sha256': hashlib.sha256(data).hexdigest(), 'size': len(data), 'component': comp, 'source': [source], 'data': data}
    taken = [n.lower() for n in [*desc, 'runtime.json', *licenses]]
    if len(set(taken)) != len(taken):
        die(f'two files of the pack share a name: {sorted(taken)}')
    for n, x in licenses.items():
        with open(os.path.join(d, n), 'wb') as f:
            f.write(x.pop('data'))
    vendor, min_driver, provider = CUDA_GPU
    rj = {'runtime': pack, 'onnxruntime': ort_version, 'provider': 'cpu+cuda', 'providers': ['cpu', provider],
          'gpu': {'vendor': vendor, 'min_driver': min_driver, 'providers': [provider]}, 'wheels': wheels, 'files': desc, 'licenses': licenses}
    # loaded through a release worker: every DLL from the pack, the CUDA provider there; the self-test on this PC
    worker = a.check_worker or cargo_build('release')
    info = json.loads(subprocess.run([worker, 'info', '--runtime-dir', d], capture_output=True, text=True, check=True).stdout)
    ort = info['onnxruntime']
    loaded = {n: p for n, p in ort.get('modules', {}).items() if p}
    if 'error' in ort or os.path.normcase(os.path.dirname(loaded.get('onnxruntime.dll') or '')) != os.path.normcase(os.path.abspath(d)):
        die(f'the pack did not load from itself: {ort}')
    if 'CUDAExecutionProvider' not in ort.get('available', []):
        die(f'the pack\'s ONNX Runtime has no CUDA provider: {ort.get("available")}')
    env = dict(os.environ, PATH=os.path.abspath(d) + os.pathsep + os.environ.get('PATH', ''))  # cuDNN loads its parts by name
    # the self-test on a real network (--check-net: a zero-onnx/1 export's model.onnx + reference.npz): the built-in probe
    # graph fails CUDA's priors (2.4e-3, duplicate scatter slots) where real exports pass at ~1e-6 (zero-farm PROTOCOL 9.6)
    net = ['--model', os.path.join(a.check_net, 'model.onnx'), '--reference', os.path.join(a.check_net, 'reference.npz')] if a.check_net else []
    pr = subprocess.run([worker, 'probe-providers', '--runtime-dir', d, *net, '--providers', 'cuda,cpu', '--reps', '2'], capture_output=True, text=True, env=env)
    probe = json.loads(pr.stdout) if pr.returncode == 0 and pr.stdout.strip().startswith('{') else None
    if not probe or 'cpu' not in probe.get('providers', []):
        die(f'probe-providers through the pack: {pr.stdout[-500:]} {pr.stderr[-800:]}')
    if 'cuda' not in probe['providers'] and not a.no_gpu_check:
        die(f'the CUDA provider did not pass on this PC (--no-gpu-check to sign anyway): {pr.stderr[-1500:]}')
    rj['ort_build_info'] = ort['version']
    rj['check'] = {'available': ort.get('available'), 'providers': probe['providers'], 'graph': probe.get('graph'),
                   'results': [{k: r.get(k) for k in ('provider', 'gpu', 'ok', 'why', 'rows_per_s', 'cpu_fallback_ops')} for r in probe.get('results', [])]}
    with open(os.path.join(d, 'runtime.json'), 'w', encoding='utf-8', newline='') as f:
        json.dump(rj, f, indent=1)
    print(f'runtime pack {pack}: ONNX Runtime {ort_version} ({", ".join(ort.get("available", []))}), every DLL from the pack; '
          f'probe: {probe["providers"]}; {sum(x["size"] for x in desc.values()) / 1e6:.0f} MB of DLLs')
    min_agent = a.min_agent if git_semver_ge(a.min_agent, ALT_MIN_AGENT) else ALT_MIN_AGENT
    bid = sign_publish(a, d, ['--kind', 'runtime', '--version', pack, '--min-agent', min_agent, '--gpu-vendor', vendor, '--gpu-min-driver', min_driver,
                              '--gpu-providers', provider, '--zst-level', str(a.zst_level)])
    return bid, d


def git_semver_ge(a, b):
    p = lambda v: [int(x) for x in v.split('-')[0].split('+')[0].split('.')]
    return p(a) >= p(b)


def resign_worker(a):
    """A signed worker build's executable signed again for another runtime pack (zero-farm PROTOCOL.md 9.1: one
    signed worker manifest per pack): the same engine and core commits, profile, aw, zero_onnx and entry, the new
    `requires_runtime`, `min_agent` at least the old one's -> (build_id, directory)."""
    src = a.resign
    print(farm_release(a, 'verify', src))
    with open(os.path.join(src, 'manifest.json'), encoding='utf-8') as f:
        m = json.load(f)
    if m.get('kind') != 'worker' or not a.runtime_id:
        die(f'--resign takes a worker build directory and --runtime-id (the other pack); {src} is a {m.get("kind")} build')
    entry = m['entry']
    version = f'{m["version"]}-rt{a.runtime_id[:8]}'
    d = os.path.join(a.out or os.path.join(target_dir(), 'dist'), version)
    if os.path.exists(d):
        shutil.rmtree(d)
    os.makedirs(d)
    shutil.copy2(os.path.join(src, entry), os.path.join(d, entry))
    if file_sha256(os.path.join(d, entry)) != next(x['sha256'] for x in m['files'] if x['path'] == entry):
        die(f'{entry}: not the bytes {src}\'s manifest names')
    old_min = m.get('min_agent') or MIN_AGENT
    min_agent = a.min_agent if git_semver_ge(a.min_agent, old_min) else old_min
    sign = ['--kind', 'worker', '--version', version, '--game', m['game'], '--engine-commit', m['engine_commit'], '--core-commit', m['core_commit'],
            '--profile', m['profile'], '--aw', str(m['aw']), '--zero-onnx', ','.join(str(x) for x in m.get('zero_onnx') or []),
            '--min-agent', min_agent, '--entry', entry, '--requires-runtime', a.runtime_id]
    bid = sign_publish(a, d, sign)
    info = json.loads(subprocess.run([os.path.join(d, entry), 'info', '--ort-dll', 'none'], capture_output=True, text=True, check=True).stdout)
    if info['build_id'] != bid:
        die(f'the signed worker says build {info["build_id"]}, want {bid}')
    return bid, d


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    g = ap.add_mutually_exclusive_group(required=True)
    g.add_argument('--profile', choices=('release', 'fuzz'))
    g.add_argument('--runtime', action='store_true', help='the runtime pack (ONNX Runtime + its MSVC runtime)')
    g.add_argument('--resign', metavar='BUILD_DIR', help='sign a signed worker build\'s executable again for --runtime-id (PROTOCOL 9.1)')
    g.add_argument('--publish-dir', metavar='BUILD_DIR', help='publish a signed build directory (verified first)')
    ap.add_argument('--root', help='the checkout to build (default: this one): a clean worktree at the commit to release')
    ap.add_argument('--runtime-id', help='release builds: the runtime pack build_id they load ONNX Runtime from')
    ap.add_argument('--pack', choices=('cpu', 'winml', 'cuda'), default='cpu',
                    help='--runtime: the CPU pack (default), the Windows ML GPU pack or the CUDA pack (NVIDIA, PROTOCOL 9)')
    ap.add_argument('--wheel-dir', help='--pack cuda: where the PyPI wheels are cached (default <target>/farm-build/wheels)')
    ap.add_argument('--zst-level', type=int, default=19, help='--pack cuda: the zstd level of the compressed copies (PROTOCOL 9.3)')
    ap.add_argument('--no-gpu-check', action='store_true', help='--pack cuda: sign even if the CUDA provider fails on this PC')
    ap.add_argument('--check-net', help='--pack cuda: a directory with a real export (model.onnx + reference.npz) for the CUDA '
                    'self-test (the built-in probe graph fails CUDA)')
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
    ap.add_argument('--farm-release', default=FARM_RELEASE)
    ap.add_argument('--diagnostic', action='store_true',
                    help='a diagnostic release worker: ZERO_PROBE_VARIANTS names the probe variants zero-core embeds '
                         '(they may hold real networks); its version gets a -diag suffix; for canary test runs only')
    a = ap.parse_args()
    if a.root:
        global ROOT
        ROOT = os.path.abspath(a.root)
    check_variants_env(a)
    if a.publish_dir:
        print(farm_release(a, 'verify', a.publish_dir))
        print(farm_release(a, 'publish', a.publish_dir, '--url', a.url, token=True))
        print(f'published to {a.url}')
        print(f'build directory {a.publish_dir}')
        with open(os.path.join(a.publish_dir, 'manifest.json'), 'rb') as f:
            print(f'build_id {hashlib.sha256(f.read()).hexdigest()[:24]}')
        return
    if a.resign:
        bid, d = resign_worker(a)
    elif a.runtime:
        bid, d = {'winml': build_winml, 'cuda': build_cuda}.get(a.pack, build_runtime)(a)
    else:
        bid, d = build_worker(a)
    print(f'build directory {d}')
    if a.publish:
        print(farm_release(a, 'publish', d, '--url', a.url, token=True))
        print(f'published to {a.url}')
    print(f'build_id {bid}')


if __name__ == '__main__':
    main()
