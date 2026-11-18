use super::*;

#[test]
fn attempt_event_buffer_grows_past_initial_capacity() -> Result<(), String> {
    let mut build = build(&IngestSink::default(), 170)?;
    let token = build
        .begin_folder_attempt(0, &[], 170)
        .map_err(|error| format!("{error:?}"))?;
    for event_seq in 0..5 {
        build
            .record_folder_attempt_event(
                token,
                event_seq,
                FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::Progress),
            )
            .map_err(|error| format!("{error:?}"))?;
    }
    build
        .end_folder_attempt(token, 5, 0)
        .map_err(|error| format!("{error:?}"))?;
    let result = build
        .publish_partial(id(171)?)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        summary(&result, 170)?.status,
        FolderAttemptStatus::EndedUnproven
    );
    Ok(())
}

#[test]
fn event_buffer_growth_budget_failure_rejects_event_and_holds_attempt() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = build(&sink, 180)?;
    build
        .add_file(crate::Observation {
            path: vec![b"file".to_vec()],
            size: None,
        })
        .map_err(|error| format!("{error:?}"))?;
    let token = build
        .begin_folder_attempt(0, &[], 180)
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
        .record_folder_attempt_event(
            token,
            3,
            FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::Progress),
        )
        .map_err(|error| format!("{error:?}"))?;

    let blocker = sink
        .folder_evidence_budget
        .reserve_remaining()
        .ok_or("reserve remaining folder evidence budget failed")?;
    assert_eq!(
        build.record_folder_attempt_event(
            token,
            4,
            FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::Progress),
        ),
        Err(FolderAttemptError::CapacityExceeded)
    );
    drop(blocker);

    let result = build
        .publish_partial(id(181)?)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(summary(&result, 180)?.status, FolderAttemptStatus::Held);
    assert!(
        result
            .items()
            .iter()
            .any(|item| item.name.as_slice() == b"file")
    );
    Ok(())
}

#[test]
fn rootless_build_rejects_folder_attempt_registration() -> Result<(), String> {
    let mut build = IngestSink::default()
        .open(id(190)?)
        .map_err(|error| format!("{error:?}"))?;
    build
        .add_directory(Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        build.begin_folder_attempt(0, &[], 190),
        Err(FolderAttemptError::InvalidDirectory)
    );
    Ok(())
}
