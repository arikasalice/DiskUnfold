use super::*;
use du_core_contracts::{
    CallEnvelope, ErrorCode, ResultEnvelope,
    history_ipc::HistoryListResponse,
    job::{HistoryAppend, JobHistory, JobState, JobSummary},
};
use du_engine_api::api::{CallerCtx, Channel, Engine, EngineBuilder};
use serde_json::{Value, json};

fn append(database: &SharedDatabase, n: u8, time: i64) -> Result<(), String> {
    let id = Id::from_v7_bytes([n; 16]);
    let result = database.history(|| 400).append(&JobSummary {
        job_id: id.clone(),
        kind_id: "scan.metadata".into(),
        kind_version: 1,
        scope_summary: "metadata: 1 root".into(),
        state: JobState::Done,
        counts: Default::default(),
        failure_reason_key: None,
        unprocessed_summary: None,
        result_version_id: None,
        started_at_ms: Some(time - 1),
        finished_at_ms: time,
        correlation_id: id,
    })?;
    assert_eq!(result, HistoryAppend::Added);
    Ok(())
}

fn call(engine: &Engine, body: Value) -> Result<ResultEnvelope, Box<dyn std::error::Error>> {
    let id = Id::from_v7_bytes([42; 16]);
    let message = json!({
        "command": "du_call",
        "args": {"envelope": {
            "name": "history.list", "contractVersion": 1, "correlationId": id, "body": body
        }}
    });
    let response = crate::ipc::dispatch(engine, &message.to_string()).ok_or("no response")?;
    let envelope: ResultEnvelope = serde_json::from_str(&response)?;
    assert_eq!(envelope.correlation_id, id);
    Ok(envelope)
}

#[test]
fn registered_ipc_reads_sqlite_and_authenticates_the_next_page()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let database = crate::startup_database::open(dir.path())?;
    append(&database, 1, 100)?;
    append(&database, 2, 200)?;
    let reader = HistoryReader::open(dir.path(), database.clone())?;
    let mut builder = EngineBuilder::default();
    du_engine_api::history::register(&mut builder, reader, &du_platform_os::entropy::OsRandom)
        .map_err(|error| format!("{error:?}"))?;
    let engine = builder.build()?;
    for channel in [Channel::Cli, Channel::Mcp] {
        let id = Id::from_v7_bytes([41; 16]);
        let denied = engine.call(
            CallEnvelope {
                name: "history.list".into(),
                contract_version: 1,
                correlation_id: id.clone(),
                body: json!({}),
            },
            &CallerCtx::new(channel, id, None),
        );
        assert_eq!(
            denied.error.ok_or("non-GUI history access")?.reason_key,
            "command.not_provided"
        );
    }
    let filter = json!({"kinds": ["scan.metadata"], "states": ["done"]});
    let first = call(&engine, json!({"filter": filter, "count": 1}))?;
    assert!(first.error.is_none());
    let first: HistoryListResponse = serde_json::from_value(first.body)?;
    assert_eq!(first.revision.max_record_seq, "2");
    assert_eq!(first.rows[0].job_ref, Some(Id::from_v7_bytes([2; 16])));
    let summary = first.rows[0].summary.as_ref().ok_or("no summary")?;
    assert_eq!(summary.scope_summary.as_deref(), Some("metadata: 1 root"));
    assert_eq!(summary.session_restore_available, Some(false));
    let cursor = first.next_cursor.ok_or("no cursor")?;

    append(&database, 3, 300)?;
    let request = json!({"filter": filter, "cursor": cursor, "count": 1});
    let next = call(&engine, request.clone())?;
    assert!(next.error.is_none());
    let next: HistoryListResponse = serde_json::from_value(next.body)?;
    assert_eq!(next.revision.max_record_seq, "2");
    assert_eq!(next.rows[0].job_ref, Some(Id::from_v7_bytes([1; 16])));
    assert!(next.next_cursor.is_none());

    for invalid in [
        json!({"cursor": cursor}),
        json!({"filter": filter, "cursor": format!("{cursor}x")}),
        json!({"filter": {"unknown": true}}),
    ] {
        assert_eq!(
            call(&engine, invalid)?.error.ok_or("no error")?.code,
            ErrorCode::InvalidArgument
        );
    }
    let db = rusqlite::Connection::open(dir.path().join("diskunfold.db"))?;
    db.execute(
        "UPDATE job_runtime_history_state SET delete_generation=1",
        [],
    )?;
    let stale = call(&engine, request)?.error.ok_or("no stale error")?;
    assert_eq!(stale.code, ErrorCode::StaleVersion);
    assert_eq!(stale.reason_key, "history.version_changed");
    Ok(())
}
