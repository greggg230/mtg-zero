#!/usr/bin/env bash
# The heavy-command throttle (docs/ENGINE.md 166.1): run a command holding one of a machine-wide set of slots, so side
# work (cargo builds and tests, fuzz, digests, tools/build_py.sh, pytest, smokes, gates) never piles up beside a live run.
#
#   bash tools/slotd.sh <command> [args...]      # wait for a slot, run the command holding it, exit with its status
#   bash tools/slotd.sh --status                 # the slots, who holds them, the settings and the memory headroom
#
# State (shared by every checkout and session on the machine, outside any repo):
#   $MTGZ_SLOTS_HOME, default E:/mtg-zero-state on the desktop (Windows), ~/.mtgz-state elsewhere (the Mac)
#     slots.conf   settings, re-read on every look (tune it live), `key=value` lines, `#` comments; a bare number is
#                  `slots` (the old fable-fix slots.conf format):
#                    slots=3            how many slots
#                    free=3             slots taken without the memory check
#                    min_commit_gb=14   a slot above `free` needs this much commit headroom (Windows) ...
#                    min_avail_gb=6     ... and this much available RAM
#                    floor_avail_gb=2   no slot at all is taken while available RAM is below this (a live run would page)
#                    jobs=10            CARGO_BUILD_JOBS for the command when the caller set none
#     slots/slotN  a held slot: pid (this script's), cmd (cwd + command), since
# A slot whose holder is gone is taken over. MTGZ_SLOT_WAIT_MAX=<seconds> gives up after that long without a slot
# (exit 75, "[slotd.sh] no slot after ..."; tools/gate.py sets it, so a step's timeout covers its wait).
# On the desktop, a slot held through the old E:/mtg-zero-scratch/fable-fix/slot*.sh (their slot dirs) counts as held
# until its holder ends: the old scripts are shims to this one now.
set -u
if [ -n "${MTGZ_SLOTS_HOME:-}" ]; then STATE="$MTGZ_SLOTS_HOME"
elif [ -d /e ] && [ "$(uname -o 2>/dev/null)" = Msys ]; then STATE=E:/mtg-zero-state
else STATE="$HOME/.mtgz-state"; fi
CONF="$STATE/slots.conf"
DIR="$STATE/slots"
LEGACY=""
if [ -z "${MTGZ_SLOTS_HOME:-}" ] && [ "$STATE" = E:/mtg-zero-state ]; then LEGACY=E:/mtg-zero-scratch/fable-fix/slots; fi
mkdir -p "$DIR"

setting() {   # setting <key> <default>
  local v=""
  if [ -f "$CONF" ]; then
    v=$(sed -n "s/^[[:space:]]*$1[[:space:]]*=[[:space:]]*\([0-9.]*\).*/\1/p" "$CONF" | tail -1)
    if [ -z "$v" ] && [ "$1" = slots ]; then v=$(grep -E '^[[:space:]]*[0-9]+[[:space:]]*$' "$CONF" | tail -1 | tr -dc '0-9'); fi
  fi
  echo "${v:-$2}"
}

# memory: "<available GB> <commit headroom GB>" (headroom 999 where commit does not apply), or nothing when unknown
memory() {
  if command -v typeperf >/dev/null 2>&1; then
    local line c l a
    line=$(typeperf "\Memory\Committed Bytes" "\Memory\Commit Limit" "\Memory\Available MBytes" -sc 1 2>/dev/null | sed -n '3p' | tr -d '"\r')
    c=$(echo "$line" | cut -d, -f2); l=$(echo "$line" | cut -d, -f3); a=$(echo "$line" | cut -d, -f4)
    [ -n "$c" ] && [ -n "$l" ] && [ -n "$a" ] || return 0
    awk -v c="$c" -v l="$l" -v a="$a" 'BEGIN{printf "%.1f %.1f\n", a/1024, (l-c)/1073741824}'
  elif command -v vm_stat >/dev/null 2>&1; then          # macOS: free + inactive + speculative pages
    vm_stat | awk '/page size of/{ps=$8} /Pages (free|inactive|speculative)/{gsub("\\.","",$NF); n+=$NF}
                   END{if (ps) printf "%.1f 999\n", n*ps/1073741824}'
  elif [ -r /proc/meminfo ]; then
    awk '/MemAvailable/{printf "%.1f 999\n", $2/1048576}' /proc/meminfo
  fi
}

