#!/usr/bin/env bash
set -euo pipefail

fail() {
    printf 'package-linux: %s\n' "$*" >&2
    exit 1
}

usage() {
    cat <<'USAGE'
Usage:
  package-linux.sh --binary PATH --version X.Y.Z --cpu x64|arm64 \
    [--maintainer "Name <email>"] --notices PATH --debian-copyright PATH \
    --output-dir NEW_DIR

--maintainer defaults to the project value DiskUnfold Maintainers
<arikasalice@gmail.com>.

Creates one Debian package and one tar.gz for the native Ubuntu runner. The
output directory must not exist. The tar.gz contains exactly the executable,
LICENSE, the explicitly supplied THIRD-PARTY-NOTICES.txt, the install-form
marker and a README.txt that lists the runtime packages to install with apt
(the same dependencies the .deb declares). The .deb also
requires an explicit Debian copyright file. The staging directory includes a
du-release-tools manifest and executable copy, plus package and executable
checksums.

This script does not build the executable, certify notice completeness, sign,
create provenance, generate the public release inventory, publish, or install.
USAGE
}

binary=''
version=''
cpu=''
maintainer='DiskUnfold Maintainers <arikasalice@gmail.com>'
notices=''
debian_copyright=''
output_dir=''
while (($#)); do
    case "$1" in
        --binary|--version|--cpu|--maintainer|--notices|--debian-copyright|--output-dir)
            (($# >= 2)) || fail "missing value for $1"
            case "$1" in
                --binary) binary=$2 ;;
                --version) version=$2 ;;
                --cpu) cpu=$2 ;;
                --maintainer) maintainer=$2 ;;
                --notices) notices=$2 ;;
                --debian-copyright) debian_copyright=$2 ;;
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

[[ -n "$binary" && -n "$version" && -n "$cpu" && -n "$maintainer" && -n "$notices" && -n "$debian_copyright" && -n "$output_dir" ]] || {
    usage >&2
    fail 'all options except --maintainer are required'
}
[[ "$version" =~ ^[0-9]+(\.[0-9]+){1,2}$ ]] || fail 'version must be numeric, such as 0.0.1'
[[ "$maintainer" != *$'\n'* && "$maintainer" != *$'\r'* ]] || fail 'maintainer must fit on one control-file line'
[[ "$maintainer" == *"<"*"@"*">" ]] || fail 'maintainer must use the form Name <email>'
case "$cpu" in
    x64)
        deb_arch=amd64
        expected_machine='Advanced Micro Devices X86-64'
        expected_uname=x86_64
        ;;
    arm64)
        deb_arch=arm64
        expected_machine=AArch64
        expected_uname=aarch64
        ;;
    *)
        fail 'CPU must be x64 or arm64'
        ;;
esac

