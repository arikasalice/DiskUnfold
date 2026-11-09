<img src="gui-web/src/app/assets/diskunfold-icon.svg" width="96" height="96" alt="DiskUnfold app icon">

# DiskUnfold

Understand what takes up your space.

DiskUnfold is an open-source desktop project for understanding where your disk space goes. The goal is to explore storage visually, see what changed between scans, and review cleanup plans before making changes to your files.

Version 0.1 is in development. The foundation includes a Rust desktop shell with an embedded React interface, a command-line entry point, and shared Rust contracts. Storage analysis, cleanup, MCP tools, and release packaging are planned.

## Run the foundation

Use the Rust toolchain from `rust-toolchain.toml` and Node.js from `.node-version`. Windows needs MSVC build tools with the Windows SDK resource compiler, the Microsoft Visual C++ runtime and the system WebView2 runtime. The executable links the WebView2 loader statically. macOS needs Xcode Command Line Tools and uses system WKWebView. On Ubuntu 24.04, install `build-essential pkg-config libgtk-4-dev libwebkitgtk-6.0-dev`. The Linux bindings require GTK 4.10 or newer and dynamically link the system libraries.

```sh
npm --prefix gui-web ci --ignore-scripts
npm --prefix gui-web run build
cargo run --locked -p du-launcher
cargo run --locked -p du-launcher -- --help
cargo run --locked -p du-launcher -- --version
```

The same executable opens the GUI without arguments and prints help or version information when requested. Use `--no-default-features` on the Cargo command for a development build that exercises only the CLI.

The Windows GUI build embeds the app icon and opens without a console window. When invoking this executable directly, use `& .\diskunfold.exe --version | Write-Output` in PowerShell or `start "" /b /wait diskunfold.exe --version` in Command Prompt to wait for CLI output and an exit code. The CLI-only development build uses the Windows console subsystem.

## Verify changes

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features
cargo run --locked -p du-arch-check --
npm --prefix gui-web run check
npm --prefix gui-web run test
```

After changing the common contracts, regenerate the TypeScript declarations and review the diff:

```sh
cargo run --locked -p du-core-contracts --features typescript --example export_types -- gui-web/src/ipc/generated
```

On Linux with Xvfb, Openbox, xdotool, and a D-Bus session, the startup test checks that the same binary prints CLI output, opens a window, and exits on a window close request:

```sh
cargo build --locked -p du-launcher
bash release-packaging/launcher/smoke-linux.sh target/debug/diskunfold
```

The quality workflow publishes test, structure, duplication, and coverage reports. The 80% line coverage target is tracked separately for Rust and the GUI. See [quality tools](release-packaging/quality/README.md) for local reporting commands and the target matrix.

## Architecture and feature handoff

The DiskUnfold 0.1 development baseline is AIDA architecture BL-HLD-1.15 (revision 214) and BL-DLD-1.5 (revision 1321). It records the native webview, Windows launcher and license gate decisions against the foundation at commit `4cfb134`. Cargo crates correspond to the design targets. Entry points and composition are separate from domain logic. DLD sections 3.2 and 3.4 record the implementation and verification scope.

| AIDA design items | Foundation location |
| --- | --- |
| AFR1.gui-web.ADD1, AFR2.app-host.ADD1, AFR4.cli.ADD1 | GUI, native webview lifecycle, typed IPC, CLI mode and launcher |
| AFR5.core-contracts.ADD1, AFR27.core-contracts.ADD1, AFR31.core-contracts.ADD1, AFR32.core-contracts.ADD1 | Value types, port definitions and common envelopes |
| AFR26.app-db.ADD1 | SQLite connection configuration and temporary database tests |
| AFR34.release-packaging.ADD1 | Cargo dependency and Rust source checks, GUI import rules |
| AFR35.platform-os.ADD1 | Development-only fake filesystem, time, random bytes and faults |
| AFR22.release-packaging.ADD1, AFR33.release-packaging.ADD1, AFR35.release-packaging.ADD1 | Locked toolchains, CI and quality reports |

`app-host` embeds and serves the Vite assets and routes typed IPC. `platform-os` owns native windowing and WebView callbacks. The webview accepts embedded resources from its assigned origin. Only the trusted top-level document can send IPC. Feature development starts with a published feature design and passing foundation checks. The AIDA version currently has no configured code sources.

Ubuntu 24.04 x64 is the local Linux verification environment. Windows and macOS x64 and ARM64 adapters passed cross-target static checks. The Windows x64 executable was linked, inspected for DLL imports, and smoke-tested on Windows 11. Windows ARM64, macOS native execution, Linux ARM64, and hosted CI require their own runner or device verification.

DiskUnfold is [MIT licensed](LICENSE).
