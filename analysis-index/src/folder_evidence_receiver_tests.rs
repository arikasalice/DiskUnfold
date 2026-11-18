use super::*;
use crate::IngestSink;
use du_core_contracts::{
    Id, PathBytes, PathEncoding,
    ports::{PortError, PortErrorKind},
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
    scan_observation::{
        CorrelatedDirectoryEvent, FilterCallbackPhase, FilterDecision, MetadataObservation,
        ObservedEntry, ObservedKind, TimedObservedEntry,
    },
};

fn id(value: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{value:012x}"))
        .map_err(|error| format!("{error:?}"))
}

fn build(sink: &IngestSink, version: u32) -> Result<crate::BuildHandle, String> {
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"root"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut build = sink
        .open_observed(id(version)?, root)
        .map_err(|error| format!("{error:?}"))?;
    build
        .add_directory(Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    Ok(build)
}

fn exclude(name: &[u8]) -> FolderAttemptInput {
    FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::FilterCallback {
        child_id: 0,
        raw_name: name.to_vec(),
        decision: FilterDecision::Exclude,
        phase: FilterCallbackPhase::BeforeMetadata,
    })
}

#[test]
fn build_handle_receives_exclude_once_and_pins_an_unproven_summary() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = build(&sink, 1)?;
    let token = build
        .begin_folder_attempt(0, &[], 77)
        .map_err(|error| format!("{error:?}"))?;
    let callback = exclude(b"hidden");
    build
        .record_folder_attempt_event(token, 0, callback.clone())
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(token, 0, callback)
        .map_err(|error| format!("{error:?}"))?;
    build
        .end_folder_attempt(token, 1, 1)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(build.end_folder_attempt(token, 1, 1), Ok(()));
    assert_eq!(
        build.record_folder_attempt_event(token, 0, exclude(b"hidden")),
        Ok(())
    );

    let first = build
        .publish_partial(id(2).map_err(|error| error.to_string())?)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        first.folder_attempt_summaries().map(|rows| rows[0].status),
        Some(FolderAttemptStatus::EndedUnproven)
    );
    assert_eq!(
        first
            .folder_attempt_summaries()
            .map(|rows| rows[0].child_count_exclusive),
        Some(Some(1))
    );
    let pinned_items = first.items().to_vec();

    assert_eq!(
        build.record_folder_attempt_event(
            token,
            1,
            FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::Progress),
        ),
        Err(FolderAttemptError::Conflict)
    );
    assert_eq!(
        build.end_folder_attempt(token, 1, 1),
        Err(FolderAttemptError::InvalidEnd)
    );
    assert_eq!(
        build.record_folder_attempt_event(token, 0, exclude(b"hidden")),
        Err(FolderAttemptError::Conflict)
    );
    let second = build
        .publish_partial(id(3).map_err(|error| error.to_string())?)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        first.folder_attempt_summaries().map(|rows| rows[0].status),
        Some(FolderAttemptStatus::EndedUnproven)
    );
    assert_eq!(
        second.folder_attempt_summaries().map(|rows| rows[0].status),
        Some(FolderAttemptStatus::Held)
    );
    assert_eq!(first.items(), pinned_items.as_slice());
    Ok(())
}

#[test]
fn first_attempt_reserves_a_small_registry_chunk() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = build(&sink, 9)?;
    build
        .begin_folder_attempt(0, &[], 9)
        .map_err(|error| format!("{error:?}"))?;
    let registry_lease = build
        .folder_attempts
        .registry_lease
        .as_ref()
        .ok_or("missing registry reservation")?;
    assert_eq!(
        registry_lease.bytes(),
        std::mem::size_of::<super::AttemptState>() * build.folder_attempts.attempts.capacity()
    );
    assert!(
        registry_lease.bytes()
            < std::mem::size_of::<super::AttemptState>() * super::MAX_ATTEMPTS_PER_BUILD
    );
    let state = build
        .folder_attempts
        .attempts
        .first()
        .ok_or("missing registered attempt")?;
    assert_eq!(
        state
            .event_capacity_lease
            .as_ref()
            .ok_or("missing event capacity reservation")?
            .bytes(),
        std::mem::size_of::<super::StoredAttemptEvent>() * state.events.capacity()
    );
    Ok(())
}

