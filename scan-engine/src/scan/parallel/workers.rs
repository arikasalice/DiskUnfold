//! Worker scheduling and bounded per-root directory processing.
//!
//! Each worker parks outside queue locks and device permits after transferring
//! an item or returned batch into owned state. An in-flight adapter call is
//! allowed to return first; a pause request is not itself a completed pause.
//! Internal abort wakes parked flows without setting the user cancellation flag.
use super::*;
mod cancel_drain;
mod disconnected;
mod read_batch;
use read_batch::{ReadFailure, next_entries};

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
        shared.abort(context);
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
    if context.checkpoint() == Checkpoint::Cancel && !shared.abort.load(Ordering::Acquire) {
        return cancel_drain::drain(shared, &mut local, sender, context, policy);
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
    let mut queue = match shared.queue.lock() {
        Ok(queue) => queue,
        Err(error) => {
            drop(error.into_inner());
            shared.abort(context);
            return None;
        }
    };
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
        queue = match shared.wake.wait_timeout(queue, Duration::from_millis(25)) {
            Ok((queue, _)) => queue,
            Err(error) => {
                drop(error.into_inner());
                shared.abort(context);
                return None;
            }
        };
        drop(queue);
        if flow.safe_point() == Checkpoint::Cancel {
            return None;
        }
        queue = match shared.queue.lock() {
            Ok(queue) => queue,
            Err(error) => {
                drop(error.into_inner());
                shared.abort(context);
                return None;
            }
        };
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
    if shared.abort.load(Ordering::Acquire) {
        return true;
    }
    if shared.disconnects.closed(frame.root_index) {
        return disconnected::close_frame(frame, shared, sender, context, policy);
    }
    if context.checkpoint() == Checkpoint::Cancel {
        return cancel_drain::frame(frame, sender, policy);
    }
    if !frame.case_observed {
        frame.case_observed = true;
        if let Some(evidence) = frame
            .directory
            .observe_directory_case(&|| context.utc_now())
        {
            if sender
                .send(Event::DirectoryCase(
                    frame.root_index,
                    frame.path.clone(),
                    evidence,
                ))
                .is_err()
            {
                return false;
            }
            if context.checkpoint() == Checkpoint::Cancel {
                return cancel_drain::frame(frame, sender, policy);
            }
            if shared.disconnects.closed(frame.root_index) {
                return disconnected::close_frame(frame, shared, sender, context, policy);
            }
        }
    }
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
                requeue(shared, frame, local, context);
                return true;
            }
            Err(()) => return false,
        }
    }
    if !publish_first_batch(&mut frame, sender, shared, context) {
        return false;
    }
    if frame.batch_pending {
        frame.batch_pending = false;
        requeue(shared, frame, local, context);
        return true;
    }
    if context.checkpoint() == Checkpoint::Cancel {
        return cancel_drain::frame(frame, sender, policy);
    }
    if shared.disconnects.closed(frame.root_index) {
        return disconnected::close_frame(frame, shared, sender, context, policy);
    }
    let outcome = next_entries(&mut frame, context, policy);
    process_batch(frame, shared, sender, context, local, outcome)
}

