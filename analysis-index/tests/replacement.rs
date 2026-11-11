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
        .map_err(|error| format!("{error:?}"))?;
    build
        .add_directory(Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    Ok(build)
}

#[derive(Default)]
struct Notices(Mutex<Vec<ResultVersionNotice>>);

impl ResultVersionPublisher for Notices {
    fn publish_result_version(&self, notice: ResultVersionNotice) {
        if let Ok(mut notices) = self.0.lock() {
            notices.push(notice);
        }
    }
}

#[test]
fn replacement_keeps_old_pins_and_publishes_once_after_seal() -> Result<(), String> {
    let notices = Arc::new(Notices::default());
    let sink = IngestSink::with_version_publisher(notices.clone());
    let lineage = sink
        .bind_current_analysis("replacement-success", id(40)?)
        .map_err(|error| format!("{error:?}"))?;
    let old = build(&sink, &lineage, 41)?
        .seal(id(42)?, ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;

    let mut candidate = sink
        .open_replacement(&old)
        .map_err(|error| format!("{error:?}"))?;
    candidate
        .add_directory(Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    assert!(Arc::ptr_eq(
        &sink
            .pin_current(&id(42)?)
            .map_err(|error| format!("{error:?}"))?,
        &old
    ));
    assert_eq!(
        notices.0.lock().map_err(|_| "notice lock poisoned")?.len(),
        1
    );

    let new = candidate
        .seal(id(43)?, ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    assert!(Arc::ptr_eq(
        &sink
            .pin_current(&id(43)?)
            .map_err(|error| format!("{error:?}"))?,
        &new
    ));
    assert_eq!(old.version_id(), &id(42)?);
    assert_eq!(old.origin(), "scan");
    assert_eq!(old.items().len(), 1);
    assert_eq!(new.version_id(), &id(43)?);
    assert_eq!(new.origin(), "incremental");
    let notices = notices.0.lock().map_err(|_| "notice lock poisoned")?;
    assert_eq!(notices.len(), 2);
    assert_eq!(notices[1].body.change, ResultVersionChange::Published);
    assert_eq!(notices[1].body.origin, "incremental");
    assert_eq!(notices[1].body.source_ref, lineage);
    assert_eq!(notices[1].body.previous_result_version, Some(id(42)?));
    assert_eq!(notices[1].result_version, Some(id(43)?));
    Ok(())
}

#[test]
fn withdrawing_scan_partial_reports_the_restored_incremental_origin() -> Result<(), String> {
    let notices = Arc::new(Notices::default());
    let sink = IngestSink::with_version_publisher(notices.clone());
    let lineage = sink
        .bind_current_analysis("restore-incremental", id(100)?)
        .map_err(|error| format!("{error:?}"))?;
    let base = build(&sink, &lineage, 101)?
        .seal(id(102)?, ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    let mut candidate = sink
        .open_replacement(&base)
        .map_err(|error| format!("{error:?}"))?;
    candidate
        .add_directory(Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    let incremental = candidate
        .seal(id(103)?, ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    let mut scan = build(&sink, &lineage, 104)?;
    scan.publish_partial(id(105)?)
        .map_err(|error| format!("{error:?}"))?;
    scan.abort();

    let current = sink
        .pin_current(incremental.version_id())
        .map_err(|error| format!("{error:?}"))?;
    assert!(Arc::ptr_eq(&current, &incremental));
    let events = notices.0.lock().map_err(|_| "notice lock poisoned")?;
    assert_eq!(events.len(), 4);
    assert_eq!(events[2].body.origin, "scan");
    assert_eq!(events[3].body.change, ResultVersionChange::Withdrawn);
    assert_eq!(events[3].body.origin, "incremental");
    assert_eq!(events[3].body.previous_result_version, Some(id(105)?));
    assert_eq!(events[3].result_version, Some(id(103)?));
    assert!(events[3].validate());
    Ok(())
}

#[test]
fn superseded_replacement_is_discarded_without_publication() -> Result<(), String> {
    let notices = Arc::new(Notices::default());
    let sink = IngestSink::with_version_publisher(notices.clone());
    let lineage = sink
        .bind_current_analysis("replacement-race", id(50)?)
        .map_err(|error| format!("{error:?}"))?;
    let old = build(&sink, &lineage, 51)?
        .seal(id(52)?, ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    let mut candidate = sink
        .open_replacement(&old)
        .map_err(|error| format!("{error:?}"))?;
    candidate
        .add_directory(Vec::new())
        .map_err(|error| format!("{error:?}"))?;

    build(&sink, &lineage, 53)?
        .seal(id(54)?, ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    assert!(matches!(
        candidate.seal(id(55)?, ResultCompleteness::Complete),
        Err(du_analysis_index::BuildError::BaseSuperseded)
    ));
    assert_eq!(old.version_id(), &id(52)?);
    assert_eq!(old.items().len(), 1);
    assert_eq!(
        sink.pin_current(&id(54)?)
            .map_err(|error| format!("{error:?}"))?
            .version_id(),
        &id(54)?
    );
    assert_eq!(
        sink.pin_current(&id(55)?).err(),
        Some(QueryError::StaleVersion)
    );
    let notices = notices.0.lock().map_err(|_| "notice lock poisoned")?;
    assert_eq!(notices.len(), 2);
    assert_eq!(notices[1].body.previous_result_version, Some(id(52)?));
    assert_eq!(notices[1].body.origin, "scan");
    assert_eq!(notices[1].result_version, Some(id(54)?));
    Ok(())
}

#[test]
fn failed_replacement_does_not_publish_or_withdraw_the_base() -> Result<(), String> {
    let notices = Arc::new(Notices::default());
    let sink = IngestSink::with_version_publisher(notices.clone());
    let lineage = sink
        .bind_current_analysis("replacement-failure", id(60)?)
        .map_err(|error| format!("{error:?}"))?;
    let old = build(&sink, &lineage, 61)?
        .seal(id(62)?, ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    let mut candidate = sink
        .open_replacement(&old)
        .map_err(|error| format!("{error:?}"))?;
    candidate
        .add_directory(Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        candidate.add_directory(Vec::new()),
        Err(du_analysis_index::BuildError::InvalidTree)
    );

    assert!(Arc::ptr_eq(
        &sink
            .pin_current(&id(62)?)
            .map_err(|error| format!("{error:?}"))?,
        &old
    ));
    assert_eq!(
        notices.0.lock().map_err(|_| "notice lock poisoned")?.len(),
        1
    );
    Ok(())
}

#[test]
fn active_scan_blocks_replacement_until_scan_finishes() -> Result<(), String> {
    let notices = Arc::new(Notices::default());
    let sink = IngestSink::with_version_publisher(notices.clone());
    let lineage = sink
        .bind_current_analysis("replacement-active-scan", id(70)?)
        .map_err(|error| format!("{error:?}"))?;
    let base = build(&sink, &lineage, 71)?
        .seal(id(72)?, ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;

    let mut scan = build(&sink, &lineage, 73)?;
    scan.publish_partial(id(74)?)
        .map_err(|error| format!("{error:?}"))?;
    assert!(matches!(
        sink.open_replacement(&base),
        Err(du_analysis_index::BuildError::Busy)
    ));

    scan.abort();
    assert!(Arc::ptr_eq(
        &sink
            .pin_current(&id(72)?)
            .map_err(|error| format!("{error:?}"))?,
        &base
    ));
    assert_eq!(
        sink.pin_current(&id(74)?).err(),
        Some(QueryError::StaleVersion)
    );
    let notices = notices.0.lock().map_err(|_| "notice lock poisoned")?;
    assert_eq!(notices.len(), 3);
    assert_eq!(notices[1].body.change, ResultVersionChange::Published);
    assert_eq!(notices[1].result_version, Some(id(74)?));
    assert_eq!(notices[2].body.change, ResultVersionChange::Withdrawn);
    assert_eq!(notices[2].result_version, Some(id(72)?));
    Ok(())
}

#[test]
fn scan_started_after_replacement_open_blocks_replacement_seal() -> Result<(), String> {
    let notices = Arc::new(Notices::default());
    let sink = IngestSink::with_version_publisher(notices.clone());
    let lineage = sink
        .bind_current_analysis("replacement-seal-active-scan", id(75)?)
        .map_err(|error| format!("{error:?}"))?;
    let base = build(&sink, &lineage, 76)?
        .seal(id(77)?, ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    let mut candidate = sink
        .open_replacement(&base)
        .map_err(|error| format!("{error:?}"))?;
    candidate
        .add_directory(Vec::new())
        .map_err(|error| format!("{error:?}"))?;

    let scan = build(&sink, &lineage, 78)?;
    assert!(matches!(
        candidate.seal(id(80)?, ResultCompleteness::Complete),
        Err(du_analysis_index::BuildError::Busy)
    ));
    let final_scan = scan
        .seal(id(81)?, ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    assert!(Arc::ptr_eq(
        &sink
            .pin_current(&id(81)?)
            .map_err(|error| format!("{error:?}"))?,
        &final_scan
    ));
    assert_eq!(
        sink.pin_current(&id(80)?).err(),
        Some(QueryError::StaleVersion)
    );
    let notices = notices.0.lock().map_err(|_| "notice lock poisoned")?;
    assert_eq!(notices.len(), 2);
    assert_eq!(notices[1].result_version, Some(id(81)?));
    Ok(())
}

#[test]
fn concurrent_replacements_publish_one_winner_and_no_failed_notice() -> Result<(), String> {
    let notices = Arc::new(Notices::default());
    let sink = IngestSink::with_version_publisher(notices.clone());
    let lineage = sink
        .bind_current_analysis("replacement-seal-race", id(80)?)
        .map_err(|error| format!("{error:?}"))?;
    let base = build(&sink, &lineage, 81)?
        .seal(id(82)?, ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    let mut first = sink
        .open_replacement(&base)
        .map_err(|error| format!("{error:?}"))?;
    let mut second = sink
        .open_replacement(&base)
        .map_err(|error| format!("{error:?}"))?;
    first
        .add_directory(Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    second
        .add_directory(Vec::new())
        .map_err(|error| format!("{error:?}"))?;

    let start = Arc::new(std::sync::Barrier::new(3));
    let first_start = Arc::clone(&start);
    let first_version = id(83)?;
    let first_join = std::thread::spawn(move || {
        first_start.wait();
        first.seal(first_version, ResultCompleteness::Complete)
    });
    let second_start = Arc::clone(&start);
    let second_version = id(84)?;
    let second_join = std::thread::spawn(move || {
        second_start.wait();
        second.seal(second_version, ResultCompleteness::Complete)
    });
    start.wait();
    let outcomes = [
        first_join
            .join()
            .map_err(|_| "first replacement thread panicked")?,
        second_join
            .join()
            .map_err(|_| "second replacement thread panicked")?,
    ];
    let winners: Vec<_> = outcomes
        .iter()
        .filter_map(|outcome| outcome.as_ref().ok())
        .collect();
    let failures: Vec<_> = outcomes.iter().filter(|outcome| outcome.is_err()).collect();
    assert_eq!(winners.len(), 1);
    assert_eq!(failures.len(), 1);
    assert!(matches!(
        failures[0],
        Err(du_analysis_index::BuildError::BaseSuperseded)
    ));
    let current = sink
        .pin()
        .map_err(|error| format!("{error:?}"))?
        .ok_or("replacement winner was not current")?;
    assert_eq!(current.version_id(), winners[0].version_id());

    let notices = notices.0.lock().map_err(|_| "notice lock poisoned")?;
    assert_eq!(notices.len(), 2);
    assert_eq!(notices[1].body.change, ResultVersionChange::Published);
    assert_eq!(notices[1].body.previous_result_version, Some(id(82)?));
    assert_eq!(
        notices[1].result_version,
        Some(current.version_id().clone())
    );
    Ok(())
}
