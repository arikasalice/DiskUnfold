use super::*;
use du_analysis_index::{SampleSessionKey, ScopeHeader};
use du_core_contracts::{
    PathBytes, PathEncoding, Timestamp,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
    scope_observation_log::ScopeObservation,
};
use std::sync::Mutex;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const HEX: &str = "00ff10abcdef0123456789abcdef0123456789abcdef0123456789abcdef9a5c";

#[derive(Default)]
struct Log {
    rows: Mutex<Vec<ScopeObservation>>,
    fail: Mutex<Option<ScopeObservationWriteError>>,
    calls: Mutex<u32>,
}

impl Log {
    fn rows(&self) -> Vec<ScopeObservation> {
        self.rows
            .lock()
            .map(|rows| rows.clone())
            .unwrap_or_default()
    }
    fn calls(&self) -> u32 {
        self.calls.lock().map(|calls| *calls).unwrap_or(u32::MAX)
    }
    fn failing(error: ScopeObservationWriteError) -> Self {
        let log = Self::default();
        if let Ok(mut fail) = log.fail.lock() {
            *fail = Some(error);
        }
        log
    }
}

impl ScopeObservationLog for Log {
    fn append(
        &self,
        observation: &ScopeObservation,
    ) -> Result<ScopeObservationAppend, ScopeObservationWriteError> {
        if let Ok(mut calls) = self.calls.lock() {
            *calls += 1;
        }
        if let Some(error) = self.fail.lock().ok().and_then(|fail| *fail) {
            return Err(error);
        }
        let mut rows = self
            .rows
            .lock()
            .map_err(|_| ScopeObservationWriteError::Storage)?;
        if rows.iter().any(|row| row.job_id == observation.job_id) {
            return Ok(ScopeObservationAppend::AlreadyRecorded);
        }
        rows.push(observation.clone());
        Ok(ScopeObservationAppend::Added)
    }
}

fn id(n: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
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

fn summary(
    job: u32,
    kind: &str,
    state: JobState,
    version: Option<Id>,
) -> Result<JobSummary, String> {
    Ok(JobSummary {
        job_id: id(job)?,
        kind_id: kind.into(),
        kind_version: 1,
        scope_summary: "metadata: 1 root".into(),
        state,
        counts: Default::default(),
        failure_reason_key: None,
        unprocessed_summary: None,
        result_version_id: version,
        started_at_ms: Some(1),
        finished_at_ms: 2,
        correlation_id: id(job)?,
    })
}

/// Seals one scan result in `sink` and returns its version.
fn seal(
    sink: &IngestSink,
    fingerprint: &str,
    first_id: u32,
    observed_at: Option<i64>,
) -> Result<Id, String> {
    let mut build = sink
        .open_scoped_forest(
            ScopeHeader {
                version_id: id(first_id)?,
                fingerprint: fingerprint.into(),
            },
            vec![root()],
            None,
        )
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    let version = build
        .seal_at(
            id(first_id + 1)?,
            ResultCompleteness::Complete,
            observed_at.and_then(Timestamp::from_unix_millis),
        )
        .map_err(|e| format!("{e:?}"))?;
    Ok(version.version_id().clone())
}

fn run(sink: &IngestSink, log: &Log, summary: &JobSummary) -> Result<Outcome, String> {
    Ok(record(sink, log, summary, id(500)?, 42))
}

#[test]
fn a_done_and_a_partial_scan_are_each_recorded_once() -> TestResult {
    for (job, state) in [(1, JobState::Done), (2, JobState::Partial)] {
        let sink = IngestSink::default();
        let log = Log::default();
        let version = seal(&sink, HEX, 10, Some(9_000))?;
        let finished = summary(job, SCAN_KIND, state, Some(version))?;
        assert_eq!(run(&sink, &log, &finished)?, Outcome::Recorded);
        let rows = log.rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].job_id, id(job)?);
        assert_eq!(rows[0].observation_id, id(500)?);
        assert_eq!(rows[0].observed_at_ms, 9_000);
        assert_eq!(rows[0].created_at_ms, 42);
        // Delivering the same end again is a success that adds nothing.
        assert_eq!(run(&sink, &log, &finished)?, Outcome::AlreadyRecorded);
        assert_eq!(log.rows().len(), 1);
    }
    Ok(())
}

