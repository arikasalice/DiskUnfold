//! F15 FR8.HD1.app-db.DD1: a failed or interrupted migration restores the pre-migration backup.
use super::run::apply_managed_run_faulty;
use super::*;
use crate::{
    AppDb, HISTORY_DEFINITIONS, HISTORY_SCHEMA_VERSION,
    migration::{Definition, MigrationError},
};
use std::{cell::RefCell, path::PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Fixture {
    _dir: tempfile::TempDir,
    path: PathBuf,
    root: PathBuf,
    original: String,
}

fn fixture() -> Result<Fixture, Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("diskunfold.db");
    let root = dir.path().join("db-backups");
    fs::create_dir(&root)?;
    let db = AppDb::create_gui(&path).map_err(|error| format!("{error:?}"))?;
    db.connection.execute_batch(
        "INSERT INTO user_env_setting VALUES ('locale.display_language', '\"ko\"', 'screen', 1, 2, 1);
         INSERT INTO user_env_setting VALUES ('space.shared_table_max_entries', '200000', 'screen', 1, 2, 1);
         UPDATE user_env_setting_version SET settings_version=7;",
    )?;
    drop(db);
    let original = digest(&path)?;
    Ok(Fixture {
        _dir: dir,
        path,
        root,
        original,
    })
}

fn digest(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    Ok(restore::logical_digest(&db).map_err(|error| format!("{error:?}"))?)
}

fn version(path: &Path) -> Result<u32, Box<dyn std::error::Error>> {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    Ok(db.pragma_query_value(None, "user_version", |row| row.get(0))?)
}

/// Runs `body` with the built-in definitions where one step has a failing statement.
fn with_failing_step<R>(index: usize, body: impl FnOnce(&Definitions<'_>) -> R) -> R {
    let sql: Vec<String> = HISTORY_DEFINITIONS
        .steps
        .iter()
        .enumerate()
        .map(|(position, definition)| {
            if position == index {
                format!("{}\nINSERT INTO missing_table VALUES (1);", definition.sql)
            } else {
                definition.sql.to_owned()
            }
        })
        .collect();
    let steps: Vec<Definition<'_>> = sql
        .iter()
        .zip(HISTORY_DEFINITIONS.steps)
        .map(|(sql, definition)| Definition {
            number: definition.number,
            sql,
        })
        .collect();
    body(&Definitions {
        baseline: HISTORY_DEFINITIONS.baseline,
        steps: &steps,
    })
}

fn journal_file(root: &Path) -> PathBuf {
    root.join(JOURNAL)
}

fn restored_record(root: &Path, sequence: u64) -> PathBuf {
    root.join(format!("restored-{sequence:020}.json"))
}

fn wal_is_empty(path: &Path) -> bool {
    let mut wal = path.as_os_str().to_owned();
    wal.push("-wal");
    fs::metadata(PathBuf::from(wal)).map_or(true, |meta| meta.len() == 0)
}

fn corrupt_backup(root: &Path, sequence: u64) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let copy = root.join(format!("backup-{sequence:020}")).join(COPY);
    let original = fs::read(&copy)?;
    let mut bytes = original.clone();
    bytes[100] ^= 0xff;
    fs::write(copy, bytes)?;
    Ok(original)
}

fn failed(
    result: Result<ManagedRunOutcome, Box<ManagedRunFailure>>,
) -> Result<ManagedRunFailure, Box<dyn std::error::Error>> {
    match result {
        Ok(outcome) => Err(format!("migration unexpectedly succeeded: {outcome:?}").into()),
        Err(failure) => Ok(*failure),
    }
}

