//! Drain returned observations and record the unread range of a closed directory.
use super::*;

pub(super) fn observe_batch(shared: &Shared, root: u32, batch: &[TimedObservedEntry]) {
    if batch.iter().any(|timed| disconnect::is_entry(&timed.entry)) {
        shared.disconnects.close(root);
    }
}

pub(super) fn close_frame(
    frame: Frame,
    shared: &Shared,
    sender: &SyncSender<Event>,
    context: &dyn JobContext,
    policy: Policy<'_>,
) -> bool {
    let gap = Event::Failure {
        root_index: frame.root_index,
        path: frame.path.clone(),
        root: frame.root,
        reason: disconnect::REASON,
        stage: du_core_contracts::scan_failures::ScanFailureStage::Enumerate,
        observed_at: context.utc_now(),
    };
    let root = frame.root_index;
    let path = frame.path.clone();
    let sent =
        cancel_drain::frame(frame, &shared.visited, sender, policy) && sender.send(gap).is_ok();
    complete(shared, root, &path, context);
    sent
}
