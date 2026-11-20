//! Bounded handle-based workers in per-storage lanes and a single result publisher.
use super::{
    PreMetadataFilter, ResultIds,
    admission::{self, Admission, Boundary, Visited},
    record,
};
mod control;
mod frame;
mod lanes;
mod queue;
pub(crate) use control::ScanControlSession;
pub use control::{ScanControlError, ScanControlHandle, ScanPriorityOutcome};
use du_analysis_index::{
    BuildError, BuildHandle, SharingEvidence, folder_collection::UnreadReason,
    publication_cadence::PublicationCadence,
};
use du_core_contracts::{
    Id,
    job::{Checkpoint, JobContext, JobCounts, JobFlow, JobFlowError, JobOutcome},
    ports::{PortError, PortErrorKind},
    result::ResultCompleteness,
    scan_observation::{
        AnchoredDirectory, DirectoryCaseEvidence, ObservedEntry, TimedObservedEntry,
    },
};
use frame::Frame;
#[cfg(test)]
use std::collections::VecDeque;
use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, RecvTimeoutError, SyncSender},
    },
    thread,
    time::Duration,
};

mod correlated_budget;
mod disconnect;
mod lane_meter;
mod lane_report;
mod preparation;
mod resource;
mod scheduler;
pub(crate) use resource::valid_user_limit;
mod volume_reports;

/// Directory work held in one lane queue, counting items claimed from it that
/// will be returned to it (NFR3.HD1.app-host.DD1). Work that does not fit
/// stays on the finding worker's stack.
///
/// Measured (release-packaging/quality/f02-queue-capacity-measurement.md):
/// 8 to 1,024 give the same throughput with 1-16 workers, 1 or 4 lanes and
/// read delays up to 1 ms; unbounded was slower. 64 is kept: no value was
/// faster and the shallowest-first share it sets was not measured.
///
/// With `L` lanes, `W` workers, depth `D` and read batch `B` (at most 8,192;
/// adapters return 256), lane queues hold `L * QUEUED_DIRECTORIES` items plus
/// the roots, stacks `W * (D + 2)`, each worker one claimed item and a child.
/// An item keeps its last batch until its next read, so held entries stay
/// within `(L * QUEUED_DIRECTORIES + W * (D + 4) + roots) * B`, 89 % reached
/// with 16 lanes of 8,192 (1.9 GiB). Handles stay within twice the items.
const QUEUED_DIRECTORIES: usize = 64;
const BUFFERED_OBSERVATIONS: usize = 8192;
const MAX_WORKERS: usize = 64;
#[cfg(test)]
static FAIL_SPAWN_AT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(usize::MAX);
#[cfg(test)]
static TEST_WORKER_STARTED: AtomicBool = AtomicBool::new(false);

struct Queue {
    /// Lane queues, worker seats and the optional queued-work peak. The peak
    /// covers all lane queues, including initial roots; worker stacks, active
    /// items and the observation channel are excluded.
    lanes: lanes::Lanes,
    priority: Option<queue::PriorityRegion>,
    outstanding: Option<queue::OutstandingPaths>,
    remaining: usize,
    root_remaining: Vec<usize>,
    started: bool,
}

