#!/bin/bash
# Assemble a signed universal DiskUnfold.app and a drag-install DMG.
set -euo pipefail

usage() {
    cat <<'USAGE'
Usage: package-dmg.sh --binary PATH --version X.Y.Z --bundle-id REVERSE_DNS --output-dir NEW_DIR

The output directory must not exist. It will contain the DMG, a one-row
release-tools manifest.json, and the signed executable staged for digest
re-verification. This script does not build the binary, create provenance,
publish artifacts, notarize, or install the app.
USAGE
}

fail() {
    printf 'package-dmg: %s\n' "$*" >&2
    exit 1
}

binary=''
version=''
bundle_id=''
output_dir=''
while (($#)); do
    case "$1" in
        --binary|--version|--bundle-id|--output-dir)
            (($# >= 2)) || fail "missing value for $1"
            case "$1" in
                --binary) binary=$2 ;;
                --version) version=$2 ;;
                --bundle-id) bundle_id=$2 ;;
                --output-dir) output_dir=$2 ;;
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

[[ -n "$binary" && -n "$version" && -n "$bundle_id" && -n "$output_dir" ]] || {
    usage >&2
    fail 'all four options are required'
}
[[ "$version" =~ ^[0-9]+(\.[0-9]+){1,2}$ ]] || fail 'version must be numeric, such as 0.1.0'
[[ "$bundle_id" =~ ^[A-Za-z][A-Za-z0-9-]*(\.[A-Za-z][A-Za-z0-9-]*)+$ ]] || \
    fail 'bundle identifier must be reverse-DNS text; use the project-confirmed identifier'
[[ -f "$binary" && ! -L "$binary" && -x "$binary" ]] || \
    fail 'binary must be an executable regular file, not a symbolic link'

for tool in codesign hdiutil lipo plutil shasum ditto; do
    command -v "$tool" >/dev/null 2>&1 || fail "required macOS tool is unavailable: $tool"
done
[[ "$(uname -s)" == Darwin ]] || fail 'must run on macOS'

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
icon_path="$script_dir/../../app-host/icons/icon.icns"
entitlements_path="$script_dir/entitlements.plist"
[[ -f "$icon_path" ]] || fail "application icon is missing: $icon_path"
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
mkdir "$output_root" || fail 'could not claim output directory'

work_dir=''
mount_path=''
mounted=0
completed=0
cleanup() {
    status=$?
    if ((mounted)); then
        hdiutil detach "$mount_path" >/dev/null 2>&1 || true
    fi
    if [[ -n "$work_dir" && -d "$work_dir" ]]; then
        case "$work_dir" in
            "$output_root"/.work.*) rm -rf -- "$work_dir" ;;
            *) printf 'package-dmg: refusing to remove unexpected work path: %s\n' "$work_dir" >&2 ;;
        esac
    fi
    if ((!completed)) && [[ -d "$output_root" ]]; then
        rmdir "$output_root" 2>/dev/null || true
    fi
    exit "$status"
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

work_dir=$(mktemp -d "$output_root/.work.XXXXXX") || fail 'could not create private staging directory'
app_root="$work_dir/DiskUnfold.app"
contents="$app_root/Contents"
mkdir -p "$contents/MacOS" "$contents/Resources" "$work_dir/dmg-root" "$work_dir/mount" \
    "$work_dir/unpacked/macos-universal"
install -m 0755 "$binary" "$contents/MacOS/diskunfold"
install -m 0644 "$icon_path" "$contents/Resources/DiskUnfold.icns"
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

# Ad-hoc signing preserves Hardened Runtime without asserting an Apple identity.
codesign --force --sign - --timestamp=none --options runtime \
    --entitlements "$entitlements_path" "$app_root"
codesign --verify --deep --strict "$app_root" || fail 'ad-hoc app signature verification failed'
signature_details=$(codesign --display --verbose=4 "$app_root" 2>&1) || fail 'could not inspect app signature'
printf '%s\n' "$signature_details" | grep -Eiq 'Signature=adhoc' || fail 'signature is not ad-hoc'
printf '%s\n' "$signature_details" | grep -Eiq 'flags=.*runtime' || fail 'Hardened Runtime is not enabled'

