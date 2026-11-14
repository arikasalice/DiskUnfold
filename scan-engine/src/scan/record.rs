//! Recording shared by the sequential and parallel collectors.
//!
//! FR12.HD1.app-host.DD1 as decided by DEC-006: only a range whose contents
//! could not be confirmed is a scope gap. Both collectors record every entry
//! and every directory they could not open or enumerate through these
//! functions, so one input yields the same failure list, scope gaps, counts
//! and completeness from either collector.
//!
//! NFR5.HD1.app-host.DD1 progress counts every recorded item once: an entry
//! adds one to either `completed` or `failed`, never to both. A failed item
//! is one that leaves a scope gap, so `failed` equals the number of gaps.
use super::{
    admission::{self, Admission, Unread},
    issues,
};
use du_analysis_index::{
    BuildError, BuildHandle, FileIdentity, Observation, SharingEvidence, SpaceObservation,
};
use du_core_contracts::{
    SizeAmount, SizeBasis, SizeEvidence, SizeValue, Timestamp,
    job::JobCounts,
    scan_failures::{ScanFailureKind, ScanFailureStage, ScanIssue},
    scan_observation::{MetadataObservation, ObservedEntry, ObservedKind, TimedObservedEntry},
};

/// Supplies the observation time of a scope gap, evaluated only for a gap.
pub(super) type GapClock<'a> = &'a dyn Fn() -> Option<Timestamp>;

/// How the directory of a failed range was counted before the failure.
///
/// The failure keeps the directory counted once, as failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Counted {
    /// A scope root that was not counted yet; the failure counts it.
    Uncounted,
    /// A directory counted as completed when it was enumerated; the failure
    /// moves that count to failed.
    Completed,
    /// A root already counted as failed, such as an unverifiable root.
    Failed,
}

/// Records one enumerated entry with its admission row.
///
/// An unentered row keeps only its unread range. Any other row records the
/// observation; when the metadata, the kind or the file size could not be
/// established, the entry is also a scope gap at its own path. `row` must
/// not be [`Admission::Excluded`]; an excluded entry is never recorded.
pub(super) fn record_item(
    build: &mut BuildHandle,
    counts: &mut JobCounts,
    gap_at: GapClock<'_>,
    (root_index, path, timed): (u32, Vec<Vec<u8>>, &TimedObservedEntry),
    row: Admission,
) -> Result<(), BuildError> {
    let entry = &timed.entry;
    if let Some(unread) = row.unread() {
        let observed_at = timed.observed_at.as_ref();
        let range = (root_index, path.as_slice());
        return admission::record_unentered(build, range, entry, unread, counts, observed_at);
    }
    let gap = entry_gap(entry);
    record_at(
        build,
        root_index,
        entry,
        &path,
        counts,
        timed.observed_at.as_ref(),
    )?;
    if gap.is_none()
        && entry
            .metadata
            .as_ref()
            .is_ok_and(|meta| meta.kind == ObservedKind::File)
        && let Some(observation) = timed.placeholder_observation
    {
        build.retain_placeholder_observation(root_index, &path, observation)?;
    }
    if gap.is_none()
        && let Some(time) = &timed.observed_at
    {
        build.set_item_observed_at(root_index, &path, time.clone())?;
    }
    if let Some(reason) = gap {
        build.record_gap_at(root_index, path, reason.into(), gap_at())?;
    }
    Ok(())
}

/// Records a directory that could not be opened or completely enumerated.
///
/// Its contents are unknown, so the range is listed as unread at `stage`,
/// counted once as a failure according to `counted` and kept as a scope gap.
/// `observed_at` is the time of the failure and `gap_at` the time recorded
/// with the gap.
pub(super) fn record_failure(
    build: &mut BuildHandle,
    counts: &mut JobCounts,
    (root_index, path): (u32, Vec<Vec<u8>>),
    (stage, reason): (ScanFailureStage, &str),
    (observed_at, gap_at): (Option<Timestamp>, Option<Timestamp>),
    counted: Counted,
) -> Result<(), BuildError> {
    match counted {
        Counted::Uncounted => counts.failed += 1,
        Counted::Completed => {
            counts.completed = counts.completed.saturating_sub(1);
            counts.failed += 1;
        }
        Counted::Failed => {}
    }
    build.record_scan_issue(ScanIssue {
        root_index,
        relative: path.clone(),
        kind: ScanFailureKind::UnreadRange,
        stage,
        reason_key: reason.into(),
        observed_at,
    })?;
    build.record_gap_at(root_index, path, reason.into(), gap_at)
}

