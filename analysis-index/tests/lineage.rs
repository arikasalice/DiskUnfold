use du_analysis_index::{BuildHandle, IngestSink, QueryError};
use du_core_contracts::{
    Id, PathBytes, PathEncoding,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
    result_events::{ResultVersionChange, ResultVersionNotice, ResultVersionPublisher},
};
use std::sync::{Arc, Mutex};

fn id(n: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn build(sink: &IngestSink, lineage: &Id, scope: u32) -> Result<BuildHandle, String> {
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::WindowsUtf16le,
            raw: PathBytes::from_bytes(&[67, 0]),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut build = sink
        .open_forest_in_lineage(id(scope)?, vec![root], lineage.clone())
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory(Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    Ok(build)
}
fn current(sink: &IngestSink) -> Result<Option<Id>, String> {
    Ok(sink
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .map(|r| r.version_id().clone()))
}

#[derive(Default)]
struct Notices(Mutex<Vec<ResultVersionNotice>>);

#[derive(Default)]
struct ReentrantReader {
    sink: Mutex<Option<IngestSink>>,
    observed: Mutex<Vec<bool>>,
}

impl ResultVersionPublisher for ReentrantReader {
    fn publish_result_version(&self, notice: ResultVersionNotice) {
        let visible = self
            .sink
            .lock()
            .ok()
            .and_then(|guard| {
                let sink = guard.as_ref()?;
                let version = notice.result_version.as_ref()?;
                Some(sink.pin_current(version).is_ok())
            })
            .unwrap_or(false);
        if let Ok(mut observed) = self.observed.lock() {
            observed.push(visible);
        }
    }
}

#[test]
fn result_publisher_can_read_committed_current_without_store_lock() -> Result<(), String> {
    use std::sync::mpsc;
    use std::time::Duration;
    let observer = Arc::new(ReentrantReader::default());
    let sink = IngestSink::with_version_publisher(observer.clone());
    *observer.sink.lock().map_err(|_| "sink lock poisoned")? = Some(sink.clone());
    let lineage = sink
        .bind_current_analysis("callback-read", id(100)?)
        .map_err(|e| format!("{e:?}"))?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = build(&sink, &lineage, 101).and_then(|b| {
            b.seal(id(102)?, ResultCompleteness::Complete)
                .map(|_| ())
                .map_err(|e| format!("{e:?}"))
        });
        let _ = tx.send(result);
    });
    rx.recv_timeout(Duration::from_secs(3))
        .map_err(|e| format!("publication did not finish: {e}"))??;
    assert_eq!(
        *observer
            .observed
            .lock()
            .map_err(|_| "observed lock poisoned")?,
        [true]
    );
    *observer.sink.lock().map_err(|_| "sink lock poisoned")? = None;
    Ok(())
}

impl ResultVersionPublisher for Notices {
    fn publish_result_version(&self, notice: ResultVersionNotice) {
        if let Ok(mut notices) = self.0.lock() {
            notices.push(notice);
        }
    }
}

