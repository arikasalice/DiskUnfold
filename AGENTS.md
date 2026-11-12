# DiskUnfold engineering instructions

This is the shared repository guide for contributors and coding agents. Update it when a repository-wide rule changes. Tool-specific entry files point here.

## Product purpose

DiskUnfold helps people discover what takes up disk space, investigate folders and files, compare observations, and review cleanup before authorizing file changes. The 0.1 design covers Windows, macOS and Linux. Disk analysis and cleanup run locally. The product design gives the GUI ownership of file-change approval.

The deployment unit is one executable per platform. The launcher selects GUI or CLI mode. The GUI runs an embedded React frontend in the system webview. The design places the local MCP gateway inside the GUI process. Check the branch and its test evidence for implementation and platform support.

## Find the source of truth

| Question | Starting point |
| --- | --- |
| What behavior does the feature require? | Find its feature number in AIDA DiskUnfold 0.1. Read the published SRS for requirements, HLD for user flows and service contracts, and DLD for implementation and verification. |
| Which service owns the contract? | Use the published AIDA architecture HLD/DLD, the ownership table below and the feature's design item IDs. Architecture DLD §§3.2 and 3.4 distinguish the foundation from scheduled feature work. |
| Where does a request enter the application? | Start at [the launcher](release-packaging/launcher/), [the GUI IPC boundary](app-host/src/ipc.rs), [the CLI](cli/), or [the MCP gateway](mcp-gateway/). Follow registration into [engine-api](engine-api/src/api.rs) and the owning component. |
| Where are shared types and their GUI bindings? | [core-contracts](core-contracts/) owns shared Rust types and ports. [The GUI IPC entry](gui-web/src/ipc/index.ts) exposes the generated types and typed client. |
| Which dependencies and checks are enforced? | [The dependency allowlist](release-packaging/allowed-deps.toml), [architecture checker](release-packaging/arch-check/), [quality workflow](.github/workflows/quality.yml) and [quality guide](release-packaging/quality/README.md). |
| How do I run and verify on a platform? | See the [README](README.md) for prerequisites and the [launcher directory](release-packaging/launcher/) for smoke scripts. Verify support with results from the applicable platform. |

Begin with `git status --short --branch`. Preserve existing work, isolate implementation from `main`, and record the starting commit. The original integration baselines were BL-HLD-1.17 (revision 220) and BL-DLD-1.6 (revision 1335). For each change, check the applicable published revision and map the feature's design item IDs to code and tests. Keep draft design text in AIDA and surface missing or conflicting contracts before adding public behavior. Keep commits focused and include the relevant design IDs when available.

## Agent delegation

- Explicitly select `gpt-6-luna` for every new subagent at every delegation level. Assign follow-up work only to Luna subagents. The designated Terra development and Astra design leads retain their models in their own project chats.
- Use `xhigh` reasoning effort for Luna assignments that require design judgment, complex implementation, or investigation. Keep the existing effort for routine builds, test execution, and mechanical verification.
- Delegate builds, test execution and repeatable verification to Luna workers. Team leads own work assignment, review the logs and affected entry points, and return reproducible failures to the implementation owner.
- Close completed subagent sessions when the tooling supports it. Otherwise leave them completed and do not reuse them. If a running subagent on another model cannot switch models, let it finish its current assignment and review its result before closing it. Use a new Luna subagent for any follow-up work that requires delegation.
- Team leads give each delegate a fixed starting commit, design references, owned paths, dependencies and acceptance checks. They review each submitted diff and verify the affected entry point. Send findings with evidence and reproduction steps to the owner and review the revised result.
- The integration lead relays cross-team findings, resolves dependencies, assigns owners for shared contracts and reviews team submissions before merging. Escalate unresolved or conflicting reviews and changes that affect another team's work to that lead. Report the commit, design IDs, test evidence, unverified scope and any decision needed.
- The design lead checks rendered changes against the approved design and returns visual or interaction findings to the implementation owner through the integration lead.

## Database and protocol coordination

- Bring changes to shared storage or protocol contracts to the database and protocol team. Identify the published AIDA items and revisions, owning modules, callers, compatibility needs, failure modes and acceptance checks. The integration lead assigns shared changes and orders dependent work. The feature owner implements the agreed contract and returns evidence for review. Resolve missing or conflicting contracts before changing the behavior they govern.

### Database and durable files

- Use `app-db` for the shared SQLite connection policy, schema registry and physical adapters. Domain owners define table meaning, keys, invariants and write permissions; consumers read through narrow ports. Document primary and foreign keys, null and unknown semantics, delete behavior, and indexes for actual lookup and ordering paths. For a new hot path, show the query plan and bound the amount of work using representative data.
- Register schema changes in the shared migration sequence and preserve the meaning of existing rows. Route production upgrades through the owning GUI startup path and verify the backup before applying a migration. Verify this path on each platform before enabling upgrades there. Commit schema changes and their version bookkeeping together. Define what an older or read-only consumer can open and how it reports an unsupported schema. CLI and other read-only consumers must not create, repair or migrate the database.
- Group dependent writes and the checks that authorize them in one transaction. Define the expected version or identity, when locks are taken, how busy or stale writes fail, and whether a retry is safe. Preserve committed state on rollback. Keep unrelated I/O and expensive computation out of the write transaction; recheck the relevant state at commit.
- When durable files and database rows change together, name the source of truth, the file publication and row commit points, and who reconciles each interrupted state. Publish success only after the required steps complete. Preserve evidence needed for recovery and follow the feature contract for orphan files, missing files and deletion records. Keep session-only state out of the database unless its published contract changes.
- Verify upgrades with representative older data and test a normal write, boundary values, concurrent or stale writers, rollback, interruption and reopen. For file plus database changes, exercise failure on each side of the commit boundary and verify restart behavior through the owning entry point.

