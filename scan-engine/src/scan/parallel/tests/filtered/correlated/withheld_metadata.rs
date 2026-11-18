use super::super::*;
use super::fixtures::*;

#[test]
fn withheld_correlated_metadata_keeps_the_item_and_does_not_prove_the_attempt()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = worker_test_lock();
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
    let outcome = crate::scan::parallel::collect_open_forest_filtered(
        build,
        vec![Box::new(WithheldMetadataRoot { step: 0 })],
        &ids,
        &context,
        1,
        &|_, _, _| FilterDecision::Admit,
        crate::scan::parallel::DevicePlan {
            groups: vec![0],
            limit: 1,
        },
    );
    assert!(outcome.result_version_id.is_some());
    let result = sink
        .pin()
        .map_err(|error| format!("{error:?}"))?
        .ok_or("missing")?;
    let item = result
        .items()
        .iter()
        .find(|item| item.name == b"keep")
        .ok_or("ordinary item was dropped")?;
    assert!(item.parent.is_some());
    assert!(
        result
            .scan_issues()
            .iter()
            .any(|issue| issue.relative == [b"keep".to_vec()])
    );
    assert_eq!(result.unread_folder_counts().cancelled, 0);
    let attempts = result.folder_attempt_summaries().ok_or("missing attempt")?;
    assert_eq!(attempts.len(), 1);
    assert_ne!(
        attempts[0].status,
        du_analysis_index::FolderAttemptStatus::EndedUnproven
    );
    Ok(())
}
