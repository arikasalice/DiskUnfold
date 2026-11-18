//! The hidden size aggregation rule setting at startup and scan admission (F03 FR14.HD2.app-host.DD4).
use super::*;
use du_core_contracts::size_rule::DEFAULT_SIZE_RULE_ID;
use du_user_env::space::AGGREGATION_RULE;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// A second registered rule, which the production bundle does not have yet.
const OTHER: &str = "size.agg.other";

struct Allow;
impl OutboundFilter for Allow {
    fn filter(
        &self,
        _: &str,
        _: &CallerCtx,
        response: serde_json::Value,
    ) -> Result<serde_json::Value, EngineError> {
        Ok(response)
    }
}

fn ids() -> Vec<String> {
    vec![DEFAULT_SIZE_RULE_ID.to_owned(), OTHER.to_owned()]
}

/// Stores `id` the way an earlier write of this setting did.
fn store(path: &Path, id: &str) -> TestResult {
    let mut db = AppDb::open_or_create_gui(path).map_err(|e| format!("{e:?}"))?;
    db.commit(
        0,
        &SettingsChanges::from([(AGGREGATION_RULE.into(), Some(json!(id)))]),
        context(),
    )
    .map_err(|e| format!("{e:?}"))?;
    Ok(())
}

fn open_with(path: &Path, registered: Vec<String>) -> Result<Arc<StartupSettings>, io::Error> {
    Ok(Arc::new(StartupSettings::open_with_size_rule_ids(
        &test_database(path)?,
        100,
        &["en", "ko"],
        registered,
    )?))
}

#[test]
fn an_absent_value_names_the_default_rule() -> TestResult {
    let dir = tempfile::tempdir()?;
    let settings = open_with(&dir.path().join("absent.db"), ids())?;
    assert_eq!(settings.size_rule_source()(), DEFAULT_SIZE_RULE_ID);
    Ok(())
}

#[test]
fn a_stored_registered_id_is_what_the_source_returns() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("stored.db");
    store(&path, OTHER)?;
    let settings = open_with(&path, ids())?;
    assert!(settings.status.invalid_keys.is_empty());
    assert_eq!(settings.size_rule_source()(), OTHER);
    Ok(())
}

#[test]
fn a_stored_unregistered_id_yields_the_default_and_keeps_the_row_in_quarantine() -> TestResult {
    // A downgrade: the stored rule is not registered by this build.
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("downgrade.db");
    store(&path, OTHER)?;
    let settings = StartupSettings::open(&test_database(&path)?, 100, &["en", "ko"])?;
    assert_eq!(settings.status.invalid_keys, [AGGREGATION_RULE]);
    assert_eq!(
        Arc::new(settings).size_rule_source()(),
        DEFAULT_SIZE_RULE_ID
    );
    let db = Connection::open(&path)?;
    let (live, quarantined): (i64, i64) = db.query_row(
        "SELECT (SELECT count(*) FROM user_env_setting WHERE setting_key = ?1),
                (SELECT count(*) FROM user_env_setting_quarantine WHERE setting_key = ?1)",
        [AGGREGATION_RULE],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    assert_eq!((live, quarantined), (0, 1));
    Ok(())
}

#[test]
fn a_write_rejects_unregistered_ids_and_applies_from_the_next_read() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("write.db");
    let settings = open_with(&path, ids())?;
    let source = settings.clone().size_rule_source();
    assert_eq!(source(), DEFAULT_SIZE_RULE_ID);
    {
        let mut service = settings.service.lock().map_err(|_| "poisoned")?;
        let bad = SettingsChanges::from([(AGGREGATION_RULE.into(), Some(json!("size.agg.nope")))]);
        assert_eq!(service.review(0, &bad), Err(SettingsError::InvalidValue));
        let good = SettingsChanges::from([(AGGREGATION_RULE.into(), Some(json!(OTHER)))]);
        let hash = service.review(0, &good).map_err(|e| format!("{e:?}"))?;
        service
            .apply(0, &good, hash, context())
            .map_err(|e| format!("{e:?}"))?;
    }
    // The same source now names the new rule; an analysis that already read it keeps its own.
    assert_eq!(source(), OTHER);
    drop(source);
    drop(settings);
    assert_eq!(open_with(&path, ids())?.size_rule_source()(), OTHER);
    Ok(())
}

#[test]
fn the_restore_report_never_lists_the_setting() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("report.db");
    store(&path, OTHER)?;
    let settings = open_with(&path, ids())?;
    assert_eq!(
        settings
            .service
            .lock()
            .map_err(|_| "poisoned")?
            .snapshot()
            .values
            .get(AGGREGATION_RULE),
        Some(&json!(OTHER))
    );
    let mut builder = EngineBuilder::default();
    builder.with_outbound_filter(Allow);
    settings.register_restore_status(&mut builder);
    let engine = builder.build()?;
    let envelope = json!({
        "name": "settings.restore_status", "contractVersion": 1,
        "correlationId": "0195f856-ab83-7a59-9343-10d29775c9a9", "body": {}
    });
    let request = json!({"command": "du_call", "args": {"envelope": envelope}});
    let reply = crate::ipc::dispatch(&engine, &request.to_string()).ok_or("no reply")?;
    let result: ResultEnvelope = serde_json::from_str(&reply)?;
    assert!(result.error.is_none());
    let items = result.body["items"].as_array().ok_or("no items")?;
    assert_eq!(items.len(), 12);
    assert!(items.iter().all(|item| item["key"] != AGGREGATION_RULE));
    assert!(!result.body.to_string().contains("aggregation"));
    Ok(())
}
