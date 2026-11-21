//! Sequential collection from already verified directory handles.
mod admission;
mod issues;
mod parallel;
mod record;
mod sequential;
use du_analysis_index::IngestSink;
use du_core_contracts::{
    Id,
    job::{Checkpoint, JobContext, JobCounts, JobOutcome},
    scan_observation::AnchoredDirectory,
};
pub(crate) use parallel::{
    DevicePlan, LaneDevice, ScanControlSession, collect_open_build, collect_open_forest_diagnostic,
    collect_open_forest_diagnostic_with_group, collect_open_forest_filtered,
};
pub use parallel::{ScanControlError, ScanControlHandle, ScanPriorityOutcome};

/// Whether a user worker limit fits the greatest permit among the job's lanes.
pub fn valid_resource_limit(limit: Option<u32>, max_permit: u32) -> bool {
    parallel::valid_user_limit(limit, max_permit)
}

/// Supplies distinct result identifiers for partial and terminal publications.
pub trait ResultIds: Send + Sync {
    fn next_id(&self) -> Id;
}

/// A verified identity on device 1 for a test root; it differs between live
/// handles that are not zero-sized and from every small fixture inode.
#[cfg(test)]
pub(crate) fn test_identity<T>(
    handle: &T,
) -> Option<du_core_contracts::scan_observation::HandleIdentity> {
    Some(du_core_contracts::scan_observation::HandleIdentity {
        device: 1,
        inode: (1 << 63) | std::ptr::from_ref(handle).addr() as u64,
        ancestors: Vec::new(),
    })
}

/// A frozen decision over a root, parent path and native child name.
/// An undecidable match must be returned as `Undecidable`, never `Exclude`;
/// the collector then records that name as unread and never observes it.
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
/// racing between those operations can still allow one call to start. A call
/// already waiting inside the adapter is not interrupted by cancellation; it
/// returns only when the adapter or the OS ends it.
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

/// Collects one root on the caller's worker thread, with no path-based reopen.
///
/// `root` must have been validated by the adapter. `include` is evaluated before
/// opening a directory. The adapter must bound each returned batch to 8,192 items.
/// This collector is sequential and does not implement resource policy.
///
/// Failures, scope gaps, counts and completeness have the parallel collector's
/// meaning (FR12.HD1.app-host.DD1): a directory that could not be opened or
/// enumerated, and an entry whose metadata could not be established, is a
/// scope gap. An explicit disconnection closes the directory that reported it;
/// directories already being read are read to their end without opening any
/// more, and each enumerated directory left unread is a gap of its own. A
/// cancellation records the entries already returned without opening any.
/// Each item is counted once, as completed or as failed, the root included
/// (NFR5.HD1.app-host.DD1).
/// A failed first root read leaves no result unless it reports a disconnection.
/// A collection that ends without a result reports its counted items as
/// unprocessed, as described at [`unpublished_counts`].
pub fn collect(
    sink: &IngestSink,
    scope: Id,
    root: Box<dyn AnchoredDirectory>,
    ids: &dyn ResultIds,
    mut checkpoint: impl FnMut() -> Checkpoint,
    mut include: impl FnMut(&[Vec<u8>]) -> bool,
) -> JobOutcome {
    let mut counts = JobCounts::default();
    let result = sequential::run(
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
            // A cancelled sequential collection always leaves its root unfinished.
            unprocessed_summary: (!complete).then(|| "unprocessed_roots=1".into()),
            result_version_id: Some(id),
        },
        Err(_) => internal_failure(counts),
    }
}

/// Moves the items a collection counted as completed to `unprocessed` when
/// it ends without publishing a result (NFR5.HD1.app-host.DD1).
///
/// None of those items is usable, because no result keeps them. `failed`
/// still counts the scope gaps the collection met, and each unit stays in
/// exactly one field. With `completed` at zero the runtime reports the job as
/// failed instead of partial. A cancelled scan publishes a cancelled result
/// and keeps its counts; the sample load releases its versions on any end
/// other than done, cancellation included, and applies this rule then. The
/// terminal `completed + failed` can be lower than the last progress report.
pub(crate) fn unpublished_counts(mut counts: JobCounts) -> JobCounts {
    counts.unprocessed = counts.unprocessed.saturating_add(counts.completed);
    counts.completed = 0;
    counts
}

/// The outcome of a collection that failed internally and published no result.
pub(crate) fn internal_failure(counts: JobCounts) -> JobOutcome {
    JobOutcome {
        counts: unpublished_counts(counts),
        completed_scope: false,
        failure_reason_key: Some("support.internal_error".into()),
        unprocessed_summary: None,
        result_version_id: None,
    }
}
