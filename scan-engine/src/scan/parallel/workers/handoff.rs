//! Returning work to its lane and counting outstanding directory work.
//!
//! None of these operations waits for room: work that does not fit in the lane
//! queue goes onto the finding worker's own stack (NFR3.HD1.app-host.DD1).
use super::*;

/// Returns the turn's item and an optional child that did not fit earlier.
///
/// The item goes back first, so a child stacked by this call lies above it and
/// the worker continues into the child before resuming the item.
pub(super) fn requeue(
    shared: &Shared,
    turn: &mut Turn<'_>,
    frame: Frame,
    child: Option<Frame>,
    context: &dyn JobContext,
) {
    let reserved = std::mem::take(&mut turn.reserved);
    let mut queue = match shared.queue.lock() {
        Ok(queue) => queue,
        Err(error) => {
            drop(error.into_inner());
            shared.abort(context);
            return;
        }
    };
    queue.requeue(turn.seat, turn.lane, reserved, frame);
    if let Some(child) = child
        && let Some(child) = queue.offer(turn.lane, child)
    {
        queue.stack(turn.seat, turn.lane, child);
    }
    shared.wake.notify_one();
}

/// Counts a new child as outstanding and queues it in its root's lane.
///
/// Returns the child when the lane queue has no room; the caller keeps it.
pub(super) fn schedule(
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
    let tracked = !shared.track_priority_paths
        || registered
        || queue
            .outstanding
            .as_mut()
            .is_some_and(|outstanding| outstanding.register(frame.root_index, &frame.path));
    let Some(lane) = queue.lanes.lane_of(frame.root_index).filter(|_| tracked) else {
        drop(queue);
        shared.abort(context);
        return Some(frame);
    };
    queue.remaining += 1;
    queue.root_remaining[frame.root_index as usize] += 1;
    let rejected = queue.offer(lane, frame);
    if rejected.is_none() {
        shared.wake.notify_one();
    }
    rejected
}

pub(super) fn register_path(
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

pub(super) fn release_path(
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

pub(super) fn complete(
    shared: &Shared,
    root_index: u32,
    path: &[Vec<u8>],
    context: &dyn JobContext,
) {
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