fn process_batch(
    mut frame: Frame,
    shared: &Shared,
    sender: &SyncSender<Event>,
    context: &dyn JobContext,
    local: &mut Vec<Frame>,
    outcome: Result<Option<Vec<TimedObservedEntry>>, ReadFailure>,
) -> bool {
    match outcome {
        Ok(None) => {
            requeue(shared, frame, local, context);
            true
        }
        Err(ReadFailure::Filter) => context.checkpoint() == Checkpoint::Cancel,
        Ok(Some(batch)) if batch.is_empty() => {
            if context.checkpoint() == Checkpoint::Cancel {
                complete(shared, frame.root_index, context);
                return true;
            }
            finish_directory(&frame, sender, shared, context)
        }
        Ok(Some(batch)) if batch.len() <= 8192 => {
            disconnected::observe_batch(shared, frame.root_index, &batch);
            frame.first_batch |= frame.root && !frame.root_published;
            frame.batch_pending = true;
            frame.entries = batch.into();
            requeue(shared, frame, local, context);
            true
        }
        outcome => {
            let failure_reason = match outcome {
                Err(ReadFailure::Observation(error)) => reason(&error),
                _ => "scan.unclassified",
            };
            shared
                .disconnects
                .observe_reason(frame.root_index, failure_reason);
            let _ = sender.send(Event::Failure {
                root_index: frame.root_index,
                path: frame.path.clone(),
                root: frame.root,
                reason: failure_reason,
                stage: du_core_contracts::scan_failures::ScanFailureStage::Enumerate,
                observed_at: context.utc_now(),
            });
            complete(shared, frame.root_index, context);
            true
        }
    }
}

fn finish_directory(
    frame: &Frame,
    sender: &SyncSender<Event>,
    shared: &Shared,
    context: &dyn JobContext,
) -> bool {
    if frame.root && !frame.root_published && !send_root_batch(frame, sender, shared, context) {
        return false;
    }
    complete(shared, frame.root_index, context);
    true
}

fn publish_first_batch(
    frame: &mut Frame,
    sender: &SyncSender<Event>,
    shared: &Shared,
    context: &dyn JobContext,
) -> bool {
    if frame.root && frame.first_batch {
        frame.first_batch = false;
        if !send_root_batch(frame, sender, shared, context) {
            return false;
        }
        frame.root_published = true;
    }
    true
}

fn send_root_batch(
    frame: &Frame,
    sender: &SyncSender<Event>,
    shared: &Shared,
    context: &dyn JobContext,
) -> bool {
    if context.checkpoint() == Checkpoint::Cancel {
        return true;
    }
    let (modified_at, observed_at) = if shared.disconnects.closed(frame.root_index) {
        (None, None)
    } else {
        frame
            .directory
            .observed_modified_at_timed(&|| context.utc_now())
    };
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
    if !eligible_directory(entry)
        || context.checkpoint() == Checkpoint::Cancel
        || shared.disconnects.closed(frame.root_index)
    {
        return Ok(None);
    }
    if frame.root && !frame.root_published {
        frame.deferred.push_back(timed);
        return Ok(None);
    }
    match frame.directory.open_child(entry) {
        Ok(directory) => Ok(schedule(
            shared,
            context,
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
                case_observed: false,
            },
        )),
        Err(error) => {
            let reason = reason(&error);
            shared.disconnects.observe_reason(frame.root_index, reason);
            sender
                .send(Event::Failure {
                    root_index: frame.root_index,
                    path,
                    root: false,
                    reason,
                    stage: du_core_contracts::scan_failures::ScanFailureStage::OpenDirectory,
                    observed_at: context.utc_now(),
                })
                .map_err(|_| ())?;
            Ok(None)
        }
    }
}
fn requeue(shared: &Shared, frame: Frame, local: &mut Vec<Frame>, context: &dyn JobContext) {
    let mut queue = match shared.queue.lock() {
        Ok(queue) => queue,
        Err(error) => {
            drop(error.into_inner());
            shared.abort(context);
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
fn schedule(shared: &Shared, context: &dyn JobContext, frame: Frame) -> Option<Frame> {
    let mut queue = match shared.queue.lock() {
        Ok(queue) => queue,
        Err(error) => {
            drop(error.into_inner());
            shared.abort(context);
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
fn complete(shared: &Shared, root_index: u32, context: &dyn JobContext) {
    match shared.queue.lock() {
        Ok(mut queue) => {
            queue.remaining -= 1;
            queue.root_remaining[root_index as usize] -= 1;
            shared.wake.notify_all();
        }
        Err(error) => {
            drop(error.into_inner());
            shared.abort(context);
        }
    }
}

#[cfg(test)]
mod permit_tests;
