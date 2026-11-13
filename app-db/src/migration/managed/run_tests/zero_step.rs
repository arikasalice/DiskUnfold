use super::*;

#[test]
fn zero_step_run_checks_revision_and_scans_once_without_publishing_backup() -> TestResult {
    let sandbox = tempfile::tempdir()?;
    let path = sandbox.path().join("diskunfold.db");
    let root = sandbox.path().join("backups");
    drop(AppDb::create_gui_with_history(&path).map_err(|error| format!("{error:?}"))?);
    fs::create_dir(&root)?;
    let db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    publish(&db, HISTORY_SCHEMA_VERSION, &root).map_err(|error| format!("{error:?}"))?;
    drop(db);
    let backups_before = file_tree(&root)?;
    let inspector_calls = Cell::new(0);
    let outcome = apply_managed_run_with(
        &path,
        &root,
        &HISTORY_DEFINITIONS,
        HISTORY_SCHEMA_VERSION,
        |root| {
            inspector_calls.set(inspector_calls.get() + 1);
            inspect_retention(root)
        },
    )
    .map_err(|failure| format!("{failure:?}"))?;
    assert_eq!(outcome.revision, HISTORY_SCHEMA_VERSION);
    assert_eq!(outcome.applied_steps, 0);
    assert_eq!(outcome.retention.content_valid, 1);
    assert_eq!(inspector_calls.get(), 1);
    assert_eq!(file_tree(&root)?, backups_before);
    assert_eq!(
        AppDb::open_read_only(&path)
            .map_err(|error| format!("{error:?}"))?
            .schema_version()?,
        HISTORY_SCHEMA_VERSION
    );
    Ok(())
}

#[test]
fn zero_step_run_accepts_a_recognized_baseline_with_empty_definitions() -> TestResult {
    let sandbox = tempfile::tempdir()?;
    let path = sandbox.path().join("diskunfold.db");
    let root = sandbox.path().join("backups");
    seed(&path)?;
    fs::create_dir(&root)?;
    let empty_definitions = Definitions {
        baseline: HISTORY_DEFINITIONS.baseline,
        steps: &[],
    };
    let inspector_calls = Cell::new(0);
    let outcome = apply_managed_run_with(&path, &root, &empty_definitions, 1, |root| {
        inspector_calls.set(inspector_calls.get() + 1);
        inspect_retention(root)
    })
    .map_err(|failure| format!("{failure:?}"))?;
    assert_eq!((outcome.revision, outcome.applied_steps), (1, 0));
    assert_eq!(inspector_calls.get(), 1);
    assert_eq!(fs::read_dir(&root)?.count(), 0);
    assert_eq!(state(&path)?.0, 1);
    Ok(())
}

#[test]
fn zero_step_preflight_errors_precede_missing_database_errors() -> TestResult {
    let sandbox = tempfile::tempdir()?;
    let path = sandbox.path().join("missing.db");
    let root = sandbox.path().join("backups");
    fs::create_dir(&root)?;

    let invalid_definitions = Definitions {
        baseline: 0,
        steps: &[],
    };
    let invalid = take_failure(
        apply_managed_run_with(&path, &root, &invalid_definitions, 1, inspect_retention),
        "invalid definitions should fail before opening the database",
    )?;
    assert!(matches!(
        invalid.migration,
        MigrationError::InvalidDefinitions
    ));

    let unsupported_revision = take_failure(
        apply_managed_run_with(
            &path,
            &root,
            &HISTORY_DEFINITIONS,
            HISTORY_SCHEMA_VERSION + 1,
            inspect_retention,
        ),
        "an unsupported starting revision should fail before opening the database",
    )?;
    assert!(matches!(
        unsupported_revision.migration,
        MigrationError::VersionMismatch
    ));
    assert!(!path.exists());
    assert_eq!(fs::read_dir(&root)?.count(), 0);
    Ok(())
}

#[test]
fn zero_step_run_rejects_foreign_database_with_the_same_revision_unchanged() -> TestResult {
    let sandbox = tempfile::tempdir()?;
    let path = sandbox.path().join("foreign.db");
    let root = sandbox.path().join("backups");
    let db = Connection::open(&path)?;
    db.pragma_update(None, "user_version", HISTORY_SCHEMA_VERSION)?;
    drop(db);
    fs::create_dir(&root)?;
    fs::write(root.join("retained.txt"), b"keep this backup evidence")?;
    let database_before = fs::read(&path)?;
    let backups_before = file_tree(&root)?;
    let inspector_calls = Cell::new(0);

    let failure = take_failure(
        apply_managed_run_with(
            &path,
            &root,
            &HISTORY_DEFINITIONS,
            HISTORY_SCHEMA_VERSION,
            |root| {
                inspector_calls.set(inspector_calls.get() + 1);
                inspect_retention(root)
            },
        ),
        "same-version foreign databases must be rejected",
    )?;

    assert!(matches!(failure.migration, MigrationError::ForeignDatabase));
    assert_eq!(inspector_calls.get(), 1);
    assert_eq!(fs::read(&path)?, database_before);
    assert_eq!(file_tree(&root)?, backups_before);
    let db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let version: u32 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
    assert_eq!(version, HISTORY_SCHEMA_VERSION);
    Ok(())
}

#[test]
fn zero_step_run_rejects_tampered_current_history_without_changes() -> TestResult {
    let sandbox = tempfile::tempdir()?;
    let path = sandbox.path().join("diskunfold.db");
    let root = sandbox.path().join("backups");
    drop(AppDb::create_gui_with_history(&path).map_err(|error| format!("{error:?}"))?);
    let db = Connection::open(&path)?;
    db.execute_batch(
        "DROP TRIGGER app_db_schema_meta_no_update;
         UPDATE app_db_schema_meta SET definition_hash = zeroblob(32)
         WHERE migration_no = (SELECT MAX(migration_no) FROM app_db_schema_meta);",
    )?;
    drop(db);
    fs::create_dir(&root)?;
    fs::write(root.join("retained.txt"), b"keep this backup evidence")?;
    let database_before = fs::read(&path)?;
    let backups_before = file_tree(&root)?;
    let inspector_calls = Cell::new(0);

    let failure = take_failure(
        apply_managed_run_with(
            &path,
            &root,
            &HISTORY_DEFINITIONS,
            HISTORY_SCHEMA_VERSION,
            |root| {
                inspector_calls.set(inspector_calls.get() + 1);
                inspect_retention(root)
            },
        ),
        "a tampered current migration history must be rejected",
    )?;

    assert!(matches!(failure.migration, MigrationError::Tampered));
    assert_eq!(inspector_calls.get(), 1);
    assert_eq!(fs::read(&path)?, database_before);
    assert_eq!(file_tree(&root)?, backups_before);
    let db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let version: u32 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
    assert_eq!(version, HISTORY_SCHEMA_VERSION);
    Ok(())
}
