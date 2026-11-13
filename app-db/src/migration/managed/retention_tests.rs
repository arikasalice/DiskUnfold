use super::*;
use crate::{AppDb, migration::Definition};

fn prepare(root: &Path, source: &Connection, count: u64) -> Result<(), Box<dyn std::error::Error>> {
    for expected in 1..=count {
        assert_eq!(
            publish(source, 1, root).map_err(|error| format!("{error:?}"))?,
            expected
        );
    }
    Ok(())
}

#[test]
fn newest_three_boundary_is_diagnostic_and_never_deletes() -> Result<(), Box<dyn std::error::Error>>
{
    let sandbox = tempfile::tempdir()?;
    let root = sandbox.path().join("backups");
    fs::create_dir(&root)?;
    let database =
        AppDb::create_gui(&sandbox.path().join("app.db")).map_err(|e| format!("{e:?}"))?;
    prepare(&root, &database.connection, 3)?;
    let three = inspect_retention(&root).map_err(|e| format!("{e:?}"))?;
    assert_eq!(three.content_valid, 3);
    assert_eq!(three.older_than_three, 0);
    assert_eq!(
        publish(&database.connection, 1, &root).map_err(|e| format!("{e:?}"))?,
        4
    );
    let four = inspect_retention(&root).map_err(|e| format!("{e:?}"))?;
    assert_eq!(four.content_valid, 4);
    assert!(four.cleanup_unavailable);
    assert_eq!(four.older_than_three, if cfg!(unix) { 1 } else { 0 });
    for sequence in 1..=4 {
        assert!(
            root.join(format!("backup-{sequence:020}"))
                .join(COPY)
                .exists()
        );
    }
    Ok(())
}

#[test]
fn linked_replaced_and_incomplete_entries_are_preserved_on_reinspection()
-> Result<(), Box<dyn std::error::Error>> {
    let sandbox = tempfile::tempdir()?;
    let root = sandbox.path().join("backups");
    fs::create_dir(&root)?;
    let database =
        AppDb::create_gui(&sandbox.path().join("app.db")).map_err(|e| format!("{e:?}"))?;
    prepare(&root, &database.connection, 4)?;
    let first = root.join("backup-00000000000000000001");
    let second = root.join("backup-00000000000000000002");
    let third = root.join("backup-00000000000000000003");
    let external = sandbox.path().join("external.sqlite");
    fs::hard_link(first.join(COPY), &external)?;
    let preserved_marker = sandbox.path().join("old-marker.json");
    fs::rename(second.join(MARKER), &preserved_marker)?;
    fs::write(second.join(MARKER), b"replacement")?;
    fs::remove_file(third.join(MARKER))?;
    fs::write(root.join("unregistered"), b"keep")?;
    let report = inspect_retention(&root).map_err(|e| format!("{e:?}"))?;
    assert_eq!(report.content_valid, 2);
    assert!(!link_counts_safe(&first).map_err(|e| format!("{e:?}"))?);
    assert_eq!(report.recognized, if cfg!(unix) { 1 } else { 0 });
    assert!(report.cleanup_unavailable);
    assert_eq!(fs::read(second.join(MARKER))?, b"replacement");
    assert!(external.exists() && first.join(COPY).exists() && preserved_marker.exists());
    assert!(third.join(COPY).exists());
    assert_eq!(fs::read(root.join("unregistered"))?, b"keep");
    Ok(())
}

#[test]
fn verification_io_failure_is_reported_without_deleting_any_entry()
-> Result<(), Box<dyn std::error::Error>> {
    let sandbox = tempfile::tempdir()?;
    let root = sandbox.path().join("backups");
    fs::create_dir(&root)?;
    let database =
        AppDb::create_gui(&sandbox.path().join("app.db")).map_err(|e| format!("{e:?}"))?;
    prepare(&root, &database.connection, 4)?;
    let report = inspect_retention_with(&root, |path, sequence| {
        if sequence == 2 {
            return Err(MigrationError::Io(
                std::io::ErrorKind::PermissionDenied.into(),
            ));
        }
        validate_entry(path, sequence)
    })
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(report.content_valid, 3);
    assert!(report.inspection_failed && report.cleanup_unavailable);
    for sequence in 1..=4 {
        assert!(
            root.join(format!("backup-{sequence:020}"))
                .join(COPY)
                .exists()
        );
    }
    Ok(())
}

#[test]
fn partial_inspection_failure_after_commit_is_diagnostic_only()
-> Result<(), Box<dyn std::error::Error>> {
    let sandbox = tempfile::tempdir()?;
    let root = sandbox.path().join("backups");
    fs::create_dir(&root)?;
    let path = sandbox.path().join("app.db");
    drop(AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?);
    let steps = [Definition {
        number: 2,
        sql: "CREATE TABLE example(value INTEGER) STRICT;",
    }];
    let result = apply_next_managed_with(
        &path,
        &root,
        &Definitions {
            baseline: 1,
            steps: &steps,
        },
        |root| {
            inspect_retention_with(root, |_, _| {
                Err(MigrationError::Io(
                    std::io::ErrorKind::PermissionDenied.into(),
                ))
            })
        },
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.revision, 2);
    assert!(result.retention.inspection_failed && result.retention.cleanup_unavailable);
    assert!(root.join("backup-00000000000000000001").join(COPY).exists());
    let db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    assert_eq!(
        db.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))?,
        2
    );
    Ok(())
}
