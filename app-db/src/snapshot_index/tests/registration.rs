use super::*;

#[test]
fn register_rejects_bad_file_names_scope_markers_and_condition_json() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = installed(&dir)?;
    let mut no_scope = row(39, 139, Some(100));
    no_scope.scope_label.clear();
    assert!(db.register(&no_scope, &[], None)?.inserted);
    for length in [1, 31, 33] {
        let mut invalid = row(length as u64, 100 + length as u64, Some(100));
        invalid.scope_label = vec![0; length];
        assert_eq!(
            db.register(&invalid, &[], None),
            Err(SnapshotIndexError::InvalidInput)
        );
    }
    let mut bad_name = row(40, 140, Some(100));
    bad_name.file_name = "../snapshot.dusnap".into();
    assert_eq!(
        db.register(&bad_name, &[], None),
        Err(SnapshotIndexError::InvalidInput)
    );
    let mut bad_json = row(41, 141, Some(100));
    bad_json.condition_summary = "not-json".into();
    assert_eq!(
        db.register(&bad_json, &[], None),
        Err(SnapshotIndexError::InvalidInput)
    );
    let mut maximum = row(43, 143, Some(100));
    maximum.condition_summary = format!("\"{}\"", "x".repeat(MAX_CONDITION_SUMMARY_BYTES - 2));
    assert!(db.register(&maximum, &[], None)?.inserted);
    let mut oversized = row(42, 142, Some(100));
    oversized.condition_summary = format!("\"{}\"", "x".repeat(MAX_CONDITION_SUMMARY_BYTES));
    assert_eq!(
        db.register(&oversized, &[], None),
        Err(SnapshotIndexError::InvalidInput)
    );
    assert_eq!(count(&db.connection, "snapshot_store_snapshot")?, 2);
    Ok(())
}
