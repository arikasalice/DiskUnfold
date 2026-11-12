use super::*;

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

fn call(
    engine: &du_engine_api::api::Engine,
    body: serde_json::Value,
) -> Result<ResultEnvelope, Box<dyn std::error::Error>> {
    let envelope = json!({"name":"space.display_preferences", "contractVersion":1, "correlationId":"0195f856-ab83-7a59-9343-10d29775c9a9", "body":body});
    let request = json!({"command":"du_call", "args":{"envelope":envelope}});
    let reply = crate::ipc::dispatch(engine, &request.to_string()).ok_or("no response")?;
    Ok(serde_json::from_str(&reply)?)
}

fn build_engine(
    settings: Arc<StartupSettings>,
) -> Result<du_engine_api::api::Engine, Box<dyn std::error::Error>> {
    let mut builder = EngineBuilder::default();
    builder.with_outbound_filter(Allow);
    settings.register_display_preferences(&mut builder);
    Ok(builder.build()?)
}

#[test]
fn gui_dispatch_reads_one_snapshot_and_rejects_wrong_channels_and_bodies()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("valid.db");
    let mut db = AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?;
    db.commit(
        0,
        &SettingsChanges::from([
            (space::SIZE_UNIT_SYSTEM.into(), Some(json!("si"))),
            (space::SIZE_UNIT.into(), Some(json!("MB"))),
            (space::DEFAULT_BASIS.into(), Some(json!("logical"))),
        ]),
        context(),
    )
    .map_err(|e| format!("{e:?}"))?;
    drop(db);
    let settings = Arc::new(StartupSettings::open(
        &test_database(&path)?,
        100,
        &["en", "ko"],
    )?);
    let engine = build_engine(settings.clone())?;
    let expected = json!({"settingsRevision":"1", "unitSystem":"si", "unit":"MB", "defaultBasis":"logical", "warningKey":null});
    assert_eq!(call(&engine, json!({}))?.body, expected);
    for body in [
        json!({"future":true}),
        json!([]),
        json!(null),
        json!("bad"),
        json!(1),
    ] {
        assert_eq!(
            call(&engine, body)?
                .error
                .ok_or("invalid display preference request accepted")?
                .reason_key,
            "command.body_invalid"
        );
    }
    {
        let mut service = settings.service.lock().map_err(|_| "poisoned")?;
        let changes = SettingsChanges::from([
            (space::SIZE_UNIT.into(), Some(json!("GB"))),
            (space::DEFAULT_BASIS.into(), Some(json!("unique_physical"))),
        ]);
        let hash = service.review(1, &changes).map_err(|e| format!("{e:?}"))?;
        service
            .apply(1, &changes, hash, context())
            .map_err(|e| format!("{e:?}"))?;
    }
    assert_eq!(
        call(&engine, json!({}))?.body,
        json!({"settingsRevision":"2", "unitSystem":"si", "unit":"GB", "defaultBasis":"unique_physical", "warningKey":null})
    );
    let envelope: CallEnvelope = serde_json::from_value(
        json!({"name":"space.display_preferences", "contractVersion":1, "correlationId":"0195f856-ab83-7a59-9343-10d29775c9a9", "body":{}}),
    )?;
    for channel in [Channel::Cli, Channel::Mcp] {
        let caller = CallerCtx::new(channel, envelope.correlation_id.clone(), None);
        assert!(engine.call(envelope.clone(), &caller).error.is_some());
    }
    Ok(())
}

