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
