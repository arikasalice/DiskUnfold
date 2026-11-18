use super::*;
use crate::{FolderAttemptFinal, FolderAttemptInput, FolderAttemptStatus, IngestSink};
use du_core_contracts::{
    Id, PathBytes, PathEncoding,
    ports::{PortError, PortErrorKind},
    result::{ResultNativePath, ResultScopeRoot, ResultVersionRead},
    scan_observation::{
        CorrelatedDirectoryEvent, FilterCallbackPhase, FilterDecision, MetadataObservation,
        ObservedEntry, ObservedKind, TimedObservedEntry,
    },
};

fn id(value: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{value:012x}"))
        .map_err(|error| format!("{error:?}"))
}

fn build(version: u32) -> Result<crate::BuildHandle, String> {
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"root"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut build = IngestSink::default()
        .open_observed(id(version)?, root)
        .map_err(|error| format!("{error:?}"))?;
    build
        .add_directory(Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    Ok(build)
}

fn filter(child_id: u64, name: &[u8], decision: FilterDecision) -> FolderAttemptInput {
    FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::FilterCallback {
        child_id,
        raw_name: name.to_vec(),
        decision,
        phase: FilterCallbackPhase::BeforeMetadata,
    })
}

fn observation(name: &[u8]) -> TimedObservedEntry {
    TimedObservedEntry {
        entry: ObservedEntry {
            name: name.to_vec(),
            enumerated_inode: 1,
            enumerated_kind: ObservedKind::File,
            metadata: Ok(MetadataObservation {
                kind: ObservedKind::File,
                device: 1,
                inode: 1,
                size: Err(PortError {
                    kind: PortErrorKind::Unsupported,
                    reason_key: "scan.test_size_unavailable".into(),
                    os_code: None,
                }),
                modified_at: None,
                native_modified_at: None,
                allocated: None,
                link_count: None,
                space_identity: None,
            }),
        },
        observed_at: None,
        placeholder_observation: None,
    }
}

fn status(result: &crate::ResultVersion, attempt_id: u64) -> Result<FolderAttemptStatus, String> {
    result
        .folder_attempt_summaries()
        .ok_or("folder attempt summaries are missing")?
        .iter()
        .find(|row| row.attempt_id == attempt_id)
        .map(|row| row.status)
        .ok_or_else(|| format!("attempt {attempt_id} is missing"))
}

fn publish_status(
    build: &mut crate::BuildHandle,
    version: u32,
    attempt_id: u64,
) -> Result<FolderAttemptStatus, String> {
    let result = build
        .publish_partial(id(version)?)
        .map_err(|error| format!("{error:?}"))?;
    status(&result, attempt_id)
}

#[test]
fn event_sequence_gap_holds_attempt() -> Result<(), String> {
    let mut build = build(201)?;
    let token = build
        .begin_folder_attempt(0, &[], 201)
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(
            token,
            0,
            FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::Progress),
        )
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        build.record_folder_attempt_event(
            token,
            2,
            FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::Progress),
        ),
        Err(FolderAttemptError::SequenceGap)
    );
    assert_eq!(
        publish_status(&mut build, 202, 201)?,
        FolderAttemptStatus::Held
    );
    Ok(())
}

#[test]
fn same_sequence_different_payload_conflicts_and_holds_attempt() -> Result<(), String> {
    let mut build = build(211)?;
    let token = build
        .begin_folder_attempt(0, &[], 211)
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(token, 0, filter(0, b"first", FilterDecision::Exclude))
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        build.record_folder_attempt_event(
            token,
            0,
            filter(0, b"different", FilterDecision::Exclude),
        ),
        Err(FolderAttemptError::Conflict)
    );
    assert_eq!(
        publish_status(&mut build, 212, 211)?,
        FolderAttemptStatus::Held
    );
    Ok(())
}

