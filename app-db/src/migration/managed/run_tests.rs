use super::run::apply_managed_run_with;
use super::*;
use crate::{AppDb, HISTORY_DEFINITIONS, HISTORY_SCHEMA_VERSION};
use std::{
    cell::Cell,
    io::{Seek, SeekFrom},
    path::PathBuf,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;
type FileTree = Vec<(PathBuf, Option<Vec<u8>>)>;

fn take_failure(
    result: Result<ManagedRunOutcome, Box<ManagedRunFailure>>,
    message: &'static str,
) -> Result<ManagedRunFailure, Box<dyn std::error::Error>> {
    match result {
        Ok(_) => Err(message.into()),
        Err(failure) => Ok(*failure),
    }
}

fn seed(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let db = AppDb::create_gui(path).map_err(|error| format!("{error:?}"))?;
    db.connection.execute_batch(
        "INSERT INTO user_env_setting VALUES ('locale.display_language', '\"ko\"', 'screen', 1, 2, 1); UPDATE user_env_setting_version SET settings_version=7;",
    )?;
    Ok(())
}

fn state(path: &Path) -> Result<(u32, i64, String), Box<dyn std::error::Error>> {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let version = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let settings = db.query_row(
        "SELECT settings_version FROM user_env_setting_version",
        [],
        |row| row.get(0),
    )?;
    let value = db.query_row("SELECT value_json FROM user_env_setting", [], |row| {
        row.get(0)
    })?;
    Ok((version, settings, value))
}

fn file_tree(root: &Path) -> Result<FileTree, Box<dyn std::error::Error>> {
    fn visit(
        root: &Path,
        directory: &Path,
        entries: &mut Vec<(PathBuf, Option<Vec<u8>>)>,
    ) -> TestResult {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            let relative = path.strip_prefix(root)?.to_path_buf();
            if entry.file_type()?.is_dir() {
                entries.push((relative, None));
                visit(root, &path, entries)?;
            } else {
                entries.push((relative, Some(fs::read(path)?)));
            }
        }
        Ok(())
    }

    let mut entries = Vec::new();
    visit(root, root, &mut entries)?;
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(entries)
}

fn seed_with_two_backups(path: &Path, root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir(root)?;
    seed(path)?;
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    publish(&db, 1, root).map_err(|error| format!("{error:?}"))?;
    publish(&db, 1, root).map_err(|error| format!("{error:?}"))?;
    Ok(())
}

fn counted_retention(
    root: &Path,
    validations: &Cell<usize>,
    quick_checks: &Cell<usize>,
    copy_bytes: &Cell<u64>,
) -> Result<RetentionDiagnostic, MigrationError> {
    inspect_retention_with(root, |path, sequence| {
        validations.set(validations.get() + 1);
        let bytes = fs::metadata(path.join(COPY)).map_err(io)?.len();
        copy_bytes.set(copy_bytes.get() + bytes);
        let result = validate_entry(path, sequence);
        if result.is_ok() {
            quick_checks.set(quick_checks.get() + 1);
        }
        result
    })
}

fn assert_rejected_without_managed_backup(
    path: &Path,
    root: &Path,
    starting_revision: u32,
) -> TestResult {
    let before = state(path)?;
    let inspector_calls = Cell::new(0);
    let failed_run = apply_managed_run_with(
        path,
        root,
        &HISTORY_DEFINITIONS,
        starting_revision,
        |root| {
            inspector_calls.set(inspector_calls.get() + 1);
            inspect_retention(root)
        },
    );
    let failure = match failed_run {
        Ok(_) => return Err("stale starting revision must be rejected".into()),
        Err(failure) => failure,
    };
    assert!(matches!(failure.migration, MigrationError::VersionMismatch));
    assert_eq!(state(path)?, before);
    assert_eq!(inspector_calls.get(), 1);
    assert!(!failure.retention.inspection_failed);
    assert_eq!(fs::read_dir(root)?.count(), 0);
    Ok(())
}

