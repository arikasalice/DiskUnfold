//! Sequential collection from already verified directory handles.
mod issues;
mod parallel;
use du_analysis_index::{
    BuildError, BuildHandle, FileIdentity, IngestSink, Observation, SharingEvidence,
    SpaceObservation,
};
use du_core_contracts::{
    Id, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    job::{Checkpoint, JobContext, JobCounts, JobOutcome},
    result::ResultCompleteness,
    scan_observation::{AnchoredDirectory, MetadataObservation, ObservedEntry, ObservedKind},
};
pub(crate) use parallel::{
    DevicePlan, collect_open_build, collect_open_forest_filtered, collect_open_forest_with_groups,
};
use std::collections::VecDeque;

/// Supplies distinct result identifiers for partial and terminal publications.
pub trait ResultIds: Send + Sync {
    fn next_id(&self) -> Id;
}

/// A frozen decision over a root, parent path and native child name.
/// An undecidable match must be returned as `Undecidable`, never `Exclude`.
pub type PreMetadataFilter<'a> = dyn Fn(u32, &[Vec<u8>], &[u8]) -> du_core_contracts::scan_observation::FilterDecision
    + Send
    + Sync
    + 'a;

/// Collects a verified root using at most `limit` JobContext metadata flows.
///
/// Each worker retains its flow permit through every in-flight OS call. The
/// caller thread alone writes and publishes the result build. The metadata flow
/// limit and `limit` must be nonzero; no unpermitted fallback I/O is performed.
/// A root handle must already be validated and must not be reopened by path.
/// A busy runtime is retried with cancellation checks until the first permit is
/// available. An unsupported flow or a zero worker limit fails without OS calls.
/// Cancellation is checked before each open and enumeration call. `JobContext`
/// does not atomically couple that check to issuing the call, so a cancellation
/// racing between those operations can still allow one call to start.
pub fn collect_parallel(
    sink: &IngestSink,
    scope: Id,
    root: Box<dyn AnchoredDirectory>,
    ids: &dyn ResultIds,
    context: &dyn JobContext,
    limit: usize,
    include: &(dyn Fn(&[Vec<u8>]) -> bool + Sync),
) -> JobOutcome {
    collect_open_build(sink.open(scope), root, ids, context, limit, include)
}

/// Collects one verified root with a predicate enforced before metadata opens.
/// The predicate receives the root index, parent components and native name.
/// It must remain deterministic and immutable for the lifetime of the scan.
/// An unsupported adapter fails the collection instead of falling back.
pub fn collect_parallel_filtered(
    sink: &IngestSink,
    scope: Id,
    root: Box<dyn AnchoredDirectory>,
    ids: &dyn ResultIds,
    context: &dyn JobContext,
    limit: usize,
    include: &PreMetadataFilter<'_>,
) -> JobOutcome {
    collect_open_forest_filtered(
        sink.open(scope),
        vec![root],
        ids,
        context,
        limit,
        include,
        DevicePlan {
            groups: vec![0],
            limit: 4,
        },
    )
}

struct Frame {
    path: Vec<Vec<u8>>,
    dir: Box<dyn AnchoredDirectory>,
    entries: VecDeque<ObservedEntry>,
    recorded_directories: VecDeque<ObservedEntry>,
}

/// Collects one root on the caller's worker thread, with no path-based reopen.
///
/// `root` must have been validated by the adapter. `include` is evaluated before
/// opening a directory. The adapter must bound each returned batch to 8,192 items.
/// This initial collector is sequential and does not implement resource policy.
pub fn collect(
    sink: &IngestSink,
    scope: Id,
    root: Box<dyn AnchoredDirectory>,
    ids: &dyn ResultIds,
    mut checkpoint: impl FnMut() -> Checkpoint,
    mut include: impl FnMut(&[Vec<u8>]) -> bool,
) -> JobOutcome {
    let mut counts = JobCounts::default();
    let result = run(
        sink,
        scope,
        root,
        ids,
        &mut checkpoint,
        &mut include,
        &mut counts,
    );
    match result {
        Ok((id, complete)) => JobOutcome {
            counts,
            completed_scope: complete,
            failure_reason_key: None,
            unprocessed_summary: None,
            result_version_id: Some(id),
        },
        Err(_) => JobOutcome {
            counts,
            completed_scope: false,
            failure_reason_key: Some("support.internal_error".into()),
            unprocessed_summary: None,
            result_version_id: None,
        },
    }
}

