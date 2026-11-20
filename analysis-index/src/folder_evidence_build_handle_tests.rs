use super::*;
use crate::{FolderAttemptFinal, FolderAttemptInput, FolderAttemptStatus, IngestSink};
use du_core_contracts::scan_observation::ObservedValue;
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

fn build(sink: &IngestSink, version: u32) -> Result<crate::BuildHandle, String> {
    let mut build = sink
        .open_observed(id(version)?, root())
        .map_err(|error| format!("{error:?}"))?;
    build
        .add_directory(Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    Ok(build)
}

fn admitted(child_id: u64, name: &[u8]) -> FolderAttemptInput {
    FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::FilterCallback {
        child_id,
        raw_name: name.to_vec(),
        decision: FilterDecision::Admit,
        phase: FilterCallbackPhase::BeforeMetadata,
    })
}

fn observation(name: &[u8], kind: ObservedKind) -> TimedObservedEntry {
    TimedObservedEntry {
        entry: ObservedEntry {
            name: name.to_vec(),
            enumerated_inode: 1,
            enumerated_kind: kind.clone(),
            metadata: Ok(MetadataObservation {
                kind,
                device: 1,
                inode: 1,
                size: Err(PortError {
                    kind: PortErrorKind::Unsupported,
                    reason_key: "scan.test_size_unavailable".into(),
                    os_code: None,
                }),
                modified_at: ObservedValue::NotApplicable,
                native_modified_at: None,
                allocated: ObservedValue::NotApplicable,
                link_count: ObservedValue::NotApplicable,
                space_identity: ObservedValue::NotApplicable,
            }),
        },
        observed_at: None,
        placeholder_observation: None,
    }
}

fn observation_error(name: &[u8], kind: ObservedKind) -> TimedObservedEntry {
    TimedObservedEntry {
        entry: ObservedEntry {
            name: name.to_vec(),
            enumerated_inode: 1,
            enumerated_kind: kind,
            metadata: Err(PortError {
                kind: PortErrorKind::Unsupported,
                reason_key: "scan.test_observation_unavailable".into(),
                os_code: None,
            }),
        },
        observed_at: None,
        placeholder_observation: None,
    }
}

fn summary(
    result: &crate::ResultVersion,
    attempt_id: u64,
) -> Result<crate::FolderAttemptSummary, String> {
    result
        .folder_attempt_summaries()
        .ok_or("folder attempt summaries are missing")?
        .iter()
        .find(|row| row.attempt_id == attempt_id)
        .copied()
        .ok_or_else(|| format!("attempt {attempt_id} is missing"))
}

#[test]
fn held_folder_evidence_does_not_change_done_scan_completion() -> Result<(), String> {
    let sink = IngestSink::default();
    let lineage = id(1001)?;
    sink.bind_current_analysis("held-folder-completion", lineage.clone())
        .map_err(|error| format!("{error:?}"))?;
    let mut build = sink
        .open_forest_in_lineage(id(1002)?, vec![root()], lineage)
        .map_err(|error| format!("{error:?}"))?;
    build
        .add_directory(Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    let token = build
        .begin_folder_attempt(0, &[], 100)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        build.end_folder_attempt(token, 0, 1),
        Err(FolderAttemptError::InvalidEnd)
    );

    let version_id = id(1003)?;
    let result = build
        .seal(version_id.clone(), ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(result.completeness(), ResultCompleteness::Complete);
    assert!(
        sink.observe_scan_terminal(&version_id, Some(ScanCompletion::Done))
            .map_err(|error| format!("{error:?}"))?
    );
    let completed = sink
        .pin_completed_scan(&version_id)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(completed.completion(), ScanCompletion::Done);
    assert_eq!(
        summary(completed.version(), 100)?.status,
        FolderAttemptStatus::Held
    );
    Ok(())
}

#[test]
fn recorded_file_without_child_attempt_ends_unproven() -> Result<(), String> {
    let mut build = build(&IngestSink::default(), 110)?;
    build
        .add_file(crate::Observation {
            path: vec![b"file".to_vec()],
            size: None,
        })
        .map_err(|error| format!("{error:?}"))?;
    let token = build
        .begin_folder_attempt(0, &[], 110)
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(token, 0, admitted(0, b"file"))
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(
            token,
            1,
            FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::Observation {
                child_id: 0,
                entry: observation(b"file", ObservedKind::File),
            }),
        )
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(
            token,
            2,
            FolderAttemptInput::Final {
                child_id: 0,
                outcome: FolderAttemptFinal::Recorded {
                    child_attempt: None,
                },
            },
        )
        .map_err(|error| format!("{error:?}"))?;
    build
        .end_folder_attempt(token, 3, 1)
        .map_err(|error| format!("{error:?}"))?;
    let result = build
        .publish_partial(id(111)?)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        summary(&result, 110)?.status,
        FolderAttemptStatus::EndedUnproven
    );
    Ok(())
}

