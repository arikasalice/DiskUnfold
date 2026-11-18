use super::*;

#[test]
fn callback_and_final_order_is_irrelevant_only_with_unique_child_ids() -> AttemptTestResult {
    let store = store()?;
    let mut ledger = AttemptLedger::new(7);
    start(
        &mut ledger,
        &store,
        Some(ChildCorrelationCapability::StableChildId),
    );
    // callback_id, child_id, and event_seq have distinct scopes. The two callback
    // IDs map to child IDs in reverse order; the Final frames arrive in child-ID order.
    ledger.record(
        seen(
            1,
            0,
            Some(1),
            b"second",
            du_core_contracts::scan_observation::FilterDecision::Admit,
            CallbackPhase::BeforeMetadata,
        ),
        &store,
    );
    ledger.record(
        seen(
            2,
            1,
            Some(0),
            b"first",
            du_core_contracts::scan_observation::FilterDecision::Admit,
            CallbackPhase::BeforeMetadata,
        ),
        &store,
    );
    ledger.record(
        final_event(
            3,
            0,
            DirectoryChildFinal::Recorded {
                kind: ObservedKind::File,
                child_attempt_id: None,
            },
        ),
        &store,
    );
    ledger.record(
        final_event(
            4,
            1,
            DirectoryChildFinal::Recorded {
                kind: ObservedKind::File,
                child_attempt_id: None,
            },
        ),
        &store,
    );
    ledger.record(end(5, 2, 2), &store);
    assert_eq!(
        ledger.attempts.get(&key()).map(|state| state.status),
        Some(AttemptStatus::EndedUnproven)
    );

    // Two distinct callbacks claiming one child are not zipped by position or
    // guessed to be duplicate delivery; the candidate requires unique correlation.
    let mut ambiguous = AttemptLedger::new(7);
    start(
        &mut ambiguous,
        &store,
        Some(ChildCorrelationCapability::StableChildId),
    );
    for (event_seq, callback_id) in [(1, 0), (2, 1)] {
        ambiguous.record(
            seen(
                event_seq,
                callback_id,
                Some(0),
                b"same-child",
                du_core_contracts::scan_observation::FilterDecision::Admit,
                CallbackPhase::BeforeMetadata,
            ),
            &store,
        );
    }
    ambiguous.record(
        final_event(
            3,
            0,
            DirectoryChildFinal::Recorded {
                kind: ObservedKind::File,
                child_attempt_id: None,
            },
        ),
        &store,
    );
    ambiguous.record(end(4, 2, 1), &store);
    assert_eq!(
        ambiguous.attempts.get(&key()).map(|state| state.status),
        Some(AttemptStatus::Held)
    );
    Ok(())
}

#[test]
fn eof_child_count_includes_excluded_terminal_and_requires_dense_seen_ids() -> AttemptTestResult {
    let store = store()?;
    let mut excluded = AttemptLedger::new(7);
    start(
        &mut excluded,
        &store,
        Some(ChildCorrelationCapability::StableChildId),
    );
    excluded.record(
        seen(
            1,
            0,
            Some(0),
            b"admitted",
            du_core_contracts::scan_observation::FilterDecision::Admit,
            CallbackPhase::BeforeMetadata,
        ),
        &store,
    );
    excluded.record(
        seen(
            2,
            1,
            Some(1),
            b"excluded",
            du_core_contracts::scan_observation::FilterDecision::Exclude,
            CallbackPhase::BeforeMetadata,
        ),
        &store,
    );
    excluded.record(
        final_event(
            3,
            0,
            DirectoryChildFinal::Recorded {
                kind: ObservedKind::File,
                child_attempt_id: None,
            },
        ),
        &store,
    );
    // Exclude callback itself is terminal; no separate Final is emitted or counted.
    excluded.record(end(4, 2, 2), &store);
    assert_eq!(
        excluded.attempts.get(&key()).map(|state| state.status),
        Some(AttemptStatus::EndedUnproven)
    );

    let mut missing = AttemptLedger::new(7);
    start(
        &mut missing,
        &store,
        Some(ChildCorrelationCapability::StableChildId),
    );
    for (event_seq, callback_id, child_id, name) in
        [(1, 0, 0, &b"first"[..]), (2, 1, 2, &b"last"[..])]
    {
        missing.record(
            seen(
                event_seq,
                callback_id,
                Some(child_id),
                name,
                du_core_contracts::scan_observation::FilterDecision::Exclude,
                CallbackPhase::AfterMetadata,
            ),
            &store,
        );
    }
    missing.record(end(3, 2, 3), &store);
    assert_eq!(
        missing.attempts.get(&key()).map(|state| state.status),
        Some(AttemptStatus::Held)
    );
    Ok(())
}

