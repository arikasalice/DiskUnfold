//! Operating-system adapters. The in-memory implementation is test-only.
#[cfg(feature = "fake")]
pub mod fake;
#[cfg(feature = "webview")]
pub mod webview;
#[cfg(all(windows, feature = "windows-console"))]
#[allow(unsafe_code)]
pub mod windows_console;
