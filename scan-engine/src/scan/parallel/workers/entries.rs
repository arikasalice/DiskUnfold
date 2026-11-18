//! Directory admission, child scheduling and opening of assigned children.
//!
//! An admitted child is queued with a shared reference to this directory's
//! handle and is opened relative to it only when a worker claims it.
use super::*;
use crate::scan::parallel::frame::{OpenFailure, QueuedEntry};

#[derive(Clone, Copy)]
struct ChildReservation {
    can_descend: bool,
    registered: bool,
}

/// The excluded callback is terminal and must not get a separate final event.
/// An entered child is unresolved until its own attempt has been registered.
fn correlated_final(
    admission: Admission,
    metadata_failed: bool,
    child: Option<du_analysis_index::FolderAttemptToken>,
) -> Option<du_analysis_index::FolderAttemptFinal> {
    use du_analysis_index::FolderAttemptFinal;
    if admission == Admission::Excluded {
        return None;
    }
    if metadata_failed {
        return Some(FolderAttemptFinal::Unresolved);
    }
    Some(match admission {
        Admission::Record | Admission::UntraversedLink => FolderAttemptFinal::Recorded {
            child_attempt: None,
        },
        Admission::OutsideBoundary => FolderAttemptFinal::Boundary,
        Admission::Revisit => FolderAttemptFinal::Revisit,
        Admission::Enter => match child {
            Some(token) => FolderAttemptFinal::Recorded {
                child_attempt: Some(token),
            },
            None => FolderAttemptFinal::Unresolved,
        },
        Admission::Excluded | Admission::Unverifiable => FolderAttemptFinal::Unresolved,
    })
}

#[cfg(test)]
mod correlated_final_tests {
    use super::*;
    use du_analysis_index::FolderAttemptFinal;
    use du_core_contracts::scan_observation::{MetadataObservation, ObservedKind};

    fn observed() -> ObservedEntry {
        ObservedEntry {
            name: b"child".to_vec(),
            enumerated_inode: 1,
            enumerated_kind: ObservedKind::Directory,
            metadata: Ok(MetadataObservation {
                kind: ObservedKind::Directory,
                device: 1,
                inode: 1,
                size: Ok(0),
                allocated: None,
                link_count: None,
                space_identity: None,
                modified_at: None,
                native_modified_at: None,
            }),
        }
    }

    #[test]
    fn excluded_has_no_final_and_unregistered_child_is_unresolved() {
        let entry = observed();
        assert!(correlated_final(Admission::Excluded, entry.metadata.is_err(), None).is_none());
        assert!(matches!(
            correlated_final(Admission::Enter, entry.metadata.is_err(), None),
            Some(FolderAttemptFinal::Unresolved)
        ));
    }

    #[test]
    fn boundaries_and_revisits_remain_distinct() {
        let entry = observed();
        assert!(matches!(
            correlated_final(Admission::OutsideBoundary, entry.metadata.is_err(), None),
            Some(FolderAttemptFinal::Boundary)
        ));
        assert!(matches!(
            correlated_final(Admission::Revisit, entry.metadata.is_err(), None),
            Some(FolderAttemptFinal::Revisit)
        ));
        assert!(matches!(
            correlated_final(Admission::Unverifiable, entry.metadata.is_err(), None),
            Some(FolderAttemptFinal::Unresolved)
        ));
    }
}

