use super::*;
use crate::AppDb;

const STEP2: &str =
    "CREATE TABLE demo_value (value INTEGER NOT NULL) STRICT; INSERT INTO demo_value VALUES(42);";
const STEP3: &str = "CREATE TABLE demo_other (value INTEGER NOT NULL) STRICT;";

fn definitions() -> [Definition<'static>; 2] {
    [
        Definition {
            number: 2,
            sql: STEP2,
        },
        Definition {
            number: 3,
            sql: STEP3,
        },
    ]
}

fn version(path: &Path) -> rusqlite::Result<u32> {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.pragma_query_value(None, "user_version", |row| row.get(0))
}

#[test]
fn applies_registered_numbers_in_order_without_assuming_contiguity()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("app.db");
    drop(AppDb::create_gui(&path).map_err(|error| format!("{error:?}"))?);
    let noncanonical = [Definition {
        number: 2,
        sql: "CREATE TABLE demo_value(value INTEGER);\r\n",
    }];
    let rejected_backup = dir.path().join("rejected.db");
    assert!(matches!(
        apply_next(
            &path,
            &rejected_backup,
            &Definitions {
                baseline: 1,
                steps: &noncanonical
            }
        ),
        Err(MigrationError::InvalidDefinitions)
    ));
    assert!(!rejected_backup.exists());
    let mut steps = definitions();
    steps[0].number = 12;
    steps[1].number = 34;
    let registered = Definitions {
        baseline: 1,
        steps: &steps,
    };
    for (index, expected) in [12, 34].into_iter().enumerate() {
        let backup = dir.path().join(format!("backup-{index}.db"));
        assert_eq!(
            apply_next(&path, &backup, &registered).map_err(|error| format!("{error:?}"))?,
            expected
        );
        assert_eq!(version(&backup)?, [1, 12][index]);
        assert_eq!(version(&path)?, expected);
    }
    Ok(())
}

#[test]
fn applies_one_definition_after_recoverable_online_backup() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("app.db");
    let backup = dir.path().join("backup.db");
    drop(AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?);
    let seeded = Connection::open(&path)?;
    seeded.execute_batch("INSERT INTO user_env_setting VALUES ('locale.display_language', '\"ko\"', 'screen', 1, 2, 1); UPDATE user_env_setting_version SET settings_version=7;")?;
    drop(seeded);
    let cli = AppDb::open_read_only(&path).map_err(|e| format!("{e:?}"))?;
    assert_eq!(cli.schema_version()?, 1);
    drop(cli);
    let steps = definitions();
    let set = Definitions {
        baseline: 1,
        steps: &steps,
    };
    assert!(apply_next(&path, &dir.path().join("missing").join("backup.db"), &set).is_err());
    assert_eq!(version(&path)?, 1);
    assert_eq!(
        apply_next(&path, &backup, &set).map_err(|e| format!("{e:?}"))?,
        2
    );
    assert_eq!(version(&backup)?, 1);
    let copy = Connection::open_with_flags(&backup, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    assert_eq!(
        copy.query_row(
            "SELECT settings_version FROM user_env_setting_version",
            [],
            |r| r.get::<_, i64>(0)
        )?,
        7
    );
    assert_eq!(
        copy.query_row("SELECT value_json FROM user_env_setting", [], |r| r
            .get::<_, String>(0))?,
        "\"ko\""
    );
    assert_eq!(
        copy.query_row("SELECT count(*) FROM user_env_setting_version", [], |r| r
            .get::<_, i64>(
            0
        ))?,
        1
    );
    assert_eq!(version(&path)?, 2);
    let db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    assert_eq!(
        db.query_row(
            "SELECT settings_version FROM user_env_setting_version",
            [],
            |r| r.get::<_, i64>(0)
        )?,
        7
    );
    assert_eq!(
        db.query_row("SELECT value FROM demo_value", [], |r| r.get::<_, i64>(0))?,
        42
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM app_db_schema_meta", [], |r| r
            .get::<_, i64>(0))?,
        1
    );
    drop(db);
    assert!(matches!(
        AppDb::open_read_only(&path),
        Err(OpenError::HistorySchema(error)) if matches!(*error, MigrationError::Tampered)
    ));
    assert_eq!(version(&path)?, 2);
    Ok(())
}

