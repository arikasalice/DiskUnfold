use super::*;

#[test]
fn event_sequence_and_child_id_have_separate_scopes_and_exact_replay_rules() -> AttemptTestResult {
    let store = store()?;
    let mut ledger = AttemptLedger::new(7);
    start(
        &mut ledger,
        &store,
        Some(ChildCorrelationCapability::StableChildId),
    );
    let callback = seen(
        1,
        0,
        Some(0),
        b"a",
        du_core_contracts::scan_observation::FilterDecision::Admit,
        CallbackPhase::BeforeMetadata,
    );
    ledger.record(callback.clone(), &store);
    ledger.record(callback.clone(), &store);
    let final_frame = final_event(
        2,
        0,
        DirectoryChildFinal::Recorded {
            kind: ObservedKind::File,
            child_attempt_id: None,
        },
    );
    ledger.record(final_frame.clone(), &store);
    ledger.record(final_frame, &store);
    let barrier = end(3, 1, 1);
    ledger.record(barrier.clone(), &store);
    ledger.record(barrier, &store);
    ledger.record(callback, &store);

    let state = ledger.attempts.get(&key()).ok_or("attempt is missing")?;
    assert_eq!(state.status, AttemptStatus::EndedUnproven);
    assert_eq!(state.events.len(), 3);
    assert_eq!(state.events[1].event_seq, 1);
    assert_eq!(state.events[2].event_seq, 2);
    Ok(())
}

#[test]
fn same_sequence_identical_exclude_replay_is_one_terminal() -> AttemptTestResult {
    let store = store()?;
    let mut ledger = AttemptLedger::new(7);
    start(
        &mut ledger,
        &store,
        Some(ChildCorrelationCapability::StableChildId),
    );
    let excluded = seen(
        1,
        0,
        Some(0),
        b"excluded",
        du_core_contracts::scan_observation::FilterDecision::Exclude,
        CallbackPhase::BeforeMetadata,
    );
    ledger.record(excluded.clone(), &store);
    ledger.record(excluded, &store);
    ledger.record(end(2, 1, 1), &store);

    let state = ledger.attempts.get(&key()).ok_or("attempt is missing")?;
    assert_eq!(state.status, AttemptStatus::EndedUnproven);
    assert_eq!(state.events.len(), 2);
    Ok(())
}

#[test]
fn equal_payload_at_a_different_sequence_is_a_conflict() -> AttemptTestResult {
    let store = store()?;
    let mut repeated_callback = AttemptLedger::new(7);
    start(
        &mut repeated_callback,
        &store,
        Some(ChildCorrelationCapability::StableChildId),
    );
    repeated_callback.record(
        seen(
            1,
            0,
            Some(0),
            b"a",
            du_core_contracts::scan_observation::FilterDecision::Admit,
            CallbackPhase::BeforeMetadata,
        ),
        &store,
    );
    repeated_callback.record(
        seen(
            2,
            0,
            Some(0),
            b"a",
            du_core_contracts::scan_observation::FilterDecision::Admit,
            CallbackPhase::BeforeMetadata,
        ),
        &store,
    );
    repeated_callback.record(end(3, 1, 1), &store);
    assert_eq!(
        repeated_callback
            .attempts
            .get(&key())
            .map(|state| state.status),
        Some(AttemptStatus::Held)
    );

    let mut repeated_final = AttemptLedger::new(7);
    start(
        &mut repeated_final,
        &store,
        Some(ChildCorrelationCapability::StableChildId),
    );
    repeated_final.record(
        seen(
            1,
            0,
            Some(0),
            b"a",
            du_core_contracts::scan_observation::FilterDecision::Admit,
            CallbackPhase::BeforeMetadata,
        ),
        &store,
    );
    let classification = DirectoryChildFinal::Recorded {
        kind: ObservedKind::File,
        child_attempt_id: None,
    };
    repeated_final.record(final_event(2, 0, classification.clone()), &store);
    repeated_final.record(final_event(3, 0, classification), &store);
    repeated_final.record(end(4, 1, 1), &store);
    assert_eq!(
        repeated_final
            .attempts
            .get(&key())
            .map(|state| state.status),
        Some(AttemptStatus::Held)
    );
    Ok(())
}

