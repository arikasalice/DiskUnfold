//! Linear, bounded validation of one folder attempt at its EOF barrier.
use crate::{
    folder_evidence_api::{
        FolderAttemptError, FolderAttemptFinal, FolderAttemptInput, FolderAttemptStatus,
        FolderAttemptToken,
    },
    folder_evidence_attempt::AttemptState,
    folder_evidence_budget::{FolderEvidenceBudget, FolderEvidenceLease},
    observation_store::ObservationStore,
};
use du_core_contracts::{
    result::ResultItemKind,
    scan_observation::{CorrelatedDirectoryEvent, ObservedKind, TimedObservedEntry},
};
use std::{mem::size_of, sync::Arc};

#[derive(Clone, Copy, Default)]
struct ChildEvidence<'a> {
    callback: Option<(
        &'a [u8],
        du_core_contracts::scan_observation::FilterDecision,
        u64,
    )>,
    observation: Option<(&'a TimedObservedEntry, u64)>,
    final_outcome: Option<(FolderAttemptFinal, u64)>,
}

pub(crate) fn valid_attempt(
    attempt: &AttemptState,
    child_count_exclusive: u64,
    attempts: &[AttemptState],
    store: &ObservationStore,
    budget: &Arc<FolderEvidenceBudget>,
) -> Result<bool, FolderAttemptError> {
    if attempt.status != FolderAttemptStatus::Pending {
        return Ok(false);
    }
    let event_count =
        u64::try_from(attempt.events.len()).map_err(|_| FolderAttemptError::CapacityExceeded)?;
    if event_count < child_count_exclusive {
        return Ok(false);
    }
    let Ok(child_count) = usize::try_from(child_count_exclusive) else {
        return Err(FolderAttemptError::CapacityExceeded);
    };
    // The lease stays bound until validation ends so the scratch rows remain charged.
    let (_scratch_lease, mut children, mut name_order) =
        reserve_scratch(attempt, child_count, budget)?;
    if !collect_children(attempt, &mut children, &mut name_order) {
        return Ok(false);
    }
    if !children_terminal(attempt, &children, attempts, store) {
        return Ok(false);
    }
    Ok(unique_names(&children, &mut name_order))
}

type Scratch<'a> = (FolderEvidenceLease, Vec<ChildEvidence<'a>>, Vec<usize>);

