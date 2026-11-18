//! Worker turns over lane work.
//!
//! One turn opens one claimed child, reads one batch of one directory or
//! handles one returned entry, then returns the directory to its lane.
//!
//! Each worker parks outside the scheduler lock after transferring an item or
//! returned batch into owned state. An in-flight adapter call is allowed to
//! return first; a pause request is not itself a completed pause. Internal
//! abort wakes parked flows without setting the user cancellation flag.
//!
//! After an explicit disconnection a directory whose enumeration started is
//! read to its end without opening children; one not started yet is closed
//! (see `disconnected`).
//!
//! Each directory reports the entries an exclusion rule dropped directly
//! below it once, when its listing ends or stops. After cancellation every
//! registered directory left unopened or partly read is reported as unread
//! once (see `cancel_drain`).
use super::*;
mod cancel_drain;
mod correlated_read;
mod correlated_transport;
mod disconnected;
mod dispatch;
mod entries;
mod handoff;
#[cfg(test)]
mod priority_dispatch_tests;
mod read_batch;
use entries::{EntryServices, open_pending, process_entry};
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
    // The case observation precedes the first read in the same turn, so it
    // marks a directory whose enumeration started in an earlier turn.
    let started = frame.case_observed;
    if disconnected::unstarted_on_closed(&frame, shared, started) {
        return disconnected::close_frame(frame, shared, sender, context, policy);
    }
    if context.checkpoint() == Checkpoint::Cancel {
        return cancel_drain::frame(frame, shared, sender, policy);
    }
    if frame.directory.is_pending() {
        return open_turn(frame, shared, sender, context, policy, turn);
    }
    match observe_once(&mut frame, shared, sender, context) {
        Observed::Ready => {}
        Observed::Failed => return false,
        Observed::Cancelled => return cancel_drain::frame(frame, shared, sender, policy),
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
        let services = EntryServices {
            shared,
            sender,
            context,
            policy,
        };
        return match process_entry(&mut frame, entry, recorded, false, &services) {
            Ok(child) => {
                requeue(shared, turn, frame, child, context);
                true
            }
            Err(()) => false,
        };
    }
    if !publish_first_batch(&mut frame, sender, context) {
        return false;
    }
    if frame.batch_pending {
        frame.batch_pending = false;
        requeue(shared, turn, frame, None, context);
        return true;
    }
    if context.checkpoint() == Checkpoint::Cancel {
        return cancel_drain::frame(frame, shared, sender, policy);
    }
    if disconnected::unstarted_on_closed(&frame, shared, started) {
        return disconnected::close_frame(frame, shared, sender, context, policy);
    }
    if matches!(policy, Policy::BeforeMetadata(_)) {
        match correlated_read::read(&mut frame, shared, sender, context, policy) {
            correlated_read::Read::Legacy => {}
            correlated_read::Read::Turn(result) => {
                return correlated_read::finish(
                    frame, *result, shared, sender, context, policy, turn,
                );
            }
        }
    }
    let mut undecided = Vec::new();
    let outcome = next_entries(&mut frame, context, policy, &mut undecided);
    if !send_undecided(&frame, undecided, sender) {
        return false;
    }
    process_batch(frame, shared, sender, (context, policy), turn, outcome)
}

/// Opens a claimed pending child as a turn of its own.
fn open_turn(
    mut frame: Frame,
    shared: &Shared,
    sender: &SyncSender<Event>,
    context: &dyn JobContext,
    policy: Policy<'_>,
    turn: &mut Turn<'_>,
) -> bool {
    if let Some(finished) = open_pending(&mut frame, shared, sender, context) {
        return finished;
    }
    if frame.directory.is_pending() {
        // Waiting for the parent handle stopped; nothing was opened.
        return shared.abort.load(Ordering::Acquire)
            || cancel_drain::frame(frame, shared, sender, policy);
    }
    // The open is this turn's only OS call. The opened directory goes back
    // to its lane, so an area prioritized while the open was in flight
    // applies to the next assignment (FR8.HD1.app-host.DD1).
    requeue(shared, turn, frame, None, context);
    true
}

enum Observed {
    Ready,
    Failed,
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
    (context, policy): (&dyn JobContext, Policy<'_>),
    turn: &mut Turn<'_>,
    outcome: Result<Option<Vec<TimedObservedEntry>>, ReadFailure>,
) -> bool {
    match outcome {
        Ok(None) => {
            requeue(shared, turn, frame, None, context);
            true
        }
        // Cancellation stops a filtered read; the directory was not read to its end.
        Err(ReadFailure::Filter) => {
            context.checkpoint() == Checkpoint::Cancel
                && cancel_drain::frame(frame, shared, sender, policy)
        }
        Err(ReadFailure::Unavailable) => false,
        Ok(Some(batch)) if batch.is_empty() => {
            if context.checkpoint() == Checkpoint::Cancel {
                // The listing reached its end; the directory is not unread.
                let sent = send_excluded(&frame, true, sender);
                complete(shared, frame.root_index, &frame.path, context);
                return sent;
            }
            finish_directory(&frame, sender, shared, context)
        }
        Ok(Some(batch)) if batch.len() <= 8192 => {
            let own = disconnected::observe_batch(shared, frame.root_index, &batch);
            frame.first_batch |= frame.root && !frame.root_published;
            frame.entries = batch
                .into_iter()
                .map(super::frame::QueuedEntry::from)
                .collect();
            if own {
                // This directory's own read reported the disconnection.
                return disconnected::close_frame(frame, shared, sender, context, policy);
            }
            frame.batch_pending = true;
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
            let _ = send_excluded(&frame, false, sender);
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
    if frame.root && !frame.root_published && !send_root_batch(frame, sender, context) {
        return false;
    }
    if !send_excluded(frame, true, sender) {
        return false;
    }
    complete(shared, frame.root_index, &frame.path, context);
    true
}

/// Reports the entries excluded directly below `frame` once, when its
/// listing ended (`complete`) or stopped; nothing is sent for none.
fn send_excluded(frame: &Frame, complete: bool, sender: &SyncSender<Event>) -> bool {
    let Some((count, complete)) = record::excluded_children(frame.excluded, complete) else {
        return true;
    };
    sender
        .send(Event::Excluded {
            root_index: frame.root_index,
            path: frame.path.clone(),
            count,
            complete,
        })
        .is_ok()
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
    if context.checkpoint() == Checkpoint::Cancel {
        return true;
    }
    // A root whose own read reported a disconnection never gets here; any
    // other root is read to its end, so it is observed like any other.
    let (modified_at, observed_at) = frame
        .directory
        .with(|directory| directory.observed_modified_at_timed(&|| context.utc_now()))
        .unwrap_or((None, None));
    sender
        .send(Event::RootBatch(frame.root_index, modified_at, observed_at))
        .is_ok()
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

#[cfg(test)]
mod permit_tests;
