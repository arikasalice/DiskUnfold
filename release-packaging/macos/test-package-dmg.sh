#!/bin/bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
repo_root=$(cd "$script_dir/../.." && pwd -P)
package_script="$script_dir/package-dmg.sh"
license_path="$repo_root/LICENSE"
[[ "$(uname -s)" == Darwin ]] || {
    printf 'test-package-dmg: must run on macOS\n' >&2
    exit 1
}
for tool in clang codesign hdiutil lipo python3 mount stat; do
    command -v "$tool" >/dev/null 2>&1 || {
        printf 'test-package-dmg: required tool unavailable: %s\n' "$tool" >&2
        exit 1
    }
done
[[ -f "$license_path" && ! -L "$license_path" && -s "$license_path" ]] || {
    printf 'test-package-dmg: repository LICENSE is missing or invalid\n' >&2
    exit 1
}

tmp_base=${TMPDIR:-/tmp}
test_root=$(mktemp -d "$tmp_base/diskunfold-macos-package-test.XXXXXX")
test_identity=$(stat -f '%d:%i' "$test_root")
mount_path="$test_root/mount"
mount_image=''
attach_attempted=0
mounted=0

image_is_mounted_at() {
    hdiutil info -plist > "$test_root/hdiutil-info.plist" || return 2
    python3 - "$1" "$2" "$test_root/hdiutil-info.plist" <<'PY'
import os
import plistlib
import sys

try:
    image_path = os.path.realpath(sys.argv[1])
    mount_path = os.path.realpath(sys.argv[2])
    with open(sys.argv[3], "rb") as stream:
        images = plistlib.load(stream).get("images", [])
    matches = [
        entity
        for image in images
        if os.path.realpath(image.get("image-path", "")) == image_path
        for entity in image.get("system-entities", [])
        if os.path.realpath(entity.get("mount-point", "")) == mount_path
    ]
except (OSError, ValueError, TypeError):
    sys.exit(2)
sys.exit(0 if len(matches) == 1 else (1 if not matches else 2))
PY
}

path_is_mounted() {
    mount > "$test_root/mount-list.txt" || return 2
    python3 - "$1" "$test_root/mount-list.txt" <<'PY'
import os
import sys

expected = os.path.realpath(sys.argv[1])
try:
    with open(sys.argv[2], encoding="utf-8") as stream:
        for line in stream:
            if " on " not in line:
                continue
            mount_path = line.split(" on ", 1)[1].rsplit(" (", 1)[0]
            if os.path.realpath(mount_path) == expected:
                sys.exit(0)
except OSError:
    sys.exit(2)
sys.exit(1)
PY
}

assert_no_test_mounts() {
    local info_path="$test_root/hdiutil-info.plist"
    hdiutil info -plist > "$info_path"
    python3 - "$test_root" "$info_path" <<'PY'
import os
import plistlib
import sys

root = os.path.realpath(sys.argv[1]).rstrip(os.sep) + os.sep
try:
    with open(sys.argv[2], "rb") as stream:
        images = plistlib.load(stream).get("images", [])
except (OSError, ValueError, TypeError):
    sys.exit(2)
for image in images:
    paths = [image.get("image-path", "")]
    paths.extend(entity.get("mount-point", "") for entity in image.get("system-entities", []))
    if any(os.path.realpath(path).startswith(root) for path in paths if path):
        print(f"test-package-dmg: an image or mount remains under {root}", file=sys.stderr)
        sys.exit(1)
PY
}

require_unmounted_path() {
    local status=0
    if path_is_mounted "$1"; then
        printf 'test-package-dmg: test image remains mounted: %s\n' "$1" >&2
        return 1
    else
        status=$?
    fi
    if ((status != 1)); then
        printf 'test-package-dmg: could not verify mount state: %s\n' "$1" >&2
        return 1
    fi
}

