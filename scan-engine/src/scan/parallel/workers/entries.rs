//! Directory admission and child scheduling for an observed entry.
use super::*;

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
        admit(frame, entry, &path, &shared.visited, policy)
    };
    if admission == Admission::Excluded {
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
        return Ok(None);
    }
    if frame.root && !frame.root_published {
        frame.deferred.push_back(timed);
        return Ok(None);
    }
    open_and_schedule(
        frame,
        entry,
        path,
        reservation.registered,
        shared,
        sender,
        context,
    )
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

fn open_and_schedule(
    frame: &Frame,
    entry: &ObservedEntry,
    path: Vec<Vec<u8>>,
    registered: bool,
    shared: &Shared,
    sender: &SyncSender<Event>,
    context: &dyn JobContext,
) -> Result<Option<Frame>, ()> {
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
                boundary: Boundary::child(entry),
            },
            registered,
        )),
        Err(error) => {
            if registered && !release_path(shared, frame.root_index, &path, context) {
                return Err(());
            }
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
