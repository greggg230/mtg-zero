#!/usr/bin/env bash
# The S4 races on the MacBook (docs/ENGINE.md 109.2): the deck evaluator's picks (`mtgz.sealed.compare evalpick`,
# ENGINE.md 108.6) and the value pick (`compare choice`, 107.2) raced with a network pilot on the Mac's CPU, detached,
# while the desktop trains; results fetched back to E:/mtg-zero/s4a/races/<name>/.
#
#   bash tools/mac_races.sh launch CKPT [options]   # copy the checkpoint and evaluators, pin the code, start (detached)
#   bash tools/mac_races.sh status NAME             # the jobs done, the last lines of the running ones
#   bash tools/mac_races.sh fetch NAME              # out/, logs/ and progress -> E:/mtg-zero/s4a/races/NAME/
#   bash tools/mac_races.sh wait NAME               # poll every 10 minutes until it ends, then fetch
#   bash tools/mac_races.sh stop NAME               # kill its jobs (the results so far stay on the Mac)
#
# launch options (defaults in brackets):
#   --name NAME          [<run>-<checkpoint>, e.g. eoe1-iter_0080]
#   --evaluators DIR     the S4a evaluators: DIR/evaluator_linear.pt, DIR/evaluator_set.pt [E:/mtg-zero/s4a/v0_time]
#   --evalpick-pools L   comma-separated pool seeds, one evalpick race each ['' = none] [2026,...,2033: 108.6]
#   --evalpick-args A    [--collation arena --k 32 --worlds 8 --vs 3 --race --games 128 --min-games 256
#                         --max-games 1024 --delta 0.03: 108.6's race]
#   --choice-pools L     ['' = none] [1001,1002,1003,1004: the value-pick race of iteration 39, E:/mtg-zero-scratch/race1]
#   --choice-args A      [--collation arena --k 32 --worlds 8 --vs 3 --games 512 --block 512: race1's pool 1001]
#   --procs P            race processes at once [6: ~10 cores, 109.2]      --threads T   search threads each [4]
#   --torch N            torch threads each (OMP_NUM_THREADS) [2]     --nice N   [10: below run evaluations (0)]
# fetch / wait: --out DIR (where the results go) [E:/mtg-zero/s4a/races/NAME]
#
# The code is this checkout's committed HEAD, pinned on the Mac (`tools/mac.sh --pin races`:
# ~/mtg-zero-wt/races-<sha12>, built once; a later launch at the same commit reuses it and never rebuilds under a
# running race). The checkpoint goes to ~/mtgz-races/NAME/run/models/ beside its run's config.json (the pilot's move
# rule, belief tilts and root search come from it: zero.h2h.run_rule), sha256-checked. Each pool is one job
# (`python -m mtgz.sealed.compare evalpick|choice --pool-seed N ... --pilot <ckpt> --sims 64 --device cpu`); P jobs run
# at once at nice N under one detached runner (nohup: it survives the ssh session), each writing
# out/<kind>-<pool>.json (the command's --json) and logs/<kind>-<pool>.{out,log}; progress.txt gets a line per job.
# When every job has ended: out/evalpick.json (the pools' outputs in one, with mtgz.sealed.compare.evalpick_summary
# over them: what one `evalpick --pool-seed a,b,...` run prints) and out/choice.json (the pools, and the picked
# build's mean paired difference against the random builds over pools), then DONE (or FAILED: a job's exit != 0).
# A comparison replays exactly from its seeds, block and pilot whatever the threads and processes (ENGINE.md 104.3).
set -euo pipefail
HOST="${MTGZ_MAC:-greggkeithley@100.95.248.116}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
usage() { sed -n 2,23p "$0"; exit 2; }
[ $# -ge 2 ] || usage
CMD="$1"; shift
ssh_mac() { ssh -o BatchMode=yes -o ConnectTimeout=20 "$HOST" "$@"; }

if [ "$CMD" != launch ]; then
  NAME="$1"; shift
  OUT="E:/mtg-zero/s4a/races/$NAME"
  while [ $# -gt 0 ]; do case "$1" in --out) OUT="$2"; shift 2 ;; *) usage ;; esac; done
  D="mtgz-races/$NAME"
  ssh_mac "test -d $D" || { echo "mac_races: no race $NAME on the Mac (~/$D)" >&2; exit 1; }
  fetch() {
    mkdir -p "$OUT"
    scp -q -r -o BatchMode=yes "$HOST:$D/out" "$HOST:$D/logs" "$OUT/"
    scp -q -o BatchMode=yes "$HOST:$D/progress.txt" "$HOST:$D/jobs.txt" "$HOST:$D/settings.json" "$HOST:$D/runner.log" "$OUT/"
    ssh_mac "cd $D && ls DONE FAILED 2>/dev/null" > "$OUT/state.txt" || true
    echo "fetched $NAME -> $OUT ($(cat "$OUT/state.txt" 2>/dev/null | tr '\n' ' ')$(wc -l < "$OUT/progress.txt") of $(wc -l < "$OUT/jobs.txt") jobs ended)"
  }
  case "$CMD" in
    status)
      ssh_mac "cd $D && echo \"jobs: \$(wc -l < jobs.txt), ended: \$(wc -l < progress.txt)\" && cat progress.txt && ls DONE FAILED 2>/dev/null;
               for f in logs/*.log; do [ -s \"\$f\" ] && printf '%s: %s\n' \"\$f\" \"\$(tail -n 1 \"\$f\" | cut -c1-200)\"; done; true" ;;
    fetch) fetch ;;
    wait)
      until ssh_mac "test -e $D/DONE -o -e $D/FAILED"; do sleep 600; done
      fetch ;;
    stop)
      # the runner's xargs (no new job), the runner, then the jobs' python processes (their command lines name ~/$D/run)
      ssh_mac "cd $D && r=\$(cat runner.pid 2>/dev/null || echo 0) && { [ \$r = 0 ] || { pkill -TERM -P \$r; kill -TERM \$r; }; } 2>/dev/null;
               pkill -TERM -f '$D/run/models/' 2>/dev/null; touch FAILED; echo stopped" ;;
    *) usage ;;
  esac
  exit 0
fi

# ---------------------------------------------------------------------------------------------------------------- launch
CKPT="$1"; shift
NAME=""; EVALS="E:/mtg-zero/s4a/v0_time"
EP_POOLS="2026,2027,2028,2029,2030,2031,2032,2033"
EP_ARGS="--collation arena --k 32 --worlds 8 --vs 3 --race --games 128 --min-games 256 --max-games 1024 --delta 0.03"
CH_POOLS="1001,1002,1003,1004"
CH_ARGS="--collation arena --k 32 --worlds 8 --vs 3 --games 512 --block 512"
PROCS=6; THREADS=4; TORCH=2; NICE=10
while [ $# -gt 0 ]; do
  case "$1" in
    --name) NAME="$2" ;; --evaluators) EVALS="$2" ;; --evalpick-pools) EP_POOLS="$2" ;; --evalpick-args) EP_ARGS="$2" ;;
    --choice-pools) CH_POOLS="$2" ;; --choice-args) CH_ARGS="$2" ;; --procs) PROCS="$2" ;; --threads) THREADS="$2" ;;
    --torch) TORCH="$2" ;; --nice) NICE="$2" ;; *) usage ;;
  esac
  shift 2
done
[ -f "$CKPT" ] || { echo "mac_races: no checkpoint $CKPT" >&2; exit 1; }
RUNDIR="$(dirname "$(dirname "$CKPT")")"
[ -f "$RUNDIR/config.json" ] || { echo "mac_races: no $RUNDIR/config.json beside the checkpoint's models/ (the pilot's move rule)" >&2; exit 1; }
for f in evaluator_linear.pt evaluator_set.pt; do
  [ -z "$EP_POOLS" ] || [ -f "$EVALS/$f" ] || { echo "mac_races: no $EVALS/$f" >&2; exit 1; }
done
FILE="$(basename "$CKPT")"
[ -n "$NAME" ] || NAME="$(basename "$RUNDIR")-${FILE%.pt}"
case "$NAME" in *[!A-Za-z0-9._-]*) echo "mac_races: name $NAME: letters, digits, . _ - only" >&2; exit 1 ;; esac
JOBS="$(
  for p in ${EP_POOLS//,/ }; do echo "evalpick $p"; done
  for p in ${CH_POOLS//,/ }; do echo "choice $p"; done
)"
[ -n "$JOBS" ] || { echo "mac_races: no pools" >&2; exit 1; }
SHA="$(git rev-parse HEAD)"
D="mtgz-races/$NAME"
WT="mtg-zero-wt/races-${SHA:0:12}"
if ssh_mac "test -e $D"; then echo "mac_races: ~/$D exists on the Mac (another name, or remove it there)" >&2; exit 1; fi

# the code: this commit pinned and built once (never rebuilt under a running race)
if ssh_mac "test -f $WT/mtgz/_engine.so && cd $WT && test \"\$(git rev-parse HEAD)\" = $SHA"; then
  echo "mac_races: reusing the Mac's pinned checkout ~/$WT"
else
  bash tools/mac.sh --pin races
fi

# the checkpoint beside its run's config, the evaluators
ssh_mac "mkdir -p $D/run/models $D/evaluators $D/out $D/logs && touch $D/progress.txt"
scp -q -o BatchMode=yes "$RUNDIR/config.json" "$HOST:$D/run/config.json"
scp -q -o BatchMode=yes "$CKPT" "$HOST:$D/run/models/$FILE"
if [ -n "$EP_POOLS" ]; then
  scp -q -o BatchMode=yes "$EVALS/evaluator_linear.pt" "$EVALS/evaluator_set.pt" "$HOST:$D/evaluators/"
  [ -f "$EVALS/report.json" ] && scp -q -o BatchMode=yes "$EVALS/report.json" "$HOST:$D/evaluators/"
fi
want="$(sha256sum "$CKPT" | cut -c1-64)"
got="$(ssh_mac "shasum -a 256 $D/run/models/$FILE" | cut -c1-64)"
[ "$want" = "$got" ] || { echo "mac_races: the checkpoint's copy differs (sha256 $got, here $want)" >&2; exit 1; }

# the jobs, the settings, the runner
ssh_mac "cat > $D/jobs.txt" <<< "$JOBS"
SETTINGS="{\"name\": \"$NAME\", \"checkpoint\": \"$CKPT\", \"sha256\": \"$want\", \"commit\": \"$SHA\", \"evaluators\": \"$EVALS\",
 \"evalpick_pools\": \"$EP_POOLS\", \"evalpick_args\": \"$EP_ARGS\", \"choice_pools\": \"$CH_POOLS\", \"choice_args\": \"$CH_ARGS\",
 \"procs\": $PROCS, \"threads\": $THREADS, \"torch\": $TORCH, \"nice\": $NICE, \"launched\": \"$(date -u +%FT%TZ)\"}"
ssh_mac "cat > $D/settings.json" <<< "$SETTINGS"
ssh_mac "cat > $D/run.sh" <<EOF
#!/bin/bash
# written by tools/mac_races.sh launch ($SHA): the races of $NAME
set -uo pipefail
export D="\$HOME/$D" WT="\$HOME/$WT"
export PATH="\$HOME/.venvs/dominion-zero/bin:\$PATH" OMP_NUM_THREADS=$TORCH PYTHONUNBUFFERED=1
cd "\$WT"
job() {
  local kind=\$1 pool=\$2 t0=\$(date +%s) extra
  if [ "\$kind" = evalpick ]; then
    extra="--evaluator \$D/evaluators/evaluator_linear.pt,\$D/evaluators/evaluator_set.pt $EP_ARGS"
  else
    extra="$CH_ARGS"
  fi
  python -m mtgz.sealed.compare "\$kind" --pool-seed "\$pool" \$extra --pilot "\$D/run/models/$FILE" --sims 64 \\
    --device cpu --threads $THREADS --json "\$D/out/\$kind-\$pool.json" > "\$D/logs/\$kind-\$pool.out" 2> "\$D/logs/\$kind-\$pool.log"
  local rc=\$?
  echo "\$(date '+%F %T') \$kind \$pool exit \$rc \$(( \$(date +%s) - t0 ))s" >> "\$D/progress.txt"
  return \$rc
}
export -f job
xargs -P $PROCS -L 1 bash -c 'job "\$@"' _ < "\$D/jobs.txt"
rc=\$?
python - "\$D" <<'PY'
import glob, json, math, os, sys
from mtgz.sealed.compare import evalpick_summary
d = sys.argv[1]
per = []
for f in sorted(glob.glob(os.path.join(d, 'out', 'evalpick-*.json'))):
    with open(f, encoding='utf-8') as fh:
        o = json.load(fh)
    o.pop('summary', None)
    per.append(o)
if per:
    with open(os.path.join(d, 'out', 'evalpick.json'), 'w', encoding='utf-8') as fh:
        json.dump({'pools': per, 'summary': evalpick_summary([o['evalpick']['paired'] for o in per])}, fh, indent=1)
ch = []
for f in sorted(glob.glob(os.path.join(d, 'out', 'choice-*.json'))):
    with open(f, encoding='utf-8') as fh:
        ch.append(json.load(fh))
if ch:
    xs = []
    for o in ch:
        pk = f"picked{o['choice']['picked']}"
        pairs = o.get('pairs') or {}
        ds = [s['diff'] for k, s in pairs.items() if k.startswith(pk + '-random')]
        if not ds and o.get('vs_winner') is not None:
            ds = [None]
        xs.append(sum(ds) / len(ds) if ds and None not in ds else None)
    ok = [x for x in xs if x is not None]
    n = len(ok)
    m = sum(ok) / n if n else None
    se = math.sqrt(sum((x - m) ** 2 for x in ok) / (n - 1) / n) if n > 1 else None
    with open(os.path.join(d, 'out', 'choice.json'), 'w', encoding='utf-8') as fh:
        json.dump({'pools': ch, 'summary': {'picked_vs_random': {'pools': n, 'mean': None if m is None else round(m, 4),
                   'se': None if se is None else round(se, 4), 'per_pool': xs}}}, fh, indent=1)
PY
[ \$rc = 0 ] && touch "\$D/DONE" || touch "\$D/FAILED"
EOF
ssh_mac "cd $D && chmod +x run.sh && (nohup nice -n $NICE bash run.sh > runner.log 2>&1 < /dev/null & echo \$! > runner.pid) && sleep 2 && echo \"started: pid \$(cat runner.pid), \$(wc -l < jobs.txt) jobs, $PROCS at once\""
echo "mac_races: $NAME running on the Mac (~/$D). Later: bash tools/mac_races.sh status $NAME | wait $NAME | fetch $NAME"
