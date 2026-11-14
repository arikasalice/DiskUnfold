//! Worker turns over lane work and bounded per-directory processing.
//!
//! Each worker parks outside the scheduler lock after transferring an item or
//! returned batch into owned state. An in-flight adapter call is allowed to
//! return first; a pause request is not itself a completed pause. Internal
//! abort wakes parked flows without setting the user cancellation flag.
use super::*;
mod cancel_drain;
mod disconnected;
mod dispatch;
mod entries;
mod handoff;
#[cfg(test)]
mod priority_dispatch_tests;
mod read_batch;
use entries::{open_pending, process_entry};
use handoff::{complete, register_path, release_path, requeue, schedule};
use read_batch::{ReadFailure, next_entries};

pub(super) fn worker(
    shared: &Shared,
    sender: SyncSender<Event>,
    context: &dyn JobContext,
    policy: Policy<'_>,
    flow: Box<dyn JobFlow>,
) {
    if let Some(seat) = SeatGuard::register(shared) {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            worker_loop(shared, seat.index, &sender, context, policy, &*flow)
        }));
        drop(seat);
        if !matches!(result, Ok(true)) {
            shared.abort(context);
        }
    } else {
        shared.abort(context);
    }
    flow.finish(JobCounts::default());
}

fn worker_loop(
    shared: &Shared,
    seat: usize,
    sender: &SyncSender<Event>,
    context: &dyn JobContext,
    policy: Policy<'_>,
    flow: &dyn JobFlow,
) -> bool {
    while context.checkpoint() != Checkpoint::Cancel && !shared.abort.load(Ordering::Acquire) {
        if flow.safe_point() == Checkpoint::Cancel {
            break;
        }
        let Some(Claim { frame, mut turn }) = dispatch::take(shared, context, seat, flow) else {
            break;
        };
        if !process(frame, shared, sender, context, policy, &mut turn) {
            return false;
        }
    }
    if context.checkpoint() == Checkpoint::Cancel && !shared.abort.load(Ordering::Acquire) {
        return cancel_drain::drain(shared, seat, sender, context, policy);
    }
    true
}

/// A worker's seat; dropping it leaves the lane, also during unwinding.
struct SeatGuard<'a> {
    shared: &'a Shared,
    index: usize,
}

impl<'a> SeatGuard<'a> {
    fn register(shared: &'a Shared) -> Option<Self> {
        let index = shared.queue.lock().ok()?.lanes.register_seat();
        Some(Self { shared, index })
    }
}

impl Drop for SeatGuard<'_> {
    fn drop(&mut self) {
        let mut queue = self
            .shared
            .queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        queue.lanes.release_seat(self.index);
        self.shared.wake.notify_all();
    }
}

/// One claimed item's lane, seat and reserved lane-queue slot.
///
/// The slot of an item claimed from the lane queue is released when the turn
/// ends without returning the item, also during unwinding.
struct Turn<'a> {
    shared: &'a Shared,
    seat: usize,
    lane: usize,
    reserved: bool,
}

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        if self.reserved {
            let mut queue = self
                .shared
                .queue
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            queue.lanes.release_reservation(self.lane);
        }
    }
}

struct Claim<'a> {
    frame: Frame,
    turn: Turn<'a>,
}

fn process(
    mut frame: Frame,
    shared: &Shared,
    sender: &SyncSender<Event>,
    context: &dyn JobContext,
    policy: Policy<'_>,
    turn: &mut Turn<'_>,
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
    if frame.directory.is_pending()
        && let Some(finished) = open_pending(&mut frame, shared, sender, context)
    {
        return finished;
    }
    match observe_once(&mut frame, shared, sender, context) {
        Observed::Ready => {}
        Observed::Failed => return false,
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
        return match process_entry(&mut frame, entry, recorded, shared, sender, context, policy) {
            Ok(child) => {
                requeue(shared, turn, frame, child, context);
                true
            }
            Err(()) => false,
        };
    }
    if !publish_first_batch(&mut frame, sender, shared, context) {
        return false;
    }
    if frame.batch_pending {
        frame.batch_pending = false;
        requeue(shared, turn, frame, None, context);
        return true;
    }
    if context.checkpoint() == Checkpoint::Cancel {
        return cancel_drain::frame(frame, &shared.visited, sender, policy);
    }
    if shared.disconnects.closed(frame.root_index) {
        return disconnected::close_frame(frame, shared, sender, context, policy);
    }
    let mut undecided = Vec::new();
    let outcome = next_entries(&frame, context, policy, &mut undecided);
    if !send_undecided(&frame, undecided, sender) {
        return false;
    }
    process_batch(frame, shared, sender, context, turn, outcome)
}

enum Observed {
    Ready,
    Failed,
    Cancelled,
    Disconnected,
}

/// Observes per-directory facts once, before the first enumeration.
///
/// A root first reports its own identity so that the admission table can
/// compare child volumes with it. Both calls run in the claiming worker's turn.
fn observe_once(
    frame: &mut Frame,
    shared: &Shared,
    sender: &SyncSender<Event>,
    context: &dyn JobContext,
) -> Observed {
    if frame.boundary == Boundary::Pending {
        match frame
            .directory
            .with(|directory| shared.visited.observe_root(directory))
        {
            Some(boundary) => frame.boundary = boundary,
            None => return Observed::Failed,
        }
    }
    if frame.case_observed {
        return Observed::Ready;
    }
    frame.case_observed = true;
    let Some(evidence) = frame
        .directory
        .with(|directory| directory.observe_directory_case(&|| context.utc_now()))
    else {
        return Observed::Failed;
    };
    let Some(evidence) = evidence else {
        return Observed::Ready;
    };
    let event = Event::DirectoryCase(frame.root_index, frame.path.clone(), evidence);
    if sender.send(event).is_err() {
        Observed::Failed
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
    turn: &mut Turn<'_>,
    outcome: Result<Option<Vec<TimedObservedEntry>>, ReadFailure>,
) -> bool {
    match outcome {
        Ok(None) => {
            requeue(shared, turn, frame, None, context);
            true
        }
        Err(ReadFailure::Filter) => context.checkpoint() == Checkpoint::Cancel,
        Err(ReadFailure::Unavailable) => false,
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
            requeue(shared, turn, frame, None, context);
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
            .with(|directory| directory.observed_modified_at_timed(&|| context.utc_now()))
            .unwrap_or((None, None))
    };
    sender
        .send(Event::RootBatch(frame.root_index, modified_at, observed_at))
        .is_ok()
}

/// Applies the admission table to an entry returned for this frame.
///
/// A legacy path predicate is the exclusion row; entries filtered before
/// metadata already passed it inside the adapter.
fn admit(
    frame: &Frame,
    entry: &ObservedEntry,
    path: &[Vec<u8>],
    visited: &Visited,
    policy: Policy<'_>,
) -> Admission {
    let excluded = matches!(policy, Policy::Legacy(include) if !include(path));
    admission::decide(excluded, entry, frame.boundary, visited)
}

#[cfg(test)]
mod permit_tests;
