//! Read-only access to raw stored protection rule versions.

/// One owned, unverified stored row. This is neither an integrity check nor
/// authority to make a protection decision. Consumers must validate its format,
/// rule semantics and canonical hash before using it as a policy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RulesetRow {
    /// Monotonically increasing stored rule version.
    pub version: i64,
    /// Stored digest bytes, pending verification against the canonical body.
    pub ruleset_hash: Vec<u8>,
    /// Identifier for the stored digest format.
    pub hash_format: String,
    /// Classification table version referenced by this row.
    pub class_table_version: String,
    /// Known-location table version referenced by this row.
    pub known_location_table_version: String,
    /// Format version of the stored rule body.
    pub rule_format_version: i64,
    /// Stored origin of the version.
    pub origin: String,
    /// Stored change summary JSON.
    pub change_summary: String,
    /// Stored rule body JSON.
    pub body: String,
    /// Creation time in milliseconds since the Unix epoch.
    pub created_at_ms: i64,
}

/// Structural corruption is distinct from inability to read the store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RulesetReadError {
    /// A row or its storage is structurally invalid.
    Corrupt,
    /// The storage operation did not return a usable result.
    StorageUnavailable,
}

/// Each call returns one owned row from one storage snapshot. Later appends do
/// not change it, but the returned current row need not remain current.
pub trait RulesetRead {
    /// Reads the current row, or returns `None` if no version exists.
    fn read_current(&self) -> Result<Option<RulesetRow>, RulesetReadError>;
    /// Reads exactly the requested version, or returns `None` if it is absent.
    fn read_version(&self, version: i64) -> Result<Option<RulesetRow>, RulesetReadError>;
}
