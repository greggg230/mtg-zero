"""Build, sign and publish zero-farm builds of this checkout (docs/ENGINE.md 114.3; zero-farm PROTOCOL.md 2).

    python tools/farm_build.py --profile release|fuzz [--runtime-id BUILD_ID] [--root CHECKOUT] [--publish] [--url URL]
    python tools/farm_build.py --profile release|fuzz --platform macos-arm64 [--runtime-id ID] [--root CHECKOUT] [--publish]
    python tools/farm_build.py --runtime --pack macos-cpu [--ort-version V] [--check-worker MAC_PATH] [--publish]
    python tools/farm_build.py --runtime [--ort-dll PATH] [--msvc-dir DIR] [--publish] [--url URL]
    python tools/farm_build.py --runtime --pack winml [--winml-version V | --winml-nupkg F] [--publish] [--url URL]
    python tools/farm_build.py --runtime --pack dml [--gpu-vendor amd] [--winml-version V | --winml-nupkg F] [--publish] [--url URL]
    python tools/farm_build.py --runtime --pack cuda --msvc-dir DIR [--check-net EXPORT_DIR] [--publish] [--url URL]
    python tools/farm_build.py --runtime --pack torch --msvc-dir DIR [--no-gpu-check] [--no-sign] [--publish] [--url URL]
    python tools/farm_build.py --torch --runtime-id TORCH_PACK_ID --pyshim zf-pyshim.exe [--check-runtime PACK_DIR] [--root CHECKOUT]
        [--no-sign] [--publish]
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

The DirectML pack (`--runtime --pack dml`; zero-farm PROTOCOL rev 22, AMD GPUs through DirectML only): the same package's
ONNX Runtime and DirectML.dll with its license files, WITHOUT Microsoft.Windows.AI.MachineLearning.dll - no
execution-provider catalog, so no vendor provider (AMD's `amdgpu` / `migraphx` among them) can be registered through
it: CPU and DirectML only. Its manifest requires a GPU of `--gpu-vendor` (default `amd`; PROTOCOL 9.2 `gpu`:
`{"vendor", "providers": ["dml"]}`, min_agent at least 0.2.10), so the coordinator leases it only to PCs with such an
adapter and run alternates take their `when.vendor` from it; another vendor (a test on an NVIDIA PC) makes a separate
pack. Version `dml-<package version>-ort-<ONNX Runtime>` (+ `-<vendor>` for a vendor other than amd). Checked as the
Windows ML pack, plus: the worker's probe through it reports no catalog.

macOS on Apple silicon (zero-farm PROTOCOL.md 11, docs/ENGINE.md 197), on the maintainer's Mac (`MTGZ_MAC` in
~/.config/zero/local.env, as tools/mac.sh): `--platform macos-arm64` builds `mtgz-worker` there at exactly the clean
checkout's commit - the commit pushed to the Mac's mirror (`mac` remote, refs/heads/farm-build-<sha12>) and the zero-core
pin to its core mirror (tools/mac.sh's), checked out forced and clean in the Mac's one farm checkout
~/mtg-zero-wt/farm-build (HEAD and the submodule checked there), built with that cargo profile under `nice -n 15` in its
own target dir ~/cargo-target/mtg-zero/farm-build with no RUSTFLAGS (mac.sh's `-undefined dynamic_lookup` is for the
PyO3 module: a farm executable must resolve every symbol at link time) - then copied back and checked here (a thin
arm64 Mach-O executable whose only libraries are /usr/lib's and /System's: `MAC_DYLIB_PREFIXES`; an ad-hoc code
signature, which the arm64 linker adds) and there (`codesign --verify`, `info`, `fuzz-check`). Its build directory holds
`mtgz-worker` (no extension), signed with `farm-release sign ... --platform macos-arm64 --entry mtgz-worker --min-agent
0.3.0` (PROTOCOL.md 11.3: a manifest that names a platform needs agents from 0.3.0), then the signed manifest is
copied next to the executable on the Mac and `info` / `fuzz-check` read it there (build_id, commit, profile). Signed
here, never on the Mac (the release key stays on this machine).

The macOS runtime pack (`--runtime --pack macos-cpu`): ONNX Runtime's official osx-arm64 library from the NuGet package
Microsoft.ML.OnnxRuntime (`--ort-version`, default MAC_ORT_VERSION = the Windows CPU pack's 1.31.1; the GitHub release
tarballs are not reachable here): the package checked against nuget.org's catalog hash (SHA-512), its nuspec's version
and license file, `runtimes/osx-arm64/native/libonnxruntime.dylib` (the library itself, the name the worker loads from a
runtime_dir) and, unmodified, the package's `LICENSE` and `ThirdPartyNotices.txt` (byte for byte the Windows CPU
pack's). The dylib is checked here (a thin arm64 Mach-O dylib, its LC_BUILD_VERSION minimum macOS recorded) and on the
Mac (`codesign --verify --strict`, signed by Microsoft's Developer ID team MAC_ORT_TEAM), then loaded through a macOS
release worker (`--check-worker`: a path on the Mac; default the release farm build of this checkout, built there):
`info --runtime-dir` (the library loaded from the pack) and `probe-providers --providers cpu` (the CPU passes its
self-test). `runtime.json` like the Windows pack's (each file's version, signer, sha256, source; the licenses; the
package), signed `--kind runtime --platform macos-arm64 --min-agent 0.3.0`.

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
WINML_CATALOG_DLL = 'Microsoft.Windows.AI.MachineLearning.dll'    # (the provider catalog: left out of the DirectML pack)
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
# macOS (PROTOCOL.md 11): what a farm worker may link (the system's own libraries), the agents that know platforms,
# the Mac's farm checkout and target dir (relative to its home), the ONNX Runtime of the macOS pack (NuGet) and the
# Developer ID team that signs Microsoft's macOS binaries
MAC_DYLIB_PREFIXES = ('/usr/lib/', '/System/Library/')
PLATFORM_MIN_AGENT = '0.3.0'
MAC_FARM_CHECKOUT = 'mtg-zero-wt/farm-build'
MAC_FARM_TARGET = 'cargo-target/mtg-zero/farm-build'
MAC_ORT_PACKAGE = 'microsoft.ml.onnxruntime'
MAC_ORT_VERSION = '1.31.1'
MAC_ORT_TEAM = 'UBF8T346G9'
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


def build_dml(a):
    """The DirectML runtime pack (module docs: the Windows ML pack without its provider catalog) -> (build_id,
    directory)."""
    return build_winml(a, catalog=False)


def build_winml(a, catalog=True):
    """The Windows ML runtime pack (module docs) -> (build_id, directory); `catalog` False: the DirectML pack (no
    Microsoft.Windows.AI.MachineLearning.dll)."""
    files = WINML_FILES if catalog else tuple(n for n in WINML_FILES if n != WINML_CATALOG_DLL)
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
        pack = f'{"winml" if catalog else "dml"}-{version}-ort-{ort_version or "unknown"}'
        if not catalog and (a.gpu_vendor or 'amd') != 'amd':
            pack += f'-{a.gpu_vendor}'
        d = os.path.join(a.out or os.path.join(tdir, 'dist'), pack)
        if os.path.exists(d):
            shutil.rmtree(d)
        os.makedirs(d)
        for n in files:
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
    for n in files:
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
    rj = {'runtime': pack, 'onnxruntime': ort_version or desc['onnxruntime.dll']['version'],
          'provider': 'cpu+dml+winml' if catalog else 'cpu+dml',
          'providers': ['cpu', 'dml', 'tensorrt_rtx', 'amdgpu', 'migraphx', 'openvino'] if catalog else ['cpu', 'dml'],
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
    if not catalog and (probe.get('catalog') or {}).get('present'):
        die(f'the DirectML pack reports a provider catalog: {probe.get("catalog")}')
    rj['ort_build_info'] = ort['version']
    rj['check'] = {'available': ort.get('available'), 'cpu_selftest_max_abs': next(r['selftest']['max_abs'] for r in probe['results'] if r['provider'] == 'cpu')}
    with open(os.path.join(d, 'runtime.json'), 'w', encoding='utf-8') as f:
        json.dump(rj, f, indent=1)
    print(f'runtime pack {pack}: ONNX Runtime {rj["onnxruntime"]} ({", ".join(ort.get("available", []))}), every DLL from the pack')
    extra = []
    min_agent = a.min_agent
    if not catalog:
        # (PROTOCOL 9.2: a pack with `gpu` needs agents from 0.2.10)
        rj['gpu'] = {'vendor': a.gpu_vendor or 'amd', 'providers': ['dml']}
        with open(os.path.join(d, 'runtime.json'), 'w', encoding='utf-8') as f:
            json.dump(rj, f, indent=1)
        min_agent = max(min_agent, '0.2.10', key=lambda v: tuple(int(x) for x in v.split('.')))
        extra = ['--gpu-vendor', a.gpu_vendor or 'amd', '--gpu-providers', 'dml']
    bid = sign_publish(a, d, ['--kind', 'runtime', '--version', pack, '--min-agent', min_agent] + extra)
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


# ---------------------------------------------------------------------- the torch runtime (docs/ENGINE.md 241)
# The torch runtime pack (zero-farm's per-machine torch runtime; zero-core docs/az.md 78): Windows' embeddable CPython at
# the minor version `mtgz/_engine.pyd` is built for (cp313), torch's CUDA build and the third-party modules the torch
# worker's play path imports (measured: numpy, typing_extensions; torch's wheel brings torchgen and functorch), each from
# its publisher, pinned by sha256.
TORCH_PYTHON = ('3.13.14', 'https://www.python.org/ftp/python/3.13.14/python-3.13.14-embed-amd64.zip',
                '90b4e5b9898b72d744650524bff92377c367f44bd5fbd09e3148656c080ad907')
TORCH_WHEEL = ('torch', '2.11.0+cu128', 'cp313-cp313-win_amd64', 'https://download.pytorch.org/whl/cu128/torch-2.11.0%2Bcu128-cp313-cp313-win_amd64.whl',
               '6f367e62fd81b75cdf23ca4b75ced834d2db2cf98d1588ac935bde345de9de23')
TORCH_PYPI = (('numpy', '2.3.5', 'cp313-cp313-win_amd64'), ('typing_extensions', '4.15.0', 'py3-none-any'))
TORCH_CUDA = '12.8'
# the pack's hard requirement (PROTOCOL 9.2, 19.1): an NVIDIA GPU whose driver is CUDA 12.8's (570.65: torch cu128's
# toolkit, Blackwell included), the torch provider, and the owner's opt-in (zero-farm's per-machine `opt_in`)
TORCH_GPU = ('nvidia', '570.65', 'torch-cuda')
TORCH_OPT_IN = 'torch'
TORCH_MIN_AGENT = '0.3.19'                # (zero-farm PROTOCOL rev 27, 19: agents that read opt-in packs; sign --opt-in needs it)
# what a pack leaves out of the wheels: nothing the worker loads - C++ headers and import libraries for building
# extensions, CMake files, type stubs, a protobuf compiler, bytecode caches, test suites (every importable package stays:
# an opt-in manifest may list 20,000 files in 8 MiB, zero-farm PROTOCOL 19.1)
TORCH_PRUNE_DIRS = ('torch/include/', 'torch/share/', 'torch/bin/', 'numpy/_pyinstaller/')
TORCH_PRUNE_PARTS = ('/tests/',)
TORCH_PRUNE_EXT = ('.lib', '.pyi', '.h', '.hpp', '.cuh')
MANIFEST_MAX = 8 << 20                  # an opt-in runtime manifest's limits (zero-farm PROTOCOL 19.1): 8 MiB, 20,000 files
MANIFEST_MAX_FILES = 20000
# a path segment the coordinator takes (zero-farm: [A-Za-z0-9._-]; torch's dist-info is `torch-2.11.0+cu128.dist-info`)
SEGMENT_OK = set('ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._-')
# the checkout's data files the game package reads at import and in play (measured with an audit hook on a real job,
# docs/ENGINE.md 241): the decklists a spec names, EoE's card list
TORCH_BUILD_DATA = ('data/decks/', 'engine/src/az/eoe_cards.txt')
PROBE = r'''
import json, sys, time
import torch
ok = torch.cuda.is_available()
out = {"python": sys.version.split()[0], "torch": torch.__version__, "cuda": torch.version.cuda,
       "cudnn": torch.backends.cudnn.version(), "available": ok, "devices": torch.cuda.device_count() if ok else 0}
if ok:
    x = torch.randn(512, 512, device="cuda")
    y = (x @ x).sum().item()
    out.update(device=torch.cuda.get_device_name(0), matmul_ok=y == y, capability=list(torch.cuda.get_device_capability(0)))
print(json.dumps(out))
'''


def fetch(url, sha256, path):
    """`url` into `path` (cached), checked against `sha256` -> path."""
    import urllib.request
    if os.path.exists(path) and file_sha256(path) == sha256:
        return path
    os.makedirs(os.path.dirname(path), exist_ok=True)
    print(f'fetching {url}', flush=True)
    tmp = path + '.part'
    with urllib.request.urlopen(url, timeout=3600) as r, open(tmp, 'wb') as f:
        shutil.copyfileobj(r, f, 1 << 20)
    if file_sha256(tmp) != sha256:
        die(f'{url}: the download is not the pinned sha256 {sha256}')
    os.replace(tmp, path)
    return path


def unpack_wheel(path, what, dest, prune=True):
    """A wheel's members into `dest` (site-packages), each checked against its RECORD; the pack's prunes left out ->
    (files, bytes, [license members])."""
    import csv
    import io
    import zipfile
    n = size = 0
    lic = []
    with zipfile.ZipFile(path) as z:
        rec = next(m for m in z.namelist() if m.endswith('.dist-info/RECORD'))
        rows = {r[0]: r[1] for r in csv.reader(io.TextIOWrapper(z.open(rec), encoding='utf-8')) if len(r) >= 2}
        for m in z.namelist():
            if m.endswith('/') or '__pycache__/' in m:
                continue
            if prune and (m.startswith(TORCH_PRUNE_DIRS) or m.lower().endswith(TORCH_PRUNE_EXT)
                          or any(x in '/' + m for x in TORCH_PRUNE_PARTS)):
                continue
            data = z.read(m) if m == rec else wheel_member(z, rows, m, what)
            # (a dist-info directory named with the local version's '+', which the coordinator refuses in a path:
            # '-' instead; its METADATA keeps the version, nothing imports by the directory's name)
            parts = m.split('/')
            if parts[0].endswith('.dist-info'):
                parts[0] = parts[0].replace('+', '-')
            out = os.path.join(dest, *parts)
            os.makedirs(os.path.dirname(out), exist_ok=True)
            with open(out, 'wb') as f:
                f.write(data)
            n += 1
            size += len(data)
            low = m.lower()
            if '.dist-info/' in low and ('license' in low or 'notice' in low or 'copying' in low):
                lic.append(m)
    return n, size, lic


def build_torch_pack(a):
    """The torch runtime pack (module docs, docs/ENGINE.md 241) -> (build_id, directory)."""
    import zipfile
    tdir = target_dir()
    cache = a.wheel_dir or os.path.join(tdir, 'wheels')
    pyv, pyurl, pysha = TORCH_PYTHON
    tname, tver, ttag, turl, tsha = TORCH_WHEEL
    pack = f'torch-{tver.split("+")[0]}-cu{TORCH_CUDA.replace(".", "")}-py{"".join(pyv.split(".")[:2])}'
    d = os.path.join(a.out or os.path.join(tdir, 'dist'), pack)
    if os.path.exists(d):
        shutil.rmtree(d)
    py = os.path.join(d, 'python')
    sp = os.path.join(py, 'Lib', 'site-packages')
    os.makedirs(sp)
    # the embeddable CPython: as python.org ships it, its ._pth naming the stdlib zip, its own directory and the pack's
    # site-packages (no `import site`: nothing of the PC's Python reaches it; zf-pyshim runs it with -I too)
    zp = fetch(pyurl, pysha, os.path.join(cache, os.path.basename(pyurl)))
    with zipfile.ZipFile(zp) as z:
        z.extractall(py)
    st, subj = sig_of(os.path.join(py, 'python.exe'))
    if st != 'Valid' or 'Python Software Foundation' not in subj:
        die(f'python.exe: signature {st} by {subj!r}: expected the Python Software Foundation\'s')
    pth = os.path.join(py, f'python{"".join(pyv.split(".")[:2])}._pth')
    with open(pth, 'w', encoding='ascii', newline='\r\n') as f:
        f.write(f'python{"".join(pyv.split(".")[:2])}.zip\n.\nLib\\site-packages\n')
    sources = [{'component': 'python', 'version': pyv, 'url': pyurl, 'sha256': pysha}]
    wp = fetch(turl, tsha, os.path.join(cache, f'{tname}-{tver}-{ttag}.whl'))
    stats = {'torch': unpack_wheel(wp, f'{tname} {tver}', sp)}
    sources.append({'component': tname, 'version': tver, 'url': turl, 'sha256': tsha})
    for name, ver, tag in TORCH_PYPI:
        p, src = pypi_wheel(name, ver, tag, cache)
        stats[name] = unpack_wheel(p, f'{name} {ver}', sp)
        sources.append({'component': name, 'version': ver, **src})
    # the MSVC runtime torch's DLLs import (app-local next to python.exe; at least the toolset they were linked with)
    lib = os.path.join(sp, 'torch', 'lib')
    need = sorted({m for f in os.listdir(lib) if f.endswith('.dll') for m in pe_imports(os.path.join(lib, f))} & set(MSVC))
    msvc = {}
    if need:
        if not a.msvc_dir:
            die(f'--msvc-dir: torch\'s DLLs import {need}: give the VS redist folder (the pack carries them app-local)')
        for n in need:
            src = os.path.join(a.msvc_dir, n)
            st, subj = sig_of(src)
            if st != 'Valid' or 'O=Microsoft Corporation' not in subj:
                die(f'{src}: signature {st} by {subj!r}: the MSVC runtime must be Microsoft-signed')
            shutil.copy2(src, os.path.join(py, n))
            msvc[n] = {'version': file_version(src), 'sha256': file_sha256(src), 'source': os.path.abspath(src)}
        with open(os.path.join(lib, 'torch_cpu.dll'), 'rb') as f:
            b = f.read(4096)
        pe = struct.unpack_from('<I', b, 0x3C)[0]
        link = [b[pe + 24 + 2], b[pe + 24 + 3]]
        for n, x in msvc.items():
            if [int(v) for v in x['version'].split('.')[:2]] < link:
                die(f'{n} is {x["version"]}, older than the toolset {link[0]}.{link[1]} torch was linked with')
        for n, (data, comp, source) in msvc_licenses(a.msvc_dir, [x['version'] for x in msvc.values()]).items():
            with open(os.path.join(d, n), 'wb') as f:
                f.write(data)
    # the probe (the agent's, contract section 3): the pack's own Python, isolated, imports torch and multiplies on the GPU
    # (-B: no bytecode written into the pack - its files are exactly the signed ones)
    pr = subprocess.run([os.path.join(py, 'python.exe'), '-I', '-B', '-X', 'utf8', '-c', PROBE], capture_output=True, text=True, cwd=d)
    probe = json.loads(pr.stdout.strip().splitlines()[-1]) if pr.returncode == 0 and pr.stdout.strip() else None
    if not probe:
        die(f'the pack\'s probe failed: {pr.stdout[-500:]} {pr.stderr[-1500:]}')
    if not probe.get('available') and not a.no_gpu_check:
        die(f'torch sees no CUDA device through the pack on this PC (--no-gpu-check to sign anyway): {probe}')
    files = [os.path.relpath(os.path.join(r, f), d).replace('\\', '/') for r, _, fs in os.walk(d) for f in fs]
    bad = [f for f in files if len(f) > 260 or any(not seg or set(seg) - SEGMENT_OK for seg in f.split('/'))]
    if bad:
        die(f'{len(bad)} path(s) the coordinator refuses ([A-Za-z0-9._-] per segment, at most 260 characters): {bad[:8]}')
    total = sum(os.path.getsize(os.path.join(d, f)) for f in files)
    est = sum(len(f) + 230 for f in files) + 4096
    vendor, min_driver, provider = TORCH_GPU
    rj = {'runtime': pack, 'python': pyv, 'torch': probe['torch'], 'cuda': probe['cuda'], 'cudnn': probe['cudnn'],
          'numpy': dict((n, v) for n, v, _ in TORCH_PYPI)['numpy'], 'provider': provider, 'providers': [provider],
          'gpu': {'vendor': vendor, 'min_driver': min_driver, 'providers': [provider]}, 'opt_in': TORCH_OPT_IN,
          'entry_python': 'python/python.exe', 'sources': sources, 'msvc': msvc,
          'unpacked': {k: {'files': v[0], 'bytes': v[1], 'licenses': v[2]} for k, v in stats.items()},
          'files': len(files), 'bytes': total, 'manifest_estimate': est, 'check': probe}
    with open(os.path.join(d, 'runtime.json'), 'w', encoding='utf-8', newline='') as f:
        json.dump(rj, f, indent=1)
    print(f'runtime pack {pack}: Python {pyv}, torch {probe["torch"]} (CUDA {probe["cuda"]}, cuDNN {probe["cudnn"]}), '
          f'{len(files)} files, {total / 1e9:.2f} GB; probe {probe}')
    if est > MANIFEST_MAX * 0.9 or len(files) > MANIFEST_MAX_FILES:
        die(f'{len(files)} files: the signed manifest would be ~{est:,} bytes, near its {MANIFEST_MAX:,}-byte limit '
            f'({MANIFEST_MAX_FILES:,} files at most)')
    if a.no_sign:
        print('--no-sign: the directory is built and checked, not signed')
        return None, d
    min_agent = a.min_agent if git_semver_ge(a.min_agent, TORCH_MIN_AGENT) else TORCH_MIN_AGENT
    bid = sign_publish(a, d, ['--kind', 'runtime', '--version', pack, '--min-agent', min_agent, '--gpu-vendor', vendor,
                              '--gpu-min-driver', min_driver, '--gpu-providers', provider, '--opt-in', TORCH_OPT_IN,
                              '--zst-level', str(a.zst_level)])
    with open(os.path.join(d, 'manifest.json'), 'rb') as f:
        n = len(f.read())
    if n > MANIFEST_MAX:
        die(f'manifest.json is {n:,} bytes: over the {MANIFEST_MAX:,}-byte limit agents read')
    return bid, d


def tracked(root, prefix):
    """The tracked files under `prefix` of the checkout `root` (relative, '/'), the committed ones only."""
    out = subprocess.run(['git', 'ls-files', '-z', '--', prefix], cwd=root, capture_output=True, check=True).stdout
    return [p for p in out.decode('utf-8').split('\0') if p]


def build_torch_worker(a):
    """The torch worker build (module docs, docs/ENGINE.md 241) -> (build_id or None with --no-sign, directory)."""
    ec, cc = clean_tree()
    if not a.pyshim or not os.path.isfile(a.pyshim):
        die(f'--pyshim: zero-farm\'s zf-pyshim.exe (the build\'s entry), not {a.pyshim!r}')
    if not a.runtime_id and not a.no_sign:
        die('--torch: give --runtime-id (the torch runtime pack the build requires)')
    # the engine module at exactly this commit (tools/build_py.sh: the artifact cache, else cargo) - what the worker imports
    env = dict(os.environ)
    env.pop('ZERO_CORE_UNPINNED', None)
    subprocess.run(['bash', os.path.join(ROOT, 'tools', 'build_py.sh')], cwd=ROOT, env=env, check=True)
    pyd = os.path.join(ROOT, 'mtgz', '_engine.pyd')
    version = f'mtg-{ec[:7]}-torch'
    d = os.path.join(a.out or os.path.join(target_dir(), 'dist'), version)
    if os.path.exists(d):
        shutil.rmtree(d)
    os.makedirs(d)
    core = os.path.join(ROOT, 'third_party', 'zero-core')
    n = 0
    for rel in tracked(ROOT, 'mtgz'):
        if rel.endswith('.py') or '/' in rel and not rel.endswith(('.pyd', '.so')):
            dst = os.path.join(d, *rel.split('/'))
            os.makedirs(os.path.dirname(dst), exist_ok=True)
            shutil.copy2(os.path.join(ROOT, rel), dst)
            n += 1
    for rel in tracked(core, 'python/zero'):
        if rel.endswith('.pyd') or rel.endswith('.so'):
            continue
        dst = os.path.join(d, 'third_party', 'zero-core', *rel.split('/'))      # (a checkout's layout: mtgz finds its core)
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        shutil.copy2(os.path.join(core, rel), dst)
        n += 1
    for rel in [r for x in TORCH_BUILD_DATA for r in tracked(ROOT, x)]:
        dst = os.path.join(d, *rel.split('/'))
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        shutil.copy2(os.path.join(ROOT, rel), dst)
        n += 1
    shutil.copy2(pyd, os.path.join(d, 'mtgz', '_engine.pyd'))
    shutil.copy2(os.path.join(ROOT, 'tools', 'torch_worker_main.py'), os.path.join(d, 'worker_main.py'))
    shutil.copy2(a.pyshim, os.path.join(d, 'zf-pyshim.exe'))
    print(f'torch worker {version}: {n} files (Python, the package data), mtgz/_engine.pyd, worker_main.py, zf-pyshim.exe', flush=True)
    if a.no_sign:
        bid = None
    else:
        sign = ['--kind', 'worker', '--version', version, '--game', 'mtg', '--engine-commit', ec, '--core-commit', cc,
                '--profile', 'release', '--aw', '1', '--zero-onnx', '', '--min-agent',
                a.min_agent if git_semver_ge(a.min_agent, TORCH_MIN_AGENT) else TORCH_MIN_AGENT,
                '--entry', 'zf-pyshim.exe', '--requires-runtime', a.runtime_id]
        bid = sign_publish(a, d, sign)
    # the build answers `info` as signed: the runtime pack's Python when given (--check-runtime), else this one
    rt = a.check_runtime
    python = [os.path.join(rt, 'python', 'python.exe'), '-I'] if rt else [sys.executable]
    info = json.loads(subprocess.run([*python, '-X', 'utf8', os.path.join(d, 'worker_main.py'), 'info'], capture_output=True,
                                     text=True, check=True, cwd=d).stdout)
    want = bid or 'unsigned-dev'
    if info['build_id'] != want or 'rolling' not in info['features'] or info['kinds'] != ['selfplay']:
        die(f'the torch worker says {info} (want build {want}, kinds [selfplay], rolling)')
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


# ------------------------------------------------------------------------------------------- macOS (PROTOCOL.md 11)

def macho_info(path):
    """A thin 64-bit Mach-O file -> {cputype, filetype, dylibs, rpaths, minos, signed}: its header and load commands
    (LC_LOAD_DYLIB and its weak / re-export / upward kinds, LC_RPATH, LC_BUILD_VERSION, LC_CODE_SIGNATURE)."""
    with open(path, 'rb') as f:
        b = f.read()
    magic = struct.unpack_from('<I', b, 0)[0]
    if magic != 0xFEEDFACF:
        die(f'{path}: not a thin 64-bit Mach-O file (magic {magic:#x}; a universal binary is not what the pack ships)')
    cputype, _sub, filetype, ncmds, _size, _flags, _res = struct.unpack_from('<iiIIIII', b, 4)
    out = {'cputype': cputype, 'filetype': filetype, 'dylibs': [], 'rpaths': [], 'minos': None, 'signed': False}
    off = 32
    for _ in range(ncmds):
        cmd, size = struct.unpack_from('<II', b, off)
        if cmd in (0xC, 0x80000018, 0x8000001F, 0x80000023):
            name = struct.unpack_from('<I', b, off + 8)[0]
            out['dylibs'].append(b[off + name:off + size].split(b'\0')[0].decode())
        elif cmd == 0x8000001C:
            name = struct.unpack_from('<I', b, off + 8)[0]
            out['rpaths'].append(b[off + name:off + size].split(b'\0')[0].decode())
        elif cmd == 0x32:
            v = struct.unpack_from('<I', b, off + 12)[0]
            out['minos'] = f'{v >> 16}.{(v >> 8) & 0xFF}.{v & 0xFF}'
        elif cmd == 0x1D:
            out['signed'] = True
        off += size
    return out


CPU_TYPE_ARM64 = 0x0100000C


def check_mac_executable(exe):
    """The macOS worker as a farm executable must be (module docs) -> its Mach-O info."""
    m = macho_info(exe)
    if m['cputype'] != CPU_TYPE_ARM64 or m['filetype'] != 2:
        die(f'{exe}: cputype {m["cputype"]:#x}, filetype {m["filetype"]}: want an arm64 executable')
    bad = [d for d in m['dylibs'] if not d.startswith(MAC_DYLIB_PREFIXES)]
    if bad or m['rpaths']:
        die(f'{exe} links {bad} (rpaths {m["rpaths"]}): a farm worker links only the system\'s libraries {MAC_DYLIB_PREFIXES}')
    if not m['signed']:
        die(f'{exe}: no code signature (the arm64 linker signs ad hoc; macOS kills an unsigned arm64 executable)')
    return m


def mac_host():
    h = os.environ.get('MTGZ_MAC') or local_config.get('MTGZ_MAC')
    if not h:
        die('--platform macos-arm64 builds on the Mac: set MTGZ_MAC=user@host in ~/.config/zero/local.env (tools/local.env.example)')
    return h


def mac_run(script, what, timeout=None):
    """A bash script on the Mac (sent on stdin; `set -euo pipefail`, the Mac's cargo and Homebrew on PATH) -> its
    stdout; a failure dies with its output's end."""
    head = 'set -euo pipefail\nexport PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin"\n'
    # (bytes: text mode on Windows would send CRLF line ends, which bash on the Mac reads as part of each command)
    p = subprocess.run(['ssh', '-o', 'BatchMode=yes', mac_host(), 'bash -s'], input=(head + script).encode(), capture_output=True,
                       timeout=timeout)
    out, err = p.stdout.decode('utf-8', 'replace'), p.stderr.decode('utf-8', 'replace')
    if p.returncode:
        die(f'on the Mac, {what} (exit {p.returncode}):\n{(out + err)[-3000:]}')
    return out


def mac_copy(src, dst, back=False):
    """scp a file or directory to the Mac (`dst` relative to its home), or back (`back`: `src` on the Mac)."""
    args = ['scp', '-q', '-o', 'BatchMode=yes', '-r']
    args += [f'{mac_host()}:{src}', dst] if back else [src, f'{mac_host()}:{dst}']
    p = subprocess.run(args, capture_output=True, text=True)
    if p.returncode:
        die(f'scp {"from" if back else "to"} the Mac: {p.stderr.strip()[-500:]}')


def mac_json(text, what):
    for ln in reversed(text.splitlines()):
        if ln.startswith('{'):
            try:
                return json.loads(ln)
            except ValueError:
                pass
    die(f'{what}: no JSON in its output:\n{text[-1500:]}')


def mac_build(profile, ec, cc):
    """mtgz-worker built on the Mac at exactly `ec` (zero-core `cc`) with cargo profile `profile` (module docs) -> the
    executable's path on the Mac, relative to its home."""
    host = mac_host()
    tag = f'farm-build-{ec[:12]}'
    mac_run('[ -d mtg-zero.git ] || git init -q --bare mtg-zero.git\n[ -d mtg-zero-core.git ] || git init -q --bare mtg-zero-core.git\n',
            'the mirrors')
    git('push', '-q', '-f', f'{host}:mtg-zero.git', f'{ec}:refs/heads/{tag}')
    git('push', '-q', '-f', f'{host}:mtg-zero-core.git', f'{cc}:refs/heads/pin-{cc}', cwd=os.path.join(ROOT, 'third_party', 'zero-core'))
    exe = f'{MAC_FARM_TARGET}/{profile}/mtgz-worker'
    print(f'building mtgz-worker (profile {profile}) at {ec[:12]} on the Mac in ~/{MAC_FARM_CHECKOUT} (target ~/{MAC_FARM_TARGET})',
          flush=True)
    out = mac_run(f"""
[ -d "$HOME/mtg-zero/.git" ] || git clone -q --no-checkout "$HOME/mtg-zero.git" "$HOME/mtg-zero"
cd "$HOME/mtg-zero"
git config submodule.third_party/zero-core.url "$HOME/mtg-zero-core.git"
git fetch -q origin "+refs/heads/{tag}:refs/remotes/origin/{tag}"
WT="$HOME/{MAC_FARM_CHECKOUT}"
LOCK="$WT.mac-lock"
mkdir -p "$(dirname "$WT")"
while ! mkdir "$LOCK" 2>/dev/null; do
  OWNER="$(cat "$LOCK/pid" 2>/dev/null || true)"
  if [ -n "$OWNER" ] && ! kill -0 "$OWNER" 2>/dev/null; then rm -rf "$LOCK"; continue; fi
  sleep 10
done
echo $$ > "$LOCK/pid"
trap 'rm -rf "$LOCK"' EXIT
[ -d "$WT" ] || git worktree add -q --detach "$WT" {ec}
cd "$WT"
git checkout -q -f --detach {ec}
git clean -fdq
git -c protocol.file.allow=always submodule update -q --init --force third_party/zero-core
git -C third_party/zero-core clean -fdq
test "$(git rev-parse HEAD)" = {ec}
test "$(git -C third_party/zero-core rev-parse HEAD)" = {cc}
test -z "$(git status --porcelain --untracked-files=no --ignore-submodules=none)"
unset RUSTFLAGS CARGO_ENCODED_RUSTFLAGS
export CARGO_TARGET_DIR="$HOME/{MAC_FARM_TARGET}" CARGO_BUILD_JOBS="${{CARGO_BUILD_JOBS:-6}}"
nice -n 15 cargo build --profile {profile} -p mtgz_worker 2>&1 | tail -3
EXE="$HOME/{exe}"
codesign --verify "$EXE"
file "$EXE"
otool -L "$EXE"
""", f'the build at {ec[:12]}', timeout=6 * 3600)
    print(out.strip()[-1500:])
    return exe


def mac_worker_json(exe_rel, *args, what=None, env=''):
    """`<worker on the Mac> args...` -> its last JSON line (exit status not checked: probe-providers exits 2 when not
    even the CPU passes, which the caller judges)."""
    q = ' '.join("'" + x.replace("'", "'\\''") + "'" for x in args)
    out = mac_run(f'cd "$HOME" && {env} "$HOME/{exe_rel}" {q} || true\n', what or f'{exe_rel} {args[0]}')
    return mac_json(out, what or args[0])


def build_worker_mac(a):
    """The macOS release / fuzz worker build (module docs) -> (build_id, directory)."""
    ec, cc = clean_tree()
    exe_rel = mac_build(a.profile, ec, cc)
    tdir = target_dir()
    version = f'mtg-{ec[:7]}-{a.profile}-macos-arm64'
    d = os.path.join(a.out or os.path.join(tdir, 'dist'), version)
    if os.path.exists(d):
        shutil.rmtree(d)
    os.makedirs(d)
    exe = os.path.join(d, 'mtgz-worker')
    mac_copy(exe_rel, exe, back=True)
    m = check_mac_executable(exe)
    print(f'mtgz-worker (macOS): arm64 executable, minimum macOS {m["minos"]}, links {m["dylibs"]}, ad-hoc signed')
    info = mac_worker_json(exe_rel, 'info', '--ort-dll', 'none', what='info')
    if info.get('platform') != 'macos-arm64':
        die(f'the Mac\'s worker says platform {info.get("platform")!r}')
    zo = info['zero_onnx']
    sign = ['--kind', 'worker', '--version', version, '--game', 'mtg', '--engine-commit', ec, '--core-commit', cc,
            '--profile', a.profile, '--aw', '1', '--zero-onnx', ','.join(map(str, zo)) if a.profile == 'release' else '',
            '--min-agent', max_semver(a.min_agent, PLATFORM_MIN_AGENT), '--entry', 'mtgz-worker', '--platform', 'macos-arm64']
    if a.profile == 'release':
        if not a.runtime_id:
            die('a release build plays networks: give --runtime-id (the macOS runtime pack it loads ONNX Runtime from)')
        sign += ['--requires-runtime', a.runtime_id]
    bid = sign_publish(a, d, sign)
    # the signed build on the Mac: its info names the build, its fuzz-check the commit and profile
    remote = f'mtgz-macfarm/builds/{bid}'
    mac_run(f'rm -rf "$HOME/{remote}" && mkdir -p "$HOME/{remote}"\n', 'the build directory')
    for n in os.listdir(d):
        mac_copy(os.path.join(d, n), f'{remote}/{n}')
    mac_run(f'chmod 755 "$HOME/{remote}/mtgz-worker"\n', 'chmod')
    info = mac_worker_json(f'{remote}/mtgz-worker', 'info', '--ort-dll', 'none', what='info (signed)')
    want_kinds = ['selfplay', 'eval', 'fuzz'] if a.profile == 'release' else ['fuzz']
    if info['build_id'] != bid or info['kinds'] != want_kinds or info.get('zero_onnx') != zo:
        die(f'the signed worker on the Mac says build {info["build_id"]}, kinds {info["kinds"]}, zero_onnx {info.get("zero_onnx")} '
            f'(want {bid}, {want_kinds}, {zo})')
    chk = mac_worker_json(f'{remote}/mtgz-worker', 'fuzz-check', '--mode', 'games', what='fuzz-check (signed)')
    if chk.get('commit') != ec or chk.get('profile') != a.profile:
        die(f'fuzz-check of the signed worker on the Mac: {chk}')
    print(f'signed macOS worker on the Mac: ~/{remote} (build {bid}, commit {ec[:12]}, profile {a.profile})')
    return bid, d


def max_semver(x, y):
    return x if git_semver_ge(x, y) else y


def nuget_package(name, version, cache_dir):
    """The NuGet package `name` `version` from nuget.org, cached in `cache_dir`, checked against nuget.org's catalog
    (its packageHash, SHA-512) -> (path, {url, sha256, sha512, catalog})."""
    import base64
    import urllib.request
    reg = f'https://api.nuget.org/v3/registration5-gz-semver2/{name}/{version}.json'
    req = urllib.request.Request(reg, headers={'Accept-Encoding': 'gzip'})
    with urllib.request.urlopen(req, timeout=60) as r:
        body = r.read()
        if r.headers.get('Content-Encoding') == 'gzip' or body[:2] == b'\x1f\x8b':
            import gzip
            body = gzip.decompress(body)
    leaf = json.loads(body)
    with urllib.request.urlopen(leaf['catalogEntry'], timeout=60) as r:
        cat = json.load(r)
    if cat.get('packageHashAlgorithm') != 'SHA512':
        die(f'{name} {version}: the catalog\'s hash is {cat.get("packageHashAlgorithm")}')
    url = leaf['packageContent']
    os.makedirs(cache_dir, exist_ok=True)
    path = os.path.join(cache_dir, f'{name}.{version}.nupkg')
    sha512 = lambda p: base64.b64encode(hashlib.sha512(open(p, 'rb').read()).digest()).decode()
    if not (os.path.exists(path) and sha512(path) == cat['packageHash']):
        print(f'fetching {url}', flush=True)
        tmp = path + '.part'
        with urllib.request.urlopen(url, timeout=3600) as r, open(tmp, 'wb') as f:
            shutil.copyfileobj(r, f, 1 << 20)
        if sha512(tmp) != cat['packageHash']:
            die(f'{url}: not the package nuget.org\'s catalog names (SHA-512)')
        os.replace(tmp, path)
    return path, {'url': url, 'sha256': file_sha256(path), 'sha512': cat['packageHash'], 'catalog': leaf['catalogEntry'],
                  'published': cat.get('published')}


def build_mac_runtime(a):
    """The macOS runtime pack (module docs) -> (build_id, directory)."""
    import io
    import zipfile
    version = a.ort_version or MAC_ORT_VERSION
    tdir = target_dir()
    path, src = nuget_package(MAC_ORT_PACKAGE, version, a.wheel_dir or os.path.join(tdir, 'nuget'))
    member = 'runtimes/osx-arm64/native/libonnxruntime.dylib'
    with zipfile.ZipFile(path) as z:
        nuspec = next(n for n in z.namelist() if n.endswith('.nuspec') and '/' not in n)
        spec = z.read(nuspec).decode('utf-8-sig', 'replace')
        if f'<version>{version}</version>' not in spec:
            die(f'{nuspec} is not version {version}')
        if '<license type="file">LICENSE</license>' not in spec:
            die(f'{nuspec}: the package no longer names LICENSE as its license (look at what it ships now)')
        dylib = z.read(member)
        lic = {n: z.read(n) for n in ORT_LICENSE_FILES}
    pack = f'ort-{version}-cpu-macos-arm64'
    d = os.path.join(a.out or os.path.join(tdir, 'dist'), pack)
    if os.path.exists(d):
        shutil.rmtree(d)
    os.makedirs(d)
    with open(os.path.join(d, 'libonnxruntime.dylib'), 'wb') as f:
        f.write(dylib)
    for n, data in lic.items():
        with open(os.path.join(d, n), 'wb') as f:
            f.write(data)
    m = macho_info(os.path.join(d, 'libonnxruntime.dylib'))
    if m['cputype'] != CPU_TYPE_ARM64 or m['filetype'] != 6:
        die(f'{member}: cputype {m["cputype"]:#x}, filetype {m["filetype"]}: want an arm64 dylib')
    bad = [x for x in m['dylibs'] if not x.startswith(MAC_DYLIB_PREFIXES) and not x.startswith('@rpath/libonnxruntime')]
    if bad:
        die(f'{member} links {bad}: the pack carries ONNX Runtime alone (the system\'s libraries only)')
    # on the Mac: Microsoft's signature, then loaded through a macOS release worker
    worker = a.check_worker
    if not worker:
        ec, cc = clean_tree()
        worker = mac_build('release', ec, cc)
    remote = f'mtgz-macfarm/packs/{pack}'
    mac_run(f'rm -rf "$HOME/{remote}" && mkdir -p "$HOME/{remote}"\n', 'the pack directory')
    for n in os.listdir(d):
        mac_copy(os.path.join(d, n), f'{remote}/{n}')
    sig = mac_run(f'cd "$HOME/{remote}" && codesign --verify --strict libonnxruntime.dylib && codesign -dv --verbose=2 libonnxruntime.dylib 2>&1\n',
                  'codesign of the dylib')
    team = next((ln.split('=', 1)[1].strip() for ln in sig.splitlines() if ln.startswith('TeamIdentifier=')), None)
    auth = next((ln.split('=', 1)[1].strip() for ln in sig.splitlines() if ln.startswith('Authority=')), None)
    if team != MAC_ORT_TEAM:
        die(f'libonnxruntime.dylib: signed by team {team!r} ({auth}), not Microsoft\'s {MAC_ORT_TEAM}')
    info = mac_worker_json(worker, 'info', '--runtime-dir', f'{remote}', what='info --runtime-dir')
    ort = info['onnxruntime']
    loaded = (ort.get('modules') or {}).get('libonnxruntime.dylib') or ''
    if 'error' in ort or not loaded.endswith(f'/{remote}/libonnxruntime.dylib'):
        die(f'the pack did not load from itself on the Mac: {ort}')
    probe = mac_worker_json(worker, 'probe-providers', '--runtime-dir', f'{remote}', '--providers', 'cpu', '--reps', '0',
                            what='probe-providers --providers cpu')
    if probe.get('providers') != ['cpu']:
        die(f'probe-providers --providers cpu through the pack on the Mac: {json.dumps(probe)[:1500]}')
    data = open(os.path.join(d, 'libonnxruntime.dylib'), 'rb').read()
    files = {'libonnxruntime.dylib': {'version': version, 'sha256': hashlib.sha256(data).hexdigest(), 'size': len(data),
                                      'signature': 'Developer ID (codesign --verify --strict)', 'signer': auth, 'team': team,
                                      'minos': m['minos'], 'source': f'{os.path.basename(path)}/{member}'}}
    licenses = {n: {'sha256': hashlib.sha256(x).hexdigest(), 'size': len(x), 'component': 'ONNX Runtime',
                    'source': f'{os.path.basename(path)}/{n}'} for n, x in lic.items()}
    rj = {'runtime': pack, 'onnxruntime': version, 'provider': 'cpu', 'platform': 'macos-arm64', 'minos': m['minos'],
          'nuget': {'package': MAC_ORT_PACKAGE, 'version': version, **src}, 'files': files, 'licenses': licenses,
          'ort_build_info': ort.get('version'),
          'check': {'available': ort.get('available'),
                    'cpu_selftest_max_abs': next(r['selftest']['max_abs'] for r in probe['results'] if r['provider'] == 'cpu')}}
    with open(os.path.join(d, 'runtime.json'), 'w', encoding='utf-8', newline='\n') as f:
        json.dump(rj, f, indent=1)
    print(f'runtime pack {pack}: ONNX Runtime {version} for macOS {m["minos"]}+ ({", ".join(ort.get("available", []))}), signed by '
          f'{auth}; loaded from the pack on the Mac, the CPU self-test passed')
    bid = sign_publish(a, d, ['--kind', 'runtime', '--version', pack, '--min-agent', max_semver(a.min_agent, PLATFORM_MIN_AGENT),
                              '--platform', 'macos-arm64'])
    mac_copy(os.path.join(d, 'runtime.json'), f'{remote}/runtime.json')
    for n in ('manifest.json', 'manifest.json.minisig'):
        mac_copy(os.path.join(d, n), f'{remote}/{n}')
    return bid, d


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    g = ap.add_mutually_exclusive_group(required=True)
    g.add_argument('--profile', choices=('release', 'fuzz'))
    g.add_argument('--runtime', action='store_true', help='the runtime pack (ONNX Runtime + its MSVC runtime)')
    g.add_argument('--resign', metavar='BUILD_DIR', help='sign a signed worker build\'s executable again for --runtime-id (PROTOCOL 9.1)')
    g.add_argument('--publish-dir', metavar='BUILD_DIR', help='publish a signed build directory (verified first)')
    g.add_argument('--torch', action='store_true', help='the torch worker build (docs/ENGINE.md 241): the Python of the commit, '
                   'its engine module and zf-pyshim.exe, for the torch runtime pack (--runtime-id)')
    ap.add_argument('--root', help='the checkout to build (default: this one): a clean worktree at the commit to release')
    ap.add_argument('--runtime-id', help='release builds: the runtime pack build_id they load ONNX Runtime from')
    ap.add_argument('--pack', choices=('cpu', 'winml', 'dml', 'cuda', 'macos-cpu', 'torch'), default='cpu',
                    help='--runtime: the CPU pack (default), the Windows ML GPU pack, the CUDA pack (NVIDIA, PROTOCOL 9) or the '
                         'macOS CPU pack (Apple silicon, PROTOCOL 11)')
    ap.add_argument('--platform', choices=('windows-x64', 'macos-arm64'), default='windows-x64',
                    help='--profile: the worker\'s platform (macos-arm64: built on the Mac, PROTOCOL 11)')
    ap.add_argument('--ort-version', help=f'--pack macos-cpu: the Microsoft.ML.OnnxRuntime version (default {MAC_ORT_VERSION})')
    ap.add_argument('--wheel-dir', help='--pack cuda: where the PyPI wheels are cached (default <target>/farm-build/wheels)')
    ap.add_argument('--zst-level', type=int, default=19, help='--pack cuda: the zstd level of the compressed copies (PROTOCOL 9.3)')
    ap.add_argument('--no-gpu-check', action='store_true', help='--pack cuda: sign even if the CUDA provider fails on this PC')
    ap.add_argument('--check-net', help='--pack cuda: a directory with a real export (model.onnx + reference.npz) for the CUDA '
                    'self-test (the built-in probe graph fails CUDA)')
    ap.add_argument('--pyshim', help='--torch: the zf-pyshim.exe of zero-farm (the entry of the build)')
    ap.add_argument('--check-runtime', help='--torch: a torch runtime pack directory whose Python checks the build')
    ap.add_argument('--no-sign', action='store_true', help='--torch / --pack torch: build and check the directory, sign nothing')
    ap.add_argument('--gpu-vendor', choices=('amd', 'nvidia', 'intel'), help='--pack dml: the GPU vendor its manifest requires (default amd)')
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
        bid, d = {'winml': build_winml, 'dml': build_dml, 'cuda': build_cuda, 'macos-cpu': build_mac_runtime,
                  'torch': build_torch_pack}.get(a.pack, build_runtime)(a)
    elif a.torch:
        bid, d = build_torch_worker(a)
    elif a.platform == 'macos-arm64':
        bid, d = build_worker_mac(a)
    else:
        bid, d = build_worker(a)
    print(f'build directory {d}')
    if a.publish and bid is None:
        die('--publish: an unsigned build (--no-sign) is never published')
    if a.publish:
        print(farm_release(a, 'publish', d, '--url', a.url, token=True))
        print(f'published to {a.url}')
    print(f'build_id {bid}')


if __name__ == '__main__':
    main()
