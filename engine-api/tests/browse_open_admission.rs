//! End-to-end admission of F05 typed requests into a pinned query session.

use std::sync::Arc;

use du_analysis_index::{IngestSink, Observation};
use du_core_contracts::{
    Id, browse_open_ipc::BrowseOpenRequest, browse_page_ipc::ResultOrigin,
    result::ResultCompleteness,
};
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
        "condition": { "kind": "any", "children": [
            { "kind": "predicate", "predicate": { "kind": "type", "value": "directory" } },
            { "kind": "predicate", "predicate": { "kind": "size", "basis": "logical", "op": "at_least", "bytes": "1" } }
        ] }
    }))
    .map_err(|error| error.to_string())?;
    let open = session
        .open_live(request, || false)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(open.request_seq, 9);
    assert_eq!(open.page.opened.sort_strategies[0].id, "sort.path");
    assert_eq!(open.page.opened.sort_strategies[0].revision, 1);
    let first = open.page.opened.page.clone();
    assert_eq!(first.result_version, id(2));
    assert_eq!(first.completeness, ResultCompleteness::Partial);
    assert_eq!(first.unknown_count, 1);
    assert_eq!(first.rows[0].observation.name, b"a");
    let cursor = open
        .page
        .next_cursor
        .clone()
        .ok_or("missing cursor for second folder")?;
    let wire = open
        .into_response(ResultOrigin("scan".into()))
        .map_err(|error| format!("{error:?}"))?;
    let body = serde_json::to_value(wire).map_err(|error| error.to_string())?;
    assert_eq!(body["requestSeq"], 9);
    assert_eq!(body["rootKind"], "scope_root");
    assert_eq!(body["origin"], "scan");
    assert_eq!(body["page"]["unknownCount"], 1);
    let row = &body["page"]["rows"][0];
    assert_eq!(row["flags"]["unenumerated"]["state"], "unknown");
    assert_eq!(row["flags"]["excluded"]["state"], "unknown");
    assert_eq!(row["correspondence"]["state"], "unknown");
    let continuation = serde_json::from_value(json!({ "nextCursor": cursor, "count": 1 }))
        .map_err(|error| error.to_string())?;
    let next = session
        .read_request(continuation)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(next.page.rows[0].observation.name, b"b");
    assert_eq!(next.page.result_version, first.result_version);
    assert_eq!(next.page.unknown_count, 1);
    assert!(next.next_cursor.is_none());
    let position = serde_json::from_value(json!({
        "queryRef": first.query_ref, "resultVersion": first.result_version, "start": 1, "count": 1
    }))
    .map_err(|error| error.to_string())?;
    let same = session
        .read_request(position)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(same.page, next.page);
    let wire = next.into_response().map_err(|error| format!("{error:?}"))?;
    let body = serde_json::to_value(wire).map_err(|error| error.to_string())?;
    assert_eq!(body["rowKind"], "folder");
    assert_eq!(body["resultVersion"], json!(first.result_version));
    assert_eq!(body["unknownCount"], 1);
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
