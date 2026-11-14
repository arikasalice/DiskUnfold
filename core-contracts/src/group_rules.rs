//! F36 virtual-group rule storage port. analysis-index owns the meaning of the
//! stored values; storage adapters persist them without interpreting conditions.
use crate::Id;

/// One stored rule row, in display order within its ruleset version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredGroupRule {
    /// Stable group identity assigned when the rule was first created.
    pub group_id: Id,
    pub display_name: String,
    pub display_order: u32,
    /// Canonical `QueryCondition` JSON.
    pub condition_json: String,
    pub condition_model_version: u32,
    /// JSON array of `{ "id": string, "revision": number }` predicate strategies.
    pub predicates_json: String,
}

/// The single stored ruleset with its version, write revision and fingerprint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredGroupRuleset {
    pub ruleset_id: Id,
    pub format_version: u32,
    /// Increases by one for each stored content change.
    pub version_no: u32,
    /// Optimistic write revision compared by `GroupRulesStore::replace`.
    pub revision: u32,
    /// BLAKE3 digest of the canonical rule content.
    pub fingerprint: [u8; 32],
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub rules: Vec<StoredGroupRule>,
}

/// Outcome of one whole-ruleset replacement transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupRulesSaveOutcome {
    /// The replacement committed.
    Saved,
    /// The stored revision differs from the expected revision; nothing was written.
    Conflict { current_revision: u32 },
    /// The database stayed locked beyond the busy timeout; nothing was written.
    Busy,
    /// Another storage failure; the transaction was rolled back.
    Failed,
}

/// Failure to read the stored ruleset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupRulesReadError {
    Busy,
    /// Stored rows are structurally invalid.
    Corrupt,
    Unavailable,
}

/// Storage port for the profile's single group ruleset.
///
/// `read` returns the header and rows from one snapshot, or `None` when no
/// ruleset was ever stored (an empty ruleset at revision zero). `replace`
/// commits `next` only if the stored revision equals `expected_revision`;
/// zero means that no ruleset exists yet. Callers compute versions and the
/// fingerprint; the adapter checks only the revision.
pub trait GroupRulesStore: Send + Sync {
    fn read(&self) -> Result<Option<StoredGroupRuleset>, GroupRulesReadError>;
    fn replace(&self, expected_revision: u32, next: &StoredGroupRuleset) -> GroupRulesSaveOutcome;
}
