# Quality gate

`targets.json` is the single native OS/CPU matrix. Native jobs verify the Rust host triple before checks. Linux installs GTK4 and WebKitGTK 6.0 development packages. Windows uses MSVC and the system WebView2 runtime. macOS uses Xcode Command Line Tools and the system WebKit framework. A target stays unverified until its native checks run.

Pull requests run the three `prCheck` native targets (`linux-x64`, `windows-x64`, `macos-arm64`) and the `static` checks; Windows UI smoke runs only when GUI-related paths change. Pushes to `main` or `0.0.1` run that set plus all UI smoke shards, coverage and duplication reporting; caches are saved on scheduled runs and pushes to the repository's default branch. The scheduled run (02:37 KST) and manual runs with `full=true` (the default) use all six native targets, UI smoke, three treemap repeats, coverage and duplication reporting. The `static` job checks RustSec advisories, allowed licenses and crate registries, npm licenses, and the lockfile-derived direct dependency register. After an approved dependency change, update `dependency-annotations.json` with the exact version, SPDX license, role, distribution classification and reason, then run `node release-packaging/quality/dependency-register.mjs --write` and review both diffs. The distribution classification is a direct-dependency estimate; release artifact inspection remains required. CI fails when the register differs. Both package managers use committed lockfiles; npm installation disables lifecycle scripts.

The artifact `quality-reports` includes Rust and GUI LCOV, test output and `coverage-summary.json`. The separate `quality-policy` artifact holds the npm license inventories (`npm-licenses-production.json` and `npm-licenses-all.json`) and the jscpd JSON/HTML report (50 tokens). For each language TEM is `5 * min(line coverage / 0.8, 1) * function coverage * passed / executed`. Missing data is null (N/A). The 80% coverage target is informational. Failing tests block CI, and the report still includes any coverage collected during the run. A failed run is ineligible as a coverage baseline. Duplication is reported without an arbitrary threshold. `targets` records the intended runners; the `static` job runs the architecture checker and its report is in `quality-policy`.

`fuzz-targets.json` is the registry of parser fuzz targets. Each entry declares its repository-relative crate `owner`, Cargo binary `name`, and run duration `seconds` (1–600). The owner keeps the fuzz manifest, lockfile and optional seed inputs under `fuzz/`. When registering the first target, pin `toolchain` to a dated nightly and `cargoFuzzVersion` to an exact version.

PR CI checks the registry and Cargo target declarations with `node release-packaging/quality/fuzz.mjs --check`. The separate daily and manual workflow runs the registered targets with the pinned nightly. It fetches each fuzz workspace with `--locked`, runs offline and fails if the lockfile changes. Its cache preserves corpora by crate and target. Failure inputs are uploaded for 90 days and can be promoted to regression fixtures. An empty registry reports `not configured` in both workflows.

The npm gate uses the complete CycloneDX output from `npm sbom --sbom-format=cyclonedx --include=dev --package-lock-only`, matches every locked location to its alias and version, and checks installed production paths with `npm ls --all --omit=dev --json --long`. The lock-only inventory covers optional packages for other platforms. Anything reachable from the product is checked against the shipped allowlist. The build/test allowlist also permits BlueOak-1.0.0 and MPL-2.0. The Rust SPDX evaluator rejects unidentified expressions and evaluates the allowed branches of valid expressions. Release artifact inspection is required to determine actual distribution contents.

## CI layout and caching policy

`quality.yml` uses three tiers:

- Pull requests run the three targets marked `prCheck` in `targets.json`, `static`, and UI smoke only when its GUI path expression matches a changed file.
- Pushes to `main` or `0.0.1` run the pull-request set plus all UI smoke shards, coverage and jscpd. Caches are saved only on a push to the repository's default branch.
- The daily schedule (02:37 KST; `37 17 * * *`) and manual dispatches with `full=true` (the default) run all six native targets, UI smoke, three treemap repeats, coverage and jscpd. For a scheduled run, `gate` skips the remaining work when the most recent successful scheduled run has the same `head_sha`.

There is no `lint`-only matrix: each `native` matrix job runs both clippy commands and the workspace tests for its target. No quality job is currently configured as a required check in the repository ruleset.

| Job | Work |
| --- | --- |
| `gate` | On scheduled runs, stops the workflow if the last successful scheduled run already covered the same commit. Runs on `ubuntu-slim`. |
| `prepare` | Runs `npm ci --ignore-scripts` and the Vite build once; publishes `gui-dist` and `targets` artifacts and outputs the native matrix and `ui_smoke` decision. |
| `static` | Formatting, GUI checks, Windows drop-bridge tests, architecture check, generated-contract diff, cargo-deny, npm licenses, dependency register, release workflow policy, coverage/matrix/fuzz checks, and jscpd outside PRs. Artifact `quality-policy`. |
| `native` (per selected target) | Two clippy commands and workspace tests; ignored NTFS or btrfs observation tests where applicable; Linux launch and WebKit smoke. Uses the three PR targets or all six full-tier targets. |
| `ui-smoke` | Three shards on `windows-2022`; each uploads `ui-smoke-windows-<n>`. Not configured as a required check. |
| `reports` | Rust and GUI coverage and base-commit comparison; excluded on PRs. Publishes `quality-reports`, and `coverage-baseline` on pushes. |