#[test]
fn same_observation_payload_at_different_sequences_is_invalid_at_end() -> Result<(), String> {
    let mut build = build(221)?;
    build
        .add_file(crate::Observation {
            path: vec![b"file".to_vec()],
            size: None,
        })
        .map_err(|error| format!("{error:?}"))?;
    let token = build
        .begin_folder_attempt(0, &[], 221)
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(token, 0, filter(0, b"file", FilterDecision::Admit))
        .map_err(|error| format!("{error:?}"))?;
    let observed = FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::Observation {
        child_id: 0,
        entry: observation(b"file"),
    });
    build
        .record_folder_attempt_event(token, 1, observed.clone())
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(token, 2, observed)
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(
            token,
            3,
            FolderAttemptInput::Final {
                child_id: 0,
                outcome: FolderAttemptFinal::Recorded {
                    child_attempt: None,
                },
            },
        )
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        build.end_folder_attempt(token, 4, 1),
        Err(FolderAttemptError::InvalidEnd)
    );
    assert_eq!(
        publish_status(&mut build, 222, 221)?,
        FolderAttemptStatus::Held
    );
    Ok(())
}

#[test]
fn distinct_admitted_children_with_same_raw_name_fail_end() -> Result<(), String> {
    let mut build = build(231)?;
    build
        .add_file(crate::Observation {
            path: vec![b"duplicate".to_vec()],
            size: None,
        })
        .map_err(|error| format!("{error:?}"))?;
    let token = build
        .begin_folder_attempt(0, &[], 231)
        .map_err(|error| format!("{error:?}"))?;
    for child_id in 0..2 {
        let first_seq = child_id * 3;
        build
            .record_folder_attempt_event(
                token,
                first_seq,
                filter(child_id, b"duplicate", FilterDecision::Admit),
            )
            .map_err(|error| format!("{error:?}"))?;
        build
            .record_folder_attempt_event(
                token,
                first_seq + 1,
                FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::Observation {
                    child_id,
                    entry: observation(b"duplicate"),
                }),
            )
            .map_err(|error| format!("{error:?}"))?;
        build
            .record_folder_attempt_event(
                token,
                first_seq + 2,
                FolderAttemptInput::Final {
                    child_id,
                    outcome: FolderAttemptFinal::Recorded {
                        child_attempt: None,
                    },
                },
            )
            .map_err(|error| format!("{error:?}"))?;
    }
    assert_eq!(
        build.end_folder_attempt(token, 6, 2),
        Err(FolderAttemptError::InvalidEnd)
    );
    assert_eq!(
        publish_status(&mut build, 232, 231)?,
        FolderAttemptStatus::Held
    );
    Ok(())
}

#[test]
fn unresolved_final_for_admitted_child_holds_on_record() -> Result<(), String> {
    let mut build = build(241)?;
    let token = build
        .begin_folder_attempt(0, &[], 241)
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(token, 0, filter(0, b"child", FilterDecision::Admit))
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(
            token,
            1,
            FolderAttemptInput::Final {
                child_id: 0,
                outcome: FolderAttemptFinal::Unresolved,
            },
        )
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        publish_status(&mut build, 242, 241)?,
        FolderAttemptStatus::Held
    );
    Ok(())
}

#[test]
fn undecidable_filter_callback_fails_end() -> Result<(), String> {
    let mut build = build(251)?;
    let token = build
        .begin_folder_attempt(0, &[], 251)
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(token, 0, filter(0, b"child", FilterDecision::Undecidable))
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        build.end_folder_attempt(token, 1, 1),
        Err(FolderAttemptError::InvalidEnd)
    );
    assert_eq!(
        publish_status(&mut build, 252, 251)?,
        FolderAttemptStatus::Held
    );
    Ok(())
}

#[test]
fn pending_cleared_holds_on_record() -> Result<(), String> {
    let mut build = build(261)?;
    let token = build
        .begin_folder_attempt(0, &[], 261)
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(token, 0, FolderAttemptInput::PendingCleared { child_id: 0 })
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        publish_status(&mut build, 262, 261)?,
        FolderAttemptStatus::Held
    );
    Ok(())
}

#[test]
fn transport_failure_holds_on_record() -> Result<(), String> {
    let mut build = build(271)?;
    let token = build
        .begin_folder_attempt(0, &[], 271)
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(token, 0, FolderAttemptInput::TransportFailure)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        publish_status(&mut build, 272, 271)?,
        FolderAttemptStatus::Held
    );
    Ok(())
}
