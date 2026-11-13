//! F15 restore decisions, failure records and journal file handling.
use super::*;

#[test]
fn interrupted_run_records_the_step_that_never_committed() -> TestResult {
    let f = fixture()?;
    let plan = JournalPlan {
        source_version: 1,
        target_version: HISTORY_SCHEMA_VERSION,
    };
    apply_managed_step(&f.path, &f.root, &HISTORY_DEFINITIONS, Some(1), Some(&plan))
        .map_err(|error| format!("{error:?}"))?;
    recover_interrupted(&f.path, &f.root, &HISTORY_DEFINITIONS)
        .map_err(|error| format!("{error:?}"))?;
    let record: Value = serde_json::from_slice(&fs::read(restored_record(&f.root, 1))?)?;
    assert_eq!(
        record["failure"],
        json!({"step": 3, "category": "interrupted"})
    );
    Ok(())
}

fn migrate_to_target_leaving_the_journal(f: &Fixture) -> TestResult {
    let plan = JournalPlan {
        source_version: 1,
        target_version: HISTORY_SCHEMA_VERSION,
    };
    let mut revision = 1;
    for _ in HISTORY_DEFINITIONS.steps {
        let journal = (revision == 1).then_some(&plan);
        revision = apply_managed_step(
            &f.path,
            &f.root,
            &HISTORY_DEFINITIONS,
            Some(revision),
            journal,
        )
        .map_err(|error| format!("{error:?}"))?
        .revision;
    }
    Ok(())
}

#[test]
fn transient_error_with_open_journal_and_database_at_target_never_restores() -> TestResult {
    let f = fixture()?;
    migrate_to_target_leaving_the_journal(&f)?;
    let migrated = digest(&f.path)?;
    let journal = fs::read(journal_file(&f.root))?;
    for transient in [
        || MigrationError::BackupBusy,
        || MigrationError::Io(std::io::ErrorKind::TimedOut.into()),
        || {
            MigrationError::Sqlite(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
                None,
            ))
        },
    ] {
        let error = restore::recover_with(&f.path, &f.root, &HISTORY_DEFINITIONS, &mut |stage| {
            if stage == RestoreStage::Deciding {
                Err(transient())
            } else {
                Ok(())
            }
        })
        .err()
        .ok_or("a transient error was treated as a decision")?;
        assert!(
            matches!(error, MigrationError::RestoreFailed(_)),
            "{error:?}"
        );
        // The newer database and the journal are untouched and nothing was restored.
        assert_eq!(version(&f.path)?, HISTORY_SCHEMA_VERSION);
        assert_eq!(digest(&f.path)?, migrated);
        assert_eq!(fs::read(journal_file(&f.root))?, journal);
        assert!(!restored_record(&f.root, 1).exists());
    }
    // Without the error the same state is recognized as complete.
    assert_eq!(
        recover_interrupted(&f.path, &f.root, &HISTORY_DEFINITIONS)
            .map_err(|error| format!("{error:?}"))?,
        RecoveryOutcome::AlreadyComplete {
            version: HISTORY_SCHEMA_VERSION
        }
    );
    assert_eq!(digest(&f.path)?, migrated);
    Ok(())
}

#[test]
fn database_past_the_target_with_an_open_journal_is_never_restored() -> TestResult {
    let f = fixture()?;
    migrate_to_target_leaving_the_journal(&f)?;
    Connection::open(&f.path)?.pragma_update(None, "user_version", HISTORY_SCHEMA_VERSION + 1)?;
    let before = digest(&f.path)?;
    let error = recover_interrupted(&f.path, &f.root, &HISTORY_DEFINITIONS)
        .err()
        .ok_or("a newer database was handled")?;
    assert!(
        matches!(error, MigrationError::RestoreFailed(_)),
        "{error:?}"
    );
    assert_eq!(digest(&f.path)?, before);
    assert!(journal_file(&f.root).is_file());
    Ok(())
}

