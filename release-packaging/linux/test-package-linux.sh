#!/usr/bin/env bash
set -euo pipefail

fail() {
    printf 'test-package-linux: %s\n' "$*" >&2
    exit 1
}

usage() {
    cat <<'USAGE'
Usage: test-package-linux.sh --binary PATH

Runs a packaging and release-inventory integration check with the supplied
native GUI-enabled launcher. It uses clearly labelled, incomplete notice and
Debian copyright fixtures that are not suitable for a release. The test checks
both package formats, the real du-release-tools CLI output, checksums, collisions,
symlink rejection, CPU matching, and output preservation.
USAGE
}

binary=''
while (($#)); do
    case "$1" in
        --binary)
            (($# >= 2)) || fail 'missing value for --binary'
            binary=$2
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
[[ -n "$binary" ]] || {
    usage >&2
    fail '--binary is required'
}
[[ "$binary" == /* ]] || binary="$PWD/$binary"
[[ -f "$binary" && ! -L "$binary" && -x "$binary" ]] || \
    fail 'binary must be an executable regular file, not a symbolic link'

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
package_script="$script_dir/package-linux.sh"
repo_root=$(cd "$script_dir/../.." && pwd -P)
command -v cargo >/dev/null 2>&1 || fail 'cargo is required to verify the staging manifest with du-release-tools'
test_root=$(mktemp -d "${TMPDIR:-/tmp}/diskunfold-linux-package-test.XXXXXXXX")
test_identity=$(stat -c '%d:%i' -- "$test_root")

cleanup() {
    local status=$?
    trap - EXIT
    local current_identity=''
    if [[ -d "$test_root" && ! -L "$test_root" ]]; then
        current_identity=$(stat -c '%d:%i' -- "$test_root") || current_identity=''
        if [[ "$current_identity" == "$test_identity" ]]; then
            rm -rf -- "$test_root"
        else
            printf 'test-package-linux: preserving test directory whose identity changed: %s\n' "$test_root" >&2
            ((status != 0)) || status=1
        fi
    fi
    exit "$status"
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

cat > "$test_root/THIRD-PARTY-NOTICES.fixture.txt" <<'NOTICE'
TEST FIXTURE ONLY. This notice text is incomplete and must never be published.
NOTICE
notices="$test_root/THIRD-PARTY-NOTICES.fixture.txt"
cat > "$test_root/copyright.fixture.txt" <<'COPYRIGHT'
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: DiskUnfold
Upstream-Contact: DiskUnfold Maintainers <arikasalice@gmail.com>
Source: https://github.com/arikasalice/DiskUnfold
Files: *
Copyright: TEST FIXTURE ONLY
License: MIT
COPYRIGHT
debian_copyright="$test_root/copyright.fixture.txt"
output="$test_root/package"
cpu=$(uname -m)
case "$cpu" in
    x86_64) cpu=x64 ;;
    aarch64) cpu=arm64 ;;
    *) fail "unsupported test runner CPU: $cpu" ;;
esac
if [[ "$cpu" == x64 ]]; then
    other_cpu=arm64
else
    other_cpu=x64
fi
maintainer='DiskUnfold Maintainers <arikasalice@gmail.com>'

"$package_script" --binary "$binary" --version 0.1.0 --cpu "$cpu" \
    --notices "$notices" \
    --debian-copyright "$debian_copyright" --output-dir "$output"

deb_name="DiskUnfold_0.1.0_${cpu}.deb"
tar_name="DiskUnfold-0.1.0-linux-${cpu}.tar.gz"
[[ -f "$output/$deb_name" && -f "$output/$tar_name" ]]
[[ -f "$output/SHA256SUMS" && -f "$output/EXECUTABLE-SHA256.json" && -f "$output/manifest.json" ]]
[[ -f "$output/unpacked/linux-$cpu/diskunfold" && ! -L "$output/unpacked/linux-$cpu/diskunfold" ]]
cmp -s "$output/unpacked/linux-$cpu/diskunfold" "$binary" || \
    fail 'staged executable differs from the supplied launcher'
(
    cd "$output"
    sha256sum -c SHA256SUMS
)

release_tools_output="$test_root/release-tools"
(
    cd "$repo_root"
    cargo run --locked -p du-release-tools --bin build-release-inventory -- \
        "$output/manifest.json" "$output" "$release_tools_output"
)
cmp -s "$output/SHA256SUMS" "$release_tools_output/SHA256SUMS" || \
    fail 'package checksums differ from du-release-tools output'
cmp -s "$output/EXECUTABLE-SHA256.json" "$release_tools_output/EXECUTABLE-SHA256.json" || \
    fail 'executable checksums differ from du-release-tools output'
format_order=$(awk -F'"' '/"format":/ { print $4 }' "$release_tools_output/release-inventory.json" | paste -sd ' ' -)
[[ "$format_order" == 'deb tar-gz' ]] || fail 'release inventory formats are not in canonical order'
supported_versions=$(awk '
    /"supported_versions": \[/ { in_versions = 1; next }
    in_versions && /\]/ { in_versions = 0; next }
    in_versions { gsub(/[",[:space:]]/, ""); print }
' "$release_tools_output/release-inventory.json")
[[ "$supported_versions" == $'24.04\n26.04\n24.04\n26.04' ]] || \
    fail 'release inventory rows do not each list exactly Ubuntu 24.04 and 26.04'
cpu_rows=$(grep -c "\"cpu\": \"$cpu\"" "$release_tools_output/release-inventory.json")
[[ "$cpu_rows" -eq 2 ]] || fail 'release inventory does not preserve the native package CPU'

dpkg_arch=$(dpkg --print-architecture)
[[ "$(dpkg-deb -f "$output/$deb_name" Package)" == diskunfold ]]
[[ "$(dpkg-deb -f "$output/$deb_name" Version)" == 0.1.0 ]]
[[ "$(dpkg-deb -f "$output/$deb_name" Architecture)" == "$dpkg_arch" ]]
[[ "$(dpkg-deb -f "$output/$deb_name" Maintainer)" == "$maintainer" ]]
[[ "$(dpkg-deb -f "$output/$deb_name" Depends)" == *libwebkitgtk-6.0-4* ]]

tar --list --file "$output/$tar_name" --gzip | LC_ALL=C sort > "$test_root/tar-members"
printf '%s\n' \
    "DiskUnfold-0.1.0-linux-${cpu}/" \
    "DiskUnfold-0.1.0-linux-${cpu}/LICENSE" \
    "DiskUnfold-0.1.0-linux-${cpu}/install-form" \
    "DiskUnfold-0.1.0-linux-${cpu}/THIRD-PARTY-NOTICES.txt" \
    "DiskUnfold-0.1.0-linux-${cpu}/diskunfold" | LC_ALL=C sort > "$test_root/tar-members.expected"
cmp -s "$test_root/tar-members.expected" "$test_root/tar-members" || \
    fail 'tar.gz contains unexpected entries'
tar -xOzf "$output/$tar_name" "DiskUnfold-0.1.0-linux-${cpu}/THIRD-PARTY-NOTICES.txt" | \
    cmp - "$notices" || fail 'tar.gz notices differ from the explicit input'
# The app reports TarGz only when this marker names the form.
[[ "$(tar -xOzf "$output/$tar_name" "DiskUnfold-0.1.0-linux-${cpu}/install-form" | tr -d '[:space:]')" == tar_gz ]] || \
    fail 'tar.gz install-form marker is missing or wrong'
# The Debian package is recognized from dpkg records, so it ships no marker.
if dpkg-deb --fsys-tarfile "$output/$deb_name" | tar -tf - | grep -q 'install-form'; then
    fail 'deb must not contain an install-form marker'
fi
dpkg-deb --fsys-tarfile "$output/$deb_name" | \
    tar -xO -f - ./usr/share/doc/diskunfold/THIRD-PARTY-NOTICES.txt | \
    cmp - "$notices" || fail 'deb notices differ from the explicit input'
dpkg-deb --fsys-tarfile "$output/$deb_name" | \
    tar -xO -f - ./usr/share/doc/diskunfold/copyright | \
    cmp - "$debian_copyright" || fail 'Debian copyright differs from the explicit input'

package_digest_before=$(sha256sum "$output/$deb_name" | awk '{ print $1 }')
if "$package_script" --binary "$binary" --version 0.1.0 --cpu "$cpu" \
    --maintainer "$maintainer" --notices "$notices" \
    --debian-copyright "$debian_copyright" --output-dir "$output" >/dev/null 2>&1; then
    fail 'existing output directory was accepted'
fi
package_digest_after=$(sha256sum "$output/$deb_name" | awk '{ print $1 }')
[[ "$package_digest_before" == "$package_digest_after" ]] || \
    fail 'collision attempt changed the existing package'

ln -s "$notices" "$test_root/notices-link"
if "$package_script" --binary "$binary" --version 0.1.0 --cpu "$cpu" \
    --maintainer "$maintainer" --notices "$test_root/notices-link" \
    --debian-copyright "$debian_copyright" \
    --output-dir "$test_root/symlink-output" >/dev/null 2>&1; then
    fail 'symbolic-link notices input was accepted'
fi
[[ ! -e "$test_root/symlink-output" && ! -L "$test_root/symlink-output" ]] || \
    fail 'invalid notices input created a package output directory'

: > "$test_root/empty-notices.txt"
if "$package_script" --binary "$binary" --version 0.1.0 --cpu "$cpu" \
    --maintainer "$maintainer" --notices "$test_root/empty-notices.txt" \
    --debian-copyright "$debian_copyright" \
    --output-dir "$test_root/empty-notices-output" >/dev/null 2>&1; then
    fail 'empty notices input was accepted'
fi
if "$package_script" --binary "$binary" --version 0.1.0 --cpu "$cpu" \
    --maintainer "$maintainer" \
    --debian-copyright "$debian_copyright" \
    --output-dir "$test_root/missing-notices-output" >/dev/null 2>&1; then
    fail 'a missing --notices option was accepted'
fi
[[ ! -e "$test_root/empty-notices-output" && ! -e "$test_root/missing-notices-output" ]] || \
    fail 'rejected notices input created a package output directory'

ln -s "$debian_copyright" "$test_root/copyright-link"
if "$package_script" --binary "$binary" --version 0.1.0 --cpu "$cpu" \
    --maintainer "$maintainer" --notices "$notices" \
    --debian-copyright "$test_root/copyright-link" \
    --output-dir "$test_root/symlink-copyright-output" >/dev/null 2>&1; then
    fail 'symbolic-link Debian copyright input was accepted'
fi
[[ ! -e "$test_root/symlink-copyright-output" && ! -L "$test_root/symlink-copyright-output" ]] || \
    fail 'invalid Debian copyright input created a package output directory'

printf 'Format: invalid\n' > "$test_root/invalid-copyright.fixture.txt"
if "$package_script" --binary "$binary" --version 0.1.0 --cpu "$cpu" \
    --maintainer "$maintainer" --notices "$notices" \
    --debian-copyright "$test_root/invalid-copyright.fixture.txt" \
    --output-dir "$test_root/invalid-copyright-output" >/dev/null 2>&1; then
    fail 'Debian copyright input without required metadata was accepted'
fi
[[ ! -e "$test_root/invalid-copyright-output" && ! -L "$test_root/invalid-copyright-output" ]] || \
    fail 'invalid Debian copyright input created a package output directory'

if "$package_script" --binary "$binary" --version 0.1.0 --cpu "$other_cpu" \
    --maintainer "$maintainer" --notices "$notices" \
    --debian-copyright "$debian_copyright" \
    --output-dir "$test_root/wrong-cpu-output" >/dev/null 2>&1; then
    fail 'CPU mismatch was accepted'
fi
[[ ! -e "$test_root/wrong-cpu-output" && ! -L "$test_root/wrong-cpu-output" ]] || \
    fail 'CPU mismatch created a package output directory'

mkdir "$test_root/pre-existing-output"
printf 'keep\n' > "$test_root/pre-existing-output/sentinel"
if "$package_script" --binary "$binary" --version 0.1.0 --cpu "$cpu" \
    --maintainer "$maintainer" --notices "$notices" \
    --debian-copyright "$debian_copyright" \
    --output-dir "$test_root/pre-existing-output" >/dev/null 2>&1; then
    fail 'pre-existing output directory was accepted'
fi
[[ "$(cat "$test_root/pre-existing-output/sentinel")" == keep ]] || \
    fail 'pre-existing output contents were changed'

race_output="$test_root/race-output"
mv_wrapper_dir="$test_root/mv-wrapper"
mkdir "$mv_wrapper_dir"
cat > "$mv_wrapper_dir/mv" <<'MOVE_WRAPPER'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == -nT ]]; then
    shift
    [[ "${1:-}" == -- ]] && shift
    stage=$1
    output=$2
    if [[ "$output" == "${DISKUNFOLD_RACE_OUTPUT:?}" ]]; then
        mkdir "$output"
        printf 'competitor-kept\n' > "$output/sentinel"
    fi
    exec /usr/bin/mv -nT -- "$stage" "$output"
fi
exec /usr/bin/mv "$@"
MOVE_WRAPPER
chmod 0755 "$mv_wrapper_dir/mv"
if PATH="$mv_wrapper_dir:$PATH" DISKUNFOLD_RACE_OUTPUT="$race_output" \
    "$package_script" --binary "$binary" --version 0.1.0 --cpu "$cpu" \
    --maintainer "$maintainer" --notices "$notices" \
    --debian-copyright "$debian_copyright" \
    --output-dir "$race_output" >/dev/null 2>&1; then
    fail 'output created after preflight was replaced or accepted'
fi
[[ "$(cat "$race_output/sentinel")" == competitor-kept ]] || \
    fail 'output created after preflight was changed'
if find "$test_root" -maxdepth 1 \( -name '.race-output.staging.*' -o -name '.race-output.work.*' \) -print -quit | grep -q .; then
    fail 'private staging directories were left after output collision'
fi

printf 'Linux deb/tar.gz package integration checks passed (test notices only).\n'
