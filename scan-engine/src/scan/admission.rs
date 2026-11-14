//! First-match directory admission for one frozen scope version.
//!
//! FR1.HD1.app-host.DD1 and FR6.HD1.app-host.DD1 order the rows as follows:
//! exclusion rule → excluded, link → untraversed link, other volume → outside
//! boundary, visited → revisit blocked, undecidable boundary → unverifiable,
//! otherwise → enter. The sequential and parallel collectors use only this table.
use du_analysis_index::{BuildError, BuildHandle, Observation};
use du_core_contracts::{
    Timestamp,
    job::JobCounts,
    scan_failures::{ScanFailureKind, ScanFailureStage, ScanIssue},
    scan_observation::{FilterDecision, MetadataObservation, ObservedEntry, ObservedKind},
};
mod visited;
pub(super) use visited::{RootStart, Visited};

/// Reason kept for entries the table did not enter.
///
/// The published failure classification has no kind, stage or reason key for
/// an outside-boundary, revisited or unverifiable entry. Until it does, such an
/// entry stays an unread range with the generic unclassified reason.
const UNENTERED_REASON: &str = "scan.unclassified";

/// The first matching row for one enumerated entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Admission {
    /// An exclusion rule matched; the entry is neither recorded nor opened.
    Excluded,
    /// Not a traversable directory; the reported observation is recorded as is.
    Record,
    /// A link recorded with its own observation and an untraversed marker.
    UntraversedLink,
    /// Observed on another volume than its parent directory.
    OutsideBoundary,
    /// A directory whose volume and file identity was already admitted.
    Revisit,
    /// A directory whose volume boundary cannot be decided.
    Unverifiable,
    /// A same-volume directory seen for the first time; it may be opened.
    Enter,
}

impl Admission {
    /// True for rows that leave the entry as an unread range without entering it.
    pub(super) fn unentered(self) -> bool {
        matches!(
            self,
            Self::OutsideBoundary | Self::Revisit | Self::Unverifiable
        )
    }
}

/// The volume that the children of one directory are compared with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Boundary {
    /// The observed volume identity of the directory.
    Volume(u64),
    /// The identity of the root could not be observed or verified.
    Unknown,
}

impl Boundary {
    /// The boundary of a child directory admitted with [`Admission::Enter`].
    pub(super) fn child(entry: &ObservedEntry) -> Self {
        entry
            .metadata
            .as_ref()
            .map_or(Self::Unknown, |meta| Self::Volume(meta.device))
    }
}

/// The root and parent boundary that one enumerated entry is decided against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Parent {
    /// Index of the scope root whose traversal enumerated the entry.
    pub root: u32,
    /// Volume boundary of the directory that enumerated the entry.
    pub boundary: Boundary,
}

/// Applies the exclusion row before any metadata is observed.
///
/// An undecidable name is withheld from the adapter, so it is never opened,
/// and is kept for an unverifiable record. Other decisions pass through.
pub(super) fn exclusion(
    decision: FilterDecision,
    name: &[u8],
    undecided: &mut Vec<Vec<u8>>,
) -> FilterDecision {
    if decision == FilterDecision::Undecidable {
        undecided.push(name.to_vec());
        return FilterDecision::Exclude;
    }
    decision
}

/// Decides one enumerated entry with the first matching row.
///
/// `excluded` is the frozen exclusion decision for an entry that was not
/// filtered before metadata. Only an `Enter` result reserves the identity.
/// Entries without consistent metadata are recorded and never entered.
pub(super) fn decide(
    excluded: bool,
    entry: &ObservedEntry,
    parent: Parent,
    visited: &Visited,
) -> Admission {
    decide_with(excluded, entry, parent, visited, true)
}

/// Decides like [`decide`] without reserving the identity of an `Enter` row.
///
/// For entries that will not be opened, such as those drained after
/// cancellation, so that no directory is reserved without being entered.
pub(super) fn preview(
    excluded: bool,
    entry: &ObservedEntry,
    parent: Parent,
    visited: &Visited,
) -> Admission {
    decide_with(excluded, entry, parent, visited, false)
}

