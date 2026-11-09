# Quality gate

`targets.json` is the single native OS/CPU matrix. Labels are documented GitHub-hosted runner labels; availability to this repository is unverified until each job actually runs. The matrix does not claim success for a queued or unavailable runner. Native jobs verify the Rust host triple before checks. Linux installs WebKitGTK and Tauri build prerequisites; Windows runners require the installed MSVC toolchain and WebView2 SDK/runtime; macOS runners require Xcode command-line tools. GitHub plan, runner availability, ARM ecosystem support and toolchain downloads can prevent a job from starting or completing.

Every PR and main push runs formatting, clippy, workspace tests with all features, architecture checks, GUI checks/build/tests, and a generated-contract diff. The reports job checks RustSec advisories, allowed licenses and crate registries, npm licenses, and the lockfile-derived direct dependency register. After an approved dependency change, update `dependency-annotations.json` with the exact version, SPDX license, role, distribution classification and reason, then run `node release-packaging/quality/dependency-register.mjs --write` and review both diffs. The distribution classification is a direct-dependency estimate; release artifact inspection remains required. CI fails when the register differs. Both package managers use committed lockfiles; npm installation disables lifecycle scripts.

The artifact `quality-reports` includes Rust and GUI LCOV, test output, `coverage-summary.json`, npm license inventories (`npm-licenses-production.json` and `npm-licenses-all.json`), fuzz report and jscpd JSON/HTML report (50 tokens). For each language TEM is `5 * min(line coverage / 0.8, 1) * function coverage * passed / executed`; missing data is null (N/A). The 80% coverage target is informational, but failing tests block CI. Duplication is reported without an arbitrary threshold. `targets` records the intended runners and each native job uploads its architecture report.

`fuzz-targets.json` is the registry of parser fuzz targets. An empty registry reports `not configured`, not a pass. Entries have a cargo-fuzz target name and positive duration in seconds. A registered target must be runnable with cargo-fuzz and fails the gate on any error; Pin `toolchain` (nightly date) and `cargoFuzzVersion` in the registry when registering the first target; the runner installs both.


The production npm graph is checked against the shipped allowlist separately from all installed packages. The latter also permits the OSI-approved BlueOak-1.0.0 and MPL-2.0 only as build/test dependencies; it still rejects unknown or unapproved expressions. This does not prove that a bundler copied no tool resources into a distribution: release artifact inspection remains necessary. Current development-only `spdx-exceptions` (CC-BY-3.0) and `spdx-ranges` (MIT AND CC-BY-3.0) are brought in by `license-checker-rseidelsohn` through its SPDX parser/comparison dependencies. CC-BY-3.0 is not in the current build-tool allowlist, so the gate remains blocked pending replacement or a documented approved exception. `lightningcss` and its platform package are Vite build dependencies under MPL-2.0; `chownr` is a development dependency under BlueOak-1.0.0. Package classification is checked from the installed dependency graph; it is not evidence of release artifact contents.

## Local verification and open gates

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

The foundation uses cargo-deny 0.20.2 and cargo-llvm-cov 0.6.21 in CI. The current Rust gate flags the Tauri dependency graph's MPL-2.0 packages (`cssparser`, `cssparser-macros`, `dtoa-short`, `option-ext`, `selectors`) and the unmaintained `proc-macro-error` dependency brought in through GTK tooling. These findings require a dependency change or a recorded project decision before the gate can turn green. The Rust source and package ban checks pass. SQLite uses bundled 3.54.1 via rusqlite 0.41.1 and libsqlite3-sys 0.39.1.