struct Shared {
    queue: Arc<Mutex<Queue>>,
    wake: Arc<Condvar>,
    abort: AtomicBool,
    disconnects: disconnect::Disconnects,
    track_priority_paths: bool,
    visited: Visited,
    correlated_budget: Arc<correlated_budget::CorrelatedBudget>,
    /// Wait evidence per lane, indexed like `queue.lanes`.
    meters: Arc<lane_meter::LaneMeters>,
}
impl Shared {
    /// Stops all scan flows after an internal failure, without requesting user cancellation.
    fn abort(&self, context: &dyn JobContext) {
        self.abort.store(true, Ordering::Release);
        self.wake.notify_all();
        // The runtime may acquire its own locks; never hold the queue lock here.
        context.abort_flows();
    }
}
enum Event {
    /// Registration is independent of the attempt event sequence.
    FolderAttemptBegin {
        root_index: u32,
        path: Vec<Vec<u8>>,
        attempt_id: u64,
        reply: mpsc::Sender<
            Result<du_analysis_index::FolderAttemptToken, du_analysis_index::FolderAttemptError>,
        >,
        _permit: correlated_budget::Permit,
    },
    /// A result is acknowledged only after the receiving build accepts it.
    FolderAttemptRecord {
        token: du_analysis_index::FolderAttemptToken,
        seq: u64,
        input: Box<du_analysis_index::FolderAttemptInput>,
        reply: Option<mpsc::Sender<Result<(), du_analysis_index::FolderAttemptError>>>,
        _permit: Option<correlated_budget::Permit>,
    },
    /// Acknowledged EOF barrier, sent only after every numbered event was acknowledged.
    FolderAttemptEnd {
        token: du_analysis_index::FolderAttemptToken,
        event_watermark_exclusive: u64,
        child_count_exclusive: u64,
        reply: mpsc::Sender<Result<(), du_analysis_index::FolderAttemptError>>,
    },
    Item(
        u32,
        Vec<Vec<u8>>,
        Box<TimedObservedEntry>,
        Admission,
        Option<correlated_budget::Permit>,
    ),
    /// A correlated observation whose dynamic payload and path are charged to the scan budget.
    CorrelatedItem(
        u32,
        Vec<Vec<u8>>,
        Box<TimedObservedEntry>,
        Admission,
        correlated_budget::Permit,
    ),
    /// A name withheld by an undecidable exclusion rule; it was never observed.
    Undecided(u32, Vec<Vec<u8>>),
    DirectoryCase(u32, Vec<Vec<u8>>, DirectoryCaseEvidence),
    /// Entries an exclusion rule dropped directly below a registered
    /// directory, sent once when its listing ends (`complete`) or stops, and
    /// only for a nonzero count.
    Excluded {
        root_index: u32,
        path: Vec<Vec<u8>>,
        count: u32,
        complete: bool,
    },
    /// A registered directory that cancellation left unopened or partly read.
    /// Sent once per directory, after the event that registered it.
    Unread(u32, Vec<Vec<u8>>),
    Failure {
        root_index: u32,
        path: Vec<Vec<u8>>,
        root: bool,
        reason: &'static str,
        stage: du_core_contracts::scan_failures::ScanFailureStage,
        observed_at: Option<du_core_contracts::Timestamp>,
    },
    RootBatch(
        u32,
        Option<du_core_contracts::Timestamp>,
        Option<du_core_contracts::Timestamp>,
    ),
}
struct Control<'a> {
    ids: &'a dyn ResultIds,
    context: &'a dyn JobContext,
    limit: usize,
    policy: Policy<'a>,
}

struct ScanOptions<'a> {
    devices: DevicePlan,
    diagnostics: Option<&'a crate::scan_job::ScanDiagnostics>,
    scan_control: Option<&'a ScanControlSession>,
    correlated_budget: Option<Arc<correlated_budget::CorrelatedBudget>>,
}

#[derive(Clone, Copy)]
enum Policy<'a> {
    Legacy(&'a (dyn Fn(&[Vec<u8>]) -> bool + Sync)),
    /// Every entry is collected, as with a legacy policy that includes all, and a folder
    /// is enumerated with correlated evidence first where the adapter supports it, so a
    /// scope without exclusion rules still records its folder attempts (F02 DLD IF-19,
    /// #218). An adapter without correlated support keeps its legacy enumeration.
    Evidence,
    BeforeMetadata(&'a PreMetadataFilter<'a>),
}
impl Policy<'_> {
    /// Whether a folder is enumerated with correlated evidence where the adapter supports it.
    fn correlated(self) -> bool {
        !matches!(self, Policy::Legacy(_))
    }
}
struct Progress {
    failed_roots: Vec<bool>,
    root_published: Vec<bool>,
    /// Roots already counted as failed; any other root counts as completed
    /// since its admission, and its first failure moves it to failed.
    counted_roots: record::FailedRanges,
    published: bool,
    cadence: PublicationCadence,
    /// The sharing evidence of each root's volume, for every file below it.
    sharing: Vec<SharingEvidence>,
}
trait MonotonicClock: Sync {
    fn now(&self) -> Option<Duration>;
}
struct ContextClock<'a>(&'a dyn JobContext);
impl MonotonicClock for ContextClock<'_> {
    fn now(&self) -> Option<Duration> {
        self.0.monotonic_now()
    }
}
pub(crate) use queue::DevicePlan;

/// Runs the same collector against a build prepared with source evidence.
pub(crate) fn collect_open_build(
    build: Result<BuildHandle, BuildError>,
    root: Box<dyn AnchoredDirectory>,
    ids: &dyn ResultIds,
    context: &dyn JobContext,
    limit: usize,
    include: &(dyn Fn(&[Vec<u8>]) -> bool + Sync),
) -> JobOutcome {
    collect_open_forest(build, vec![root], ids, context, limit, include)
}

/// Collect all roots into the same already opened forest build.
pub(crate) fn collect_open_forest(
    build: Result<BuildHandle, BuildError>,
    roots: Vec<Box<dyn AnchoredDirectory>>,
    ids: &dyn ResultIds,
    context: &dyn JobContext,
    limit: usize,
    include: &(dyn Fn(&[Vec<u8>]) -> bool + Sync),
) -> JobOutcome {
    let groups = vec![0; roots.len()];
    collect_open_forest_with_groups(
        build,
        roots,
        ids,
        context,
        limit,
        include,
        DevicePlan { groups, limit: 4 },
    )
}

