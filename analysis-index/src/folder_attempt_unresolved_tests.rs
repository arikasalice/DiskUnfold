use super::*;

#[test]
fn unresolved_final_and_undecidable_filter_hold_at_end() -> AttemptTestResult {
    let store = store()?;
    let mut unresolved_final = AttemptLedger::new(7);
    start(
        &mut unresolved_final,
        &store,
        Some(ChildCorrelationCapability::StableChildId),
    );
    unresolved_final.record(
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
    unresolved_final.record(final_event(2, 0, DirectoryChildFinal::Unresolved), &store);
    unresolved_final.record(end(3, 1, 1), &store);
    assert_eq!(
        unresolved_final
            .attempts
            .get(&key())
            .map(|state| state.status),
        Some(AttemptStatus::Held)
    );

    let mut undecidable = AttemptLedger::new(7);
    start(
        &mut undecidable,
        &store,
        Some(ChildCorrelationCapability::StableChildId),
    );
    undecidable.record(
        seen(
            1,
            0,
            Some(0),
            b"a",
            du_core_contracts::scan_observation::FilterDecision::Undecidable,
            CallbackPhase::Unknown,
        ),
        &store,
    );
    undecidable.record(end(2, 1, 1), &store);
    assert_eq!(
        undecidable.attempts.get(&key()).map(|state| state.status),
        Some(AttemptStatus::Held)
    );
    Ok(())
}
