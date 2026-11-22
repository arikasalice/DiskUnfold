//! F07 snapshot index schema installed by the GUI migration chain.

/// Snapshot index tables, triggers and indexes for migration eleven.
pub(crate) const DEFINITION: &str = include_str!("snapshot_index_schema.sql");

#[cfg(test)]
mod tests;
