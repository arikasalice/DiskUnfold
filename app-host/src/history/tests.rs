use super::*;
use du_core_contracts::{
    CallEnvelope, ErrorCode, EventEnvelope, PathBytes, ResultEnvelope,
    analysis_ipc::{AnalysisJobListRow, AnalysisJobStatus, ScanStart, ScopeReview, SpaceTotal},
    history_ipc::{
        HISTORY_CHANGED, HistoryChangedBody, HistoryDetail, HistoryDetailCompleteness,
        HistoryEndState, HistoryFollowUp, HistoryListResponse, HistoryRelation,
    },
    job::{HistoryAppend, JobHistory, JobState, JobSummary},
};
use du_engine_api::api::{CallerCtx, Channel, Engine, EngineBuilder};
use serde_json::{Value, json};
use std::{
    os::windows::ffi::OsStrExt,
    sync::Arc,
    time::{Duration, Instant},
};

mod async_browse;
mod items;
mod many_roots;
mod measurement;
mod scan_cancellation;
mod scope_replacement;

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
fn registered_detail_distinguishes_unreadable_removed_and_readable_history()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let database = crate::startup_database::open(dir.path())?;
    append(&database, 1, 100)?;
    append(&database, 2, 200)?;
    let sql = rusqlite::Connection::open(dir.path().join("diskunfold.db"))?;
    sql.execute_batch(
        "DROP TRIGGER job_runtime_job_link_no_update;
         UPDATE job_runtime_job_link
           SET predecessor_job_id=X'01010101010171018101010101010101',
               relation_kind='retry', plan_version_id=X'03030303030373038303030303030303'
           WHERE job_id=X'02020202020272028202020202020202';",
    )?;
    let mut builder = EngineBuilder::default();
    du_engine_api::history::detail::register(
        &mut builder,
        HistoryReader::open(dir.path(), database)?,
    );
    let engine = builder.build()?;
    let original = Id::from_v7_bytes([1; 16]);
    let retry = Id::from_v7_bytes([2; 16]);
    let get = |job: &Id| ipc(&engine, "history.get", json!({"jobRef": job}));

    let follow_ups = assert_complete_detail(serde_json::from_value(get(&original)?.body)?, &retry)?;

    let plan_missing: HistoryDetail = serde_json::from_value(get(&retry)?.body)?;
    assert_eq!(
        plan_missing.workflow.executed_plan,
        Some(Id::from_v7_bytes([3; 16]))
    );
    assert!(plan_missing.workflow.plan_versions.is_none());
    assert_eq!(plan_missing.missing_reasons, ["plan_unavailable"]);

    sql.execute_batch(
        "DROP TRIGGER job_runtime_job_record_no_update;
         PRAGMA ignore_check_constraints=ON;
         UPDATE job_runtime_job_record SET final_state='unrecognized'
          WHERE job_id=X'01010101010171018101010101010101';",
    )?;
    let partial: HistoryDetail = serde_json::from_value(get(&original)?.body)?;
    assert_eq!(partial.completeness, HistoryDetailCompleteness::Partial);
    assert!(partial.summary.is_none());
    assert_eq!(partial.missing_reasons, ["read_failed"]);
    assert_eq!(partial.follow_ups, Some(follow_ups));

    sql.execute_batch(
        "BEGIN IMMEDIATE;
         UPDATE app_db_purge_gate SET open=1;
         DELETE FROM job_runtime_job_link WHERE job_id=X'01010101010171018101010101010101';
         DELETE FROM job_runtime_job_record WHERE job_id=X'01010101010171018101010101010101';
         UPDATE app_db_purge_gate SET open=0;
         COMMIT;",
    )?;
    let removed = get(&original)?.error.ok_or("missing removed error")?;
    assert_eq!(removed.code, ErrorCode::NotFound);
    assert_eq!(removed.reason_key, "history.removed");
    let orphan: HistoryDetail = serde_json::from_value(get(&retry)?.body)?;
    assert!(
        orphan
            .summary
            .ok_or("missing retry summary")?
            .predecessor_job_ref
            .is_none()
    );
    assert!(
        orphan
            .missing_reasons
            .iter()
            .any(|reason| reason == "read_failed")
    );

    assert_detail_requires_gui(&engine, &retry)?;
    sql.execute_batch("DROP TABLE job_runtime_job_start")?;
    let unavailable = get(&retry)?.error.ok_or("missing unavailable error")?;
    assert_eq!(unavailable.code, ErrorCode::Internal);
    assert_eq!(unavailable.reason_key, "history.storage_unavailable");
    Ok(())
}

