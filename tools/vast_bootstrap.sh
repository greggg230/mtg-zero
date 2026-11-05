#!/usr/bin/env bash
# Turn a fresh rented Linux GPU box (a PyTorch + CUDA Docker image, root) into an MTG Zero machine:
# run over ssh by tools/vast.py prep (`bash -s`, with RUST_TOOLCHAIN set). Idempotent: a second run only
# checks. Based on dominion-zero tools/box_bootstrap.sh. Prints @@sections: @@apt @@rust @@pip @@torch @@sysinfo
# Needs: apt (build-essential, pkg-config, curl, git), rustup (minimal profile, the desktop's toolchain),
# numpy + psutil + pytest + flask for the image's python. The image's python (pytorch/pytorch 2.11 images:
# /usr/bin/python, PEP 668 "externally managed", no conda) takes pip's --break-system-packages (a throwaway
# container) and --ignore-installed for Debian-owned packages pip cannot uninstall (blinker).
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
export PATH="/opt/conda/bin:$HOME/.cargo/bin:$HOME/.local/bin:/usr/local/bin:$PATH"
TOOLCHAIN="${RUST_TOOLCHAIN:-stable}"
SUDO=""; [ "$(id -u)" = 0 ] || SUDO="sudo"

echo "@@apt"
need=""
for c in cc make pkg-config curl git tar; do command -v "$c" >/dev/null 2>&1 || need="$need $c"; done
if [ -n "$need" ]; then
  t0=$(date +%s)
  $SUDO apt-get update -qq >/tmp/mtgz_apt.log 2>&1 || { tail -5 /tmp/mtgz_apt.log; exit 3; }
  $SUDO apt-get install -y -qq --no-install-recommends build-essential pkg-config curl ca-certificates git \
      >>/tmp/mtgz_apt.log 2>&1 || { tail -5 /tmp/mtgz_apt.log; exit 3; }
  echo "installed:$need in $(( $(date +%s) - t0 ))s"
else
  echo "present"
fi

echo "@@rust"
if ! command -v cargo >/dev/null 2>&1 || ! rustup toolchain list 2>/dev/null | grep -q "^${TOOLCHAIN}"; then
  t0=$(date +%s)
  if ! command -v rustup >/dev/null 2>&1; then
    curl -sSf https://sh.rustup.rs -o /tmp/rustup.sh
    sh /tmp/rustup.sh -y -q --profile minimal --default-toolchain "$TOOLCHAIN" >/tmp/mtgz_rustup.log 2>&1 \
      || { tail -5 /tmp/mtgz_rustup.log; exit 4; }
  else
    rustup toolchain install -q --profile minimal "$TOOLCHAIN" >/tmp/mtgz_rustup.log 2>&1 || { tail -5 /tmp/mtgz_rustup.log; exit 4; }
    rustup default "$TOOLCHAIN" >/dev/null
  fi
  echo "installed in $(( $(date +%s) - t0 ))s"
fi
rustc --version

echo "@@pip"
PY="$(command -v python || command -v python3)"
if ! "$PY" -c "import numpy, psutil, pytest, flask" 2>/dev/null; then
  PIPX=""
  "$PY" -m pip install --help 2>/dev/null | grep -q -- --break-system-packages && PIPX="--break-system-packages"
  "$PY" -m pip install -q --disable-pip-version-check --root-user-action=ignore $PIPX --ignore-installed blinker \
      numpy psutil pytest flask >/tmp/mtgz_pip.log 2>&1 || { tail -5 /tmp/mtgz_pip.log; exit 5; }
fi
"$PY" -c "import numpy, psutil, pytest; print('python', '$PY', 'numpy', numpy.__version__, 'psutil', psutil.__version__, 'pytest', pytest.__version__)"

echo "@@torch"
"$PY" - <<'EOF'
import torch
ok = torch.cuda.is_available()
print('torch', torch.__version__, 'cuda', torch.version.cuda, 'available', ok)
if ok:
    x = torch.randn(1024, 1024, device='cuda')
    print('device', torch.cuda.get_device_name(0), 'capability', torch.cuda.get_device_capability(0),
          'matmul ok', float((x @ x).sum().item()) == float((x @ x).sum().item()))
EOF

echo "@@sysinfo"
"$PY" - <<'EOF'
import json, os, shutil, platform
d = {'python': platform.python_version(), 'nproc': os.cpu_count(), 'home': os.path.expanduser('~'),
     'disk_free_gb': round(shutil.disk_usage(os.path.expanduser('~')).free / 2**30, 1)}
try:
    d['affinity'] = len(os.sched_getaffinity(0))
except Exception:
    pass
for line in open('/proc/cpuinfo'):
    if line.startswith('model name'):
        d['cpu_name'] = line.split(':', 1)[1].strip()
        break
for line in open('/proc/meminfo'):
    if line.startswith('MemTotal'):
        d['ram_gb'] = round(int(line.split()[1]) / 2**20, 1)
print(json.dumps(d))
EOF