#[test]
fn gui_dispatch_returns_registered_defaults_for_a_fresh_store()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("fresh.db");
    let settings = Arc::new(StartupSettings::open(
        &test_database(&path)?,
        100,
        &["en", "ko"],
    )?);
    let engine = build_engine(settings.clone())?;
    assert_eq!(
        call(&engine, json!({}))?.body,
        json!({"settingsRevision":"0", "unitSystem":"iec", "unit":"auto", "defaultBasis":"allocated", "warningKey":null})
    );
    let db = Connection::open(&path)?;
    db.execute_batch(
        "UPDATE user_env_setting_version SET schema_version=2; DROP TABLE user_env_setting;",
    )?;
    drop(db);
    let settings = Arc::new(StartupSettings::open(
        &test_database(&path)?,
        100,
        &["en", "ko"],
    )?);
    let engine = build_engine(settings)?;
    assert_eq!(
        call(&engine, json!({}))?.body,
        json!({"settingsRevision":"0", "unitSystem":"iec", "unit":"auto", "defaultBasis":"allocated", "warningKey":"settings.restore_failed"})
    );
    Ok(())
}

#[test]
fn gui_dispatch_projects_legacy_mismatch_and_restore_failure_without_writes()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("legacy.db");
    let mut db = AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?;
    db.commit(
        0,
        &SettingsChanges::from([
            (space::SIZE_UNIT_SYSTEM.into(), Some(json!("si"))),
            (space::SIZE_UNIT.into(), Some(json!("MiB"))),
        ]),
        context(),
    )
    .map_err(|e| format!("{e:?}"))?;
    drop(db);
    let settings = Arc::new(StartupSettings::open(
        &test_database(&path)?,
        100,
        &["en", "ko"],
    )?);
    let engine = build_engine(settings.clone())?;
    assert_eq!(
        call(&engine, json!({}))?.body,
        json!({"settingsRevision":"1", "unitSystem":"si", "unit":"auto", "defaultBasis":"allocated", "warningKey":"settings.value_reset"})
    );
    let db = Connection::open(&path)?;
    let raw: String = db.query_row(
        "SELECT value_json FROM user_env_setting WHERE setting_key=?1",
        [space::SIZE_UNIT],
        |row| row.get(0),
    )?;
    assert_eq!(raw, "\"MiB\"");
    drop(db);
    {
        let mut service = settings.service.lock().map_err(|_| "poisoned")?;
        let changes = SettingsChanges::from([(space::SIZE_UNIT.into(), Some(json!("MB")))]);
        let hash = service.review(1, &changes).map_err(|e| format!("{e:?}"))?;
        service
            .apply(1, &changes, hash, context())
            .map_err(|e| format!("{e:?}"))?;
    }
    assert_eq!(
        call(&engine, json!({}))?.body,
        json!({"settingsRevision":"2", "unitSystem":"si", "unit":"MB", "defaultBasis":"allocated", "warningKey":null})
    );
    let db = Connection::open(&path)?;
    db.execute_batch("UPDATE user_env_setting_version SET schema_version=3;")?;
    drop(db);
    let settings = Arc::new(StartupSettings::open(
        &test_database(&path)?,
        100,
        &["en", "ko"],
    )?);
    let engine = build_engine(settings)?;
    assert_eq!(
        call(&engine, json!({}))?.body,
        json!({"settingsRevision":"0", "unitSystem":"iec", "unit":"auto", "defaultBasis":"allocated", "warningKey":"settings.schema_newer"})
    );
    Ok(())
}

#[test]
fn gui_dispatch_reports_individual_restoration_warning() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("invalid.db");
    let mut db = AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?;
    db.commit(
        0,
        &SettingsChanges::from([(space::SIZE_UNIT.into(), Some(json!("bad")))]),
        context(),
    )
    .map_err(|e| format!("{e:?}"))?;
    drop(db);
    let settings = Arc::new(StartupSettings::open(
        &test_database(&path)?,
        100,
        &["en", "ko"],
    )?);
    let engine = build_engine(settings)?;
    let result = call(&engine, json!({}))?;
    assert_eq!(
        result.body,
        json!({"settingsRevision":"2", "unitSystem":"iec", "unit":"auto", "defaultBasis":"allocated", "warningKey":"settings.value_invalid"})
    );
    assert_eq!(call(&engine, json!({}))?.body, result.body);
    Ok(())
}
