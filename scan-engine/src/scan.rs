//! Sequential collection from already verified directory handles.
use du_analysis_index::{BuildError, BuildHandle, IngestSink, Observation};
use du_core_contracts::{
    Id, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    job::{Checkpoint, JobCounts, JobOutcome},
    result::ResultCompleteness,
    scan_observation::{AnchoredDirectory, ObservedEntry, ObservedKind},
};
use std::collections::VecDeque;

/// Supplies distinct result identifiers for partial and terminal publications.
pub trait ResultIds: Send + Sync {
    fn next_id(&self) -> Id;
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
    build.add_structure(Vec::new())?;
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
            if let Some(child) = open_child(&*frame.dir, &entry, path, counts) {
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
                && let Some(child) = open_child(&*frame.dir, &entry, path, counts)
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
            _ => {
                counts.failed += 1;
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
            build.add(Observation {
                path: path.to_vec(),
                size: None,
            })?;
            return Ok(false);
        }
    };
    if matches!(
        meta.kind,
        ObservedKind::File | ObservedKind::Unknown | ObservedKind::Other
    ) {
        let size = meta.size.as_ref().ok().map(|size| SizeValue {
            basis: SizeBasis::Logical,
            evidence: SizeEvidence::OsReported,
            amount: SizeAmount::Exact {
                bytes: (*size).into(),
            },
        });
        let size = if meta.kind == ObservedKind::File {
            size
        } else {
            None
        };
        if size.is_none() {
            counts.failed += 1;
        }
        build.add(Observation {
            path: path.to_vec(),
            size,
        })?;
    } else {
        build.add_structure(path.to_vec())?;
    }
    counts.completed += 1;
    Ok(meta.kind == ObservedKind::Directory)
}

fn open_child(
    parent: &dyn AnchoredDirectory,
    entry: &ObservedEntry,
    path: Vec<Vec<u8>>,
    counts: &mut JobCounts,
) -> Option<Frame> {
    match parent.open_child(entry) {
        Ok(dir) => Some(Frame {
            path,
            dir,
            entries: VecDeque::new(),
            recorded_directories: VecDeque::new(),
        }),
        Err(_) => {
            counts.failed += 1;
            None
        }
    }
}
