use super::*;

#[test]
fn snapshot_is_immutable_after_later_attempt_validation_failure() -> AttemptTestResult {
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
            du_core_contracts::scan_observation::FilterDecision::Admit,
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
    let built = crate::tree::build_items(&store, 1, true).map_err(|error| format!("{error:?}"))?;
    let history = Arc::new(HistoryBudget::default());
    let pinned = ledger
        .snapshot(&built, &history)
        .ok_or("snapshot was not created")?;

    ledger.record(
        seen(
            1,
            0,
            Some(0),
            b"changed",
            du_core_contracts::scan_observation::FilterDecision::Admit,
            CallbackPhase::BeforeMetadata,
        ),
        &store,
    );
    assert_eq!(
        ledger.attempts.get(&key()).map(|state| state.status),
        Some(AttemptStatus::Held)
    );
    assert_eq!(pinned.attempts[0].status, AttemptStatus::EndedUnproven);
    assert!(matches!(
        &pinned.attempts[0].events[1].payload,
        crate::folder_attempt_contract::AttemptEventPayload::CallbackSeen {
            raw_name,
            ..
        } if raw_name == b"a"
    ));
    Ok(())
}

#[test]
fn history_budget_is_reserved_before_snapshot_copy_and_released_after_drop() -> AttemptTestResult {
    // Checks requested-byte accounting only; allocator overhead and live ingestion are excluded.
    let store = store()?;
    let mut ledger = AttemptLedger::new(7);
    start(&mut ledger, &store, None);
    let built = crate::tree::build_items(&store, 1, true).map_err(|error| format!("{error:?}"))?;
    let history = Arc::new(HistoryBudget::default());
    let full = history
        .reserve(crate::folder_attempt_history::MAX_HISTORY_ACCOUNTED_BYTES)
        .ok_or("full history reservation failed")?;
    assert!(ledger.snapshot(&built, &history).is_none());
    drop(full);
    let pinned = ledger
        .snapshot(&built, &history)
        .ok_or("snapshot reservation was not available")?;
    assert!(history.used_bytes() > 0);
    drop(pinned);
    assert_eq!(history.used_bytes(), 0);
    Ok(())
}
