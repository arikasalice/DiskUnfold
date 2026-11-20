use super::super::*;
use super::fixtures::*;

#[test]
fn correlated_worker_records_observation_without_promoting_attempt()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = worker_test_lock();
    use du_core_contracts::scan_observation::FilterCallbackPhase;
    for phase in [
        FilterCallbackPhase::BeforeMetadata,
        FilterCallbackPhase::AfterMetadata,
    ] {
        let ids = Ids(AtomicUsize::new(1));
        let context = Context {
            completed: Arc::new(AtomicUsize::new(0)),
            abandoned: Arc::new(AtomicUsize::new(0)),
        };
        let sink = IngestSink::default();
        let build = sink.open_forest(
            ids.next_id(),
            vec![super::super::super::observed_root("C:\\R0", 1)],
        );
        let roots: Vec<Box<dyn AnchoredDirectory>> =
            vec![Box::new(CorrelatedRoot { step: 0, phase })];
        let outcome = crate::scan::parallel::collect_open_forest_filtered(
            build,
            roots,
            &ids,
            &context,
            1,
            &|_, _, _| FilterDecision::Admit,
            crate::scan::parallel::DevicePlan {
                groups: vec![0],
                limit: 4,
            },
        );
        assert!(outcome.result_version_id.is_some());
        let result = sink
            .pin()
            .map_err(|e| format!("{e:?}"))?
            .ok_or("missing result")?;
        assert_eq!(
            result
                .items()
                .iter()
                .filter(|item| item.name == b"keep")
                .count(),
            1
        );
        let attempts = result.folder_attempt_summaries().ok_or("missing attempt")?;
        assert_eq!(attempts.len(), 1);
        // EndedUnproven does not promote to Exact.
        assert_eq!(
            attempts[0].status,
            du_analysis_index::FolderAttemptStatus::EndedUnproven
        );
    }
    Ok(())
}

/// F02 #218: the production entry, for a scope without exclusion rules, enumerates with
/// correlated evidence where the adapter supports it and counts the attempts that ended.
#[test]
fn the_production_entry_records_attempt_evidence_without_exclusion_rules()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = worker_test_lock();
    use du_core_contracts::scan_observation::FilterCallbackPhase;
    let ids = Ids(AtomicUsize::new(1));
    let context = Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    };
    let sink = IngestSink::default();
    let build = sink.open_forest(
        ids.next_id(),
        vec![super::super::super::observed_root("C:\\R0", 1)],
    );
    let roots: Vec<Box<dyn AnchoredDirectory>> = vec![Box::new(CorrelatedRoot {
        step: 0,
        phase: FilterCallbackPhase::BeforeMetadata,
    })];
    let diagnostics = crate::scan_job::ScanDiagnostics::default();
    let outcome = crate::scan::parallel::collect_open_forest_diagnostic(
        build,
        roots,
        &ids,
        &context,
        1,
        None,
        (
            crate::scan::parallel::DevicePlan {
                groups: vec![0],
                limit: 4,
            },
            Some(&diagnostics),
            None,
            None,
        ),
    );
    assert!(outcome.result_version_id.is_some());
    let result = sink
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing result")?;
    let attempts = result.folder_attempt_summaries().ok_or("missing attempt")?;
    assert_eq!(attempts.len(), 1);
    assert_eq!(diagnostics.ended_folder_attempts(), 1);
    Ok(())
}