ge() { awk -v a="$1" -v b="$2" 'BEGIN{exit !(a >= b)}'; }

holder_alive() {   # holder_alive <slot dir>
  local p
  p=$(cat "$1/pid" 2>/dev/null)
  [ -n "$p" ] && kill -0 "$p" 2>/dev/null
}

if [ "${1:-}" = --status ]; then
  echo "state $STATE: slots=$(setting slots 3) free=$(setting free 3) min_commit_gb=$(setting min_commit_gb 14)" \
       "min_avail_gb=$(setting min_avail_gb 6) floor_avail_gb=$(setting floor_avail_gb 2) jobs=$(setting jobs 10)"
  echo "memory (available GB, commit headroom GB): $(memory)"
  for d in "$DIR"/slot* ${LEGACY:+"$LEGACY"/slot*}; do
    [ -d "$d" ] || continue
    if holder_alive "$d"; then st=held; else st="held by a gone pid"; fi
    echo "$d: $st since $(cat "$d/since" 2>/dev/null) - $(tr '\n' ' ' < "$d/cmd" 2>/dev/null)"
  done
  exit 0
fi
[ $# -gt 0 ] || { sed -n 2,26p "$0"; exit 2; }

export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-$(setting jobs 10)}"
WAIT_MAX="${MTGZ_SLOT_WAIT_MAX:-0}"
waited=0
said=""
while :; do
  SLOTS=$(setting slots 3); FREE=$(setting free 3)
  MEM=$(memory); AVAIL=${MEM%% *}; HEAD=${MEM##* }
  floor_ok=1
  if [ -n "$MEM" ] && ! ge "$AVAIL" "$(setting floor_avail_gb 2)"; then floor_ok=""; fi
  roomy=""
  if [ -n "$MEM" ] && ge "$HEAD" "$(setting min_commit_gb 14)" && ge "$AVAIL" "$(setting min_avail_gb 6)"; then roomy=1; fi
  if [ -n "$floor_ok" ]; then
    for i in $(seq 1 "$SLOTS"); do
      d="$DIR/slot$i"
      if [ -d "$d" ] && ! holder_alive "$d"; then rm -rf "$d"; fi
      if [ -n "$LEGACY" ] && [ -d "$LEGACY/slot$i" ] && holder_alive "$LEGACY/slot$i"; then continue; fi
      if [ "$i" -gt "$FREE" ] && [ -z "$roomy" ]; then continue; fi
      if mkdir "$d" 2>/dev/null; then
        echo $$ > "$d/pid"; printf '%s\n' "$PWD" "$*" > "$d/cmd"; date +%s > "$d/since"
        trap 'rm -rf "$d"' EXIT INT TERM HUP
        [ "$waited" -gt 0 ] && echo "[slotd.sh] got slot $i after ${waited}s" >&2
        "$@"
        exit $?
      fi
    done
  fi
  if [ "$WAIT_MAX" -gt 0 ] && [ "$waited" -ge "$WAIT_MAX" ]; then
    echo "[slotd.sh] no slot after ${waited}s (MTGZ_SLOT_WAIT_MAX=$WAIT_MAX; bash tools/slotd.sh --status)" >&2
    exit 75
  fi
  why="all $SLOTS slots busy"
  [ -z "$floor_ok" ] && why="available RAM ${AVAIL} GB below floor_avail_gb $(setting floor_avail_gb 2)"
  if [ "$said" != "$why" ]; then
    echo "[slotd.sh] waiting: $why ($STATE; bash tools/slotd.sh --status)" >&2
    said="$why"
  fi
  sleep 20; waited=$((waited+20))
done
