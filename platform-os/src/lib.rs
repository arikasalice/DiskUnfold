//! Operating-system adapters. The in-memory implementation is test-only.
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
pub mod application_data;
pub mod change_ops;
#[cfg(unix)]
pub mod cli_link;
pub mod device;
pub mod entropy;
#[cfg(feature = "fake")]
pub mod fake;
pub mod install_form;
#[cfg(any(windows, unix))]
#[allow(unsafe_code)]
pub mod interrupt;
#[cfg(target_os = "linux")]
pub mod linux_instance_lock;
#[cfg(target_os = "linux")]
pub mod linux_observation;
#[cfg(all(target_os = "linux", feature = "webview"))]
pub mod linux_time_zone;
#[cfg(target_os = "linux")]
pub mod linux_ui_language;
#[cfg(target_os = "macos")]
pub mod macos_instance_lock;
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
pub mod macos_observation;
#[cfg(all(target_os = "macos", feature = "webview"))]
pub mod macos_time_zone;
#[cfg(target_os = "macos")]
pub mod macos_ui_language;
pub mod os_release;
pub mod path_link;
#[allow(unsafe_code)]
pub mod process_memory;
pub mod self_executable;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
#[allow(unsafe_code)]
pub mod snapshot_file_system;
pub mod startup;
pub mod support_environment;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix_change;
pub mod volume;
#[cfg(feature = "webview")]
pub mod webview;
#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
pub mod windows_clipboard;
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
#[cfg(windows)]
#[allow(unsafe_code)]
pub mod windows_os_release;
#[cfg(windows)]
#[allow(unsafe_code)]
pub mod windows_package_identity;
#[cfg(windows)]
#[allow(unsafe_code)]
pub mod windows_placeholder_mode;
#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
pub mod windows_snapshot;
#[cfg(windows)]
#[allow(unsafe_code)]
pub mod windows_time_zone;
#[cfg(any(windows, test))]
#[allow(unsafe_code)]
pub mod windows_ui_language;
#[cfg(target_os = "windows")]
pub mod windows_watch_decode;
#[cfg(target_os = "windows")]
pub mod windows_watch_events;
#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
pub mod windows_watch_native;
#[cfg(target_os = "windows")]
pub mod windows_watch_root;