#[test]
fn notices_report_each_commit_and_withdraw_only_within_its_lineage() -> Result<(), String> {
    let notices = Arc::new(Notices::default());
    let sink = IngestSink::with_version_publisher(notices.clone());
    let a = sink
        .bind_current_analysis("a", id(1)?)
        .map_err(|e| format!("{e:?}"))?;
    let b = sink
        .bind_current_analysis("b", id(2)?)
        .map_err(|e| format!("{e:?}"))?;
    build(&sink, &a, 10)?
        .seal(id(20)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let mut replaces_a = build(&sink, &a, 11)?;
    replaces_a
        .publish_partial(id(21)?)
        .map_err(|e| format!("{e:?}"))?;
    let mut opens_b = build(&sink, &b, 12)?;
    opens_b
        .publish_partial(id(22)?)
        .map_err(|e| format!("{e:?}"))?;
    drop(opens_b);
    drop(replaces_a);
    drop(build(&sink, &a, 13)?);

    let notices = notices.0.lock().map_err(|_| "notice lock poisoned")?;
    assert_eq!(notices.len(), 5);
    use ResultVersionChange::{Published, Withdrawn};
    let expected = [
        (&a, 1, Published, None, Some(id(20)?)),
        (&a, 2, Published, Some(id(20)?), Some(id(21)?)),
        (&b, 1, Published, None, Some(id(22)?)),
        (&b, 2, Withdrawn, Some(id(22)?), None),
        (&a, 3, Withdrawn, Some(id(21)?), Some(id(20)?)),
    ];
    for (notice, (lineage, sequence, change, previous, current)) in notices.iter().zip(expected) {
        assert!(notice.validate());
        assert_eq!(&notice.body.lineage_id, lineage);
        assert_eq!(&notice.body.source_ref, lineage);
        assert_eq!(notice.body.sequence, sequence);
        assert_eq!(notice.body.change, change);
        assert_eq!(notice.body.previous_result_version, previous);
        assert_eq!(notice.result_version, current);
    }
    Ok(())
}

#[test]
fn same_scope_reuses_lineage_and_independent_currents_survive_replacement() -> Result<(), String> {
    let sink = IngestSink::default();
    let a = sink
        .bind_current_analysis("scope-a", id(1)?)
        .map_err(|e| format!("{e:?}"))?;
    let b = sink
        .bind_current_analysis("scope-b", id(2)?)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        a,
        sink.bind_current_analysis("scope-a", id(3)?)
            .map_err(|e| format!("{e:?}"))?
    );
    let old = build(&sink, &a, 10)?
        .seal(id(20)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    build(&sink, &b, 11)?
        .seal(id(21)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let new = build(&sink, &a, 12)?
        .seal(id(22)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(old.lineage_id(), Some(&a));
    assert_eq!(new.lineage_id(), Some(&a));
    assert_eq!(
        sink.pin_current(&id(20)?).err(),
        Some(QueryError::StaleVersion)
    );
    assert_eq!(old.items().len(), 1);
    assert_eq!(
        sink.pin_current(&id(21)?)
            .map_err(|e| format!("{e:?}"))?
            .lineage_id(),
        Some(&b)
    );
    assert_eq!(current(&sink)?, Some(id(22)?));
    assert!(!Arc::ptr_eq(&old, &new));
    Ok(())
}

#[test]
fn failure_restores_own_baseline_and_previous_focus_without_cross_lineage_replacement()
-> Result<(), String> {
    let sink = IngestSink::default();
    let a = sink
        .bind_current_analysis("a", id(1)?)
        .map_err(|e| format!("{e:?}"))?;
    let b = sink
        .bind_current_analysis("b", id(2)?)
        .map_err(|e| format!("{e:?}"))?;
    build(&sink, &a, 10)?
        .seal(id(20)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    build(&sink, &b, 11)?
        .seal(id(21)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let mut failed = build(&sink, &a, 12)?;
    let pin = failed
        .publish_partial(id(22)?)
        .map_err(|e| format!("{e:?}"))?;
    drop(failed);
    assert_eq!(current(&sink)?, Some(id(21)?));
    assert!(sink.pin_current(&id(20)?).is_ok());
    assert!(sink.pin_current(&id(21)?).is_ok());
    assert_eq!(
        sink.pin_current(&id(22)?).err(),
        Some(QueryError::StaleVersion)
    );
    assert_eq!(pin.items().len(), 1);
    let c = sink
        .bind_current_analysis("c", id(3)?)
        .map_err(|e| format!("{e:?}"))?;
    let mut failed = build(&sink, &c, 13)?;
    failed
        .publish_partial(id(23)?)
        .map_err(|e| format!("{e:?}"))?;
    drop(failed);
    assert_eq!(current(&sink)?, Some(id(21)?));
    assert!(sink.pin_current(&id(20)?).is_ok());
    Ok(())
}

#[test]
fn abort_only_reverts_focus_when_its_build_last_changed_it() -> Result<(), String> {
    let sink = IngestSink::default();
    let a = sink
        .bind_current_analysis("a", id(1)?)
        .map_err(|e| format!("{e:?}"))?;
    let b = sink
        .bind_current_analysis("b", id(2)?)
        .map_err(|e| format!("{e:?}"))?;
    let mut a_build = build(&sink, &a, 10)?;
    a_build
        .publish_partial(id(20)?)
        .map_err(|e| format!("{e:?}"))?;
    let mut b_build = build(&sink, &b, 11)?;
    b_build
        .publish_partial(id(21)?)
        .map_err(|e| format!("{e:?}"))?;
    a_build
        .publish_partial(id(22)?)
        .map_err(|e| format!("{e:?}"))?;
    drop(a_build);
    assert_eq!(current(&sink)?, Some(id(21)?));
    drop(b_build);
    assert_eq!(current(&sink)?, None);
    Ok(())
}

#[test]
fn first_failed_build_with_no_baseline_leaves_no_current_result() -> Result<(), String> {
    let sink = IngestSink::default();
    let a = sink
        .bind_current_analysis("a", id(1)?)
        .map_err(|e| format!("{e:?}"))?;
    let mut failed = build(&sink, &a, 10)?;
    let partial = failed
        .publish_partial(id(20)?)
        .map_err(|e| format!("{e:?}"))?;
    drop(failed);
    assert_eq!(current(&sink)?, None);
    assert_eq!(sink.pin_current(&id(20)?).err(), Some(QueryError::NotFound));
    assert_eq!(partial.items().len(), 1);
    Ok(())
}

#[test]
fn failed_build_does_not_steal_focus_from_a_later_other_lineage() -> Result<(), String> {
    let sink = IngestSink::default();
    let a = sink
        .bind_current_analysis("a", id(1)?)
        .map_err(|e| format!("{e:?}"))?;
    let b = sink
        .bind_current_analysis("b", id(2)?)
        .map_err(|e| format!("{e:?}"))?;
    let mut failed = build(&sink, &a, 10)?;
    failed
        .publish_partial(id(20)?)
        .map_err(|e| format!("{e:?}"))?;
    build(&sink, &b, 11)?
        .seal(id(21)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    drop(failed);
    assert_eq!(current(&sink)?, Some(id(21)?));
    assert!(sink.pin_current(&id(21)?).is_ok());
    Ok(())
}

#[test]
fn overlapping_withdrawals_restore_latest_surviving_current() -> Result<(), String> {
    let sink = IngestSink::default();
    let c = sink
        .bind_current_analysis("c", id(3)?)
        .map_err(|e| format!("{e:?}"))?;
    let b = sink
        .bind_current_analysis("b", id(2)?)
        .map_err(|e| format!("{e:?}"))?;
    let a = sink
        .bind_current_analysis("a", id(1)?)
        .map_err(|e| format!("{e:?}"))?;
    build(&sink, &c, 10)?
        .seal(id(20)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let mut b_build = build(&sink, &b, 11)?;
    b_build
        .publish_partial(id(21)?)
        .map_err(|e| format!("{e:?}"))?;
    let mut a_build = build(&sink, &a, 12)?;
    a_build
        .publish_partial(id(22)?)
        .map_err(|e| format!("{e:?}"))?;
    drop(b_build);
    assert_eq!(current(&sink)?, Some(id(22)?));
    drop(a_build);
    assert_eq!(current(&sink)?, Some(id(20)?));
    assert!(sink.pin_current(&id(20)?).is_ok());
    Ok(())
}

#[test]
fn restored_baseline_keeps_its_original_publication_order() -> Result<(), String> {
    let sink = IngestSink::default();
    let a = sink
        .bind_current_analysis("a", id(1)?)
        .map_err(|e| format!("{e:?}"))?;
    let b = sink
        .bind_current_analysis("b", id(2)?)
        .map_err(|e| format!("{e:?}"))?;
    build(&sink, &a, 10)?
        .seal(id(20)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    build(&sink, &b, 11)?
        .seal(id(21)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let mut a_build = build(&sink, &a, 12)?;
    a_build
        .publish_partial(id(22)?)
        .map_err(|e| format!("{e:?}"))?;
    drop(a_build);
    assert_eq!(current(&sink)?, Some(id(21)?));
    assert!(sink.pin_current(&id(20)?).is_ok());
    Ok(())
}

#[test]
fn scan_result_never_reports_a_prior_or_restored_run_as_current() -> Result<(), String> {
    let sink = IngestSink::default();
    let lineage = sink
        .bind_current_analysis("a", id(1)?)
        .map_err(|e| format!("{e:?}"))?;
    let other = sink
        .bind_current_analysis("b", id(2)?)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        sink.scan_result(&lineage, &id(10)?)
            .map_err(|e| format!("{e:?}"))?
            .sequence,
        0
    );
    build(&sink, &lineage, 10)?
        .seal(id(20)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    build(&sink, &other, 12)?
        .seal(id(22)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let prior = sink
        .scan_result(&lineage, &id(11)?)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(prior.sequence, 1);
    assert_eq!(prior.result_version, None);
    let mut failed = build(&sink, &lineage, 11)?;
    failed
        .publish_partial(id(21)?)
        .map_err(|e| format!("{e:?}"))?;
    let partial = sink
        .scan_result(&lineage, &id(11)?)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        (partial.sequence, partial.result_version),
        (2, Some(id(21)?))
    );
    drop(failed);
    let restored = sink
        .scan_result(&lineage, &id(11)?)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!((restored.sequence, restored.result_version), (3, None));
    assert_eq!(
        sink.scan_result(&lineage, &id(10)?)
            .map_err(|e| format!("{e:?}"))?
            .result_version,
        Some(id(20)?)
    );
    assert_eq!(
        sink.scan_result(&other, &id(12)?)
            .map_err(|e| format!("{e:?}"))?
            .result_version,
        Some(id(22)?)
    );
    Ok(())
}