#[test]
fn different_sequence_duplicate_exclude_and_exclude_then_final_hold() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut duplicate = build(&sink, 10)?;
    let token = duplicate
        .begin_folder_attempt(0, &[], 10)
        .map_err(|error| format!("{error:?}"))?;
    duplicate
        .record_folder_attempt_event(token, 0, exclude(b"hidden"))
        .map_err(|error| format!("{error:?}"))?;
    duplicate
        .record_folder_attempt_event(token, 1, exclude(b"hidden"))
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        duplicate.end_folder_attempt(token, 2, 1),
        Err(FolderAttemptError::InvalidEnd)
    );
    assert_eq!(
        duplicate.end_folder_attempt(token, 2, 1),
        Err(FolderAttemptError::InvalidEnd)
    );
    duplicate.abort();

    let mut changed = build(&sink, 12)?;
    let token = changed
        .begin_folder_attempt(0, &[], 12)
        .map_err(|error| format!("{error:?}"))?;
    changed
        .record_folder_attempt_event(token, 0, exclude(b"original"))
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        changed.record_folder_attempt_event(token, 0, exclude(b"changed")),
        Err(FolderAttemptError::Conflict)
    );
    let mut followed_by_final = build(&IngestSink::default(), 11)?;
    let token = followed_by_final
        .begin_folder_attempt(0, &[], 11)
        .map_err(|error| format!("{error:?}"))?;
    followed_by_final
        .record_folder_attempt_event(token, 0, exclude(b"hidden"))
        .map_err(|error| format!("{error:?}"))?;
    followed_by_final
        .record_folder_attempt_event(
            token,
            1,
            FolderAttemptInput::Final {
                child_id: 0,
                outcome: FolderAttemptFinal::Recorded {
                    child_attempt: None,
                },
            },
        )
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        followed_by_final.end_folder_attempt(token, 2, 1),
        Err(FolderAttemptError::InvalidEnd)
    );
    Ok(())
}

#[test]
fn excluded_after_metadata_callback_is_a_single_terminal_event() -> Result<(), String> {
    let mut build = build(&IngestSink::default(), 15)?;
    let token = build
        .begin_folder_attempt(0, &[], 15)
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(
            token,
            0,
            FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::FilterCallback {
                child_id: 0,
                raw_name: b"hidden".to_vec(),
                decision: FilterDecision::Exclude,
                phase: FilterCallbackPhase::AfterMetadata,
            }),
        )
        .map_err(|error| format!("{error:?}"))?;
    build
        .end_folder_attempt(token, 1, 1)
        .map_err(|error| format!("{error:?}"))?;
    let result = build
        .publish_partial(id(16).map_err(|error| error.to_string())?)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(result.items().len(), 1);
    assert_eq!(
        result.folder_attempt_summaries().map(|rows| rows[0].status),
        Some(FolderAttemptStatus::EndedUnproven)
    );
    Ok(())
}

#[test]
fn exclude_followed_by_same_child_observation_holds_the_attempt() -> Result<(), String> {
    let mut build = build(&IngestSink::default(), 16)?;
    let token = build
        .begin_folder_attempt(0, &[], 16)
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(token, 0, exclude(b"hidden"))
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(
            token,
            1,
            FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::Observation {
                child_id: 0,
                entry: TimedObservedEntry {
                    entry: ObservedEntry {
                        name: b"hidden".to_vec(),
                        enumerated_inode: 1,
                        enumerated_kind: ObservedKind::File,
                        metadata: Err(PortError {
                            kind: PortErrorKind::Unsupported,
                            reason_key: "scan.test_observation_unavailable".into(),
                            os_code: None,
                        }),
                    },
                    observed_at: None,
                    placeholder_observation: None,
                },
            }),
        )
        .map_err(|error| format!("{error:?}"))?;

    assert_eq!(
        build.end_folder_attempt(token, 2, 1),
        Err(FolderAttemptError::InvalidEnd)
    );
    let result = build
        .publish_partial(id(17)?)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        result.folder_attempt_summaries().map(|rows| rows[0].status),
        Some(FolderAttemptStatus::Held)
    );
    Ok(())
}

