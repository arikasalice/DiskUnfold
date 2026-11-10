//! Worker scheduling and bounded per-root directory processing.
//!
//! Each worker parks outside queue locks and device permits after transferring
//! an item or returned batch into owned state. An in-flight adapter call is
//! allowed to return first; a pause request is not itself a completed pause.
//! A shared abort cannot wake a flow already blocked inside `safe_point`; the
//! runtime must resume or cancel that flow until an abort-aware port exists.
use super::*;

pub(super) fn worker(
    shared: &Shared,
    sender: SyncSender<Event>,
    context: &dyn JobContext,
    policy: Policy<'_>,
    flow: Box<dyn JobFlow>,
) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        worker_loop(shared, &sender, context, policy, &*flow)
    }));
    if !matches!(result, Ok(true)) {
        shared.abort();
    }
    flow.finish(JobCounts::default());
}

fn worker_loop(
    shared: &Shared,
    sender: &SyncSender<Event>,
    context: &dyn JobContext,
    policy: Policy<'_>,
    flow: &dyn JobFlow,
) -> bool {
    let mut local = Vec::new();
    let mut prefer_shared = true;
    while context.checkpoint() != Checkpoint::Cancel && !shared.abort.load(Ordering::Acquire) {
        if flow.safe_point() == Checkpoint::Cancel {
            break;
        }
        let claim = take(shared, context, &mut local, prefer_shared, flow);
        prefer_shared = !prefer_shared;
        let Some(Claim {
            frame,
            permit: _permit,
        }) = claim
        else {
            break;
        };
        if !process(frame, shared, sender, context, policy, &mut local) {
            return false;
        }
    }
    true
}

struct Permit<'a> {
    shared: &'a Shared,
    group: usize,
}
impl Drop for Permit<'_> {
    fn drop(&mut self) {
        let mut queue = self
            .shared
            .queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        queue.active_groups[self.group] -= 1;
        self.shared.wake.notify_all();
    }
}
struct Claim<'a> {
    frame: Frame,
    permit: Permit<'a>,
}

fn claim_ready<'a>(shared: &'a Shared, queue: &mut Queue) -> Option<Claim<'a>> {
    if queue.started {
        let (frame, group) = queue.pop_fair()?;
        Some(Claim {
            frame,
            permit: Permit { shared, group },
        })
    } else {
        None
    }
}

fn claim_local<'a>(
    shared: &'a Shared,
    queue: &mut Queue,
    local: &mut Vec<Frame>,
) -> Option<Claim<'a>> {
    if !queue.started {
        return None;
    }
    let index = local.iter().rposition(|frame| {
        let group = queue.root_groups[frame.root_index as usize];
        queue.active_groups[group] < queue.device_limit
    })?;
    let frame = local.remove(index);
    let group = queue.root_groups[frame.root_index as usize];
    queue.active_groups[group] += 1;
    Some(Claim {
        frame,
        permit: Permit { shared, group },
    })
}

fn take<'a>(
    shared: &'a Shared,
    context: &dyn JobContext,
    local: &mut Vec<Frame>,
    prefer_shared: bool,
    flow: &dyn JobFlow,
) -> Option<Claim<'a>> {
    let mut queue = shared.queue.lock().ok()?;
    loop {
        if context.checkpoint() == Checkpoint::Cancel || shared.abort.load(Ordering::Acquire) {
            return None;
        }
        let claim = if prefer_shared {
            claim_ready(shared, &mut queue).or_else(|| claim_local(shared, &mut queue, local))
        } else {
            claim_local(shared, &mut queue, local).or_else(|| claim_ready(shared, &mut queue))
        };
        if claim.is_some() {
            return claim;
        }
        if queue.remaining == 0 {
            return None;
        }
        queue = shared
            .wake
            .wait_timeout(queue, Duration::from_millis(25))
            .ok()?
            .0;
        drop(queue);
        if flow.safe_point() == Checkpoint::Cancel {
            return None;
        }
        queue = shared.queue.lock().ok()?;
    }
}

