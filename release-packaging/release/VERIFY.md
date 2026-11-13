# Verify a DiskUnfold download

This page is published with every release. It tells you how to compare a downloaded file with the project's official records. DiskUnfold does not check any of this for you at run time and never contacts the network to verify itself. A digest shown on a screen is not a verification result.

## What each file proves

| Check | Needs | What a pass proves | What it does not prove |
| --- | --- | --- | --- |
| SHA-256 match against `SHA256SUMS` | The `SHA256SUMS` file from the same release page | Your file has the same bytes as the file the release lists | That the release page or the list is authentic |
| Build provenance (`gh attestation verify`) | GitHub CLI 2.102.0 or newer, internet access | GitHub signed a statement that this repository's `release.yml` workflow built a file with this SHA-256 from this tag and commit on a GitHub-hosted runner | That the source code is free of defects, or that the workflow itself is safe |
| Platform signature | Windows: SignPath or Store; macOS: Hardened Runtime ad-hoc | See the "Signing" column in `DISTRIBUTION.md` | An ad-hoc signature does not identify a publisher. A file marked "unsigned" has no platform signature at all. |

`DISTRIBUTION.md` lists the signing method of every file. Files marked unsigned are unsigned.

## Three outcomes

- **Match.** The checksum equals the list entry and the attestation verifies. Use the file.
- **Mismatch.** The checksum differs or GitHub rejects the attestation. Stop using the file, delete it, and download it again from the official release page. If it still fails, report it.
- **Cannot verify.** `SHA256SUMS` or the GitHub CLI is missing, or GitHub is unreachable. The file is not verified. Fetch what is missing and run the check again.

## Steps (Windows, macOS, Ubuntu)

1. Download the file, `SHA256SUMS` and `release-provenance.json` from the same release page.
2. Compare the checksum.
   - Windows PowerShell: `Get-FileHash -LiteralPath '<file>' -Algorithm SHA256`
   - macOS: `shasum -a 256 '<file>'`
   - Ubuntu: `sha256sum '<file>'`

   The result must equal the line for that file name in `SHA256SUMS`.
3. Install the GitHub CLI from https://cli.github.com and run the one-line check. Replace `OWNER/REPO` with the repository that hosts the release and `vX.Y.Z` with the release tag:

   ```text
   gh attestation verify <file> --repo OWNER/REPO --signer-workflow OWNER/REPO/.github/workflows/release.yml --source-ref refs/tags/vX.Y.Z --deny-self-hosted-runners
   ```

   The tag and repository come from you, not from the file. A pass means GitHub holds a signed attestation, issued to that workflow at that tag, for exactly this SHA-256.

## One command with the project script

The repository script runs both steps and reports the three outcomes. It needs Node (version in `.node-version`) and a clone of the repository at the release tag:

```text
node release-packaging/release/verify-download.mjs <file> --repo OWNER/REPO --tag vX.Y.Z --sums SHA256SUMS
```

Exit code 0 is a match, 1 a mismatch and 3 cannot verify. Without `--gh-sha256` the script hashes the `gh` found on your computer; that only records which program ran. Pass `--gh-sha256 <digest>` from a source you trust for an independent pin.

## Strict policy check with the project verifier

`verify-release-provenance` checks the attestation against an exact policy: file digest, repository, tag, commit, workflow path, run ID, run attempt, event and builder ID. Build it from the tagged source and give it the values from `release-provenance.json`:

```text
cargo build --locked --release -p du-release-tools --bin verify-release-provenance
node release-packaging/release/verify-download.mjs <file> --repo OWNER/REPO --tag vX.Y.Z --sums SHA256SUMS \
  --provenance release-provenance.json --verifier target/release/verify-release-provenance \
  --gh-sha256 <digest of your gh>
```

See `release-packaging/release-tools/PROVENANCE.md` for the exact trust boundary of that command.

## Executables inside packages

`EXECUTABLE-SHA256.json` lists the SHA-256 of the executable inside each package. After you install or extract a package, hash the executable and compare it with the entry for that package. The release workflow does not attest these executables separately; they are covered through the package they ship in.

## SBOM

Each package has a CycloneDX SBOM named `<package file name>.cdx.json`. It lists the third-party Cargo crates and npm packages that a build for that package's CPU and operating system links, with their versions and SPDX license expressions, and it records the SHA-256 of the package it describes and the Rust targets it was written for. The release workflow checked that every package in the SBOM is in `THIRD-PARTY-NOTICES` (which covers every operating system) and that the SBOM matches the set that `cargo tree` and `npm ls` report for the target. `fuzz-gate.json` and the "Fuzz gate" section of `DISTRIBUTION.md` say whether any fuzz target was registered. The SBOM files and `fuzz-gate.json` are listed in `SHA256SUMS` and covered by the same build provenance as the packages.
