#!/usr/bin/env bash
# From the desktop: the Mac's fuzz farm (docs/ENGINE.md 50.2) - its state, what each mode fuzzed
# (per hour of its running time and per hour of the farm), the back-offs, and every failure with its
# reproduction, the ones first seen since the last check marked NEW - and the match-noise study's
# progress (50.1).
#
#   bash tools/fuzz_farm_status.sh            # failures new since the last check marked NEW
#   bash tools/fuzz_farm_status.sh --all      # every failure NEW (ignores the last check)
#   bash tools/fuzz_farm_status.sh --json     # the raw status.json
#   bash tools/fuzz_farm_status.sh --noise    # + the noise study's summary.md
#   bash tools/fuzz_farm_status.sh --fetch-noise [DIR]   # copy the study's summary and progress to DIR
#                                                         # (default E:/mtg-zero-scratch/noise)
set -euo pipefail
HOST="${MTGZ_MAC:-greggkeithley@100.95.248.116}"
MARK="$HOME/.mtgz-fuzzfarm-lastcheck"
PY='$HOME/.venvs/dominion-zero/bin/python'
since="$(cat "$MARK" 2>/dev/null || echo 0)"
case "${1:-}" in
  --all) since=0 ;;
  --json) ssh -o BatchMode=yes "$HOST" "$PY ~/mtgz-fuzzfarm/bin/fuzz_farm.py status --json"; exit ;;
  --fetch-noise)
    dir="${2:-E:/mtg-zero-scratch/noise}"
    mkdir -p "$dir"
    scp -q "$HOST:mtgz-noise/summary.md" "$HOST:mtgz-noise/summary.json" "$dir/"
    scp -q "$HOST:mtgz-noise/progress_*.json" "$HOST:mtgz-noise/log.txt" "$dir/" 2>/dev/null || true
    echo "copied to $dir:"; ls -la "$dir"; exit ;;
esac
now="$(date +%s)"
ssh -o BatchMode=yes "$HOST" "$PY ~/mtgz-fuzzfarm/bin/fuzz_farm.py status --since $since
echo
if [ -f ~/mtgz-noise/progress_net.json ]; then
  $PY -c 'import json,time,os; p=json.load(open(os.path.expanduser(\"~/mtgz-noise/progress_net.json\"))); print(\"noise study (net):\", p[\"done\"], \"/\", p[\"total\"], \"match replays,\", p[\"matches_per_h\"], \"per h,\", p[\"leaves_per_s\"], \"leaves/s, paused\", p[\"paused_s\"], \"s, eta\", p[\"eta_h\"], \"h,\", int(time.time()-p[\"time\"]), \"s ago\", \"(done)\" if p.get(\"final\") and p[\"done\"]>=p[\"total\"] else \"\")'
else echo 'noise study: no net progress yet'; tail -2 ~/mtgz-noise/log.txt 2>/dev/null || true; fi
if [ '${1:-}' = --noise ] && [ -f ~/mtgz-noise/summary.md ]; then echo; cat ~/mtgz-noise/summary.md; fi"
echo "$now" > "$MARK"
