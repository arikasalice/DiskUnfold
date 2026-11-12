//! Assemble the GUI and own its lifecycle.

#[cfg(all(feature = "gui", windows))]
mod clipboard;
#[cfg(all(feature = "gui", windows))]
mod history;
#[cfg(feature = "gui")]
mod ipc;
#[cfg(feature = "gui")]
mod lifecycle;
#[cfg(all(feature = "gui", windows))]
mod locale;
#[cfg(all(feature = "gui", windows))]
mod locale_delivery;
#[cfg(feature = "gui")]
pub mod recent_scope_body;
#[cfg(all(feature = "gui", windows))]
mod recent_scope_list;
#[cfg(all(feature = "gui", windows))]
mod recent_scope_open;
#[cfg(all(feature = "gui", windows))]
mod recent_scope_worker;
#[cfg(all(feature = "gui", windows))]
mod settings;
#[cfg(all(feature = "gui", windows))]
mod shared_database;
#[cfg(all(feature = "gui", windows))]
mod startup_database;

#[cfg(feature = "gui")]
pub use lifecycle::run;
#[cfg(all(feature = "gui", windows))]
pub use lifecycle::run_with_recent_scope;
#[cfg(all(feature = "gui", windows))]
pub use recent_scope_worker::RecentScopeWriteWarning;