#[test]
fn multi_step_run_scans_the_final_backup_set_once() -> TestResult {
    let baseline = tempfile::tempdir()?;
    let baseline_path = baseline.path().join("diskunfold.db");
    let baseline_root = baseline.path().join("backups");
    seed_with_two_backups(&baseline_path, &baseline_root)?;
    let baseline_validations = Cell::new(0);
    let baseline_quick_checks = Cell::new(0);
    let baseline_bytes = Cell::new(0);
    for expected in [2, 3, 4, 5, 6] {
        let result = apply_next_managed_with(
            &baseline_path,
            &baseline_root,
            &HISTORY_DEFINITIONS,
            |root| {
                counted_retention(
                    root,
                    &baseline_validations,
                    &baseline_quick_checks,
                    &baseline_bytes,
                )
            },
        )
        .map_err(|error| format!("{error:?}"))?;
        assert_eq!(result.revision, expected);
    }

    let optimized = tempfile::tempdir()?;
    let optimized_path = optimized.path().join("diskunfold.db");
    let optimized_root = optimized.path().join("backups");
    seed_with_two_backups(&optimized_path, &optimized_root)?;
    let optimized_validations = Cell::new(0);
    let optimized_quick_checks = Cell::new(0);
    let optimized_bytes = Cell::new(0);
    let outcome = apply_managed_run_with(
        &optimized_path,
        &optimized_root,
        &HISTORY_DEFINITIONS,
        1,
        |root| {
            counted_retention(
                root,
                &optimized_validations,
                &optimized_quick_checks,
                &optimized_bytes,
            )
        },
    )
    .map_err(|failure| format!("{failure:?}"))?;
    assert_eq!((outcome.revision, outcome.applied_steps), (6, 5));
    assert_eq!(outcome.retention.content_valid, 7);
    assert_eq!(baseline_validations.get(), 25);
    assert_eq!(baseline_quick_checks.get(), 25);
    assert_eq!(optimized_validations.get(), 7);
    assert_eq!(optimized_quick_checks.get(), 7);
    assert!(optimized_bytes.get() < baseline_bytes.get());
    println!(
        "retention scans: per-step validations={} quick_checks={} copy_bytes={}; terminal validations={} quick_checks={} copy_bytes={}",
        baseline_validations.get(),
        baseline_quick_checks.get(),
        baseline_bytes.get(),
        optimized_validations.get(),
        optimized_quick_checks.get(),
        optimized_bytes.get(),
    );
    assert_eq!(state(&optimized_path)?.0, 6);
    Ok(())
}

#[test]
fn failed_step_is_diagnosed_restored_and_restart_migrates_from_the_old_revision() -> TestResult {
    let sandbox = tempfile::tempdir()?;
    let path = sandbox.path().join("diskunfold.db");
    let root = sandbox.path().join("backups");
    seed_with_two_backups(&path, &root)?;
    let good_step_2 = &HISTORY_DEFINITIONS.steps[0];
    let good_step_3 = &HISTORY_DEFINITIONS.steps[1];
    let bad_sql = format!("{}\nINSERT INTO missing_table VALUES (1);", good_step_3.sql);
    let failed_steps = [
        crate::migration::Definition {
            number: good_step_2.number,
            sql: good_step_2.sql,
        },
        crate::migration::Definition {
            number: good_step_3.number,
            sql: &bad_sql,
        },
        crate::migration::Definition {
            number: HISTORY_DEFINITIONS.steps[2].number,
            sql: HISTORY_DEFINITIONS.steps[2].sql,
        },
        crate::migration::Definition {
            number: HISTORY_DEFINITIONS.steps[3].number,
            sql: HISTORY_DEFINITIONS.steps[3].sql,
        },
        crate::migration::Definition {
            number: HISTORY_DEFINITIONS.steps[4].number,
            sql: HISTORY_DEFINITIONS.steps[4].sql,
        },
    ];
    let failing_definitions = Definitions {
        baseline: HISTORY_DEFINITIONS.baseline,
        steps: &failed_steps,
    };
    let failure_validations = Cell::new(0);
    let failure_quick_checks = Cell::new(0);
    let failure_bytes = Cell::new(0);
    let failed_run = apply_managed_run_with(&path, &root, &failing_definitions, 1, |root| {
        counted_retention(
            root,
            &failure_validations,
            &failure_quick_checks,
            &failure_bytes,
        )
    });
    let failure = match failed_run {
        Ok(_) => return Err("the second definition must fail".into()),
        Err(failure) => failure,
    };
    assert!(matches!(failure.migration, MigrationError::Sqlite(_)));
    assert!(matches!(
        failure.restore,
        RestoreStatus::Restored(RestoredBackup {
            sequence: 3,
            version: 1
        })
    ));
    assert_eq!(state(&path)?.0, 1);
    assert_eq!(failure.retention.content_valid, 4);
    assert!(!failure.retention.inspection_failed);
    assert!(failure.retention.cleanup_unavailable);
    assert_eq!(failure_validations.get(), 4);
    assert_eq!(failure_quick_checks.get(), 4);
    assert!(valid_entry(&root.join("backup-00000000000000000004"), 4));

    let restart_validations = Cell::new(0);
    let restart_quick_checks = Cell::new(0);
    let restart_bytes = Cell::new(0);
    let restarted = apply_managed_run_with(&path, &root, &HISTORY_DEFINITIONS, 1, |root| {
        counted_retention(
            root,
            &restart_validations,
            &restart_quick_checks,
            &restart_bytes,
        )
    })
    .map_err(|failure| format!("{failure:?}"))?;
    assert_eq!((restarted.revision, restarted.applied_steps), (6, 5));
    assert_eq!(restarted.retention.content_valid, 9);
    assert_eq!(restart_validations.get(), 9);
    assert_eq!(restart_quick_checks.get(), 9);
    assert_eq!(state(&path)?.0, 6);
    Ok(())
}