/// The gap reason of an entry whose observation is incomplete or inconsistent.
///
/// Every listed failure of a recorded entry is a gap except an untraversed
/// link, whose own observation is complete.
fn entry_gap(entry: &ObservedEntry) -> Option<&'static str> {
    issues::classify(entry)
        .filter(|(kind, _, _)| *kind != ScanFailureKind::UntraversedLink)
        .map(|(_, _, reason)| reason)
}

/// Records only the timestamp supplied with the metadata observation.
fn record_at(
    build: &mut BuildHandle,
    root_index: u32,
    entry: &ObservedEntry,
    path: &[Vec<u8>],
    counts: &mut JobCounts,
    observed_at: Option<&Timestamp>,
) -> Result<(), BuildError> {
    if let Some((kind, stage, reason)) = issues::classify(entry) {
        build.record_scan_issue(ScanIssue {
            root_index,
            relative: path.to_vec(),
            kind,
            stage,
            reason_key: reason.into(),
            observed_at: observed_at.cloned(),
        })?;
    }
    let meta = match &entry.metadata {
        Ok(meta)
            if entry.enumerated_inode != 0
                && entry.enumerated_inode == meta.inode
                && (entry.enumerated_kind == ObservedKind::Unknown
                    || entry.enumerated_kind == meta.kind) =>
        {
            meta
        }
        _ => {
            counts.failed += 1;
            return build.add_at(
                root_index,
                Observation {
                    path: path.to_vec(),
                    size: None,
                },
            );
        }
    };
    match meta.kind {
        ObservedKind::Directory => build.add_directory_at(root_index, path.to_vec())?,
        ObservedKind::Link => build.add_untraversed_link_at(root_index, path.to_vec())?,
        ObservedKind::File => record_file(build, root_index, meta, path.to_vec(), counts)?,
        // Unreachable through the admission table, which leaves every
        // consistent mount point outside the boundary; an inconsistent one
        // fails the check above and is a gap. A boundary is not a failure.
        ObservedKind::MountPoint => {
            let range = (root_index, path);
            let unread = Unread::OtherVolume;
            return admission::record_unentered(build, range, entry, unread, counts, observed_at);
        }
        // An unclassified kind is a listed failure and a gap, counted once.
        ObservedKind::Unknown | ObservedKind::Other => {
            counts.failed += 1;
            return build.add_at(
                root_index,
                Observation {
                    path: path.to_vec(),
                    size: None,
                },
            );
        }
    }
    if meta.kind == ObservedKind::File
        && let Some(inode) = std::num::NonZeroU64::new(entry.enumerated_inode)
    {
        build.retain_enumerated_inode(root_index, path, inode)?;
    }
    if matches!(meta.kind, ObservedKind::File | ObservedKind::Directory)
        && let Some(time) = &meta.modified_at
    {
        build.set_modified_at(root_index, path, time.clone())?;
    }
    if meta.kind == ObservedKind::File
        && let Some(time) = meta.native_modified_at
    {
        build.set_native_modified_at(root_index, path, time)?;
    }
    let sized = meta.kind != ObservedKind::File || meta.size.is_ok();
    if sized {
        counts.completed += 1;
    }
    Ok(())
}

/// Records only facts observed from the file handle retained by the adapter.
///
/// A file whose size could not be read is counted by the caller as failed.
fn record_file(
    build: &mut BuildHandle,
    root_index: u32,
    meta: &MetadataObservation,
    path: Vec<Vec<u8>>,
    counts: &mut JobCounts,
) -> Result<(), BuildError> {
    let size = meta
        .size
        .as_ref()
        .ok()
        .map(|&bytes| os_size(SizeBasis::Logical, bytes));
    if size.is_none() {
        counts.failed += 1;
    }
    let observation = Observation { path, size };
    if meta.allocated.is_none() && meta.space_identity.is_none() && meta.link_count.is_none() {
        return build.add_file_at(root_index, observation);
    }
    build.add_with_space_at(
        root_index,
        observation,
        SpaceObservation {
            allocated: meta
                .allocated
                .map(|bytes| os_size(SizeBasis::Allocated, bytes)),
            identity: meta.space_identity.as_ref().map(|id| FileIdentity {
                volume: id.volume.clone(),
                file: id.file.clone(),
            }),
            link_count: meta.link_count,
            sharing: SharingEvidence::Unknown,
        },
    )
}

fn os_size(basis: SizeBasis, bytes: u64) -> SizeValue {
    SizeValue {
        basis,
        evidence: SizeEvidence::OsReported,
        amount: SizeAmount::Exact {
            bytes: bytes.into(),
        },
    }
}