#[test]
fn the_five_cases_that_leave_no_observation() -> TestResult {
    // 1. No sealed result: a failed or cancelled job, a job without a result, an interim one.
    let sink = IngestSink::default();
    let log = Log::default();
    let version = seal(&sink, HEX, 10, Some(9_000))?;
    for state in [JobState::Failed, JobState::Cancelled] {
        let ended = summary(1, SCAN_KIND, state, Some(version.clone()))?;
        assert_eq!(
            run(&sink, &log, &ended)?,
            Outcome::Skipped(Skip::NoSealedResult)
        );
    }
    let none = summary(1, SCAN_KIND, JobState::Done, None)?;
    assert_eq!(
        run(&sink, &log, &none)?,
        Outcome::Skipped(Skip::NoSealedResult)
    );
    let interim_sink = IngestSink::default();
    let mut build = interim_sink
        .open_scoped_forest(
            ScopeHeader {
                version_id: id(20)?,
                fingerprint: HEX.into(),
            },
            vec![root()],
            None,
        )
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    let interim = build
        .publish_partial(id(21)?)
        .map_err(|e| format!("{e:?}"))?;
    let unsealed = summary(
        2,
        SCAN_KIND,
        JobState::Partial,
        Some(interim.version_id().clone()),
    )?;
    assert_eq!(
        run(&interim_sink, &log, &unsealed)?,
        Outcome::Skipped(Skip::NoSealedResult)
    );

    // 2. A withdrawn result.
    let lineage_sink = IngestSink::default();
    let lineage = lineage_sink
        .bind_current_analysis(HEX, id(30)?)
        .map_err(|e| format!("{e:?}"))?;
    let mut withdrawn = lineage_sink
        .open_scoped_forest(
            ScopeHeader {
                version_id: id(31)?,
                fingerprint: HEX.into(),
            },
            vec![root()],
            Some(lineage),
        )
        .map_err(|e| format!("{e:?}"))?;
    withdrawn
        .add_directory_at(0, Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    let candidate = withdrawn
        .publish_partial(id(32)?)
        .map_err(|e| format!("{e:?}"))?;
    withdrawn.abort();
    let gone = summary(
        3,
        SCAN_KIND,
        JobState::Done,
        Some(candidate.version_id().clone()),
    )?;
    assert_eq!(
        run(&lineage_sink, &log, &gone)?,
        Outcome::Skipped(Skip::Withdrawn)
    );

    // 3. A sample source, by job kind and by the result's own data source.
    let sample = summary(4, "scan.sample", JobState::Done, Some(version.clone()))?;
    assert_eq!(run(&sink, &log, &sample)?, Outcome::Skipped(Skip::NotScan));
    let sample_sink = IngestSink::default();
    let mut sample_build = sample_sink
        .open_sample_forest(id(40)?, vec![root()], &SampleSessionKey::new(id(41)?))
        .map_err(|e| format!("{e:?}"))?;
    sample_build
        .add_directory_at(0, Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    let sample_result = sample_build
        .seal_at(
            id(42)?,
            ResultCompleteness::Complete,
            Timestamp::from_unix_millis(1),
        )
        .map_err(|e| format!("{e:?}"))?;
    let loaded = summary(
        5,
        SCAN_KIND,
        JobState::Done,
        Some(sample_result.version_id().clone()),
    )?;
    assert_eq!(
        run(&sample_sink, &log, &loaded)?,
        Outcome::Skipped(Skip::SampleSource)
    );

    // 4. No storage is bound for the CLI, so `ScanEvents` never reaches this function; see
    //    `scan_events_without_a_log_record_nothing`.
    // 5. No observation time; the job's own end time is never used instead.
    let untimed_sink = IngestSink::default();
    let untimed_version = seal(&untimed_sink, HEX, 50, None)?;
    let untimed = summary(6, SCAN_KIND, JobState::Done, Some(untimed_version))?;
    assert_eq!(
        run(&untimed_sink, &log, &untimed)?,
        Outcome::Skipped(Skip::NoObservedAt)
    );
    assert_eq!((log.rows().len(), log.calls()), (0, 0));
    Ok(())
}

#[test]
fn an_unconvertible_fingerprint_is_reported_and_nothing_is_stored() -> TestResult {
    for (n, bad) in [
        HEX.to_uppercase(),
        HEX[..60].to_owned(),
        "not-a-fingerprint".into(),
    ]
    .into_iter()
    .enumerate()
    {
        let sink = IngestSink::default();
        let log = Log::default();
        let version = seal(&sink, &bad, 10 + 10 * n as u32, Some(5))?;
        let finished = summary(1, SCAN_KIND, JobState::Done, Some(version))?;
        assert_eq!(
            run(&sink, &log, &finished)?,
            Outcome::Failed(Failure::Conversion(ObservationSkip::MalformedFingerprint))
        );
        assert_eq!(log.calls(), 0);
    }
    Ok(())
}

#[test]
fn a_storage_failure_is_reported_with_its_reason_and_stores_nothing() -> TestResult {
    for (error, reason) in [
        (ScopeObservationWriteError::Busy, "db.busy"),
        (ScopeObservationWriteError::DiskFull, "db.disk_full"),
        (ScopeObservationWriteError::Invalid, "value_not_storable"),
        (ScopeObservationWriteError::Storage, "storage_failed"),
    ] {
        let sink = IngestSink::default();
        let log = Log::failing(error);
        let version = seal(&sink, HEX, 10, Some(5))?;
        let finished = summary(1, SCAN_KIND, JobState::Done, Some(version))?;
        let outcome = run(&sink, &log, &finished)?;
        assert_eq!(outcome, Outcome::Failed(Failure::Store(error)));
        assert!(matches!(outcome, Outcome::Failed(failure) if failure.reason() == reason));
        assert!(log.rows().is_empty());
    }
    Ok(())
}