#[test]
fn upgrade_from_older_data_keeps_rows_and_leaves_no_journal() -> TestResult {
    let f = fixture()?;
    let outcome = apply_managed_run(&f.path, &f.root, &HISTORY_DEFINITIONS, 1)
        .map_err(|failure| format!("{failure:?}"))?;
    assert_eq!((outcome.revision, outcome.applied_steps), (6, 5));
    assert_eq!(version(&f.path)?, HISTORY_SCHEMA_VERSION);
    assert!(!journal_file(&f.root).exists());
    let db = Connection::open_with_flags(&f.path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let rows: Vec<(String, String)> = db
        .prepare("SELECT setting_key, value_json FROM user_env_setting ORDER BY 1")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    assert_eq!(
        rows,
        [
            ("locale.display_language".into(), "\"ko\"".into()),
            ("space.shared_table_max_entries".into(), "200000".into())
        ]
    );
    // The backup of the starting revision is the pre-migration backup and is kept.
    let first = root_digest(&f.root.join("backup-00000000000000000001").join(COPY))?;
    assert_eq!(first, f.original);
    Ok(())
}

fn root_digest(copy: &Path) -> Result<String, Box<dyn std::error::Error>> {
    digest(copy)
}

#[test]
fn failure_at_each_step_restores_every_old_row() -> TestResult {
    for index in 0..HISTORY_DEFINITIONS.steps.len() {
        let f = fixture()?;
        let failure = with_failing_step(index, |definitions| {
            failed(apply_managed_run(&f.path, &f.root, definitions, 1))
        })?;
        assert!(
            matches!(failure.migration, MigrationError::Sqlite(_)),
            "step {index}: {:?}",
            failure.migration
        );
        assert!(
            matches!(
                failure.restore,
                RestoreStatus::Restored(RestoredBackup {
                    sequence: 1,
                    version: 1
                })
            ),
            "step {index}: {:?}",
            failure.restore
        );
        assert_eq!(version(&f.path)?, 1, "step {index}");
        assert_eq!(digest(&f.path)?, f.original, "step {index}");
        assert!(wal_is_empty(&f.path));
        let reader = AppDb::open_read_only(&f.path).map_err(|error| format!("{error:?}"))?;
        assert_eq!(reader.schema_version()?, 1);
        drop(reader);
        assert!(!journal_file(&f.root).exists());
        assert!(restored_record(&f.root, 1).is_file());
        let record: Value = serde_json::from_slice(&fs::read(restored_record(&f.root, 1))?)?;
        let expected_step = HISTORY_DEFINITIONS.steps[index].number;
        assert_eq!(
            record["failure"],
            json!({"step": expected_step, "category": "sqlite"}),
            "step {index}"
        );
        assert!(valid_entry(&f.root.join("backup-00000000000000000001"), 1));
        assert!(!failure.retention.inspection_failed);
    }
    Ok(())
}

#[test]
fn backup_failure_before_any_write_needs_no_restore() -> TestResult {
    let f = fixture()?;
    // A missing backup directory makes the first publication fail before any write.
    let missing = f.root.join("absent");
    let failure = failed(apply_managed_run(
        &f.path,
        &missing,
        &HISTORY_DEFINITIONS,
        1,
    ))?;
    assert!(matches!(failure.migration, MigrationError::Backup(_)));
    assert!(matches!(failure.restore, RestoreStatus::NotNeeded));
    assert_eq!(digest(&f.path)?, f.original);
    assert!(!journal_file(&f.root).exists());
    Ok(())
}

#[test]
fn restore_failure_keeps_database_backups_and_journal_until_repaired() -> TestResult {
    let f = fixture()?;
    let repaired = RefCell::new(None);
    let failure = with_failing_step(2, |definitions| {
        failed(apply_managed_run_faulty(
            &f.path,
            &f.root,
            definitions,
            1,
            inspect_retention,
            &mut |stage| {
                if stage == RestoreStage::Begun {
                    *repaired.borrow_mut() =
                        Some(corrupt_backup(&f.root, 1).map_err(|_| MigrationError::Integrity)?);
                }
                Ok(())
            },
        ))
    })?;
    assert!(matches!(failure.migration, MigrationError::Sqlite(_)));
    assert!(matches!(
        failure.restore,
        RestoreStatus::Failed(MigrationError::Integrity)
    ));
    // Nothing was deleted: the partially migrated database, all backups and the journal remain.
    assert_eq!(version(&f.path)?, 3);
    assert_ne!(digest(&f.path)?, f.original);
    assert!(journal_file(&f.root).is_file());
    for sequence in 1..=3 {
        assert!(f.root.join(format!("backup-{sequence:020}")).is_dir());
    }
    let error = recover_interrupted(&f.path, &f.root, &HISTORY_DEFINITIONS)
        .err()
        .ok_or("recovery used a corrupt backup")?;
    assert!(matches!(error, MigrationError::RestoreFailed(_)));
    assert!(journal_file(&f.root).is_file());
    assert_eq!(version(&f.path)?, 3);

    let good = repaired.into_inner().ok_or("backup was not corrupted")?;
    fs::write(f.root.join("backup-00000000000000000001").join(COPY), good)?;
    let outcome = recover_interrupted(&f.path, &f.root, &HISTORY_DEFINITIONS)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        outcome,
        RecoveryOutcome::Restored(RestoredBackup {
            sequence: 1,
            version: 1
        })
    );
    assert_eq!(digest(&f.path)?, f.original);
    assert!(!journal_file(&f.root).exists());
    Ok(())
}

