#!/usr/bin/env bash
# Native quit smoke for Linux: starts one isolated GUI instance under Xvfb, closes
# its window with xdotool (the window manager sends a close request) and checks for
# an orderly exit. Needs xvfb-run, dbus-run-session, openbox, xdotool and python3.
# Restricted containers may pass WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1; this
# script never sets it and records whether it was set.
set -euo pipefail

if [[ "$#" -ne 1 ]]; then
    echo "Usage: bash smoke-linux-quit.sh /path/to/diskunfold" >&2
    exit 2
fi
app="$(realpath "$1")"
for tool in xvfb-run dbus-run-session openbox xdotool python3 flock; do
    command -v "$tool" >/dev/null 2>&1 || { echo "Required tool is missing: $tool" >&2; exit 77; }
done

root="$(mktemp -d "${TMPDIR:-/tmp}/diskunfold-linux-quit.XXXXXX")"
export QUIT_ROOT="$root"
export HOME="$root/home" XDG_DATA_HOME="$root/xdg-data" XDG_CONFIG_HOME="$root/xdg-config" XDG_CACHE_HOME="$root/xdg-cache"
export DISKUNFOLD_DATA_DIR="$root/data"
mkdir -p "$HOME" "$DISKUNFOLD_DATA_DIR"
echo "webkit_sandbox_disabled=${WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS:-0}"
# PROBE_IPC=1 also reads app.startup_status and app.product_info through the
# WebKitGTK remote inspector on a loopback port (needs node).
export SMOKE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
if [[ "${PROBE_IPC:-0}" == 1 ]]; then export WEBKIT_INSPECTOR_HTTP_SERVER=127.0.0.1:9333; fi

status=0
xvfb-run -a dbus-run-session bash -s -- "$app" <<'SMOKE' || status=$?
set -uo pipefail
export NO_AT_BRIDGE=1
openbox >/dev/null 2>&1 &
wm=$!
"$1" >"$QUIT_ROOT/stdout.txt" 2>"$QUIT_ROOT/stderr.txt" &
app_pid=$!
trap 'kill "$app_pid" "$wm" 2>/dev/null || true' EXIT
fail=0
check() { # name, status (0 = ok)
    if [[ "$2" -eq 0 ]]; then echo "check $1=pass"; else echo "check $1=FAIL"; fail=1; fi
}

window="$(timeout 30 xdotool search --sync --onlyvisible --pid "$app_pid" --name '^DiskUnfold$' | head -n 1)"
[[ -n "$window" ]]; check window_visible $?
# The database exists once startup has opened it.
for _ in $(seq 100); do [[ -f "$DISKUNFOLD_DATA_DIR/diskunfold.db" ]] && break; sleep 0.2; done
echo "files_before_close: $(cd "$DISKUNFOLD_DATA_DIR" && ls -A | tr '\n' ' ')"
sleep 2
if [[ "${PROBE_IPC:-0}" == 1 ]]; then
    echo "ipc: $(node "$SMOKE_DIR/smoke-webview-ipc.mjs" webkit:9333 app.startup_status app.product_info)"
fi
xdotool windowactivate --sync "$window"
xdotool key alt+F4
if timeout 10 tail --pid="$app_pid" -f /dev/null; then exited=0; else exited=1; fi
check exited_within_10s "$exited"
wait "$app_pid"; code=$?
echo "exit_code=$code"
[[ "$code" -eq 0 ]]; check exit_code_zero $?
[[ ! -d /proc/$app_pid ]]; check process_gone $?
# A defunct entry left by an earlier run is not a live process.
ps -eo stat=,comm= | awk '$2 == "diskunfold" && $1 !~ /^Z/ { found = 1 } END { exit found }'; check no_diskunfold_process $?
# Releasing the lock means a non-blocking exclusive flock on the sentinel succeeds.
flock -n -x "$DISKUNFOLD_DATA_DIR/.diskunfold.instance.lock" true; check lock_released $?
echo "files_after_close: $(cd "$DISKUNFOLD_DATA_DIR" && ls -A | tr '\n' ' ')"
! ls "$DISKUNFOLD_DATA_DIR"/diskunfold.db-wal "$DISKUNFOLD_DATA_DIR"/diskunfold.db-shm >/dev/null 2>&1; check no_wal_or_shm $?
# Check a copy so the check cannot create sidecar files next to the real database.
cp "$DISKUNFOLD_DATA_DIR/diskunfold.db" "$QUIT_ROOT/integrity-copy.db"
python3 - "$QUIT_ROOT/integrity-copy.db" <<'PY'
import sqlite3, sys
c = sqlite3.connect(sys.argv[1])
header = open(sys.argv[1], 'rb').read(20)[18:20].hex()
states = [r[0] for r in c.execute('select final_state from job_runtime_job_record')]
print('database integrity=%s journal_mode_header=%s job_states=%s' % (c.execute('pragma integrity_check').fetchone()[0], header, states))
PY
echo "stderr_bytes=$(stat -c %s "$QUIT_ROOT/stderr.txt")"
echo "stdout_bytes=$(stat -c %s "$QUIT_ROOT/stdout.txt")"
exit "$fail"
SMOKE
if [[ "$status" -eq 0 ]]; then
    echo 'Native Linux quit smoke passed.'
    rm -rf -- "$root"
else
    echo "Native Linux quit smoke failed; evidence retained at: $root" >&2
    exit 1
fi