Evidence behind the layout (measurements from 2026-11-21): across eight successful runs, each run had 16 jobs, including four macOS jobs. Queue wait reached 68 minutes and total run time ranged from 20 to 98 minutes. The macOS Intel native test build took just over 9 minutes; UI smoke took about 20 minutes. Rust cache misses were observed in these runs. The workflow saves Rust caches only on scheduled runs and pushes to the repository's default branch; the configured condition refers to `github.event.repository.default_branch`. The 10 GB Actions cache limit was exceeded. These are baseline measurements, not measured results after the restructure. The goal is a median end-to-end time of 15 minutes or less across eight PRs and macOS queue wait of 5 minutes or less.

Compile settings are job-wide environment variables in `quality.yml`: `CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0` and `RUST_BACKTRACE=short`. They shrink compile and link work and cache size and leave panic messages intact. They do not touch `Cargo.toml` profiles or release builds. Coverage counts come from LLVM coverage mapping, not debug info. The default Linux x64 linker is already rust-lld on the pinned toolchain; no extra linker is installed.

Cache policy:

- `Swatinem/rust-cache` uses shared keys `native-<os>-<cpu>` for native targets, `static` for the static job, and `coverage` for reports. `ui-smoke` reads `native-windows-x64` and sets `save-if: false`. Rust caches are saved only by scheduled runs and pushes to the repository's default branch (`CACHE_SAVE`); pull requests and other manual runs restore without saving. The workflow does not hard-code `main` in this condition.
- The Actions cache quota is 10 GB. The estimated quality cache set is about 2.5 GB: roughly nine Rust caches, two architecture-specific apt caches and one or two npm caches. The measured total exceeded the quota; review and remove old Actions caches when approaching the limit.
- `actions/setup-node` enables the npm cache in `prepare`, `static` and `reports`, using `gui-web/package-lock.json`; all three use the same Node version and lockfile inputs for the key. `npm ci --ignore-scripts` still runs in each job that needs the dependencies.
- All Linux jobs pass the same `LINUX_PACKAGES` list to `.github/actions/apt-install`; the cache is separated by runner OS and CPU architecture and uses the `apt-debs-v3` prefix. The action always restores `.deb` files with `actions/cache/restore`; it saves them with `actions/cache/save` only when `save-cache=true` and the exact cache was not already present. Apt still updates its package index and installs the packages on every run.
- `cargo-deny` 0.20.2 and `cargo-llvm-cov` 0.9.1 are installed from prebuilt binaries with `taiki-e/install-action` v2.90.7, pinned by commit SHA.
- Cache hits do not skip checks: each `native` job still runs both clippy commands and the full workspace tests for its target.
- GitHub-maintained actions run on the Node 24 runtime and are pinned by commit SHA with a version comment: `actions/checkout` v7.0.1, `actions/setup-node` v7.1.0, `actions/cache/restore` and `actions/cache/save` v6.1.0, `actions/upload-artifact` v7.0.2, and `actions/download-artifact` v8.0.2. `taiki-e/install-action` v2.90.7 is also pinned by commit SHA. These actions need Actions runner 2.327.1 or newer, which GitHub-hosted runners provide. `release.yml` and `fuzz.yml` set `package-manager-cache: false` on `actions/setup-node`; `release-packaging/release/lint-workflow.mjs --verify-remote` checks each pin against its version comment.
- Concurrency: a newer pull request run or push run cancels the older run. Scheduled and manual runs are never cancelled.
- Path filters are not used. They can skip a required check and leave a pull request waiting for a status that never appears. The optional UI smoke job alone uses `prepare`'s `gui` expression to skip PR runs unless a changed path matches `gui-web/`, `app-host/`, `platform-os/`, `user-env/`, `core-contracts/`, `engine-api/`, `analysis-index/`, `scan-engine/`, `job-runtime/`, `app-db/`, `snapshot-store/`, `snapshot-files/`, `release-packaging/launcher/`, `release-packaging/quality/windows-ui-smoke`, `Cargo.lock`, or `.github/workflows/quality.yml`; it is not a required check.
- `release.yml` and `fuzz.yml` take no Rust or npm build caches. Official release builds must come from clean sources and a fresh toolchain so the provenance covers exactly the bytes built; do not restore `target/` into a release build. The release tool builds in the gate and inventory jobs stay uncached for the same reason.

## Windows UI smoke and captures

The `prepare` job builds the web assets once. The `ui-smoke` job runs on `windows-2022` in three shards, downloads those assets and builds the launcher. It calls `release-packaging/quality/windows-ui-smoke.ps1` with `-Shard` and `-ShardCount 3`; the script assigns scenarios by their JSON order index modulo 3. Each shard starts `release-packaging/launcher/smoke-windows.ps1` once per assigned scenario with isolated data (a fresh profile under `%TEMP%`, removed afterwards). Each artifact is named `ui-smoke-windows-<n>` and contains logs, `summary.json` and review PNGs. The job summary lists pass or fail per scenario. Scheduled runs and full manual runs also repeat `treemap` and `treemap-narrow` three times on shard 1. Pull requests run UI smoke only when a changed path matches the GUI path expression listed above.