fn reserve_scratch<'a>(
    attempt: &AttemptState,
    child_count: usize,
    budget: &Arc<FolderEvidenceBudget>,
) -> Result<Scratch<'a>, FolderAttemptError> {
    let Some(scratch_bytes) =
        child_count.checked_mul(size_of::<ChildEvidence<'_>>() + size_of::<usize>())
    else {
        return Err(FolderAttemptError::CapacityExceeded);
    };
    let Some(mut scratch_lease) = budget.reserve_for_folder(&attempt.scope, scratch_bytes) else {
        return Err(FolderAttemptError::CapacityExceeded);
    };
    let mut children = Vec::new();
    if children.try_reserve_exact(child_count).is_err() {
        return Err(FolderAttemptError::CapacityExceeded);
    }
    children.resize(child_count, ChildEvidence::default());
    let mut name_order = Vec::new();
    if name_order.try_reserve_exact(child_count).is_err() {
        return Err(FolderAttemptError::CapacityExceeded);
    }
    let Some(actual_scratch_bytes) = children
        .capacity()
        .checked_mul(size_of::<ChildEvidence<'_>>())
        .and_then(|bytes| {
            name_order
                .capacity()
                .checked_mul(size_of::<usize>())
                .and_then(|extra| bytes.checked_add(extra))
        })
    else {
        return Err(FolderAttemptError::CapacityExceeded);
    };
    if !scratch_lease.reconcile_bytes(budget, Some(&attempt.scope), actual_scratch_bytes) {
        return Err(FolderAttemptError::CapacityExceeded);
    }
    Ok((scratch_lease, children, name_order))
}

/// Indexes every stored event by child ID; returns false on any gap, duplicate, or terminal.
fn collect_children<'a>(
    attempt: &'a AttemptState,
    children: &mut [ChildEvidence<'a>],
    name_order: &mut Vec<usize>,
) -> bool {
    for (index, event) in attempt.events.iter().enumerate() {
        if u64::try_from(index).ok() != Some(event.event_seq) {
            return false;
        }
        let Some(input) = event.input.as_ref() else {
            return false;
        };
        match input {
            FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::FilterCallback {
                child_id,
                raw_name,
                decision,
                phase: _,
            }) => {
                let Ok(child_index) = usize::try_from(*child_id) else {
                    return false;
                };
                let Some(child) = children.get_mut(child_index) else {
                    return false;
                };
                if child
                    .callback
                    .replace((raw_name.as_slice(), *decision, event.event_seq))
                    .is_some()
                {
                    return false;
                }
                name_order.push(child_index);
            }
            FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::Observation {
                child_id,
                entry,
            }) => {
                let Some(child) = child_slot(children, *child_id) else {
                    return false;
                };
                if child
                    .observation
                    .replace((entry, event.event_seq))
                    .is_some()
                {
                    return false;
                }
            }
            FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::Progress) => {}
            FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::End { .. })
            | FolderAttemptInput::PendingCleared { .. }
            | FolderAttemptInput::TransportFailure
            | FolderAttemptInput::Cancelled => return false,
            FolderAttemptInput::Final { child_id, outcome } => {
                let Some(child) = child_slot(children, *child_id) else {
                    return false;
                };
                if child
                    .final_outcome
                    .replace((*outcome, event.event_seq))
                    .is_some()
                {
                    return false;
                }
            }
        }
    }
    true
}

fn child_slot<'s, 'a>(
    children: &'s mut [ChildEvidence<'a>],
    child_id: u64,
) -> Option<&'s mut ChildEvidence<'a>> {
    children.get_mut(usize::try_from(child_id).ok()?)
}

