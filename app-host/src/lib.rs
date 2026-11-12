//! Assemble the GUI and own its lifecycle.

#[cfg(all(feature = "gui", windows))]
mod clipboard;
#[cfg(all(feature = "gui", windows))]
mod history;
#[cfg(feature = "gui")]
mod ipc;
#[cfg(feature = "gui")]
mod lifecycle;
#[cfg(all(
    feature = "gui",
    any(windows, target_os = "linux", target_os = "macos")
))]
mod locale;
#[cfg(all(
    feature = "gui",
    any(windows, target_os = "linux", target_os = "macos")
))]
mod locale_delivery;
#[cfg(feature = "gui")]
pub mod recent_scope_body;
#[cfg(all(feature = "gui", windows))]
mod recent_scope_list;
#[cfg(all(feature = "gui", windows))]
mod recent_scope_open;
#[cfg(all(feature = "gui", windows))]
mod recent_scope_worker;
#[cfg(all(
    feature = "gui",
    any(windows, target_os = "linux", target_os = "macos")
))]
mod settings;
#[cfg(all(
    feature = "gui",
    any(windows, target_os = "linux", target_os = "macos")
))]
mod shared_database;
#[cfg(feature = "gui")]
mod shutdown;
#[cfg(feature = "gui")]
mod startup_checks;
#[cfg(all(
    feature = "gui",
    any(windows, target_os = "linux", target_os = "macos")
))]
mod startup_database;

#[cfg(feature = "gui")]
pub use lifecycle::run;
#[cfg(all(feature = "gui", windows))]
pub use lifecycle::run_with_recent_scope;
#[cfg(all(feature = "gui", windows))]
pub use recent_scope_worker::RecentScopeWriteWarning;