### Protocols and in-process ports

- Define shared wire types and stable error codes and reason keys in `core-contracts`. Regenerate TypeScript from Rust and review the serialized shape and typed IPC map together. Specify how omitted, null and unknown values behave. Within a contract version, add only optional fields and preserve existing field meanings and errors. Accept unknown fields in envelopes, responses and events; keep command-specific validation of GUI request bodies and separate MCP request validation. Use the published versioning plan for incompatible changes on external interfaces. Keep IPC, CLI output, MCP and Rust port contracts distinct; translate at the adapter boundary.
- Derive caller and session authority at the host or transport. Validate envelopes and allowed operations at ingress, and enforce ownership, scope and pinned evidence where the use case admits or applies the operation. Request IDs, result IDs and client state do not grant authority. Bind asynchronous jobs, queries and events to their owning context, and retain the GUI approval boundary for file changes.
- Specify the scope and lifetime of each version, revision, cursor and sequence. Define how cancellation, replacement, duplicates, delayed results and lost acknowledgements affect that scope. When the contract requires an event for a state change, the owner commits the state before publishing. Define the bounded delivery path, how gaps or overflow are signaled, and how consumers rebuild from an authoritative baseline without applying stale events.
- For channels subject to the published sharing policy, filter data before rendering or delivery. Apply channel authorization, scope, filtering and any required preview or digest checks before exposing bytes. Test compatible payloads, bad input and denied callers, plus applicable races, retries, overflow and resynchronization through the affected entry points.

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

## Architecture and allowed directions

```text
launcher -> app-host | cli
gui-web -> app-host -> engine-api -> business components
app-host -> mcp-gateway -> engine-api
cli -> engine-api
app-host -> change-executor (approved changes)
app-host | cli -> platform-os | app-db | snapshot-files (adapter composition)
business components and adapters -> core-contracts (shared types and ports)
```

- The diagram summarizes entry points and composition. The [dependency allowlist](release-packaging/allowed-deps.toml) governs exact direct Rust crate edges, including optional, build and development dependencies. Keep the graph acyclic. Only `app-host` may directly depend on `change-executor`.
- The GUI sends typed IPC through the trusted webview document. `app-host` enforces the caller boundary and dispatches registered use cases. `engine-api` orchestrates business components through their published interfaces. Each event keeps its published producer and delivery path, and `app-host` relays GUI events through IPC.
- Define shared ports and values in `core-contracts`. Business components own decisions. The composition root injects native, database and file-storage adapters. Keep business policy and I/O out of `core-contracts`, and keep network and filesystem access in the designated adapters.
- Keep CLI and MCP on their published analysis and planning entry points. Route file-change approval and execution through the GUI boundary.

## Service internals

- Use the crate root as its public entry and expose the smallest API its callers need. Keep request admission and orchestration at that boundary. Put business rules and state transitions in their owning modules, and keep adapter I/O and transport mapping at their own boundaries.
- Where a Rust service uses `api`, `domain` and `ports` modules, the allowed direction is `api -> domain -> ports`. Domain logic stays independent of transport, composition, registries and concrete adapters. Resolve strategies from the owner's registry at assembly or admission and keep their published identity with the result where required.
- Give each module one reason to change. Place reusable behavior with its owner and inject external dependencies through narrow ports. The architecture checker rejects `common`, `util`, `helpers` and `misc` module names.
- Keep feature screens in `gui-web/src/features/<area>/`, reusable controls in `components/`, view state in `state/`, and direct native messaging in `ipc/bridge.ts`. Features stay independent of other feature areas. Access generated contracts through the IPC entry point.
- Register all non-layout UI elements in `gui-web/src/components/index.ts` and use that common component entry point in app and feature views. Wrap interactive controls with React Aria inside the component layer. Keep theme tokens in `gui-web/src/theme/`. The GUI lint gate rejects raw non-layout JSX and direct UI library imports outside the component layer.

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

**Documentation comments.** Write English Rustdoc or TSDoc for new or changed public interfaces. Describe caller obligations, invariants, and meaningful error or concurrency behavior when applicable. Use brief inline comments for decisions the code cannot express on its own.

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
- Exercise the normal entry point affected by the change. On Windows, use [the native smoke script](release-packaging/launcher/smoke-windows.ps1) with an exact build and isolated test data. Coordinate ownership of the GUI session so only one DiskUnfold test instance runs at a time.
- See the [quality guide](release-packaging/quality/README.md) for license checks, coverage reports and the native target matrix. Use measured data for SAM/TEM reporting and distinguish missing data from a zero value or a pass.
- Report the exact checks and platforms used, failures and checks that could not run. A build or smoke test on one platform is evidence for that platform.
