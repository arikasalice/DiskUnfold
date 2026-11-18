//! Directory admission, child scheduling and opening of assigned children.
//!
//! An admitted child is queued with a shared reference to this directory's
//! handle and is opened relative to it only when a worker claims it.
use super::*;
use crate::scan::parallel::frame::{OpenFailure, QueuedEntry};
mod correlated;

#[derive(Clone, Copy)]
struct ChildReservation {
    can_descend: bool,
    registered: bool,
}

pub(super) struct EntryServices<'a> {
    pub(super) shared: &'a Shared,
    pub(super) sender: &'a SyncSender<Event>,
    pub(super) context: &'a dyn JobContext,
    pub(super) policy: Policy<'a>,
}

pub(super) fn process_entry(
    frame: &mut Frame,
    queued: QueuedEntry,
    recorded: bool,
    force_no_descend: bool,
    services: &EntryServices<'_>,
) -> Result<Option<Frame>, ()> {
    let QueuedEntry {
        timed,
        child_id,
        child_attempt,
        payload_permit,
    } = queued;
    let mut timed = Some(timed);
    let mut payload_permit = payload_permit;
    let correlated_entry = child_id.is_some();
    let mut child_attempt = child_attempt.unwrap_or_default();
    let (path, mut path_permit) =
        correlated::entry_path(frame, &timed, correlated_entry, services)?;
    let entry = &timed.as_ref().ok_or(())?.entry;
    let metadata_failed = entry.metadata.is_err();
    // Deferred root entries were admitted, reserved and sent in the first pass.
    let admission = if recorded {
        Admission::Enter
    } else {
        // After cancellation or disconnection the entry will not be opened.
        let stopping = services.context.checkpoint() == Checkpoint::Cancel
            || services.shared.disconnects.closed(frame.root_index);
        admit(
            frame,
            entry,
            &path,
            &services.shared.visited,
            services.policy,
            !stopping,
        )
    };
    if admission == Admission::Excluded {
        if let Some(child_id) = child_id {
            frame.correlated.finish_child(child_id);
        }
        frame.excluded = frame.excluded.saturating_add(1);
        return Ok(None);
    }
    let enter = admission == Admission::Enter;
    let reservation = reserve_child(
        frame,
        enter && !force_no_descend,
        &path,
        recorded,
        services.shared,
        services.context,
    )?;
    let (mut child_entry, mut child_entry_permit) = correlated::copy_child_entry(
        entry,
        correlated_entry,
        enter,
        recorded,
        reservation.can_descend,
        services,
    );
    if !recorded {
        correlated::publish_item(
            frame,
            &mut timed,
            &mut payload_permit,
            &path,
            admission,
            correlated_entry,
            services,
        )?;
    }
    // Item is sent before Begin on this channel. Its acknowledgement proves the
    // folder observation exists before the child token is registered.
    correlated::register_child_attempt(
        frame,
        &mut child_attempt,
        enter,
        reservation.can_descend,
        &path,
        services,
    );
    correlated::record_final(
        frame,
        child_id,
        recorded,
        admission,
        metadata_failed,
        child_attempt.token(),
        services,
    );
    if let Some(child_id) = child_id {
        frame.correlated.finish_child(child_id);
    }
    if !continue_child(frame, &path, reservation, services.shared, services.context)? {
        return stopped_child(
            frame,
            path,
            enter,
            force_no_descend,
            &mut child_attempt,
            services,
        );
    }
    if frame.root && !frame.root_published {
        let (deferred_timed, deferred_permit) = if correlated_entry && !recorded {
            let entry = child_entry.take().ok_or(())?;
            (TimedObservedEntry::from(entry), child_entry_permit.take())
        } else {
            (timed.take().ok_or(())?, payload_permit.take())
        };
        frame.deferred.push_back(QueuedEntry {
            timed: deferred_timed,
            child_id,
            child_attempt: enter.then_some(child_attempt),
            payload_permit: enter.then_some(deferred_permit).flatten(),
        });
        return Ok(None);
    }
    let Some(parent) = frame.directory.anchor().cloned() else {
        return Err(());
    };
    let (child_entry, child_permit) = if correlated_entry && !recorded {
        (child_entry.take().ok_or(())?, child_entry_permit.take())
    } else {
        (timed.take().ok_or(())?.entry, payload_permit.take())
    };
    let child = Frame::child(
        frame.root_index,
        path,
        parent,
        child_entry,
        child_permit,
        path_permit.take(),
        child_attempt,
    );
    Ok(schedule(
        services.shared,
        services.context,
        child,
        reservation.registered,
    ))
}

fn stopped_child(
    frame: &Frame,
    path: Vec<Vec<u8>>,
    enter: bool,
    force_no_descend: bool,
    child_attempt: &mut super::frame::correlated::Attempt,
    services: &EntryServices<'_>,
) -> Result<Option<Frame>, ()> {
    if !enter {
        return Ok(None);
    }
    // Cancellation and disconnection keep their distinct unread evidence.
    let cancelled = services.context.checkpoint() == Checkpoint::Cancel;
    if cancelled {
        correlated_transport::cancel(services.sender, services.shared, child_attempt);
    } else {
        correlated_transport::fail_best_effort(services.sender, services.shared, child_attempt);
    }
    let sent = if cancelled {
        super::cancel_drain::unread(frame.root_index, path, services.sender)
    } else if services.shared.disconnects.closed(frame.root_index) {
        super::disconnected::unread(frame.root_index, path, services.sender, services.context)
    } else if force_no_descend {
        services
            .sender
            .send(Event::Failure {
                root_index: frame.root_index,
                path,
                root: false,
                reason: "scan.unclassified",
                stage: du_core_contracts::scan_failures::ScanFailureStage::Traverse,
                observed_at: services.context.utc_now(),
            })
            .is_ok()
    } else {
        true
    };
    if sent { Ok(None) } else { Err(()) }
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
    correlated_transport::fail_best_effort(sender, shared, &mut frame.correlated);
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