cleanup() {
    local status=$?
    local current_identity=''
    local owned=0
    local image_status=1
    local mount_status=1
    if [[ -d "$test_root" && ! -L "$test_root" ]]; then
        current_identity=$(stat -f '%d:%i' "$test_root" 2>/dev/null || true)
        [[ -n "$current_identity" && "$current_identity" == "$test_identity" ]] && owned=1
    fi
    if ((attach_attempted || mounted)); then
        if ((!owned)) || [[ -z "$mount_image" ]]; then
            printf 'test-package-dmg: staging identity or image path is unknown; preserving %s\n' "$test_root" >&2
            status=1
        else
            if image_is_mounted_at "$mount_image" "$mount_path"; then
                if hdiutil detach "$mount_path" >/dev/null 2>&1; then
                    if require_unmounted_path "$mount_path"; then
                        attach_attempted=0
                        mounted=0
                    else
                        status=1
                    fi
                else
                    printf 'test-package-dmg: could not detach the verified test image; preserving %s\n' "$test_root" >&2
                    status=1
                fi
            else
                image_status=$?
                if ((image_status == 1)); then
                    if path_is_mounted "$mount_path"; then
                        printf 'test-package-dmg: mountpoint belongs to an unverified image; preserving %s\n' "$test_root" >&2
                        status=1
                    else
                        mount_status=$?
                        if ((mount_status != 1)); then
                            printf 'test-package-dmg: mount state is unknown; preserving %s\n' "$test_root" >&2
                            status=1
                        else
                            attach_attempted=0
                            mounted=0
                        fi
                    fi
                else
                    printf 'test-package-dmg: could not match the exact test image and mount; preserving %s\n' "$test_root" >&2
                    status=1
                fi
            fi
        fi
    fi
    if ((owned && !attach_attempted && !mounted)); then
        case "$test_root" in
            "$tmp_base"/diskunfold-macos-package-test.*)
                if assert_no_test_mounts; then
                    rm -rf -- "$test_root" || { printf 'test-package-dmg: could not remove test root: %s\n' "$test_root" >&2; status=1; }
                else
                    printf 'test-package-dmg: preserving test root because an image or mount remains beneath it: %s\n' "$test_root" >&2
                    status=1
                fi
                ;;
            *)
                printf 'test-package-dmg: refusing to remove unexpected test path: %s\n' "$test_root" >&2
                status=1
                ;;
        esac
    elif ((!owned)); then
        printf 'test-package-dmg: refusing to remove test root whose identity changed: %s\n' "$test_root" >&2
        status=1
    fi
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
cat > "$test_root/THIRD-PARTY-NOTICES.fixture.txt" <<'NOTICE'
TEST FIXTURE ONLY. This is not an official third-party notices artifact.
NOTICE
notices_path="$test_root/THIRD-PARTY-NOTICES.fixture.txt"

package_dir="$test_root/package"
"$package_script" --binary "$test_root/diskunfold-universal" --version 0.1.0 \
    --notices "$notices_path" --output-dir "$package_dir"
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
mount_image="$dmg"
attach_attempted=1
hdiutil attach -readonly -nobrowse -mountpoint "$mount_path" "$dmg" >/dev/null
mounted=1
image_is_mounted_at "$dmg" "$mount_path" || {
    printf 'test-package-dmg: could not verify ownership of the attached test image\n' >&2
    exit 1
}
app="$mount_path/DiskUnfold.app"
[[ -x "$app/Contents/MacOS/diskunfold" ]]
[[ -L "$mount_path/Applications" && "$(readlink "$mount_path/Applications")" == /Applications ]]
[[ -f "$mount_path/LICENSE" && ! -L "$mount_path/LICENSE" ]]
[[ -f "$mount_path/THIRD-PARTY-NOTICES.txt" && ! -L "$mount_path/THIRD-PARTY-NOTICES.txt" ]]
cmp -s "$license_path" "$mount_path/LICENSE"
cmp -s "$notices_path" "$mount_path/THIRD-PARTY-NOTICES.txt"
plutil -lint "$app/Contents/Info.plist" >/dev/null
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Contents/Info.plist")" == com.diskunfold.app ]]
[[ "$(lipo -archs "$app/Contents/MacOS/diskunfold" | tr ' ' '\n' | LC_ALL=C sort | paste -sd ' ' -)" == 'arm64 x86_64' ]]
codesign --verify --all-architectures --deep --strict "$app"
signature=$(codesign --display --verbose=4 "$app" 2>&1)
printf '%s\n' "$signature" | grep -Eiq 'Signature=adhoc'
printf '%s\n' "$signature" | grep -Eiq 'flags=.*runtime'
hdiutil detach "$mount_path" >/dev/null
require_unmounted_path "$mount_path"
mounted=0
attach_attempted=0
mount_image=''

before=$(shasum -a 256 "$dmg" | awk '{print $1}')
if "$package_script" --binary "$test_root/diskunfold-universal" --version 0.1.0 \
    --bundle-id com.diskunfold.app --notices "$notices_path" --output-dir "$package_dir" >/dev/null 2>&1; then
    printf 'test-package-dmg: existing output directory was accepted\n' >&2
    exit 1
fi
after=$(shasum -a 256 "$dmg" | awk '{print $1}')
[[ "$before" == "$after" ]]

