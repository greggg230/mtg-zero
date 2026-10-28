#!/usr/bin/env bash
# The Mac's fuzz farm (docs/ENGINE.md 50.2, tools/fuzz_farm.py): installs and controls the user
# LaunchAgent com.mtgzero.fuzzfarm, which runs `python ~/mtgz-fuzzfarm/bin/fuzz_farm.py run` at nice 19
# (restarted after a crash, started at login; a clean STOP is not restarted). Run ON THE MAC, from a
# checkout whose tools/ should be installed - from the desktop:
#
#   bash tools/mac.sh 'bash tools/fuzz_farm.sh install'   # copy this checkout's farm into ~/mtgz-fuzzfarm/bin, (re)load the agent
#   bash tools/mac.sh 'bash tools/fuzz_farm.sh stop'      # STOP: the jobs are killed, the agent stays loaded but idle
#   bash tools/mac.sh 'bash tools/fuzz_farm.sh start'     # rm STOP, (re)start
#   bash tools/mac.sh 'bash tools/fuzz_farm.sh pause'     # PAUSE: the jobs are stopped (SIGSTOP) until resume
#   bash tools/mac.sh 'bash tools/fuzz_farm.sh resume'
#   bash tools/mac.sh 'bash tools/fuzz_farm.sh uninstall' # unload and remove the agent (the farm's files stay)
#   bash tools/fuzz_farm_status.sh                         # from the desktop: status + new failures
#
# The farm fuzzes ~/mtg-zero.git's main (config.json "ref"): push main there from the desktop with
# `git push mac main` (or `bash tools/mac.sh true` in main's checkout, which also pushes zero-core's pin).
set -euo pipefail
FARM="${MTGZ_FARM:-$HOME/mtgz-fuzzfarm}"
LABEL=com.mtgzero.fuzzfarm
PLIST="$HOME/Library/LaunchAgents/$LABEL.plist"
PY="$HOME/.venvs/dominion-zero/bin/python"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DOMAIN="gui/$(id -u)"

loaded() { launchctl print "$DOMAIN/$LABEL" >/dev/null 2>&1; }

case "${1:-}" in
  install)
    mkdir -p "$FARM/bin" "$FARM/logs" "$FARM/failures"
    cp "$HERE/fuzz_farm.py" "$HERE/mac_backoff.py" "$FARM/bin/"
    git -C "$HERE/.." rev-parse HEAD > "$FARM/bin/INSTALLED_FROM"
    cat > "$PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>$LABEL</string>
	<key>ProgramArguments</key>
	<array>
		<string>$PY</string>
		<string>$FARM/bin/fuzz_farm.py</string>
		<string>run</string>
	</array>
	<key>WorkingDirectory</key>
	<string>$FARM</string>
	<key>EnvironmentVariables</key>
	<dict>
		<key>PATH</key>
		<string>$HOME/.venvs/dominion-zero/bin:$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
		<key>HOME</key>
		<string>$HOME</string>
	</dict>
	<key>RunAtLoad</key>
	<true/>
	<key>KeepAlive</key>
	<dict>
		<key>SuccessfulExit</key>
		<false/>
	</dict>
	<key>Nice</key>
	<integer>19</integer>
	<key>LowPriorityIO</key>
	<true/>
	<key>ThrottleInterval</key>
	<integer>60</integer>
	<key>StandardOutPath</key>
	<string>$FARM/logs/launchd.out.log</string>
	<key>StandardErrorPath</key>
	<string>$FARM/logs/launchd.err.log</string>
</dict>
</plist>
EOF
    plutil -lint "$PLIST" >/dev/null
    if loaded; then launchctl bootout "$DOMAIN/$LABEL" || true; sleep 2; fi
    rm -f "$FARM/STOP"
    launchctl bootstrap "$DOMAIN" "$PLIST"
    echo "installed $LABEL from $(cat "$FARM/bin/INSTALLED_FROM" | cut -c1-12); farm: $FARM"
    ;;
  start)
    rm -f "$FARM/STOP"
    if loaded; then launchctl kickstart -k "$DOMAIN/$LABEL"; else launchctl bootstrap "$DOMAIN" "$PLIST"; fi
    echo started
    ;;
  stop)
    touch "$FARM/STOP"
    for _ in $(seq 1 60); do
      pid="$(cat "$FARM/farm.pid" 2>/dev/null || true)"
      if [ -z "$pid" ] || ! kill -0 "$pid" 2>/dev/null; then echo stopped; exit 0; fi
      sleep 2
    done
    echo "still running after 120 s (pid $pid)" >&2; exit 1
    ;;
  pause) touch "$FARM/PAUSE"; echo "paused (its jobs stop within a few seconds)" ;;
  resume) rm -f "$FARM/PAUSE"; echo resumed ;;
  uninstall)
    if loaded; then launchctl bootout "$DOMAIN/$LABEL" || true; fi
    rm -f "$PLIST"
    echo "uninstalled $LABEL (files kept in $FARM)"
    ;;
  status) shift; "$PY" "$FARM/bin/fuzz_farm.py" status "$@" ;;
  *) sed -n 2,16p "$0"; exit 2 ;;
esac
