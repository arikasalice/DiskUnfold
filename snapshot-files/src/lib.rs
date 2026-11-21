//! Stages and durably registers immutable snapshot files.

mod file_store;
mod format_registry;

pub use file_store::SnapshotFileStore;
pub use format_registry::FormatRegistry;