thin_dir="$test_root/thin-package"
if "$package_script" --binary "$test_root/diskunfold-arm64" --version 0.1.0 \
    --bundle-id com.diskunfold.app --notices "$notices_path" --output-dir "$thin_dir" >/dev/null 2>&1; then
    printf 'test-package-dmg: thin binary was accepted\n' >&2
    exit 1
fi
[[ ! -e "$thin_dir" && ! -L "$thin_dir" ]]

ln -s "$test_root/diskunfold-universal" "$test_root/binary-link"
if "$package_script" --binary "$test_root/binary-link" --version 0.1.0 \
    --bundle-id com.diskunfold.app --notices "$notices_path" --output-dir "$test_root/binary-link-package" >/dev/null 2>&1; then
    printf 'test-package-dmg: binary symlink was accepted\n' >&2
    exit 1
fi
[[ ! -e "$test_root/binary-link-package" ]]

ln -s "$notices_path" "$test_root/notices-link"
if "$package_script" --binary "$test_root/diskunfold-universal" --version 0.1.0 \
    --bundle-id com.diskunfold.app --notices "$test_root/notices-link" \
    --output-dir "$test_root/notices-link-package" >/dev/null 2>&1; then
    printf 'test-package-dmg: notices symlink was accepted\n' >&2
    exit 1
fi
[[ ! -e "$test_root/notices-link-package" ]]

: > "$test_root/empty-notices.txt"
if "$package_script" --binary "$test_root/diskunfold-universal" --version 0.1.0 \
    --bundle-id com.diskunfold.app --notices "$test_root/empty-notices.txt" \
    --output-dir "$test_root/empty-notices-package" >/dev/null 2>&1; then
    printf 'test-package-dmg: empty notices input was accepted\n' >&2
    exit 1
fi
[[ ! -e "$test_root/empty-notices-package" ]]

mkdir "$test_root/output-link-target"
printf 'preserve this file\n' > "$test_root/output-link-target/sentinel.txt"
ln -s "$test_root/output-link-target" "$test_root/output-link"
if "$package_script" --binary "$test_root/diskunfold-universal" --version 0.1.0 \
    --bundle-id com.diskunfold.app --notices "$notices_path" \
    --output-dir "$test_root/output-link" >/dev/null 2>&1; then
    printf 'test-package-dmg: output symlink was accepted\n' >&2
    exit 1
fi
[[ "$(cat "$test_root/output-link-target/sentinel.txt")" == 'preserve this file' ]]

real_hdiutil=$(command -v hdiutil)
wrapper_dir="$test_root/attach-wrapper-bin"
mkdir "$wrapper_dir"
cat > "$wrapper_dir/hdiutil" <<'WRAPPER'
#!/bin/bash
if [[ "${1:-}" == attach ]]; then
    "$REAL_HDIUTIL" "$@" || exit $?
    exit 41
fi
exec "$REAL_HDIUTIL" "$@"
WRAPPER
chmod 0755 "$wrapper_dir/hdiutil"
attach_failure_dir="$test_root/attach-failure-package"
attach_log="$test_root/attach-failure.log"
if REAL_HDIUTIL="$real_hdiutil" PATH="$wrapper_dir:$PATH" "$package_script" \
    --binary "$test_root/diskunfold-universal" --version 0.1.0 \
    --bundle-id com.diskunfold.app --notices "$notices_path" \
    --output-dir "$attach_failure_dir" >"$attach_log" 2>&1; then
    printf 'test-package-dmg: attach failure injection unexpectedly succeeded\n' >&2
    exit 1
fi
grep -q 'could not mount created disk image' "$attach_log"
if grep -Eq 'preserving staging|could not detach the verified image' "$attach_log"; then
    printf 'test-package-dmg: attach failure cleanup did not finish\n' >&2
    cat "$attach_log" >&2
    exit 1
fi
[[ ! -e "$attach_failure_dir" && ! -L "$attach_failure_dir" ]]
if compgen -G "$test_root/.work.*" >/dev/null; then
    printf 'test-package-dmg: failed attach left staging files behind\n' >&2
    exit 1
fi
assert_no_test_mounts

