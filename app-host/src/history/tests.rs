use super::*;
use du_core_contracts::{
    CallEnvelope, ErrorCode, EventEnvelope, PathBytes, ResultEnvelope,
    analysis_ipc::{ScanStart, ScopeReview},
    history_ipc::{HISTORY_CHANGED, HistoryChangedBody, HistoryListResponse},
    job::{HistoryAppend, JobHistory, JobState, JobSummary},
};
use du_engine_api::api::{CallerCtx, Channel, Engine, EngineBuilder};
use serde_json::{Value, json};
use std::{
    os::windows::ffi::OsStrExt,
    sync::Arc,
    time::{Duration, Instant},
};

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
    ipc(engine, "history.list", body)
}

fn ipc(
    engine: &Engine,
    name: &str,
    body: Value,
) -> Result<ResultEnvelope, Box<dyn std::error::Error>> {
    let id = Id::from_v7_bytes([42; 16]);
    let message = json!({
        "command": "du_call",
        "args": {"envelope": {
            "name": name, "contractVersion": 1, "correlationId": id, "body": body
        }}
    });
    let response = crate::ipc::dispatch(engine, &message.to_string()).ok_or("no response")?;
    let envelope: ResultEnvelope = serde_json::from_str(&response)?;
    assert_eq!(envelope.correlation_id, id);
    Ok(envelope)
}

#[test]
fn windows_scan_delivers_a_history_change_only_for_a_committed_revision()
-> Result<(), Box<dyn std::error::Error>> {
    let profile = tempfile::tempdir()?;
    let target = tempfile::tempdir()?;
    std::fs::write(target.path().join("test.txt"), b"history event fixture")?;
    let database = crate::startup_database::open(profile.path())?;
    let reader = HistoryReader::open(profile.path(), database.clone())?;
    let mut builder = EngineBuilder::default();
    let random = &du_platform_os::entropy::OsRandom;
    let events = du_engine_api::desktop::register(
        &mut builder,
        Arc::new(du_platform_os::windows_observation::WindowsScopeRoots),
        Arc::new(du_platform_os::volume::WindowsVolumes),
        Arc::new(database.history(|| 400)),
        random,
    )?;
    du_engine_api::history::register(&mut builder, reader, random)
        .map_err(|error| format!("{error:?}"))?;
    let engine = builder.build()?;
    let bytes: Vec<_> = target
        .path()
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect();
    let draft = json!({
        "includes": [{
            "encoding": "windows_utf16le", "raw": PathBytes::from_bytes(&bytes),
            "display": target.path(), "lossy": false
        }],
        "exclusions": [], "boundary": "current_volume", "includedMounts": [], "source": "gui"
    });
    let start = |request_seq| -> Result<ScanStart, Box<dyn std::error::Error>> {
        let review = ipc(
            &engine,
            "scope.review",
            json!({"draft": draft, "requestSeq": request_seq}),
        )?;
        assert!(review.error.is_none(), "{:?}", review.error);
        let review: ScopeReview = serde_json::from_value(review.body)?;
        let scan = ipc(
            &engine,
            "scan.start",
            json!({"draft": draft, "reviewFingerprint": review.review_fingerprint}),
        )?;
        assert!(scan.error.is_none(), "{:?}", scan.error);
        Ok(serde_json::from_value(scan.body)?)
    };
    let sql = rusqlite::Connection::open(profile.path().join("diskunfold.db"))?;
    sql.execute_batch("CREATE TRIGGER reject_link BEFORE INSERT ON job_runtime_job_link BEGIN SELECT RAISE(ABORT, 'fixture'); END;")?;
    let rejected = start(1)?;
    let failed_events = terminal_batch(&events, &rejected.job_ref)?;
    assert!(
        !failed_events
            .iter()
            .any(|event| event.topic == HISTORY_CHANGED)
    );
    let finished = failed_events
        .last()
        .ok_or("missing failed-write terminal")?;
    assert_eq!(finished.body["state"], "done");
    assert_eq!(finished.body["historyRecorded"], false);
    let before: HistoryListResponse = serde_json::from_value(call(&engine, json!({}))?.body)?;
    assert!(before.rows.is_empty());

    sql.execute_batch("DROP TRIGGER reject_link")?;
    let saved = start(2)?;
    assert_ne!(saved.job_ref, rejected.job_ref);
    let committed = terminal_batch(&events, &saved.job_ref)?;
    let finished = committed
        .iter()
        .find(|event| event.topic == "job.finished")
        .ok_or("missing terminal")?;
    let changes: Vec<_> = committed
        .iter()
        .filter(|event| event.topic == HISTORY_CHANGED)
        .collect();
    assert_eq!(changes.len(), 1);
    assert!(finished.seq < changes[0].seq);
    assert_eq!(finished.body["historyRecorded"], true);
    let changed: HistoryChangedBody = serde_json::from_value(changes[0].body.clone())?;
    let after: HistoryListResponse = serde_json::from_value(call(&engine, json!({}))?.body)?;
    assert_eq!(changed.revision, after.revision);
    assert_eq!(after.revision.max_record_seq, "1");
    assert_eq!(after.rows[0].job_ref.as_ref(), Some(&saved.job_ref));
    Ok(())
}

fn terminal_batch(
    source: &du_engine_api::desktop::DesktopEvents,
    job: &Id,
) -> Result<Vec<EventEnvelope>, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut terminal_seen = None;
    let mut events = Vec::new();
    while Instant::now() < deadline {
        if let Some(json) = source.next_json() {
            let event: EventEnvelope = serde_json::from_str(&json)?;
            if event.topic == "job.finished" && event.job_ref.as_ref() == Some(job) {
                terminal_seen = Some(Instant::now());
            }
            events.push(event);
        } else if terminal_seen.is_some_and(|at| at.elapsed() >= Duration::from_millis(250)) {
            return Ok(events);
        } else {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    Err("timed out waiting for scan completion".into())
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
