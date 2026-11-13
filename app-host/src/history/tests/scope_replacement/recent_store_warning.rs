use super::*;
use std::{
    num::NonZeroUsize,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
};

#[test]
fn disabled_storage_with_writer_and_missing_source_are_silent()
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
    let cap = NonZeroUsize::new(2).ok_or("zero retention cap")?;
    let encode_calls = Arc::new(AtomicUsize::new(0));
    let counted_calls = Arc::clone(&encode_calls);
    let worker = crate::recent_scope_worker::RecentScopeWorker::start_for_test(
        admissions,
        database.clone(),
        cap,
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
    terminal_batch(&events, &scan.job_ref)?;

    let sql = Connection::open(
        profile
            .path()
            .join(du_platform_os::application_data::DATABASE_FILE),
    )?;
    sql.execute(
        "INSERT INTO user_env_setting
            (setting_key, value_json, modified_source, created_at_ms, updated_at_ms, row_version)
         VALUES ('scope.remember_recent', 'false', 'owner', 1, 1, 1)",
        [],
    )?;
    sql.execute_batch("BEGIN IMMEDIATE")?;
    observer(&request, &response).ok_or("missing disabled-save delivery callback")?(
        ResponseDelivery::Posted,
    );
    completed.recv_timeout(Duration::from_secs(5))?;
    assert!(matches!(
        warnings.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    let rows: u32 = sql.query_row("SELECT count(*) FROM user_env_recent_scope", [], |row| {
        row.get(0)
    })?;
    assert_eq!(rows, 0);
    assert_eq!(encode_calls.load(Ordering::Relaxed), 0);
    sql.execute_batch("ROLLBACK")?;

    sql.execute(
        "UPDATE user_env_setting SET value_json='true' WHERE setting_key='scope.remember_recent'",
        [],
    )?;
    assert_eq!(
        sql.query_row("SELECT count(*) FROM user_env_recent_scope", [], |row| row
            .get::<_, u32>(
            0
        ))?,
        0,
        "turning remember on does not replay the completed off attempt"
    );
    let missing = fixture.path().join("missing-source");
    fs::create_dir(&missing)?;
    fs::write(missing.join("item.bin"), [2; 32])?;
    let draft = scope_draft(&missing);
    let review: ScopeReview = serde_json::from_value(
        ipc(
            &engine,
            "scope.review",
            json!({"draft": draft, "requestSeq": 2}),
        )?
        .body,
    )?;
    let (request, response, _) = scan_response(&engine, &draft, &review)?;
    let posted = observer(&request, &response).ok_or("missing source-test callback")?;
    fs::rename(&missing, fixture.path().join("missing-source-moved"))?;
    posted(ResponseDelivery::Posted);
    completed.recv_timeout(Duration::from_secs(5))?;
    assert!(matches!(
        warnings.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert_eq!(encode_calls.load(Ordering::Relaxed), 1);
    let rows: u32 = sql.query_row("SELECT count(*) FROM user_env_recent_scope", [], |row| {
        row.get(0)
    })?;
    assert_eq!(rows, 0);
    Ok(())
}

mod final_gate;

#[test]
fn encode_failure_resolves_remember_setting_after_observation()
-> Result<(), Box<dyn std::error::Error>> {
    use du_platform_os::webview::ResponseDelivery;

    let fixture = tempfile::tempdir()?;
    let profile = tempfile::tempdir()?;
    let target = fixture.path().join("candidate-one");
    let target_after = fixture.path().join("candidate-two");
    fs::create_dir(&target)?;
    fs::write(target.join("item.bin"), [1; 32])?;
    fs::create_dir(&target_after)?;
    fs::write(target_after.join("item.bin"), [2; 32])?;

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
    let encoder = move |_: &du_engine_api::desktop::RecentScopeAdmission, _: &AtomicBool| {
        if calls.fetch_add(1, Ordering::Relaxed) == 0 {
            let _ = entered_sender.send(());
            let _ = release_receiver_for_encoder
                .lock()
                .map_err(|_| ())
                .and_then(|receiver| {
                    receiver
                        .recv_timeout(Duration::from_secs(5))
                        .map_err(|_| ())
                });
        }
        Err(crate::recent_scope_body::BodyError::Malformed)
    };
    let cap = NonZeroUsize::new(2).ok_or("zero retention cap")?;
    let worker = crate::recent_scope_worker::RecentScopeWorker::start_for_test(
        admissions,
        database.clone(),
        cap,
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
    sql.busy_timeout(Duration::from_millis(250))?;
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
    let (request, response, first_scan) = scan_response(&engine, &draft, &review)?;
    assert!(!first_scan.existing);
    observer(&request, &response).ok_or("missing delayed-encode callback")?(
        ResponseDelivery::Posted,
    );
    entered.recv_timeout(Duration::from_secs(5))?;

    // A settings commit must not wait for the blocked read-only preparation.
    let commit_started = Instant::now();
    sql.execute(
        "UPDATE user_env_setting SET value_json='false' WHERE setting_key='scope.remember_recent'",
        [],
    )?;
    assert!(commit_started.elapsed() < Duration::from_millis(250));
    release_sender.send(())?;
    completed.recv_timeout(Duration::from_secs(5))?;
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

    // The initial on gate admitted read-only preparation. Its failure was
    // resolved at the final gate after the setting changed to off, so that gate
    // linearizes the silent outcome. A later on value does not replay it; a new
    // eligible admission sees on and warns once.
    sql.execute(
        "UPDATE user_env_setting SET value_json='true' WHERE setting_key='scope.remember_recent'",
        [],
    )?;
    let draft_after = scope_draft(&target_after);
    let review_after: ScopeReview = serde_json::from_value(
        ipc(
            &engine,
            "scope.review",
            json!({"draft": draft_after, "requestSeq": 2}),
        )?
        .body,
    )?;
    let (request, response, second_scan) = scan_response(&engine, &draft_after, &review_after)?;
    assert!(!second_scan.existing);
    observer(&request, &response).ok_or("missing second invalid-encode callback")?(
        ResponseDelivery::Posted,
    );
    completed.recv_timeout(Duration::from_secs(5))?;
    let warning = warnings.recv_timeout(Duration::from_secs(5))?;
    assert_eq!(warning.job_ref, second_scan.job_ref);
    assert_eq!(warning.correlation_id, Id::from_v7_bytes([42; 16]));
    assert_eq!(warning.reason_key(), "scope.recent_store_failed");
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
    Ok(())
}

fn scope_draft(target: &std::path::Path) -> Value {
    let raw: Vec<_> = target
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect();
    json!({
        "includes": [{"encoding": "windows_utf16le", "raw": PathBytes::from_bytes(&raw),
            "display": target.to_string_lossy(), "lossy": false}],
        "exclusions": [], "boundary": "current_volume", "includedMounts": [], "source": "gui"
    })
}
