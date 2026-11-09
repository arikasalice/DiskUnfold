# Quality gate

`targets.json` is the single native OS/CPU matrix. Native jobs verify the Rust host triple before checks. Linux installs GTK4 and WebKitGTK 6.0 development packages. Windows uses MSVC and the system WebView2 runtime. macOS uses Xcode Command Line Tools and the system WebKit framework. A target stays unverified until its native checks run.

Every PR and main push runs formatting, clippy, workspace tests with all features, architecture checks, GUI checks/build/tests, and a generated-contract diff. The reports job checks RustSec advisories, allowed licenses and crate registries, npm licenses, and the lockfile-derived direct dependency register. After an approved dependency change, update `dependency-annotations.json` with the exact version, SPDX license, role, distribution classification and reason, then run `node release-packaging/quality/dependency-register.mjs --write` and review both diffs. The distribution classification is a direct-dependency estimate; release artifact inspection remains required. CI fails when the register differs. Both package managers use committed lockfiles; npm installation disables lifecycle scripts.

The artifact `quality-reports` includes Rust and GUI LCOV, test output, `coverage-summary.json`, npm license inventories (`npm-licenses-production.json` and `npm-licenses-all.json`), fuzz report and jscpd JSON/HTML report (50 tokens). For each language TEM is `5 * min(line coverage / 0.8, 1) * function coverage * passed / executed`. Missing data is null (N/A). The 80% coverage target is informational. Failing tests block CI, and the report still includes any coverage collected during the run. A failed run is ineligible as a coverage baseline. Duplication is reported without an arbitrary threshold. `targets` records the intended runners and each native job uploads its architecture report.

`fuzz-targets.json` is the registry of parser fuzz targets. Each entry declares its repository-relative crate `owner`, Cargo binary `name`, and run duration `seconds` (1–600). The owner keeps the fuzz manifest, lockfile and optional seed inputs under `fuzz/`. When registering the first target, pin `toolchain` to a dated nightly and `cargoFuzzVersion` to an exact version.

PR CI checks the registry and Cargo target declarations with `node release-packaging/quality/fuzz.mjs --check`. The separate daily and manual workflow runs the registered targets with the pinned nightly. It fetches each fuzz workspace with `--locked`, runs offline and fails if the lockfile changes. Its cache preserves corpora by crate and target. Failure inputs are uploaded for 90 days and can be promoted to regression fixtures. An empty registry reports `not configured` in both workflows.

The npm gate uses the complete CycloneDX output from `npm sbom --sbom-format=cyclonedx --include=dev --package-lock-only`, matches every locked location to its alias and version, and checks installed production paths with `npm ls --all --omit=dev --json --long`. The lock-only inventory covers optional packages for other platforms. Anything reachable from the product is checked against the shipped allowlist. The build/test allowlist also permits BlueOak-1.0.0 and MPL-2.0. The Rust SPDX evaluator rejects unidentified expressions and evaluates the allowed branches of valid expressions. Release artifact inspection is required to determine actual distribution contents.

## Local verification

From the repository root after building the web assets, run the pinned quality tools:

```sh
cargo deny --locked --all-features check advisories licenses sources bans
node release-packaging/quality/npm-licenses.mjs
node release-packaging/quality/dependency-register.mjs
node release-packaging/quality/rust-coverage.mjs
npm --prefix gui-web run test:coverage
node release-packaging/quality/coverage.mjs
```

To compare runs, give the coverage script a previous `coverage-summary.json` as its first argument. The new report records each line coverage delta and whether coverage decreased.

The quality workflow publishes a small coverage baseline artifact on main pushes. A PR compares with its exact base commit, and a main push compares with its previous commit. The lookup is restricted to completed push runs of this repository's quality workflow. The report checks the source commit, run, platform and measurement settings, and compares each language only when both coverage commands and their tests succeeded. A missing, expired or incompatible baseline gives N/A. The Actions job summary shows changes in percentage points, and declines remain informational.

CI pins cargo-deny 0.20.2 and cargo-llvm-cov 0.9.1. The native desktop graph removes the Tauri dependencies rejected by the Rust gate, including the GTK3 path to `proc-macro-error` (RUSTSEC-2024-0370). The CycloneDX gate removes the old npm license checker and its CC-BY-3.0 data dependencies. Both changes are validated with the same allowlists.
