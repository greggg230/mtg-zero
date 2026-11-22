#!/usr/bin/env bash
# Run a command on the MacBook (Apple M4 Max: 12 performance + 4 efficiency cores, 64 GB, no CUDA)
# against this checkout's COMMITTED HEAD: cargo tests, fuzz runs, search smokes, pytest (no GPU),
# while the desktop trains. The engine's results are bit-identical to the desktop's (ENGINE.md 44).
#
#   bash tools/mac.sh cargo test --release --no-fail-fast
#   bash tools/mac.sh 'FUZZ_GAMES=20000 FUZZ_MATCHES=2000 cargo test --release -p engine --test fuzz'
#   bash tools/mac.sh 'bash tools/build_py.sh && python -m pytest -q tests'
#   bash tools/mac.sh 'bash tools/build_py.sh && python tools/search_smoke.py --specs random,match --seeds 0-1'
#   bash tools/mac.sh --eval-checkout     # a run's remote evaluation: this commit's checkout (below)
#
# How: pushes HEAD to the Mac's bare repo ~/mtg-zero.git (git remote `mac`) as refs/heads/<branch>
# and zero-core's pinned commit to ~/mtg-zero-core.git (the Mac has no GitHub access to the core,
# so the submodule's URL there is this mirror), checks the commit out (detached) in
# ~/mtg-zero-wt/<branch> (~/mtg-zero for main), runs `git submodule update --init` there, and runs
# the command in it with Rust (~/.cargo/bin), the venv ~/.venvs/dominion-zero (torch CPU/MPS,
# numpy, pytest, flask, psutil) first on PATH, CARGO_TARGET_DIR=~/cargo-target/mtg-zero/<branch>
# (one per checkout, never shared) and RUSTFLAGS with macOS's link flags for the PyO3 module
# (`-undefined dynamic_lookup`: every command gets them, so a target dir never rebuilds for them).
# Uncommitted changes are NOT sent: commit first (a WIP commit is fine). The command travels
# base64-encoded (quotes, pipes and semicolons arrive intact). The Mac: MTGZ_MAC=user@host, the machine's own
# setting in ~/.config/zero/local.env (tools/local.env.example; the environment wins), never committed.
# Commands of the SAME commit share a Mac checkout and run at once (a gate's parallel steps, several gates of one
# commit: docs/ENGINE.md 238); a command of another commit waits until the checkout is free, then checks its own out
# (a command that waits goes before later arrivals). --pin, --eval-checkout and MTGZ_MAC_EXCLUSIVE=1 take the
# checkout alone. How many run at once on the Mac is tools/slotd.sh's there, for the callers that use it (the gate);
# the "[mac] <host> <branch> @ <sha>" line and the "[mac] done @ <sha> (exit N)" line after the command
# carry the Mac checkout's own HEAD, read there (tools/gate.py checks both; docs/ENGINE.md 143).
#
# --eval-checkout: the checkout a run's remote evaluation uses (run setting eval_remote 'mac',
# mtgz/az/mac.py): ~/mtg-zero-wt/run-<sha12> at exactly this HEAD (the working tree must be clean:
# the run's code is a commit), its engine built (tools/build_py.sh) and stamped with the commit.
# Run it in the run's checkout (e.g. its pinned worktree) before the run starts with eval_remote.
#
# --pin NAME: a checkout for a long job on the Mac (ENGINE.md 50: the match-noise study):
# ~/mtg-zero-wt/NAME-<sha12> at exactly this HEAD (clean tree), its engine built (target dir
# ~/cargo-target/mtg-zero/NAME-<sha12>); later mac.sh commands never check anything out there, so a
# job started in it keeps its code and its engine module while the branch's own checkout moves on
# (--pin again at the same commit re-installs the module there: not while its job runs).
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
. "$ROOT/tools/local_env.sh"
HOST="${MTGZ_MAC:-}"
[ -n "$HOST" ] || { echo "$(basename "$0"): no Mac configured: set MTGZ_MAC=user@host in ~/.config/zero/local.env (tools/local.env.example)" >&2; exit 2; }
cd "$ROOT"
[ $# -gt 0 ] || { sed -n 2,36p "$0"; exit 2; }

SHA="$(git rev-parse HEAD)"
PIN="$(git rev-parse HEAD:third_party/zero-core)"            # zero-core's pinned commit
# What keeps this checkout from being its commit: tracked changes, and untracked files where a build, a test or a
# run picks them up (mtgz/checkout.py's rule, docs/ENGINE.md 143: under engine/ mtgz/ tests/ tools/ card_ir/ py/
# data/ third_party/ .cargo/, the root's *.py *.rs *.toml *.cfg *.ini *.lock, anything untracked in the submodule).
# Neither reaches the Mac (it gets the commit), so a desktop run importing them would run other code than the Mac.
not_the_commit() {
  git status --short --untracked-files=no
  git ls-files --others --exclude-standard -- engine mtgz tests tools card_ir py data third_party .cargo \
    ':(glob)*.py' ':(glob)*.rs' ':(glob)*.toml' ':(glob)*.cfg' ':(glob)*.ini' ':(glob)*.lock' | sed 's/^/?? /'
  if [ -e third_party/zero-core/.git ]; then
    git -C third_party/zero-core ls-files --others --exclude-standard | sed 's|^|?? third_party/zero-core/|'
  fi
}
DIRTY="$(not_the_commit)"
if [ "$1" = --pin ]; then
  [ $# -eq 2 ] || { echo "mac.sh: --pin NAME" >&2; exit 2; }
  if [ -n "$DIRTY" ]; then
    echo "mac.sh --pin: the checkout is not exactly its commit (a pinned checkout is a commit):" >&2
    printf '%s\n' "$DIRTY" >&2
    exit 1
  fi
  BRANCH="$2-${SHA:0:12}"
  CMD='set -euo pipefail
bash tools/build_py.sh
echo "pinned checkout ready: $PWD (target $CARGO_TARGET_DIR)"'
elif [ "$1" = --eval-checkout ]; then
  [ $# -eq 1 ] || { echo "mac.sh: --eval-checkout takes no command" >&2; exit 2; }
  if [ -n "$DIRTY" ]; then
    echo "mac.sh --eval-checkout: the checkout is not exactly its commit (a run's remote evaluation needs its exact code):" >&2
    printf '%s\n' "$DIRTY" >&2
    exit 1
  fi
  BRANCH="run-${SHA:0:12}"
  # the stamp (in the worktree's git dir) says this checkout's engine was built at this commit
  CMD='set -euo pipefail
STAMP="$(git rev-parse --git-path mtgz-engine-commit)"; rm -f "$STAMP"
bash tools/build_py.sh
git rev-parse HEAD > "$STAMP"
python -m mtgz.az.remote check '"$SHA"'
echo "eval checkout ready: $PWD"'
else
  BRANCH="$(git rev-parse --abbrev-ref HEAD)"
  [ "$BRANCH" = HEAD ] && BRANCH="detached-${SHA:0:9}"
  if [ -n "$DIRTY" ]; then
    echo "mac.sh: warning: uncommitted changes and untracked files are not sent (running $SHA):" >&2
    printf '%s\n' "$DIRTY" | head -8 >&2
  fi
  if [ $# -eq 1 ]; then CMD="$1"; else CMD="$(printf '%q ' "$@")"; fi
fi
SAFE="$(printf '%s' "$BRANCH" | tr '/' '_')"
case "$1" in
  --pin|--eval-checkout) MODE=excl ;;
  *) if [ "${MTGZ_MAC_EXCLUSIVE:-0}" = 1 ]; then MODE=excl; else MODE=shared; fi ;;
esac

CORE="$ROOT/third_party/zero-core"
if ! git -C "$CORE" cat-file -e "$PIN^{commit}" 2>/dev/null; then
  echo "mac.sh: zero-core's pinned ${PIN:0:12} is not in third_party/zero-core (bash tools/build_py.sh initialises it)" >&2
  exit 1
fi
ssh -o BatchMode=yes "$HOST" '[ -d mtg-zero.git ] || git init -q --bare mtg-zero.git
[ -d mtg-zero-core.git ] || git init -q --bare mtg-zero-core.git'
git remote get-url mac >/dev/null 2>&1 || git remote add mac "$HOST:mtg-zero.git" 2>/dev/null \
  || git remote get-url mac >/dev/null          # (another mac.sh of this checkout added it meanwhile)
# several mac.sh of one checkout push the same refs at once (a gate's parallel steps): a push that lost the race for
# the ref's lock is tried again
push_retry() {
  local k err
  for k in 1 2 3 4 5; do
    if err="$("$@" 2>&1)"; then return 0; fi
    sleep $((k * 2))
  done
  printf '%s
' "$err" >&2
  "$@"
}
push_retry git push -q -f mac "HEAD:refs/heads/$BRANCH"
push_retry git -C "$CORE" push -q "$HOST:mtg-zero-core.git" "$PIN:refs/heads/pin-$PIN"

# Ship the command base64-encoded so quotes / pipes / semicolons in it reach the Mac intact
# (it is spliced into the remote script below).
CMD_B64="$(printf '%s' "$CMD" | base64 | tr -d '\n')"
ssh -o BatchMode=yes "$HOST" "bash -s" <<EOF
set -euo pipefail
export PATH="\$HOME/.venvs/dominion-zero/bin:\$HOME/.cargo/bin:/opt/homebrew/bin:\$PATH"
if [ "$BRANCH" = main ]; then WT="\$HOME/mtg-zero"; else WT="\$HOME/mtg-zero-wt/$SAFE"; fi
# Who may use the checkout (docs/ENGINE.md 143, 238). A second mac.sh once force-switched the sources under a running
# cargo test, so a command of ANOTHER commit waits for the checkout; commands of the SAME commit share it and run at
# once. \$USERS holds one file per running command, named by its pid: "<sha> <shared|excl> <what>"; a file whose
# process is gone is dropped. \$LOCK (a directory holding its owner's pid; one whose owner is gone is taken over) is
# held only while a command looks at \$USERS and syncs the checkout - an older mac.sh holds it for its whole command,
# and is waited for. \$WANT: the commands waiting for the checkout; a later arrival that could share goes after them.
# (bash 3.2: macOS's own)
LOCK="\$WT.mac-lock"
USERS="\$WT.mac-users"
WANT="\$WT.mac-want"
ME=\$\$
HOLD=""
mkdir -p "\$USERS" "\$WANT"
cleanup() {
  rm -f "\$USERS/\$ME" "\$WANT/\$ME"
  if [ -n "\$HOLD" ]; then rm -rf "\$LOCK"; fi
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
take_lock() {
  local owner said=""
  while ! mkdir "\$LOCK" 2>/dev/null; do
    [ -d "\$LOCK" ] || { echo "[mac] cannot create the lock \$LOCK" >&2; exit 1; }
    owner="\$(cat "\$LOCK/pid" 2>/dev/null || true)"
    if [ -n "\$owner" ] && ! kill -0 "\$owner" 2>/dev/null; then
      echo "[mac] \$WT: taking over the lock of pid \$owner (gone)" >&2; rm -rf "\$LOCK"; continue
    fi
    if [ -z "\$owner" ] && [ -n "\$(find "\$LOCK" -maxdepth 0 -mmin +2 2>/dev/null)" ]; then
      echo "[mac] \$WT: taking over a lock without an owner" >&2; rm -rf "\$LOCK"; continue
    fi
    if [ -z "\$said" ] && [ -f "\$LOCK/what" ]; then       # (an older mac.sh: it holds the lock for its command)
      echo "[mac] \$WT is in use (\$(cat "\$LOCK/what" 2>/dev/null || echo '?'), pid \${owner:-?}): waiting for it" >&2
      said=1
    fi
    sleep 1
  done
  HOLD=1
  echo \$ME > "\$LOCK/pid"
}
drop_lock() { rm -rf "\$LOCK"; HOLD=""; }
live() {        # live <dir>: the pids of its files whose process is alive (the others' files are dropped)
  local f p
  for f in "\$1"/*; do
    [ -e "\$f" ] || continue
    p="\${f##*/}"
    if kill -0 "\$p" 2>/dev/null; then echo "\$p"; else rm -f "\$f"; fi
  done
}
SAID=""
WAITED=""
while :; do
  take_lock
  OTHERS=""
  CLASH=""
  for p in \$(live "\$USERS"); do
    [ "\$p" = "\$ME" ] && continue
    OSHA=""; OMODE=""; OWHAT=""
    read -r OSHA OMODE OWHAT < "\$USERS/\$p" || true
    OTHERS=1
    if [ "$MODE" = excl ] || [ "\$OMODE" != shared ] || [ "\$OSHA" != "$SHA" ]; then CLASH="\$OWHAT, pid \$p"; fi
  done
  if [ -z "\$CLASH" ] && [ -n "\$OTHERS" ] && [ -z "\$WAITED" ]; then
    for p in \$(live "\$WANT"); do
      [ "\$p" = "\$ME" ] || CLASH="\$(cat "\$WANT/\$p" 2>/dev/null || echo '?'), pid \$p, waits for it and goes first"
    done
  fi
  [ -z "\$CLASH" ] && break
  printf '%s\n' "$BRANCH ${SHA:0:12}" > "\$WANT/\$ME"
  WAITED=1
  drop_lock
  if [ -z "\$SAID" ]; then
    echo "[mac] \$WT is in use (\$CLASH): waiting for it" >&2
    SAID=1
  fi
  sleep 5
done
rm -f "\$WANT/\$ME"
if [ -z "\$OTHERS" ]; then
  # nobody runs in the checkout: bring it to this commit (the bare repos' refs, config and worktree list are written
  # under the lock too: several mac.sh arrive at once)
  if [ ! -d "\$HOME/mtg-zero/.git" ]; then
    git clone -q --no-checkout "\$HOME/mtg-zero.git" "\$HOME/mtg-zero"
  fi
  cd "\$HOME/mtg-zero"
  git config submodule.third_party/zero-core.url "\$HOME/mtg-zero-core.git"
  git fetch -q origin "+refs/heads/$BRANCH:refs/remotes/origin/$BRANCH"
  if [ "$BRANCH" != main ] && [ ! -d "\$WT" ]; then git worktree add -q --detach "\$WT" $SHA; fi
  cd "\$WT" && git checkout -q -f --detach $SHA
  git -c protocol.file.allow=always submodule update -q --init third_party/zero-core
else
  # commands of this commit run here: the checkout is theirs, untouched (its HEAD is checked, never moved)
  cd "\$WT"
  AT="\$(git rev-parse HEAD)"
  if [ "\$AT" != "$SHA" ]; then
    echo "[mac] \$WT is at \$AT while commands of $SHA run in it: refusing" >&2; exit 1
  fi
fi
printf '%s\n' "$SHA $MODE $BRANCH ${SHA:0:12} since \$(date '+%Y-%m-%d %H:%M:%S')" > "\$USERS/\$ME"
drop_lock
export CARGO_TARGET_DIR="\$HOME/cargo-target/mtg-zero/$SAFE" PYTHON=python PYO3_PYTHON=python
export CARGO_BUILD_JOBS=12 RUSTFLAGS="-C link-arg=-undefined -C link-arg=dynamic_lookup"
CMD="\$(printf '%s' '$CMD_B64' | base64 --decode)"
# the Mac checkout's own HEAD, read there - before and after the command (tools/gate.py wants both = the gated sha)
printf '[mac] %s %s @ %s: %s\n' "\$(hostname -s)" "$BRANCH" "\$(git rev-parse HEAD)" "\$CMD" >&2
set +e
bash -c "\$CMD"
RC=\$?
set -e
printf '[mac] done @ %s (exit %s)\n' "\$(git rev-parse HEAD)" "\$RC" >&2
exit \$RC
EOF