pub(crate) fn collect_open_forest_with_groups(
    build: Result<BuildHandle, BuildError>,
    roots: Vec<Box<dyn AnchoredDirectory>>,
    ids: &dyn ResultIds,
    context: &dyn JobContext,
    limit: usize,
    include: &(dyn Fn(&[Vec<u8>]) -> bool + Sync),
    devices: DevicePlan,
) -> JobOutcome {
    collect_with_policy(
        build,
        roots,
        ids,
        context,
        limit,
        Policy::Legacy(include),
        ScanOptions {
            devices,
            diagnostics: None,
            scan_control: None,
            correlated_budget: None,
        },
    )
}

pub(crate) fn collect_open_forest_filtered(
    build: Result<BuildHandle, BuildError>,
    roots: Vec<Box<dyn AnchoredDirectory>>,
    ids: &dyn ResultIds,
    context: &dyn JobContext,
    limit: usize,
    include: &PreMetadataFilter<'_>,
    devices: DevicePlan,
) -> JobOutcome {
    collect_with_policy(
        build,
        roots,
        ids,
        context,
        limit,
        Policy::BeforeMetadata(include),
        ScanOptions {
            devices,
            diagnostics: None,
            scan_control: None,
            correlated_budget: None,
        },
    )
}

#[cfg(test)]
fn collect_open_forest_filtered_with_budget(
    build: Result<BuildHandle, BuildError>,
    roots: Vec<Box<dyn AnchoredDirectory>>,
    ids: &dyn ResultIds,
    context: &dyn JobContext,
    limit: usize,
    include: &PreMetadataFilter<'_>,
    options: (DevicePlan, Arc<correlated_budget::CorrelatedBudget>),
) -> JobOutcome {
    let (devices, budget) = options;
    collect_with_policy(
        build,
        roots,
        ids,
        context,
        limit,
        Policy::BeforeMetadata(include),
        ScanOptions {
            devices,
            diagnostics: None,
            scan_control: None,
            correlated_budget: Some(budget),
        },
    )
}

pub(crate) fn collect_open_forest_diagnostic(
    build: Result<BuildHandle, BuildError>,
    roots: Vec<Box<dyn AnchoredDirectory>>,
    ids: &dyn ResultIds,
    context: &dyn JobContext,
    limit: usize,
    filter: Option<&PreMetadataFilter<'_>>,
    options: (
        DevicePlan,
        Option<&crate::scan_job::ScanDiagnostics>,
        Option<&ScanControlSession>,
    ),
) -> JobOutcome {
    let policy = filter.map_or(Policy::Evidence, Policy::BeforeMetadata);
    let (devices, diagnostics, scan_control) = options;
    collect_with_policy(
        build,
        roots,
        ids,
        context,
        limit,
        policy,
        ScanOptions {
            devices,
            diagnostics,
            scan_control,
            correlated_budget: None,
        },
    )
}

fn collect_with_policy(
    build: Result<BuildHandle, BuildError>,
    roots: Vec<Box<dyn AnchoredDirectory>>,
    ids: &dyn ResultIds,
    context: &dyn JobContext,
    limit: usize,
    policy: Policy<'_>,
    options: ScanOptions<'_>,
) -> JobOutcome {
    struct FinishControl<'a>(Option<&'a ScanControlSession>);
    impl Drop for FinishControl<'_> {
        fn drop(&mut self) {
            if let Some(control) = self.0 {
                control.finish();
            }
        }
    }
    let _finish_control = FinishControl(options.scan_control);
    let mut counts = JobCounts::default();
    let worker_control = Control {
        ids,
        context,
        limit,
        policy,
    };
    let clock = ContextClock(context);
    let result =
        build.and_then(|build| run(build, roots, worker_control, &mut counts, &clock, options));
    match result {
        Ok((id, cancelled, unprocessed_roots)) => JobOutcome {
            counts,
            completed_scope: !cancelled,
            failure_reason_key: None,
            unprocessed_summary: cancelled
                .then(|| format!("unprocessed_roots={unprocessed_roots}")),
            result_version_id: Some(id),
        },
        Err(_) => super::internal_failure(counts),
    }
}

mod body;
use body::run;

mod publisher;
use publisher::{Services, consume, reason};

mod spawn;
use spawn::{spawn_workers, start_queue};

#[cfg(test)]
mod tests;

mod flows;
use flows::acquire_flows;

mod workers;
use workers::worker;
