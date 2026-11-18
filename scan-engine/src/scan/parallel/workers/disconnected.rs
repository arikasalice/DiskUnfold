//! Closing directories on a volume that reported an explicit disconnection.
//!
//! FR12.HD1.app-host.DD1: the result does not depend on worker scheduling for
//! the directory that reported the disconnection and for its ancestors. That
//! directory is closed with its returned entries. A directory whose
//! enumeration had started is read to its end, opening nothing more. Every
//! enumerated directory that is not read is a gap at its own path, so another
//! directory is either read to its end or left as a gap without contents.
use super::*;

/// Closes the volume when a returned batch carries disconnection evidence.
///
/// Returns true when the directory's own read reported the disconnection.
pub(super) fn observe_batch(shared: &Shared, root: u32, batch: &[TimedObservedEntry]) -> bool {
    let disconnected = batch.iter().any(|timed| disconnect::is_entry(&timed.entry));
    if disconnected {
        shared.disconnects.close(root);
    }
    disconnected
}

/// Whether `frame` stops here: its volume closed before its enumeration started.
///
/// `started` tells whether a batch of this directory was read in an earlier turn.
pub(super) fn unstarted_on_closed(frame: &Frame, shared: &Shared, started: bool) -> bool {
    !started && shared.disconnects.closed(frame.root_index)
}

/// Closes a directory that is not read further, recording it as a gap.
///
/// Its returned entries are recorded without opening any of them, and every
/// entered directory among them is a gap of its own.
pub(super) fn close_frame(
    mut frame: Frame,
    shared: &Shared,
    sender: &SyncSender<Event>,
    context: &dyn JobContext,
    policy: Policy<'_>,
) -> bool {
    correlated_transport::fail_best_effort(sender, shared, &mut frame.correlated);
    let sent = drain(&mut frame, shared, sender, context, policy)
        && unread(frame.root_index, frame.path.clone(), sender, context)
        && super::send_excluded(&frame, false, sender);
    complete(shared, frame.root_index, &frame.path, context);
    sent
}

/// Records one enumerated directory that will not be read as a gap.
pub(super) fn unread(
    root_index: u32,
    path: Vec<Vec<u8>>,
    sender: &SyncSender<Event>,
    context: &dyn JobContext,
) -> bool {
    sender
        .send(Event::Failure {
            root_index,
            root: path.is_empty(),
            path,
            reason: disconnect::REASON,
            stage: du_core_contracts::scan_failures::ScanFailureStage::Enumerate,
            observed_at: context.utc_now(),
        })
        .is_ok()
}

fn drain(
    frame: &mut Frame,
    shared: &Shared,
    sender: &SyncSender<Event>,
    context: &dyn JobContext,
    policy: Policy<'_>,
) -> bool {
    while let Some(mut queued) = frame.entries.pop_front() {
        let timed = queued.timed;
        let mut path = frame.path.clone();
        path.push(timed.entry.name.clone());
        let admission = super::admit(frame, &timed.entry, &path, &shared.visited, policy, false);
        if admission == Admission::Excluded {
            frame.excluded = frame.excluded.saturating_add(1);
            continue;
        }
        let item = Event::Item(
            frame.root_index,
            path.clone(),
            Box::new(timed),
            admission,
            queued.payload_permit.take(),
        );
        if sender.send(item).is_err() {
            return false;
        }
        if admission == Admission::Enter
            && let Some(attempt) = queued.child_attempt.as_mut()
        {
            correlated_transport::fail_best_effort(sender, shared, attempt);
        }
        if admission == Admission::Enter && !unread(frame.root_index, path, sender, context) {
            return false;
        }
    }
    // Deferred root entries were recorded, reserved and registered already.
    while let Some(mut queued) = frame.deferred.pop_front() {
        let timed = queued.timed;
        let mut path = frame.path.clone();
        path.push(timed.entry.name.clone());
        if shared.track_priority_paths && !release_path(shared, frame.root_index, &path, context) {
            return false;
        }
        if let Some(attempt) = queued.child_attempt.as_mut() {
            correlated_transport::fail_best_effort(sender, shared, attempt);
        }
        if !unread(frame.root_index, path, sender, context) {
            return false;
        }
    }
    true
}