#[test]
fn backup_failure_sql_failure_and_foreign_files_preserve_state()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("app.db");
    drop(AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?);
    let steps = definitions();
    let set = Definitions {
        baseline: 1,
        steps: &steps,
    };
    let existing = dir.path().join("existing.db");
    std::fs::write(&existing, b"preserve")?;
    assert!(apply_next(&path, &existing, &set).is_err());
    assert_eq!(std::fs::read(&existing)?, b"preserve");
    assert_eq!(version(&path)?, 1);
    let invalid = [Definition {
        number: 2,
        sql: "CREATE TABLE partial(value INTEGER); INSERT INTO missing VALUES(1);",
    }];
    assert!(
        apply_next(
            &path,
            &dir.path().join("copy.db"),
            &Definitions {
                baseline: 1,
                steps: &invalid
            }
        )
        .is_err()
    );
    assert_eq!(version(&path)?, 1);
    let db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    assert!(!db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='partial')",
        [],
        |r| r.get::<_, bool>(0)
    )?);
    drop(db);
    let foreign = dir.path().join("foreign.db");
    let db = Connection::open(&foreign)?;
    db.execute("CREATE TABLE foreign_table(value INTEGER)", [])?;
    drop(db);
    assert!(matches!(
        apply_next(&foreign, &dir.path().join("foreign-backup.db"), &set),
        Err(MigrationError::ForeignDatabase)
    ));
    assert!(!dir.path().join("foreign-backup.db").exists());
    Ok(())
}

#[test]
fn definition_cannot_commit_outside_the_runner_transaction()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("app.db");
    drop(AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?);
    let definitions = [Definition {
        number: 2,
        sql: "CREATE TABLE escaped(value INTEGER); COMMIT;",
    }];
    assert!(
        apply_next(
            &path,
            &dir.path().join("backup.db"),
            &Definitions {
                baseline: 1,
                steps: &definitions
            }
        )
        .is_err()
    );
    assert_eq!(version(&path)?, 1);
    let db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    assert!(!db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='escaped')",
        [],
        |row| row.get::<_, bool>(0)
    )?);
    Ok(())
}

#[test]
fn definition_cannot_remove_migration_history_guards() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("app.db");
    drop(AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?);
    let steps = [
        Definition {
            number: 2,
            sql: STEP2,
        },
        Definition {
            number: 3,
            sql: "DROP TRIGGER app_db_schema_meta_no_delete;",
        },
    ];
    let set = Definitions {
        baseline: 1,
        steps: &steps,
    };
    apply_next(&path, &dir.path().join("first.db"), &set).map_err(|e| format!("{e:?}"))?;
    assert!(apply_next(&path, &dir.path().join("second.db"), &set).is_err());
    assert_eq!(version(&path)?, 2);
    let db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    assert!(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='app_db_schema_meta_no_delete')",
        [],
        |r| r.get::<_, bool>(0)
    )?);
    Ok(())
}

#[test]
fn changed_history_is_rejected_before_backup() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("app.db");
    drop(AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?);
    let steps = definitions();
    let set = Definitions {
        baseline: 1,
        steps: &steps,
    };
    apply_next(&path, &dir.path().join("first.db"), &set).map_err(|e| format!("{e:?}"))?;
    let changed = [
        Definition {
            number: 2,
            sql: "SELECT 1;",
        },
        Definition {
            number: 3,
            sql: STEP3,
        },
    ];
    let backup = dir.path().join("second.db");
    assert!(matches!(
        apply_next(
            &path,
            &backup,
            &Definitions {
                baseline: 1,
                steps: &changed
            }
        ),
        Err(MigrationError::Tampered)
    ));
    assert_eq!(version(&path)?, 2);
    assert!(!backup.exists());
    Ok(())
}

