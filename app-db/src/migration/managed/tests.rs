use super::*;
use crate::{AppDb, migration::Definition};

fn test_definition(sql: &str) -> [Definition<'_>; 1] {
    [Definition { number: 2, sql }]
}

#[test]
fn inspection_failure_is_diagnostic_after_successful_commit()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let root = dir.path().join("backups");
    fs::create_dir(&root)?;
    let path = dir.path().join("app.db");
    drop(AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?);
    let steps = test_definition("CREATE TABLE demo(value INTEGER) STRICT;");
    let result = apply_next_managed_with(
        &path,
        &root,
        &Definitions {
            baseline: 1,
            steps: &steps,
        },
        |_| Err(MigrationError::Integrity),
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.revision, 2);
    assert!(result.retention.inspection_failed && result.retention.cleanup_unavailable);
    let db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    assert_eq!(
        db.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))?,
        2
    );
    assert!(root.join("backup-00000000000000000001").is_dir());
    Ok(())
}

#[test]
fn completed_backups_are_verified_and_cleanup_is_never_activated()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let root = dir.path().join("backups");
    fs::create_dir(&root)?;
    let db = AppDb::create_gui(&dir.path().join("app.db")).map_err(|e| format!("{e:?}"))?;
    for expected in 1..=4 {
        assert_eq!(
            publish(&db.connection, 1, &root).map_err(|e| format!("{e:?}"))?,
            expected
        );
    }
    fs::create_dir(root.join("pending-00000000000000000005-incomplete"))?;
    fs::write(root.join("unregistered"), b"preserve")?;
    let result = inspect_retention(&root).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        result,
        RetentionDiagnostic {
            content_valid: 4,
            recognized: if cfg!(unix) { 4 } else { 0 },
            ignored: if cfg!(unix) { 2 } else { 6 },
            older_than_three: if cfg!(unix) { 1 } else { 0 },
            cleanup_unavailable: true,
            inspection_failed: false
        }
    );
    for sequence in 1..=4 {
        assert!(root.join(format!("backup-{sequence:020}")).is_dir());
    }
    assert_eq!(fs::read(root.join("unregistered"))?, b"preserve");
    assert_eq!(
        publish(&db.connection, 1, &root).map_err(|e| format!("{e:?}"))?,
        6
    );
    Ok(())
}

#[test]
fn bad_hash_and_unknown_contents_are_never_recognized() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let root = dir.path().join("backups");
    fs::create_dir(&root)?;
    let db = AppDb::create_gui(&dir.path().join("app.db")).map_err(|e| format!("{e:?}"))?;
    publish(&db.connection, 1, &root).map_err(|e| format!("{e:?}"))?;
    publish(&db.connection, 1, &root).map_err(|e| format!("{e:?}"))?;
    let first = root.join("backup-00000000000000000001");
    let second = root.join("backup-00000000000000000002");
    fs::write(first.join(MARKER), b"{}")?;
    fs::write(second.join("unknown"), b"keep")?;
    let diag = inspect_retention(&root).map_err(|e| format!("{e:?}"))?;
    assert_eq!((diag.recognized, diag.ignored), (0, 2));
    assert_eq!(fs::read(second.join("unknown"))?, b"keep");
    Ok(())
}

#[test]
fn modified_copy_is_preserved_and_excluded() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let root = dir.path().join("backups");
    fs::create_dir(&root)?;
    let db = AppDb::create_gui(&dir.path().join("app.db")).map_err(|e| format!("{e:?}"))?;
    publish(&db.connection, 1, &root).map_err(|e| format!("{e:?}"))?;
    let copy = root.join("backup-00000000000000000001").join(COPY);
    let mut file = OpenOptions::new().append(true).open(&copy)?;
    file.write_all(b"altered")?;
    drop(file);
    let diagnostic = inspect_retention(&root).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        (
            diagnostic.content_valid,
            diagnostic.recognized,
            diagnostic.ignored
        ),
        (0, 0, 1)
    );
    assert!(copy.exists());
    Ok(())
}

#[test]
fn matching_hash_without_application_identity_is_not_owned()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let root = dir.path().join("backups");
    fs::create_dir(&root)?;
    let db = AppDb::create_gui(&dir.path().join("app.db")).map_err(|e| format!("{e:?}"))?;
    publish(&db.connection, 1, &root).map_err(|e| format!("{e:?}"))?;
    let entry = root.join("backup-00000000000000000001");
    let copy = entry.join(COPY);
    let foreign = Connection::open(&copy)?;
    foreign.pragma_update(None, "application_id", 0)?;
    drop(foreign);
    let marker = entry.join(MARKER);
    let mut value: Value = serde_json::from_slice(&fs::read(&marker)?)?;
    value["blake3"] = Value::String(hash_file(&copy).map_err(|e| format!("{e:?}"))?);
    fs::write(&marker, serde_json::to_vec(&value)?)?;
    let diag = inspect_retention(&root).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        (diag.content_valid, diag.recognized, diag.ignored),
        (0, 0, 1)
    );
    assert!(copy.exists());
    Ok(())
}

