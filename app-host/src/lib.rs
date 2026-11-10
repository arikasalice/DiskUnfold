//! Assemble the GUI and own its lifecycle.

#[cfg(feature = "gui")]
mod ipc;
#[cfg(feature = "gui")]
mod lifecycle;
#[cfg(all(feature = "gui", windows))]
mod locale;
#[cfg(all(feature = "gui", windows))]
mod settings;

#[cfg(feature = "gui")]
pub use lifecycle::run;
