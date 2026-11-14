//! The sample load job through the result store and the job runtime.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};

use du_analysis_index::{IngestSink, SampleSessionKey};
use du_core_contracts::{
    Id, Timestamp,
    data_source::DataSource,
    job::{
        Checkpoint, HistoryAppend, JobBody, JobContext, JobFlow, JobFlowError, JobHistory,
        JobState, JobSummary,
    },
    result::{ResultCompleteness, ResultVersionRead},
    sample_ipc::SAMPLE_LOAD_FAILED,
};
use du_job_runtime::{
    JobClock, JobEvent, JobEvents, JobIdentity, JobPriority, JobRuntime, Submission, SubmitResult,
};
use du_scan_engine::{
    sample_bundle::{SampleObservationState, ValidatedBundle, embedded::EmbeddedSampleBundle},
    sample_job::{
        SAMPLE_JOB_KIND, SampleLoad, SampleLoadEnd, SampleLoadReport, SampleLoadTarget, descriptor,
    },
    scan::ResultIds,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn id(n: u64) -> Id {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}")).unwrap_or_else(|_| unreachable!())
}

/// Issues distinct identifiers and remembers every one it handed out.
#[derive(Default)]
struct Ids {
    next: AtomicU64,
    issued: Mutex<Vec<Id>>,
}

impl ResultIds for Ids {
    fn next_id(&self) -> Id {
        let value = id(100 + self.next.fetch_add(1, Ordering::SeqCst));
        if let Ok(mut issued) = self.issued.lock() {
            issued.push(value.clone());
        }
        value
    }
}

/// Runtime context stand-in: a wall clock far from the bundle's times, optional cancellation
/// after the first sealed point, and an optional session release at a chosen checkpoint poll
/// after the first sealed point (poll 0 falls between the two points, poll 2 inside the second).
struct Context {
    report: Arc<SampleLoadReport>,
    sink: IngestSink,
    session: SampleSessionKey,
    cancel_after_first: bool,
    release_at_poll: Option<usize>,
    polls_after_first: AtomicUsize,
}

impl JobContext for Context {
    fn checkpoint(&self) -> Checkpoint {
        let sealed = self.report.observations().len();
        if sealed >= 1 && self.cancel_after_first {
            return Checkpoint::Cancel;
        }
        if sealed == 1
            && let Some(poll) = self.release_at_poll
            && self.polls_after_first.fetch_add(1, Ordering::SeqCst) == poll
        {
            let _ = self.sink.release_sample_session(&self.session);
        }
        Checkpoint::Continue
    }

    fn utc_now(&self) -> Option<Timestamp> {
        Timestamp::from_unix_millis(4_000_000_000_000)
    }

    fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
        // The sample job must never ask the runtime for an I/O permit.
        Err(JobFlowError::UnsupportedResource)
    }
}

struct Run {
    sink: IngestSink,
    session: SampleSessionKey,
    ids: Arc<Ids>,
    report: Arc<SampleLoadReport>,
    bundle: Arc<ValidatedBundle>,
    job: SampleLoad,
}

fn setup() -> Result<Run, Box<dyn std::error::Error>> {
    let bundle = Arc::new(EmbeddedSampleBundle::validate().map_err(|e| format!("{e:?}"))?);
    let sink = IngestSink::default();
    let session = SampleSessionKey::new(id(1));
    let ids = Arc::new(Ids::default());
    let report = Arc::new(SampleLoadReport::default());
    let job = SampleLoad::new(
        Arc::clone(&bundle),
        SampleLoadTarget {
            sink: sink.clone(),
            session: session.clone(),
            scope_version_id: id(2),
            ids: ids.clone(),
        },
        Arc::clone(&report),
    );
    Ok(Run {
        sink,
        session,
        ids,
        report,
        bundle,
        job,
    })
}

fn context(run: &Run, cancel: bool, release_at_poll: Option<usize>) -> Arc<dyn JobContext> {
    Arc::new(Context {
        report: Arc::clone(&run.report),
        sink: run.sink.clone(),
        session: run.session.clone(),
        cancel_after_first: cancel,
        release_at_poll,
        polls_after_first: AtomicUsize::new(0),
    })
}

/// The gap examples of one observation point, which the collector counts as failed.
fn gaps(run: &Run, position: usize) -> u64 {
    run.bundle.observations()[position]
        .items
        .iter()
        .filter(|item| {
            matches!(
                item.state,
                Some(
                    SampleObservationState::AccessDenied
                        | SampleObservationState::Vanished
                        | SampleObservationState::SizeUnknown
                )
            )
        })
        .count() as u64
}