fn decide_with(
    excluded: bool,
    entry: &ObservedEntry,
    parent: Parent,
    visited: &Visited,
    reserve: bool,
) -> Admission {
    if excluded {
        return Admission::Excluded;
    }
    let Some(meta) = consistent(entry) else {
        return Admission::Record;
    };
    if meta.kind == ObservedKind::Link {
        return Admission::UntraversedLink;
    }
    if let Boundary::Volume(device) = parent.boundary
        && meta.device != device
    {
        return Admission::OutsideBoundary;
    }
    if meta.kind != ObservedKind::Directory {
        return Admission::Record;
    }
    let (root, device, inode) = (parent.root, meta.device, meta.inode);
    let first = match parent.boundary {
        Boundary::Volume(_) if reserve => visited.reserve(root, device, inode),
        _ => !visited.known(root, device, inode),
    };
    match (first, parent.boundary) {
        (false, _) => Admission::Revisit,
        (true, Boundary::Volume(_)) => Admission::Enter,
        (true, Boundary::Unknown) => Admission::Unverifiable,
    }
}

fn consistent(entry: &ObservedEntry) -> Option<&MetadataObservation> {
    entry.metadata.as_ref().ok().filter(|meta| {
        meta.inode != 0
            && meta.inode == entry.enumerated_inode
            && (entry.enumerated_kind == ObservedKind::Unknown
                || entry.enumerated_kind == meta.kind)
            && !matches!(meta.kind, ObservedKind::Unknown | ObservedKind::Other)
    })
}

/// Records an entry that the table did not enter as an unread range.
///
/// No size, time or space facts are kept because they may describe another
/// volume or a range that was not read.
pub(super) fn record_unentered(
    build: &mut BuildHandle,
    root_index: u32,
    entry: &ObservedEntry,
    path: &[Vec<u8>],
    observed_at: Option<&Timestamp>,
) -> Result<(), BuildError> {
    let directory = entry
        .metadata
        .as_ref()
        .is_ok_and(|meta| meta.kind == ObservedKind::Directory);
    if directory {
        build.add_directory_at(root_index, path.to_vec())?;
    } else {
        build.add_at(
            root_index,
            Observation {
                path: path.to_vec(),
                size: None,
            },
        )?;
    }
    record_unread(build, root_index, path.to_vec(), observed_at)
}

/// Records a range that was not read, without any observation of its own.
pub(super) fn record_unread(
    build: &mut BuildHandle,
    root_index: u32,
    path: Vec<Vec<u8>>,
    observed_at: Option<&Timestamp>,
) -> Result<(), BuildError> {
    build.record_scan_issue(ScanIssue {
        root_index,
        relative: path.clone(),
        kind: ScanFailureKind::UnreadRange,
        stage: ScanFailureStage::Traverse,
        reason_key: UNENTERED_REASON.into(),
        observed_at: observed_at.cloned(),
    })?;
    build.record_gap_at(
        root_index,
        path,
        UNENTERED_REASON.into(),
        observed_at.cloned(),
    )
}

/// Records how a root starts, before any of its entries is observed.
///
/// A root without a verified identity is an unread range counted as a
/// failure, because none of its child directories can be entered. A root that
/// repeats an earlier root is a revisit and is never enumerated.
pub(super) fn record_root_start(
    build: &mut BuildHandle,
    root_index: u32,
    start: RootStart,
    counts: &mut JobCounts,
    observed_at: Option<&Timestamp>,
) -> Result<(), BuildError> {
    match start {
        RootStart::Collect(Boundary::Volume(_)) => return Ok(()),
        RootStart::Collect(Boundary::Unknown) => counts.failed += 1,
        RootStart::Revisit => counts.completed += 1,
    }
    record_unread(build, root_index, Vec::new(), observed_at)
}

#[cfg(test)]
mod tests;