#[test]
fn recorded_file_with_child_attempt_is_held() -> Result<(), String> {
    let mut build = build(&IngestSink::default(), 120)?;
    build
        .add_file(crate::Observation {
            path: vec![b"file".to_vec()],
            size: None,
        })
        .map_err(|error| format!("{error:?}"))?;
    let token = build
        .begin_folder_attempt(0, &[], 120)
        .map_err(|error| format!("{error:?}"))?;
    let unrelated_attempt = build
        .begin_folder_attempt(0, &[], 121)
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(token, 0, admitted(0, b"file"))
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(
            token,
            1,
            FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::Observation {
                child_id: 0,
                entry: observation(b"file", ObservedKind::File),
            }),
        )
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(
            token,
            2,
            FolderAttemptInput::Final {
                child_id: 0,
                outcome: FolderAttemptFinal::Recorded {
                    child_attempt: Some(unrelated_attempt),
                },
            },
        )
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        build.end_folder_attempt(token, 3, 1),
        Err(FolderAttemptError::InvalidEnd)
    );
    let result = build
        .publish_partial(id(122)?)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(summary(&result, 120)?.status, FolderAttemptStatus::Held);
    Ok(())
}

#[test]
fn recorded_directory_with_registered_matching_child_attempt_ends() -> Result<(), String> {
    let mut build = build(&IngestSink::default(), 130)?;
    let child_path = vec![b"child".to_vec()];
    build
        .add_directory(child_path.clone())
        .map_err(|error| format!("{error:?}"))?;
    let parent = build
        .begin_folder_attempt(0, &[], 130)
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(parent, 0, admitted(0, b"child"))
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(
            parent,
            1,
            FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::Observation {
                child_id: 0,
                entry: observation(b"child", ObservedKind::Directory),
            }),
        )
        .map_err(|error| format!("{error:?}"))?;
    let child = build
        .begin_folder_attempt(0, &child_path, 131)
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(
            parent,
            2,
            FolderAttemptInput::Final {
                child_id: 0,
                outcome: FolderAttemptFinal::Recorded {
                    child_attempt: Some(child),
                },
            },
        )
        .map_err(|error| format!("{error:?}"))?;
    build
        .end_folder_attempt(parent, 3, 1)
        .map_err(|error| format!("{error:?}"))?;
    let result = build
        .publish_partial(id(132)?)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        summary(&result, 130)?.status,
        FolderAttemptStatus::EndedUnproven
    );
    Ok(())
}

#[test]
fn recorded_directory_with_mismatched_child_attempt_is_held() -> Result<(), String> {
    let mut build = build(&IngestSink::default(), 140)?;
    let child_path = vec![b"child".to_vec()];
    let other_path = vec![b"other".to_vec()];
    build
        .add_directory(child_path)
        .map_err(|error| format!("{error:?}"))?;
    build
        .add_directory(other_path.clone())
        .map_err(|error| format!("{error:?}"))?;
    let parent = build
        .begin_folder_attempt(0, &[], 140)
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(parent, 0, admitted(0, b"child"))
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(
            parent,
            1,
            FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::Observation {
                child_id: 0,
                entry: observation(b"child", ObservedKind::Directory),
            }),
        )
        .map_err(|error| format!("{error:?}"))?;
    let mismatched = build
        .begin_folder_attempt(0, &other_path, 141)
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(
            parent,
            2,
            FolderAttemptInput::Final {
                child_id: 0,
                outcome: FolderAttemptFinal::Recorded {
                    child_attempt: Some(mismatched),
                },
            },
        )
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        build.end_folder_attempt(parent, 3, 1),
        Err(FolderAttemptError::InvalidEnd)
    );
    let result = build
        .publish_partial(id(142)?)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(summary(&result, 140)?.status, FolderAttemptStatus::Held);
    Ok(())
}

#[test]
fn successful_directory_revisit_ends_unproven() -> Result<(), String> {
    let mut build = build(&IngestSink::default(), 150)?;
    build
        .add_directory(vec![b"revisited".to_vec()])
        .map_err(|error| format!("{error:?}"))?;
    let token = build
        .begin_folder_attempt(0, &[], 150)
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(token, 0, admitted(0, b"revisited"))
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(
            token,
            1,
            FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::Observation {
                child_id: 0,
                entry: observation(b"revisited", ObservedKind::Directory),
            }),
        )
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(
            token,
            2,
            FolderAttemptInput::Final {
                child_id: 0,
                outcome: FolderAttemptFinal::Revisit,
            },
        )
        .map_err(|error| format!("{error:?}"))?;
    build
        .end_folder_attempt(token, 3, 1)
        .map_err(|error| format!("{error:?}"))?;
    let result = build
        .publish_partial(id(151)?)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        summary(&result, 150)?.status,
        FolderAttemptStatus::EndedUnproven
    );
    Ok(())
}

#[test]
fn revisit_with_metadata_error_is_held() -> Result<(), String> {
    let mut build = build(&IngestSink::default(), 160)?;
    build
        .add_file(crate::Observation {
            path: vec![b"not-a-directory".to_vec()],
            size: None,
        })
        .map_err(|error| format!("{error:?}"))?;
    let token = build
        .begin_folder_attempt(0, &[], 160)
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(token, 0, admitted(0, b"not-a-directory"))
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(
            token,
            1,
            FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::Observation {
                child_id: 0,
                entry: observation_error(b"not-a-directory", ObservedKind::File),
            }),
        )
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(
            token,
            2,
            FolderAttemptInput::Final {
                child_id: 0,
                outcome: FolderAttemptFinal::Revisit,
            },
        )
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        build.end_folder_attempt(token, 3, 1),
        Err(FolderAttemptError::InvalidEnd)
    );
    let result = build
        .publish_partial(id(161)?)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(summary(&result, 160)?.status, FolderAttemptStatus::Held);
    Ok(())
}

mod growth_tests;
