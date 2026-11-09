#!/usr/bin/env bash
# Run on a Linux development host with Xvfb, Openbox, xdotool and dbus-run-session.
set -euo pipefail

if [[ "$#" -ne 1 ]]; then
    echo "Usage: bash smoke-linux.sh /path/to/diskunfold" >&2
    exit 2
fi
app="$(realpath "$1")"

"$app" --help
"$app" --version
xvfb-run -a dbus-run-session bash -s -- "$app" <<'SMOKE'
set -euo pipefail
export NO_AT_BRIDGE=1
openbox >/dev/null 2>&1 &
wm=$!
"$1" &
app_pid=$!
trap 'kill "$app_pid" "$wm" 2>/dev/null || true' EXIT

window="$(timeout 20 xdotool search --sync --onlyvisible --pid "$app_pid" --name '^DiskUnfold$')"
xdotool windowactivate --sync "${window%%$'\n'*}"
xdotool key alt+F4
timeout 10 tail --pid="$app_pid" -f /dev/null
wait "$app_pid"
printf '%s\n' 'DiskUnfold opened a window and exited after a close request.'
SMOKE