#[test]
fn duplicate_exclude_or_exclude_followed_by_final_holds_attempt() -> AttemptTestResult {
    let store = store()?;
    let mut duplicate = AttemptLedger::new(7);
    start(
        &mut duplicate,
        &store,
        Some(ChildCorrelationCapability::StableChildId),
    );
    duplicate.record(
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
    duplicate.record(
        seen(
            2,
            0,
            Some(0),
            b"excluded",
            du_core_contracts::scan_observation::FilterDecision::Exclude,
            CallbackPhase::BeforeMetadata,
        ),
        &store,
    );
    duplicate.record(end(3, 1, 1), &store);
    assert_eq!(
        duplicate.attempts.get(&key()).map(|state| state.status),
        Some(AttemptStatus::Held)
    );

    let mut followed_by_final = AttemptLedger::new(7);
    start(
        &mut followed_by_final,
        &store,
        Some(ChildCorrelationCapability::StableChildId),
    );
    followed_by_final.record(
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
    followed_by_final.record(
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
    followed_by_final.record(end(3, 1, 1), &store);
    assert_eq!(
        followed_by_final
            .attempts
            .get(&key())
            .map(|state| state.status),
        Some(AttemptStatus::Held)
    );
    Ok(())
}

#[test]
fn boundary_and_revisit_are_terminal_admission_outcomes_not_gaps() -> AttemptTestResult {
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
            b"mount",
            du_core_contracts::scan_observation::FilterDecision::Admit,
            CallbackPhase::BeforeMetadata,
        ),
        &store,
    );
    ledger.record(
        final_event(
            2,
            0,
            DirectoryChildFinal::Boundary {
                kind: crate::folder_marks::BoundaryKind::OtherVolume,
            },
        ),
        &store,
    );
    ledger.record(
        seen(
            3,
            1,
            Some(1),
            b"revisit",
            du_core_contracts::scan_observation::FilterDecision::Admit,
            CallbackPhase::BeforeMetadata,
        ),
        &store,
    );
    ledger.record(final_event(4, 1, DirectoryChildFinal::Revisit), &store);
    ledger.record(end(5, 2, 2), &store);
    assert_eq!(
        ledger.attempts.get(&key()).map(|state| state.status),
        Some(AttemptStatus::EndedUnproven)
    );
    Ok(())
}

#[test]
fn duplicate_raw_name_with_distinct_child_ids_holds_attempt() -> AttemptTestResult {
    let store = store()?;
    let mut ledger = AttemptLedger::new(7);
    start(
        &mut ledger,
        &store,
        Some(ChildCorrelationCapability::StableChildId),
    );
    for (event_seq, callback_id, child_id) in [(1, 0, 0), (2, 1, 1)] {
        ledger.record(
            seen(
                event_seq,
                callback_id,
                Some(child_id),
                b"duplicate",
                du_core_contracts::scan_observation::FilterDecision::Admit,
                CallbackPhase::BeforeMetadata,
            ),
            &store,
        );
    }
    for (event_seq, child_id) in [(3, 0), (4, 1)] {
        ledger.record(
            final_event(
                event_seq,
                child_id,
                DirectoryChildFinal::Recorded {
                    kind: ObservedKind::File,
                    child_attempt_id: None,
                },
            ),
            &store,
        );
    }
    ledger.record(end(5, 2, 2), &store);
    assert_eq!(
        ledger.attempts.get(&key()).map(|state| state.status),
        Some(AttemptStatus::Held)
    );
    Ok(())
}

#[test]
fn empty_directory_end_is_observed_but_not_size_promotion() -> AttemptTestResult {
    let store = store()?;
    let mut ledger = AttemptLedger::new(7);
    start(
        &mut ledger,
        &store,
        Some(ChildCorrelationCapability::StableChildId),
    );
    ledger.record(end(1, 0, 0), &store);
    assert_eq!(
        ledger.attempts.get(&key()).map(|state| state.status),
        Some(AttemptStatus::EndedUnproven)
    );
    Ok(())
}

#[test]
fn progress_without_discard_is_not_a_gap_but_unresolved_seen_at_end_is() -> AttemptTestResult {
    let store = store()?;
    let mut complete_after_progress = AttemptLedger::new(7);
    start(
        &mut complete_after_progress,
        &store,
        Some(ChildCorrelationCapability::StableChildId),
    );
    complete_after_progress.record(
        DirectoryAttemptEvent::Progress {
            key: key(),
            event_seq: 1,
        },
        &store,
    );
    complete_after_progress.record(
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
    complete_after_progress.record(
        final_event(
            3,
            0,
            DirectoryChildFinal::Recorded {
                kind: ObservedKind::File,
                child_attempt_id: None,
            },
        ),
        &store,
    );
    complete_after_progress.record(end(4, 1, 1), &store);
    assert_eq!(
        complete_after_progress
            .attempts
            .get(&key())
            .map(|state| state.status),
        Some(AttemptStatus::EndedUnproven)
    );

    let mut unresolved = AttemptLedger::new(7);
    start(
        &mut unresolved,
        &store,
        Some(ChildCorrelationCapability::StableChildId),
    );
    unresolved.record(
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
    unresolved.record(end(2, 1, 1), &store);
    assert_eq!(
        unresolved.attempts.get(&key()).map(|state| state.status),
        Some(AttemptStatus::Held)
    );
    Ok(())
}
