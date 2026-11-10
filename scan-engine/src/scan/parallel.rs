//! Bounded handle-based workers and a single result publisher.
use super::{ResultIds, record_at};
use du_analysis_index::{BuildError, BuildHandle};
use du_core_contracts::{
    Id,
    job::{Checkpoint, JobContext, JobCounts, JobFlow, JobFlowError, JobOutcome},
    ports::{PortError, PortErrorKind},
    result::ResultCompleteness,
    scan_observation::{AnchoredDirectory, ObservedEntry, ObservedKind},
};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    thread,
    time::Duration,
};

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
    entries: VecDeque<ObservedEntry>,
    deferred: VecDeque<ObservedEntry>,
    root: bool,
    first_batch: bool,
    root_published: bool,
    batch_pending: bool,
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
}
enum Event {
    Item(u32, Vec<Vec<u8>>, Box<ObservedEntry>),
    Failure {
        root_index: u32,
        path: Vec<Vec<u8>>,
        root: bool,
        reason: &'static str,
    },
    RootBatch(u32),
}
struct Control<'a> {
    ids: &'a dyn ResultIds,
    context: &'a dyn JobContext,
    limit: usize,
    include: &'a (dyn Fn(&[Vec<u8>]) -> bool + Sync),
}
struct Progress {
    failed_roots: Vec<bool>,
    root_published: Vec<bool>,
    published: bool,
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
    let mut counts = JobCounts::default();
    let control = Control {
        ids,
        context,
        limit,
        include,
    };
    let result = build.and_then(|build| run(build, roots, control, &mut counts, devices));
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
) -> Result<(Id, bool, usize), BuildError> {
    let Control {
        ids,
        context,
        limit,
        include,
    } = control;
    if !devices.valid(roots.len()) {
        return Err(BuildError::Unavailable);
    }
    let root_count = roots.len();
    for index in 0..root_count {
        build.add_directory_at(index as u32, Vec::new())?;
    }
    if context.checkpoint() == Checkpoint::Cancel {
        let id = ids.next_id();
        build.seal(id.clone(), ResultCompleteness::Cancelled)?;
        return Ok((id, true, root_count));
    }
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
    };
    let (sender, receiver) = mpsc::sync_channel(BUFFERED_OBSERVATIONS);
    let mut progress = Progress {
        failed_roots: vec![false; root_count],
        root_published: vec![false; root_count],
        published: false,
    };
    let mut failure = None;
    thread::scope(|scope| {
        let spawned = spawn_workers(scope, flows, &shared, &sender, context, include)
            .and_then(|()| start_queue(&shared));
        drop(sender);
        if let Err(error) = spawned {
            failure = Some(error);
            shared.abort.store(true, Ordering::Release);
            shared.wake.notify_all();
            drop(receiver);
            return;
        }
        while let Ok(event) = receiver.recv() {
            let result = progress.record(event, &mut build, counts, context, ids);
            if let Err(error) = result {
                failure = Some(error);
                shared.abort.store(true, Ordering::Release);
                shared.wake.notify_all();
                break;
            }
        }
        drop(receiver);
    });
    if let Some(error) = failure {
        return Err(error);
    }
    if shared.abort.load(Ordering::Acquire) {
        return Err(BuildError::Unavailable);
    }
    let cancelled = context.checkpoint() == Checkpoint::Cancel;
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
    if progress.failed_roots.iter().all(|failed| *failed) && !cancelled {
        return Err(BuildError::Unavailable);
    }
    let completeness = if cancelled {
        ResultCompleteness::Cancelled
    } else if counts.failed > 0 {
        ResultCompleteness::Missing
    } else {
        ResultCompleteness::Complete
    };
    let id = ids.next_id();
    build.seal_at(id.clone(), completeness, context.utc_now())?;
    Ok((id, cancelled, unprocessed_roots))
}

impl Progress {
    fn record(
        &mut self,
        event: Event,
        build: &mut BuildHandle,
        counts: &mut JobCounts,
        context: &dyn JobContext,
        ids: &dyn ResultIds,
    ) -> Result<(), BuildError> {
        match event {
            Event::Item(root_index, path, entry) => {
                let gap = entry_gap(&entry);
                record_at(build, root_index, &entry, &path, counts)?;
                if let Some(reason) = gap {
                    build.record_gap_at(root_index, path, reason.into(), context.utc_now())?;
                }
                Ok(())
            }
            Event::Failure {
                root_index,
                path,
                root,
                reason,
            } => {
                counts.failed += 1;
                if root && !self.root_published[root_index as usize] {
                    self.failed_roots[root_index as usize] = true;
                }
                build.record_gap_at(root_index, path, reason.into(), context.utc_now())
            }
            Event::RootBatch(root_index) => {
                self.root_published[root_index as usize] = true;
                if !self.published {
                    self.published = true;
                    build.publish_partial_at(ids.next_id(), context.utc_now())?;
                }
                Ok(())
            }
        }
    }
}

fn reason(error: &PortError) -> &'static str {
    match error.kind {
        PortErrorKind::PermissionDenied => "scan.path_denied",
        PortErrorKind::NotFound => "scan.path_vanished",
        _ => "scan.unclassified",
    }
}

fn entry_gap(entry: &ObservedEntry) -> Option<&'static str> {
    match &entry.metadata {
        Err(error) => Some(reason(error)),
        Ok(meta)
            if entry.enumerated_inode == 0
                || entry.enumerated_inode != meta.inode
                || (entry.enumerated_kind != ObservedKind::Unknown
                    && entry.enumerated_kind != meta.kind) =>
        {
            Some("scan.unclassified")
        }
        Ok(meta) if matches!(meta.kind, ObservedKind::Unknown | ObservedKind::Other) => {
            Some("scan.unclassified")
        }
        Ok(meta) if meta.kind == ObservedKind::File => meta.size.as_ref().err().map(reason),
        _ => None,
    }
}

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
    include: &'scope (dyn Fn(&[Vec<u8>]) -> bool + Sync),
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
                    worker(shared, sender, context, include, flow);
                } else {
                    shared.abort.store(true, Ordering::Release);
                    shared.wake.notify_all();
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
            shared.abort.store(true, Ordering::Release);
            shared.wake.notify_all();
            return Err(BuildError::Unavailable);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

fn acquire_flows(
    context: &dyn JobContext,
    limit: usize,
) -> Result<Vec<Box<dyn JobFlow>>, JobFlowError> {
    if limit == 0 {
        return Err(JobFlowError::UnsupportedResource);
    }
    let first = loop {
        if context.checkpoint() == Checkpoint::Cancel {
            return Err(JobFlowError::Cancelled);
        }
        match context.try_start_flow() {
            Ok(flow) => break flow,
            Err(JobFlowError::Busy) => thread::sleep(Duration::from_millis(25)),
            Err(error) => return Err(error),
        }
    };
    let mut flows = Vec::new();
    flows.push(first);
    for _ in 1..limit.min(MAX_WORKERS) {
        match context.try_start_flow() {
            Ok(flow) => flows.push(flow),
            Err(JobFlowError::Busy) => break,
            Err(_) => break,
        }
    }
    Ok(flows)
}

mod workers;
use workers::worker;
