//! Bounded handle-based workers and a single result publisher.
use super::{PreMetadataFilter, ResultIds, record_at};
use du_analysis_index::{BuildError, BuildHandle, publication_cadence::PublicationCadence};
use du_core_contracts::{
    Id,
    job::{Checkpoint, JobContext, JobCounts, JobFlow, JobFlowError, JobOutcome},
    ports::{PortError, PortErrorKind},
    result::ResultCompleteness,
    scan_observation::{
        AnchoredDirectory, DirectoryCaseEvidence, ObservedEntry, ObservedKind, TimedObservedEntry,
    },
};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, RecvTimeoutError, SyncSender},
    },
    thread,
    time::Duration,
};

mod disconnect;
mod volume_reports;

const QUEUED_DIRECTORIES: usize = 64;
const BUFFERED_OBSERVATIONS: usize = 8192;
const MAX_WORKERS: usize = 64;
#[cfg(test)]
static FAIL_SPAWN_AT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(usize::MAX);
#[cfg(test)]
static TEST_WORKER_STARTED: AtomicBool = AtomicBool::new(false);

struct Frame {
    root_index: u32,
    path: Vec<Vec<u8>>,
    directory: Box<dyn AnchoredDirectory>,
    entries: VecDeque<TimedObservedEntry>,
    deferred: VecDeque<TimedObservedEntry>,
    root: bool,
    first_batch: bool,
    root_published: bool,
    batch_pending: bool,
    case_observed: bool,
}
struct Queue {
    ready: VecDeque<Frame>,
    last_root: Option<u32>,
    remaining: usize,
    root_remaining: Vec<usize>,
    root_groups: Vec<usize>,
    active_groups: Vec<usize>,
    device_limit: usize,
    started: bool,
}
impl Queue {
    fn pop_fair(&mut self) -> Option<(Frame, usize)> {
        let last = self.last_root;
        let index = self
            .ready
            .iter()
            .enumerate()
            .filter(|(_, frame)| {
                self.active_groups[self.root_groups[frame.root_index as usize]] < self.device_limit
            })
            .min_by_key(|(_, frame)| {
                (
                    last.is_some_and(|value| frame.root_index <= value),
                    frame.root_index,
                )
            })
            .map(|(index, _)| index)?;
        let frame = self.ready.remove(index)?;
        self.last_root = Some(frame.root_index);
        let group = self.root_groups[frame.root_index as usize];
        self.active_groups[group] += 1;
        Some((frame, group))
    }
}
struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
    abort: AtomicBool,
    disconnects: disconnect::Disconnects,
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
    Item(u32, Vec<Vec<u8>>, Box<TimedObservedEntry>),
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
}

#[derive(Clone, Copy)]
enum Policy<'a> {
    Legacy(&'a (dyn Fn(&[Vec<u8>]) -> bool + Sync)),
    BeforeMetadata(&'a PreMetadataFilter<'a>),
}
struct Progress {
    failed_roots: Vec<bool>,
    root_published: Vec<bool>,
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
pub(crate) struct DevicePlan {
    pub groups: Vec<usize>,
    pub limit: usize,
}
impl DevicePlan {
    fn valid(&self, count: usize) -> bool {
        count != 0
            && u32::try_from(count).is_ok()
            && self.groups.len() == count
            && self.limit != 0
            && self.groups.iter().all(|&group| group < count)
    }
}

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
        devices,
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
        devices,
    )
}