#[test]
fn both_observations_are_published_as_sample_versions_in_order() -> TestResult {
    let run = setup()?;
    let outcome = run.job.run(context(&run, false, None));
    assert!(outcome.completed_scope && outcome.failure_reason_key.is_none());
    assert_eq!(run.report.end(), Some(SampleLoadEnd::Done));
    let sealed = run.report.observations();
    assert_eq!(sealed.len(), 2);
    for (position, (index, version_id, observed_at)) in sealed.iter().enumerate() {
        assert_eq!(*index as usize, position);
        let expected = &run.bundle.observations()[position];
        let version = run
            .sink
            .pin_current(version_id)
            .map_err(|e| format!("{e:?}"))?;
        assert_eq!(version.data_source(), DataSource::Sample);
        // Both points carry the bundle's gap examples, so the collector seals them Missing.
        assert_eq!(version.completeness(), ResultCompleteness::Missing);
        assert_eq!(version.observed_at(), Some(&expected.observed_at));
        assert_eq!(observed_at.as_ref(), Some(&expected.observed_at));
        // One entry per bundle item plus the scope root directory.
        assert_eq!(version.items().len(), expected.items.len() + 1);
    }
    assert_ne!(sealed[0].1, sealed[1].1);
    // The collector counts each bundle item and each point's scope root once,
    // so the total adds one root per observation point.
    let declared = run.bundle.header().declared_item_count;
    let roots = run.bundle.observation_count() as u64;
    let (done, total) = run.report.progress();
    assert_eq!((done, total), (declared + roots, declared + roots));
    assert_eq!(outcome.counts.completed + outcome.counts.failed, total);
    assert_eq!(outcome.counts.unprocessed, 0);
    // A sample version never becomes the implicit current result.
    assert!(run.sink.pin().map_err(|e| format!("{e:?}"))?.is_none());
    Ok(())
}

#[test]
fn cancelling_after_the_first_point_leaves_no_version() -> TestResult {
    let run = setup()?;
    let outcome = run.job.run(context(&run, true, None));
    assert!(!outcome.completed_scope && outcome.failure_reason_key.is_none());
    assert_eq!(run.report.end(), Some(SampleLoadEnd::Cancelled));
    assert!(run.report.observations().is_empty());
    // Only the first point was collected: its items and its root. No version
    // stays reachable, so nothing is completed (NFR5.HD1.app-host.DD1): the
    // first point's gap examples stay failed and every other unit of both
    // points, roots included, is unprocessed.
    let points = run.bundle.observations();
    let first = points[0].items.len() as u64 + 1;
    let second = points[1].items.len() as u64 + 1;
    let counts = &outcome.counts;
    assert_eq!(counts.completed, 0);
    assert_eq!(counts.failed, gaps(&run, 0));
    assert_eq!(counts.unprocessed, first + second - counts.failed);
    assert_eq!(run.job.progress_total(), first + second);
    assert_eq!(
        outcome.unprocessed_summary.as_deref(),
        Some(format!("unprocessed_items={}", counts.unprocessed).as_str())
    );
    let issued = run.ids.issued.lock().map_err(|_| "poisoned")?.clone();
    assert!(!issued.is_empty());
    for version_id in issued {
        assert!(run.sink.pin_current(&version_id).is_err());
    }
    // The job already released the session, so nothing is left to release.
    assert_eq!(
        run.sink
            .release_sample_session(&run.session)
            .map_err(|e| format!("{e:?}"))?,
        0
    );
    Ok(())
}

#[test]
fn a_failing_point_discards_the_whole_session() -> TestResult {
    let run = setup()?;
    let outcome = run.job.run(context(&run, false, Some(2)));
    assert_eq!(
        outcome.failure_reason_key.as_deref(),
        Some(SAMPLE_LOAD_FAILED)
    );
    assert!(!outcome.completed_scope);
    assert_eq!(run.report.end(), Some(SampleLoadEnd::Failed));
    assert!(run.report.observations().is_empty());
    let issued = run.ids.issued.lock().map_err(|_| "poisoned")?.clone();
    // The second point failed after its root was admitted. No version stays
    // reachable, so nothing is completed; the gap examples met before the
    // failure stay failed and every other unit of the total is unprocessed.
    let total = run.job.progress_total();
    let failed = outcome.counts.failed;
    assert_eq!(outcome.counts.completed, 0);
    assert!(
        failed >= gaps(&run, 0) && failed <= gaps(&run, 0) + gaps(&run, 1),
        "failed={failed}"
    );
    assert_eq!(outcome.counts.unprocessed, total - failed);
    for version_id in issued {
        assert!(run.sink.pin_current(&version_id).is_err());
    }
    assert_eq!(
        run.sink
            .release_sample_session(&run.session)
            .map_err(|e| format!("{e:?}"))?,
        0
    );
    Ok(())
}

