use crate::{FolderAttemptInput, FolderAttemptStatus, IngestSink};
use du_core_contracts::{
    Id, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
    scan_observation::{CorrelatedDirectoryEvent, FilterCallbackPhase, FilterDecision},
};
use std::sync::Arc;

fn id(value: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{value:012x}"))
        .map_err(|error| format!("{error:?}"))
}

fn root() -> ResultScopeRoot {
    ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"root"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    }
}

fn excluded(raw_name: Vec<u8>) -> FolderAttemptInput {
    FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::FilterCallback {
        child_id: 0,
        raw_name,
        decision: FilterDecision::Exclude,
        phase: FilterCallbackPhase::BeforeMetadata,
    })
}

// Synthetic index input exercises value retention, not operating-system evidence.
fn measured_file(name: &[u8], bytes: u64) -> crate::Observation {
    crate::Observation {
        path: vec![name.to_vec()],
        size: Some(SizeValue {
            basis: SizeBasis::Logical,
            evidence: SizeEvidence::Measured,
            amount: SizeAmount::Exact {
                bytes: bytes.into(),
            },
        }),
    }
}

#[test]
fn budget_is_shared_by_sink_clones_and_isolated_between_sink_instances() {
    let sink = IngestSink::default();
    let clone = sink.clone();
    let separate = IngestSink::default();
    assert!(Arc::ptr_eq(
        &sink.folder_evidence_budget,
        &clone.folder_evidence_budget
    ));
    assert!(!Arc::ptr_eq(
        &sink.folder_evidence_budget,
        &separate.folder_evidence_budget
    ));
}

