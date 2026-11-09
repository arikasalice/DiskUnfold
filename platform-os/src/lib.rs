//! Operating-system adapters. The in-memory implementation is test-only.
#[cfg(feature = "fake")]
pub mod fake;
#[cfg(target_os = "linux")]
pub mod scan_observation;
pub mod volume;
#[cfg(feature = "webview")]
pub mod webview;
#[cfg(all(windows, feature = "windows-console"))]
#[allow(unsafe_code)]
pub mod windows_console;
#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
pub mod windows_observation;
