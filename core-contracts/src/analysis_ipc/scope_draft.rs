//! The caller-selected analysis scope, sent in requests and returned in responses.
use serde::{Deserialize, Deserializer, Serialize};

use super::{ScopeBoundary, ScopeSource};
use crate::{OsPath, PathBytes, PathEncoding};

/// Decodes a path inside a scope draft and rejects unknown fields.
///
/// `scope.review` and `scan.start` request bodies decode scope drafts, so their
/// paths are strict. The shared `OsPath` stays open because responses and events
/// also carry it. `OsPath` has no invariant across its fields; each field keeps
/// its own validation here, and serde checks that the fields match `OsPath`.
#[derive(Deserialize)]
#[serde(remote = "OsPath", rename_all = "camelCase", deny_unknown_fields)]
struct StrictPath {
    encoding: PathEncoding,
    raw: PathBytes,
    display: String,
    lossy: bool,
}

/// Applies `StrictPath` to every element of a path list.
fn strict_paths<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<OsPath>, D::Error> {
    #[derive(Deserialize)]
    #[serde(transparent)]
    struct Element(#[serde(with = "StrictPath")] OsPath);
    let elements = Vec::<Element>::deserialize(deserializer)?;
    Ok(elements.into_iter().map(|element| element.0).collect())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
// Kept strict with `ScopeDraft`, which carries it in requests and responses. The
// path is strict as well; see `StrictPath`.
#[serde(rename_all = "snake_case", tag = "kind", deny_unknown_fields)]
pub enum ScopeExclusion {
    Path {
        #[serde(deserialize_with = "StrictPath::deserialize")]
        path: OsPath,
    },
    NamePattern {
        pattern: String,
    },
}

/// Scope selected by the caller. An OS adapter verifies the raw paths before use.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
// Kept strict: `scope.review` and `scan.start` requests decode it, and `ScopeReview`
// and `RecentScopeOpen` return it. A new field needs a strict request type split off.
// Its paths are strict as well; see `StrictPath`.
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopeDraft {
    #[serde(deserialize_with = "strict_paths")]
    pub includes: Vec<OsPath>,
    pub exclusions: Vec<ScopeExclusion>,
    pub boundary: ScopeBoundary,
    #[serde(deserialize_with = "strict_paths")]
    pub included_mounts: Vec<OsPath>,
    pub source: ScopeSource,
}
