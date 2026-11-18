//! Publisher acknowledgements for correlated folder attempts.
//!
//! Workers hold no directory lease or scheduler lock while waiting here.
use super::*;
use crate::scan::parallel::correlated_budget;
use du_analysis_index::{FolderAttemptError, FolderAttemptInput, FolderAttemptToken};
use std::sync::mpsc::{Receiver, TrySendError};

const POLL: Duration = Duration::from_millis(25);
const ACK_BYTES: usize = 1024;
const EVENT_BYTES: usize = 1024;

#[allow(dead_code)]
pub(super) enum Failure {
    Cancelled,
    Aborted,
    Disconnected,
    Capacity,
    Rejected(FolderAttemptError),
}

fn stop(shared: &Shared, context: &dyn JobContext) -> Option<Failure> {
    if shared.abort.load(Ordering::Acquire) {
        Some(Failure::Aborted)
    } else if context.checkpoint() == Checkpoint::Cancel {
        Some(Failure::Cancelled)
    } else {
        None
    }
}

/// Waits without holding a directory lease or scheduler lock.
fn send(
    sender: &SyncSender<Event>,
    mut event: Event,
    shared: &Shared,
    context: &dyn JobContext,
) -> Result<(), Failure> {
    loop {
        if let Some(failure) = stop(shared, context) {
            return Err(failure);
        }
        match sender.try_send(event) {
            Ok(()) => return Ok(()),
            Err(TrySendError::Disconnected(_)) => return Err(Failure::Disconnected),
            Err(TrySendError::Full(returned)) => {
                event = returned;
                std::thread::sleep(POLL);
            }
        }
    }
}

fn receive<T>(
    receiver: Receiver<Result<T, FolderAttemptError>>,
    shared: &Shared,
    context: &dyn JobContext,
) -> Result<T, Failure> {
    loop {
        if let Some(failure) = stop(shared, context) {
            return Err(failure);
        }
        match receiver.recv_timeout(POLL) {
            Ok(Ok(value)) => return Ok(value),
            Ok(Err(error)) => return Err(Failure::Rejected(error)),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Err(Failure::Disconnected),
        }
    }
}

/// Clones and registers a path only after its folder observation was queued to the publisher.
pub(super) fn begin(
    sender: &SyncSender<Event>,
    shared: &Shared,
    context: &dyn JobContext,
    root_index: u32,
    path: &[Vec<u8>],
    attempt_id: u64,
) -> Result<FolderAttemptToken, Failure> {
    let requested = correlated_budget::path_clone_reservation(path).ok_or(Failure::Capacity)?;
    let Some(mut permit) = shared
        .correlated_budget
        .reserve(requested.saturating_add(EVENT_BYTES))
    else {
        return Err(Failure::Capacity);
    };
    let cloned = correlated_budget::try_clone_path(path).ok_or(Failure::Capacity)?;
    let actual = correlated_budget::path_heap_capacity(&cloned, cloned.capacity())
        .and_then(|bytes| bytes.checked_add(EVENT_BYTES))
        .ok_or(Failure::Capacity)?;
    if !permit.resize(actual) {
        return Err(Failure::Capacity);
    }
    let Some(_ack_permit) = shared.correlated_budget.reserve(ACK_BYTES) else {
        return Err(Failure::Capacity);
    };
    let (reply, receiver) = mpsc::channel();
    send(
        sender,
        Event::FolderAttemptBegin {
            root_index,
            path: cloned,
            attempt_id,
            reply,
            _permit: permit,
        },
        shared,
        context,
    )?;
    receive(receiver, shared, context)
}

/// Acknowledgement means the publisher accepted this exact sequence and payload.
pub(super) fn record(
    sender: &SyncSender<Event>,
    shared: &Shared,
    context: &dyn JobContext,
    token: FolderAttemptToken,
    seq: u64,
    input: FolderAttemptInput,
) -> Result<(), Failure> {
    let Some(_ack_permit) = shared.correlated_budget.reserve(ACK_BYTES) else {
        return Err(Failure::Capacity);
    };
    let Some(event_permit) = shared.correlated_budget.reserve(EVENT_BYTES) else {
        return Err(Failure::Capacity);
    };
    let (reply, receiver) = mpsc::channel();
    send(
        sender,
        Event::FolderAttemptEvent {
            token,
            seq,
            input,
            reply: Some(reply),
            _permit: Some(event_permit),
        },
        shared,
        context,
    )?;
    receive(receiver, shared, context)
}

/// End is an acknowledged barrier after all numbered events have been accepted.
pub(super) fn end(
    sender: &SyncSender<Event>,
    shared: &Shared,
    context: &dyn JobContext,
    token: FolderAttemptToken,
    event_watermark_exclusive: u64,
    child_count_exclusive: u64,
) -> Result<(), Failure> {
    let Some(_ack_permit) = shared.correlated_budget.reserve(ACK_BYTES) else {
        return Err(Failure::Capacity);
    };
    let Some(_event_permit) = shared.correlated_budget.reserve(EVENT_BYTES) else {
        return Err(Failure::Capacity);
    };
    let (reply, receiver) = mpsc::channel();
    send(
        sender,
        Event::FolderAttemptEnd {
            token,
            event_watermark_exclusive,
            child_count_exclusive,
            reply,
        },
        shared,
        context,
    )?;
    receive(receiver, shared, context)
}

/// Records a terminal transport failure without waiting for a reply.
pub(super) fn fail_best_effort(
    sender: &SyncSender<Event>,
    shared: &Shared,
    attempt: &mut frame::correlated::Attempt,
) {
    let Some(token) = attempt.token() else {
        attempt.cancel();
        return;
    };
    let Some(seq) = attempt.final_sequence() else {
        attempt.cancel();
        return;
    };
    let Some(permit) = shared.correlated_budget.reserve(EVENT_BYTES) else {
        attempt.cancel();
        return;
    };
    let _ = sender.try_send(Event::FolderAttemptEvent {
        token,
        seq,
        input: FolderAttemptInput::TransportFailure,
        reply: None,
        _permit: Some(permit),
    });
    attempt.cancel();
}

/// Cancellation is best-effort; an unaccepted notice leaves the attempt pending at seal time.
pub(super) fn cancel(
    sender: &SyncSender<Event>,
    shared: &Shared,
    attempt: &mut frame::correlated::Attempt,
) {
    let Some(token) = attempt.token() else {
        attempt.cancel();
        return;
    };
    let Some(seq) = attempt.final_sequence() else {
        attempt.cancel();
        return;
    };
    if let Some(permit) = shared.correlated_budget.reserve(EVENT_BYTES) {
        let _ = sender.try_send(Event::FolderAttemptEvent {
            token,
            seq,
            input: FolderAttemptInput::Cancelled,
            reply: None,
            _permit: Some(permit),
        });
    }
    attempt.cancel();
}
