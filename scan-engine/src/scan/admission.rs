//! First-match directory admission for one frozen scope version.
//!
//! FR1.HD1.app-host.DD1 and FR6.HD1.app-host.DD1 order the rows as follows:
//! exclusion rule → excluded, link → untraversed link, other volume or mount
//! point → outside boundary, visited → revisit blocked, undecidable boundary →
//! unverifiable, otherwise → enter. The sequential and parallel collectors use
//! only this table.
//!
//! A directory on another volume than the known parent's is outside the
//! boundary even when its enumerated inode differs from the observed one: btrfs
//! lists a subvolume with its subvolume ID while the subvolume root reports
//! inode 256.
use du_analysis_index::{BuildError, BuildHandle, Observation};
use du_core_contracts::{
    Timestamp,
    job::JobCounts,
    scan_failures::{ScanFailureKind, ScanFailureStage, ScanIssue, unentered_reason},
    scan_observation::{FilterDecision, MetadataObservation, ObservedEntry, ObservedKind},
};
mod visited;
pub(super) use visited::{RootStart, Visited};

/// The first matching row for one enumerated entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Admission {
    /// An exclusion rule matched; the entry is neither recorded nor opened.
    Excluded,
    /// Not a traversable directory; the reported observation is recorded as is.
    Record,
    /// A link recorded with its own observation and an untraversed marker.
    UntraversedLink,
    /// Observed on another volume than its parent directory, or a mount point.
    OutsideBoundary,
    /// A directory whose volume and file identity was already admitted.
    Revisit,
    /// A directory whose volume boundary cannot be decided.
    Unverifiable,
    /// A same-volume directory seen for the first time; it may be opened.
    Enter,
}

impl Admission {
    /// The unread range of a row that leaves the entry without entering it.
    pub(super) fn unread(self) -> Option<Unread> {
        match self {
            Self::OutsideBoundary => Some(Unread::OtherVolume),
            Self::Revisit => Some(Unread::Revisit),
            Self::Unverifiable => Some(Unread::Unverifiable),
            _ => None,
        }
    }
}

/// Why a range was left unread, and whether that lowers completeness.
///
/// Every range is listed as an unread range at the traverse stage. Only an
/// unverifiable range is a scope gap and a failure; another volume is a
/// boundary of the scope version and a revisit was already collected once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Unread {
    /// Another volume, including a mount point.
    OtherVolume,
    /// A directory already reserved by another path or an earlier root.
    Revisit,
    /// The boundary, the exclusion rule or the identity could not be decided.
    Unverifiable,
}

