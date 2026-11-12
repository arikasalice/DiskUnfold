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

Pass a project-selected Debian maintainer value. For example, the syntax is
Name <email>; the packager does not invent project contact information.

    bash release-packaging/linux/package-linux.sh \
      --binary target/x86_64-unknown-linux-gnu/release/diskunfold \
      --version 0.1.0 \
      --cpu x64 \
      --maintainer "Project Maintainer <maintainer@example.org>" \
      --notices /path/to/verified/THIRD-PARTY-NOTICES.txt \
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
dynamic library links. WebKitGTK remains a system package dependency. The tar.gz
contains one top-level folder with exactly diskunfold, LICENSE, and
THIRD-PARTY-NOTICES.txt. It includes no WebKitGTK, GTK, GLib, or other system
libraries. Both formats are unsigned by this script. SHA256SUMS and
EXECUTABLE-SHA256.json contain digests of the packages and packaged executables;
they do not authenticate a publisher or establish build provenance.

## Release inventory limitation

This script does not emit manifest.json or a full distribution table. The
current du-release-tools input models OS support as one inclusive minimum and
maximum range. It cannot represent the exact supported Ubuntu set
{24.04, 26.04} without implying support for intermediate Ubuntu releases.
A release-tools change should add an `exact_versions` list to the OS descriptor
and make `minimum` and `maximum` optional while preserving their current meaning
for existing manifests. The discrete form should look like
`{"family":"ubuntu","exact_versions":["24.04","26.04"]}`. Validation
should accept exactly one of these forms:
(1) `minimum` with an optional `maximum` and no `exact_versions`, or (2) a
nonempty, duplicate-free `exact_versions` list with neither range bound.
Canonical output should sort exact versions by the existing numeric OS version
comparator and render each listed release explicitly; it must not expand an
exact list into a range. Tests should prove that `{24.04, 26.04}` does not
claim 25.04 and should reject empty, duplicate, or mixed forms. This requires a
coordinated `release-tools` schema, validation, renderer, and CLI test change;
this Linux packaging change does not alter that shared contract.
Until that contract exists, the release workflow must use the official support
table for exact verification states and must not infer those states from these
package artifacts.

This packaging step does not create signatures, SLSA provenance, SBOMs, a
release-wide inventory, or a publication. The F15 release workflow still owns
those checks and must verify the actual production notices before publication.

## Test

After building the native launcher, run:

    bash release-packaging/linux/test-package-linux.sh \
      --binary target/x86_64-unknown-linux-gnu/release/diskunfold

The test uses an explicitly labelled, incomplete notice fixture and is never
suitable for publication. It validates the deb and tar.gz contents, metadata,
checksums, collision behavior (including a path created immediately before the
final rename), symlink rejection, CPU matching, and preservation of existing
output data. The GUI launch smoke check remains
release-packaging/launcher/smoke-linux.sh; run it on the native Ubuntu runner
with its Xvfb, Openbox, xdotool, and D-Bus prerequisites.

## Validation performed for this change

The native x64 GUI launcher built on Ubuntu 24.04 amd64 in a disposable Linux
container. The package integration checks and a `.deb` install/CLI/uninstall
cycle passed there. The existing Xvfb GUI smoke check passed with the WebKit
sandbox disabled only for that container run because its namespace restriction
prevented `bwrap` from starting; this does not verify a real desktop session.
Ubuntu 26.04, ARM64, a real desktop session, production notice completeness,
and official support-table verification remain unverified.
