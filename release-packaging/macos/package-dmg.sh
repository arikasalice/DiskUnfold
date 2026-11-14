#!/bin/bash
# Assemble a signed universal DiskUnfold.app and a drag-install DMG.
set -euo pipefail

usage() {
    cat <<'USAGE'
Usage: package-dmg.sh --binary PATH --version X.Y.Z [--bundle-id REVERSE_DNS] [--signing-identity NAME [--keychain PATH]] --notices PATH --output-dir NEW_DIR

--bundle-id defaults to the project identifier com.diskunfold.app.
--signing-identity names a code-signing identity in the keychain (--keychain, or the default
search list). The default is ad-hoc signing ("-"), whose code requirement is the executable's
hash and so changes with every build. A named identity makes the code requirement
"identifier + certificate", which stays the same between builds, so macOS keeps privacy
grants such as Full Disk Access across updates. The identity is meant for the fixed
self-signed certificate; it is not a Developer ID and the result is not notarized.
The output directory must not exist. The DMG contains the app, repository
LICENSE, and the explicitly supplied THIRD-PARTY-NOTICES file. The output also
contains a one-row release-tools manifest.json and the signed executable staged
for digest re-verification. This script does not validate notice completeness,
build the binary, create provenance, publish artifacts, notarize, or install
the app.
USAGE
}

fail() {
    printf 'package-dmg: %s\n' "$*" >&2
    exit 1
}

binary=''
version=''
bundle_id='com.diskunfold.app'
notices=''
output_dir=''
signing_identity='-'
keychain=''
while (($#)); do
    case "$1" in
        --binary|--version|--bundle-id|--notices|--output-dir|--signing-identity|--keychain)
            (($# >= 2)) || fail "missing value for $1"
            case "$1" in
                --binary) binary=$2 ;;
                --version) version=$2 ;;
                --bundle-id) bundle_id=$2 ;;
                --notices) notices=$2 ;;
                --output-dir) output_dir=$2 ;;
                --signing-identity) signing_identity=$2 ;;
                --keychain) keychain=$2 ;;
            esac
            shift 2
            ;;
        --help|-h)
            usage
            exit 0
            ;;
        *)
            fail "unknown argument: $1"
            ;;
    esac
done

[[ -n "$binary" && -n "$version" && -n "$bundle_id" && -n "$notices" && -n "$output_dir" ]] || {
    usage >&2
    fail '--binary, --version, --notices and --output-dir are required'
}
[[ "$version" =~ ^[0-9]+(\.[0-9]+){1,2}$ ]] || fail 'version must be numeric, such as 0.0.1'
[[ "$bundle_id" =~ ^[A-Za-z][A-Za-z0-9-]*(\.[A-Za-z][A-Za-z0-9-]*)+$ ]] || \
    fail 'bundle identifier must be reverse-DNS text; the project identifier is com.diskunfold.app'