fn run(
    sink: &IngestSink,
    scope: Id,
    mut root: Box<dyn AnchoredDirectory>,
    ids: &dyn ResultIds,
    checkpoint: &mut dyn FnMut() -> Checkpoint,
    include: &mut dyn FnMut(&[Vec<u8>]) -> bool,
    counts: &mut JobCounts,
) -> Result<(Id, bool), BuildError> {
    let mut build = sink.open(scope)?;
    build.add_directory(Vec::new())?;
    let first_batch = record_root_batch(&mut build, &mut *root, checkpoint, include, counts)?;
    let cancelled = match first_batch {
        RootBatch::Cancelled => true,
        RootBatch::Exhausted => false,
        RootBatch::Ready(recorded_directories) => {
            build.publish_partial(ids.next_id())?;
            walk(
                &mut build,
                Frame {
                    path: Vec::new(),
                    dir: root,
                    entries: VecDeque::new(),
                    recorded_directories,
                },
                checkpoint,
                include,
                counts,
            )?
        }
    };
    let completeness = if cancelled {
        ResultCompleteness::Cancelled
    } else if counts.failed > 0 {
        ResultCompleteness::Missing
    } else {
        ResultCompleteness::Complete
    };
    let id = ids.next_id();
    build.seal(id.clone(), completeness)?;
    Ok((id, !cancelled))
}

enum RootBatch {
    Cancelled,
    Exhausted,
    Ready(VecDeque<ObservedEntry>),
}

/// Records the first root batch without retaining an open handle for every child.
fn record_root_batch(
    build: &mut BuildHandle,
    root: &mut dyn AnchoredDirectory,
    checkpoint: &mut dyn FnMut() -> Checkpoint,
    include: &mut dyn FnMut(&[Vec<u8>]) -> bool,
    counts: &mut JobCounts,
) -> Result<RootBatch, BuildError> {
    if checkpoint() == Checkpoint::Cancel {
        return Ok(RootBatch::Cancelled);
    }
    if let Some(evidence) = root.observe_directory_case(&|| None) {
        build.set_directory_case_evidence(0, &[], evidence)?;
    }
    let entries = match root.next_batch() {
        Ok(entries) if entries.len() <= 8192 => entries,
        _ => {
            counts.failed += 1;
            return Err(BuildError::Unavailable);
        }
    };
    if entries.is_empty() {
        return Ok(RootBatch::Exhausted);
    }
    let mut directories = VecDeque::new();
    for entry in entries {
        if checkpoint() == Checkpoint::Cancel {
            return Ok(RootBatch::Cancelled);
        }
        let path = [entry.name.clone()];
        if include(&path) && record(build, &entry, &path, counts)? {
            directories.push_back(entry);
        }
    }
    Ok(RootBatch::Ready(directories))
}

