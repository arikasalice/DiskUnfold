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
use du_analysis_index::{BuildError, BuildHandle, publication_cadence::PublicationCadence};
use du_core_contracts::{
    Id,
    job::{Checkpoint, JobContext, JobCounts, JobFlow, JobFlowError, JobOutcome},
    ports::{PortError, PortErrorKind},
    result::ResultCompleteness,
    scan_observation::{
        AnchoredDirectory, DirectoryCaseEvidence, ObservedEntry, TimedObservedEntry,
    },
};
#[cfg(test)]
use frame::Directory;
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

mod disconnect;
mod preparation;
mod scheduler;
mod volume_reports;

/// Directory work held in one lane queue, counting items claimed from it that
/// will be returned to it (NFR3.HD1.app-host.DD1).
///
/// Provisional value. The F02 DLD fixes the 8,192-entry stage channel but no
/// lane queue size. 64 matches the largest worker count of one lane, the SSD
/// device permit and [`MAX_WORKERS`], so every worker of a full lane can find
/// shared work without stealing. A queued directory holds at most one returned
/// batch, and a pending child shares its parent's handle, so open handles stay
/// within twice the queued, stacked and running directories. Work that does
/// not fit stays on the finding worker's stack. Confirm the value by
/// measurement before it is published in the design.
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
    Item(u32, Vec<Vec<u8>>, Box<TimedObservedEntry>, Admission),
    /// A name withheld by an undecidable exclusion rule; it was never observed.
    Undecided(u32, Vec<Vec<u8>>),
    DirectoryCase(u32, Vec<Vec<u8>>, DirectoryCaseEvidence),
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
    scan_control: Option<&'a ScanControlSession>,
}

#[derive(Clone, Copy)]
enum Policy<'a> {
    Legacy(&'a (dyn Fn(&[Vec<u8>]) -> bool + Sync)),
    BeforeMetadata(&'a PreMetadataFilter<'a>),
}
struct Progress {
    failed_roots: Vec<bool>,
    root_published: Vec<bool>,
    /// Roots whose item was already counted, so a failure counts them once.
    counted_roots: Vec<bool>,
    published: bool,
    cadence: PublicationCadence,
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
        (devices, None, None),
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
        (devices, None, None),
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
    let include = |_: &[Vec<u8>]| true;
    let policy = filter.map_or(Policy::Legacy(&include), Policy::BeforeMetadata);
    collect_with_policy(build, roots, ids, context, limit, policy, options)
}

fn collect_with_policy(
    build: Result<BuildHandle, BuildError>,
    roots: Vec<Box<dyn AnchoredDirectory>>,
    ids: &dyn ResultIds,
    context: &dyn JobContext,
    limit: usize,
    policy: Policy<'_>,
    options: (
        DevicePlan,
        Option<&crate::scan_job::ScanDiagnostics>,
        Option<&ScanControlSession>,
    ),
) -> JobOutcome {
    struct FinishControl<'a>(Option<&'a ScanControlSession>);
    impl Drop for FinishControl<'_> {
        fn drop(&mut self) {
            if let Some(control) = self.0 {
                control.finish();
            }
        }
    }
    let (devices, diagnostics, scan_control) = options;
    let _finish_control = FinishControl(scan_control);
    let mut counts = JobCounts::default();
    let worker_control = Control {
        ids,
        context,
        limit,
        policy,
        scan_control,
    };
    let clock = ContextClock(context);
    let result = build.and_then(|build| {
        run(
            build,
            roots,
            worker_control,
            &mut counts,
            devices,
            &clock,
            diagnostics,
        )
    });
    match result {
        Ok((id, cancelled, unprocessed_roots)) => JobOutcome {
            counts,
            completed_scope: !cancelled,
            failure_reason_key: None,
            unprocessed_summary: cancelled
                .then(|| format!("unprocessed_roots={unprocessed_roots}")),
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
