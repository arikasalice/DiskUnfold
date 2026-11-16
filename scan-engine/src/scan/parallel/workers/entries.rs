//! Directory admission, child scheduling and opening of assigned children.
//!
//! An admitted child is queued with a shared reference to this directory's
//! handle and is opened relative to it only when a worker claims it.
use super::*;
use crate::scan::parallel::frame::OpenFailure;

#[derive(Clone, Copy)]
struct ChildReservation {
    can_descend: bool,
    registered: bool,
}

pub(super) fn process_entry(
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
    // Deferred root entries were admitted, reserved and sent in the first pass.
    let admission = if recorded {
        Admission::Enter
    } else {
        // After cancellation or disconnection the entry will not be opened.
        let stopping = context.checkpoint() == Checkpoint::Cancel
            || shared.disconnects.closed(frame.root_index);
        admit(frame, entry, &path, &shared.visited, policy, !stopping)
    };
    if admission == Admission::Excluded {
        frame.excluded = frame.excluded.saturating_add(1);
        return Ok(None);
    }
    let enter = admission == Admission::Enter;
    let reservation = reserve_child(frame, enter, &path, recorded, shared, context)?;
    if !recorded {
        sender
            .send(Event::Item(
                frame.root_index,
                path.clone(),
                Box::new(timed.clone()),
                admission,
            ))
            .map_err(|_| ())?;
    }
    if !continue_child(frame, &path, reservation, shared, context)? {
        // An entered directory left unread on a disconnected volume is a gap
        // of its own; a cancelled one is recorded and marked as unread.
        let cancelled = context.checkpoint() == Checkpoint::Cancel;
        if enter {
            let sent = if cancelled {
                super::cancel_drain::unread(frame.root_index, path, sender)
            } else if shared.disconnects.closed(frame.root_index) {
                super::disconnected::unread(frame.root_index, path, sender, context)
            } else {
                true
            };
            if !sent {
                return Err(());
            }
        }
        return Ok(None);
    }
    if frame.root && !frame.root_published {
        frame.deferred.push_back(timed);
        return Ok(None);
    }
    let Some(parent) = frame.directory.anchor().cloned() else {
        return Err(());
    };
    let child = Frame::child(frame.root_index, path, parent, timed.entry);
    Ok(schedule(shared, context, child, reservation.registered))
}

fn reserve_child(
    frame: &Frame,
    eligible: bool,
    path: &[Vec<u8>],
    recorded: bool,
    shared: &Shared,
    context: &dyn JobContext,
) -> Result<ChildReservation, ()> {
    let can_descend = eligible
        && context.checkpoint() != Checkpoint::Cancel
        && !shared.disconnects.closed(frame.root_index);
    let registered = if !shared.track_priority_paths {
        false
    } else if can_descend && !recorded {
        if !register_path(shared, frame.root_index, path, context) {
            return Err(());
        }
        true
    } else {
        recorded && eligible
    };
    Ok(ChildReservation {
        can_descend,
        registered,
    })
}

fn continue_child(
    frame: &Frame,
    path: &[Vec<u8>],
    reservation: ChildReservation,
    shared: &Shared,
    context: &dyn JobContext,
) -> Result<bool, ()> {
    if reservation.can_descend
        && context.checkpoint() != Checkpoint::Cancel
        && !shared.disconnects.closed(frame.root_index)
    {
        return Ok(true);
    }
    if reservation.registered && !release_path(shared, frame.root_index, path, context) {
        return Err(());
    }
    Ok(false)
}

/// Opens a claimed child relative to its parent handle.
///
/// Returns `None` when the child is open and processing continues in this
/// turn, or when cancellation or an abort stopped the wait for its parent
/// handle; the child is then still pending. A rejected open is recorded as an
/// open-stage failure of the child and completes its outstanding work;
/// `Some(false)` stops the worker.
pub(super) fn open_pending(
    frame: &mut Frame,
    shared: &Shared,
    sender: &SyncSender<Event>,
    context: &dyn JobContext,
) -> Option<bool> {
    let stop =
        || context.checkpoint() == Checkpoint::Cancel || shared.abort.load(Ordering::Acquire);
    let error = match frame.directory.open(&stop) {
        Ok(()) => return None,
        Err(OpenFailure::Stopped) => return None,
        Err(OpenFailure::Unavailable) => return Some(false),
        Err(OpenFailure::Port(error)) => error,
    };
    let reason = reason(&error);
    shared.disconnects.observe_reason(frame.root_index, reason);
    let sent = sender
        .send(Event::Failure {
            root_index: frame.root_index,
            path: frame.path.clone(),
            root: false,
            reason,
            stage: du_core_contracts::scan_failures::ScanFailureStage::OpenDirectory,
            observed_at: context.utc_now(),
        })
        .is_ok();
    complete(shared, frame.root_index, &frame.path, context);
    Some(sent)
}
