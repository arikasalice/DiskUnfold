use super::*;
use du_core_contracts::{
    Id,
    job::{HistoryAppend, JobState, JobSummary},
};
use std::path::Path;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn writer(path: &Path) -> Result<AppDb, String> {
    AppDb::create_gui_with_history(path).map_err(|error| format!("{error:?}"))
}

fn reader(path: &Path) -> Result<AppDb, String> {
    AppDb::open_read_only(path).map_err(|error| format!("{error:?}"))
}

fn append(db: &mut AppDb, n: u8, time: i64) -> Result<(), String> {
    let id = Id::from_v7_bytes([n; 16]);
    let summary = JobSummary {
        job_id: id.clone(),
        kind_id: "scan.metadata".into(),
        kind_version: 1,
        scope_summary: "metadata: 1 root".into(),
        state: JobState::Done,
        counts: Default::default(),
        failure_reason_key: None,
        unprocessed_summary: None,
        result_version_id: None,
        started_at_ms: Some(time - 1),
        finished_at_ms: time,
        correlation_id: id,
    };
    assert_eq!(
        db.append_scan_history(&summary, time)?,
        HistoryAppend::Added
    );
    Ok(())
}

fn read(
    db: &mut AppDb,
    filter: Filter,
    next: Option<Continuation>,
    limit: usize,
) -> Result<Page, String> {
    db.read_history_page(filter, next, limit)
        .map_err(|error| format!("{error:?}"))
}

#[test]
fn first_page_uses_one_snapshot_even_if_writer_commits_between_state_and_rows() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("history.db");
    let mut writer = writer(&path)?;
    append(&mut writer, 1, 90)?;
    append(&mut writer, 2, 100)?;
    append(&mut writer, 3, 100)?;
    let mut reader = reader(&path)?;
    let first = reader
        .read_history_page_with(Filter::default(), None, 1, || {
            assert!(append(&mut writer, 4, 110).is_ok());
        })
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(first.revision.max_record_seq, 3);
    assert_eq!(first.rows[0].position.record_seq, 3);
    let second = read(&mut reader, Filter::default(), first.next.clone(), 2)?;
    assert_eq!(second.revision, first.revision);
    assert_eq!(
        second
            .rows
            .iter()
            .map(|entry| entry.position.record_seq)
            .collect::<Vec<_>>(),
        [2, 1]
    );
    assert!(second.next.is_none());
    writer.connection.execute(
        "UPDATE job_runtime_history_state SET delete_generation=1",
        [],
    )?;
    assert!(matches!(
        reader.read_history_page(Filter::default(), first.next, 1),
        Err(Error::VersionChanged)
    ));
    Ok(())
}

#[test]
fn filter_and_read_connection_are_enforced() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("history.db");
    let mut writer = writer(&path)?;
    for n in 1..=3 {
        append(&mut writer, n, i64::from(n) * 10)?;
    }
    let filter = Filter {
        finished_from_ms: Some(10),
        finished_before_ms: Some(30),
        kinds: vec!["scan.metadata".into()],
        states: vec![EndState::Done],
        ..Filter::default()
    };
    assert!(matches!(
        writer.read_history_page(filter.clone(), None, 1),
        Err(Error::RequiresReadConnection)
    ));
    let mut reader = reader(&path)?;
    let first = read(&mut reader, filter.clone(), None, 1)?;
    assert_eq!(first.rows[0].position.record_seq, 2);
    let second = read(&mut reader, filter.clone(), first.next.clone(), 1)?;
    assert_eq!(second.rows[0].position.record_seq, 1);
    assert!(second.next.is_none());
    for limit in [0, 1001] {
        assert!(matches!(
            reader.read_history_page(filter.clone(), None, limit),
            Err(Error::InvalidRequest)
        ));
    }
    assert!(matches!(
        reader.read_history_page(Filter::default(), first.next, 1),
        Err(Error::InvalidRequest)
    ));
    assert!(
        read(
            &mut reader,
            Filter {
                relations: vec![Relation::Retry],
                ..filter
            },
            None,
            1000
        )?
        .rows
        .is_empty()
    );
    Ok(())
}

#[test]
fn interrupted_and_missing_summaries_preserve_sequence_without_leaking_paths() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("history.db");
    let writer = writer(&path)?;
    writer.connection.execute_batch(
        "BEGIN IMMEDIATE;
         INSERT INTO job_runtime_job_start
             (job_id, kind_id, kind_version, scope_summary, started_at_ms, record_origin, created_at_ms)
         VALUES (zeroblob(16), 'change.trash', 1, 'C:\\private', 50, 'this_app', 50);
         INSERT INTO job_runtime_job_interrupt VALUES (zeroblob(16), 100);
         INSERT INTO job_runtime_job_link
             (job_id, predecessor_job_id, relation_kind, record_origin, reckon_at_ms, record_seq, created_at_ms)
         VALUES (zeroblob(16), zeroblob(16), 'retry', 'this_app', 100, 1, 100),
                (X'01010101010101010101010101010101', NULL, NULL, 'this_app', 110, 2, 110);
         UPDATE job_runtime_history_state SET last_record_seq=2;
         COMMIT;",
    )?;
    let mut reader = reader(&path)?;
    let first = read(&mut reader, Filter::default(), None, 1)?;
    assert!(first.rows[0].summary.is_none());
    let next = read(&mut reader, Filter::default(), first.next, 1)?;
    let summary = next.rows[0]
        .summary
        .as_ref()
        .ok_or("missing interruption")?;
    assert_eq!(summary.state, EndState::Interrupted);
    assert_eq!(summary.started_at_ms, Some(50));
    assert!(summary.finished_at_ms.is_none());
    assert!(summary.counts.is_none());
    assert!(summary.scope_summary.is_none());
    assert!(!format!("{next:?}").contains("private"));
    let only_interrupted = read(
        &mut reader,
        Filter {
            states: vec![EndState::Interrupted],
            relations: vec![Relation::Retry],
            ..Filter::default()
        },
        None,
        1000,
    )?;
    assert_eq!(only_interrupted.rows, next.rows);
    Ok(())
}
