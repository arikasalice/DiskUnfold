use super::*;

#[test]
fn replacement_pins_new_evidence_without_mutating_the_previous_version() -> Result<(), String> {
    let sink = IngestSink::default();
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"root"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    sink.bind_current_analysis("replacement-evidence", id(500)?)
        .map_err(|error| format!("{error:?}"))?;
    let mut original = sink
        .open_forest_in_lineage(id(50)?, vec![root], id(500)?)
        .map_err(|error| format!("{error:?}"))?;
    original
        .add_directory(Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    let original_token = original
        .begin_folder_attempt(0, &[], 4)
        .map_err(|error| format!("{error:?}"))?;
    original
        .record_folder_attempt_event(original_token, 0, exclude(b"old"))
        .map_err(|error| format!("{error:?}"))?;
    original
        .end_folder_attempt(original_token, 1, 1)
        .map_err(|error| format!("{error:?}"))?;
    let previous = original
        .seal(
            id(51).map_err(|error| error.to_string())?,
            ResultCompleteness::Partial,
        )
        .map_err(|error| format!("{error:?}"))?;

    let mut replacement = sink
        .open_replacement(&previous)
        .map_err(|error| format!("{error:?}"))?;
    replacement
        .add_directory(Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    let replacement_token = replacement
        .begin_folder_attempt(0, &[], 4)
        .map_err(|error| format!("{error:?}"))?;
    replacement
        .record_folder_attempt_event(replacement_token, 0, exclude(b"new"))
        .map_err(|error| format!("{error:?}"))?;
    replacement
        .end_folder_attempt(replacement_token, 1, 1)
        .map_err(|error| format!("{error:?}"))?;
    let current = replacement
        .seal(
            id(52).map_err(|error| error.to_string())?,
            ResultCompleteness::Partial,
        )
        .map_err(|error| format!("{error:?}"))?;

    assert_eq!(
        previous
            .folder_attempt_summaries()
            .map(|rows| rows[0].attempt_id),
        Some(4)
    );
    assert_eq!(
        previous
            .folder_attempt_summaries()
            .map(|rows| rows[0].status),
        Some(FolderAttemptStatus::EndedUnproven)
    );
    assert_eq!(
        current
            .folder_attempt_summaries()
            .map(|rows| rows[0].attempt_id),
        Some(4)
    );
    assert_eq!(
        current
            .folder_attempt_summaries()
            .map(|rows| rows[0].status),
        Some(FolderAttemptStatus::EndedUnproven)
    );
    Ok(())
}