#[test]
fn empty_folder_ends_but_progress_cannot_substitute_for_a_child_terminal() -> Result<(), String> {
    let mut build = build(&IngestSink::default(), 17)?;
    let empty = build
        .begin_folder_attempt(0, &[], 17)
        .map_err(|error| format!("{error:?}"))?;
    build
        .end_folder_attempt(empty, 0, 0)
        .map_err(|error| format!("{error:?}"))?;

    let missing = build
        .begin_folder_attempt(0, &[], 18)
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(
            missing,
            0,
            FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::Progress),
        )
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        build.end_folder_attempt(missing, 1, 1),
        Err(FolderAttemptError::InvalidEnd)
    );
    let gap = build
        .begin_folder_attempt(0, &[], 19)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        build.record_folder_attempt_event(
            gap,
            1,
            FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::Progress),
        ),
        Err(FolderAttemptError::SequenceGap)
    );
    assert_eq!(
        build.end_folder_attempt(gap, 0, 0),
        Err(FolderAttemptError::InvalidEnd)
    );
    let result = build
        .publish_partial(id(19).map_err(|error| error.to_string())?)
        .map_err(|error| format!("{error:?}"))?;
    let rows = result
        .folder_attempt_summaries()
        .ok_or("attempt summaries are missing")?;
    assert_eq!(rows[0].status, FolderAttemptStatus::EndedUnproven);
    assert_eq!(rows[1].status, FolderAttemptStatus::Held);
    assert_eq!(rows[2].status, FolderAttemptStatus::Held);
    Ok(())
}

#[test]
fn end_validation_budget_failure_is_distinct_and_replayed_as_failure() -> Result<(), String> {
    let mut build = build(&IngestSink::default(), 22)?;
    let token = build
        .begin_folder_attempt(0, &[], 22)
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(token, 0, exclude(b"hidden"))
        .map_err(|error| format!("{error:?}"))?;
    let blocker = build
        .folder_attempts
        .budget
        .reserve_folder_remaining(&build.folder_attempts.attempts[0].scope)
        .ok_or("folder budget reservation failed")?;
    assert_eq!(
        build.end_folder_attempt(token, 1, 1),
        Err(FolderAttemptError::CapacityExceeded)
    );
    assert_eq!(
        build.end_folder_attempt(token, 1, 1),
        Err(FolderAttemptError::CapacityExceeded)
    );
    drop(blocker);
    let result = build
        .publish_partial(id(23).map_err(|error| error.to_string())?)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        result.folder_attempt_summaries().map(|rows| rows[0].status),
        Some(FolderAttemptStatus::Held)
    );
    Ok(())
}

#[test]
fn stale_build_token_is_rejected_after_replacement_generation_changes() -> Result<(), String> {
    let sink = IngestSink::default();
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"root"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    sink.bind_current_analysis("stale-token", id(300)?)
        .map_err(|error| format!("{error:?}"))?;
    let mut original = sink
        .open_forest_in_lineage(id(30)?, vec![root], id(300)?)
        .map_err(|error| format!("{error:?}"))?;
    original
        .add_directory(Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    let old_token = original
        .begin_folder_attempt(0, &[], 5)
        .map_err(|error| format!("{error:?}"))?;
    let previous = original
        .seal(id(31)?, ResultCompleteness::Partial)
        .map_err(|error| format!("{error:?}"))?;
    let mut replacement = sink
        .open_replacement(&previous)
        .map_err(|error| format!("{error:?}"))?;
    replacement
        .add_directory(Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    replacement
        .begin_folder_attempt(0, &[], 5)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        replacement.record_folder_attempt_event(old_token, 0, exclude(b"late")),
        Err(FolderAttemptError::WrongBuild)
    );
    Ok(())
}

#[test]
fn folder_payload_budget_failure_holds_before_accepting_event() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = build(&sink, 40)?;
    let token = build
        .begin_folder_attempt(0, &[], 40)
        .map_err(|error| format!("{error:?}"))?;
    let mut raw_name = Vec::new();
    raw_name
        .try_reserve_exact(MAX_FOLDER_MANAGED_BYTES + 1)
        .map_err(|error| format!("{error:?}"))?;
    raw_name.resize(MAX_FOLDER_MANAGED_BYTES + 1, b'x');
    let input = FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::FilterCallback {
        child_id: 0,
        raw_name,
        decision: FilterDecision::Exclude,
        phase: FilterCallbackPhase::BeforeMetadata,
    });
    assert_eq!(
        build.record_folder_attempt_event(token, 0, input),
        Err(FolderAttemptError::CapacityExceeded)
    );
    let result = build
        .publish_partial(id(41).map_err(|error| error.to_string())?)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        result.folder_attempt_summaries().map(|rows| rows[0].status),
        Some(FolderAttemptStatus::Held)
    );
    Ok(())
}

#[path = "folder_evidence_replacement_tests.rs"]
mod replacement_tests;

#[path = "folder_evidence_receiver_boundary_tests.rs"]
mod registry_boundary_tests;

#[path = "folder_evidence_phase_tests.rs"]
mod phase_tests;