/// Returns true when traversal stopped at a cancellation checkpoint.
fn walk(
    build: &mut BuildHandle,
    root: Frame,
    checkpoint: &mut dyn FnMut() -> Checkpoint,
    include: &mut dyn FnMut(&[Vec<u8>]) -> bool,
    counts: &mut JobCounts,
) -> Result<bool, BuildError> {
    let mut stack = vec![root];
    while let Some(frame) = stack.last_mut() {
        if checkpoint() == Checkpoint::Cancel {
            return Ok(true);
        }
        if let Some(entry) = frame.recorded_directories.pop_front() {
            let path = vec![entry.name.clone()];
            if let Some(child) = open_child(build, &*frame.dir, &entry, path, counts, checkpoint)? {
                stack.push(child);
            }
            continue;
        }
        if let Some(entry) = frame.entries.pop_front() {
            let mut path = frame.path.clone();
            path.push(entry.name.clone());
            if !include(&path) {
                continue;
            }
            if record(build, &entry, &path, counts)?
                && let Some(child) =
                    open_child(build, &*frame.dir, &entry, path, counts, checkpoint)?
            {
                stack.push(child);
            }
            continue;
        }
        match frame.dir.next_batch() {
            Ok(batch) if batch.is_empty() => {
                stack.pop();
            }
            Ok(batch) if batch.len() <= 8192 => frame.entries = batch.into(),
            failure => {
                counts.failed += 1;
                let reason = match failure {
                    Err(ref error) => issues::reason(error),
                    _ => "scan.unclassified",
                };
                issues::unread(
                    build,
                    0,
                    frame.path.clone(),
                    du_core_contracts::scan_failures::ScanFailureStage::Enumerate,
                    reason,
                    None,
                )?;
                stack.pop();
            }
        }
    }
    Ok(false)
}

fn record(
    build: &mut BuildHandle,
    entry: &ObservedEntry,
    path: &[Vec<u8>],
    counts: &mut JobCounts,
) -> Result<bool, BuildError> {
    record_at(build, 0, entry, path, counts, None)
}

/// Records only the timestamp supplied with the metadata observation.
fn record_at(
    build: &mut BuildHandle,
    root_index: u32,
    entry: &ObservedEntry,
    path: &[Vec<u8>],
    counts: &mut JobCounts,
    observed_at: Option<&du_core_contracts::Timestamp>,
) -> Result<bool, BuildError> {
    if let Some((kind, stage, reason)) = issues::classify(entry) {
        build.record_scan_issue(du_core_contracts::scan_failures::ScanIssue {
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
            build.add_at(
                root_index,
                Observation {
                    path: path.to_vec(),
                    size: None,
                },
            )?;
            return Ok(false);
        }
    };
    match meta.kind {
        ObservedKind::Directory => build.add_directory_at(root_index, path.to_vec())?,
        ObservedKind::Link => build.add_untraversed_link_at(root_index, path.to_vec())?,
        ObservedKind::File => record_file(build, root_index, meta, path.to_vec(), counts)?,
        ObservedKind::Unknown | ObservedKind::Other => {
            counts.failed += 1;
            build.add_at(
                root_index,
                Observation {
                    path: path.to_vec(),
                    size: None,
                },
            )?;
        }
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
    counts.completed += 1;
    Ok(meta.kind == ObservedKind::Directory)
}

/// Records only facts observed from the file handle retained by the adapter.
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

fn open_child(
    build: &mut BuildHandle,
    parent: &dyn AnchoredDirectory,
    entry: &ObservedEntry,
    path: Vec<Vec<u8>>,
    counts: &mut JobCounts,
    checkpoint: &mut dyn FnMut() -> Checkpoint,
) -> Result<Option<Frame>, BuildError> {
    match parent.open_child(entry) {
        Ok(dir) => {
            if checkpoint() != Checkpoint::Cancel
                && let Some(evidence) = dir.observe_directory_case(&|| None)
            {
                build.set_directory_case_evidence(0, &path, evidence)?;
            }
            Ok(Some(Frame {
                path,
                dir,
                entries: VecDeque::new(),
                recorded_directories: VecDeque::new(),
            }))
        }
        Err(error) => {
            counts.failed += 1;
            issues::unread(
                build,
                0,
                path,
                du_core_contracts::scan_failures::ScanFailureStage::OpenDirectory,
                issues::reason(&error),
                None,
            )?;
            Ok(None)
        }
    }
}
