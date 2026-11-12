#!/bin/bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
package_script="$script_dir/package-dmg.sh"
[[ "$(uname -s)" == Darwin ]] || {
    printf 'test-package-dmg: must run on macOS\n' >&2
    exit 1
}
for tool in clang codesign hdiutil lipo python3; do
    command -v "$tool" >/dev/null 2>&1 || {
        printf 'test-package-dmg: required tool unavailable: %s\n' "$tool" >&2
        exit 1
    }
done

tmp_base=${TMPDIR:-/tmp}
test_root=$(mktemp -d "$tmp_base/diskunfold-macos-package-test.XXXXXX")
mount_path="$test_root/mount"
mounted=0
cleanup() {
    status=$?
    if ((mounted)); then
        hdiutil detach "$mount_path" >/dev/null 2>&1 || true
    fi
    case "$test_root" in
        "$tmp_base"/diskunfold-macos-package-test.*) rm -rf -- "$test_root" ;;
        *) printf 'test-package-dmg: refusing to remove unexpected test path: %s\n' "$test_root" >&2 ;;
    esac
    exit "$status"
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

cat > "$test_root/main.c" <<'SOURCE'
int main(void) { return 0; }
SOURCE
clang -arch arm64 -mmacosx-version-min=15.0 -O2 -o "$test_root/diskunfold-arm64" "$test_root/main.c"
clang -arch x86_64 -mmacosx-version-min=15.0 -O2 -o "$test_root/diskunfold-x86_64" "$test_root/main.c"
lipo -create "$test_root/diskunfold-arm64" "$test_root/diskunfold-x86_64" \
    -output "$test_root/diskunfold-universal"

package_dir="$test_root/package"
"$package_script" --binary "$test_root/diskunfold-universal" --version 0.1.0 \
    --bundle-id org.example.diskunfold.packagingtest --output-dir "$package_dir"
dmg="$package_dir/DiskUnfold-0.1.0-universal.dmg"
manifest="$package_dir/manifest.json"
[[ -f "$dmg" && -f "$manifest" && -x "$package_dir/unpacked/macos-universal/diskunfold" ]]

python3 - "$manifest" "$dmg" "$package_dir/unpacked/macos-universal/diskunfold" <<'PY'
import hashlib
import json
import pathlib
import sys

manifest_path, dmg_path, executable_path = map(pathlib.Path, sys.argv[1:])
data = json.loads(manifest_path.read_text(encoding="utf-8"))
assert set(data) == {"product_name", "version", "rows", "channel_references", "staged_files"}
assert data["product_name"] == "DiskUnfold" and data["version"] == "0.1.0"
assert data["channel_references"] == []
assert len(data["rows"]) == 1
row = data["rows"][0]
assert set(row) == {
    "file_name", "version", "os", "cpu", "format", "channel", "signature",
    "signature_scope", "sha256", "executable_file_name", "executable_sha256",
    "run_instructions",
}
assert row["file_name"] == dmg_path.name
assert row["cpu"] == "universal" and row["format"] == "dmg"
assert row["signature"] == "hardened_runtime_ad_hoc"
assert "no notarization" in row["signature_scope"]
digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
assert row["sha256"] == digest(dmg_path)
assert row["executable_sha256"] == digest(executable_path)
assert len(data["staged_files"]) == 2
print("release manifest fields and SHA-256 values verified")
PY

mkdir "$mount_path"
hdiutil attach -readonly -nobrowse -mountpoint "$mount_path" "$dmg" >/dev/null
mounted=1
app="$mount_path/DiskUnfold.app"
[[ -x "$app/Contents/MacOS/diskunfold" ]]
[[ -L "$mount_path/Applications" && "$(readlink "$mount_path/Applications")" == /Applications ]]
plutil -lint "$app/Contents/Info.plist" >/dev/null
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Contents/Info.plist")" == org.example.diskunfold.packagingtest ]]
[[ "$(lipo -archs "$app/Contents/MacOS/diskunfold" | tr ' ' '\n' | LC_ALL=C sort | paste -sd ' ' -)" == 'arm64 x86_64' ]]
codesign --verify --deep --strict "$app"
signature=$(codesign --display --verbose=4 "$app" 2>&1)
printf '%s\n' "$signature" | grep -Eiq 'Signature=adhoc'
printf '%s\n' "$signature" | grep -Eiq 'flags=.*runtime'
hdiutil detach "$mount_path" >/dev/null
mounted=0

before=$(shasum -a 256 "$dmg" | awk '{print $1}')
if "$package_script" --binary "$test_root/diskunfold-universal" --version 0.1.0 \
    --bundle-id org.example.diskunfold.packagingtest --output-dir "$package_dir" >/dev/null 2>&1; then
    printf 'test-package-dmg: existing output directory was accepted\n' >&2
    exit 1
fi
after=$(shasum -a 256 "$dmg" | awk '{print $1}')
[[ "$before" == "$after" ]]

thin_dir="$test_root/thin-package"
if "$package_script" --binary "$test_root/diskunfold-arm64" --version 0.1.0 \
    --bundle-id org.example.diskunfold.packagingtest --output-dir "$thin_dir" >/dev/null 2>&1; then
    printf 'test-package-dmg: thin binary was accepted\n' >&2
    exit 1
fi
[[ ! -e "$thin_dir" ]]

printf 'macOS DMG packaging checks passed\n'
