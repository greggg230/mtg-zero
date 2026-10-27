#!/usr/bin/env bash
# A watchdog for a supervised run (run it in the background from a Claude session; it exits on
# the first event and prints what it saw). Fires on: a new supervisor crash/restart/exit line, the
# supervisor pid gone, no write to progress.json/log.txt/stdout.txt for 75 min without PAUSE
# (progress.json alone is not written during Elo phases), NaN/Inf in an "iter N train" line, and
# a collapse (heuristic win <= 2% while self-play rounds sit at 150+, runs/mtg4 2026-10-27).
#   bash tools/watch_run.sh "C:/Users/Gregg Keithley/mtg-zero/runs/mtg5" [hours, default 24]
set -u
RUN="$1"; HOURS="${2:-24}"
SUP_PID="$(python -c "import json,sys;print(json.load(open(sys.argv[1]))['pid'])" "$RUN/supervisor_state.json")"
STALL_MIN=75
START_LINES="$(wc -l < "$RUN/supervisor.log")"
LOG_LINES="$(wc -l < "$RUN/log.txt")"
END=$(( $(date +%s) + HOURS*3600 ))
echo "watch start $(date '+%F %T') $RUN supervisor pid $SUP_PID"
while [ "$(date +%s)" -lt "$END" ]; do
  sleep 60
  NOW_LINES="$(wc -l < "$RUN/supervisor.log")"
  if [ "$NOW_LINES" -gt "$START_LINES" ]; then
    NEW="$(tail -n +"$((START_LINES+1))" "$RUN/supervisor.log")"
    if echo "$NEW" | grep -Eiq "exited with code|restart|stall|panic|error|supervisor done"; then
      echo "SUPERVISOR EVENT $(date '+%F %T'):"; echo "$NEW"; tail -5 "$RUN/log.txt" | cut -c1-300; exit 0
    fi
    START_LINES="$NOW_LINES"
  fi
  NEWLOG="$(tail -n +"$((LOG_LINES+1))" "$RUN/log.txt")"
  if echo "$NEWLOG" | grep -E "iter [0-9]+ train:" | grep -Eq "NaN|Infinity|nan|inf\b"; then
    echo "NON-FINITE TRAINING $(date '+%F %T'):"; echo "$NEWLOG" | grep -E "iter [0-9]+ train:" | cut -c1-300; exit 0
  fi
  if echo "$NEWLOG" | grep -Eq "vs heuristic: \{'win': 0\.0[0-2]?," && echo "$NEWLOG" | grep -E "selfplay:" | grep -Eq '"rounds": 1[5-9][0-9]'; then
    echo "COLLAPSE SIGNATURE $(date '+%F %T'):"; echo "$NEWLOG" | grep -E "selfplay:|vs heuristic|vs random" | cut -c1-300; exit 0
  fi
  if ! tasklist //FI "PID eq $SUP_PID" //NH 2>/dev/null | grep -q "$SUP_PID"; then
    echo "SUPERVISOR GONE $(date '+%F %T') (pid $SUP_PID)"; tail -5 "$RUN/supervisor.log"; tail -5 "$RUN/log.txt" | cut -c1-300; exit 0
  fi
  NEWEST=0
  for f in progress.json log.txt stdout.txt; do
    m="$(stat -c %Y "$RUN/$f" 2>/dev/null || echo 0)"; [ "$m" -gt "$NEWEST" ] && NEWEST="$m"
  done
  AGE_MIN=$(( ( $(date +%s) - NEWEST ) / 60 ))
  if [ "$AGE_MIN" -ge "$STALL_MIN" ] && [ ! -e "$RUN/PAUSE" ]; then
    echo "STALL $(date '+%F %T'): no write for $AGE_MIN min"; cat "$RUN/progress.json"; echo; tail -5 "$RUN/log.txt" | cut -c1-300; exit 0
  fi
done
echo "watch expired after $HOURS h without events $(date '+%F %T')"
