//! F01 recent-scope storage schema installed by the GUI migration chain.

/// The table and indexes; the application migration chain owns its number.
pub(crate) const DEFINITION: &str = include_str!("recent_scope_schema.sql");

#[cfg(test)]
mod tests;