fn process(
    mut frame: Frame,
    shared: &Shared,
    sender: &SyncSender<Event>,
    context: &dyn JobContext,
    policy: Policy<'_>,
    local: &mut Vec<Frame>,
) -> bool {
    if context.checkpoint() != Checkpoint::Cancel && !shared.abort.load(Ordering::Acquire) {
        let recorded = frame.root_published && !frame.deferred.is_empty();
        let next = if recorded {
            frame.deferred.pop_front()
        } else {
            frame.entries.pop_front()
        };
        if let Some(entry) = next {
            match process_entry(&mut frame, entry, recorded, shared, sender, context, policy) {
                Ok(Some(child)) => {
                    local.push(frame);
                    local.push(child);
                    return true;
                }
                Ok(None) => {
                    requeue(shared, frame, local);
                    return true;
                }
                Err(()) => return false,
            }
        }
        if !publish_first_batch(&mut frame, sender, context) {
            return false;
        }
        if frame.batch_pending {
            frame.batch_pending = false;
            requeue(shared, frame, local);
            return true;
        }
        let outcome = next_entries(&mut frame, context, policy);
        match outcome {
            Ok(None) => {
                requeue(shared, frame, local);
                return true;
            }
            Err(ReadFailure::Filter) => {
                return context.checkpoint() == Checkpoint::Cancel;
            }
            Ok(Some(batch)) if batch.is_empty() => {
                return finish_directory(&frame, sender, shared, context);
            }
            Ok(Some(batch)) if batch.len() <= 8192 => {
                frame.first_batch |= frame.root && !frame.root_published;
                frame.batch_pending = true;
                frame.entries = batch.into();
                requeue(shared, frame, local);
                return true;
            }
            outcome => {
                let failure_reason = match outcome {
                    Err(ReadFailure::Observation(error)) => reason(&error),
                    _ => "scan.unclassified",
                };
                let _ = sender.send(Event::Failure {
                    root_index: frame.root_index,
                    path: frame.path.clone(),
                    root: frame.root,
                    reason: failure_reason,
                    stage: du_core_contracts::scan_failures::ScanFailureStage::Enumerate,
                    observed_at: context.utc_now(),
                });
                complete(shared, frame.root_index);
                return true;
            }
        }
    }
    true
}

fn finish_directory(
    frame: &Frame,
    sender: &SyncSender<Event>,
    shared: &Shared,
    context: &dyn JobContext,
) -> bool {
    if frame.root && !frame.root_published && !send_root_batch(frame, sender, context) {
        return false;
    }
    complete(shared, frame.root_index);
    true
}

enum ReadFailure {
    Filter,
    Observation(PortError),
}

fn next_entries(
    frame: &mut Frame,
    context: &dyn JobContext,
    policy: Policy<'_>,
) -> Result<Option<Vec<TimedObservedEntry>>, ReadFailure> {
    let Policy::BeforeMetadata(include) = policy else {
        return frame
            .directory
            .next_batch_timed(&|| context.utc_now())
            .map(Some)
            .map_err(ReadFailure::Observation);
    };
    let mut decide = |name: &[u8]| {
        if context.checkpoint() == Checkpoint::Cancel {
            du_core_contracts::scan_observation::FilterDecision::Undecidable
        } else {
            include(frame.root_index, &frame.path, name)
        }
    };
    let batch = frame
        .directory
        .next_filtered_timed(&mut decide, &|| context.utc_now())
        .map_err(|error| {
            if error.kind == PortErrorKind::Unsupported {
                ReadFailure::Filter
            } else {
                ReadFailure::Observation(error)
            }
        })?;
    match batch {
        du_core_contracts::scan_observation::TimedFilteredBatch::Entries(entries)
            if !entries.is_empty() =>
        {
            Ok(Some(entries))
        }
        du_core_contracts::scan_observation::TimedFilteredBatch::Progress => Ok(None),
        du_core_contracts::scan_observation::TimedFilteredBatch::End => Ok(Some(Vec::new())),
        _ => Err(ReadFailure::Filter),
    }
}

fn publish_first_batch(
    frame: &mut Frame,
    sender: &SyncSender<Event>,
    context: &dyn JobContext,
) -> bool {
    if frame.root && frame.first_batch {
        frame.first_batch = false;
        if !send_root_batch(frame, sender, context) {
            return false;
        }
        frame.root_published = true;
    }
    true
}

fn send_root_batch(frame: &Frame, sender: &SyncSender<Event>, context: &dyn JobContext) -> bool {
    let (modified_at, observed_at) = frame
        .directory
        .observed_modified_at_timed(&|| context.utc_now());
    sender
        .send(Event::RootBatch(frame.root_index, modified_at, observed_at))
        .is_ok()
}