/// Checks that each child reached exactly the terminal its filter decision allows.
fn children_terminal(
    attempt: &AttemptState,
    children: &[ChildEvidence<'_>],
    attempts: &[AttemptState],
    store: &ObservationStore,
) -> bool {
    for child in children {
        let Some((raw_name, decision, callback_seq)) = child.callback else {
            return false;
        };
        match decision {
            du_core_contracts::scan_observation::FilterDecision::Exclude => {
                // The common adapter contract makes Exclude the only terminal for this child.
                if child.final_outcome.is_some() || child.observation.is_some() {
                    return false;
                }
            }
            du_core_contracts::scan_observation::FilterDecision::Admit => {
                let Some((observation, observation_seq)) = child.observation else {
                    return false;
                };
                if observation.entry.name.as_slice() != raw_name {
                    return false;
                }
                if !callback_precedes_observation(callback_seq, observation_seq) {
                    return false;
                }
                let Some((outcome, final_seq)) = child.final_outcome else {
                    return false;
                };
                if final_seq <= callback_seq || final_seq <= observation_seq {
                    return false;
                }
                if !valid_final(attempt.token, outcome, observation, attempts, store) {
                    return false;
                }
            }
            du_core_contracts::scan_observation::FilterDecision::Undecidable => {
                return false;
            }
        }
    }
    true
}

fn unique_names(children: &[ChildEvidence<'_>], name_order: &mut [usize]) -> bool {
    name_order.sort_unstable_by(|left, right| {
        let left_name = children[*left]
            .callback
            .map(|(name, _, _)| name)
            .unwrap_or_default();
        let right_name = children[*right]
            .callback
            .map(|(name, _, _)| name)
            .unwrap_or_default();
        left_name.cmp(right_name)
    });
    !name_order.windows(2).any(|pair| {
        let left = children[pair[0]].callback.map(|(name, _, _)| name);
        let right = children[pair[1]].callback.map(|(name, _, _)| name);
        left == right
    })
}

fn callback_precedes_observation(callback_seq: u64, observation_seq: u64) -> bool {
    // Phase describes when OS metadata was observed relative to the predicate,
    // not when F02 transports events. macOS can return a callback after a bulk
    // metadata read, then return its pending observation on the next call.
    callback_seq < observation_seq
}

fn valid_final(
    parent: FolderAttemptToken,
    outcome: FolderAttemptFinal,
    observed: &TimedObservedEntry,
    attempts: &[AttemptState],
    store: &ObservationStore,
) -> bool {
    let Some(child_node) = store.child_node(parent.folder_node, &observed.entry.name) else {
        return false;
    };
    let Some(stored) = store.observation(child_node) else {
        return false;
    };
    let observed_kind = match &observed.entry.metadata {
        Ok(metadata) => &metadata.kind,
        Err(_) => &observed.entry.enumerated_kind,
    };
    match outcome {
        FolderAttemptFinal::Recorded { child_attempt } => match observed_kind {
            ObservedKind::Directory => {
                stored.kind == ResultItemKind::Directory
                    && child_attempt.is_some_and(|token| {
                        child_token_matches(parent, token, child_node, attempts)
                    })
            }
            ObservedKind::MountPoint | ObservedKind::MountedFile => false,
            ObservedKind::File => {
                matches!(stored.kind, ResultItemKind::File | ResultItemKind::Unknown)
                    && child_attempt.is_none()
                    && !store.is_boundary(child_node)
            }
            ObservedKind::Link => {
                matches!(
                    stored.kind,
                    ResultItemKind::UntraversedLink | ResultItemKind::Unknown
                ) && child_attempt.is_none()
            }
            // A firmlink of the confirmed root volume group is entered once like a
            // directory; anywhere else it stays an untraversed link (#263).
            ObservedKind::Firmlink => match child_attempt {
                Some(token) => {
                    stored.kind == ResultItemKind::Directory
                        && child_token_matches(parent, token, child_node, attempts)
                }
                None => matches!(
                    stored.kind,
                    ResultItemKind::UntraversedLink | ResultItemKind::Unknown
                ),
            },
            ObservedKind::Other | ObservedKind::Unknown => {
                matches!(
                    stored.kind,
                    ResultItemKind::File
                        | ResultItemKind::Unknown
                        | ResultItemKind::UnknownStructure
                ) && child_attempt.is_none()
            }
        },
        FolderAttemptFinal::Boundary => {
            observed.entry.metadata.is_ok()
                && boundary_item(observed_kind, &stored.kind, store.is_boundary(child_node))
        }
        FolderAttemptFinal::Revisit => {
            observed.entry.metadata.is_ok()
                && stored.kind == ResultItemKind::Directory
                && observed_kind == &ObservedKind::Directory
        }
        FolderAttemptFinal::Unresolved => false,
    }
}

fn child_token_matches(
    parent: FolderAttemptToken,
    token: FolderAttemptToken,
    child_node: u32,
    attempts: &[AttemptState],
) -> bool {
    token.build_generation == parent.build_generation
        && token.root_index == parent.root_index
        && token.folder_node == child_node
        && attempts.iter().any(|state| state.token == token)
}

/// True when a boundary row stores the item kind that the collector records
/// for the observed kind: a directory item for another volume or a mount
/// point, and a boundary file item for a mounted file or a file on another
/// volume (F03 FR5.HD1).
fn boundary_item(observed: &ObservedKind, stored: &ResultItemKind, boundary_file: bool) -> bool {
    match observed {
        ObservedKind::Directory | ObservedKind::MountPoint => stored == &ResultItemKind::Directory,
        ObservedKind::MountedFile | ObservedKind::File => {
            boundary_file && stored == &ResultItemKind::File
        }
        _ => false,
    }
}
