//! One acknowledged correlated read turn, including the adapter EOF barrier.
use super::*;
use crate::scan::parallel::{
    correlated_budget::{self, MAX_EVENT_PAYLOAD_BYTES},
    frame::{QueuedEntry, correlated::Step},
};
use du_core_contracts::scan_observation::CorrelatedDirectoryEvent;
use std::mem::size_of;

pub(super) enum Read {
    Legacy,
    Turn(TurnResult),
}

pub(super) struct TurnResult {
    step: Step,
    undecided: Vec<Vec<u8>>,
    returned_permit: Option<correlated_budget::Permit>,
}

pub(super) fn read(
    frame: &mut Frame,
    shared: &Shared,
    sender: &SyncSender<Event>,
    context: &dyn JobContext,
    policy: Policy<'_>,
) -> Read {
    let Policy::BeforeMetadata(include) = policy else {
        return Read::Legacy;
    };
    if frame.correlated.unselected() {
        match frame
            .correlated
            .select(&frame.directory, &shared.correlated_budget)
        {
            Step::Legacy => {
                if frame.correlated.token().is_some() {
                    correlated_transport::fail_best_effort(sender, shared, &mut frame.correlated);
                }
                return Read::Legacy;
            }
            Step::Progress => {
                if frame.correlated.token().is_none() {
                    let Some(attempt_id) = correlated_budget::next_attempt_id() else {
                        frame.correlated.evidence_failed();
                        return Read::Turn(TurnResult {
                            step: Step::Progress,
                            undecided: Vec::new(),
                            returned_permit: None,
                        });
                    };
                    match correlated_transport::begin(
                        sender,
                        shared,
                        context,
                        frame.root_index,
                        &frame.path,
                        attempt_id,
                    ) {
                        Ok(token) => frame.correlated.bind(token),
                        Err(_) => {
                            // The adapter is already selected. Keep its ordinary observations
                            // flowing even when optional result registration cannot be retained.
                            frame.correlated.evidence_failed();
                            return Read::Turn(TurnResult {
                                step: Step::Progress,
                                undecided: Vec::new(),
                                returned_permit: None,
                            });
                        }
                    }
                }
            }
            _ => return failed_turn(),
        }
    }
    if frame.correlated.legacy() {
        return Read::Legacy;
    }
    let mut undecided = Vec::new();
    let mut excluded = 0_u64;
    let root = frame.root_index;
    let path = &frame.path;
    let mut decide = |name: &[u8]| {
        if context.checkpoint() == Checkpoint::Cancel {
            du_core_contracts::scan_observation::FilterDecision::Undecidable
        } else {
            let decision = include(root, path, name);
            if decision == du_core_contracts::scan_observation::FilterDecision::Exclude {
                excluded = excluded.saturating_add(1);
            }
            admission::exclusion(decision, name, &mut undecided)
        }
    };
    let raw_step = frame.correlated.next(
        &frame.directory,
        &mut decide,
        &|| context.utc_now(),
        &|| context.checkpoint() == Checkpoint::Cancel || shared.abort.load(Ordering::Acquire),
    );
    frame.excluded = frame.excluded.saturating_add(excluded);
    let mut returned_permit = None;
    let step = match raw_step {
        Step::Event {
            seq,
            payload,
            mut observation,
        } => {
            let result = match frame.correlated.token() {
                Some(token) => correlated_transport::record(
                    sender,
                    shared,
                    context,
                    token,
                    seq,
                    du_analysis_index::FolderAttemptInput::Adapter(payload),
                ),
                None => Ok(()),
            };
            if let Some(entry) = observation.as_mut() {
                let bytes = correlated_budget::timed_heap_capacity(&entry.timed)
                    .and_then(|bytes| bytes.checked_add(size_of::<QueuedEntry>()));
                let permit = bytes.and_then(|bytes| frame.correlated.retain_event(bytes));
                if let Some(permit) = permit {
                    entry.payload_permit = Some(permit);
                } else {
                    // Evidence accounting must not discard an ordinary observation that
                    // was already returned by the adapter. Continue through the F02 path.
                    frame.correlated.evidence_failed();
                    return Read::Turn(TurnResult {
                        step: Step::Accepted { observation },
                        undecided,
                        returned_permit: None,
                    });
                }
            } else {
                frame.correlated.release_event();
            }
            if result.is_err() {
                // Preserve a returned observation for normal F02 recording, but keep this
                // folder attempt held and stop reading it on the next turn.
                frame.correlated.evidence_failed();
            }
            Step::Accepted { observation }
        }
        Step::Returned(payload @ CorrelatedDirectoryEvent::Observation { .. }) => {
            let bytes = correlated_budget::event_payload_capacity(&payload)
                .filter(|bytes| *bytes <= MAX_EVENT_PAYLOAD_BYTES)
                .and_then(|bytes| bytes.checked_add(size_of::<QueuedEntry>()));
            returned_permit = bytes.and_then(|bytes| frame.correlated.retain_event(bytes));
            Step::Returned(payload)
        }
        Step::Returned(payload) => {
            frame.correlated.release_event();
            Step::Returned(payload)
        }
        other => other,
    };
    Read::Turn(TurnResult {
        step,
        undecided,
        returned_permit,
    })
}