alternate_mount="$test_root/alternate-mount"
mkdir "$alternate_mount"
alternate_wrapper_dir="$test_root/alternate-mount-wrapper-bin"
mkdir "$alternate_wrapper_dir"
cat > "$alternate_wrapper_dir/hdiutil" <<'WRAPPER'
#!/bin/bash
if [[ "${1:-}" == attach ]]; then
    shift
    args=()
    while (($#)); do
        if [[ "$1" == -mountpoint ]]; then
            args+=(-mountpoint "$ALT_MOUNT")
            shift 2
        else
            args+=("$1")
            shift
        fi
    done
    "$REAL_HDIUTIL" attach "${args[@]}"
    result=$?
    ((result == 0)) || exit "$result"
    exit 42
fi
exec "$REAL_HDIUTIL" "$@"
WRAPPER
chmod 0755 "$alternate_wrapper_dir/hdiutil"
alternate_output="$test_root/alternate-mount-package"
alternate_log="$test_root/alternate-mount.log"
if ALT_MOUNT="$alternate_mount" REAL_HDIUTIL="$real_hdiutil" PATH="$alternate_wrapper_dir:$PATH" \
    "$package_script" --binary "$test_root/diskunfold-universal" --version 0.1.0 \
    --bundle-id com.diskunfold.app --notices "$notices_path" \
    --output-dir "$alternate_output" >"$alternate_log" 2>&1; then
    printf 'test-package-dmg: unexpected-mountpoint injection unexpectedly succeeded\n' >&2
    exit 1
fi
grep -q 'generated image is mounted somewhere other than its expected mount point' "$alternate_log"
[[ ! -e "$alternate_output" && ! -L "$alternate_output" ]]
work_dirs=("$test_root"/.work.*)
[[ ${#work_dirs[@]} -eq 1 && -d "${work_dirs[0]}" && ! -L "${work_dirs[0]}" ]]
preserved_work=${work_dirs[0]}
preserved_identity=$(stat -f '%d:%i' "$preserved_work")
preserved_image="$preserved_work/DiskUnfold-0.1.0-universal.dmg"
image_is_mounted_at "$preserved_image" "$alternate_mount"
attach_attempted=1
mounted=1
mount_image="$preserved_image"
mount_path="$alternate_mount"
hdiutil detach "$alternate_mount" >/dev/null
require_unmounted_path "$alternate_mount"
attach_attempted=0
mounted=0
mount_image=''
assert_no_test_mounts
[[ -d "$preserved_work" && ! -L "$preserved_work" ]]
[[ "$(stat -f '%d:%i' "$preserved_work")" == "$preserved_identity" ]]
rm -rf -- "$preserved_work"
mount_path="$test_root/mount"

real_python3=$(command -v python3)
race_wrapper_dir="$test_root/race-wrapper-bin"
mkdir "$race_wrapper_dir"
cat > "$race_wrapper_dir/python3" <<'WRAPPER'
#!/bin/bash
set -euo pipefail
script_file=$(mktemp "$TEST_ROOT/python-wrapper.XXXXXX")
cat > "$script_file"
if grep -q 'renamex_np' "$script_file"; then
    mkdir "$RACE_OUTPUT"
    printf 'external actor sentinel\n' > "$RACE_OUTPUT/sentinel.txt"
fi
"$REAL_PYTHON3" "$@" < "$script_file"
status=$?
rm -f -- "$script_file"
exit "$status"
WRAPPER
chmod 0755 "$race_wrapper_dir/python3"
race_output="$test_root/raced-output"
race_log="$test_root/race-publish.log"
if TEST_ROOT="$test_root" RACE_OUTPUT="$race_output" REAL_PYTHON3="$real_python3" \
    PATH="$race_wrapper_dir:$PATH" "$package_script" \
    --binary "$test_root/diskunfold-universal" --version 0.1.0 \
    --bundle-id com.diskunfold.app --notices "$notices_path" \
    --output-dir "$race_output" >"$race_log" 2>&1; then
    printf 'test-package-dmg: output race injection unexpectedly succeeded\n' >&2
    exit 1
fi
[[ -f "$race_output/sentinel.txt" && "$(cat "$race_output/sentinel.txt")" == 'external actor sentinel' ]]
[[ ! -e "$race_output/manifest.json" && ! -e "$race_output/DiskUnfold-0.1.0-universal.dmg" ]]
grep -q 'could not atomically publish the complete package directory' "$race_log"
if compgen -G "$test_root/.work.*" >/dev/null; then
    printf 'test-package-dmg: output race left staging files behind\n' >&2
    exit 1
fi
assert_no_test_mounts


# A named signing identity makes the designated requirement "identifier + certificate", so it
# does not change between builds; ad-hoc signing pins it to the executable hash instead.
for tool in openssl security; do
    command -v "$tool" >/dev/null 2>&1 || { printf 'test-package-dmg: required tool unavailable: %s\n' "$tool" >&2; exit 1; }
done
identity_name='DiskUnfold Test Signing'
cat > "$test_root/identity.cnf" <<'CNF'
[req]
distinguished_name=dn
x509_extensions=ext
prompt=no
[dn]
CN=DiskUnfold Test Signing
[ext]
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature
extendedKeyUsage=critical,codeSigning
CNF
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$test_root/identity.key" \
    -out "$test_root/identity.crt" -days 2 -config "$test_root/identity.cnf" >/dev/null 2>&1
keychain_secret=$(openssl rand -hex 12)
openssl pkcs12 -export -inkey "$test_root/identity.key" -in "$test_root/identity.crt" \
    -name "$identity_name" -out "$test_root/identity.p12" -passout "pass:$keychain_secret" >/dev/null 2>&1
test_keychain="$test_root/signing.keychain-db"
security create-keychain -p "$keychain_secret" "$test_keychain"
security set-keychain-settings "$test_keychain"
security unlock-keychain -p "$keychain_secret" "$test_keychain"
security import "$test_root/identity.p12" -k "$test_keychain" -P "$keychain_secret" -T /usr/bin/codesign >/dev/null
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$keychain_secret" "$test_keychain" >/dev/null

# A second build: the same program with different bytes.
cat > "$test_root/main2.c" <<'SOURCE'
int main(void) { return 1; }
SOURCE
clang -arch arm64 -mmacosx-version-min=15.0 -O2 -o "$test_root/second-arm64" "$test_root/main2.c"
clang -arch x86_64 -mmacosx-version-min=15.0 -O2 -o "$test_root/second-x86_64" "$test_root/main2.c"
lipo -create "$test_root/second-arm64" "$test_root/second-x86_64" -output "$test_root/second-universal"

requirement_of() { codesign --display --requirements - "$1" 2>&1 | grep 'designated =>'; }
first_signed="$test_root/signed-1"
second_signed="$test_root/signed-2"
"$package_script" --binary "$test_root/diskunfold-universal" --version 0.1.0 \
    --signing-identity "$identity_name" --keychain "$test_keychain" \
    --notices "$notices_path" --output-dir "$first_signed" >/dev/null
"$package_script" --binary "$test_root/second-universal" --version 0.1.0 \
    --signing-identity "$identity_name" --keychain "$test_keychain" \
    --notices "$notices_path" --output-dir "$second_signed" >/dev/null
first_requirement=$(requirement_of "$first_signed/unpacked/macos-universal/diskunfold")
second_requirement=$(requirement_of "$second_signed/unpacked/macos-universal/diskunfold")
printf '%s\n' "$first_requirement" | grep -Eq 'identifier "com.diskunfold.app" and certificate (leaf|root) = H"[0-9a-f]{40}"'
if printf '%s\n' "$first_requirement" | grep -Eiq 'cdhash'; then
    printf 'test-package-dmg: identity-signed requirement depends on a build hash\n' >&2
    exit 1
fi
[[ "$first_requirement" == "$second_requirement" ]] || {
    printf 'test-package-dmg: requirement changed between builds: %s vs %s\n' "$first_requirement" "$second_requirement" >&2
    exit 1
}
python3 - "$first_signed/manifest.json" <<'PY'
import json
import sys

row = json.load(open(sys.argv[1], encoding="utf-8"))["rows"][0]
assert row["signature"] == "hardened_runtime_self_signed", row["signature"]
assert "not notarized" in row["signature_scope"] and "Not a Developer ID" in row["signature_scope"]
print("self-signed manifest label verified")
PY

# The ad-hoc requirement of the same two builds does differ, which is why grants were lost.
adhoc_second="$test_root/adhoc-2"
"$package_script" --binary "$test_root/second-universal" --version 0.1.0 \
    --notices "$notices_path" --output-dir "$adhoc_second" >/dev/null
adhoc_first_requirement=$(requirement_of "$package_dir/unpacked/macos-universal/diskunfold")
adhoc_second_requirement=$(requirement_of "$adhoc_second/unpacked/macos-universal/diskunfold")
printf '%s\n' "$adhoc_first_requirement" | grep -Eiq 'cdhash'
[[ "$adhoc_first_requirement" != "$adhoc_second_requirement" ]]

# An identity that is not in the keychain stops the packager and leaves no output.
if "$package_script" --binary "$test_root/diskunfold-universal" --version 0.1.0 \
    --signing-identity 'DiskUnfold Missing Identity' --keychain "$test_keychain" \
    --notices "$notices_path" --output-dir "$test_root/missing-identity" >/dev/null 2>&1; then
    printf 'test-package-dmg: a missing signing identity was accepted\n' >&2
    exit 1
fi
[[ ! -e "$test_root/missing-identity" ]]
security delete-keychain "$test_keychain"
assert_no_test_mounts

printf 'macOS DMG packaging checks passed (arm64 host; x86_64 slice cross-compiled)\n'