signed_executable="$contents/MacOS/diskunfold"
lipo -verify_arch arm64 "$signed_executable" >/dev/null || fail 'signed executable lost its arm64 slice'
lipo -verify_arch x86_64 "$signed_executable" >/dev/null || fail 'signed executable lost its x86_64 slice'
signed_arches=$(lipo -archs "$signed_executable")
sorted_signed_arches=$(printf '%s\n' "$signed_arches" | tr ' ' '\n' | LC_ALL=C sort | paste -sd ' ' -)
[[ "$sorted_signed_arches" == 'arm64 x86_64' ]] || fail 'signed executable architecture set changed'

dmg_name="DiskUnfold-$version-universal.dmg"
ditto --rsrc --extattr "$app_root" "$work_dir/dmg-root/DiskUnfold.app"
ln -s /Applications "$work_dir/dmg-root/Applications"
hdiutil create -quiet -format UDZO -imagekey zlib-level=9 -volname DiskUnfold \
    -srcfolder "$work_dir/dmg-root" "$work_dir/$dmg_name"
hdiutil verify "$work_dir/$dmg_name" >/dev/null || fail 'created disk image failed verification'

mount_path="$work_dir/mount"
hdiutil attach -readonly -nobrowse -mountpoint "$mount_path" "$work_dir/$dmg_name" >/dev/null || \
    fail 'could not mount created disk image for content verification'
mounted=1
mounted_app="$mount_path/DiskUnfold.app"
[[ -d "$mounted_app" && -x "$mounted_app/Contents/MacOS/diskunfold" ]] || \
    fail 'disk image does not contain the expected application bundle'
[[ -L "$mount_path/Applications" && "$(readlink "$mount_path/Applications")" == /Applications ]] || \
    fail 'disk image does not contain the Applications folder alias'
codesign --verify --deep --strict "$mounted_app" || fail 'mounted app signature verification failed'
mounted_arches=$(lipo -archs "$mounted_app/Contents/MacOS/diskunfold")
sorted_mounted_arches=$(printf '%s\n' "$mounted_arches" | tr ' ' '\n' | LC_ALL=C sort | paste -sd ' ' -)
[[ "$sorted_mounted_arches" == 'arm64 x86_64' ]] || fail 'mounted executable architecture set is invalid'

cp -p "$mounted_app/Contents/MacOS/diskunfold" "$work_dir/unpacked/macos-universal/diskunfold"
source_digest=$(shasum -a 256 "$signed_executable" | awk '{print $1}')
mounted_digest=$(shasum -a 256 "$work_dir/unpacked/macos-universal/diskunfold" | awk '{print $1}')
[[ "$source_digest" == "$mounted_digest" ]] || fail 'packaged executable digest does not match the signed app executable'
package_digest=$(shasum -a 256 "$work_dir/$dmg_name" | awk '{print $1}')
[[ "$package_digest" =~ ^[0-9a-f]{64}$ && "$mounted_digest" =~ ^[0-9a-f]{64}$ ]] || \
    fail 'SHA-256 tool returned an invalid digest'
hdiutil detach "$mount_path" >/dev/null || fail 'could not detach the disk image created by this run'
mounted=0

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
    "signature": "hardened_runtime_ad_hoc",
    "signature_scope": "Hardened Runtime ad-hoc code signature; no Apple developer identity and no notarization.",
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

mv "$work_dir/unpacked" "$output_root/unpacked"
mv "$work_dir/manifest.json" "$output_root/manifest.json"
mv "$work_dir/$dmg_name" "$output_root/$dmg_name"
completed=1
printf 'Created %s\n' "$output_root/$dmg_name"
printf 'Manifest: %s\n' "$output_root/manifest.json"
printf 'Staged executable: %s\n' "$output_root/unpacked/macos-universal/diskunfold"