fn failed_turn() -> Read {
    Read::Turn(TurnResult {
        step: Step::Failed,
        undecided: Vec::new(),
        returned_permit: None,
    })
}

pub(super) fn finish(
    mut frame: Frame,
    result: TurnResult,
    shared: &Shared,
    sender: &SyncSender<Event>,
    context: &dyn JobContext,
    policy: Policy<'_>,
    turn: &mut Turn<'_>,
) -> bool {
    if !send_undecided(&frame, result.undecided, sender) {
        return false;
    }
    match result.step {
        Step::Progress | Step::Accepted { observation: None } => {
            requeue(shared, turn, frame, None, context);
            true
        }
        Step::Accepted {
            observation: Some(entry),
        } => accepted_observation(frame, entry, shared, sender, context, policy, turn),
        Step::Returned(CorrelatedDirectoryEvent::Observation { child_id, entry }) => {
            if !frame
                .correlated
                .matches_callback(child_id, &entry.entry.name)
            {
                correlated_transport::fail_best_effort(sender, shared, &mut frame.correlated);
                return false;
            }
            let mut queued = QueuedEntry {
                timed: entry,
                child_id: Some(child_id),
                child_attempt: None,
                payload_permit: result.returned_permit,
            };
            if context.checkpoint() == Checkpoint::Cancel {
                frame.entries.push_back(queued);
                cancel_drain::frame(frame, shared, sender, policy)
            } else {
                // Preserve the returned fact first, mark any entered child unread, then
                // report the incomplete directory. Requeueing is unsafe after abort.
                frame.correlated.evidence_failed();
                match process_entry(
                    &mut frame, queued, false, true, shared, sender, context, policy,
                ) {
                    Ok(None) => process_batch(
                        frame,
                        shared,
                        sender,
                        (context, policy),
                        turn,
                        Err(ReadFailure::Observation(PortError {
                            kind: PortErrorKind::Io,
                            reason_key: "scan.unclassified".into(),
                            os_code: None,
                        })),
                    ),
                    _ => false,
                }
            }
        }
        Step::End {
            event_watermark_exclusive,
            child_count_exclusive,
        } => {
            let result = frame.correlated.token().map_or(Ok(()), |token| {
                correlated_transport::end(
                    sender,
                    shared,
                    context,
                    token,
                    event_watermark_exclusive,
                    child_count_exclusive,
                )
            });
            match result {
                Ok(()) => frame.correlated.end_acknowledged(),
                Err(correlated_transport::Failure::Cancelled) => {
                    return cancel_drain::frame(frame, shared, sender, policy);
                }
                Err(
                    correlated_transport::Failure::Aborted
                    | correlated_transport::Failure::Disconnected,
                ) => {
                    correlated_transport::fail_best_effort(sender, shared, &mut frame.correlated);
                    return false;
                }
                Err(_) => {
                    // EOF is real for F02 even when the evidence receiver refuses its barrier.
                    correlated_transport::fail_best_effort(sender, shared, &mut frame.correlated);
                }
            }
            process_batch(
                frame,
                shared,
                sender,
                (context, policy),
                turn,
                Ok(Some(Vec::new())),
            )
        }
        Step::Returned(_) | Step::Failed | Step::Legacy => {
            if context.checkpoint() == Checkpoint::Cancel {
                cancel_drain::frame(frame, shared, sender, policy)
            } else {
                correlated_transport::fail_best_effort(sender, shared, &mut frame.correlated);
                process_batch(
                    frame,
                    shared,
                    sender,
                    (context, policy),
                    turn,
                    Err(ReadFailure::Observation(PortError {
                        kind: PortErrorKind::Io,
                        reason_key: "scan.unclassified".into(),
                        os_code: None,
                    })),
                )
            }
        }
    }
}

fn accepted_observation(
    mut frame: Frame,
    entry: QueuedEntry,
    shared: &Shared,
    sender: &SyncSender<Event>,
    context: &dyn JobContext,
    policy: Policy<'_>,
    turn: &mut Turn<'_>,
) -> bool {
    if !entry.child_id.is_some_and(|id| {
        frame
            .correlated
            .matches_callback(id, &entry.timed.entry.name)
    }) {
        correlated_transport::fail_best_effort(sender, shared, &mut frame.correlated);
        return false;
    }
    let disconnected =
        disconnected::observe_batch(shared, frame.root_index, std::slice::from_ref(&entry.timed));
    frame.first_batch |= frame.root && !frame.root_published;
    frame.entries.push_back(entry);
    if disconnected {
        return disconnected::close_frame(frame, shared, sender, context, policy);
    }
    frame.batch_pending = true;
    requeue(shared, turn, frame, None, context);
    true
}
