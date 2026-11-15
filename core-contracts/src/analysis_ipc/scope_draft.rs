//! The caller-selected analysis scope, sent in requests and returned in responses.
use serde::{Deserialize, Serialize};

use super::{ScopeBoundary, ScopeSource};
use crate::OsPath;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
// Kept strict with `ScopeDraft`, which carries it in requests and responses.
#[serde(rename_all = "snake_case", tag = "kind", deny_unknown_fields)]
pub enum ScopeExclusion {
    Path { path: OsPath },
    NamePattern { pattern: String },
}

/// Scope selected by the caller. An OS adapter verifies the raw paths before use.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
// Kept strict: `scope.review` and `scan.start` requests decode it, and `ScopeReview`
// and `RecentScopeOpen` return it. A new field needs a strict request type split off.
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopeDraft {
    pub includes: Vec<OsPath>,
    pub exclusions: Vec<ScopeExclusion>,
    pub boundary: ScopeBoundary,
    pub included_mounts: Vec<OsPath>,
    pub source: ScopeSource,
}
