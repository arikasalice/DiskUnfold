use super::*;

#[test]
fn stores_scope_roots_for_list_and_detail_and_caps_the_stored_prefix() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("scope-roots.db");
    let mut writer = installed(&path)?;
    for (job_id, roots, scope) in [
        (1, vec!["C:\\one".to_owned()], "metadata: 1 root".to_owned()),
        (
            2,
            vec!["/one".into(), "/two".into(), "/three".into()],
            "metadata: 3 roots".to_owned(),
        ),
        (
            3,
            (0..150).map(|index| format!("/root-{index}")).collect(),
            "metadata: 150 roots".to_owned(),
        ),
    ] {
        let mut job = summary(job_id);
        job.scope_summary = scope;
        job.scope_roots = roots;
        assert_eq!(
            writer.append_scan_history(&job, i64::from(job_id)),
            Ok(HistoryAppend::Added)
        );
    }
    let mut old = summary(4);
    old.finished_at_ms = 100;
    assert_eq!(
        writer.append_scan_history(&old, 0),
        Ok(HistoryAppend::Added)
    );
    let old_id = id(4).v7_bytes().ok_or("invalid old fixture job id")?;
    let tx = writer.connection.transaction()?;
    tx.execute("UPDATE app_db_purge_gate SET open=1", [])?;
    tx.execute(
        "DELETE FROM job_runtime_job_scope_root WHERE job_id=?1",
        [old_id.as_slice()],
    )?;
    tx.execute("UPDATE app_db_purge_gate SET open=0", [])?;
    tx.commit()?;
    let large_job_id = id(3).v7_bytes().ok_or("invalid fixture job id")?;
    let stored_roots: i64 = writer.connection.query_row(
        "SELECT count(*) FROM job_runtime_job_scope_root WHERE job_id=?1",
        [large_job_id.as_slice()],
        |row| row.get(0),
    )?;
    assert_eq!(stored_roots, 100);
    drop(writer);

    let mut reader = AppDb::open_read_only(&path).map_err(|error| format!("{error:?}"))?;
    let page = reader
        .read_history_page(read::Filter::default(), None, 10)
        .map_err(|error| format!("{error:?}"))?;
    for (entry, expected_first, expected_count) in [
        (&page.rows[2], "C:\\one", 1),
        (&page.rows[1], "/one", 3),
        (&page.rows[0], "/root-0", 150),
    ] {
        let summary = entry
            .summary
            .as_ref()
            .ok_or("history summary unavailable")?;
        assert_eq!(summary.scope_first_root.as_deref(), Some(expected_first));
        assert_eq!(summary.scope_root_count, Some(expected_count));
        assert!(!summary.scope_roots_unavailable);
    }
    for (job_id, expected) in [
        (1, vec!["C:\\one".to_owned()]),
        (2, vec!["/one".into(), "/two".into(), "/three".into()]),
    ] {
        let detail = reader
            .read_history_detail(&id(job_id))
            .map_err(|error| format!("{error:?}"))?
            .ok_or("history detail unavailable")?;
        assert_eq!(detail.scope_roots, read::detail::Part::Present(expected));
    }
    let detail = reader
        .read_history_detail(&id(3))
        .map_err(|error| format!("{error:?}"))?
        .ok_or("history detail unavailable")?;
    assert_eq!(
        detail.scope_roots,
        read::detail::Part::Present((0..100).map(|index| format!("/root-{index}")).collect())
    );
    let root_count: u32 = match detail.summary {
        read::detail::Part::Present(summary) => {
            summary.scope_root_count.ok_or("root count absent")?
        }
        _ => return Err("history summary unavailable".into()),
    };
    assert_eq!(root_count, 150);
    let old_entry = page.rows.last().ok_or("old history row missing")?;
    let old_summary = old_entry.summary.as_ref().ok_or("old summary missing")?;
    assert_eq!(old_summary.scope_first_root, None);
    assert_eq!(old_summary.scope_root_count, None);
    assert!(!old_summary.scope_roots_unavailable);
    let old_detail = reader
        .read_history_detail(&id(4))
        .map_err(|error| format!("{error:?}"))?
        .ok_or("old history detail unavailable")?;
    assert_eq!(old_detail.scope_roots, read::detail::Part::Absent);
    assert!(matches!(
        old_detail.summary,
        read::detail::Part::Present(summary) if summary.scope_root_count.is_none()
    ));
    Ok(())
}

#[test]
fn mismatched_persisted_root_rows_are_unreadable() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("mismatched-scope-roots.db");
    let mut writer = installed(&path)?;
    writer
        .append_scan_history(&summary(1), 250)
        .map_err(|e| e.to_owned())?;
    let job_id = id(1).v7_bytes().ok_or("invalid fixture job id")?;
    writer.connection.execute(
        "INSERT INTO job_runtime_job_scope_root
             (job_id, root_seq, location_text, created_at_ms)
         VALUES (?1, 1, '/unexpected', 251)",
        [job_id.as_slice()],
    )?;
    drop(writer);

    let mut reader = AppDb::open_read_only(&path).map_err(|error| format!("{error:?}"))?;
    let page = reader
        .read_history_page(read::Filter::default(), None, 10)
        .map_err(|error| format!("{error:?}"))?;
    let row = page.rows[0]
        .summary
        .as_ref()
        .ok_or("history summary unavailable")?;
    assert_eq!(row.scope_first_root, None);
    assert_eq!(row.scope_root_count, None);
    assert!(row.scope_roots_unavailable);
    let detail = reader
        .read_history_detail(&id(1))
        .map_err(|error| format!("{error:?}"))?
        .ok_or("history detail unavailable")?;
    assert_eq!(detail.scope_roots, read::detail::Part::Unavailable);
    Ok(())
}

#[test]
fn rejects_invalid_root_lists_and_nonempty_duplicate_roots() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut db = installed(&dir.path().join("invalid-scope-roots.db"))?;
    let mut job = summary(1);
    job.scope_roots.clear();
    assert_eq!(
        db.append_scan_history(&job, 250),
        Err("history.record_failed".into())
    );
    job.scope_roots = vec![String::new()];
    assert_eq!(
        db.append_scan_history(&job, 250),
        Err("history.record_failed".into())
    );
    job.scope_roots = vec!["x".repeat(32_769)];
    assert_eq!(
        db.append_scan_history(&job, 250),
        Err("history.record_failed".into())
    );
    job.kind_id = "duplicate.check".into();
    job.scope_summary = "duplicate.check".into();
    job.scope_roots = vec!["/unexpected".into()];
    assert_eq!(
        db.append_scan_history(&job, 250),
        Err("history.record_failed".into())
    );
    assert_eq!(counts(&db)?, (0, 0, 0));
    Ok(())
}
