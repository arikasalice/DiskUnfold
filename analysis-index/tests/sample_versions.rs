use du_analysis_index::{BuildError, BuildHandle, IngestSink, QueryError, SampleSessionKey};
use du_core_contracts::{
    Id, PathBytes, PathEncoding,
    data_source::DataSource,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
    result_events::{ResultVersionChange, ResultVersionNotice, ResultVersionPublisher},
};
use std::sync::{Arc, Mutex};

type Check = Result<(), String>;

fn id(n: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn debug<E: std::fmt::Debug>(error: E) -> String {
    format!("{error:?}")
}
fn root() -> ResultScopeRoot {
    ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::WindowsUtf16le,
            raw: PathBytes::from_bytes(&[67, 0]),
        },
        volume_id: vec![1],
        file_id: vec![1],
    }
}
fn fill(mut build: BuildHandle) -> Result<BuildHandle, String> {
    build.add_directory(Vec::new()).map_err(debug)?;
    build.add_directory(vec![b"a".to_vec()]).map_err(debug)?;
    Ok(build)
}
fn sample(
    sink: &IngestSink,
    session: &SampleSessionKey,
    scope: u32,
) -> Result<BuildHandle, String> {
    fill(
        sink.open_sample_forest(id(scope)?, vec![root()], session)
            .map_err(debug)?,
    )
}
fn real(sink: &IngestSink, scope: u32) -> Result<BuildHandle, String> {
    // Only a bound lineage is a scan source that also publishes notices.
    let lineage = sink
        .bind_current_analysis(&format!("real-{scope}"), id(1000 + scope)?)
        .map_err(debug)?;
    fill(
        sink.open_forest_in_lineage(id(scope)?, vec![root()], lineage)
            .map_err(debug)?,
    )
}
fn key(n: u32) -> Result<SampleSessionKey, String> {
    Ok(SampleSessionKey::new(id(n)?))
}
fn is_missing(error: &QueryError) -> bool {
    matches!(error, QueryError::NotFound | QueryError::StaleVersion)
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
fn two_sample_builds_of_one_session_stay_current_together() -> Check {
    let sink = IngestSink::default();
    let session = key(1)?;
    let first = sample(&sink, &session, 10)?
        .seal(id(20)?, ResultCompleteness::Complete)
        .map_err(debug)?;
    let second = sample(&sink, &session, 11)?
        .seal(id(21)?, ResultCompleteness::Complete)
        .map_err(debug)?;
    for (pinned, expected) in [(&first, id(20)?), (&second, id(21)?)] {
        let current = sink.pin_current(&expected).map_err(debug)?;
        assert!(Arc::ptr_eq(&current, pinned));
        assert_eq!(current.data_source(), DataSource::Sample);
    }
    assert_ne!(first.lineage_id(), second.lineage_id());
    Ok(())
}

#[test]
fn implicit_selection_ignores_samples_but_explicit_ids_reach_them() -> Check {
    let sink = IngestSink::default();
    let session = key(1)?;
    let hidden = sample(&sink, &session, 10)?
        .seal(id(20)?, ResultCompleteness::Complete)
        .map_err(debug)?;
    assert!(sink.pin().map_err(debug)?.is_none());
    assert_eq!(sink.pin_scan(None, None).err(), Some(QueryError::NotFound));
    let by_revision = sink
        .pin_scan(None, Some(hidden.version_id()))
        .map_err(debug)?;
    assert!(Arc::ptr_eq(&by_revision, &hidden));

    let actual = real(&sink, 11)?
        .seal(id(21)?, ResultCompleteness::Complete)
        .map_err(debug)?;
    // A sample published after the real result must still not become the implicit pick.
    sample(&sink, &session, 12)?
        .seal(id(22)?, ResultCompleteness::Complete)
        .map_err(debug)?;
    let picked = sink.pin().map_err(debug)?.ok_or("no real result")?;
    assert!(Arc::ptr_eq(&picked, &actual));
    let scanned = sink.pin_scan(None, None).map_err(debug)?;
    assert!(Arc::ptr_eq(&scanned, &actual));
    assert_eq!(actual.data_source(), DataSource::Real);
    Ok(())
}

#[test]
fn release_counts_is_idempotent_and_keeps_pinned_readers() -> Check {
    let sink = IngestSink::default();
    let (one, other) = (key(1)?, key(2)?);
    let held = sample(&sink, &one, 10)?
        .seal(id(20)?, ResultCompleteness::Complete)
        .map_err(debug)?;
    sample(&sink, &one, 11)?
        .seal(id(21)?, ResultCompleteness::Complete)
        .map_err(debug)?;
    sample(&sink, &other, 12)?
        .seal(id(22)?, ResultCompleteness::Complete)
        .map_err(debug)?;
    let actual = real(&sink, 13)?
        .seal(id(23)?, ResultCompleteness::Complete)
        .map_err(debug)?;
    let pinned = sink.pin_current(&id(20)?).map_err(debug)?;

    assert_eq!(sink.release_sample_session(&one).map_err(debug)?, 2);
    assert_eq!(sink.release_sample_session(&one).map_err(debug)?, 0);
    assert_eq!(sink.release_sample_session(&key(99)?).map_err(debug)?, 0);

    for released in [20, 21] {
        assert!(
            sink.pin_current(&id(released)?)
                .err()
                .is_some_and(|e| is_missing(&e))
        );
    }
    assert_eq!(pinned.items().len(), held.items().len());
    assert!(pinned.items().len() >= 2);
    sink.pin_current(&id(22)?).map_err(debug)?;
    sink.pin_current(actual.version_id()).map_err(debug)?;
    assert_eq!(sink.release_sample_session(&other).map_err(debug)?, 1);
    assert_eq!(sink.release_sample_session(&other).map_err(debug)?, 0);
    Ok(())
}

#[test]
fn release_stops_an_open_build_from_publishing_again() -> Check {
    let sink = IngestSink::default();
    let session = key(1)?;
    let mut running = sample(&sink, &session, 10)?;
    running.publish_partial(id(20)?).map_err(debug)?;
    let mut unpublished = sample(&sink, &session, 11)?;
    assert_eq!(sink.release_sample_session(&session).map_err(debug)?, 1);

    let late = running.publish_partial(id(21)?);
    assert!(late.is_err(), "released build published: {late:?}");
    assert!(unpublished.publish_partial(id(22)?).is_err());
    for gone in [20, 21, 22] {
        assert!(
            sink.pin_current(&id(gone)?)
                .err()
                .is_some_and(|e| is_missing(&e))
        );
    }
    // A new build under the same key starts a fresh lineage and publishes normally.
    sample(&sink, &session, 12)?
        .seal(id(23)?, ResultCompleteness::Complete)
        .map_err(debug)?;
    sink.pin_current(&id(23)?).map_err(debug)?;

    let sealed = sample(&sink, &session, 13)?;
    assert_eq!(sink.release_sample_session(&session).map_err(debug)?, 1);
    assert!(matches!(
        sealed.seal(id(31)?, ResultCompleteness::Complete),
        Err(BuildError::Unavailable)
    ));
    assert!(sink.pin_current(&id(31)?).is_err());
    Ok(())
}

#[test]
fn invalid_roots_are_rejected_like_open_forest() -> Check {
    let sink = IngestSink::default();
    let result = sink.open_sample_forest(id(1)?, Vec::new(), &key(1)?);
    assert_eq!(result.err(), Some(BuildError::InvalidTree));
    Ok(())
}

#[test]
fn sample_publication_sends_no_notice_while_real_publication_does() -> Check {
    let notices = Arc::new(Notices::default());
    let sink = IngestSink::with_version_publisher(notices.clone());
    let session = key(1)?;
    let mut partial = sample(&sink, &session, 10)?;
    partial.publish_partial(id(20)?).map_err(debug)?;
    partial
        .seal(id(21)?, ResultCompleteness::Complete)
        .map_err(debug)?;
    let mut abandoned = sample(&sink, &session, 11)?;
    abandoned.publish_partial(id(22)?).map_err(debug)?;
    drop(abandoned);
    sink.release_sample_session(&session).map_err(debug)?;
    assert!(notices.0.lock().map_err(|_| "poisoned")?.is_empty());

    real(&sink, 12)?
        .seal(id(23)?, ResultCompleteness::Complete)
        .map_err(debug)?;
    let notices = notices.0.lock().map_err(|_| "poisoned")?;
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].body.change, ResultVersionChange::Published);
    assert_eq!(notices[0].body.data_source, Some(DataSource::Real));
    Ok(())
}
