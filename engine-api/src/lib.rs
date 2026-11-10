//! Own the shared entry point and use case registration.

pub mod api;
pub mod browse;
mod contracts;
pub mod current_analysis;
mod cursor_mac;
pub mod desktop;
pub mod duplicate_scope;
pub mod error;
pub mod history;
pub mod jobs;
mod pipeline;
mod registry;
pub mod scan;
mod scan_failures;
pub mod scope;
pub mod space;
mod target_enumeration;
mod target_probe;
mod targets;
pub mod viz_catalog;
pub mod viz_pages;
pub mod watch_gap;
mod workers;

#[cfg(test)]
mod tests;
