//! Assemble the GUI and own its lifecycle.

#[cfg(feature = "gui")]
mod lifecycle;

#[cfg(feature = "gui")]
pub use lifecycle::run;
