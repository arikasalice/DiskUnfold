//! End-to-end admission of F05 typed requests into a pinned query session.

use std::sync::Arc;

use du_analysis_index::{IngestSink, Observation};
use du_core_contracts::{Id, browse_open_ipc::BrowseOpenRequest, result::ResultCompleteness};
use du_engine_api::browse::folder::{
    AdmissionError, Error, FolderBrowse,
    cursor::{CursorError, Session},
};
use du_job_runtime::JobIdentity;
use serde_json::json;

struct Identity;

impl JobIdentity for Identity {
    fn next_id(&self) -> Id {
        id(3)
    }
}

fn id(value: u8) -> Id {
    Id::from_v7_bytes([value; 16])
}

#[test]
fn filters_and_continues_on_the_admitted_version() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)).map_err(|error| format!("{error:?}"))?;
    for path in [vec![], vec![b"a".to_vec()], vec![b"b".to_vec()]] {
        build
            .add_directory(path)
            .map_err(|error| format!("{error:?}"))?;
    }
    build
        .add_file(Observation {
            path: vec![b"file".to_vec()],
            size: None,
        })
        .map_err(|error| format!("{error:?}"))?;
    build
        .seal(id(2), ResultCompleteness::Partial)
        .map_err(|error| format!("{error:?}"))?;
    let session = Session::new(
        Arc::new(FolderBrowse::new(sink, Arc::new(Identity))),
        [1; 32],
    );
    let request: BrowseOpenRequest = serde_json::from_value(json!({
        "viewKind": "list", "requestSeq": 9, "pageSize": 1,
        "condition": { "kind": "predicate", "predicate": { "kind": "type", "value": "directory" } }
    }))
    .map_err(|error| error.to_string())?;
    let open = session
        .open_live(request, || false)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(open.request_seq, 9);
    let first = open.page.opened.page;
    assert_eq!(first.result_version, id(2));
    assert_eq!(first.completeness, ResultCompleteness::Partial);
    assert_eq!(first.rows[0].observation.name, b"a");
    let cursor = open
        .page
        .next_cursor
        .ok_or("missing cursor for second folder")?;
    let next = session
        .page_for(&cursor, &first.query_ref, &first.result_version, Some(1))
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(next.page.rows[0].observation.name, b"b");
    assert_eq!(next.page.result_version, first.result_version);
    assert!(next.next_cursor.is_none());
    Ok(())
}

#[test]
fn rejects_unresolved_sources_and_invalid_page_size() -> Result<(), serde_json::Error> {
    let session = Session::new(
        Arc::new(FolderBrowse::new(IngestSink::default(), Arc::new(Identity))),
        [1; 32],
    );
    let unresolved = serde_json::from_value(json!({
        "viewKind": "list", "requestSeq": 1, "sourceRef": { "kind": "scan", "ref": id(1) }
    }))?;
    assert!(matches!(
        session.open_live(unresolved, || false),
        Err(AdmissionError::SourceUnresolved)
    ));
    let invalid_count =
        serde_json::from_value(json!({ "viewKind": "list", "requestSeq": 1, "pageSize": 0 }))?;
    assert!(matches!(
        session.open_live(invalid_count, || false),
        Err(AdmissionError::Browse(CursorError::Folder(
            Error::InvalidPage
        )))
    ));
    Ok(())
}