#[test]
fn failed_definition_preserves_completed_backup_and_restart_uses_next_sequence()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let root = dir.path().join("backups");
    fs::create_dir(&root)?;
    let path = dir.path().join("app.db");
    drop(AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?);
    let bad =
        test_definition("CREATE TABLE partial(value INTEGER); INSERT INTO missing VALUES(1);");
    assert!(
        apply_next_managed(
            &path,
            &root,
            &Definitions {
                baseline: 1,
                steps: &bad
            }
        )
        .is_err()
    );
    let first = root.join("backup-00000000000000000001");
    assert!(valid_entry(&first, 1));
    let good = test_definition("CREATE TABLE demo(value INTEGER) STRICT;");
    let result = apply_next_managed(
        &path,
        &root,
        &Definitions {
            baseline: 1,
            steps: &good,
        },
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!((result.revision, result.backup_sequence), (2, 2));
    let second = root.join("backup-00000000000000000002");
    let names = fs::read_dir(&second)?
        .map(|x| x.map(|y| y.file_name()))
        .collect::<Vec<_>>();
    validate_entry(&second, 2).map_err(|e| format!("validation: {e:?}, entries: {names:?}"))?;
    Ok(())
}

#[test]
fn existing_unknown_target_is_preserved() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let root = dir.path().join("backups");
    fs::create_dir(&root)?;
    let unknown = root.join("backup-00000000000000000001");
    fs::create_dir(&unknown)?;
    fs::write(unknown.join("user.txt"), b"keep")?;
    let db = AppDb::create_gui(&dir.path().join("app.db")).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        publish(&db.connection, 1, &root).map_err(|e| format!("{e:?}"))?,
        2
    );
    assert_eq!(fs::read(unknown.join("user.txt"))?, b"keep");
    Ok(())
}

#[test]
fn linked_copy_is_preserved_and_not_eligible_for_retention()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let root = dir.path().join("backups");
    fs::create_dir(&root)?;
    let db = AppDb::create_gui(&dir.path().join("app.db")).map_err(|e| format!("{e:?}"))?;
    publish(&db.connection, 1, &root).map_err(|e| format!("{e:?}"))?;
    let copy = root.join("backup-00000000000000000001").join(COPY);
    let alias = dir.path().join("user-link.sqlite");
    fs::hard_link(&copy, &alias)?;
    let diagnostic = inspect_retention(&root).map_err(|e| format!("{e:?}"))?;
    assert_eq!(diagnostic.content_valid, 1);
    assert_eq!(diagnostic.recognized, 0);
    assert!(alias.exists() && copy.exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn symbolic_linked_entry_is_ignored_without_touching_target()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir()?;
    let root = dir.path().join("backups");
    fs::create_dir(&root)?;
    let target = dir.path().join("user-directory");
    fs::create_dir(&target)?;
    fs::write(target.join("keep"), b"untouched")?;
    symlink(&target, root.join("backup-00000000000000000001"))?;
    let result = inspect_retention(&root).map_err(|e| format!("{e:?}"))?;
    assert_eq!((result.recognized, result.ignored), (0, 1));
    assert_eq!(fs::read(target.join("keep"))?, b"untouched");
    Ok(())
}

#[test]
fn competing_managed_attempts_do_not_apply_a_definition_twice()
-> Result<(), Box<dyn std::error::Error>> {
    use std::sync::{Arc, Barrier};
    let dir = tempfile::tempdir()?;
    let root = dir.path().join("backups");
    fs::create_dir(&root)?;
    let path = dir.path().join("app.db");
    drop(AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?);
    let gate = Arc::new(Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let path = path.clone();
            let root = root.clone();
            let gate = Arc::clone(&gate);
            std::thread::spawn(move || {
                let definitions = test_definition("CREATE TABLE demo(value INTEGER) STRICT;");
                gate.wait();
                apply_next_managed(
                    &path,
                    &root,
                    &Definitions {
                        baseline: 1,
                        steps: &definitions,
                    },
                )
                .is_ok()
            })
        })
        .collect();
    let mut success = 0;
    for handle in handles {
        if handle.join().map_err(|_| "thread panicked")? {
            success += 1;
        }
    }
    assert_eq!(success, 1);
    let db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    assert_eq!(
        db.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))?,
        2
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM app_db_schema_meta", [], |row| row
            .get::<_, i64>(0))?,
        1
    );
    Ok(())
}