#[test]
fn restore_while_another_connection_holds_a_lock_fails_closed_with_backup_busy() -> TestResult {
    let f = fixture()?;
    let plan = JournalPlan {
        source_version: 1,
        target_version: HISTORY_SCHEMA_VERSION,
    };
    apply_managed_step(&f.path, &f.root, &HISTORY_DEFINITIONS, Some(1), Some(&plan))
        .map_err(|error| format!("{error:?}"))?;
    let migrated = digest(&f.path)?;

    let holder = Connection::open(&f.path)?;
    holder.execute_batch("BEGIN IMMEDIATE")?;
    let error = recover_interrupted(&f.path, &f.root, &HISTORY_DEFINITIONS)
        .err()
        .ok_or("restore succeeded while the database was locked")?;
    assert!(
        matches!(&error, MigrationError::RestoreFailed(inner) if matches!(**inner, MigrationError::BackupBusy)),
        "{error:?}"
    );
    // Nothing changed: the migrated database, the backup and the journal remain.
    assert_eq!(digest(&f.path)?, migrated);
    assert!(journal_file(&f.root).is_file());
    assert!(f.root.join("backup-00000000000000000001").is_dir());
    assert!(!restored_record(&f.root, 1).exists());

    holder.execute_batch("ROLLBACK")?;
    drop(holder);
    assert_eq!(
        recover_interrupted(&f.path, &f.root, &HISTORY_DEFINITIONS)
            .map_err(|error| format!("{error:?}"))?,
        RecoveryOutcome::Restored(RestoredBackup {
            sequence: 1,
            version: 1
        })
    );
    assert_eq!(digest(&f.path)?, f.original);
    Ok(())
}

#[test]
fn stale_journal_temp_file_is_replaced_and_the_directory_is_synced() -> TestResult {
    let f = fixture()?;
    fs::write(f.root.join("migration-journal.json.tmp"), b"stale")?;
    let plan = JournalPlan {
        source_version: 1,
        target_version: HISTORY_SCHEMA_VERSION,
    };
    apply_managed_step(&f.path, &f.root, &HISTORY_DEFINITIONS, Some(1), Some(&plan))
        .map_err(|error| format!("{error:?}"))?;
    assert!(!f.root.join("migration-journal.json.tmp").exists());
    assert!(
        restore::read_journal(&f.root)
            .map_err(|e| format!("{e:?}"))?
            .is_some()
    );
    // Both directory syncs are real operations on every platform.
    sync_dir(&f.root).map_err(|error| format!("{error:?}"))?;
    restore::clear_journal(&f.root);
    assert!(!journal_file(&f.root).exists());
    Ok(())
}

#[test]
fn journal_temp_directory_blocks_the_run_before_any_database_write() -> TestResult {
    let f = fixture()?;
    fs::create_dir(f.root.join("migration-journal.json.tmp"))?;
    let failure = failed(apply_managed_run(&f.path, &f.root, &HISTORY_DEFINITIONS, 1))?;
    assert!(matches!(failure.migration, MigrationError::Backup(_)));
    assert_eq!(version(&f.path)?, 1);
    assert_eq!(digest(&f.path)?, f.original);
    Ok(())
}

#[cfg(unix)]
#[test]
fn stale_journal_temp_symlink_is_removed_and_never_followed() -> TestResult {
    let f = fixture()?;
    let victim = f.root.join("victim");
    fs::write(&victim, b"keep")?;
    std::os::unix::fs::symlink(&victim, f.root.join("migration-journal.json.tmp"))?;
    let plan = JournalPlan {
        source_version: 1,
        target_version: HISTORY_SCHEMA_VERSION,
    };
    apply_managed_step(&f.path, &f.root, &HISTORY_DEFINITIONS, Some(1), Some(&plan))
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(fs::read(&victim)?, b"keep");
    Ok(())
}