#[test]
fn replacement_reserves_old_snapshot_and_new_event_against_one_folder_cap() -> Result<(), String> {
    let sink = IngestSink::default();
    let lineage = id(600)?;
    sink.bind_current_analysis("replacement-budget", lineage.clone())
        .map_err(|error| format!("{error:?}"))?;
    let mut original = sink
        .open_forest_in_lineage(id(60)?, vec![root()], lineage)
        .map_err(|error| format!("{error:?}"))?;
    original
        .add_directory(Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    let token = original
        .begin_folder_attempt(0, &[], 60)
        .map_err(|error| format!("{error:?}"))?;
    let mut prior_name = Vec::new();
    prior_name
        .try_reserve_exact(350_000)
        .map_err(|error| format!("{error:?}"))?;
    prior_name.resize(350_000, b'a');
    original
        .record_folder_attempt_event(token, 0, excluded(prior_name))
        .map_err(|error| format!("{error:?}"))?;
    original
        .end_folder_attempt(token, 1, 1)
        .map_err(|error| format!("{error:?}"))?;
    let previous = original
        .seal(id(61)?, ResultCompleteness::Partial)
        .map_err(|error| format!("{error:?}"))?;
    let prior_items = previous.items().to_vec();

    let mut replacement = sink
        .open_replacement(&previous)
        .map_err(|error| format!("{error:?}"))?;
    replacement
        .add_directory(Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    let token = replacement
        .begin_folder_attempt(0, &[], 61)
        .map_err(|error| format!("{error:?}"))?;
    let mut next_name = Vec::new();
    next_name
        .try_reserve_exact(700_000)
        .map_err(|error| format!("{error:?}"))?;
    next_name.resize(700_000, b'b');
    assert_eq!(
        replacement.record_folder_attempt_event(token, 0, excluded(next_name)),
        Err(crate::FolderAttemptError::CapacityExceeded)
    );
    let current = replacement
        .publish_partial(id(62)?)
        .map_err(|error| format!("{error:?}"))?;

    assert_eq!(
        previous
            .folder_attempt_summaries()
            .map(|rows| rows[0].status),
        Some(FolderAttemptStatus::EndedUnproven)
    );
    assert_eq!(previous.items(), prior_items.as_slice());
    assert_eq!(
        current
            .folder_attempt_summaries()
            .map(|rows| rows[0].status),
        Some(FolderAttemptStatus::Held)
    );
    Ok(())
}

#[test]
fn snapshot_budget_exhaustion_publishes_gap_as_missing_without_exact_folder_total()
-> Result<(), String> {
    let sink = IngestSink::default();
    let lineage = id(700)?;
    sink.bind_current_analysis("snapshot-budget", lineage.clone())
        .map_err(|error| format!("{error:?}"))?;
    let mut baseline_build = sink
        .open_forest_in_lineage(id(70)?, vec![root()], lineage.clone())
        .map_err(|error| format!("{error:?}"))?;
    baseline_build
        .add_directory(Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    let _baseline = baseline_build
        .seal(id(71)?, ResultCompleteness::Partial)
        .map_err(|error| format!("{error:?}"))?;
    let mut build = sink
        .open_forest_in_lineage(id(70)?, vec![root()], lineage.clone())
        .map_err(|error| format!("{error:?}"))?;
    build
        .add_directory(Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    build
        .add_file(measured_file(b"before", 5))
        .map_err(|error| format!("{error:?}"))?;
    let token = build
        .begin_folder_attempt(0, &[], 70)
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(token, 0, excluded(b"hidden".to_vec()))
        .map_err(|error| format!("{error:?}"))?;
    build
        .end_folder_attempt(token, 1, 1)
        .map_err(|error| format!("{error:?}"))?;
    let previous = build
        .publish_partial(id(72)?)
        .map_err(|error| format!("{error:?}"))?;
    assert!(matches!(
        sink.sealed_scan_result(previous.version_id()),
        Err(crate::SealedScanError::NotSealed)
    ));
    let previous_items = previous.items().to_vec();
    let previous_attempts = previous
        .folder_attempt_summaries()
        .ok_or("attempt summaries are missing")?
        .to_vec();
    let blocker = sink
        .folder_evidence_budget
        .reserve_remaining()
        .ok_or("reserve remaining global evidence budget failed")?;

    build
        .add_file(measured_file(b"after", 8))
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_gap_at(0, Vec::new(), "scan.test_gap".into(), None)
        .map_err(|error| format!("{error:?}"))?;
    let result = build
        .seal(id(73)?, ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;

    assert_eq!(result.completeness(), ResultCompleteness::Missing);
    assert!(result.folder_attempt_summaries().is_none());
    assert_eq!(result.scope_gaps().len(), 1);
    assert_eq!(result.items().len(), 3);
    assert_eq!(previous.items(), previous_items.as_slice());
    assert_eq!(
        previous
            .folder_attempt_summaries()
            .map(|rows| rows.to_vec()),
        Some(previous_attempts)
    );
    assert!(Arc::ptr_eq(
        &sink
            .pin_scan(Some(&lineage), None)
            .map_err(|error| format!("{error:?}"))?,
        &result
    ));
    assert!(Arc::ptr_eq(
        &sink
            .sealed_scan_result(result.version_id())
            .map_err(|error| format!("{error:?}"))?,
        &result
    ));
    let folder = result
        .folder(result.version_id(), &[])
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(folder.completeness, ResultCompleteness::Missing);
    assert!(matches!(
        &folder.logical.amount,
        SizeAmount::LowerBound { .. }
    ));
    let after = result
        .items()
        .iter()
        .find(|item| item.name.as_slice() == b"after")
        .ok_or("retained leaf observation is missing")?;
    let leaf_size = result
        .item_size(after.item_no, SizeBasis::Logical)
        .ok_or("retained leaf size is missing")?;
    assert_eq!(leaf_size.evidence, SizeEvidence::Measured);
    assert!(matches!(
        &leaf_size.amount,
        SizeAmount::Exact { bytes } if bytes.get() == 8
    ));
    drop(blocker);
    Ok(())
}

#[test]
fn complete_observation_input_keeps_existing_exact_folder_total() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink
        .open_forest(id(740)?, vec![root()])
        .map_err(|error| format!("{error:?}"))?;
    build
        .add_directory(Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    build
        .add_file(measured_file(b"known", 13))
        .map_err(|error| format!("{error:?}"))?;
    let result = build
        .seal(id(741)?, ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;

    assert_eq!(result.completeness(), ResultCompleteness::Complete);
    let folder = result
        .folder(result.version_id(), &[])
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(folder.completeness, ResultCompleteness::Complete);
    assert!(matches!(
        &folder.logical.amount,
        SizeAmount::Exact { bytes } if bytes.get() == 13
    ));
    Ok(())
}

#[path = "folder_evidence_failure_tests.rs"]
mod failure_tests;
