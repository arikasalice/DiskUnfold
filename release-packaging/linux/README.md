# Linux package build

The scripts in this directory build a native GUI-enabled launcher and package it
as the F15 Ubuntu download formats: a Debian package and a tar.gz. Run them on
Ubuntu 24.04 or 26.04, once on a native x64 runner and once on a native ARM64
runner. The scripts reject other Linux distributions, Ubuntu releases, CPU
architectures, and cross-architecture packaging.

## Build

Install the Rust toolchain selected by rust-toolchain.toml, Node selected by
.node-version, and the native packages from the Linux row in
release-packaging/quality/README.md. Then run from any working directory:

    bash release-packaging/linux/build-linux.sh

The build script checks the runner and pinned Node version, runs npm ci with
lifecycle scripts disabled, builds gui-web, and builds du-launcher with
--locked for the native GNU/Linux target. It requires the GUI launcher to
dynamically link WebKitGTK 6.0 and rejects libdbus. It prints the native CPU and
the resulting binary path for the packaging step.

## Package

Create a verified THIRD-PARTY-NOTICES file in the release workflow and pass
that file explicitly. This repository revision has no generated production
notices artifact. The packager copies the given bytes and checks them in both
package formats; it cannot determine whether the notices are complete or
legally sufficient. Do not use the test fixture as a release input.

