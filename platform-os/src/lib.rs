//! Operating-system adapters. The in-memory implementation is test-only.
#[cfg(windows)]
pub mod application_data;
pub mod entropy;
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
pub mod windows_duplicate;
#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
pub mod windows_instance_lock;
#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
pub mod windows_observation;
#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
pub mod windows_snapshot;
#[cfg(target_os = "windows")]
pub mod windows_watch_decode;
#[cfg(target_os = "windows")]
pub mod windows_watch_events;
#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
pub mod windows_watch_native;