[[ "$binary" == /* ]] || binary="$PWD/$binary"
[[ "$notices" == /* ]] || notices="$PWD/$notices"
[[ "$debian_copyright" == /* ]] || debian_copyright="$PWD/$debian_copyright"
[[ "$output_dir" == /* ]] || output_dir="$PWD/$output_dir"
[[ -f "$binary" && ! -L "$binary" && -x "$binary" ]] || \
    fail 'binary must be an executable regular file, not a symbolic link'
[[ -f "$notices" && ! -L "$notices" && -s "$notices" ]] || \
    fail 'notices must be a nonempty regular file, not a symbolic link'
[[ -f "$debian_copyright" && ! -L "$debian_copyright" && -s "$debian_copyright" ]] || \
    fail 'Debian copyright input must be a nonempty regular file, not a symbolic link'

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
repo_root=$(cd "$script_dir/../.." && pwd -P)
license_path="$repo_root/LICENSE"
[[ -f "$license_path" && ! -L "$license_path" && -s "$license_path" ]] || \
    fail "repository LICENSE is missing or invalid: $license_path"

[[ -r /etc/os-release ]] || fail '/etc/os-release is unavailable'
. /etc/os-release
[[ "${ID:-}" == ubuntu ]] || fail 'Debian package generation must run on Ubuntu'
case "${VERSION_ID:-}" in
    24.04|26.04) ;;
    *) fail "package generation requires Ubuntu 24.04 or 26.04; found ${VERSION_ID:-unknown}" ;;
esac
[[ "$(uname -m)" == "$expected_uname" ]] || \
    fail "requested CPU $cpu does not match native runner CPU $(uname -m)"
[[ "$(dpkg --print-architecture)" == "$deb_arch" ]] || \
    fail "requested CPU $cpu does not match dpkg architecture $(dpkg --print-architecture)"

for tool in dpkg dpkg-deb dpkg-shlibdeps readelf sha256sum tar gzip install stat mktemp mv rm du find cmp grep; do
    command -v "$tool" >/dev/null 2>&1 || fail "required tool is unavailable: $tool"
done

grep -Eq '^Format: https://www\.debian\.org/doc/packaging-manuals/copyright-format/1\.0/?$' "$debian_copyright" || \
    fail 'Debian copyright input must use the Debian machine-readable copyright format'
grep -Eq '^Source: [^[:space:]].*$' "$debian_copyright" || \
    fail 'Debian copyright input must identify the upstream source location'
grep -Eq '^Upstream-Contact: [^[:space:]].*$' "$debian_copyright" || \
    fail 'Debian copyright input must identify an upstream contact'

machine=$(LC_ALL=C readelf -h "$binary" 2>/dev/null | awk -F: '$1 ~ /Machine/ { sub(/^[[:space:]]+/, "", $2); print $2 }')
[[ "$machine" == "$expected_machine" ]] || \
    fail "binary ELF machine must be $expected_machine; found ${machine:-unknown}"
needed=$(LC_ALL=C readelf -d "$binary" 2>/dev/null) || fail 'binary is not a readable ELF executable'
[[ "$needed" == *libwebkitgtk-6.0.so* ]] || \
    fail 'binary does not dynamically link the system WebKitGTK 6.0 runtime'
[[ "$needed" != *libdbus-1.so* ]] || \
    fail 'binary links libdbus; the Linux package must not include or depend on libdbus'

output_parent_input=$(dirname "$output_dir")
output_name=$(basename "$output_dir")
[[ "$output_name" != '.' && "$output_name" != '/' && -n "$output_name" ]] || \
    fail 'output directory must have a final path component'
[[ -d "$output_parent_input" ]] || fail 'parent of output directory must already exist'
output_parent=$(cd "$output_parent_input" && pwd -P)
output_root="$output_parent/$output_name"
[[ ! -e "$output_root" && ! -L "$output_root" ]] || \
    fail 'output directory already exists; choose a unique new path'

source_date_epoch=${SOURCE_DATE_EPOCH:-0}
[[ "$source_date_epoch" =~ ^[0-9]+$ ]] || fail 'SOURCE_DATE_EPOCH must be a nonnegative integer'

stage=$(mktemp -d "$output_parent/.${output_name}.staging.XXXXXXXX")
stage_identity=$(stat -c '%d:%i' -- "$stage")
work=''
work_identity=''

remove_owned_dir() {
    local path=$1
    local expected_identity=$2
    local current_identity=''
    [[ -n "$path" && -n "$expected_identity" && -d "$path" && ! -L "$path" ]] || return 0
    current_identity=$(stat -c '%d:%i' -- "$path") || return 1
    if [[ "$current_identity" != "$expected_identity" ]]; then
        printf 'package-linux: preserving staging path whose identity changed: %s\n' "$path" >&2
        return 1
    fi
    rm -rf -- "$path"
}

cleanup() {
    local status=$?
    trap - EXIT
    if ! remove_owned_dir "$work" "$work_identity"; then
        ((status != 0)) || status=1
    fi
    if ! remove_owned_dir "$stage" "$stage_identity"; then
        ((status != 0)) || status=1
    fi
    exit "$status"
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

work=$(mktemp -d "$output_parent/.${output_name}.work.XXXXXXXX")
work_identity=$(stat -c '%d:%i' -- "$work")

deb_name="DiskUnfold_${version}_${cpu}.deb"
tar_name="DiskUnfold-${version}-linux-${cpu}.tar.gz"
tar_root="DiskUnfold-${version}-linux-${cpu}"
deb_root="$work/deb-root"
tar_root_path="$work/tar-root/$tar_root"

install -d -m 0755 "$deb_root/DEBIAN" "$deb_root/usr/bin" \
    "$deb_root/usr/share/doc/diskunfold" "$tar_root_path"
install -m 0755 "$binary" "$deb_root/usr/bin/diskunfold"
install -m 0644 "$license_path" "$deb_root/usr/share/doc/diskunfold/LICENSE"
install -m 0644 "$debian_copyright" "$deb_root/usr/share/doc/diskunfold/copyright"
install -m 0644 "$notices" "$deb_root/usr/share/doc/diskunfold/THIRD-PARTY-NOTICES.txt"
install -m 0755 "$binary" "$tar_root_path/diskunfold"
install -m 0644 "$license_path" "$tar_root_path/LICENSE"
install -m 0644 "$notices" "$tar_root_path/THIRD-PARTY-NOTICES.txt"
printf 'tar_gz\n' > "$tar_root_path/install-form"
chmod 0644 "$tar_root_path/install-form"

mkdir -p "$work/debian"
cat > "$work/debian/control" <<CONTROL
Source: diskunfold
Section: utils
Priority: optional
Maintainer: $maintainer
Standards-Version: 4.6.2

Package: diskunfold
Architecture: any
Depends: \${shlibs:Depends}
Description: Local disk usage analysis for desktop users
CONTROL
shlibs_output=$(cd "$work" && LC_ALL=C dpkg-shlibdeps -O -e "$deb_root/usr/bin/diskunfold" 2>"$work/dpkg-shlibdeps.log") || {
    cat "$work/dpkg-shlibdeps.log" >&2
    fail 'could not resolve the executable dynamic-library dependencies'
}
[[ "$shlibs_output" == shlibs:Depends=* ]] || fail 'dpkg-shlibdeps returned no Depends field'
depends=${shlibs_output#shlibs:Depends=}
[[ -n "$depends" ]] || fail 'the Debian package requires at least one runtime dependency'
[[ "$depends" == *libwebkitgtk-6.0-4* ]] || \
    fail 'the computed Debian dependencies omit the system WebKitGTK 6.0 package'
[[ "$depends" != *libdbus* ]] || \
    fail 'the computed Debian dependencies include libdbus, which is not allowed'

installed_size=$(du -sk -- "$deb_root/usr" | awk '{ print $1 }')

# The tar.gz has no package manager to pull in libraries, so its README names them. The list is the
# dependency set computed for the .deb: each entry without its version constraint, first alternative.
runtime_packages=$(printf '%s\n' "$depends" | tr ',' '\n' | sed -e 's/^ *//' -e 's/ *|.*$//' -e 's/ *(.*$//' | \
    LC_ALL=C sort -u | tr '\n' ' ' | sed 's/ *$//')
[[ -n "$runtime_packages" ]] || fail 'could not derive the runtime package list'
cat > "$tar_root_path/README.txt" <<README
DiskUnfold $version (tar.gz, Linux)

Run ./diskunfold from this folder. Unlike the .deb, this archive does not install
the libraries DiskUnfold needs. Install them first on Ubuntu or Debian:

    sudo apt update
    sudo apt install $runtime_packages

The graphical interface needs GTK 4 (libgtk-4-1) and WebKitGTK 6.0
(libwebkitgtk-6.0-4). The .deb package installs all of these automatically
(sudo apt install ./DiskUnfold_${version}_<cpu>.deb), so prefer it when you can.

LICENSE and THIRD-PARTY-NOTICES.txt in this folder cover DiskUnfold and the
components it includes.
README
chmod 0644 "$tar_root_path/README.txt"
cat > "$deb_root/DEBIAN/control" <<CONTROL
Package: diskunfold
Version: $version
Section: utils
Priority: optional
Architecture: $deb_arch
Maintainer: $maintainer
Installed-Size: $installed_size
Depends: $depends
Description: Local disk usage analysis for desktop users
CONTROL

find "$deb_root" -exec touch -h -d "@$source_date_epoch" -- {} +
export SOURCE_DATE_EPOCH="$source_date_epoch"
dpkg-deb --build --root-owner-group "$deb_root" "$stage/$deb_name" >/dev/null

tar --sort=name --mtime="@$source_date_epoch" --owner=0 --group=0 --numeric-owner \
    -C "$work/tar-root" -cf - "$tar_root" | gzip -n -9 > "$stage/$tar_name"

binary_sha256=$(sha256sum -- "$binary" | awk '{ print $1 }')
[[ "$binary_sha256" =~ ^[0-9a-f]{64}$ ]] || fail 'could not compute the executable SHA-256'

compare_deb_file() {
    local member=$1
    local expected_file=$2
    dpkg-deb --fsys-tarfile "$stage/$deb_name" | tar -xO -f - "$member" | cmp - "$expected_file"
}

compare_deb_file ./usr/bin/diskunfold "$binary"
compare_deb_file ./usr/share/doc/diskunfold/LICENSE "$license_path"
compare_deb_file ./usr/share/doc/diskunfold/copyright "$debian_copyright"
compare_deb_file ./usr/share/doc/diskunfold/THIRD-PARTY-NOTICES.txt "$notices"

deb_members=$(dpkg-deb --fsys-tarfile "$stage/$deb_name" | tar -tf - | awk '!/\/$/' | LC_ALL=C sort)
expected_deb_members=$(printf '%s\n' \
    ./usr/bin/diskunfold \
    ./usr/share/doc/diskunfold/LICENSE \
    ./usr/share/doc/diskunfold/THIRD-PARTY-NOTICES.txt \
    ./usr/share/doc/diskunfold/copyright | LC_ALL=C sort)
[[ "$deb_members" == "$expected_deb_members" ]] || \
    fail 'Debian payload contains unexpected files or is missing a required file'

tar -tzf "$stage/$tar_name" | LC_ALL=C sort > "$work/tar-members"
printf '%s\n' \
    "$tar_root/" \
    "$tar_root/LICENSE" \
    "$tar_root/README.txt" \
    "$tar_root/install-form" \
    "$tar_root/THIRD-PARTY-NOTICES.txt" \
    "$tar_root/diskunfold" | LC_ALL=C sort > "$work/tar-members.expected"
cmp -s "$work/tar-members.expected" "$work/tar-members" || \
    fail 'tar.gz must contain only one root folder, the executable, LICENSE, and THIRD-PARTY-NOTICES.txt'
tar -xOzf "$stage/$tar_name" "$tar_root/diskunfold" | cmp - "$binary"
tar -xOzf "$stage/$tar_name" "$tar_root/LICENSE" | cmp - "$license_path"
tar -xOzf "$stage/$tar_name" "$tar_root/THIRD-PARTY-NOTICES.txt" | cmp - "$notices"

deb_executable_sha256=$(dpkg-deb --fsys-tarfile "$stage/$deb_name" | \
    tar -xO -f - ./usr/bin/diskunfold | sha256sum | awk '{ print $1 }')
tar_executable_sha256=$(tar -xOzf "$stage/$tar_name" "$tar_root/diskunfold" | \
    sha256sum | awk '{ print $1 }')
[[ "$deb_executable_sha256" == "$binary_sha256" && "$tar_executable_sha256" == "$binary_sha256" ]] || \
    fail 'packaged executable SHA-256 does not match the built executable'
deb_sha256=$(sha256sum -- "$stage/$deb_name" | awk '{ print $1 }')
tar_sha256=$(sha256sum -- "$stage/$tar_name" | awk '{ print $1 }')
[[ "$deb_sha256" =~ ^[0-9a-f]{64}$ && "$tar_sha256" =~ ^[0-9a-f]{64}$ ]] || \
    fail 'could not compute package SHA-256 values'

unpacked_executable="unpacked/linux-$cpu/diskunfold"
install -d -m 0755 "$stage/unpacked/linux-$cpu"
install -m 0755 "$binary" "$stage/$unpacked_executable"
cmp -s "$stage/$unpacked_executable" "$binary" || \
    fail 'staged executable does not match the built executable'

(
    cd "$stage"
    printf '%s\n' "$deb_name" "$tar_name" | LC_ALL=C sort | \
        while IFS= read -r artifact; do sha256sum -- "$artifact"; done > SHA256SUMS
)
executable_checksum_json=$(cat <<JSON
{
  "schema_version": 1,
  "product_name": "DiskUnfold",
  "version": "$version",
  "executables": [
    {
      "artifact_file_name": "$deb_name",
      "executable_file_name": "diskunfold",
      "sha256": "$deb_executable_sha256"
    },
    {
      "artifact_file_name": "$tar_name",
      "executable_file_name": "diskunfold",
      "sha256": "$tar_executable_sha256"
    }
  ]
}
JSON
)
printf '%s' "$executable_checksum_json" > "$stage/EXECUTABLE-SHA256.json"

# This local staging manifest uses the exact du-release-tools input contract.
# Its exact OS set is release policy metadata; it is not a test attestation.
cat > "$stage/manifest.json" <<JSON
{
  "product_name": "DiskUnfold",
  "version": "$version",
  "rows": [
    {
      "file_name": "$deb_name",
      "version": "$version",
      "os": {"family": "ubuntu", "supported_versions": ["24.04", "26.04"]},
      "cpu": "$cpu",
      "format": "deb",
      "channel": "direct",
      "signature": "unsigned",
      "signature_scope": "The Debian package is unsigned; its checksum does not authenticate the publisher.",
      "sha256": "$deb_sha256",
      "executable_file_name": "diskunfold",
      "executable_sha256": "$deb_executable_sha256",
      "run_instructions": "Install on Ubuntu with sudo apt install ./$deb_name, then launch diskunfold."
    },
    {
      "file_name": "$tar_name",
      "version": "$version",
      "os": {"family": "ubuntu", "supported_versions": ["24.04", "26.04"]},
      "cpu": "$cpu",
      "format": "tar-gz",
      "channel": "direct",
      "signature": "unsigned",
      "signature_scope": "The tar.gz archive is unsigned; its checksum does not authenticate the publisher.",
      "sha256": "$tar_sha256",
      "executable_file_name": "diskunfold",
      "executable_sha256": "$tar_executable_sha256",
      "run_instructions": "Extract the archive, then launch ./$tar_root/diskunfold."
    }
  ],
  "channel_references": [],
  "staged_files": [
    {"artifact_file_name": "$deb_name", "kind": "package", "relative_path": "$deb_name"},
    {"artifact_file_name": "$deb_name", "kind": "executable", "relative_path": "$unpacked_executable"},
    {"artifact_file_name": "$tar_name", "kind": "package", "relative_path": "$tar_name"},
    {"artifact_file_name": "$tar_name", "kind": "executable", "relative_path": "$unpacked_executable"}
  ]
}
JSON
chmod 0644 "$stage/SHA256SUMS" "$stage/EXECUTABLE-SHA256.json" "$stage/manifest.json"
(
    cd "$stage"
    sha256sum -c SHA256SUMS >/dev/null
)

remove_owned_dir "$work" "$work_identity" || fail 'could not safely remove private work staging'
work=''
work_identity=''
mv -nT -- "$stage" "$output_root" || fail 'could not publish the new output directory without replacing an existing path'
[[ ! -e "$stage" && ! -L "$stage" ]] || \
    fail 'output path appeared during packaging; preserved it and discarded this package attempt'
stage=''
stage_identity=''

printf 'Linux packages created and inspected.\n'
printf 'deb=%s/%s\n' "$output_root" "$deb_name"
printf 'tar_gz=%s/%s\n' "$output_root" "$tar_name"
printf 'sha256sums=%s/SHA256SUMS\n' "$output_root"
printf 'executable_sha256=%s/EXECUTABLE-SHA256.json\n' "$output_root"
printf 'manifest=%s/manifest.json\n' "$output_root"
printf 'signature=unsigned; provenance=not-created; publication=not-performed\n'
