//! Selects the next eligible frame from shared and worker-local queues.
use super::*;

pub(super) fn claim_ready<'a>(shared: &'a Shared, queue: &mut Queue) -> Option<Claim<'a>> {
    if queue.started {
        let (frame, group) = queue.pop_fair()?;
        Some(Claim {
            frame,
            permit: Permit { shared, group },
        })
    } else {
        None
    }
}

fn claim_local<'a>(
    shared: &'a Shared,
    queue: &mut Queue,
    local: &mut Vec<Frame>,
) -> Option<Claim<'a>> {
    if !queue.started {
        return None;
    }
    let eligible = |frame: &&Frame| {
        let group = queue.root_groups[frame.root_index as usize];
        queue.active_groups[group] < queue.device_limit
    };
    let index = if queue.priority.is_none() {
        local.iter().rposition(|frame| eligible(&frame))?
    } else {
        local
            .iter()
            .enumerate()
            .filter(|(_, frame)| eligible(frame))
            .min_by_key(|(index, frame)| (queue.priority_rank(frame), usize::MAX - *index))
            .map(|(index, _)| index)?
    };
    let frame = local.remove(index);
    let group = queue.root_groups[frame.root_index as usize];
    queue.active_groups[group] += 1;
    Some(Claim {
        frame,
        permit: Permit { shared, group },
    })
}

pub(super) fn take<'a>(
    shared: &'a Shared,
    context: &dyn JobContext,
    local: &mut Vec<Frame>,
    prefer_shared: bool,
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
        let claim = claim_next(shared, &mut queue, local, prefer_shared);
        if claim.is_some() {
            return claim;
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

fn claim_next<'a>(
    shared: &'a Shared,
    queue: &mut Queue,
    local: &mut Vec<Frame>,
    prefer_shared: bool,
) -> Option<Claim<'a>> {
    if queue.priority.is_none() {
        return if prefer_shared {
            claim_ready(shared, queue).or_else(|| claim_local(shared, queue, local))
        } else {
            claim_local(shared, queue, local).or_else(|| claim_ready(shared, queue))
        };
    }
    let shared_rank = queue.best_ready_rank();
    let local_rank = best_local_rank(queue, local);
    match (shared_rank, local_rank) {
        (Some(shared_rank), Some(local_rank)) if shared_rank < local_rank => {
            claim_ready(shared, queue)
        }
        (Some(shared_rank), Some(local_rank)) if local_rank < shared_rank => {
            claim_local(shared, queue, local)
        }
        (Some(_), Some(_)) if prefer_shared => claim_ready(shared, queue),
        (Some(_), Some(_)) => claim_local(shared, queue, local),
        (Some(_), None) => claim_ready(shared, queue),
        (None, Some(_)) => claim_local(shared, queue, local),
        (None, None) => None,
    }
}

fn best_local_rank(queue: &Queue, local: &[Frame]) -> Option<u8> {
    if !queue.started {
        return None;
    }
    local
        .iter()
        .filter(|frame| {
            let group = queue.root_groups[frame.root_index as usize];
            queue.active_groups[group] < queue.device_limit
        })
        .map(|frame| queue.priority_rank(frame))
        .min()
}

#[cfg(test)]
pub(super) fn claim_next_for_test<'a>(
    shared: &'a Shared,
    queue: &mut Queue,
    local: &mut Vec<Frame>,
    prefer_shared: bool,
) -> Option<Claim<'a>> {
    claim_next(shared, queue, local, prefer_shared)
}
