use super::super::*;
use super::fixtures::*;

#[test]
fn path_budget_exhaustion_keeps_item_and_holds_correlated_attempt()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = worker_test_lock();
    let ids = Ids(AtomicUsize::new(1));
    let context = Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    };
    let sink = IngestSink::default();
    let budget = crate::scan::parallel::correlated_budget::CorrelatedBudget::with_limits(
        8,
        16 * 1024 * 1024,
    );
    let build = sink.open_forest(
        ids.next_id(),
        vec![super::super::super::observed_root("C:\\R0", 1)],
    );
    let outcome = crate::scan::parallel::collect_open_forest_filtered_with_budget(
        build,
        vec![Box::new(BudgetExhaustionRoot {
            step: 0,
            child: false,
            exhaust: true,
            budget: Arc::clone(&budget),
        })],
        &ids,
        &context,
        1,
        &|_, _, _| FilterDecision::Admit,
        (
            crate::scan::parallel::DevicePlan {
                groups: vec![0],
                limit: 1,
            },
            budget,
        ),
    );
    assert!(outcome.result_version_id.is_some());
    let result = sink
        .pin()
        .map_err(|error| format!("{error:?}"))?
        .ok_or("missing")?;
    assert_eq!(
        result.completeness(),
        du_core_contracts::result::ResultCompleteness::Complete
    );
    assert!(result.items().iter().any(|item| item.name == b"folder"));
    assert!(result.items().iter().any(|item| item.name == b"leaf"));
    assert_eq!(result.unread_folder_counts().cancelled, 0);
    assert_eq!(
        result
            .folder_attempt_summaries()
            .ok_or("missing attempt summary")?[0]
            .status,
        du_analysis_index::FolderAttemptStatus::Held
    );
    Ok(())
}

#[test]
fn oversized_directory_observation_holds_evidence_but_continues_f02_traversal()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = worker_test_lock();
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let context = Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    };
    let build = sink.open_forest(
        ids.next_id(),
        vec![super::super::super::observed_root("C:\\R0", 1)],
    );
    let outcome = crate::scan::parallel::collect_open_forest_filtered(
        build,
        vec![Box::new(ReturnedDirectoryRoot {
            step: 0,
            child: false,
        })],
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
    assert_eq!(
        result.completeness(),
        du_core_contracts::result::ResultCompleteness::Complete
    );
    assert!(result.items().iter().any(|item| item.name == b"folder"));
    assert!(result.items().iter().any(|item| item.name == b"leaf"));
    assert_eq!(result.unread_folder_counts().cancelled, 0);
    assert!(result.scan_issues().is_empty());
    assert!(
        result
            .folder_attempt_summaries()
            .ok_or("missing attempt summary")?
            .iter()
            .any(|summary| summary.status == du_analysis_index::FolderAttemptStatus::Held)
    );
    Ok(())
}

struct RejectChildAttempts(bool);

impl RejectChildAttempts {
    fn enable() -> Self {
        let previous = crate::scan::parallel::publisher::REJECT_CHILD_ATTEMPTS
            .with(|enabled| enabled.replace(true));
        Self(previous)
    }
}

impl Drop for RejectChildAttempts {
    fn drop(&mut self) {
        crate::scan::parallel::publisher::REJECT_CHILD_ATTEMPTS.with(|enabled| enabled.set(self.0));
    }
}

#[test]
fn refused_child_attempt_keeps_child_traversal_and_results()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = worker_test_lock();
    let _reject = RejectChildAttempts::enable();
    let ids = Ids(AtomicUsize::new(1));
    let context = Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    };
    let sink = IngestSink::default();
    let budget = crate::scan::parallel::correlated_budget::CorrelatedBudget::with_limits(
        8,
        16 * 1024 * 1024,
    );
    let build = sink.open_forest(
        ids.next_id(),
        vec![super::super::super::observed_root("C:\\R0", 1)],
    );
    let outcome = crate::scan::parallel::collect_open_forest_filtered_with_budget(
        build,
        vec![Box::new(BudgetExhaustionRoot {
            step: 0,
            child: false,
            exhaust: false,
            budget: Arc::clone(&budget),
        })],
        &ids,
        &context,
        1,
        &|_, _, _| FilterDecision::Admit,
        (
            crate::scan::parallel::DevicePlan {
                groups: vec![0],
                limit: 1,
            },
            budget,
        ),
    );
    assert!(outcome.result_version_id.is_some());
    let result = sink
        .pin()
        .map_err(|error| format!("{error:?}"))?
        .ok_or("missing")?;
    assert_eq!(
        result.completeness(),
        du_core_contracts::result::ResultCompleteness::Complete
    );
    assert!(result.items().iter().any(|item| item.name == b"folder"));
    assert!(result.items().iter().any(|item| item.name == b"leaf"));
    assert_eq!(result.unread_folder_counts().cancelled, 0);
    assert!(result.scan_issues().is_empty());
    // Only the root attempt was registered; the refused child kept ordinary traversal.
    assert_eq!(
        result
            .folder_attempt_summaries()
            .ok_or("missing attempt summary")?
            .len(),
        1
    );
    Ok(())
}