#[test]
fn stop_at_each_restore_stage_is_finished_by_the_next_start() -> TestResult {
    // Begun: before any restored page; Copied: restored but unverified; Verified: restored, journal open.
    for stage in [
        RestoreStage::Begun,
        RestoreStage::Copied,
        RestoreStage::Verified,
    ] {
        let f = fixture()?;
        let failure = with_failing_step(3, |definitions| {
            failed(apply_managed_run_faulty(
                &f.path,
                &f.root,
                definitions,
                1,
                inspect_retention,
                &mut |reached| {
                    if reached == stage {
                        Err(MigrationError::Integrity)
                    } else {
                        Ok(())
                    }
                },
            ))
        })?;
        assert!(
            matches!(failure.restore, RestoreStatus::Failed(_)),
            "{stage:?}"
        );
        assert!(journal_file(&f.root).is_file(), "{stage:?}");
        // A new run must not start over a journal that still names the backup.
        let blocked = failed(apply_managed_run(&f.path, &f.root, &HISTORY_DEFINITIONS, 1))?;
        assert!(matches!(blocked.migration, MigrationError::Interrupted));
        assert!(matches!(blocked.restore, RestoreStatus::NotNeeded));

        let outcome = recover_interrupted(&f.path, &f.root, &HISTORY_DEFINITIONS)
            .map_err(|error| format!("{stage:?}: {error:?}"))?;
        assert_eq!(
            outcome,
            RecoveryOutcome::Restored(RestoredBackup {
                sequence: 1,
                version: 1
            }),
            "{stage:?}"
        );
        assert_eq!(digest(&f.path)?, f.original, "{stage:?}");
        assert_eq!(version(&f.path)?, 1);
        assert!(!journal_file(&f.root).exists());
        assert_eq!(
            recover_interrupted(&f.path, &f.root, &HISTORY_DEFINITIONS)
                .map_err(|error| format!("{error:?}"))?,
            RecoveryOutcome::NothingPending
        );
    }
    Ok(())
}

#[test]
fn process_stop_after_a_committed_step_restores_on_restart() -> TestResult {
    let f = fixture()?;
    let plan = JournalPlan {
        source_version: 1,
        target_version: HISTORY_SCHEMA_VERSION,
    };
    let step = apply_managed_step(&f.path, &f.root, &HISTORY_DEFINITIONS, Some(1), Some(&plan))
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(step.revision, 2);
    assert!(journal_file(&f.root).is_file());
    assert_eq!(version(&f.path)?, 2);

    let outcome = recover_interrupted(&f.path, &f.root, &HISTORY_DEFINITIONS)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        outcome,
        RecoveryOutcome::Restored(RestoredBackup {
            sequence: 1,
            version: 1
        })
    );
    assert_eq!(digest(&f.path)?, f.original);
    assert_eq!(version(&f.path)?, 1);
    assert!(restored_record(&f.root, 1).is_file());
    Ok(())
}

#[test]
fn process_stop_after_the_last_commit_is_recognized_as_complete() -> TestResult {
    let f = fixture()?;
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
    assert!(journal_file(&f.root).is_file());
    let outcome = recover_interrupted(&f.path, &f.root, &HISTORY_DEFINITIONS)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        outcome,
        RecoveryOutcome::AlreadyComplete {
            version: HISTORY_SCHEMA_VERSION
        }
    );
    assert_eq!(version(&f.path)?, HISTORY_SCHEMA_VERSION);
    assert!(!journal_file(&f.root).exists());
    Ok(())
}

#[test]
fn malformed_journal_blocks_recovery_and_is_preserved() -> TestResult {
    let f = fixture()?;
    fs::write(journal_file(&f.root), b"{\"format\":\"unknown\"}")?;
    let error = recover_interrupted(&f.path, &f.root, &HISTORY_DEFINITIONS)
        .err()
        .ok_or("malformed journal was accepted")?;
    assert!(matches!(error, MigrationError::RestoreFailed(_)));
    assert_eq!(
        fs::read(journal_file(&f.root))?,
        b"{\"format\":\"unknown\"}"
    );
    let failure = failed(apply_managed_run(&f.path, &f.root, &HISTORY_DEFINITIONS, 1))?;
    assert!(matches!(failure.migration, MigrationError::Integrity));
    assert_eq!(digest(&f.path)?, f.original);
    Ok(())
}

#[test]
fn read_only_consumer_neither_migrates_nor_recovers() -> TestResult {
    let f = fixture()?;
    let plan = JournalPlan {
        source_version: 1,
        target_version: HISTORY_SCHEMA_VERSION,
    };
    apply_managed_step(&f.path, &f.root, &HISTORY_DEFINITIONS, Some(1), Some(&plan))
        .map_err(|error| format!("{error:?}"))?;
    let journal = fs::read(journal_file(&f.root))?;
    let migrated = digest(&f.path)?;

    let reader = AppDb::open_read_only(&f.path).map_err(|error| format!("{error:?}"))?;
    assert_eq!(reader.schema_version()?, 2);
    drop(reader);

    assert_eq!(digest(&f.path)?, migrated);
    assert_eq!(version(&f.path)?, 2);
    assert_eq!(fs::read(journal_file(&f.root))?, journal);
    assert!(!restored_record(&f.root, 1).exists());
    Ok(())
}

#[test]
fn recovery_without_a_backup_directory_or_journal_does_nothing() -> TestResult {
    let f = fixture()?;
    let missing = f.root.join("absent");
    assert_eq!(
        recover_interrupted(&f.path, &missing, &HISTORY_DEFINITIONS)
            .map_err(|error| format!("{error:?}"))?,
        RecoveryOutcome::NothingPending
    );
    assert_eq!(
        recover_interrupted(&f.path, &f.root, &HISTORY_DEFINITIONS)
            .map_err(|error| format!("{error:?}"))?,
        RecoveryOutcome::NothingPending
    );
    assert_eq!(digest(&f.path)?, f.original);
    Ok(())
}

#[cfg(test)]
mod decisions;