impl Unread {
    fn reason_key(self) -> &'static str {
        match self {
            Self::OtherVolume => unentered_reason::BOUNDARY_OTHER_VOLUME,
            Self::Revisit => unentered_reason::REVISIT_BLOCKED,
            Self::Unverifiable => unentered_reason::BOUNDARY_UNVERIFIABLE,
        }
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
    let Some(meta) = consistent(entry).or_else(|| other_volume_directory(entry, parent.boundary))
    else {
        return Admission::Record;
    };
    if meta.kind == ObservedKind::Link {
        return Admission::UntraversedLink;
    }
    // The adapter established another volume even when the parent's is unknown.
    if matches!(
        meta.kind,
        ObservedKind::MountPoint | ObservedKind::MountedFile
    ) {
        return Admission::OutsideBoundary;
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
    entry.metadata.as_ref().ok().filter(|meta| match meta.kind {
        // The mounted root's identity differs from the covered directory's,
        // and only a directory can be a mount point.
        ObservedKind::MountPoint => matches!(
            entry.enumerated_kind,
            ObservedKind::Unknown | ObservedKind::Directory | ObservedKind::MountPoint
        ),
        // The mounted file's identity differs from the covered file's.
        ObservedKind::MountedFile => matches!(
            entry.enumerated_kind,
            ObservedKind::Unknown | ObservedKind::File | ObservedKind::MountedFile
        ),
        ObservedKind::Unknown | ObservedKind::Other => false,
        _ => {
            meta.inode != 0
                && meta.inode == entry.enumerated_inode
                && (entry.enumerated_kind == ObservedKind::Unknown
                    || entry.enumerated_kind == meta.kind)
        }
    })
}

/// The metadata of a directory observed on another volume than the known
/// parent's, without comparing the enumerated inode.
///
/// The kinds must still agree, so a file or link replaced by a directory stays
/// a failure, and a parent whose volume is unknown keeps the comparison. The
/// device is compared only within this scan and never stored as a volume
/// identity, because btrfs assigns subvolume devices anonymously.
fn other_volume_directory(
    entry: &ObservedEntry,
    boundary: Boundary,
) -> Option<&MetadataObservation> {
    let Boundary::Volume(device) = boundary else {
        return None;
    };
    entry.metadata.as_ref().ok().filter(|meta| {
        meta.kind == ObservedKind::Directory
            && meta.device != device
            && matches!(
                entry.enumerated_kind,
                ObservedKind::Directory | ObservedKind::Unknown
            )
    })
}

/// Records an entry that the table did not enter, with its unread range.
///
/// No size, time or space facts are kept because they may describe another
/// volume or a range that was not read. A mount point stays a directory item
/// and a file on another volume, mounted or not, is a boundary file: a file
/// item without a size that adds neither bytes nor a missing item to its
/// folder.
pub(super) fn record_unentered(
    build: &mut BuildHandle,
    (root_index, path): (u32, &[Vec<u8>]),
    entry: &ObservedEntry,
    unread: Unread,
    counts: &mut JobCounts,
    observed_at: Option<&Timestamp>,
) -> Result<(), BuildError> {
    match entry.metadata.as_ref().map(|meta| &meta.kind) {
        Ok(ObservedKind::Directory | ObservedKind::MountPoint) => {
            build.add_directory_at(root_index, path.to_vec())?;
        }
        Ok(ObservedKind::MountedFile) => build.add_boundary_file_at(root_index, path.to_vec())?,
        Ok(ObservedKind::File) if unread == Unread::OtherVolume => {
            build.add_boundary_file_at(root_index, path.to_vec())?;
        }
        _ => build.add_at(
            root_index,
            Observation {
                path: path.to_vec(),
                size: None,
            },
        )?,
    }
    record_unread(
        build,
        (root_index, path.to_vec()),
        unread,
        counts,
        observed_at,
    )
}

/// Records a range that was not read, without any observation of its own.
///
/// Every range is listed. Only an unverifiable range is also a scope gap and
/// a failure; other ranges count as completed (FR12.HD1.app-host.DD1).
pub(super) fn record_unread(
    build: &mut BuildHandle,
    (root_index, path): (u32, Vec<Vec<u8>>),
    unread: Unread,
    counts: &mut JobCounts,
    observed_at: Option<&Timestamp>,
) -> Result<(), BuildError> {
    let reason_key = unread.reason_key();
    build.record_scan_issue(ScanIssue {
        root_index,
        relative: path.clone(),
        kind: ScanFailureKind::UnreadRange,
        stage: ScanFailureStage::Traverse,
        reason_key: reason_key.into(),
        observed_at: observed_at.cloned(),
    })?;
    if unread != Unread::Unverifiable {
        counts.completed += 1;
        return Ok(());
    }
    counts.failed += 1;
    build.record_gap_at(root_index, path, reason_key.into(), observed_at.cloned())
}

/// Records how a root starts and counts its item once, before any of its
/// entries is observed.
///
/// A root without a verified identity is an unverifiable range, because none
/// of its child directories can be entered, and counts as failed. A root that
/// repeats an earlier root is a revisit, is never enumerated and counts as
/// completed. A verified root counts as completed; a later failure of its own
/// read moves it to failed (NFR5.HD1.app-host.DD1).
pub(super) fn record_root_start(
    build: &mut BuildHandle,
    root_index: u32,
    start: RootStart,
    counts: &mut JobCounts,
    observed_at: Option<&Timestamp>,
) -> Result<(), BuildError> {
    let unread = match start {
        RootStart::Collect(Boundary::Volume(_)) => {
            counts.completed += 1;
            return Ok(());
        }
        RootStart::Collect(Boundary::Unknown) => Unread::Unverifiable,
        RootStart::Revisit => Unread::Revisit,
    };
    record_unread(build, (root_index, Vec::new()), unread, counts, observed_at)
}

#[cfg(test)]
mod tests;