Also pass a reviewed Debian machine-readable copyright file with the upstream
source location, upstream contact, copyright statements, and complete license
entries for the executable and its bundled components. The release uses the
generated `release-packaging/linux/debian-copyright` (see
[Debian copyright file](#debian-copyright-file)). The packager requires
the format, source, and contact fields and copies the supplied bytes to
`/usr/share/doc/diskunfold/copyright`; it does not infer or validate the complete
copyright inventory. The test fixture is incomplete and must never be published.

The Debian maintainer defaults to the project value
DiskUnfold Maintainers <arikasalice@gmail.com>. Pass --maintainer with the form
Name <email> only to override it. Use the same contact in the Debian copyright
file's Upstream-Contact field.

    bash release-packaging/linux/package-linux.sh \
      --binary target/x86_64-unknown-linux-gnu/release/diskunfold \
      --version 0.0.1 \
      --cpu x64 \
      --notices /path/to/verified/THIRD-PARTY-NOTICES.txt \
      --debian-copyright /path/to/verified/debian-copyright.txt \
      --output-dir /path/to/new/linux-release

Use arm64 and target/aarch64-unknown-linux-gnu/release/diskunfold on the native
ARM64 runner. The parent of output-dir must already exist, and output-dir must
not exist. The script stages into unique private directories, verifies package
members and SHA-256 values, and atomically renames the completed directory
with GNU mv's no-clobber mode. It checks that the stage moved, so a path that
appears after preflight is preserved and treated as a failed package attempt.
On failure it removes only the unique
staging directories created by that invocation, after checking their directory
identity.

The .deb registers /usr/bin/diskunfold and derives Depends from the executable's
dynamic library links. WebKitGTK remains a system package dependency, which the
.deb installs automatically. The tar.gz contains one top-level folder with
exactly diskunfold, LICENSE, THIRD-PARTY-NOTICES.txt, the install-form marker and
a README.txt that lists the runtime packages (the .deb's dependencies, such as
libgtk-4-1 and libwebkitgtk-6.0-4) and the `sudo apt install` command. It
includes no WebKitGTK, GTK, GLib, or other system
libraries. Both formats are unsigned by this script. `SHA256SUMS` covers the
package files and `EXECUTABLE-SHA256.json` covers the package executables; these
digests do not authenticate a publisher or establish build provenance.

## Debian copyright file

`release-packaging/linux/debian-copyright` is the DEP-5 file that the release
packages as `/usr/share/doc/diskunfold/copyright`. It is generated, not written
by hand:

    cargo run --locked -p du-release-tools --bin build-product-resources -- \
      --debian-copyright release-packaging/linux/debian-copyright

Run `npm ci --ignore-scripts` in gui-web first so the npm license files are
read. The generator uses the same inventory as THIRD-PARTY-NOTICES: the normal
Cargo dependencies of du-launcher (`cargo metadata --locked`, every target) and
the production packages of gui-web/package-lock.json. It writes one `Files: *`
stanza for DiskUnfold under the MIT LICENSE, one stanza per license expression
that lists its packages, and one standalone `License:` paragraph for every
license and exception named. Licenses that Debian ships in
`/usr/share/common-licenses` (Apache-2.0, CC0-1.0, the GPL, LGPL, MPL and GFDL
families, Artistic) get a short paragraph that points to that file, as Debian
policy and lintian expect; every other license carries its canonical SPDX text. The
`committed_debian_copyright_matches_the_generator` test fails when the committed
file differs from the generator output, so regenerate it after any dependency
change and review the diff.

The third-party `Files:` patterns (`third-party/<cargo|npm>/<name>-<version>/*`)
are virtual because those packages are compiled into the executable or embedded
in its web bundle. Copyright lines are taken from the license files bundled with
each package, then from its manifest authors. A line that begins "No copyright
statement or author recorded" means the generator found neither.

### Review and release gate

The file is a draft until a person reviews it. The release workflow (the
"Require a reviewed Debian copyright file" step, reason key
`release.debian_copyright_missing`) fails an official run unless the file is
nonempty, has no test-fixture or dry-run wording, and a review record matches
the file. No review record is committed in this revision, so the gate fails by
design.

The reviewer checks:

1. The header names the right upstream (`Source`, `Upstream-Contact`, which must
   equal the Debian maintainer) and the `Files: *` stanza matches LICENSE.
2. Every package that ships appears once. Compare the stanza members with
   THIRD-PARTY-NOTICES from `build-product-resources --release`. The list covers
   the dependency graph for every operating system, so it can name crates that a
   Linux build never links; decide whether that superset is acceptable.
3. Each package's license expression matches its upstream license. DEP-5 has no
   parentheses, so `(A OR B) AND C` is written `A or B, and C`.
4. The copyright holders are right. Check each "No copyright statement or
   author recorded" and "Authors of ..." entry against the upstream repository.
   At generation time these were pin-project-lite, serde_json, winnow,
   hashlink, rsqlite-vfs, r-efi, the objc2 family, the webview2-com family and
   npm client-only. A correction belongs in the generator, because a hand edit
   fails the match test.
5. The standalone license texts are complete and apply to the stanzas that cite
   them. For packages that offer alternatives (`or`), confirm with the project
   owner which alternative DiskUnfold relies on.
6. `lintian` on the built .deb reports no new copyright warnings.

To mark the file reviewed, commit `release-packaging/linux/debian-copyright.reviewed`
together with the final reviewed file, containing these lines:

    sha256: <output of: sha256sum release-packaging/linux/debian-copyright>
    reviewer: <name>
    reviewed-on: <YYYY-MM-DD>

The gate compares the digest, so any later regeneration or edit invalidates the
review until the record is updated.

## Release inventory staging

The output directory contains `manifest.json`, the local input to
`du-release-tools`, and `unpacked/linux-<cpu>/diskunfold`, the executable staged
for digest verification. The manifest has one row for each package format and
declares Ubuntu's exact supported set as
`{"family":"ubuntu","supported_versions":["24.04","26.04"]}`. The
manifest records declared support policy; it is not evidence that both OS
versions or either CPU was tested. The package builder separately inspects both
package payloads and checks that their executable bytes match the built binary.

Generate the canonical distribution table and checksum files from the staging
manifest with the actual release-tools CLI:

    cargo run --locked -p du-release-tools --bin build-release-inventory -- \
      /path/to/linux-release/manifest.json \
      /path/to/linux-release \
      /path/to/new-release-inventory

The CLI rehashes each staged package and executable, then writes
`release-inventory.json`, `DISTRIBUTION.md`, `SHA256SUMS`, and
`EXECUTABLE-SHA256.json`. Package membership, notice completeness, Debian
copyright completeness, signatures, provenance, and publication remain separate
release checks. Do not use the explicitly labelled test notice or copyright
fixtures for a release.

This packaging step does not create signatures, SLSA provenance, SBOMs, a
release-wide inventory, or a publication. The F15 release workflow still owns
those checks and must verify the actual production notices before publication.

## Test

After building the native launcher, run:

    bash release-packaging/linux/test-package-linux.sh \
      --binary target/x86_64-unknown-linux-gnu/release/diskunfold

The test uses an explicitly labelled, incomplete notice fixture and is never
suitable for publication. It validates the deb and tar.gz contents, metadata,
checksums, runs the actual `du-release-tools` CLI against the generated staging
manifest, and checks exact Ubuntu version and format ordering. It also checks
collision behavior (including a path created immediately before the final
rename), symlink rejection, CPU matching, and preservation of existing output
data. The GUI launch smoke check remains
release-packaging/launcher/smoke-linux.sh; run it on the native Ubuntu runner
with its Xvfb, Openbox, xdotool, and D-Bus prerequisites.

## Validation performed for this change

The native x64 GUI launcher and package integration passed in a disposable
Ubuntu 24.04 amd64 container, including manifest generation and validation by
the `du-release-tools` CLI. The package test uses incomplete notice and Debian
copyright fixtures, so production completeness remains unverified. Earlier
container verification also passed a `.deb` install/CLI/uninstall cycle and the
Xvfb GUI smoke check with WebKit sandboxing disabled because the container's
namespace restriction prevented `bwrap` from starting; this does not verify a
real desktop session. Ubuntu 26.04, ARM64, a real desktop session, and official
support-table verification remain unverified.