Scenarios live in `release-packaging/quality/windows-ui-smoke.json`. A scenario has a unique lower-case `name`, the `arguments` (switches of `smoke-windows.ps1`) and optionally `locale` (`en` or `ko`), `theme` (`light` or `dark`) and `windowSize` (`WIDTHxHEIGHT`). Its captures go to a folder named `<name>-<locale>-<theme>-<size>`. The `-CaptureTour` switch walks the main screens once and saves a PNG of each, with no value checks; the full checks run in `functional`. A script test checks the list against the parameters of `smoke-windows.ps1`. Run one scenario locally with `pwsh release-packaging/quality/windows-ui-smoke.ps1 -Binary <diskunfold.exe> -OutputDirectory <folder> -Only <name>` in an interactive Windows session; add `-ScreenSize 1920x1080` when the display is smaller than the window size.

To run one shard locally, add `-Shard <n> -ShardCount 3`.

How the settings reach the app without clicking: the theme is passed to the WebView as `prefers-color-scheme` (`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`), the display language is written into the isolated profile's settings database (`locale.display_language`, with python's `sqlite3`) after a first start has created it, and the window is sized through UI Automation. For a non-English language the English UI Automation names of the script are mapped through the repository's message bundles (`user-env/i18n`).

No quality job is currently configured as a required check in the repository ruleset. UI smoke drives a real window with UI Automation; captures show the runner's own screen size and WebView2 build, so they are review evidence and not pixel baselines.
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

## Accessibility review list

`accessibility-review.json` is the product-wide accessibility review list (NFR3.HD1.gui-web.DD1). It has three parts:

- **Task rows.** One row per main task: scope selection (F01), browsing (F05), search (F04), display language and format change (F13), plan review (F10), approval (F28), cancel (F24) and result review (F30). Each row names the owning feature and says whether the GUI has an entry point today, with the file that shows it. Feature numbers follow the AIDA DiskUnfold 0.1 feature order. The entry point flags were read from the source at commit `3fbdcbf7`. A feature owner sets `present` to true in the same change that mounts the screen.
- **Combination axes.** Assistive technology by OS (Windows Narrator, macOS VoiceOver, Ubuntu Orca), display language (`ko`, `en`), keyboard layout (Korean 2-set `ko-2set`, US English `en-us`), app scale (150, 175 and 200 percent) and the minimum window (`1024x700`). That is 36 combinations.
- **Check items.** Name, role and state conveyed; focus visible and in order; not dependent on color; composition confirmation does not run an action; truncated text readable in full; main actions reachable.

A release review records one cell for every task, combination and check item (8 x 36 x 6 = 1728 cells). `node release-packaging/quality/accessibility-review.mjs --template <productVersion> <osVersion>` prints a results file with every cell set to `unverified`. A reviewer replaces each cell with a result as the combination is exercised. Cells can carry different `osVersion` values.

A record in the results file has these fields:

| Field | Meaning |
| --- | --- |
| `task`, `check` | Ids from the list. |
| `combination` | One value for each of the five axes (`assistiveTechnology` is the id). |
| `result` | `pass`, `fail`, `unverified` or `not_applicable`. |
| `productVersion`, `osVersion` | Product build and OS build that were reviewed. A file covers one product version. |
| `evidence` | Review note, capture or test number. Required unless the result is `unverified`. |
| `blocksTask` | Required on `fail`: true when the input means kept the reviewer from completing the task. Not allowed on other results. |

The measure is the number of (task, combination) pairs with a failed check that has `blocksTask: true`. The pass threshold is 0. A failure that does not block the task still fails the review. An `unverified` or missing cell is never a pass: the review is `incomplete`. A task whose row says there is no entry point can be `not_applicable` or `unverified` but cannot `pass`.

```sh
node release-packaging/quality/accessibility-review.mjs --check
node release-packaging/quality/accessibility-review.mjs --results path/to/results.json
node --test release-packaging/quality/
```

`--check` validates the list definition without release results; the measure is then null (not available), not zero. `--results` evaluates a results file and exits 0 only when the review passes. Both modes write `quality-output/accessibility-review-report.json`. Today a person runs `--check` and the tests by hand; no workflow under `.github` runs either of them, and no workflow runs the release review. Connecting `--check` and the tests to the merge stage and the combination review to the release tag is release owner work. This tool does not drive assistive technology; a person or a separate automated run produces the results file.

## GitHub-hosted runners only

All CI build and test jobs in `quality.yml`, `fuzz.yml` and `release.yml` use GitHub-hosted Actions runners. Do not use or configure self-hosted runners or runner overrides. The six quality matrix targets keep their fixed runner labels in `targets.json`; `matrix.mjs` rejects labels that do not match the approved GitHub-hosted OS/CPU mapping. No repository variable or workflow input can change runner selection.

