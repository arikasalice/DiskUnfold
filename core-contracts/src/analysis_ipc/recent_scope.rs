//! GUI command bodies for listing and restoring saved analysis scopes.
use serde::{Deserialize, Serialize};

use crate::{Id, OsPath, Timestamp};

use super::{ScopeDraft, ScopeReview};

/// Empty GUI request for the recent-scope summaries.
#[derive(Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(type = "Record<string, never>"))]
#[serde(deny_unknown_fields)]
pub struct RecentScopeListRequest {}

/// A recent-scope summary containing only the published list fields.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct RecentScopeListRow {
    pub recent_scope_ref: Id,
    pub include_count: u32,
    pub last_used_at: Timestamp,
}

/// Recent-scope summaries ordered by last use and then by reference.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct RecentScopeList {
    pub rows: Vec<RecentScopeListRow>,
}

/// GUI request to restore and recheck a saved scope without starting a scan.
#[derive(Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecentScopeOpenRequest {
    pub recent_scope_ref: Id,
    pub request_seq: u32,
}

/// Current correspondence of one saved target's volume.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum RecentScopeTargetState {
    Matched,
    Moved,
    Disconnected,
    Unverifiable,
}

/// A saved target and any proposed current entry, in saved scope order.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct RecentScopeOpenTarget {
    pub path: OsPath,
    pub state: RecentScopeTargetState,
    pub proposed_entry: Option<OsPath>,
}

/// Restored scope with current correspondence and an optional fresh review.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct RecentScopeOpen {
    pub draft: ScopeDraft,
    pub targets: Vec<RecentScopeOpenTarget>,
    pub review: Option<ScopeReview>,
}