#[test]
fn competing_write_after_backup_rejects_stale_upgrade() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("app.db");
    drop(AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?);
    let steps = definitions();
    let set = Definitions {
        baseline: 1,
        steps: &steps,
    };
    let result = apply_next_with(&path, &dir.path().join("backup.db"), &set, || {
        let other = Connection::open(&path)?;
        other.execute(
            "UPDATE user_env_setting_version SET settings_version = 1",
            [],
        )?;
        Ok(())
    });
    assert!(result.is_err());
    assert_eq!(version(&path)?, 1);
    let db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    assert!(!db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='demo_value')",
        [],
        |r| r.get::<_, bool>(0)
    )?);
    Ok(())
}

#[test]
fn concurrent_gui_attempts_do_not_apply_the_definition_twice()
-> Result<(), Box<dyn std::error::Error>> {
    use std::sync::{Arc, Barrier};
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("app.db");
    drop(AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?);
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|index| {
            let path = path.clone();
            let backup = dir.path().join(format!("backup-{index}.db"));
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let steps = definitions();
                apply_next_with(
                    &path,
                    &backup,
                    &Definitions {
                        baseline: 1,
                        steps: &steps,
                    },
                    || {
                        barrier.wait();
                        Ok(())
                    },
                )
                .is_ok()
            })
        })
        .collect();
    let mut successes = 0;
    for handle in handles {
        if handle.join().map_err(|_| "migration thread panicked")? {
            successes += 1;
        }
    }
    assert_eq!(successes, 1);
    assert_eq!(version(&path)?, 2);
    let db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    assert_eq!(
        db.query_row("SELECT count(*) FROM app_db_schema_meta", [], |row| row
            .get::<_, i64>(0))?,
        1
    );
    Ok(())
}

#[test]
fn only_the_complete_known_legacy_schema_and_version_row_are_accepted()
-> Result<(), Box<dyn std::error::Error>> {
    let cases = [
        "DROP INDEX ix_user_env_setting_quarantine_key;",
        "CREATE TABLE unexplained_partial(value INTEGER);",
        "CREATE TABLE sqlitex_unexpected(value INTEGER);",
        "UPDATE user_env_setting_version SET schema_version=2;",
        "DELETE FROM user_env_setting_version;",
        "CREATE TABLE app_db_schema_meta(migration_no INTEGER, definition_hash BLOB);",
    ];
    for (index, alteration) in cases.iter().enumerate() {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("app.db");
        drop(AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?);
        let db = Connection::open(&path)?;
        db.execute_batch(alteration)?;
        drop(db);
        let before = std::fs::read(&path)?;
        let backup = dir.path().join(format!("backup-{index}.db"));
        let steps = definitions();
        let result = apply_next(
            &path,
            &backup,
            &Definitions {
                baseline: 1,
                steps: &steps,
            },
        );
        assert!(matches!(result, Err(MigrationError::UnrecognizedBaseline)));
        assert!(!backup.exists());
        assert_eq!(version(&path)?, 1);
        assert_eq!(std::fs::read(&path)?, before);
    }
    Ok(())
}

#[test]
fn schema_change_after_validation_cannot_be_migrated_from_stale_backup()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("app.db");
    drop(AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?);
    let steps = definitions();
    let result = apply_next_with(
        &path,
        &dir.path().join("backup.db"),
        &Definitions {
            baseline: 1,
            steps: &steps,
        },
        || {
            let other = Connection::open(&path)?;
            other.execute("CREATE TABLE concurrent_change(value INTEGER)", [])?;
            Ok(())
        },
    );
    assert!(result.is_err());
    assert_eq!(version(&path)?, 1);
    let db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    assert!(!db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='demo_value')",
        [],
        |row| row.get::<_, bool>(0)
    )?);
    Ok(())
}