fn collect_with_policy(
    build: Result<BuildHandle, BuildError>,
    roots: Vec<Box<dyn AnchoredDirectory>>,
    ids: &dyn ResultIds,
    context: &dyn JobContext,
    limit: usize,
    policy: Policy<'_>,
    devices: DevicePlan,
) -> JobOutcome {
    let mut counts = JobCounts::default();
    let control = Control {
        ids,
        context,
        limit,
        policy,
    };
    let clock = ContextClock(context);
    let result = build.and_then(|build| run(build, roots, control, &mut counts, devices, &clock));
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

fn run(
    mut build: BuildHandle,
    roots: Vec<Box<dyn AnchoredDirectory>>,
    control: Control<'_>,
    counts: &mut JobCounts,
    devices: DevicePlan,
    clock: &dyn MonotonicClock,
) -> Result<(Id, bool, usize), BuildError> {
    let Control {
        ids,
        context,
        limit,
        policy,
    } = control;
    if !devices.valid(roots.len()) {
        return Err(BuildError::Unavailable);
    }
    let root_count = roots.len();
    let volumes = build.root_volumes();
    let disconnects = disconnect::Disconnects::new(root_count, &volumes);
    let mut reports = volume_reports::Reports::new(volumes);
    build.set_volume_reports(reports.snapshot())?;
    for index in 0..root_count {
        build.add_directory_at(index as u32, Vec::new())?;
    }
    if context.checkpoint() == Checkpoint::Cancel {
        let id = ids.next_id();
        build.seal(id.clone(), ResultCompleteness::Cancelled)?;
        return Ok((id, true, root_count));
    }
    match reports.prepare_start(&roots, context, &disconnects) {
        Ok(()) => {}
        Err(JobFlowError::Cancelled) => {
            let id = ids.next_id();
            build.seal(id.clone(), ResultCompleteness::Cancelled)?;
            return Ok((id, true, root_count));
        }
        Err(_) => return Err(BuildError::Unavailable),
    }
    build.set_volume_reports(reports.snapshot())?;
    let flows = match acquire_flows(context, limit) {
        Ok(flows) => flows,
        Err(JobFlowError::Cancelled) => {
            let id = ids.next_id();
            build.seal(id.clone(), ResultCompleteness::Cancelled)?;
            return Ok((id, true, root_count));
        }
        Err(_) => return Err(BuildError::Unavailable),
    };
    let shared = Shared {
        queue: Mutex::new(Queue {
            ready: roots
                .into_iter()
                .enumerate()
                .map(|(index, root)| Frame {
                    root_index: index as u32,
                    path: Vec::new(),
                    directory: root,
                    entries: VecDeque::new(),
                    deferred: VecDeque::new(),
                    root: true,
                    first_batch: false,
                    root_published: false,
                    batch_pending: false,
                    case_observed: false,
                })
                .collect(),
            remaining: root_count,
            root_remaining: vec![1; root_count],
            root_groups: devices.groups,
            active_groups: vec![0; root_count],
            device_limit: devices.limit,
            last_root: None,
            started: false,
        }),
        wake: Condvar::new(),
        abort: AtomicBool::new(false),
        disconnects,
    };
    let (sender, receiver) = mpsc::sync_channel(BUFFERED_OBSERVATIONS);
    let mut progress = Progress {
        failed_roots: vec![false; root_count],
        root_published: vec![false; root_count],
        published: false,
        cadence: PublicationCadence::default(),
    };
    let mut failure = None;
    thread::scope(|scope| {
        let spawned = spawn_workers(scope, flows, &shared, &sender, context, policy)
            .and_then(|()| start_queue(&shared));
        drop(sender);
        if let Err(error) = spawned {
            failure = Some(error);
            shared.abort(context);
            drop(receiver);
            return;
        }
        let services = Services {
            context,
            ids,
            clock,
            abort: &shared.abort,
        };
        if let Err(error) = consume(receiver, &mut progress, &mut build, counts, services) {
            failure = Some(error);
            shared.abort(context);
        }
    });
    if let Some(error) = failure {
        return Err(error);
    }
    if shared.abort.load(Ordering::Acquire) {
        return Err(BuildError::Unavailable);
    }
    let was_cancelled = context.checkpoint() == Checkpoint::Cancel;
    if !was_cancelled && !progress.has_usable_result(&shared.disconnects) {
        return Err(BuildError::Unavailable);
    }
    reports.finish(context, was_cancelled, &shared.disconnects);
    let cancelled = was_cancelled || context.checkpoint() == Checkpoint::Cancel;
    let unprocessed_roots = if cancelled {
        shared
            .queue
            .lock()
            .map_err(|_| BuildError::Unavailable)?
            .root_remaining
            .iter()
            .filter(|&&count| count != 0)
            .count()
    } else {
        0
    };
    let completeness = if cancelled {
        ResultCompleteness::Cancelled
    } else if counts.failed > 0 {
        ResultCompleteness::Missing
    } else {
        ResultCompleteness::Complete
    };
    build.set_volume_reports(reports.snapshot())?;
    let id = ids.next_id();
    build.seal_at(id.clone(), completeness, context.utc_now())?;
    Ok((id, cancelled, unprocessed_roots))
}

mod publisher;
use publisher::{Services, consume, reason};

/// Releases the initial directory only after all workers were successfully started.
fn start_queue(shared: &Shared) -> Result<(), BuildError> {
    let mut queue = shared.queue.lock().map_err(|_| BuildError::Unavailable)?;
    queue.started = true;
    shared.wake.notify_all();
    Ok(())
}

fn spawn_workers<'scope, 'env>(
    scope: &'scope thread::Scope<'scope, 'env>,
    flows: Vec<Box<dyn JobFlow>>,
    shared: &'scope Shared,
    sender: &SyncSender<Event>,
    context: &'scope dyn JobContext,
    policy: Policy<'scope>,
) -> Result<(), BuildError> {
    let mut flows = flows.into_iter();
    #[cfg(test)]
    let mut index = 0;
    while let Some(flow) = flows.next() {
        let sender = sender.clone();
        let slot = Arc::new(Mutex::new(Some(flow)));
        let worker_slot = Arc::clone(&slot);
        #[cfg(test)]
        let fail = FAIL_SPAWN_AT.load(Ordering::Acquire) == index;
        #[cfg(not(test))]
        let fail = false;
        #[cfg(test)]
        if fail && index > 0 {
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while !TEST_WORKER_STARTED.load(Ordering::Acquire)
                && std::time::Instant::now() < deadline
            {
                thread::yield_now();
            }
        }
        let spawned = if fail {
            Err(std::io::Error::other("injected spawn failure"))
        } else {
            thread::Builder::new().spawn_scoped(scope, move || {
                #[cfg(test)]
                TEST_WORKER_STARTED.store(true, Ordering::Release);
                let flow = worker_slot.lock().ok().and_then(|mut slot| slot.take());
                if let Some(flow) = flow {
                    worker(shared, sender, context, policy, flow);
                } else {
                    shared.abort(context);
                }
            })
        };
        #[cfg(test)]
        {
            index += 1;
        }
        if spawned.is_err() {
            if let Ok(mut slot) = slot.lock()
                && let Some(flow) = slot.take()
            {
                flow.finish(JobCounts::default());
            }
            for flow in flows {
                flow.finish(JobCounts::default());
            }
            shared.abort(context);
            return Err(BuildError::Unavailable);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

mod flows;
use flows::acquire_flows;

mod workers;
use workers::worker;
