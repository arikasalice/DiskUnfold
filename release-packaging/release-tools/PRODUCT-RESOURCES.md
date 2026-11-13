# Build info, notices and support table

F15 FR5.HD1, FR6.HD1, NFR1.HD1 and NFR2.HD1 (DD: FR5.HD1.app-host.DD1, FR6.HD1.app-host.DD1, NFR2.HD1.app-host.DD1).

## Generate the release resources

    cargo run --locked -p du-release-tools --bin build-product-resources -- --out <dir> [--release --source-commit <40 hex>]

The command writes THIRD-PARTY-NOTICES, SUPPORT-TABLE.md, support-matrix.json and product-resources.json to <dir>. It exits with 1 when the release gate has blockers; --allow-blockers (development only, refused with --release) keeps the exit code 0 so a developer can inspect the output.

- Notices come from cargo metadata --locked (normal dependencies of du-launcher on every target) and the production packages of gui-web/package-lock.json. License texts are read from the crate directory and, when gui-web/node_modules is installed (npm ci --ignore-scripts), from the npm package; otherwise the SPDX expression stands in. Identical texts appear once. Output has no timestamps or paths.
- The policy is deny.toml: licenses.allow and the bans.deny crate names. Missing, denied or banned entries become blockers.
- Debian copyright: `build-product-resources --debian-copyright <file>` writes the DEP-5 file for the Linux package from the same package inventory (see release-packaging/linux/README.md). The release gate also needs a `debian-copyright.reviewed` record that matches the file digest.
- The support table comes from the supportTable section of release-packaging/quality/targets.json and the records in release-packaging/release-tools/support-records. A row without a record is not_verified and never takes another row's result. Status is derived from the records: officially_verified needs native_build, native_tests and installed_start all passed on that exact combination; any failed check gives unsupported_found; otherwise passed or limited checks give conditional.
- Key encodings: Windows release is 10 or 11, a hyphen and DisplayVersion (11-25H2) with edition = EditionID (Professional, Core); macOS release is the major version with edition macos; Linux release is VERSION_ID with edition = ID from os-release.
- A security-support basis is required on every row (kind, source, check date). A row without a complete basis is a blocker (release.support_basis_missing) and is left out of the embedded matrix. A basis with confirmed set to false is a warning in development and a blocker in --release mode (release.support_basis_unconfirmed); the embedded source text carries [unconfirmed] in development mode.

## Embed into a build

launcher/build.rs reads these environment variables:

| Variable | Meaning |
| --- | --- |
| DU_RELEASE_RESOURCES | Path (relative to the repository root) of product-resources.json. Supplies third-party notices, the support matrix and the matrix revision. |
| DU_BUILD_KIND | official requests the official mark; anything else is a developer build. |
| DU_SOURCE_COMMIT | Full commit supplied by CI. Without it a developer build records git rev-parse HEAD, suffixed -dirty when tracked files differ. |
| DU_RELEASE_TAG, DU_BUILT_AT_UTC, DU_APP_DB_FORMAT, DU_SNAPSHOT_FORMAT | Required for an official build; otherwise the fields stay unavailable. |

An official build fails unless the tag, commit, UTC time, both format versions and release-mode resources with a passed gate for the same commit are present. The logic lives in src/build_info.rs, which build.rs includes by path, so release-tools tests exercise the same code.

## Reason keys that need design registration

release.support_basis_missing, release.support_basis_unconfirmed, release.support_evidence_invalid, release.notices_license_unknown, release.notices_license_denied, release.notices_package_banned, release.source_commit_missing.