#[test]
fn conflicting_replay_or_event_sequence_gap_holds_attempt() -> AttemptTestResult {
    let store = store()?;
    let mut conflicting = AttemptLedger::new(7);
    start(
        &mut conflicting,
        &store,
        Some(ChildCorrelationCapability::StableChildId),
    );
    conflicting.record(
        seen(
            1,
            0,
            Some(0),
            b"a",
            du_core_contracts::scan_observation::FilterDecision::Admit,
            CallbackPhase::BeforeMetadata,
        ),
        &store,
    );
    conflicting.record(
        seen(
            1,
            0,
            Some(0),
            b"b",
            du_core_contracts::scan_observation::FilterDecision::Admit,
            CallbackPhase::BeforeMetadata,
        ),
        &store,
    );
    assert_eq!(
        conflicting.attempts.get(&key()).map(|state| state.status),
        Some(AttemptStatus::Held)
    );

    let mut missing = AttemptLedger::new(7);
    start(
        &mut missing,
        &store,
        Some(ChildCorrelationCapability::StableChildId),
    );
    missing.record(
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
    assert_eq!(
        missing.attempts.get(&key()).map(|state| state.status),
        Some(AttemptStatus::Held)
    );

    let mut conflicting_final = AttemptLedger::new(7);
    start(
        &mut conflicting_final,
        &store,
        Some(ChildCorrelationCapability::StableChildId),
    );
    conflicting_final.record(
        seen(
            1,
            0,
            Some(0),
            b"a",
            du_core_contracts::scan_observation::FilterDecision::Admit,
            CallbackPhase::BeforeMetadata,
        ),
        &store,
    );
    conflicting_final.record(
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
    conflicting_final.record(
        final_event(
            3,
            0,
            DirectoryChildFinal::Recorded {
                kind: ObservedKind::Directory,
                child_attempt_id: Some(10),
            },
        ),
        &store,
    );
    conflicting_final.record(end(4, 1, 1), &store);
    assert_eq!(
        conflicting_final
            .attempts
            .get(&key())
            .map(|state| state.status),
        Some(AttemptStatus::Held)
    );

    let mut conflicting_end = AttemptLedger::new(7);
    start(
        &mut conflicting_end,
        &store,
        Some(ChildCorrelationCapability::StableChildId),
    );
    conflicting_end.record(end(1, 0, 0), &store);
    conflicting_end.record(end(1, 0, 1), &store);
    assert_eq!(
        conflicting_end
            .attempts
            .get(&key())
            .map(|state| state.status),
        Some(AttemptStatus::Held)
    );
    Ok(())
}

#[test]
fn unknown_or_unjoinable_callback_capability_holds_at_end() -> AttemptTestResult {
    let store = store()?;
    let mut legacy = AttemptLedger::new(7);
    start(&mut legacy, &store, None);
    legacy.record(end(1, 0, 0), &store);
    assert_eq!(
        legacy.attempts.get(&key()).map(|state| state.status),
        Some(AttemptStatus::Held)
    );

    let mut explicit_unknown = AttemptLedger::new(7);
    start(
        &mut explicit_unknown,
        &store,
        Some(ChildCorrelationCapability::Unknown),
    );
    explicit_unknown.record(end(1, 0, 0), &store);
    assert_eq!(
        explicit_unknown
            .attempts
            .get(&key())
            .map(|state| state.status),
        Some(AttemptStatus::Held)
    );

    let mut unjoined = AttemptLedger::new(7);
    start(
        &mut unjoined,
        &store,
        Some(ChildCorrelationCapability::StableChildId),
    );
    unjoined.record(
        seen(
            1,
            0,
            None,
            b"a",
            du_core_contracts::scan_observation::FilterDecision::Admit,
            CallbackPhase::Unknown,
        ),
        &store,
    );
    unjoined.record(
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
    unjoined.record(end(3, 1, 1), &store);
    assert_eq!(
        unjoined.attempts.get(&key()).map(|state| state.status),
        Some(AttemptStatus::Held)
    );
    Ok(())
}
