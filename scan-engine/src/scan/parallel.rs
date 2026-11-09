//! Bounded handle-based workers and a single result publisher.
use super::{ResultIds, record};
use du_analysis_index::{BuildError, IngestSink};
use du_core_contracts::{
    Id,
    job::{Checkpoint, JobContext, JobCounts, JobFlow, JobFlowError, JobOutcome},
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
    path: Vec<Vec<u8>>,
    directory: Box<dyn AnchoredDirectory>,
    entries: VecDeque<ObservedEntry>,
    deferred: VecDeque<ObservedEntry>,
    root: bool,
    first_batch: bool,
    root_published: bool,
}
struct Queue {
    ready: VecDeque<Frame>,
    remaining: usize,
    started: bool,
}
struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
    abort: AtomicBool,
}
enum Event {
    Item(Vec<Vec<u8>>, Box<ObservedEntry>),
    Failure { root: bool },
    RootBatch,
}
struct Control<'a> {
    ids: &'a dyn ResultIds,
    context: &'a dyn JobContext,
    limit: usize,
    include: &'a (dyn Fn(&[Vec<u8>]) -> bool + Sync),
}

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
    let mut counts = JobCounts::default();
    let result = run(
        sink,
        scope,
        root,
        Control {
            ids,
            context,
            limit,
            include,
        },
        &mut counts,
    );
    match result {
        Ok((id, cancelled)) => JobOutcome {
            counts,
            completed_scope: !cancelled,
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
    root: Box<dyn AnchoredDirectory>,
    control: Control<'_>,
    counts: &mut JobCounts,
) -> Result<(Id, bool), BuildError> {
    let Control {
        ids,
        context,
        limit,
        include,
    } = control;
    let mut build = sink.open(scope)?;
    build.add_directory(Vec::new())?;
    if context.checkpoint() == Checkpoint::Cancel {
        let id = ids.next_id();
        build.seal(id.clone(), ResultCompleteness::Cancelled)?;
        return Ok((id, true));
    }
    let flows = match acquire_flows(context, limit) {
        Ok(flows) => flows,
        Err(JobFlowError::Cancelled) => {
            let id = ids.next_id();
            build.seal(id.clone(), ResultCompleteness::Cancelled)?;
            return Ok((id, true));
        }
        Err(_) => return Err(BuildError::Unavailable),
    };
    let shared = Shared {
        queue: Mutex::new(Queue {
            ready: VecDeque::from([Frame {
                path: Vec::new(),
                directory: root,
                entries: VecDeque::new(),
                deferred: VecDeque::new(),
                root: true,
                first_batch: false,
                root_published: false,
            }]),
            remaining: 1,
            started: false,
        }),
        wake: Condvar::new(),
        abort: AtomicBool::new(false),
    };
    let (sender, receiver) = mpsc::sync_channel(BUFFERED_OBSERVATIONS);
    let mut failed_root = false;
    let mut published = false;
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
            let result = match event {
                Event::Item(path, entry) => record(&mut build, &entry, &path, counts).map(|_| ()),
                Event::Failure { root } => {
                    counts.failed += 1;
                    failed_root |= root && !published;
                    Ok(())
                }
                Event::RootBatch if !published => {
                    published = true;
                    build.publish_partial(ids.next_id()).map(|_| ())
                }
                Event::RootBatch => Ok(()),
            };
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
    if failed_root && !cancelled {
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
    build.seal(id.clone(), completeness)?;
    Ok((id, cancelled))
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

fn worker(
    shared: &Shared,
    sender: SyncSender<Event>,
    context: &dyn JobContext,
    include: &(dyn Fn(&[Vec<u8>]) -> bool + Sync),
    flow: Box<dyn JobFlow>,
) {
    let mut local = Vec::new();
    while context.checkpoint() != Checkpoint::Cancel && !shared.abort.load(Ordering::Acquire) {
        let frame = local.pop().or_else(|| take(shared, context));
        let Some(frame) = frame else {
            break;
        };
        if !process(frame, shared, &sender, context, include, &mut local) {
            shared.abort.store(true, Ordering::Release);
            shared.wake.notify_all();
            break;
        }
    }
    flow.finish(JobCounts::default());
}

fn take(shared: &Shared, context: &dyn JobContext) -> Option<Frame> {
    let mut queue = shared.queue.lock().ok()?;
    loop {
        if queue.remaining == 0
            || context.checkpoint() == Checkpoint::Cancel
            || shared.abort.load(Ordering::Acquire)
        {
            return None;
        }
        if queue.started
            && let Some(frame) = queue.ready.pop_front()
        {
            return Some(frame);
        }
        queue = shared
            .wake
            .wait_timeout(queue, Duration::from_millis(25))
            .ok()?
            .0;
    }
}

fn process(
    mut frame: Frame,
    shared: &Shared,
    sender: &SyncSender<Event>,
    context: &dyn JobContext,
    include: &(dyn Fn(&[Vec<u8>]) -> bool + Sync),
    local: &mut Vec<Frame>,
) -> bool {
    while context.checkpoint() != Checkpoint::Cancel && !shared.abort.load(Ordering::Acquire) {
        let recorded = frame.root_published && !frame.deferred.is_empty();
        let next = if recorded {
            frame.deferred.pop_front()
        } else {
            frame.entries.pop_front()
        };
        if let Some(entry) = next {
            match process_entry(
                &mut frame, entry, recorded, shared, sender, context, include,
            ) {
                Ok(Some(child)) => {
                    local.push(frame);
                    local.push(child);
                    return true;
                }
                Ok(None) => {}
                Err(()) => return false,
            }
            continue;
        }
        if frame.root && frame.first_batch {
            frame.first_batch = false;
            if sender.send(Event::RootBatch).is_err() {
                return false;
            }
            frame.root_published = true;
            continue;
        }
        match frame.directory.next_batch() {
            Ok(batch) if batch.is_empty() => {
                if frame.root && sender.send(Event::RootBatch).is_err() {
                    return false;
                }
                complete(shared);
                return true;
            }
            Ok(batch) if batch.len() <= 8192 => {
                frame.first_batch |= frame.root && !frame.root_published;
                frame.entries = batch.into();
            }
            _ => {
                let _ = sender.send(Event::Failure { root: frame.root });
                complete(shared);
                return true;
            }
        }
    }
    true
}

fn process_entry(
    frame: &mut Frame,
    entry: ObservedEntry,
    recorded: bool,
    shared: &Shared,
    sender: &SyncSender<Event>,
    context: &dyn JobContext,
    include: &(dyn Fn(&[Vec<u8>]) -> bool + Sync),
) -> Result<Option<Frame>, ()> {
    let mut path = frame.path.clone();
    path.push(entry.name.clone());
    if !recorded && !include(&path) {
        return Ok(None);
    }
    if !recorded {
        sender
            .send(Event::Item(path.clone(), Box::new(entry.clone())))
            .map_err(|_| ())?;
    }
    if !eligible_directory(&entry) || context.checkpoint() == Checkpoint::Cancel {
        return Ok(None);
    }
    if frame.root && !frame.root_published {
        frame.deferred.push_back(entry);
        return Ok(None);
    }
    match frame.directory.open_child(&entry) {
        Ok(directory) => Ok(schedule(
            shared,
            Frame {
                path,
                directory,
                entries: VecDeque::new(),
                deferred: VecDeque::new(),
                root: false,
                first_batch: false,
                root_published: false,
            },
        )),
        Err(_) => {
            sender
                .send(Event::Failure { root: false })
                .map_err(|_| ())?;
            Ok(None)
        }
    }
}

fn eligible_directory(entry: &ObservedEntry) -> bool {
    entry.metadata.as_ref().is_ok_and(|m| {
        m.kind == ObservedKind::Directory
            && m.inode != 0
            && m.inode == entry.enumerated_inode
            && (entry.enumerated_kind == ObservedKind::Unknown || entry.enumerated_kind == m.kind)
    })
}
fn schedule(shared: &Shared, frame: Frame) -> Option<Frame> {
    if let Ok(mut queue) = shared.queue.lock() {
        queue.remaining += 1;
        if queue.ready.len() < QUEUED_DIRECTORIES {
            queue.ready.push_back(frame);
            shared.wake.notify_one();
            return None;
        }
    } else {
        shared.abort.store(true, Ordering::Release);
    }
    Some(frame)
}
fn complete(shared: &Shared) {
    if let Ok(mut queue) = shared.queue.lock() {
        queue.remaining -= 1;
        shared.wake.notify_all();
    }
}