#[test]
fn terminal_scan_observes_corruption_and_replacement_without_deleting_entries() -> TestResult {
    let sandbox = tempfile::tempdir()?;
    let path = sandbox.path().join("diskunfold.db");
    let root = sandbox.path().join("backups");
    seed_with_two_backups(&path, &root)?;
    let inspector_calls = Cell::new(0);
    let outcome = apply_managed_run_with(&path, &root, &HISTORY_DEFINITIONS, 1, |root| {
        inspector_calls.set(inspector_calls.get() + 1);
        let first = root.join("backup-00000000000000000001");
        let second = root.join("backup-00000000000000000002");
        let displaced = root.join("backup-00000000000000000002-old");
        let marker = fs::read(first.join(MARKER)).map_err(io)?;
        let original_copy = fs::read(first.join(COPY)).map_err(io)?;
        let mut corrupt = OpenOptions::new()
            .read(true)
            .write(true)
            .open(first.join(COPY))
            .map_err(io)?;
        corrupt.seek(SeekFrom::Start(100)).map_err(io)?;
        let mut byte = [0u8; 1];
        corrupt.read_exact(&mut byte).map_err(io)?;
        byte[0] ^= 0xff;
        corrupt.seek(SeekFrom::Start(100)).map_err(io)?;
        corrupt.write_all(&byte).map_err(io)?;
        drop(corrupt);
        fs::rename(&second, &displaced).map_err(io)?;
        fs::create_dir(&second).map_err(io)?;
        fs::write(second.join(MARKER), marker).map_err(io)?;
        fs::write(second.join(COPY), original_copy).map_err(io)?;
        inspect_retention(root)
    })
    .map_err(|failure| format!("{failure:?}"))?;
    assert_eq!(outcome.revision, 6);
    assert_eq!(inspector_calls.get(), 1);
    assert_eq!(outcome.retention.content_valid, 5);
    assert!(outcome.retention.ignored >= 3);
    assert!(!outcome.retention.inspection_failed);
    assert!(outcome.retention.cleanup_unavailable);
    for sequence in 1..=7 {
        assert!(root.join(format!("backup-{sequence:020}")).exists());
    }
    assert!(root.join("backup-00000000000000000002-old").exists());
    Ok(())
}

#[test]
fn terminal_inspection_failure_does_not_mask_migration_failure() -> TestResult {
    let sandbox = tempfile::tempdir()?;
    let path = sandbox.path().join("diskunfold.db");
    let root = sandbox.path().join("backups");
    fs::create_dir(&root)?;
    seed(&path)?;
    let first = &HISTORY_DEFINITIONS.steps[0];
    let bad_sql = format!("{}\nINSERT INTO missing_table VALUES (1);", first.sql);
    let steps = [crate::migration::Definition {
        number: first.number,
        sql: &bad_sql,
    }];
    let definitions = Definitions {
        baseline: HISTORY_DEFINITIONS.baseline,
        steps: &steps,
    };
    let failed_run = apply_managed_run_with(&path, &root, &definitions, 1, |_| {
        Err(MigrationError::Integrity)
    });
    let failure = match failed_run {
        Ok(_) => return Err("the definition must fail".into()),
        Err(failure) => failure,
    };
    assert!(matches!(failure.migration, MigrationError::Sqlite(_)));
    assert!(failure.retention.inspection_failed);
    assert!(failure.retention.cleanup_unavailable);
    assert_eq!(state(&path)?.0, 1);
    assert!(valid_entry(&root.join("backup-00000000000000000001"), 1));
    Ok(())
}

#[test]
fn read_only_open_does_not_migrate_a_legacy_database_or_create_backups() -> TestResult {
    let sandbox = tempfile::tempdir()?;
    let path = sandbox.path().join("diskunfold.db");
    let root = sandbox.path().join("backups");
    seed(&path)?;
    let reader = AppDb::open_read_only(&path).map_err(|error| format!("{error:?}"))?;
    assert_eq!(reader.schema_version()?, 1);
    drop(reader);
    assert_eq!(state(&path)?.0, 1);
    assert!(!root.exists());
    Ok(())
}

#[test]
fn stale_starting_revision_ahead_is_rejected_before_backup_or_schema_change() -> TestResult {
    let sandbox = tempfile::tempdir()?;
    let path = sandbox.path().join("diskunfold.db");
    let root = sandbox.path().join("backups");
    seed(&path)?;
    fs::create_dir(&root)?;
    assert_eq!(state(&path)?.0, 1);

    assert_rejected_without_managed_backup(&path, &root, 2)
}

#[test]
fn stale_starting_revision_behind_is_rejected_before_backup_or_schema_change() -> TestResult {
    let sandbox = tempfile::tempdir()?;
    let path = sandbox.path().join("diskunfold.db");
    let root = sandbox.path().join("backups");
    seed(&path)?;
    let ordinary_backup = sandbox.path().join("ordinary-backup.sqlite");
    assert_eq!(
        crate::migration::apply_next(&path, &ordinary_backup, &HISTORY_DEFINITIONS)
            .map_err(|error| format!("{error:?}"))?,
        2
    );
    fs::create_dir(&root)?;
    assert_eq!(state(&path)?.0, 2);

    assert_rejected_without_managed_backup(&path, &root, 1)
}

mod zero_step;
