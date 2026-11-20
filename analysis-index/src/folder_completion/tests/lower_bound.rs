//! #226: the cause a version gives for lower bounds that no row explains.
use super::*;
use crate::folder_collection::{
    REASON_EVIDENCE_BUDGET_EXCEEDED, REASON_EVIDENCE_NOT_RECORDED, REASON_LOWER_BOUND_UNEXPLAINED,
};

#[test]
fn the_cause_follows_the_folder_attempt_evidence_of_the_version() -> Test {
    // Every folder has an ended attempt: whatever is still a lower bound is unexplained.
    let sink = IngestSink::default();
    let version = tree(&sink, Case::AllEnded)?
        .seal(id(2)?, ResultCompleteness::Missing)
        .map_err(e)?;
    assert_eq!(
        version.lower_bound_cause(),
        Some(REASON_LOWER_BOUND_UNEXPLAINED)
    );

    // A folder read without any attempt was read while the evidence budget was spent.
    let sink = IngestSink::default();
    let mut build = tree(&sink, Case::AllEnded)?;
    build.add_directory(path(&["extra"])).map_err(e)?;
    let version = build.seal(id(2)?, ResultCompleteness::Missing).map_err(e)?;
    assert_eq!(
        version.lower_bound_cause(),
        Some(REASON_EVIDENCE_BUDGET_EXCEEDED)
    );

    // Evidence that did not fit the budget is dropped, and the version says so.
    let sink = IngestSink::default();
    let build = tree(&sink, Case::AllEnded)?;
    let _blocker = sink
        .folder_evidence_budget
        .reserve_remaining()
        .ok_or("reserve the remaining evidence budget")?;
    let version = build.seal(id(2)?, ResultCompleteness::Missing).map_err(e)?;
    assert!(version.folder_attempt_summaries().is_none());
    assert_eq!(
        version.lower_bound_cause(),
        Some(REASON_EVIDENCE_BUDGET_EXCEEDED)
    );
    Ok(())
}

#[test]
fn a_version_without_evidence_asks_for_a_new_analysis_and_a_running_scan_gives_none() -> Test {
    let sink = IngestSink::default();
    let mut build = sink.open_forest(id(1)?, vec![root()]).map_err(e)?;
    build.add_directory(Vec::new()).map_err(e)?;
    build.add_directory(path(&["a"])).map_err(e)?;
    let partial = build.publish_partial(id(2)?).map_err(e)?;
    // Lower bounds are the normal interim state while the scan runs.
    assert_eq!(partial.lower_bound_cause(), None);
    let version = build.seal(id(3)?, ResultCompleteness::Missing).map_err(e)?;
    assert_eq!(
        version.lower_bound_cause(),
        Some(REASON_EVIDENCE_NOT_RECORDED)
    );
    Ok(())
}
