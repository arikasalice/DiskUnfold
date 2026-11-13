use super::*;
use du_core_contracts::{
    Id,
    job::{JobCounts, JobState, JobSummary},
};

fn parent_id() -> [u8; 16] {
    [1, 1, 1, 1, 1, 1, 0x71, 1, 0x81, 1, 1, 1, 1, 1, 1, 1]
}

fn append(db: &mut AppDb, n: u8) -> Result<(), String> {
    let id = Id::from_v7_bytes([n; 16]);
    let value = JobSummary {
        job_id: id.clone(),
        kind_id: "scan.metadata".into(),
        kind_version: 1,
        scope_summary: "metadata: 1 root".into(),
        state: JobState::Done,
        counts: JobCounts::default(),
        failure_reason_key: None,
        unprocessed_summary: None,
        result_version_id: None,
        started_at_ms: Some(10),
        finished_at_ms: 20,
        correlation_id: id,
    };
    db.append_scan_history(&value, 30).map(|_| ())
}

#[test]
fn reads_missing_and_interrupted_components_and_successor_links()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("detail.db");
    let mut writer = AppDb::create_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    append(&mut writer, 1)?;
    writer.connection.execute_batch(
        "INSERT INTO job_runtime_job_start (job_id, kind_id, kind_version, scope_summary, started_at_ms, record_origin, created_at_ms)
         VALUES (X'02020202020202020202020202020202','change.trash',1,'C:\\private',50,'this_app',50);
         INSERT INTO job_runtime_job_interrupt VALUES (X'02020202020202020202020202020202', 60);
         INSERT INTO job_runtime_job_link (job_id, predecessor_job_id, relation_kind, record_origin, reckon_at_ms, record_seq, created_at_ms)
         VALUES (X'02020202020202020202020202020202', X'01010101010171018101010101010101', 'retry', 'this_app',60,2,60);
         UPDATE job_runtime_history_state SET last_record_seq=2;"
    )?;
    writer.connection.execute_batch("INSERT INTO job_runtime_job_link
        (job_id, predecessor_job_id, relation_kind, record_origin, reckon_at_ms, record_seq, created_at_ms)
        VALUES (X'03030303030303030303030303030303', X'09090909090909090909090909090909', 'retry', 'this_app', 70, 3, 70)")?;
    assert!(matches!(
        writer.read_history_detail(&Id::from_v7_bytes([1; 16])),
        Err(Error::RequiresReadConnection)
    ));
    let mut reader = AppDb::open_read_only(&path).map_err(|e| format!("{e:?}"))?;
    assert!(
        reader
            .read_history_detail_with([9; 16], || {})
            .map_err(|e| format!("{e:?}"))?
            .is_none()
    );
    let parent = reader
        .read_history_detail(&Id::from_v7_bytes([1; 16]))
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing parent")?;
    assert!(matches!(parent.summary, Part::Present(_)));
    assert_eq!(parent.start, Part::Absent);
    assert_eq!(parent.interrupted_at_ms, Part::Absent);
    assert_eq!(
        parent.successors,
        Part::Present(vec![Successor {
            job_id: Part::Present([2; 16]),
            relation: Part::Present(Relation::Retry),
            end: Part::Present(SuccessorEnd {
                state: EndState::Interrupted,
                finished_at_ms: None,
            }),
        }])
    );
    let child = reader
        .read_history_detail_with([2; 16], || {})
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing child")?;
    assert_eq!(child.summary, Part::Absent);
    assert_eq!(child.interrupted_at_ms, Part::Present(60));
    assert_eq!(child.predecessor_present, Part::Present(true));
    assert_eq!(
        child.link,
        Part::Present(Link {
            predecessor_job_id: Part::Present(parent_id()),
            relation: Part::Present(Relation::Retry),
            plan_version_id: Part::Absent,
            plan_lineage_id: Part::Absent,
        })
    );
    assert!(matches!(
        child.start,
        Part::Present(Start {
            scope_summary: None,
            ..
        })
    ));
    assert!(!format!("{child:?}").contains("private"));
    let orphan = reader
        .read_history_detail_with([3; 16], || {})
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing orphan")?;
    assert_eq!(orphan.predecessor_present, Part::Present(false));
    Ok(())
}

#[test]
fn corrupted_parts_are_unavailable_without_losing_readable_parts()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("corrupt.db");
    let mut writer = AppDb::create_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    append(&mut writer, 1)?;
    writer.connection.execute_batch(
        "DROP TRIGGER job_runtime_job_record_no_update;
         PRAGMA ignore_check_constraints=ON;
         UPDATE job_runtime_job_record SET final_state='future' WHERE job_id=X'01010101010171018101010101010101';
         INSERT INTO job_runtime_job_link (job_id, predecessor_job_id, relation_kind, record_origin, reckon_at_ms, record_seq, created_at_ms)
         VALUES (X'02020202020202020202020202020202', X'01010101010171018101010101010101', 'future', 'this_app',60,2,60);"
    )?;
    let mut reader = AppDb::open_read_only(&path).map_err(|e| format!("{e:?}"))?;
    let detail = reader
        .read_history_detail(&Id::from_v7_bytes([1; 16]))
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing")?;
    assert_eq!(detail.summary, Part::Unavailable);
    assert_eq!(detail.start, Part::Absent);
    assert!(matches!(detail.link, Part::Present(_)));
    assert_eq!(
        detail.successors,
        Part::Present(vec![Successor {
            job_id: Part::Present([2; 16]),
            relation: Part::Unavailable,
            end: Part::Absent,
        }])
    );
    Ok(())
}

#[test]
fn all_components_use_the_snapshot_established_before_concurrent_append()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("snapshot.db");
    let mut writer = AppDb::create_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    append(&mut writer, 1)?;
    let mut reader = AppDb::open_read_only(&path).map_err(|e| format!("{e:?}"))?;
    let detail = reader.read_history_detail_with(parent_id(), || {
        writer.connection.execute_batch("INSERT INTO job_runtime_job_link
          (job_id, predecessor_job_id, relation_kind, record_origin, reckon_at_ms, record_seq, created_at_ms)
          VALUES (X'02020202020202020202020202020202', X'01010101010171018101010101010101', 'retry', 'this_app',60,2,60)").ok();
    }).map_err(|e| format!("{e:?}"))?.ok_or("missing")?;
    assert_eq!(detail.successors, Part::Present(vec![]));
    let next = reader
        .read_history_detail(&Id::from_v7_bytes([1; 16]))
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing")?;
    assert!(matches!(next.successors, Part::Present(rows) if rows.len() == 1));
    Ok(())
}