fn process_entry(
    frame: &mut Frame,
    timed: TimedObservedEntry,
    recorded: bool,
    shared: &Shared,
    sender: &SyncSender<Event>,
    context: &dyn JobContext,
    policy: Policy<'_>,
) -> Result<Option<Frame>, ()> {
    let entry = &timed.entry;
    let mut path = frame.path.clone();
    path.push(entry.name.clone());
    if !recorded
        && let Policy::Legacy(include) = policy
        && !include(&path)
    {
        return Ok(None);
    }
    if !recorded {
        sender
            .send(Event::Item(
                frame.root_index,
                path.clone(),
                Box::new(timed.clone()),
            ))
            .map_err(|_| ())?;
    }
    if !eligible_directory(entry) || context.checkpoint() == Checkpoint::Cancel {
        return Ok(None);
    }
    if frame.root && !frame.root_published {
        frame.deferred.push_back(timed);
        return Ok(None);
    }
    match frame.directory.open_child(entry) {
        Ok(directory) => Ok(schedule(
            shared,
            Frame {
                root_index: frame.root_index,
                path,
                directory,
                entries: VecDeque::new(),
                deferred: VecDeque::new(),
                root: false,
                first_batch: false,
                root_published: false,
                batch_pending: false,
            },
        )),
        Err(error) => {
            sender
                .send(Event::Failure {
                    root_index: frame.root_index,
                    path,
                    root: false,
                    reason: reason(&error),
                    stage: du_core_contracts::scan_failures::ScanFailureStage::OpenDirectory,
                    observed_at: context.utc_now(),
                })
                .map_err(|_| ())?;
            Ok(None)
        }
    }
}
fn requeue(shared: &Shared, frame: Frame, local: &mut Vec<Frame>) {
    let mut queue = match shared.queue.lock() {
        Ok(queue) => queue,
        Err(error) => {
            let _queue = error.into_inner();
            shared.abort.store(true, Ordering::Release);
            shared.wake.notify_all();
            local.push(frame);
            return;
        }
    };
    if queue.ready.len() < QUEUED_DIRECTORIES {
        queue.ready.push_back(frame);
        shared.wake.notify_one();
        return;
    }
    local.push(frame);
}

fn eligible_directory(entry: &du_core_contracts::scan_observation::ObservedEntry) -> bool {
    entry.metadata.as_ref().is_ok_and(|m| {
        m.kind == ObservedKind::Directory
            && m.inode != 0
            && m.inode == entry.enumerated_inode
            && (entry.enumerated_kind == ObservedKind::Unknown || entry.enumerated_kind == m.kind)
    })
}
fn schedule(shared: &Shared, frame: Frame) -> Option<Frame> {
    let mut queue = match shared.queue.lock() {
        Ok(queue) => queue,
        Err(error) => {
            let _queue = error.into_inner();
            shared.abort.store(true, Ordering::Release);
            shared.wake.notify_all();
            return Some(frame);
        }
    };
    queue.remaining += 1;
    queue.root_remaining[frame.root_index as usize] += 1;
    if queue.ready.len() < QUEUED_DIRECTORIES {
        queue.ready.push_back(frame);
        shared.wake.notify_one();
        return None;
    }
    Some(frame)
}
fn complete(shared: &Shared, root_index: u32) {
    if let Ok(mut queue) = shared.queue.lock() {
        queue.remaining -= 1;
        queue.root_remaining[root_index as usize] -= 1;
        shared.wake.notify_all();
    }
}

#[cfg(test)]
mod permit_tests {
    use super::*;
    use du_core_contracts::{
        ports::{PortError, PortErrorKind},
        scan_observation::DirectoryObservations,
    };
    struct Empty;
    impl DirectoryObservations for Empty {
        fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
            Ok(Vec::new())
        }
    }
    impl AnchoredDirectory for Empty {
        fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
            Err(PortError {
                kind: PortErrorKind::Unsupported,
                reason_key: "os.unsupported".into(),
                os_code: None,
            })
        }
    }
    #[test]
    fn permit_is_released_during_panic_unwind() {
        let frame = Frame {
            root_index: 0,
            path: Vec::new(),
            directory: Box::new(Empty),
            entries: VecDeque::new(),
            deferred: VecDeque::new(),
            root: true,
            first_batch: false,
            root_published: false,
            batch_pending: false,
        };
        let shared = Shared {
            queue: Mutex::new(Queue {
                ready: VecDeque::from([frame]),
                last_root: None,
                remaining: 1,
                root_remaining: vec![1],
                root_groups: vec![0],
                active_groups: vec![0],
                device_limit: 1,
                started: true,
            }),
            wake: Condvar::new(),
            abort: AtomicBool::new(false),
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut queue = shared
                .queue
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let claim = claim_ready(&shared, &mut queue);
            drop(queue);
            let _claim = claim;
            assert!(
                shared
                    .queue
                    .lock()
                    .is_ok_and(|queue| queue.active_groups[0] == 1)
            );
            std::panic::resume_unwind(Box::new("injected panic"));
        }));
        assert!(result.is_err());
        assert!(
            shared
                .queue
                .lock()
                .is_ok_and(|queue| queue.active_groups[0] == 0)
        );
    }
}
