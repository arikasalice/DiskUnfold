# Release pipeline (F15 WP1)

`.github/workflows/release.yml` builds, packages and attests a release. Files here support it.

| File | Purpose |
| --- | --- |
| `format-versions.json` | The app database format (`du_app_db::HISTORY_SCHEMA_VERSION`) and DUSNAP envelope version (`du_snapshot_store::envelope::DUSNAP_FORMAT_VERSION`) embedded into an official build. The gate fails with `release.format_version_mismatch` when the file differs from the code, so a schema change must update this file in the same change. |
| `check-build-kind.mjs` | Reads the build info embedded in the newest launcher build and checks its kind and commit. |
| `check-build-kind.mjs --binary PATH` | Also runs the built executable's `version` command and checks that it shows the build kind and the commit (the version output comparison of release stage 3). |
| `dry-run.mjs` | Local simulation of the gate, the packaging and the later stages (SBOM, opened-package scan, channel manifests, fuzz gate with an empty crash list) on the current OS. `--run-tests` adds the workspace tests. Never official. |
| `lint-workflow.mjs` | Policy lint for the workflow: SHA pins with version comments, no TODO markers, tag-only triggers, draft-only release creation. `--verify-remote` resolves every pinned tag with `git ls-remote`. |
| `verify-download.mjs`, `VERIFY.md` | How a user verifies a download. `VERIFY.md` is published with the release. |

Gate commands live in `release-packaging/release-tools` (`release-gate`, `build-product-resources`, `build-release-inventory`, `verify-release-provenance`).

## Jobs

| Job | Requirement | What it does |
| --- | --- | --- |
| `gate` | FR5.HD1, NFR2.HD1 | Tag format, tag version equals workspace version, tag commit equals checkout and event commit, data format versions, reviewed Debian copyright (non-empty, no fixture wording, and a `debian-copyright.reviewed` record with the file's sha256 and a reviewer), `build-product-resources --release` (notices, support table, every security basis confirmed). Only a passing tag push yields `official=true`. |
| `build` | NFR1.HD1, FR9.HD1 | Native build per runner: Windows x64 and arm64, Ubuntu 24.04 x64 and arm64, macOS arm64 and x86_64. Checks the Rust host against the cell, checks the embedded build kind and the `version` output, **runs `cargo test --locked --workspace --all-features` on that CPU (stage 3, `release.test_failed`)**, packages Windows (portable EXE, MSIX) and Linux (deb, tar.gz), opens the portable EXE for the scan, records the combination. |
| `package-dmg` | FR9.HD1 | Joins the macOS slices with `lipo`, builds the DMG and mounts it read-only to copy out LICENSE and THIRD-PARTY-NOTICES.txt and the member names for the scan. |
| `artifact-scan` | FR5.HD1 | Stages 4 and 5. Opens every tar.gz, deb and MSIX (the portable EXE and DMG are opened by their native jobs) and requires LICENSE and THIRD-PARTY-NOTICES to be present at the format's place and equal to the generated files byte for byte (`release.notice_mismatch`), and no shared library, libdbus or GPL-family license file among the members (`release.license_denied`). A package that was not opened fails. |
| `notices-sbom` | FR2.HD1, FR5.HD1 | Stage 5. Writes one CycloneDX 1.5 SBOM per package (`<package>.cdx.json`) from the inventory that makes THIRD-PARTY-NOTICES (cargo metadata `--locked`, npm lockfile production packages) plus the pinned Rust and Node build tools with scope `excluded`, and fails with `release.notice_mismatch` when an SBOM and the notices list different packages, or an SBOM describes other bytes than its package. |
| `inventory` | FR1.HD1, FR2.HD1 | Merges manifests, checks the release set and signature claims, builds `SHA256SUMS`, `EXECUTABLE-SHA256.json`, `DISTRIBUTION.md` (with an SBOM column), rehashes the assets. Adds the SBOMs to the assets and to `SHA256SUMS`, so they are attested with the packages. |
| `release-gate` | NFR1.HD1 | Stage 8 (verification list ANFR4). Fails with `release.verification_failed` while an issue labelled `fuzz-crash` is open. `.github/workflows/fuzz.yml` files that issue when a run keeps crash inputs. Runs for dry runs; `attest` waits for it. An empty fuzz registry is reported, not counted as a pass. |
| `channel-manifests` | FR1.HD1 | Stage 9. Writes `channel-direct.json`, `channel-winget.json` and `channel-store.json` from `release-inventory.json` (artifact `channel-manifests`). Direct is available for a tag build. winget and Store say `unavailable` with `release.format_unavailable` until an MSI, a Store-signed MSIX and the winget identifier exist. The release manager checks and submits; this job's failure does not stop the draft. |
| `attest` | FR2.HD1 | `actions/attest-build-provenance` over `SHA256SUMS`. |
| `verify-provenance` | FR2.HD1 | `verify-release-provenance` for every package before publishing. |
| `publish-draft` | FR1.HD1 | `gh release create --draft`. A person publishes. |

These job names are not registered in a published architecture DLD and need design registration (the architecture DLD names the stages `notices-sbom`, `artifact-scan`, `release-gate` and `channel-manifests`), as do the reason keys `release.tag_format_invalid`, `release.tag_version_mismatch`, `release.tag_commit_mismatch`, `release.format_version_missing`, `release.manifest_inconsistent`, `release.artifact_missing`, `release.signature_unverified`, `release.format_unavailable`, `release.debian_copyright_missing`, `release.build_kind_mismatch`, `release.signed_input_unsupported` and `release.notarization_unsupported`. The feature HLD stage table (table 7) lists `release.test_failed`, `release.license_denied`, `release.notice_mismatch` and `release.verification_failed`; the architecture DLD key list does not yet.

## SBOM scope

Each SBOM lists the packages THIRD-PARTY-NOTICES lists: the normal Cargo dependencies of the launcher on every target and the production packages of `gui-web/package-lock.json`. A Linux package therefore also lists crates that only a Windows or macOS build links. The comparison cannot say what a given executable links, and the SBOM does not list the native C libraries that crates bundle (SQLite, zstd) beyond their crates (`libsqlite3-sys`, `zstd-sys`).

## Dry run

A manual run of the workflow (workflow_dispatch) builds developer builds and uploads `release-assets-developer`. It never attests or publishes. Locally:

```text
node release-packaging/release/dry-run.mjs --out <new directory>
```

The report states whether the real gate would pass on the tree.

## Signing

SignPath runs only when the secret `SIGNPATH_API_TOKEN` and the variables `SIGNPATH_ORGANIZATION_ID`, `SIGNPATH_PROJECT_SLUG` and `SIGNPATH_SIGNING_POLICY_SLUG` exist. The Windows packager accepts unsigned input only, so a successful signing request stops the job with `release.signed_input_unsupported` instead of packaging mixed bytes. Apple notarization stops the same way when `APPLE_NOTARY_KEY_ID` exists. Without the secrets, packages keep the signature label the packager wrote: unsigned, or ad-hoc for the DMG. The `inventory` job rejects any row that claims a signature while no signing step ran.

## Checks

```text
node --test release-packaging/release
node release-packaging/release/lint-workflow.mjs --verify-remote
actionlint .github/workflows/release.yml
```

`actionlint` is not run in CI. Run it from an isolated install (GitHub release archive, checksum verified).
