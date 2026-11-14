//! Worker scheduling and bounded per-root directory processing.
//!
//! Each worker parks outside queue locks and device permits after transferring
//! an item or returned batch into owned state. An in-flight adapter call is
//! allowed to return first; a pause request is not itself a completed pause.
//! Internal abort wakes parked flows without setting the user cancellation flag.
use super::*;
mod cancel_drain;
mod disconnected;
mod dispatch;
mod entries;
#[cfg(test)]
mod priority_dispatch_tests;
mod read_batch;
use entries::process_entry;
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
        let claim = dispatch::take(shared, context, &mut local, prefer_shared, flow);
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
        return cancel_drain::frame(frame, &shared.visited, sender, policy);
    }
    match observe_once(&mut frame, shared, sender, context) {
        Observed::Ready => {}
        Observed::SendFailed => return false,
        Observed::Cancelled => return cancel_drain::frame(frame, &shared.visited, sender, policy),
        Observed::Disconnected => {
            return disconnected::close_frame(frame, shared, sender, context, policy);
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
        return cancel_drain::frame(frame, &shared.visited, sender, policy);
    }
    if shared.disconnects.closed(frame.root_index) {
        return disconnected::close_frame(frame, shared, sender, context, policy);
    }
    let mut undecided = Vec::new();
    let outcome = next_entries(&mut frame, context, policy, &mut undecided);
    if !send_undecided(&frame, undecided, sender) {
        return false;
    }
    process_batch(frame, shared, sender, context, local, outcome)
}

enum Observed {
    Ready,
    SendFailed,
    Cancelled,
    Disconnected,
}

/// Observes per-directory facts once, before the first enumeration.
/// Root identities were already reserved before any worker started.
fn observe_once(
    frame: &mut Frame,
    shared: &Shared,
    sender: &SyncSender<Event>,
    context: &dyn JobContext,
) -> Observed {
    if frame.case_observed {
        return Observed::Ready;
    }
    frame.case_observed = true;
    let Some(evidence) = frame
        .directory
        .observe_directory_case(&|| context.utc_now())
    else {
        return Observed::Ready;
    };
    let event = Event::DirectoryCase(frame.root_index, frame.path.clone(), evidence);
    if sender.send(event).is_err() {
        Observed::SendFailed
    } else if context.checkpoint() == Checkpoint::Cancel {
        Observed::Cancelled
    } else if shared.disconnects.closed(frame.root_index) {
        Observed::Disconnected
    } else {
        Observed::Ready
    }
}

/// Reports names withheld by an undecidable exclusion as unread ranges.
fn send_undecided(frame: &Frame, names: Vec<Vec<u8>>, sender: &SyncSender<Event>) -> bool {
    names.into_iter().all(|name| {
        let mut path = frame.path.clone();
        path.push(name);
        sender
            .send(Event::Undecided(frame.root_index, path))
            .is_ok()
    })
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
                complete(shared, frame.root_index, &frame.path, context);
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
            complete(shared, frame.root_index, &frame.path, context);
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
    complete(shared, frame.root_index, &frame.path, context);
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
        queue.push_ready(frame);
        shared.wake.notify_one();
        return;
    }
    local.push(frame);
}

/// Applies the admission table to an entry returned for this frame.
///
/// A legacy path predicate is the exclusion row; entries filtered before
/// metadata already passed it inside the adapter. `reserve` is false where
/// the entry will not be opened, so no unentered directory is reserved.
fn admit(
    frame: &Frame,
    entry: &ObservedEntry,
    path: &[Vec<u8>],
    visited: &Visited,
    policy: Policy<'_>,
    reserve: bool,
) -> Admission {
    let excluded = matches!(policy, Policy::Legacy(include) if !include(path));
    let parent = admission::Parent {
        root: frame.root_index,
        boundary: frame.boundary,
    };
    if reserve {
        admission::decide(excluded, entry, parent, visited)
    } else {
        admission::preview(excluded, entry, parent, visited)
    }
}
fn schedule(
    shared: &Shared,
    context: &dyn JobContext,
    frame: Frame,
    registered: bool,
) -> Option<Frame> {
    let mut queue = match shared.queue.lock() {
        Ok(queue) => queue,
        Err(error) => {
            drop(error.into_inner());
            shared.abort(context);
            return Some(frame);
        }
    };
    if shared.track_priority_paths && !registered {
        let Some(outstanding) = queue.outstanding.as_mut() else {
            drop(queue);
            shared.abort(context);
            return Some(frame);
        };
        if !outstanding.register(frame.root_index, &frame.path) {
            drop(queue);
            shared.abort(context);
            return Some(frame);
        }
    }
    queue.remaining += 1;
    queue.root_remaining[frame.root_index as usize] += 1;
    if queue.ready.len() < QUEUED_DIRECTORIES {
        queue.push_ready(frame);
        shared.wake.notify_one();
        return None;
    }
    Some(frame)
}
fn register_path(
    shared: &Shared,
    root_index: u32,
    path: &[Vec<u8>],
    context: &dyn JobContext,
) -> bool {
    match shared.queue.lock() {
        Ok(mut queue) => {
            if queue
                .outstanding
                .as_mut()
                .is_some_and(|outstanding| outstanding.register(root_index, path))
            {
                true
            } else {
                drop(queue);
                shared.abort(context);
                false
            }
        }
        Err(error) => {
            drop(error.into_inner());
            shared.abort(context);
            false
        }
    }
}
fn release_path(
    shared: &Shared,
    root_index: u32,
    path: &[Vec<u8>],
    context: &dyn JobContext,
) -> bool {
    match shared.queue.lock() {
        Ok(mut queue) => {
            if queue.complete_path(root_index, path) {
                shared.wake.notify_all();
                true
            } else {
                drop(queue);
                shared.abort(context);
                false
            }
        }
        Err(error) => {
            drop(error.into_inner());
            shared.abort(context);
            false
        }
    }
}
fn complete(shared: &Shared, root_index: u32, path: &[Vec<u8>], context: &dyn JobContext) {
    match shared.queue.lock() {
        Ok(mut queue) => {
            queue.remaining -= 1;
            queue.root_remaining[root_index as usize] -= 1;
            if shared.track_priority_paths && !queue.complete_path(root_index, path) {
                drop(queue);
                shared.abort(context);
                return;
            }
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
