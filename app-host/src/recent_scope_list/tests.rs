use super::*;
use du_app_db::AppDb;
use du_core_contracts::{CallEnvelope, ENVELOPE_VERSION, ErrorCode, ResultEnvelope};
use rusqlite::{Connection, params};
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn identifier(n: u8) -> [u8; 16] {
    let mut bytes = [0; 16];
    bytes[6] = 0x70;
    bytes[8] = 0x80;
    bytes[15] = n;
    bytes
}

fn bounded_identifier(index: usize) -> [u8; 16] {
    let mut bytes = [0; 16];
    bytes[6] = 0x70;
    bytes[8] = 0x80;
    bytes[14..].copy_from_slice(&(index as u16).to_be_bytes());
    bytes
}

fn create_database(path: &std::path::Path) -> Result<AppDb, Box<dyn std::error::Error>> {
    AppDb::create_gui_with_history(path).map_err(|error| format!("{error:?}").into())
}

fn insert(sql: &Connection, n: u8, count: u32, time: i64) -> rusqlite::Result<()> {
    sql.execute(
        "INSERT INTO user_env_recent_scope VALUES(?1,?2,1,'{}',?3,?4,?4,?4,1)",
        params![identifier(n), [n; 32], count, time],
    )?;
    Ok(())
}

fn engine(db: AppDb) -> Result<du_engine_api::api::Engine, Box<dyn std::error::Error>> {
    let mut builder = EngineBuilder::default();
    register(
        &mut builder,
        SharedDatabase::new(db, None),
        NonZeroUsize::new(3).ok_or("invalid cap")?,
    );
    Ok(builder.build()?)
}

fn ipc(
    engine: &du_engine_api::api::Engine,
    body: Value,
) -> Result<ResultEnvelope, Box<dyn std::error::Error>> {
    let message = json!({"command":"du_call", "args":{"envelope":{
        "name":"scope.recent_list", "contractVersion":ENVELOPE_VERSION,
        "correlationId":Id::from_v7_bytes(identifier(9)), "body":body,
    }}})
    .to_string();
    let response = crate::ipc::dispatch(engine, &message).ok_or("missing list response")?;
    Ok(serde_json::from_str(&response)?)
}

#[test]
fn gui_returns_only_ordered_summary_fields_when_remembering_is_off() -> TestResult {
    use du_core_contracts::settings::{
        SettingsChanges, SettingsCommit, SettingsSource, SettingsStore,
    };

    let dir = tempfile::tempdir()?;
    let path = dir.path().join("scope.db");
    let mut db = create_database(&path)?;
    db.commit(
        0,
        &SettingsChanges::from([(
            du_user_env::scope::REMEMBER_RECENT.into(),
            Some(json!(false)),
        )]),
        SettingsCommit {
            source: SettingsSource::Screen,
            timestamp_ms: 40,
        },
    )
    .map_err(|error| format!("{error:?}"))?;
    let sql = Connection::open(&path)?;
    insert(&sql, 3, 1, 20)?;
    insert(&sql, 2, 2, 40)?;
    insert(&sql, 1, 3, 40)?;
    let response = ipc(&engine(db)?, json!({}))?;
    assert!(response.error.is_none());
    assert_eq!(
        response.body,
        json!({"rows":[
            {"recentScopeRef":Id::from_v7_bytes(identifier(1)),"includeCount":3,"lastUsedAt":"1970-01-01T00:00:00.040Z"},
            {"recentScopeRef":Id::from_v7_bytes(identifier(2)),"includeCount":2,"lastUsedAt":"1970-01-01T00:00:00.040Z"},
            {"recentScopeRef":Id::from_v7_bytes(identifier(3)),"includeCount":1,"lastUsedAt":"1970-01-01T00:00:00.020Z"},
        ]})
    );
    Ok(())
}

#[test]
fn empty_list_has_strict_body_and_is_not_provided_to_cli_or_mcp() -> TestResult {
    let dir = tempfile::tempdir()?;
    let engine = engine(create_database(&dir.path().join("scope.db"))?)?;
    assert_eq!(ipc(&engine, json!({}))?.body, json!({"rows":[]}));
    assert_eq!(
        ipc(&engine, json!({"limit":1}))?
            .error
            .ok_or("missing invalid request error")?
            .code,
        ErrorCode::InvalidArgument
    );
    let id = Id::from_v7_bytes(identifier(9));
    for channel in [Channel::Cli, Channel::Mcp] {
        let response = engine.call(
            CallEnvelope {
                name: "scope.recent_list".into(),
                contract_version: ENVELOPE_VERSION,
                correlation_id: id.clone(),
                body: json!({}),
            },
            &CallerCtx::new(channel, id.clone(), None),
        );
        assert_eq!(
            response.error.ok_or("missing channel error")?.reason_key,
            "command.not_provided"
        );
    }
    Ok(())
}

#[test]
fn corrupt_summary_or_read_failure_is_an_internal_error_with_no_rows() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("scope.db");
    let engine = engine(create_database(&path)?)?;
    let sql = Connection::open(&path)?;
    let verify_error = || -> TestResult {
        let response = ipc(&engine, json!({}))?;
        let error = response.error.ok_or("missing read error")?;
        assert_eq!(error.code, ErrorCode::Internal);
        assert_eq!(error.reason_key, "support.internal_error");
        assert!(response.body.is_null());
        Ok(())
    };
    insert(&sql, 1, 1, 40)?;
    insert(&sql, 2, 1, i64::MIN)?;
    verify_error()?;
    sql.execute("DROP TABLE user_env_recent_scope", [])?;
    verify_error()
}

#[test]
fn list_uses_the_explicit_cap_supplied_by_fixed_composition() -> TestResult {
    let dir = tempfile::tempdir()?;
    let seed = |name: &str, count: usize| -> Result<AppDb, Box<dyn std::error::Error>> {
        let path = dir.path().join(name);
        let db = create_database(&path)?;
        let sql = Connection::open(&path)?;
        for index in 1..=count {
            let id = bounded_identifier(index);
            let mut fingerprint = [0; 32];
            fingerprint[..8].copy_from_slice(&(index as u64).to_be_bytes());
            sql.execute(
                "INSERT INTO user_env_recent_scope VALUES(?1,?2,1,'{}',1,?3,?3,?3,1)",
                params![id, fingerprint, index as i64],
            )?;
        }
        Ok(db)
    };

    let at_limit = ipc(&engine(seed("at-limit.db", 3)?)?, json!({}))?;
    assert!(at_limit.error.is_none());
    assert_eq!(at_limit.body["rows"].as_array().map(Vec::len), Some(3));

    let over_limit = ipc(&engine(seed("over-limit.db", 4)?)?, json!({}))?;
    assert_eq!(
        over_limit.error.ok_or("missing overflow error")?.reason_key,
        "support.internal_error"
    );
    assert!(over_limit.body.is_null());
    Ok(())
}
