# DiskUnfold engineering instructions

This is the shared repository guide for coding agents and contributors. Update this file when a repository-wide rule changes. Tool-specific entry files point here.

## Product and design baseline

DiskUnfold is an offline desktop application for understanding disk use and reviewing user-controlled cleanup. The 0.1 design covers Windows, macOS and Linux. The current foundation has a runnable GUI shell and CLI help/version, shared contracts, isolated tests and build checks. The analysis, cleanup and MCP use cases require their own implementation work.

- Use the published AIDA DiskUnfold 0.1 architecture HLD and DLD as design sources. Feature integration starts from BL-HLD-1.17 (revision 220) and BL-DLD-1.6 (revision 1335). DLD sections 3.2 and 3.4 distinguish the verified foundation from work scheduled for feature and release development. Check the relevant published revision before changing a contract.
- Before starting a feature, check its published HLD/DLD and required foundation gates. Map its design item IDs to the owning modules and tests. Keep draft design text in AIDA. If a required contract is missing or conflicts with the implementation, surface the gap before choosing new public behavior.
- The repository is a Cargo workspace with a separately locked npm UI. The deployment unit is one executable per platform. GUI and CLI are execution modes of that executable. The eventual MCP gateway runs inside the GUI process.
- Begin with `git status --short --branch`. Preserve existing work and isolate implementation changes from `main`. Keep commits focused and include the relevant AIDA item IDs when available.

## Agent delegation

- Explicitly select `gpt-6-luna` for every new subagent at every delegation level. Assign follow-up work only to Luna subagents. The designated Terra development and Astra design leads retain their models in their own project chats.
- Use `xhigh` reasoning effort for Luna assignments that require design judgment, complex implementation, or investigation. Keep the existing effort for routine builds, test execution, and mechanical verification.
- Delegate builds, test execution and repeatable verification to Luna workers. Team leads own work assignment, review the logs and affected entry points, and return reproducible failures to the implementation owner.
- Close completed subagent sessions when the tooling supports it. Otherwise leave them completed and do not reuse them. If a running subagent on another model cannot switch models, let it finish its current assignment and review its result before closing it. Use a new Luna subagent for any follow-up work that requires delegation.
- Team leads give each delegate a fixed starting commit, design references, owned paths, dependencies and acceptance checks. They review each submitted diff and verify the affected entry point. Send findings with evidence and reproduction steps to the owner and review the revised result.
- The integration lead relays cross-team findings, resolves dependencies, assigns owners for shared contracts and reviews team submissions before merging. Escalate unresolved or conflicting reviews and changes that affect another team's work to that lead. Report the commit, design IDs, test evidence, unverified scope and any decision needed.
- The design lead checks rendered changes against the approved design and returns visual or interaction findings to the implementation owner through the integration lead.

## Database and protocol coordination

- The database and protocol team audits existing contracts and maintains shared design rules. Its project chat uses `gpt-6.1-sol` with `medium` reasoning. Use `gpt-6-luna` with `xhigh` for that team's subagents.
- Consult the team when a design changes a shared storage or protocol contract. Bring the user outcome, published AIDA item and revision, owning modules and consumers, compatibility needs, failure cases, and proposed acceptance checks. Share decisions and unresolved dependencies with the integration lead.
- For storage changes, identify the data owner, schema and migration path, transaction boundary, and behavior after interruption or concurrent access. Preserve existing data and exercise migration and recovery using representative older state.
- For protocol changes, reuse the shared contract and its generated bindings. Identify the transport and authority boundary, affected consumers, wire compatibility, and the applicable ordering, cancellation, retry, and resynchronization behavior. Exercise the change through the affected entry points.
- The feature owner implements the agreed contract and reviews the normal, boundary, and failure evidence. The integration lead coordinates cross-team sequencing and assigns ownership of changes to shared contracts.

## Paths and ownership

| Path | Responsibility |
| --- | --- |
| `gui-web/` | React shell, view state, accessible controls and the typed IPC client |
| `app-host/` | GUI lifecycle, embedded assets, IPC dispatch and app-only session/approval state |
| `cli/` | Argument parsing, read/plan entry points, output and exit codes |
| `mcp-gateway/` | Local MCP transport, request checks and delegation to the engine |
| `core-contracts/` | Shared value types, errors, envelopes and ports |
| `engine-api/` | Shared engine entry point, use-case registration and request flow |
| `job-runtime/` | Job state transitions, scheduling, cancellation and event delivery |
| `scan-engine/` | File metadata collection and scan progress |
| `analysis-index/` | Immutable result versions, aggregation and queries |
| `snapshot-store/` | Snapshot format, comparison, catalog and retention policy |
| `duplicate-engine/` | Content identity verification and duplicate results |
| `change-planner/` | Immutable plans and protection checks |
| `change-executor/` | Revalidate and apply explicitly approved file changes |
| `share-policy/` | Decide what data can leave the application |
| `export-render/` | Render outputs using the applicable sharing policy |
| `platform-os/` | OS-specific ports, native webview adapters and the development-only `fake` implementation |
| `guidance-rules/` | Offline interpretation and item management guidance |
| `user-env/` | Settings, workspaces, localization and display preferences |
| `app-db/` | SQLite storage adapter and database connection policy |
| `snapshot-files/` | Snapshot file storage adapter |
| `release-packaging/` | Single launcher, architecture checker, quality scripts and future release tooling |

