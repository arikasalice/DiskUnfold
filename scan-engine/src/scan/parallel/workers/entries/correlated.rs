//! Correlated evidence handling for admitted directory entries.
use super::*;
use crate::scan::parallel::correlated_budget::Permit;
use crate::scan::parallel::frame::correlated::Attempt;

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

pub(super) fn entry_path(
    frame: &mut Frame,
    timed: &Option<TimedObservedEntry>,
    correlated_entry: bool,
    services: &EntryServices<'_>,
) -> Result<(Vec<Vec<u8>>, Option<Permit>), ()> {
    let entry = &timed.as_ref().ok_or(())?.entry;
    if correlated_entry {
        let name = &entry.name;
        let reserved = correlated_budget::child_path_clone_reservation(&frame.path, name)
            .and_then(|requested| services.shared.correlated_budget.reserve(requested))
            .and_then(|mut permit| {
                let path = correlated_budget::try_clone_child_path(&frame.path, name)?;
                let actual = correlated_budget::path_heap_capacity(&path, path.capacity())?;
                permit.resize(actual).then_some((path, permit))
            });
        if let Some((path, permit)) = reserved {
            return Ok((path, Some(permit)));
        }
        // Correlation storage is optional; keep the ordinary F02 path alive.
        frame.correlated.evidence_failed();
    }
    let mut path = frame.path.clone();
    path.push(entry.name.clone());
    Ok((path, None))
}

pub(super) fn copy_child_entry(
    source: &ObservedEntry,
    correlated_entry: bool,
    enter: bool,
    recorded: bool,
    can_descend: bool,
    services: &EntryServices<'_>,
) -> (Option<ObservedEntry>, Option<Permit>) {
    if !correlated_entry || !enter || recorded || !can_descend {
        return (None, None);
    }
    if let Some((mut permit, copy)) = correlated_budget::entry_heap_capacity(source)
        .and_then(|bytes| bytes.checked_add(std::mem::size_of::<ObservedEntry>()))
        .and_then(|requested| {
            let permit = services.shared.correlated_budget.reserve(requested)?;
            let copy = correlated_budget::try_clone_entry(source)?;
            Some((permit, copy))
        })
    {
        let actual = correlated_budget::entry_heap_capacity(&copy)
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<ObservedEntry>()));
        if actual.is_some_and(|actual| permit.resize(actual)) {
            return (Some(copy), Some(permit));
        }
    }
    // This is the ordinary F02 child copy. Correlation budget must not stop traversal.
    (Some(source.clone()), None)
}

pub(super) fn publish_item(
    frame: &mut Frame,
    timed: &mut Option<TimedObservedEntry>,
    payload_permit: &mut Option<Permit>,
    path: &[Vec<u8>],
    admission: Admission,
    correlated_entry: bool,
    services: &EntryServices<'_>,
) -> Result<(), ()> {
    if !correlated_entry {
        let source = timed.as_ref().ok_or(())?;
        services
            .sender
            .send(Event::Item(
                frame.root_index,
                path.to_vec(),
                Box::new(source.clone()),
                admission,
                None,
            ))
            .map_err(|_| ())?;
        return Ok(());
    }
    let item_timed = timed.take().ok_or(())?;
    let mut correlated_item = None;
    if let Some(mut item_permit) = correlated_budget::path_clone_reservation(path)
        .and_then(|requested| services.shared.correlated_budget.reserve(requested))
        && let Some(item_path) = correlated_budget::try_clone_path(path)
        && let Some(actual) =
            correlated_budget::path_heap_capacity(&item_path, item_path.capacity())
        && item_permit.resize(actual)
        && let Some(mut held_permit) = payload_permit.take()
    {
        if held_permit.merge(item_permit) {
            correlated_item = Some((item_path, held_permit));
        } else {
            *payload_permit = Some(held_permit);
        }
    }
    if let Some((item_path, permit)) = correlated_item {
        services
            .sender
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
        services
            .sender
            .send(Event::Item(
                frame.root_index,
                path.to_vec(),
                Box::new(item_timed),
                admission,
                payload_permit.take(),
            ))
            .map_err(|_| ())?;
    }
    Ok(())
}

pub(super) fn register_child_attempt(
    frame: &Frame,
    child_attempt: &mut Attempt,
    enter: bool,
    can_descend: bool,
    path: &[Vec<u8>],
    services: &EntryServices<'_>,
) {
    if !(enter
        && can_descend
        && matches!(services.policy, Policy::BeforeMetadata(_))
        && child_attempt.token().is_none())
    {
        return;
    }
    if !child_attempt.prepare(&services.shared.correlated_budget) {
        return;
    }
    let Some(attempt_id) = correlated_budget::next_attempt_id() else {
        child_attempt.fallback_to_unselected();
        return;
    };
    match correlated_transport::begin(
        services.sender,
        services.shared,
        services.context,
        frame.root_index,
        path,
        attempt_id,
    ) {
        Ok(token) => child_attempt.bind(token),
        Err(_) => child_attempt.fallback_to_unselected(),
    }
}

pub(super) fn record_final(
    frame: &mut Frame,
    child_id: Option<u64>,
    recorded: bool,
    admission: Admission,
    metadata_failed: bool,
    child_token: Option<du_analysis_index::FolderAttemptToken>,
    services: &EntryServices<'_>,
) {
    if recorded {
        return;
    }
    let (Some(child_id), Some(token), Some(outcome)) = (
        child_id,
        frame.correlated.token(),
        correlated_final(admission, metadata_failed, child_token),
    ) else {
        return;
    };
    if !frame.correlated.saw_callback(child_id) {
        return;
    }
    let Some(seq) = frame.correlated.final_sequence() else {
        frame.correlated.evidence_failed();
        return;
    };
    let input = du_analysis_index::FolderAttemptInput::Final { child_id, outcome };
    if correlated_transport::record(
        services.sender,
        services.shared,
        services.context,
        token,
        seq,
        input,
    )
    .is_err()
    {
        frame.correlated.evidence_failed();
    }
}

#[cfg(test)]
mod tests {
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
