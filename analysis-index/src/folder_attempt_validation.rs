//! DRAFT test-only checks for unique callback-to-child correlation.
use super::folder_attempt_contract::{
    AttemptEventPayload, ChildCorrelationCapability, DirectoryChildFinal, SequencedAttemptEvent,
};
use du_core_contracts::scan_observation::{FilterDecision, ObservedKind};

/// Accepts an EOF count only when every ID has one terminal callback and, when selected,
/// one final admission outcome. Exact same-sequence replay is removed by the ledger first.
pub(super) fn valid_child_correlations(
    events: &[SequencedAttemptEvent],
    callback_watermark: u64,
    child_count_exclusive: u64,
) -> bool {
    let mut callback_count = 0u64;
    let mut admitted_count = 0u64;
    let mut final_count = 0u64;
    for (index, event) in events.iter().enumerate() {
        if u64::try_from(index).ok() != Some(event.event_seq) {
            return false;
        }
        match &event.payload {
            AttemptEventPayload::Started { .. } if index == 0 => {}
            AttemptEventPayload::Started { .. } => return false,
            AttemptEventPayload::CallbackSeen {
                callback_id,
                child_id,
                raw_name,
                decision,
                ..
            } => {
                let Some(child_id) = child_id else {
                    return false;
                };
                if *callback_id >= callback_watermark || *child_id >= child_count_exclusive {
                    return false;
                }
                for prior in &events[..index] {
                    if let AttemptEventPayload::CallbackSeen {
                        callback_id: prior_callback,
                        child_id: prior_child,
                        raw_name: prior_name,
                        ..
                    } = &prior.payload
                    {
                        // A repeated callback under a new event sequence is a conflict,
                        // even when its payload is equal. Same-sequence replay was handled
                        // before insertion by the event ledger.
                        if prior_callback == callback_id || prior_child == &Some(*child_id) {
                            return false;
                        }
                        if prior_name == raw_name {
                            return false;
                        }
                    }
                }
                callback_count = callback_count.saturating_add(1);
                match decision {
                    FilterDecision::Admit => admitted_count = admitted_count.saturating_add(1),
                    // Exclude itself is the single terminal for this ID. It is counted
                    // once by child_count_exclusive and has no ChildFinal frame.
                    FilterDecision::Exclude => {}
                    FilterDecision::Undecidable => return false,
                }
            }
            AttemptEventPayload::ChildFinal {
                child_id,
                classification,
            } => {
                if *child_id >= child_count_exclusive
                    || !valid_terminal_classification(classification)
                    || !events[..index].iter().any(|prior| {
                        matches!(
                            &prior.payload,
                            AttemptEventPayload::CallbackSeen {
                                child_id: Some(callback_child),
                                decision: FilterDecision::Admit,
                                ..
                            } if callback_child == child_id
                        )
                    })
                    || events[..index].iter().any(|prior| {
                        matches!(
                            &prior.payload,
                            AttemptEventPayload::ChildFinal {
                                child_id: prior_child,
                                ..
                            } if prior_child == child_id
                        )
                    })
                {
                    return false;
                }
                final_count = final_count.saturating_add(1);
            }
            AttemptEventPayload::Progress => {}
            AttemptEventPayload::PendingCleared { .. }
            | AttemptEventPayload::MetadataError
            | AttemptEventPayload::TransportFailure
            | AttemptEventPayload::Cancelled => return false,
        }
    }
    if events.first().map_or(true, |first| {
        first.event_seq != 0
            || !matches!(
                &first.payload,
                AttemptEventPayload::Started {
                    correlation: Some(ChildCorrelationCapability::StableChildId)
                }
            )
    }) || callback_count != callback_watermark
        || callback_count != child_count_exclusive
        || final_count != admitted_count
    {
        return false;
    }

    events.iter().all(|event| {
        let AttemptEventPayload::CallbackSeen {
            child_id: Some(child_id),
            decision,
            ..
        } = &event.payload
        else {
            return true;
        };
        let final_class = events.iter().find_map(|candidate| {
            if let AttemptEventPayload::ChildFinal {
                child_id: final_id,
                classification,
            } = &candidate.payload
            {
                (final_id == child_id).then_some(classification)
            } else {
                None
            }
        });
        match decision {
            FilterDecision::Admit => matches!(
                final_class,
                Some(
                    DirectoryChildFinal::Recorded { .. }
                        | DirectoryChildFinal::Boundary { .. }
                        | DirectoryChildFinal::Revisit
                )
            ),
            FilterDecision::Exclude => final_class.is_none(),
            FilterDecision::Undecidable => false,
        }
    })
}

fn valid_terminal_classification(classification: &DirectoryChildFinal) -> bool {
    match classification {
        DirectoryChildFinal::Recorded {
            kind: ObservedKind::Directory,
            child_attempt_id: Some(_),
        } => true,
        DirectoryChildFinal::Recorded {
            kind: ObservedKind::Directory | ObservedKind::MountPoint,
            child_attempt_id: None,
        } => false,
        DirectoryChildFinal::Recorded {
            child_attempt_id: Some(_),
            ..
        } => false,
        DirectoryChildFinal::Recorded { .. }
        | DirectoryChildFinal::Boundary { .. }
        | DirectoryChildFinal::Revisit => true,
        DirectoryChildFinal::Unresolved => false,
    }
}
