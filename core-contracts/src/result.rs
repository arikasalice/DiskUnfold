//! Read-only, pinned result versions and their conservative space aggregates.
use crate::{Id, PathBytes, PathEncoding, SizeValue, Timestamp};
use std::borrow::Cow;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultItem {
    pub item_no: u32,
    pub parent: Option<u32>,
    pub name: Vec<u8>,
    pub size: Option<SizeValue>,
    /// Kind established by the source, or an explicit unknown for legacy observations.
    pub kind: ResultItemKind,
}

/// The observed kind of a result item; legacy structure records are intentionally ambiguous.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ResultItemKind {
    Directory,
    File,
    UntraversedLink,
    Unknown,
    UnknownStructure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ResultCompleteness {
    Partial,
    Complete,
    Missing,
    Cancelled,
}

/// Space information derived solely from observations in one immutable result version.
///
/// `missing_items` counts observed non-link entries lacking a usable logical value.
/// Untraversed links are counted separately; neither count includes unobserved entries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpaceAggregate {
    pub logical: SizeValue,
    pub allocated: SizeValue,
    pub unique_physical: SizeValue,
    pub completeness: ResultCompleteness,
    /// Completeness of the allocated-size observations and deduplication evidence.
    pub allocated_completeness: ResultCompleteness,
    /// Completeness of the unique-physical-space evidence.
    pub unique_physical_completeness: ResultCompleteness,
    pub missing_items: u64,
    /// Number of observed links whose targets were not traversed in this subtree.
    pub untraversed_links: u64,
    /// Access failures are not supplied by the current ingest contract.
    pub inaccessible_ranges: Option<u64>,
    /// Machine-readable limits on what the observation input can establish.
    pub limitations: Cow<'static, [&'static str]>,
}

/// An operating-system path with its native encoding and no display representation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultNativePath {
    pub encoding: PathEncoding,
    pub raw: PathBytes,
}

/// Root path and identity observed on the same verified handle used for collection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultScopeRoot {
    pub path: ResultNativePath,
    pub volume_id: Vec<u8>,
    pub file_id: Vec<u8>,
}

/// An immutable snapshot held by the reader independently of later publications.
pub trait ResultVersionRead: Send + Sync {
    fn version_id(&self) -> &Id;
    fn scope_version_id(&self) -> &Id;
    fn completeness(&self) -> ResultCompleteness;
    fn items(&self) -> &[ResultItem];

    /// Returns verified root evidence when the source retained it at collection time.
    fn observed_root(&self) -> Option<&ResultScopeRoot> {
        None
    }

    /// UTC when the collector submitted the observations retained by this version.
    fn observed_at(&self) -> Option<&Timestamp> {
        None
    }
}
