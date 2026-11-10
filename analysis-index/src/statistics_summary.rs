//! Read-only summary for one complete subtree of a pinned result version.
//!
//! F03 owns the aggregate and its evidence. This module never constructs a
//! deduplicated total from rows or claims to summarize a filtered selection.
use crate::ResultVersion;
use du_core_contracts::{
    Id, ItemRef,
    result::{
        ResultCompleteness, ResultItemKind, ResultScopeGap, ResultVersionRead, SpaceAggregate,
    },
};

/// Counts of observed descendants, excluding the requested scope directory.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ObservedCounts {
    pub files: u64,
    pub directories: u64,
    pub untraversed_links: u64,
    pub unknown_kind: u64,
}

/// A subtree's original F03 aggregate and source completeness evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub result_version: Id,
    pub scope: ItemRef,
    pub result_completeness: ResultCompleteness,
    /// All source gaps, retained because an ancestor gap can affect this scope.
    pub source_gaps: Vec<ResultScopeGap>,
    pub counts: ObservedCounts,
    /// Original subtree aggregate, including basis-specific completeness and limits.
    pub aggregate: SpaceAggregate,
    /// The current unfiltered source does not supply an exclusion count.
    pub excluded_count: Option<u64>,
}

/// Invalid source reference, arithmetic overflow, or cancellation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    VersionMismatch,
    InvalidScope,
    Overflow,
    Cancelled,
}

/// Summarizes observed descendants of a directory in one already pinned version.
///
/// The F03 subtree aggregate is copied unchanged; it is not a sum of directory
/// and file rows. Counts describe only retained observations. Result completeness
/// and gaps must be considered before treating them as exhaustive. No filtered
/// subset or scope-wide total across multiple roots is implied.
pub fn summarize(
    version: &ResultVersion,
    scope: &ItemRef,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Summary, Error> {
    if cancelled() {
        return Err(Error::Cancelled);
    }
    if scope.result_version != *version.version_id() {
        return Err(Error::VersionMismatch);
    }
    let root = version
        .items()
        .get(scope.item_no as usize)
        .filter(|item| item.kind == ResultItemKind::Directory)
        .ok_or(Error::InvalidScope)?;
    let aggregate = version
        .space(root.item_no)
        .ok_or(Error::InvalidScope)?
        .clone();
    let mut counts = ObservedCounts::default();
    let mut branches = vec![version.children(root.item_no).iter()];
    while let Some(branch) = branches.last_mut() {
        let Some(item) = branch.next() else {
            branches.pop();
            continue;
        };
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let count = match item.kind {
            ResultItemKind::File => &mut counts.files,
            ResultItemKind::Directory => &mut counts.directories,
            ResultItemKind::UntraversedLink => &mut counts.untraversed_links,
            ResultItemKind::Unknown | ResultItemKind::UnknownStructure => &mut counts.unknown_kind,
        };
        *count = count.checked_add(1).ok_or(Error::Overflow)?;
        let children = version.children(item.item_no);
        if !children.is_empty() {
            branches.push(children.iter());
        }
    }
    if cancelled() {
        return Err(Error::Cancelled);
    }
    Ok(Summary {
        result_version: version.version_id().clone(),
        scope: scope.clone(),
        result_completeness: version.completeness(),
        source_gaps: version.scope_gaps().to_vec(),
        counts,
        aggregate,
        excluded_count: None,
    })
}
