# Quality gate

`targets.json` is the single native OS/CPU matrix. Native jobs verify the Rust host triple before checks. Linux installs GTK4 and WebKitGTK 6.0 development packages. Windows uses MSVC and the system WebView2 runtime. macOS uses Xcode Command Line Tools and the system WebKit framework. A target stays unverified until its native checks run.

Every PR and main push runs formatting, clippy, workspace tests with all features, architecture checks, GUI checks/build/tests, and a generated-contract diff (see the CI layout below). The policy job checks RustSec advisories, allowed licenses and crate registries, npm licenses, and the lockfile-derived direct dependency register. After an approved dependency change, update `dependency-annotations.json` with the exact version, SPDX license, role, distribution classification and reason, then run `node release-packaging/quality/dependency-register.mjs --write` and review both diffs. The distribution classification is a direct-dependency estimate; release artifact inspection remains required. CI fails when the register differs. Both package managers use committed lockfiles; npm installation disables lifecycle scripts.

The artifact `quality-reports` includes Rust and GUI LCOV, test output, `coverage-summary.json` and fuzz report. The separate `quality-policy` artifact holds the npm license inventories (`npm-licenses-production.json` and `npm-licenses-all.json`) and the jscpd JSON/HTML report (50 tokens). For each language TEM is `5 * min(line coverage / 0.8, 1) * function coverage * passed / executed`. Missing data is null (N/A). The 80% coverage target is informational. Failing tests block CI, and the report still includes any coverage collected during the run. A failed run is ineligible as a coverage baseline. Duplication is reported without an arbitrary threshold. `targets` records the intended runners and each native job uploads its architecture report.

`fuzz-targets.json` is the registry of parser fuzz targets. Each entry declares its repository-relative crate `owner`, Cargo binary `name`, and run duration `seconds` (1–600). The owner keeps the fuzz manifest, lockfile and optional seed inputs under `fuzz/`. When registering the first target, pin `toolchain` to a dated nightly and `cargoFuzzVersion` to an exact version.

PR CI checks the registry and Cargo target declarations with `node release-packaging/quality/fuzz.mjs --check`. The separate daily and manual workflow runs the registered targets with the pinned nightly. It fetches each fuzz workspace with `--locked`, runs offline and fails if the lockfile changes. Its cache preserves corpora by crate and target. Failure inputs are uploaded for 90 days and can be promoted to regression fixtures. An empty registry reports `not configured` in both workflows.

The npm gate uses the complete CycloneDX output from `npm sbom --sbom-format=cyclonedx --include=dev --package-lock-only`, matches every locked location to its alias and version, and checks installed production paths with `npm ls --all --omit=dev --json --long`. The lock-only inventory covers optional packages for other platforms. Anything reachable from the product is checked against the shipped allowlist. The build/test allowlist also permits BlueOak-1.0.0 and MPL-2.0. The Rust SPDX evaluator rejects unidentified expressions and evaluates the allowed branches of valid expressions. Release artifact inspection is required to determine actual distribution contents.

## CI layout and caching policy

`quality.yml` runs these jobs. `lint` and `native` are matrices over the six native targets in `targets.json` and run in parallel on each runner, so clippy and the GUI checks no longer queue in front of the test build. The `native` job names are unchanged, so existing required checks keep matching. The new `lint` and `policy` jobs are not required checks until a maintainer adds them to branch protection.

| Job | Work |
| --- | --- |
| `targets` | Reads `targets.json` and emits the matrix. |
| `lint` (per target) | `cargo fmt --check`, clippy, GUI `npm run check` and `npm run test`, Windows folder-drop bridge test. |
| `native` (per target) | Workspace tests, Linux launch and WebKit smoke under Xvfb, architecture checker, generated-contract diff. |
| `policy` | cargo-deny, npm licenses, dependency register, release workflow policy, fuzz registry check, duplication report. Needs no GTK/WebKit packages and no GUI build. Artifact `quality-policy`. |
| `reports` | Rust and GUI coverage, base-commit comparison. Artifacts `quality-reports` and `coverage-baseline`. |

Evidence behind the layout (run 43363082548): on macOS x64 the test profile built for 8 min 39 s and ran for about 55 s; clippy built for 5 min; the other checks took about 4 min. Compilation dominates every native job, and tests, `cargo nextest` included, are not the bottleneck, so nextest is not used. `cargo-deny` and `cargo-llvm-cov` were compiled from source on every run (170 s and 71 s).

Compile settings are job-wide environment variables in `quality.yml`: `CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0` and `RUST_BACKTRACE=short`. They shrink compile and link work and cache size and leave panic messages intact. They do not touch `Cargo.toml` profiles or release builds. Coverage counts come from LLVM coverage mapping, not debug info. The default Linux x64 linker is already rust-lld on the pinned toolchain; no extra linker is installed.

Cache policy:

- `Swatinem/rust-cache` (pinned by commit SHA) caches the Cargo registry, dependency build output and `~/.cargo/bin`. Workspace crates are not cached and always recompile. Each job has its own key: job id, target triple (`lint`/`native` matrices) or a fixed name (`policy`, `coverage`), plus the rustc version, `Cargo.lock` and every `CARGO_*`/`RUST*` variable. A change to the toolchain, lockfile or the compile settings above produces a new key. `cargo install cargo-deny` and `cargo install cargo-llvm-cov` come back from the restored `~/.cargo/bin` and become no-ops.
- Caches are saved only by pushes to `main` and manual `workflow_dispatch` runs; pull requests restore the `main` cache and never write one. Pull request code therefore cannot poison the cache that `main` and later pull requests restore, and PR runs cannot evict it. `cache-on-failure: true` still saves dependency output when a later step fails on `main`. To warm or measure a branch, run Quality manually twice on that branch; a branch cache is visible only to that branch.
- Each repository has a 10 GB cache limit with least-recently-used eviction. Twelve Rust caches (six targets times `lint` and `native`) plus `policy` and `coverage` compete for it. If Actions reports evictions, drop the `lint` cache first or save only one architecture per OS. `npm` caches from `actions/setup-node` hold downloaded tarballs only, are keyed by `gui-web/package-lock.json` and are verified by the lockfile integrity hashes during `npm ci --ignore-scripts`.
- Stale caches are bounded by the lockfile and toolchain in the key. A cache never decides whether a job passes: every job still runs `--locked` and a full workspace compile of workspace crates.
- Concurrency: a newer push to a pull request cancels the older run. Main pushes and manual runs use a per-run group and are never cancelled, because each main commit needs its own coverage baseline.
- apt: the GTK and WebKit packages are still installed with apt on every Linux job (about 35 to 40 s). A third-party apt cache action would extract packages into system paths as root, so the time was kept instead.
- Path filters are not used. They can skip a required check and leave a pull request waiting for a status that never appears.
- `release.yml` and `fuzz.yml` take no Rust or npm build caches. Official release builds must come from clean sources and a fresh toolchain so the provenance covers exactly the bytes built; do not restore `target/` into a release build. The release tool builds in the gate and inventory jobs stay uncached for the same reason.

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
