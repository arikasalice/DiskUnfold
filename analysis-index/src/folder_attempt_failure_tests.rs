use super::*;

#[test]
fn exclude_is_terminal_for_the_child() -> AttemptTestResult {
    let store = store()?;
    let mut ledger = AttemptLedger::new(7);
    start(
        &mut ledger,
        &store,
        Some(ChildCorrelationCapability::StableChildId),
    );
    ledger.record(
        seen(
            1,
            0,
            Some(0),
            b"a",
            du_core_contracts::scan_observation::FilterDecision::Exclude,
            CallbackPhase::BeforeMetadata,
        ),
        &store,
    );
    ledger.record(
        final_event(
            2,
            0,
            DirectoryChildFinal::Recorded {
                kind: ObservedKind::File,
                child_attempt_id: None,
            },
        ),
        &store,
    );
    ledger.record(end(3, 1, 1), &store);
    assert_eq!(
        ledger.attempts.get(&key()).map(|state| state.status),
        Some(AttemptStatus::Held)
    );
    Ok(())
}

#[test]
fn pending_clear_metadata_error_transport_failure_and_cancel_hold() -> AttemptTestResult {
    let store = store()?;
    let mut pending = AttemptLedger::new(7);
    start(
        &mut pending,
        &store,
        Some(ChildCorrelationCapability::StableChildId),
    );
    pending.record(
        DirectoryAttemptEvent::PendingCleared {
            key: key(),
            event_seq: 1,
            child_id: 0,
        },
        &store,
    );
    assert_eq!(
        pending.attempts.get(&key()).map(|state| state.status),
        Some(AttemptStatus::Held)
    );

    let variants = [
        DirectoryAttemptEvent::MetadataError {
            key: key(),
            event_seq: 1,
        },
        DirectoryAttemptEvent::TransportFailure {
            key: key(),
            event_seq: 1,
        },
        DirectoryAttemptEvent::Cancelled {
            key: key(),
            event_seq: 1,
        },
    ];
    for event in variants {
        let mut ledger = AttemptLedger::new(7);
        start(
            &mut ledger,
            &store,
            Some(ChildCorrelationCapability::StableChildId),
        );
        ledger.record(event, &store);
        assert_eq!(
            ledger.attempts.get(&key()).map(|state| state.status),
            Some(AttemptStatus::Held)
        );
    }
    Ok(())
}

#[test]
fn macos_after_metadata_filter_and_directory_end_do_not_prove_size_completion() -> AttemptTestResult
{
    let store = store()?;
    let mut ledger = AttemptLedger::new(7);
    start(
        &mut ledger,
        &store,
        Some(ChildCorrelationCapability::StableChildId),
    );
    ledger.record(
        seen(
            1,
            0,
            Some(0),
            b"dir",
            du_core_contracts::scan_observation::FilterDecision::Exclude,
            CallbackPhase::AfterMetadata,
        ),
        &store,
    );
    // Exclude is the terminal callback and contributes one child ID, without a Final.
    ledger.record(end(2, 1, 1), &store);
    assert_eq!(
        ledger.attempts.get(&key()).map(|state| state.status),
        Some(AttemptStatus::EndedUnproven)
    );

    let mut parent = AttemptLedger::new(7);
    start(
        &mut parent,
        &store,
        Some(ChildCorrelationCapability::StableChildId),
    );
    parent.record(
        seen(
            1,
            0,
            Some(0),
            b"nested",
            du_core_contracts::scan_observation::FilterDecision::Admit,
            CallbackPhase::BeforeMetadata,
        ),
        &store,
    );
    parent.record(
        final_event(
            2,
            0,
            DirectoryChildFinal::Recorded {
                kind: ObservedKind::Directory,
                child_attempt_id: Some(99),
            },
        ),
        &store,
    );
    parent.record(end(3, 1, 1), &store);
    assert_eq!(
        parent.attempts.get(&key()).map(|state| state.status),
        Some(AttemptStatus::EndedUnproven)
    );
    // A direct-child End does not complete the unrepresented descendant attempt.
    Ok(())
}

#[test]
fn eof_child_count_includes_excluded_ids_and_missing_tail_holds() -> AttemptTestResult {
    let store = store()?;
    let mut complete = AttemptLedger::new(7);
    start(
        &mut complete,
        &store,
        Some(ChildCorrelationCapability::StableChildId),
    );
    complete.record(
        seen(
            1,
            0,
            Some(0),
            b"excluded",
            du_core_contracts::scan_observation::FilterDecision::Exclude,
            CallbackPhase::BeforeMetadata,
        ),
        &store,
    );
    complete.record(
        seen(
            2,
            1,
            Some(1),
            b"included",
            du_core_contracts::scan_observation::FilterDecision::Admit,
            CallbackPhase::BeforeMetadata,
        ),
        &store,
    );
    complete.record(
        final_event(
            3,
            1,
            DirectoryChildFinal::Recorded {
                kind: ObservedKind::File,
                child_attempt_id: None,
            },
        ),
        &store,
    );
    // Transport frames and adapter child IDs have independent exclusive watermarks.
    complete.record(end(4, 2, 2), &store);
    assert_eq!(
        complete.attempts.get(&key()).map(|state| state.status),
        Some(AttemptStatus::EndedUnproven)
    );

    let mut missing_tail = AttemptLedger::new(7);
    start(
        &mut missing_tail,
        &store,
        Some(ChildCorrelationCapability::StableChildId),
    );
    missing_tail.record(
        seen(
            1,
            0,
            Some(0),
            b"only-one-delivered",
            du_core_contracts::scan_observation::FilterDecision::Exclude,
            CallbackPhase::BeforeMetadata,
        ),
        &store,
    );
    missing_tail.record(end(2, 1, 2), &store);
    assert_eq!(
        missing_tail.attempts.get(&key()).map(|state| state.status),
        Some(AttemptStatus::Held)
    );
    Ok(())
}