/// A close that lands between two observation points releases the session before the load
/// registers the next lineage. The store refuses that registration, so the load ends as a
/// failure (which a closed session ignores) and no version stays reachable.
#[test]
fn a_session_released_between_points_refuses_the_next_registration() -> TestResult {
    let run = setup()?;
    let outcome = run.job.run(context(&run, false, Some(0)));
    assert_eq!(
        outcome.failure_reason_key.as_deref(),
        Some(SAMPLE_LOAD_FAILED)
    );
    assert!(!outcome.completed_scope);
    assert_eq!(run.report.end(), Some(SampleLoadEnd::Failed));
    assert!(run.report.observations().is_empty());
    // The second point's build was refused before its root was admitted, so
    // only the first point's gap examples are failed; the first point's other
    // units, whose versions were released, and the whole second point are
    // unprocessed.
    let points = run.bundle.observations();
    let first = points[0].items.len() as u64 + 1;
    let second = points[1].items.len() as u64 + 1;
    let counts = &outcome.counts;
    assert_eq!(counts.completed, 0);
    assert_eq!(counts.failed, gaps(&run, 0));
    assert_eq!(counts.unprocessed, first + second - counts.failed);
    // The first point was built and discarded; nothing stays reachable.
    let issued = run.ids.issued.lock().map_err(|_| "poisoned")?.clone();
    assert!(!issued.is_empty());
    for version_id in issued {
        assert!(run.sink.pin_current(&version_id).is_err());
    }
    assert_eq!(
        run.sink
            .release_sample_session(&run.session)
            .map_err(|e| format!("{e:?}"))?,
        0
    );
    Ok(())
}

#[test]
fn loading_the_same_bundle_twice_gives_the_same_items_and_totals() -> TestResult {
    let mut shapes = Vec::new();
    for _ in 0..2 {
        let run = setup()?;
        let outcome = run.job.run(context(&run, false, None));
        let mut versions = Vec::new();
        for (index, version_id, observed_at) in run.report.observations() {
            let version = run
                .sink
                .pin_current(&version_id)
                .map_err(|e| format!("{e:?}"))?;
            versions.push((index, observed_at, format!("{:?}", version.items())));
        }
        shapes.push((versions, outcome.counts, run.report.progress()));
    }
    assert_eq!(shapes[0], shapes[1]);
    Ok(())
}

struct Identity(AtomicU64);
impl JobIdentity for Identity {
    fn next_id(&self) -> Id {
        id(9000 + self.0.fetch_add(1, Ordering::SeqCst))
    }
}
struct Clock;
impl JobClock for Clock {
    fn now_ms(&self) -> i64 {
        4_000_000_000_000
    }
}
#[derive(Default)]
struct History(AtomicUsize);
impl JobHistory for History {
    fn append(&self, _: &JobSummary) -> Result<HistoryAppend, String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(HistoryAppend::Added)
    }
}
struct Events;
impl JobEvents for Events {
    fn publish(&self, _: JobEvent) {}
}

#[test]
fn the_job_runs_to_done_in_the_runtime_without_history() -> TestResult {
    let run = setup()?;
    let history = Arc::new(History::default());
    let runtime = JobRuntime::new(
        Arc::new(Identity(AtomicU64::new(0))),
        Arc::new(Clock),
        history.clone(),
        Arc::new(Events),
    )?;
    runtime.register(descriptor())?;
    let report = Arc::clone(&run.report);
    let created = runtime.submit(Submission {
        kind_id: SAMPLE_JOB_KIND.into(),
        deduplication_key: None,
        body: Arc::new(run.job),
        priority: JobPriority::Normal,
        scope_summary: "sample".into(),
        correlation_id: id(3),
        sample: true,
    })?;
    let SubmitResult::Created(job_id) = created else {
        return Err("expected a created job".into());
    };
    assert_eq!(runtime.run_next()?, Some(job_id.clone()));
    assert_eq!(report.end(), Some(SampleLoadEnd::Done));
    assert_eq!(runtime.state(&job_id)?, JobState::Done);
    assert_eq!(report.observations().len(), 2);
    assert_eq!(history.0.load(Ordering::SeqCst), 0);
    Ok(())
}
