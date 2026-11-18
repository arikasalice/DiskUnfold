use super::*;

fn boundary_observation() -> TimedObservedEntry {
    TimedObservedEntry {
        entry: ObservedEntry {
            name: b"mount".to_vec(),
            enumerated_inode: 1,
            enumerated_kind: ObservedKind::Directory,
            metadata: Ok(MetadataObservation {
                kind: ObservedKind::MountPoint,
                device: 2,
                inode: 1,
                size: Err(PortError {
                    kind: PortErrorKind::Unsupported,
                    reason_key: "scan.boundary_size_unavailable".into(),
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

fn boundary_attempt(
    observation_first: bool,
    observation_child_id: u64,
    child_count_exclusive: u64,
) -> Result<(crate::BuildHandle, Result<(), FolderAttemptError>), String> {
    let mut build = build(&IngestSink::default(), 51)?;
    build
        .add_directory(vec![b"mount".to_vec()])
        .map_err(|error| format!("{error:?}"))?;
    let token = build
        .begin_folder_attempt(0, &[], 51)
        .map_err(|error| format!("{error:?}"))?;
    let callback = FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::FilterCallback {
        child_id: 0,
        raw_name: b"mount".to_vec(),
        decision: FilterDecision::Admit,
        phase: FilterCallbackPhase::AfterMetadata,
    });
    let observation = FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::Observation {
        child_id: observation_child_id,
        entry: boundary_observation(),
    });
    let (first, second) = if observation_first {
        (observation, callback)
    } else {
        (callback, observation)
    };
    build
        .record_folder_attempt_event(token, 0, first)
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(token, 1, second)
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_folder_attempt_event(
            token,
            2,
            FolderAttemptInput::Final {
                child_id: 0,
                outcome: FolderAttemptFinal::Boundary,
            },
        )
        .map_err(|error| format!("{error:?}"))?;
    let end = build.end_folder_attempt(token, 3, child_count_exclusive);
    Ok((build, end))
}

#[test]
fn after_metadata_is_os_timing_while_callback_stays_first_on_transport() -> Result<(), String> {
    // Synthetic bulk metadata exercises F03 correlation only; it does not establish
    // any OS adapter's handle-opening behavior or exact-result support.
    let (mut build, end) = boundary_attempt(false, 0, 1)?;
    assert_eq!(end, Ok(()));
    let result = build
        .publish_partial(id(52)?)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        result.folder_attempt_summaries().map(|rows| rows[0].status),
        Some(FolderAttemptStatus::EndedUnproven)
    );
    Ok(())
}

#[test]
fn after_metadata_does_not_allow_reversed_or_cross_child_events() -> Result<(), String> {
    let (mut reversed, end) = boundary_attempt(true, 0, 1)?;
    assert_eq!(end, Err(FolderAttemptError::InvalidEnd));
    let reversed_result = reversed
        .publish_partial(id(53)?)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        reversed_result
            .folder_attempt_summaries()
            .map(|rows| rows[0].status),
        Some(FolderAttemptStatus::Held)
    );

    let (_, end) = boundary_attempt(false, 1, 2)?;
    assert_eq!(end, Err(FolderAttemptError::InvalidEnd));
    Ok(())
}

#[test]
fn cancellation_and_eof_child_count_mismatch_hold_completion() -> Result<(), String> {
    let mut cancelled = build(&IngestSink::default(), 54)?;
    cancelled
        .add_directory(vec![b"observed".to_vec()])
        .map_err(|error| format!("{error:?}"))?;
    let token = cancelled
        .begin_folder_attempt(0, &[], 54)
        .map_err(|error| format!("{error:?}"))?;
    cancelled
        .record_folder_attempt_event(token, 0, exclude(b"hidden"))
        .map_err(|error| format!("{error:?}"))?;
    cancelled
        .record_folder_attempt_event(token, 1, FolderAttemptInput::Cancelled)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        cancelled.end_folder_attempt(token, 2, 1),
        Err(FolderAttemptError::InvalidEnd)
    );
    let cancelled_result = cancelled
        .publish_partial(id(56)?)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(cancelled_result.items().len(), 2);
    assert_eq!(
        cancelled_result
            .folder_attempt_summaries()
            .map(|rows| rows[0].status),
        Some(FolderAttemptStatus::Held)
    );

    let mut short_eof = build(&IngestSink::default(), 55)?;
    let token = short_eof
        .begin_folder_attempt(0, &[], 55)
        .map_err(|error| format!("{error:?}"))?;
    short_eof
        .record_folder_attempt_event(token, 0, exclude(b"hidden"))
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        short_eof.end_folder_attempt(token, 1, 2),
        Err(FolderAttemptError::InvalidEnd)
    );
    Ok(())
}
