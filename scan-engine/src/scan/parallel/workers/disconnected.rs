//! Drain returned observations and record the unread range of a closed directory.
use super::*;

pub(super) fn observe_batch(shared: &Shared, root: u32, batch: &[TimedObservedEntry]) {
    if batch.iter().any(|timed| {
        timed
            .entry
            .metadata
            .as_ref()
            .is_err_and(|error| reason(error) == "scan.disconnected")
    }) {
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
        reason: "scan.disconnected",
        stage: du_core_contracts::scan_failures::ScanFailureStage::Enumerate,
        observed_at: context.utc_now(),
    };
    let root = frame.root_index;
    let sent = cancel_drain::frame(frame, sender, policy) && sender.send(gap).is_ok();
    complete(shared, root, context);
    sent
}
