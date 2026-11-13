use super::*;

#[test]
fn busy_final_write_with_remember_on_warns_once_for_the_posted_admission()
-> Result<(), Box<dyn std::error::Error>> {
    use du_platform_os::webview::ResponseDelivery;

    let fixture = tempfile::tempdir()?;
    let profile = tempfile::tempdir()?;
    let target = fixture.path().join("candidate");
    fs::create_dir(&target)?;
    fs::write(target.join("item.bin"), [1; 32])?;

    let database = crate::startup_database::open(profile.path())?;
    let mut builder = EngineBuilder::default();
    let services = du_engine_api::desktop::register_services_with_recent_scope_worker(
        &mut builder,
        Arc::new(du_platform_os::windows_observation::WindowsScopeRoots),
        Arc::new(du_platform_os::volume::WindowsVolumes),
        Arc::new(database.history(|| 400)),
        &du_platform_os::entropy::OsRandom,
        Arc::new(du_core_contracts::settings::SharedTableLimit::default),
        NonZeroUsize::new(2).ok_or("zero pending capacity")?,
    )?;
    let events = services.events;
    let admissions = services
        .recent_scope_admissions
        .ok_or("missing recent-scope consumer")?;
    let (warning_sender, warnings) = mpsc::channel();
    let (completed_sender, completed) = mpsc::channel();
    let encoder_calls = Arc::new(AtomicUsize::new(0));
    let counted_calls = Arc::clone(&encoder_calls);
    let worker = crate::recent_scope_worker::RecentScopeWorker::start_for_test(
        admissions,
        database,
        NonZeroUsize::new(2).ok_or("zero retention cap")?,
        move |candidate, stopped| {
            counted_calls.fetch_add(1, Ordering::Relaxed);
            crate::recent_scope_body::RecentScopeBody::encode_admission(candidate, stopped)
        },
        move |warning| {
            let _ = warning_sender.send(warning);
        },
        move || {
            let _ = completed_sender.send(());
        },
    )?;
    let observer = worker.observer();
    let engine = builder.build()?;
    let draft = scope_draft(&target);
    let review: ScopeReview = serde_json::from_value(
        ipc(
            &engine,
            "scope.review",
            json!({"draft": draft, "requestSeq": 1}),
        )?
        .body,
    )?;
    let (request, response, scan) = scan_response(&engine, &draft, &review)?;
    assert!(!scan.existing);
    terminal_batch(&events, &scan.job_ref)?;

    let sql = Connection::open(
        profile
            .path()
            .join(du_platform_os::application_data::DATABASE_FILE),
    )?;
    sql.execute(
        "INSERT INTO user_env_setting
            (setting_key, value_json, modified_source, created_at_ms, updated_at_ms, row_version)
         VALUES ('scope.remember_recent', 'true', 'owner', 1, 1, 1)",
        [],
    )?;
    sql.execute_batch("BEGIN IMMEDIATE")?;
    observer(&request, &response).ok_or("missing busy-gate delivery callback")?(
        ResponseDelivery::Posted,
    );

    let warning = warnings.recv_timeout(Duration::from_secs(7))?;
    completed.recv_timeout(Duration::from_secs(2))?;
    assert_eq!(warning.job_ref, scan.job_ref);
    assert_eq!(warning.correlation_id, Id::from_v7_bytes([42; 16]));
    assert_eq!(warning.reason_key(), "db.busy");
    assert_eq!(encoder_calls.load(Ordering::Relaxed), 1);
    assert_eq!(
        sql.query_row("SELECT count(*) FROM user_env_recent_scope", [], |row| row
            .get::<_, u32>(
            0
        ))?,
        0
    );
    assert!(matches!(
        warnings.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    sql.execute_batch("ROLLBACK")?;
    Ok(())
}

#[test]
fn final_begin_busy_rechecks_committed_off_without_warning()
-> Result<(), Box<dyn std::error::Error>> {
    use du_platform_os::webview::ResponseDelivery;

    let fixture = tempfile::tempdir()?;
    let profile = tempfile::tempdir()?;
    let target = fixture.path().join("candidate");
    fs::create_dir(&target)?;
    fs::write(target.join("item.bin"), [1; 32])?;

    let database = crate::startup_database::open(profile.path())?;
    let mut builder = EngineBuilder::default();
    let services = du_engine_api::desktop::register_services_with_recent_scope_worker(
        &mut builder,
        Arc::new(du_platform_os::windows_observation::WindowsScopeRoots),
        Arc::new(du_platform_os::volume::WindowsVolumes),
        Arc::new(database.history(|| 400)),
        &du_platform_os::entropy::OsRandom,
        Arc::new(du_core_contracts::settings::SharedTableLimit::default),
        NonZeroUsize::new(2).ok_or("zero pending capacity")?,
    )?;
    let events = services.events;
    let admissions = services
        .recent_scope_admissions
        .ok_or("missing recent-scope consumer")?;
    let (warning_sender, warnings) = mpsc::channel();
    let (completed_sender, completed) = mpsc::channel();
    let (entered_sender, entered) = mpsc::channel();
    let (release_sender, release_receiver) = mpsc::channel();
    let release_receiver = Arc::new(std::sync::Mutex::new(release_receiver));
    let encoder_calls = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&encoder_calls);
    let release_receiver_for_encoder = Arc::clone(&release_receiver);
    let encoder = move |candidate: &du_engine_api::desktop::RecentScopeAdmission,
                        stopped: &AtomicBool| {
        calls.fetch_add(1, Ordering::Relaxed);
        let _ = entered_sender.send(());
        let _ = release_receiver_for_encoder
            .lock()
            .map_err(|_| ())
            .and_then(|receiver| {
                receiver
                    .recv_timeout(Duration::from_secs(5))
                    .map_err(|_| ())
            });
        crate::recent_scope_body::RecentScopeBody::encode_admission(candidate, stopped)
    };
    let worker = crate::recent_scope_worker::RecentScopeWorker::start_for_test(
        admissions,
        database,
        NonZeroUsize::new(2).ok_or("zero retention cap")?,
        encoder,
        move |warning| {
            let _ = warning_sender.send(warning);
        },
        move || {
            let _ = completed_sender.send(());
        },
    )?;
    let observer = worker.observer();
    let engine = builder.build()?;
    let sql = Connection::open(
        profile
            .path()
            .join(du_platform_os::application_data::DATABASE_FILE),
    )?;
    sql.execute(
        "INSERT INTO user_env_setting
            (setting_key, value_json, modified_source, created_at_ms, updated_at_ms, row_version)
         VALUES ('scope.remember_recent', 'true', 'owner', 1, 1, 1)",
        [],
    )?;

    let draft = scope_draft(&target);
    let review: ScopeReview = serde_json::from_value(
        ipc(
            &engine,
            "scope.review",
            json!({"draft": draft, "requestSeq": 1}),
        )?
        .body,
    )?;
    let (request, response, scan) = scan_response(&engine, &draft, &review)?;
    assert!(!scan.existing);
    terminal_batch(&events, &scan.job_ref)?;
    observer(&request, &response).ok_or("missing posted callback")?(ResponseDelivery::Posted);
    entered.recv_timeout(Duration::from_secs(5))?;

    sql.execute(
        "UPDATE user_env_setting SET value_json='false' WHERE setting_key='scope.remember_recent'",
        [],
    )?;
    let writer = Connection::open(
        profile
            .path()
            .join(du_platform_os::application_data::DATABASE_FILE),
    )?;
    writer.execute_batch("BEGIN IMMEDIATE")?;
    release_sender.send(())?;
    completed.recv_timeout(Duration::from_secs(7))?;

    assert_eq!(encoder_calls.load(Ordering::Relaxed), 1);
    assert!(matches!(
        warnings.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert_eq!(
        sql.query_row("SELECT count(*) FROM user_env_recent_scope", [], |row| row
            .get::<_, u32>(
            0
        ))?,
        0
    );
    writer.execute_batch("ROLLBACK")?;
    Ok(())
}

#[test]
fn busy_final_begin_with_setting_read_failure_warns_as_storage()
-> Result<(), Box<dyn std::error::Error>> {
    use du_platform_os::webview::ResponseDelivery;

    let fixture = tempfile::tempdir()?;
    let profile = tempfile::tempdir()?;
    let target = fixture.path().join("candidate");
    fs::create_dir(&target)?;
    fs::write(target.join("item.bin"), [1; 32])?;

    let database = crate::startup_database::open(profile.path())?;
    let mut builder = EngineBuilder::default();
    let services = du_engine_api::desktop::register_services_with_recent_scope_worker(
        &mut builder,
        Arc::new(du_platform_os::windows_observation::WindowsScopeRoots),
        Arc::new(du_platform_os::volume::WindowsVolumes),
        Arc::new(database.history(|| 400)),
        &du_platform_os::entropy::OsRandom,
        Arc::new(du_core_contracts::settings::SharedTableLimit::default),
        NonZeroUsize::new(2).ok_or("zero pending capacity")?,
    )?;
    let events = services.events;
    let admissions = services
        .recent_scope_admissions
        .ok_or("missing recent-scope consumer")?;
    let (warning_sender, warnings) = mpsc::channel();
    let (completed_sender, completed) = mpsc::channel();
    let (writer_started_sender, writer_started) = mpsc::channel();
    let ddl = Arc::new(std::sync::Mutex::new(Connection::open(
        profile
            .path()
            .join(du_platform_os::application_data::DATABASE_FILE),
    )?));
    let ddl_for_encoder = Arc::clone(&ddl);
    let encoder = move |candidate: &du_engine_api::desktop::RecentScopeAdmission,
                        stopped: &AtomicBool|
          -> Result<
        crate::recent_scope_body::EncodedRecentScope,
        crate::recent_scope_body::BodyError,
    > {
        let encoded =
            crate::recent_scope_body::RecentScopeBody::encode_admission(candidate, stopped)?;
        let started = ddl_for_encoder.lock().ok().is_some_and(|connection| {
            connection
                .execute_batch(
                    "ALTER TABLE user_env_setting_version
                         RENAME COLUMN schema_version TO broken_schema_version",
                )
                .is_ok()
                && connection.execute_batch("BEGIN IMMEDIATE").is_ok()
        });
        let _ = writer_started_sender.send(started);
        if !started {
            return Err(crate::recent_scope_body::BodyError::Malformed);
        }
        Ok(encoded)
    };
    let worker = crate::recent_scope_worker::RecentScopeWorker::start_for_test(
        admissions,
        database,
        NonZeroUsize::new(2).ok_or("zero retention cap")?,
        encoder,
        move |warning| {
            let _ = warning_sender.send(warning);
        },
        move || {
            let _ = completed_sender.send(());
        },
    )?;
    let observer = worker.observer();
    let engine = builder.build()?;
    let sql = Connection::open(
        profile
            .path()
            .join(du_platform_os::application_data::DATABASE_FILE),
    )?;
    sql.execute(
        "INSERT INTO user_env_setting
            (setting_key, value_json, modified_source, created_at_ms, updated_at_ms, row_version)
         VALUES ('scope.remember_recent', 'true', 'owner', 1, 1, 1)",
        [],
    )?;
    sql.execute(
        "INSERT INTO user_env_recent_scope
            (recent_scope_id,scope_fingerprint,fingerprint_format,scope_body,include_count,
             last_used_at_ms,created_at_ms,updated_at_ms,row_version)
         VALUES (X'00000000000070008000000000000001',zeroblob(32),1,'{}',1,1,1,1,1)",
        [],
    )?;

    let draft = scope_draft(&target);
    let review: ScopeReview = serde_json::from_value(
        ipc(
            &engine,
            "scope.review",
            json!({"draft": draft, "requestSeq": 1}),
        )?
        .body,
    )?;
    let (request, response, scan) = scan_response(&engine, &draft, &review)?;
    assert!(!scan.existing);
    terminal_batch(&events, &scan.job_ref)?;
    observer(&request, &response).ok_or("missing posted callback")?(ResponseDelivery::Posted);
    assert!(writer_started.recv_timeout(Duration::from_secs(5))?);
    completed.recv_timeout(Duration::from_secs(7))?;

    let warning = warnings.recv_timeout(Duration::from_secs(5))?;
    assert_eq!(
        warning.warning,
        crate::recent_scope_worker::RecentScopeWriteWarning::StorageUnavailable
    );
    assert_eq!(warning.job_ref, scan.job_ref);
    assert_eq!(warning.correlation_id, Id::from_v7_bytes([42; 16]));
    assert_eq!(warning.reason_key(), "scope.recent_store_failed");
    assert_eq!(
        sql.query_row(
            "SELECT count(*) FROM user_env_recent_scope
             WHERE recent_scope_id=X'00000000000070008000000000000001'",
            [],
            |row| row.get::<_, u32>(0),
        )?,
        1
    );
    assert_eq!(
        sql.query_row("SELECT count(*) FROM user_env_recent_scope", [], |row| row
            .get::<_, u32>(
            0
        ))?,
        1,
        "setting-read failure after BEGIN_BUSY must preserve existing rows"
    );
    assert!(matches!(
        warnings.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    ddl.lock()
        .map_err(|_| "ddl connection poisoned")?
        .execute_batch("ROLLBACK")?;
    Ok(())
}

#[test]
fn final_setting_select_failure_keeps_rows_and_warns_once() -> Result<(), Box<dyn std::error::Error>>
{
    use du_platform_os::webview::ResponseDelivery;

    let fixture = tempfile::tempdir()?;
    let profile = tempfile::tempdir()?;
    let target = fixture.path().join("candidate");
    fs::create_dir(&target)?;
    fs::write(target.join("item.bin"), [1; 32])?;

    let database = crate::startup_database::open(profile.path())?;
    let mut builder = EngineBuilder::default();
    let services = du_engine_api::desktop::register_services_with_recent_scope_worker(
        &mut builder,
        Arc::new(du_platform_os::windows_observation::WindowsScopeRoots),
        Arc::new(du_platform_os::volume::WindowsVolumes),
        Arc::new(database.history(|| 400)),
        &du_platform_os::entropy::OsRandom,
        Arc::new(du_core_contracts::settings::SharedTableLimit::default),
        NonZeroUsize::new(2).ok_or("zero pending capacity")?,
    )?;
    let events = services.events;
    let admissions = services
        .recent_scope_admissions
        .ok_or("missing recent-scope consumer")?;
    let (warning_sender, warnings) = mpsc::channel();
    let (completed_sender, completed) = mpsc::channel();
    let (altered_sender, altered) = mpsc::channel();
    let ddl = Arc::new(std::sync::Mutex::new(Connection::open(
        profile
            .path()
            .join(du_platform_os::application_data::DATABASE_FILE),
    )?));
    let ddl_for_encoder = Arc::clone(&ddl);
    let encoder = move |candidate: &du_engine_api::desktop::RecentScopeAdmission,
                        stopped: &AtomicBool|
          -> Result<
        crate::recent_scope_body::EncodedRecentScope,
        crate::recent_scope_body::BodyError,
    > {
        let encoded =
            crate::recent_scope_body::RecentScopeBody::encode_admission(candidate, stopped)?;
        let altered = ddl_for_encoder
            .lock()
            .map_err(|_| crate::recent_scope_body::BodyError::Malformed)?
            .execute_batch(
                "ALTER TABLE user_env_setting_version
                 RENAME COLUMN schema_version TO broken_schema_version",
            )
            .is_ok();
        let _ = altered_sender.send(altered);
        if !altered {
            return Err(crate::recent_scope_body::BodyError::Malformed);
        }
        Ok(encoded)
    };
    let worker = crate::recent_scope_worker::RecentScopeWorker::start_for_test(
        admissions,
        database,
        NonZeroUsize::new(2).ok_or("zero retention cap")?,
        encoder,
        move |warning| {
            let _ = warning_sender.send(warning);
        },
        move || {
            let _ = completed_sender.send(());
        },
    )?;
    let observer = worker.observer();
    let engine = builder.build()?;

    let sql = Connection::open(
        profile
            .path()
            .join(du_platform_os::application_data::DATABASE_FILE),
    )?;
    sql.execute(
        "INSERT INTO user_env_setting
            (setting_key, value_json, modified_source, created_at_ms, updated_at_ms, row_version)
         VALUES ('scope.remember_recent', 'true', 'owner', 1, 1, 1)",
        [],
    )?;
    sql.execute(
        "INSERT INTO user_env_recent_scope
            (recent_scope_id,scope_fingerprint,fingerprint_format,scope_body,include_count,
             last_used_at_ms,created_at_ms,updated_at_ms,row_version)
         VALUES (X'00000000000070008000000000000001',zeroblob(32),1,'{}',1,1,1,1,1)",
        [],
    )?;

    let draft = scope_draft(&target);
    let review: ScopeReview = serde_json::from_value(
        ipc(
            &engine,
            "scope.review",
            json!({"draft": draft, "requestSeq": 1}),
        )?
        .body,
    )?;
    let (request, response, scan) = scan_response(&engine, &draft, &review)?;
    assert!(!scan.existing);
    terminal_batch(&events, &scan.job_ref)?;
    observer(&request, &response).ok_or("missing final-gate callback")?(ResponseDelivery::Posted);
    assert!(altered.recv_timeout(Duration::from_secs(5))?);
    completed.recv_timeout(Duration::from_secs(5))?;

    let warning = warnings.recv_timeout(Duration::from_secs(5))?;
    assert_eq!(warning.job_ref, scan.job_ref);
    assert_eq!(warning.correlation_id, Id::from_v7_bytes([42; 16]));
    assert_eq!(warning.reason_key(), "scope.recent_store_failed");
    assert!(!format!("{warning:?}").contains(target.to_string_lossy().as_ref()));
    assert_eq!(
        sql.query_row(
            "SELECT count(*) FROM user_env_recent_scope
             WHERE recent_scope_id=X'00000000000070008000000000000001'",
            [],
            |row| row.get::<_, u32>(0),
        )?,
        1
    );
    assert_eq!(
        sql.query_row("SELECT count(*) FROM user_env_recent_scope", [], |row| row
            .get::<_, u32>(
            0
        ))?,
        1,
        "the failed checked final gate must not insert or evict rows"
    );
    assert!(matches!(
        warnings.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    Ok(())
}
