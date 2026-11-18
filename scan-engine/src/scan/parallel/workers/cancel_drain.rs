//! Deliver observations already returned by the OS, without opening more paths.
//!
//! Every directory drained here was registered and is not read to its end:
//! a pending child that was never opened, or an opened directory whose
//! listing stopped. It is marked as unread once, with each entered directory
//! among its returned or deferred entries, which will not be opened either.
//! An unverifiable root is not marked: its start already recorded a gap, and
//! a gap and a mark are never left on one directory.
use super::*;

/// Drains only still-owned returned entries; deferred entries were sent already.
/// Each entry keeps its admission row; no drained directory is opened or reserved.
/// The entries excluded so far are reported as an incomplete count.
pub(super) fn frame(
    mut frame: Frame,
    visited: &Visited,
    sender: &SyncSender<Event>,
    policy: Policy<'_>,
) -> bool {
    while let Some(entry) = frame.entries.pop_front() {
        let mut path = frame.path.clone();
        path.push(entry.entry.name.clone());
        let admission = super::admit(&frame, &entry.entry, &path, visited, policy, false);
        if admission == Admission::Excluded {
            frame.excluded = frame.excluded.saturating_add(1);
            continue;
        }
        let entered = (admission == Admission::Enter).then(|| path.clone());
        if sender
            .send(Event::Item(
                frame.root_index,
                path,
                Box::new(entry),
                admission,
            ))
            .is_err()
        {
            return false;
        }
        if let Some(path) = entered
            && !unread(frame.root_index, path, sender)
        {
            return false;
        }
    }
    // Deferred root entries were recorded as entered and never opened.
    while let Some(entry) = frame.deferred.pop_front() {
        let mut path = frame.path.clone();
        path.push(entry.entry.name);
        if !unread(frame.root_index, path, sender) {
            return false;
        }
    }
    // Only an unverifiable root has no volume boundary; entered children always have one.
    let unverifiable = frame.boundary == Boundary::Unknown;
    super::send_excluded(&frame, false, sender)
        && (unverifiable || unread(frame.root_index, frame.path, sender))
}

/// Marks one registered directory as left unread by cancellation.
pub(super) fn unread(root_index: u32, path: Vec<Vec<u8>>, sender: &SyncSender<Event>) -> bool {
    sender.send(Event::Unread(root_index, path)).is_ok()
}

/// Owns frames outside the queue lock before sending to the bounded publisher.
///
/// Any held item may be drained here, including items of other lanes or on
/// other workers' stacks: draining opens and reads nothing.
pub(super) fn drain(
    shared: &Shared,
    seat: usize,
    sender: &SyncSender<Event>,
    context: &dyn JobContext,
    policy: Policy<'_>,
) -> bool {
    loop {
        if shared.abort.load(Ordering::Acquire) {
            return false;
        }
        let next = match shared.queue.lock() {
            Ok(mut queue) => queue.lanes.take_any(seat),
            Err(error) => {
                drop(error.into_inner());
                shared.abort(context);
                return false;
            }
        };
        match next {
            Some(owned) => {
                if !frame(owned, &shared.visited, sender, policy) {
                    return false;
                }
            }
            None => return true,
        }
    }
}

#[cfg(test)]
mod tests;
