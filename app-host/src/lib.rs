//! Assemble the GUI and own its lifecycle.

#[cfg(all(feature = "gui", windows))]
mod history;
#[cfg(feature = "gui")]
mod ipc;
#[cfg(feature = "gui")]
mod lifecycle;
#[cfg(all(feature = "gui", windows))]
mod locale;
#[cfg(all(feature = "gui", windows))]
mod settings;
#[cfg(all(feature = "gui", windows))]
mod shared_database;
#[cfg(all(feature = "gui", windows))]
mod startup_database;

#[cfg(feature = "gui")]
pub use lifecycle::run;
