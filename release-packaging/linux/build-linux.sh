#!/usr/bin/env bash
set -euo pipefail

fail() {
    printf 'build-linux: %s\n' "$*" >&2
    exit 1
}

usage() {
    cat <<'USAGE'
Usage: build-linux.sh

Build the GUI-enabled DiskUnfold launcher on a native Ubuntu 24.04 or 26.04
runner. The script uses the pinned Node version, installs the locked web
dependencies with lifecycle scripts disabled, builds the web assets, then
builds the launcher for the runner's native Rust target.
USAGE
}

if [[ "${1:-}" == --help || "${1:-}" == -h ]]; then
    usage
    exit 0
fi
[[ "$#" -eq 0 ]] || fail 'this script accepts no positional arguments'

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
repo_root=$(cd "$script_dir/../.." && pwd -P)

[[ -r /etc/os-release ]] || fail '/etc/os-release is unavailable'
. /etc/os-release
[[ "${ID:-}" == ubuntu ]] || fail 'official Linux packaging builds run on Ubuntu only'
case "${VERSION_ID:-}" in
    24.04|26.04) ;;
    *) fail "official Linux packaging builds require Ubuntu 24.04 or 26.04; found ${VERSION_ID:-unknown}" ;;
esac

for tool in node npm rustc cargo readelf; do
    command -v "$tool" >/dev/null 2>&1 || fail "required tool is unavailable: $tool"
done

expected_node=$(tr -d '[:space:]' < "$repo_root/.node-version")
[[ -n "$expected_node" ]] || fail '.node-version is empty'
actual_node=$(node --version)
[[ "$actual_node" == "v$expected_node" ]] || \
    fail "Node $expected_node is required by .node-version; found $actual_node"

host=$(rustc -vV | awk -F': ' '$1 == "host" { print $2 }')
case "$host" in
    x86_64-unknown-linux-gnu)
        cpu=x64
        expected_machine=x86_64
        ;;
    aarch64-unknown-linux-gnu)
        cpu=arm64
        expected_machine=aarch64
        ;;
    *)
        fail "unsupported native Rust host target: ${host:-unknown}"
        ;;
esac
[[ "$(uname -m)" == "$expected_machine" ]] || \
    fail "Rust host target $host does not match the native runner CPU $(uname -m)"

npm --prefix "$repo_root/gui-web" ci --ignore-scripts
npm --prefix "$repo_root/gui-web" run build

LC_ALL=C cargo build --locked --release --package du-launcher --target "$host"
binary="$repo_root/target/$host/release/diskunfold"
[[ -f "$binary" && ! -L "$binary" && -x "$binary" ]] || \
    fail "native launcher was not produced: $binary"

needed=$(LC_ALL=C readelf -d "$binary" 2>/dev/null) || fail 'launcher is not a readable ELF executable'
[[ "$needed" == *libwebkitgtk-6.0.so* ]] || \
    fail 'GUI launcher does not dynamically link the system WebKitGTK 6.0 runtime'
[[ "$needed" != *libdbus-1.so* ]] || \
    fail 'launcher links libdbus; the Linux package must use the Secret Service backend without libdbus'

printf 'Linux GUI launcher built successfully.\n'
printf 'cpu=%s\nbinary=%s\n' "$cpu" "$binary"