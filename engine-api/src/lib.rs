//! Own the shared entry point and use case registration.

pub mod api;
mod contracts;
pub mod desktop;
pub mod error;
pub mod jobs;
mod pipeline;
mod registry;
pub mod scan;
pub mod scope;
pub mod space;
mod target_probe;
mod targets;
mod workers;

#[cfg(test)]
mod tests;