[[ "$binary" == /* ]] || binary="$PWD/$binary"
[[ "$notices" == /* ]] || notices="$PWD/$notices"
[[ "$output_dir" == /* ]] || output_dir="$PWD/$output_dir"
[[ -f "$binary" && ! -L "$binary" && -x "$binary" ]] || \
    fail 'binary must be an executable regular file, not a symbolic link'
[[ -f "$notices" && ! -L "$notices" && -s "$notices" ]] || \
    fail 'notices must be a nonempty regular file, not a symbolic link'

for tool in codesign hdiutil lipo plutil shasum ditto python3 mount stat; do
    command -v "$tool" >/dev/null 2>&1 || fail "required macOS tool is unavailable: $tool"
done
[[ "$(uname -s)" == Darwin ]] || fail 'must run on macOS'

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
icon_path="$script_dir/../../app-host/icons/icon.icns"
license_path="$script_dir/../../LICENSE"
entitlements_path="$script_dir/entitlements.plist"
[[ -f "$icon_path" ]] || fail "application icon is missing: $icon_path"
[[ -f "$license_path" && ! -L "$license_path" && -s "$license_path" ]] || \
    fail "repository LICENSE is missing or invalid: $license_path"
[[ -f "$entitlements_path" ]] || fail "entitlements file is missing: $entitlements_path"

binary_arches=$(lipo -archs "$binary") || fail 'input is not a readable Mach-O universal binary'
sorted_arches=$(printf '%s\n' "$binary_arches" | tr ' ' '\n' | LC_ALL=C sort | paste -sd ' ' -)
[[ "$sorted_arches" == 'arm64 x86_64' ]] || \
    fail "binary must contain exactly arm64 and x86_64 slices; found: $binary_arches"

output_parent_input=$(dirname "$output_dir")
output_name=$(basename "$output_dir")
[[ "$output_name" != '.' && "$output_name" != '/' ]] || fail 'output directory must have a final path component'
[[ -d "$output_parent_input" ]] || fail 'parent of output directory must already exist'
output_parent=$(cd "$output_parent_input" && pwd -P)
output_root="$output_parent/$output_name"
[[ ! -e "$output_root" && ! -L "$output_root" ]] || fail 'output directory already exists; choose a unique new path'

work_dir=''
work_identity=''
image_path=''
mount_path=''
mounted=0
attach_attempted=0

work_identity_is_current() {
    local current_identity=''
    [[ -n "$work_dir" && -d "$work_dir" && ! -L "$work_dir" ]] || return 1
    current_identity=$(stat -f '%d:%i' "$work_dir" 2>/dev/null) || return 2
    [[ -n "$work_identity" && "$current_identity" == "$work_identity" ]]
}

image_is_mounted_at() {
    local image_path=$1
    local expected_mount=$2
    local info_path="$work_dir/.hdiutil-info.plist"
    hdiutil info -plist > "$info_path" || return 2
    python3 - "$image_path" "$expected_mount" "$info_path" <<'PY'
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

image_has_mounted_volume() {
    local image_path=$1
    local info_path="$work_dir/.hdiutil-info.plist"
    hdiutil info -plist > "$info_path" || return 2
    python3 - "$image_path" "$info_path" <<'PY'
import os
import plistlib
import sys

try:
    image_path = os.path.realpath(sys.argv[1])
    with open(sys.argv[2], "rb") as stream:
        images = plistlib.load(stream).get("images", [])
    matching = [
        image
        for image in images
        if os.path.realpath(image.get("image-path", "")) == image_path
    ]
    if len(matching) > 1:
        sys.exit(2)
    mounted = [
        entity
        for image in matching
        for entity in image.get("system-entities", [])
        if entity.get("mount-point")
    ]
except (OSError, ValueError, TypeError):
    sys.exit(2)
sys.exit(0 if mounted else 1)
PY
}

require_unmounted_path() {
    local status=0
    if path_is_mounted "$1"; then
        fail "disk image remains mounted: $1"
    else
        status=$?
    fi
    ((status == 1)) || fail "could not verify mount state for: $1"
}

path_is_mounted() {
    local mount_list_path="$work_dir/.mount-list.txt"
    mount > "$mount_list_path" || return 2
    python3 - "$1" "$mount_list_path" <<'PY'
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

cleanup() {
    local exit_status=$?
    local cleanup_failed=0
    local mount_status=1
    local work_owned=0
    if work_identity_is_current; then
        work_owned=1
    fi
    if ((mounted || attach_attempted)); then
        if ((!work_owned)); then
            printf 'package-dmg: staging directory identity changed; refusing to inspect or detach from it: %s\n' "$work_dir" >&2
            cleanup_failed=1
        elif image_is_mounted_at "$image_path" "$mount_path"; then
            if hdiutil detach "$mount_path" >/dev/null 2>&1; then
                if path_is_mounted "$mount_path"; then
                    printf 'package-dmg: image remains mounted after detach; preserving staging directory: %s\n' "$mount_path" >&2
                    cleanup_failed=1
                else
                    mount_status=$?
                    if ((mount_status == 1)); then
                        mounted=0
                        attach_attempted=0
                    else
                        printf 'package-dmg: mount state is unknown after detach; preserving staging directory: %s\n' "$mount_path" >&2
                        cleanup_failed=1
                    fi
                fi
            else
                printf 'package-dmg: could not detach the verified image owned by this run; preserving staging directory: %s\n' "$mount_path" >&2
                cleanup_failed=1
            fi
        else
            mount_status=$?
            if ((mount_status == 1)); then
                if image_has_mounted_volume "$image_path"; then
                    printf 'package-dmg: generated image is mounted somewhere other than its expected mount point; preserving staging directory: %s\n' "$image_path" >&2
                    cleanup_failed=1
                else
                    mount_status=$?
                    if ((mount_status != 1)); then
                        printf 'package-dmg: could not verify whether generated image is mounted; preserving staging directory: %s\n' "$image_path" >&2
                        cleanup_failed=1
                    else
                        if path_is_mounted "$mount_path"; then
                            printf 'package-dmg: mount path is occupied by an image not verified as this run\x27s image; preserving it: %s\n' "$mount_path" >&2
                            cleanup_failed=1
                        else
                            mount_status=$?
                            if ((mount_status == 1)); then
                                mounted=0
                                attach_attempted=0
                            else
                                printf 'package-dmg: could not verify mount state; preserving staging directory: %s\n' "$mount_path" >&2
                                cleanup_failed=1
                            fi
                        fi
                    fi
                fi
            else
                printf 'package-dmg: could not match the generated image and exact mount point in system state; preserving staging directory: %s\n' "$mount_path" >&2
                cleanup_failed=1
            fi
        fi
    fi
    if [[ -n "$work_dir" ]]; then
        case "$work_dir" in
            "$output_parent"/.work.*)
                if ((!work_owned)); then
                    printf 'package-dmg: refusing to remove a staging path whose identity changed: %s\n' "$work_dir" >&2
                    cleanup_failed=1
                elif ((!mounted && !attach_attempted)); then
                    rm -rf -- "$work_dir" || {
                        printf 'package-dmg: could not remove staging directory: %s\n' "$work_dir" >&2
                        cleanup_failed=1
                    }
                fi
                ;;
            *)
                printf 'package-dmg: refusing to remove unexpected work path: %s\n' "$work_dir" >&2
                cleanup_failed=1
                ;;
        esac
    fi
    if ((cleanup_failed)); then
        exit_status=1
    fi
    exit "$exit_status"
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

work_dir=$(mktemp -d "$output_parent/.work.XXXXXX") || fail 'could not create private staging directory'
work_identity=$(stat -f '%d:%i' "$work_dir") || fail 'could not identify private staging directory'
app_root="$work_dir/DiskUnfold.app"
contents="$app_root/Contents"
mkdir -p "$contents/MacOS" "$contents/Resources" "$work_dir/dmg-root" "$work_dir/mount" \
    "$work_dir/unpacked/macos-universal"
install -m 0755 "$binary" "$contents/MacOS/diskunfold"
install -m 0644 "$icon_path" "$contents/Resources/DiskUnfold.icns"
install -m 0644 "$license_path" "$work_dir/dmg-root/LICENSE"
install -m 0644 "$notices" "$work_dir/dmg-root/THIRD-PARTY-NOTICES.txt"
cat > "$contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleDisplayName</key><string>DiskUnfold</string>
  <key>CFBundleExecutable</key><string>diskunfold</string>
  <key>CFBundleIconFile</key><string>DiskUnfold.icns</string>
  <key>CFBundleIdentifier</key><string>$bundle_id</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>DiskUnfold</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.utilities</string>
  <key>LSMinimumSystemVersion</key><string>15.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSPrincipalClass</key><string>NSApplication</string>
</dict>
</plist>
PLIST
printf 'APPL????' > "$contents/PkgInfo"
plutil -lint "$contents/Info.plist" >/dev/null || fail 'generated Info.plist is invalid'

# Ad-hoc signing preserves Hardened Runtime without asserting an Apple identity. A named identity
# (the fixed self-signed certificate) keeps the code requirement the same between builds.
sign_args=(--force --sign "$signing_identity" --timestamp=none --options runtime
    --entitlements "$entitlements_path")
[[ -z "$keychain" ]] || sign_args+=(--keychain "$keychain")
codesign "${sign_args[@]}" "$app_root"
codesign --verify --all-architectures --deep --strict "$app_root" || fail 'app signature verification failed'
signature_details=$(codesign --display --verbose=4 "$app_root" 2>&1) || fail 'could not inspect app signature'
printf '%s\n' "$signature_details" | grep -Eiq 'flags=.*runtime' || fail 'Hardened Runtime is not enabled'
requirement=$(codesign --display --requirements - "$app_root" 2>&1) || fail 'could not read the designated requirement'
if [[ "$signing_identity" == '-' ]]; then
    printf '%s\n' "$signature_details" | grep -Eiq 'Signature=adhoc' || fail 'signature is not ad-hoc'
    signature_kind=hardened_runtime_ad_hoc
    signature_scope='Hardened Runtime ad-hoc signature seals DiskUnfold.app; the DMG is not separately code-signed. No Apple developer identity and no notarization. The code requirement is a per-build hash, so macOS privacy grants do not carry over to a later build.'
else
    # The requirement must name the certificate, not a build hash, or privacy grants would not
    # survive an update.
    printf '%s\n' "$signature_details" | grep -Eiq 'Signature=adhoc' && fail 'signing identity produced an ad-hoc signature'
    # A self-signed certificate is its own root, so codesign may print "certificate root" where a
    # longer chain prints "certificate leaf"; both pin the requirement to this one certificate.
    printf '%s\n' "$requirement" | grep -Eq 'designated => .*identifier "'"$bundle_id"'" and certificate (leaf|root) = H"[0-9a-f]{40}"' || \
        fail 'designated requirement is not identifier plus certificate'
    printf '%s\n' "$requirement" | grep -Eiq 'cdhash' && fail 'designated requirement still depends on a build hash'
    signature_kind=hardened_runtime_self_signed
    signature_scope='Hardened Runtime signature with the fixed self-signed DiskUnfold Development Signing certificate seals DiskUnfold.app; the DMG is not separately code-signed. Not a Developer ID signature and not notarized: macOS reports the developer as unidentified. The code requirement (identifier plus certificate) is the same for every build so macOS privacy grants carry over.'
fi

signed_executable="$contents/MacOS/diskunfold"
# Input file first: older lipo releases (Xcode 16) reject the operation-first order.
lipo "$signed_executable" -verify_arch arm64 >/dev/null || fail 'signed executable lost its arm64 slice'
lipo "$signed_executable" -verify_arch x86_64 >/dev/null || fail 'signed executable lost its x86_64 slice'
signed_arches=$(lipo -archs "$signed_executable")
sorted_signed_arches=$(printf '%s\n' "$signed_arches" | tr ' ' '\n' | LC_ALL=C sort | paste -sd ' ' -)
[[ "$sorted_signed_arches" == 'arm64 x86_64' ]] || fail 'signed executable architecture set changed'

dmg_name="DiskUnfold-$version-universal.dmg"
image_path="$work_dir/$dmg_name"
ditto --rsrc --extattr "$app_root" "$work_dir/dmg-root/DiskUnfold.app"
ln -s /Applications "$work_dir/dmg-root/Applications"
hdiutil create -quiet -format UDZO -imagekey zlib-level=9 -volname DiskUnfold \
    -srcfolder "$work_dir/dmg-root" "$image_path"
hdiutil verify "$image_path" >/dev/null || fail 'created disk image failed verification'

mount_path="$work_dir/mount"
attach_attempted=1
hdiutil attach -readonly -nobrowse -mountpoint "$mount_path" "$image_path" >/dev/null || \
    fail 'could not mount created disk image for content verification'
mounted=1
image_is_mounted_at "$image_path" "$mount_path" || \
    fail 'could not verify that the mounted image belongs to this run'
attach_attempted=0
mounted_app="$mount_path/DiskUnfold.app"
[[ -d "$mounted_app" && -x "$mounted_app/Contents/MacOS/diskunfold" ]] || \
    fail 'disk image does not contain the expected application bundle'
[[ -L "$mount_path/Applications" && "$(readlink "$mount_path/Applications")" == /Applications ]] || \
    fail 'disk image does not contain the Applications folder alias'
[[ -f "$mount_path/LICENSE" && ! -L "$mount_path/LICENSE" ]] || fail 'disk image does not contain LICENSE'
[[ -f "$mount_path/THIRD-PARTY-NOTICES.txt" && ! -L "$mount_path/THIRD-PARTY-NOTICES.txt" ]] || \
    fail 'disk image does not contain THIRD-PARTY-NOTICES'
cmp -s "$license_path" "$mount_path/LICENSE" || fail 'packaged LICENSE differs from repository LICENSE'
cmp -s "$notices" "$mount_path/THIRD-PARTY-NOTICES.txt" || fail 'packaged notices differ from supplied notices'
codesign --verify --all-architectures --deep --strict "$mounted_app" || fail 'mounted app signature verification failed'
mounted_arches=$(lipo -archs "$mounted_app/Contents/MacOS/diskunfold")
sorted_mounted_arches=$(printf '%s\n' "$mounted_arches" | tr ' ' '\n' | LC_ALL=C sort | paste -sd ' ' -)
[[ "$sorted_mounted_arches" == 'arm64 x86_64' ]] || fail 'mounted executable architecture set is invalid'

cp -p "$mounted_app/Contents/MacOS/diskunfold" "$work_dir/unpacked/macos-universal/diskunfold"
source_digest=$(shasum -a 256 "$signed_executable" | awk '{print $1}')
mounted_digest=$(shasum -a 256 "$work_dir/unpacked/macos-universal/diskunfold" | awk '{print $1}')
[[ "$source_digest" == "$mounted_digest" ]] || fail 'packaged executable digest does not match the signed app executable'
package_digest=$(shasum -a 256 "$image_path" | awk '{print $1}')
[[ "$package_digest" =~ ^[0-9a-f]{64}$ && "$mounted_digest" =~ ^[0-9a-f]{64}$ ]] || \
    fail 'SHA-256 tool returned an invalid digest'
image_is_mounted_at "$image_path" "$mount_path" || fail 'disk image ownership changed before detach'
hdiutil detach "$mount_path" >/dev/null || fail 'could not detach the disk image created by this run'
require_unmounted_path "$mount_path"
mounted=0
attach_attempted=0

# This is the exact du-release-tools ReleaseManifest shape; it is a staging
# manifest, not provenance or a publication attestation.
cat > "$work_dir/manifest.json" <<MANIFEST
{
  "product_name": "DiskUnfold",
  "version": "$version",
  "rows": [{
    "file_name": "$dmg_name",
    "version": "$version",
    "os": {"family": "macos", "minimum": "15", "maximum": null},
    "cpu": "universal",
    "format": "dmg",
    "channel": "direct",
    "signature": "$signature_kind",
    "signature_scope": "$signature_scope",
    "sha256": "$package_digest",
    "executable_file_name": "diskunfold",
    "executable_sha256": "$mounted_digest",
    "run_instructions": "Open the disk image, drag DiskUnfold.app to Applications, then launch it."
  }],
  "channel_references": [],
  "staged_files": [
    {"artifact_file_name": "$dmg_name", "kind": "package", "relative_path": "$dmg_name"},
    {"artifact_file_name": "$dmg_name", "kind": "executable", "relative_path": "unpacked/macos-universal/diskunfold"}
  ]
}
MANIFEST

final_dir="$work_dir/final"
mkdir "$final_dir"
mv "$work_dir/unpacked" "$final_dir/unpacked"
mv "$work_dir/manifest.json" "$final_dir/manifest.json"
mv "$image_path" "$final_dir/$dmg_name"
python3 - "$final_dir" "$output_root" <<'PY' || fail 'could not atomically publish the complete package directory'
import ctypes
import os
import sys

source, destination = sys.argv[1:]
before = os.stat(source, follow_symlinks=False)
rename_exclusive = getattr(ctypes.CDLL(None, use_errno=True), "renamex_np", None)
if rename_exclusive is None:
    raise SystemExit("renamex_np is unavailable; refusing a non-atomic publication")
rename_exclusive.argtypes = (ctypes.c_char_p, ctypes.c_char_p, ctypes.c_uint)
rename_exclusive.restype = ctypes.c_int
if rename_exclusive(os.fsencode(source), os.fsencode(destination), 0x00000004) != 0:
    code = ctypes.get_errno()
    raise OSError(code, os.strerror(code), destination)
after = os.stat(destination, follow_symlinks=False)
if (before.st_dev, before.st_ino) != (after.st_dev, after.st_ino):
    raise SystemExit("published directory identity does not match staged directory")
if os.path.lexists(source):
    raise SystemExit("staging directory remains after publication")
PY
completed=1
printf 'Created %s\n' "$output_root/$dmg_name"
printf 'Manifest: %s\n' "$output_root/manifest.json"
printf 'Staged executable: %s\n' "$output_root/unpacked/macos-universal/diskunfold"
