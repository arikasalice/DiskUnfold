use super::*;

#[test]
fn history_projection_omits_scope_roots_for_records_without_stored_roots()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let database = crate::startup_database::open(dir.path())?;
    append(&database, 1, 100)?;
    let sql = rusqlite::Connection::open(dir.path().join("diskunfold.db"))?;
    let job_id = Id::from_v7_bytes([1; 16])
        .v7_bytes()
        .ok_or("invalid fixture job id")?;

    let mut builder = EngineBuilder::default();
    let random = &du_platform_os::entropy::OsRandom;
    du_engine_api::history::register(
        &mut builder,
        HistoryReader::open(dir.path(), database.clone())?,
        random,
    )
    .map_err(|error| format!("{error:?}"))?;
    du_engine_api::history::detail::register(
        &mut builder,
        HistoryReader::open(dir.path(), database)?,
    );
    let engine = builder.build()?;
    let list = ipc(&engine, "history.list", json!({}))?;
    let response: HistoryListResponse = serde_json::from_value(list.body.clone())?;
    let summary = response.rows[0]
        .summary
        .as_ref()
        .ok_or("history summary missing")?;
    assert_eq!(summary.scope_first_root.as_deref(), Some(r"C:\fixture\1"));
    assert_eq!(summary.scope_root_count, Some(1));
    let summary_json = list.body["rows"][0]["summary"]
        .as_object()
        .ok_or("history summary is not an object")?;
    assert_eq!(summary_json["scopeFirstRoot"], r"C:\fixture\1");
    assert_eq!(summary_json["scopeRootCount"], 1);

    let detail = ipc(
        &engine,
        "history.get",
        json!({"jobRef": Id::from_v7_bytes([1; 16])}),
    )?;
    let detail_value = detail.body.clone();
    let detail: HistoryDetail = serde_json::from_value(detail.body)?;
    assert_eq!(detail.scope_roots, Some(vec![r"C:\fixture\1".to_owned()]));
    assert_eq!(detail_value["scopeRoots"][0], r"C:\fixture\1");

    sql.execute_batch("BEGIN IMMEDIATE; UPDATE app_db_purge_gate SET open=1;")?;
    sql.execute(
        "DELETE FROM job_runtime_job_scope_root WHERE job_id=?1",
        [job_id.as_slice()],
    )?;
    sql.execute_batch("UPDATE app_db_purge_gate SET open=0; COMMIT;")?;

    let list = ipc(&engine, "history.list", json!({}))?;
    let response: HistoryListResponse = serde_json::from_value(list.body.clone())?;
    let summary = response.rows[0]
        .summary
        .as_ref()
        .ok_or("history summary missing")?;
    assert_eq!(summary.scope_first_root, None);
    assert_eq!(summary.scope_root_count, None);
    let summary_json = list.body["rows"][0]["summary"]
        .as_object()
        .ok_or("history summary is not an object")?;
    assert!(!summary_json.contains_key("scopeFirstRoot"));
    assert!(!summary_json.contains_key("scopeRootCount"));

    let detail = ipc(
        &engine,
        "history.get",
        json!({"jobRef": Id::from_v7_bytes([1; 16])}),
    )?;
    let detail_value = detail.body.clone();
    let detail: HistoryDetail = serde_json::from_value(detail.body)?;
    assert_eq!(detail.scope_roots, None);
    let detail_json = detail_value
        .as_object()
        .ok_or("history detail is not an object")?;
    assert!(!detail_json.contains_key("scopeRoots"));

    sql.execute(
        "INSERT INTO job_runtime_job_scope_root
             (job_id, root_seq, location_text, created_at_ms)
         VALUES (?1, 1, '/unexpected', 101)",
        [job_id.as_slice()],
    )?;
    let unreadable = ipc(&engine, "history.list", json!({}))?;
    let unreadable: HistoryListResponse = serde_json::from_value(unreadable.body)?;
    assert!(
        unreadable.rows[0]
            .missing_reasons
            .iter()
            .any(|reason| reason == "read_failed")
    );
    let unreadable = ipc(
        &engine,
        "history.get",
        json!({"jobRef": Id::from_v7_bytes([1; 16])}),
    )?;
    let unreadable: HistoryDetail = serde_json::from_value(unreadable.body)?;
    assert_eq!(unreadable.scope_roots, None);
    assert!(
        unreadable
            .missing_reasons
            .iter()
            .any(|reason| reason == "read_failed")
    );
    Ok(())
}
