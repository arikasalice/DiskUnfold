//! Claims the next item of a worker's lane, waiting outside the lock if none.
use super::*;
use crate::scan::parallel::lanes::{Origin, Taken};

/// Turns a lane claim into a turn that releases its reserved slot on drop.
pub(super) fn claim<'a>(shared: &'a Shared, queue: &mut Queue, seat: usize) -> Option<Claim<'a>> {
    let Taken {
        frame,
        lane,
        origin,
    } = queue.claim_for(seat)?;
    Some(Claim {
        frame,
        turn: Turn {
            shared,
            seat,
            lane,
            reserved: origin == Origin::Ready,
        },
    })
}

/// Waits for claimable work; a waiting worker reaches its safe point.
pub(super) fn take<'a>(
    shared: &'a Shared,
    context: &dyn JobContext,
    seat: usize,
    flow: &dyn JobFlow,
) -> Option<Claim<'a>> {
    let mut queue = match shared.queue.lock() {
        Ok(queue) => queue,
        Err(error) => {
            drop(error.into_inner());
            shared.abort(context);
            return None;
        }
    };
    loop {
        if context.checkpoint() == Checkpoint::Cancel || shared.abort.load(Ordering::Acquire) {
            return None;
        }
        let claimed = claim(shared, &mut queue, seat);
        if claimed.is_some() {
            return claimed;
        }
        if queue.remaining == 0 {
            return None;
        }
        queue = match shared.wake.wait_timeout(queue, Duration::from_millis(25)) {
            Ok((queue, _)) => queue,
            Err(error) => {
                drop(error.into_inner());
                shared.abort(context);
                return None;
            }
        };
        drop(queue);
        if flow.safe_point() == Checkpoint::Cancel {
            return None;
        }
        queue = match shared.queue.lock() {
            Ok(queue) => queue,
            Err(error) => {
                drop(error.into_inner());
                shared.abort(context);
                return None;
            }
        };
    }
}