fn assert_complete_detail(
    complete: HistoryDetail,
    retry: &Id,
) -> Result<Vec<HistoryFollowUp>, Box<dyn std::error::Error>> {
    assert_eq!(complete.completeness, HistoryDetailCompleteness::Complete);
    let summary = complete.summary.ok_or("missing original summary")?;
    assert_eq!(summary.scope_summary.as_deref(), Some("metadata: 1 root"));
    assert_eq!(summary.state, HistoryEndState::Done);
    assert_eq!(
        summary.counts.ok_or("missing recorded counts")?.completed,
        "0"
    );
    assert_eq!(complete.effects.confirmed, None);
    assert_eq!(complete.workflow.plan_versions, Some(vec![]));
    let follow_ups = complete.follow_ups.ok_or("missing follow-up list")?;
    assert_eq!(follow_ups.len(), 1);
    assert_eq!(follow_ups[0].job_ref.as_ref(), Some(retry));
    assert_eq!(follow_ups[0].relation, Some(HistoryRelation::Retry));
    assert_eq!(follow_ups[0].state, Some(HistoryEndState::Done));
    assert_eq!(follow_ups[0].finished_at_ms, Some(200));
    Ok(follow_ups)
}

fn assert_detail_requires_gui(
    engine: &Engine,
    retry: &Id,
) -> Result<(), Box<dyn std::error::Error>> {
    for channel in [Channel::Cli, Channel::Mcp] {
        let id = Id::from_v7_bytes([41; 16]);
        let response = engine.call(
            CallEnvelope {
                name: "history.get".into(),
                contract_version: 1,
                correlation_id: id.clone(),
                body: json!({"jobRef": retry}),
            },
            &CallerCtx::new(channel, id, None),
        );
        assert_eq!(
            response
                .error
                .ok_or("detail exposed outside GUI")?
                .reason_key,
            "command.not_provided"
        );
    }
    let invalid = ipc(engine, "history.get", json!({"jobRef": "invalid"}))?;
    assert_eq!(
        invalid.error.ok_or("invalid reference accepted")?.code,
        ErrorCode::InvalidArgument
    );
    Ok(())
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
    assert_recovered_views(&engine, &saved, &committed)?;
    Ok(())
}

fn assert_recovered_views(
    engine: &Engine,
    saved: &ScanStart,
    committed: &[EventEnvelope],
) -> Result<(), Box<dyn std::error::Error>> {
    // Treat the drained terminal, result, and committed history events as lost
    // to the window. Recover each view through the ordinary GUI IPC entrypoint.
    let list = ipc(engine, "job.list", json!({}))?;
    assert!(list.error.is_none(), "{:?}", list.error);
    let list: Vec<AnalysisJobListRow> = serde_json::from_value(list.body)?;
    let row = list
        .iter()
        .find(|row| row.job_ref == saved.job_ref)
        .ok_or("missing job")?;
    let status = ipc(engine, "job.get", json!({"jobRef": saved.job_ref}))?;
    assert!(status.error.is_none(), "{:?}", status.error);
    let status: AnalysisJobStatus = serde_json::from_value(status.body)?;
    let terminal = status.terminal.as_ref().ok_or("unconfirmed terminal")?;
    assert_eq!(row.terminal.as_ref(), Some(terminal));
    assert_eq!(status.state, JobState::Done);
    assert!(terminal.history_recorded);
    let result = status.scan_result.as_ref().ok_or("missing scan lineage")?;
    assert_eq!(Some(&result.lineage_id), saved.lineage_id.as_ref());
    let version = result
        .result_version
        .as_ref()
        .ok_or("missing result version")?;
    assert_eq!(status.result_version.as_ref(), Some(version));
    assert!(
        committed
            .iter()
            .any(|event| event.topic == "results.version_changed"
                && event.result_version.as_ref() == Some(version))
    );
    let totals = ipc(
        engine,
        "space.total",
        json!({"resultVersion": version, "requestSeq": 7, "target": {"kind": "scope_root"}}),
    )?;
    assert!(totals.error.is_none(), "{:?}", totals.error);
    let totals: SpaceTotal = serde_json::from_value(totals.body)?;
    assert_eq!(&totals.result_version, version);
    assert_eq!(totals.request_seq, 7);
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