The Rust package name is `du-<directory>`. The shipped binary is `diskunfold` from `release-packaging/launcher/`. The `release-packaging/arch-check/` crate and `.github/workflows/quality.yml` enforce repository rules.

## Dependency and responsibility rules

```text
launcher -> app-host / cli
gui-web -> app-host (IPC)
app-host / cli / mcp-gateway -> engine-api -> business components -> core-contracts
app-host / cli -> adapter composition -> ports defined by core-contracts
app-host -> change-executor
```

- Use `release-packaging/allowed-deps.toml` for the exact allowed crate edges, including optional, build and development dependencies. Keep the graph acyclic. Only `app-host` may directly depend on `change-executor`.
- Keep one reason to change per module. Split transport, orchestration, business policy and I/O. Define narrow ports at the contract boundary and inject OS or storage implementations at composition points. Keep business policy and I/O out of `core-contracts`.
- Inside Rust crates, keep `api` depending inward on `domain`, and `domain` depending on `ports` where needed. Keep domain code independent of registry, strategy implementations and adapters. Place reusable behavior with its actual owner. The architecture checker rejects `common`, `util`, `helpers` and `misc` module names.
- Keep feature screens under `gui-web/src/features/<area>/` as they are introduced. Keep UI primitives in `components/`, view state in `state/`, and direct native messaging in `ipc/bridge.ts`. Features stay independent of other feature areas. Access generated contracts through the IPC entry point.
- Register all non-layout UI elements in `gui-web/src/components/index.ts` and use that common component entry point in app and feature views. Wrap interactive controls with React Aria inside the component layer. Keep theme tokens in `gui-web/src/theme/`. The GUI lint gate rejects raw non-layout JSX and direct UI library imports outside the component layer.
- Treat the GUI as the approval boundary for file changes. CLI and MCP can use published analysis and planning contracts when implemented. Keep network and filesystem access in the designated adapters.

## Toolchains and build order

Work from the repository root. Read `rust-toolchain.toml` and `.node-version` for the pinned versions. Follow the [README](README.md) for the native platform prerequisites. Use a permitted development environment for native toolchains. If Windows Application Control blocks generated binaries or native modules, record the failed native check and use an approved runner or Linux environment for supported checks.

1. Install web dependencies from the lockfile with lifecycle scripts disabled.
2. Generate TypeScript bindings from Rust when shared contracts change. Review and commit the resulting diff.
3. Build the web assets before building or testing the native workspace with GUI support. `app-host` embeds `gui-web/dist/`.
4. Build and exercise the launcher. The no-argument path opens the GUI.

```sh
npm --prefix gui-web ci --ignore-scripts
cargo run --locked -p du-core-contracts --features typescript --example export_types -- gui-web/src/ipc/generated
npm --prefix gui-web run build
cargo build --locked -p du-launcher
cargo run --locked -p du-launcher -- --help
cargo run --locked -p du-launcher -- --version
```

For a CLI-only development build, use `cargo run --locked -p du-launcher --no-default-features -- --help`. The shell smoke check and its Linux prerequisites are in the [README](README.md).

## Coding style

**Rust.** Use edition 2024 and workspace versions and lints. Use `rustfmt`, Clippy and typed `Result` errors. Preserve error context and convert errors at the transport boundary. Library code avoids `unwrap`, `expect` and panics. Unsafe code is confined to `platform-os` and requires a reason and safety invariant. Keep files within 500 lines and constructors within four dependencies. The structure checker enforces a maximum of nine direct internal dependencies excluding `core-contracts` and a function complexity limit of 20.

**TypeScript and React.** Use strict TypeScript and the configured ESLint, Prettier and dependency-cruiser rules. Keep view components focused, preserve keyboard and screen-reader access, and keep domain decisions in Rust. Use typed IPC from the client and avoid manual copies of Rust types. TypeScript 7 checks the application. The TypeScript 6 dependency supports tools that still require its compiler API.

**Tooling.** Use the pinned Node runtime and cross-platform filesystem and process APIs for quality scripts. Run Cargo with `--locked` and npm with `ci --ignore-scripts`. Review changes to lockfiles, record new direct dependencies in the quality register, and keep CI actions pinned to commit hashes. Licensing and advisory policy changes require the project’s applicable approval and traceability.

## Tests and handoff

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features
cargo run --locked -p du-arch-check --
npm --prefix gui-web run check
npm --prefix gui-web run test
```

- Test domain decisions through ports with deterministic fake input. Use `platform-os` with the `fake` feature in tests, temporary SQLite databases for storage tests and injected IPC transports for web tests. Use isolated fixtures for any operation that could change files.
- Cover normal, boundary and failure behavior when changing contracts or policies. Add a violating fixture when adding a rule that blocks merges. Review delegated changes and verify their actual results.
- See the [quality guide](release-packaging/quality/README.md) for license checks, coverage reports and the native target matrix. Use measured data for SAM/TEM reporting and distinguish missing data from a zero value or a pass.
- Report the exact checks and platforms used, failures and checks that could not run. A build or smoke test on one platform is evidence for that platform.
