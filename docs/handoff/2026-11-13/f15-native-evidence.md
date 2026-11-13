# F15 native evidence (packages D, E, K, L)

Base: ac22ba92, branch codex/f15-native-evidence. Release build of du-launcher with the GUI.
Tools added: release-packaging/launcher/smoke-windows-quit.ps1, smoke-linux-quit.sh, smoke-webview-ipc.mjs.

## Windows 11 25H2 Pro x64 (WebView2)

| Scenario | Result |
| --- | --- |
| Close window (idle), release exe | exit code 0 in 0.22 s, process and WebView2 helpers gone, lock sentinel opens exclusively, diskunfold.db-wal and -shm present before close and removed after, integrity_check ok, stderr empty |
| Close window while a scan job runs | same results; job_runtime_job_record holds one row with final_state cancelled |
| Portable exe from package-windows-development.ps1 (-ProbeIpc) | app.product_info installForm "portable"; app.startup_status grade "unverifiable" (resource_integrity, webview, temporary_files, support_matrix, capability unverifiable; data_format_and_open pass); exit code 0 |

Portable exe SHA-256 b4c655815b55b9a322e28ca1c746a8bd5e364b35161266ded3270adbbfb2ed69.

## Ubuntu 24.04.5 x64 container (WebKitGTK 2.52.6, Xvfb, Openbox)

| Scenario | Result |
| --- | --- |
| Start without WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS | no window: "bwrap: Creating new namespace failed: Operation not permitted", then "Failed to fully launch dbus-proxy" |
| Start with the variable set, close with xdotool (alt+F4) | window closes, lock released, -wal/-shm removed, integrity ok, exit code 101 (Rust panic) |
| tar.gz extracted, PROBE_IPC=1 | installForm "tar_gz"; same close result |
| deb installed with dpkg -i, PROBE_IPC=1 | installForm "deb"; same close result |

Panic text: "called Result::unwrap() on an Err value: BoolError { message: "Failed to remove source" ... glib::source::SourceId::remove" after "GLib-CRITICAL: Source ID 39 was not found".
Cause: platform-os/src/webview/linux.rs removes shutdown_timer after the main loop, but its closure already returned ControlFlow::Break once shutdown_is_complete() was true, so the source is gone. Product fix needed (not made here).

deb SHA-256 b1f1ca244f7176f5afce2cc0840546af4ce59f31961e35b2842d6421fae31502; tar.gz SHA-256 04483816f0a79051c6b33223d2bc2dbe149c87bcbe36da54f82049ccef7d43b4.
The container build has no .git, so product_info commit is null there.

## Notes

- app.startup_status and app.product_info are read through the same IPC the GUI uses: WebView2 remote debugging port on Windows, WebKitGTK remote inspector on Linux. Both are test-only environment settings.
- THIRD-PARTY-NOTICES is reported unavailable on the Windows portable package (the packager ships LICENSE only).
- Not run: Linux scan-in-progress close (no UI Automation there), macOS, ARM64, MSIX, Windows Home, 26.04, 26H2.