pub(super) fn process_entry(
    frame: &mut Frame,
    queued: QueuedEntry,
    recorded: bool,
    force_no_descend: bool,
    shared: &Shared,
    sender: &SyncSender<Event>,
    context: &dyn JobContext,
    policy: Policy<'_>,
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
    let (mut path, mut path_permit) = if correlated_entry {
        let name = &timed.as_ref().ok_or(())?.entry.name;
        let reserved = correlated_budget::child_path_clone_reservation(&frame.path, name)
            .and_then(|requested| shared.correlated_budget.reserve(requested))
            .and_then(|mut permit| {
                let path = correlated_budget::try_clone_child_path(&frame.path, name)?;
                let actual = correlated_budget::path_heap_capacity(&path)?;
                permit.resize(actual).then_some((path, permit))
            });
        if let Some((path, permit)) = reserved {
            (path, Some(permit))
        } else {
            // Correlation storage is optional; keep the ordinary F02 path alive.
            frame.correlated.evidence_failed();
            let mut path = frame.path.clone();
            path.push(name.clone());
            (path, None)
        }
    } else {
        let entry = &timed.as_ref().ok_or(())?.entry;
        let mut path = frame.path.clone();
        path.push(entry.name.clone());
        (path, None)
    };
    let entry = &timed.as_ref().ok_or(())?.entry;
    let metadata_failed = entry.metadata.is_err();
    // Deferred root entries were admitted, reserved and sent in the first pass.
    let admission = if recorded {
        Admission::Enter
    } else {
        // After cancellation or disconnection the entry will not be opened.
        let stopping = context.checkpoint() == Checkpoint::Cancel
            || shared.disconnects.closed(frame.root_index);
        admit(frame, entry, &path, &shared.visited, policy, !stopping)
    };
    if admission == Admission::Excluded {
        if let Some(child_id) = child_id {
            frame.correlated.finish_child(child_id);
        }
        frame.excluded = frame.excluded.saturating_add(1);
        return Ok(None);
    }
    let enter = admission == Admission::Enter;
    let mut reservation = reserve_child(
        frame,
        enter && !force_no_descend,
        &path,
        recorded,
        shared,
        context,
    )?;
    let mut child_entry = None;
    let mut child_entry_permit = None;
    if correlated_entry && enter && !recorded && reservation.can_descend {
        let source = &timed.as_ref().ok_or(())?.entry;
        if let Some((mut permit, copy)) = correlated_budget::entry_heap_capacity(source)
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<ObservedEntry>()))
            .and_then(|requested| {
                let permit = shared.correlated_budget.reserve(requested)?;
                let copy = correlated_budget::try_clone_entry(source)?;
                Some((permit, copy))
            })
        {
            let actual = correlated_budget::entry_heap_capacity(&copy)
                .and_then(|bytes| bytes.checked_add(std::mem::size_of::<ObservedEntry>()));
            if actual.is_some_and(|actual| permit.resize(actual)) {
                child_entry = Some(copy);
                child_entry_permit = Some(permit);
            }
        }
        if child_entry.is_none() {
            // This is the ordinary F02 child copy. Correlation budget must not stop traversal.
            child_entry = Some(source.clone());
        }
    }
    if !recorded {
        if correlated_entry {
            let item_timed = timed.take().ok_or(())?;
            let mut correlated_item = None;
            if let Some(mut item_permit) = correlated_budget::path_clone_reservation(&path)
                .and_then(|requested| shared.correlated_budget.reserve(requested))
                && let Some(item_path) = correlated_budget::try_clone_path(&path)
                && let Some(actual) = correlated_budget::path_heap_capacity(&item_path)
                && item_permit.resize(actual)
                && let Some(mut held_permit) = payload_permit.take()
            {
                if held_permit.merge(item_permit) {
                    correlated_item = Some((item_path, held_permit));
                } else {
                    payload_permit = Some(held_permit);
                }
            }
            if let Some((item_path, permit)) = correlated_item {
                sender
                    .send(Event::CorrelatedItem(
                        frame.root_index,
                        item_path,
                        Box::new(item_timed),
                        admission,
                        permit,
                    ))
                    .map_err(|_| ())?;
            } else {
                // A path reservation failure holds evidence but must not discard F02's item.
                frame.correlated.evidence_failed();
                sender
                    .send(Event::Item(
                        frame.root_index,
                        path.clone(),
                        Box::new(item_timed),
                        admission,
                        payload_permit.take(),
                    ))
                    .map_err(|_| ())?;
            }
        } else {
            let source = timed.as_ref().ok_or(())?;
            sender
                .send(Event::Item(
                    frame.root_index,
                    path.clone(),
                    Box::new(source.clone()),
                    admission,
                    None,
                ))
                .map_err(|_| ())?;
        }
    }
    // Item is sent before Begin on this channel. Its acknowledgement proves the
    // folder observation exists before the child token is registered.
    if enter
        && reservation.can_descend
        && matches!(policy, Policy::BeforeMetadata(_))
        && child_attempt.token().is_none()
    {
        if child_attempt.prepare(&shared.correlated_budget) {
            if let Some(attempt_id) = correlated_budget::next_attempt_id() {
                if let Ok(token) = correlated_transport::begin(
                    sender,
                    shared,
                    context,
                    frame.root_index,
                    &path,
                    attempt_id,
                ) {
                    child_attempt.bind(token);
                } else {
                    child_attempt.fallback_to_unselected();
                }
            } else {
                child_attempt.fallback_to_unselected();
            }
        }
    }
    if !recorded
        && let (Some(child_id), Some(token), Some(outcome)) = (
            child_id,
            frame.correlated.token(),
            correlated_final(admission, metadata_failed, child_attempt.token()),
        )
        && frame.correlated.saw_callback(child_id)
    {
        if let Some(seq) = frame.correlated.final_sequence() {
            let input = du_analysis_index::FolderAttemptInput::Final { child_id, outcome };
            if correlated_transport::record(sender, shared, context, token, seq, input).is_err() {
                frame.correlated.evidence_failed();
            }
        } else {
            frame.correlated.evidence_failed();
        }
    }
    if let Some(child_id) = child_id {
        frame.correlated.finish_child(child_id);
    }
    if !continue_child(frame, &path, reservation, shared, context)? {
        // An entered directory left unread on a disconnected volume is a gap
        // of its own; a cancelled one is recorded and marked as unread.
        let cancelled = context.checkpoint() == Checkpoint::Cancel;
        if enter {
            if cancelled {
                correlated_transport::cancel(sender, shared, &mut child_attempt);
            } else {
                correlated_transport::fail_best_effort(sender, shared, &mut child_attempt);
            }
            let sent = if cancelled {
                super::cancel_drain::unread(frame.root_index, path, sender)
            } else if shared.disconnects.closed(frame.root_index) {
                super::disconnected::unread(frame.root_index, path, sender, context)
            } else if force_no_descend {
                sender
                    .send(Event::Failure {
                        root_index: frame.root_index,
                        path,
                        root: false,
                        reason: "scan.unclassified",
                        stage: du_core_contracts::scan_failures::ScanFailureStage::Traverse,
                        observed_at: context.utc_now(),
                    })
                    .is_ok()
            } else {
                true
            };
            if !sent {
                return Err(());
            }
        }
        return Ok(None);
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
    Ok(schedule(shared, context, child, reservation.registered))
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
