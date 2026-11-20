//! F24 FR2.HD1.app-host.DD1 and FR14.HD1.app-host.DD1: batched job progress.
use super::*;
use crate::{JobLaneStatusEvent, JobProgressEvent, JobWaitReason};
use du_core_contracts::job::{JobProgress, LaneReport, LaneStatusReport, LaneWait};
use std::{
    sync::{atomic::AtomicU64, mpsc},
    thread::JoinHandle,
    time::Duration,
};

mod property;
mod slow;

const WAIT: Duration = Duration::from_secs(5);
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

/// Monotonic time that moves only when the test advances it.
#[derive(Default)]
struct FakeClock(AtomicU64);
impl FakeClock {
    fn advance(&self, millis: u64) {
        self.0.fetch_add(millis, Ordering::SeqCst);
    }
}
impl JobClock for FakeClock {
    fn now_ms(&self) -> i64 {
        1_000
    }
    fn monotonic_now(&self) -> Option<Duration> {
        Some(Duration::from_millis(self.0.load(Ordering::SeqCst)))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Seen {
    State(JobState, u64),
    Progress(JobProgressEvent),
    LaneStatus(JobLaneStatusEvent),
    Finished(JobState, u64),
}
impl Seen {
    fn sequence(&self) -> u64 {
        match self {
            Self::State(_, sequence) | Self::Finished(_, sequence) => *sequence,
            Self::Progress(event) => event.sequence,
            Self::LaneStatus(event) => event.sequence,
        }
    }
}

/// A fast subscriber that records each delivery in order.
#[derive(Default)]
struct Recorder(Mutex<Vec<Seen>>);
impl Recorder {
    fn seen(&self) -> Vec<Seen> {
        self.0.lock().map(|seen| seen.clone()).unwrap_or_default()
    }
    fn progress(&self) -> Vec<JobProgressEvent> {
        self.seen()
            .into_iter()
            .filter_map(|seen| match seen {
                Seen::Progress(event) => Some(event),
                _ => None,
            })
            .collect()
    }
    fn lane_statuses(&self) -> Vec<JobLaneStatusEvent> {
        self.seen()
            .into_iter()
            .filter_map(|seen| match seen {
                Seen::LaneStatus(event) => Some(event),
                _ => None,
            })
            .collect()
    }
    fn push(&self, seen: Seen) {
        if let Ok(mut events) = self.0.lock() {
            events.push(seen);
        }
    }
}
impl JobEvents for Recorder {
    fn publish(&self, event: JobEvent) {
        match event {
            JobEvent::StateChanged {
                state, sequence, ..
            } => self.push(Seen::State(state, sequence)),
            JobEvent::Finished {
                summary, sequence, ..
            } => self.push(Seen::Finished(summary.state, sequence)),
            JobEvent::ItemResult { .. }
            | JobEvent::ResultVersionChanged(_)
            | JobEvent::ResultFreshnessChanged(_) => {}
        }
    }
    fn publish_progress(&self, event: JobProgressEvent) {
        self.push(Seen::Progress(event));
    }
    fn publish_lane_status(&self, event: JobLaneStatusEvent) {
        self.push(Seen::LaneStatus(event));
    }
}

/// Checks the per-job order: increasing sequences and one final job.finished.
fn check_order(seen: &[Seen]) -> Result<(), String> {
    let mut last = 0;
    let mut finished = false;
    for event in seen {
        if finished {
            return Err(format!("event after job.finished: {event:?}"));
        }
        if event.sequence() <= last {
            return Err(format!("sequence did not increase after {last}: {event:?}"));
        }
        last = event.sequence();
        finished = matches!(event, Seen::Finished(..));
    }
    if finished {
        Ok(())
    } else {
        Err("job.finished is missing".into())
    }
}

enum Step {
    ReportProgress(JobProgress),
    ReportLaneStatus(LaneStatusReport),
    Finish,
}

/// A body that reports progress only when the test scripts it.
struct Scripted {
    steps: Mutex<mpsc::Receiver<Step>>,
    acks: mpsc::Sender<()>,
    kept: Arc<Mutex<Option<Arc<dyn JobContext>>>>,
}
impl Scripted {
    fn next_step(&self) -> Option<Step> {
        self.steps
            .lock()
            .ok()?
            .recv()
            .ok()
            .filter(|step| !matches!(step, Step::Finish))
    }
}
impl JobBody for Scripted {
    fn kind_id(&self) -> &str {
        "scan.metadata"
    }
    fn run(&self, context: Arc<dyn JobContext>) -> JobOutcome {
        if let Ok(mut kept) = self.kept.lock() {
            *kept = Some(context.clone());
        }
        let _ = self.acks.send(());
        while let Some(step) = self.next_step() {
            match step {
                Step::ReportProgress(progress) => context.report_progress(progress),
                Step::ReportLaneStatus(status) => context.report_lane_status(status),
                Step::Finish => break,
            }
            let _ = self.acks.send(());
        }
        JobOutcome {
            counts: JobCounts {
                completed: 1,
                ..Default::default()
            },
            completed_scope: true,
            failure_reason_key: None,
            unprocessed_summary: None,
            result_version_id: None,
        }
    }
}

/// One running scripted job on a runtime with a fake monotonic clock.
struct Harness<S> {
    runtime: Arc<JobRuntime>,
    clock: Arc<FakeClock>,
    sink: Arc<S>,
    id: Id,
    steps: mpsc::Sender<Step>,
    acks: mpsc::Receiver<()>,
    kept: Arc<Mutex<Option<Arc<dyn JobContext>>>>,
    worker: Option<JoinHandle<Result<Option<Id>, RuntimeError>>>,
}
impl<S: JobEvents + 'static> Harness<S> {
    fn start(sink: Arc<S>, unit: ProgressUnit) -> TestResult<Self> {
        let clock = Arc::new(FakeClock::default());
        let runtime = Arc::new(JobRuntime::new(
            Arc::new(Identity(Id::parse("00000000-0000-7000-8000-000000000001")?)),
            clock.clone(),
            Arc::new(History::default()),
            sink.clone(),
        )?);
        let mut kind = descriptor();
        kind.progress_unit = unit;
        kind.controls = vec![JobControl::Pause, JobControl::Resume, JobControl::Cancel];
        runtime.register(kind)?;
        let (steps, script) = mpsc::channel();
        let (ack, acks) = mpsc::channel();
        let kept = Arc::new(Mutex::new(None));
        let id = created(runtime.submit(sample_submission(Arc::new(Scripted {
            steps: Mutex::new(script),
            acks: ack,
            kept: kept.clone(),
        }))?)?)?;
        let worker = {
            let runtime = runtime.clone();
            std::thread::spawn(move || runtime.run_next())
        };
        acks.recv_timeout(WAIT)?;
        // Reports wait for the test to advance past the first batch window.
        runtime.tick_progress()?;
        Ok(Self {
            runtime,
            clock,
            sink,
            id,
            steps,
            acks,
            kept,
            worker: Some(worker),
        })
    }
    fn report(&self, completed: u64, total: Option<u64>) -> TestResult {
        self.steps
            .send(Step::ReportProgress(JobProgress { completed, total }))?;
        self.acks.recv_timeout(WAIT)?;
        Ok(())
    }
    fn report_lane_status(&self, status: LaneStatusReport) -> TestResult {
        self.steps.send(Step::ReportLaneStatus(status))?;
        self.acks.recv_timeout(WAIT)?;
        Ok(())
    }
    /// Moves to the next interval, ticks, and waits until delivery catches up.
    fn next_batch(&self) -> TestResult {
        self.clock.advance(100);
        self.runtime.tick_progress()?;
        self.runtime.flush()?;
        Ok(())
    }
    fn finish(&mut self) -> TestResult {
        self.steps.send(Step::Finish)?;
        if let Some(worker) = self.worker.take() {
            worker.join().map_err(|_| "worker panicked")??;
        }
        self.runtime.flush()?;
        Ok(())
    }
    /// Reports through the body's context after the body has returned.
    fn late_report(&self, completed: u64) -> TestResult {
        let context = self
            .kept
            .lock()
            .map_err(|_| "context lock")?
            .clone()
            .ok_or("missing context")?;
        context.report_progress(JobProgress {
            completed,
            total: None,
        });
        Ok(())
    }
    fn late_lane_status(&self, status: LaneStatusReport) -> TestResult {
        let context = self
            .kept
            .lock()
            .map_err(|_| "context lock")?
            .clone()
            .ok_or("missing context")?;
        context.report_lane_status(status);
        Ok(())
    }
}

fn lane_status(remaining_directories: u64) -> LaneStatusReport {
    LaneStatusReport {
        lanes: vec![LaneReport {
            lane_index: 0,
            root_indexes: vec![0],
            in_flight: 1,
            limit: 2,
            mode: None,
            permit: None,
            strategy: None,
            wait: LaneWait::IoWait,
            wait_confirmed: true,
            remaining_directories,
            failures: 0,
        }],
    }
}

fn recorded(unit: ProgressUnit) -> TestResult<Harness<Recorder>> {
    Harness::start(Arc::new(Recorder::default()), unit)
}

#[test]
fn a_burst_within_one_interval_publishes_only_its_latest_value() -> TestResult {
    let mut job = recorded(ProgressUnit::Items)?;
    for completed in 1..=3 {
        job.report(completed, None)?;
    }
    job.runtime.tick_progress()?;
    job.runtime.flush()?;
    assert!(
        job.sink.progress().is_empty(),
        "the interval has not elapsed"
    );
    job.next_batch()?;
    let progress = job.sink.progress();
    assert_eq!(progress.len(), 1);
    let event = &progress[0];
    assert_eq!(
        (event.completed, event.total, event.remaining),
        (3, None, None)
    );
    assert_eq!(event.unit, ProgressUnit::Items);
    assert_eq!(event.wait_reason, JobWaitReason::Unknown);
    assert_eq!(event.observed_at_ms, 1_000);
    assert_eq!(event.job_id, job.id);
    assert_eq!(event.sequence, 3); // Queued and running take the first two sequences.
    assert_eq!(job.runtime.snapshot(&job.id)?.sequence, 3);
    job.finish()?;
    check_order(&job.sink.seen())?;
    Ok(())
}

#[test]
fn lane_status_batches_with_progress_in_shared_sequence_order() -> TestResult {
    let mut job = recorded(ProgressUnit::Items)?;
    job.report(5, Some(12))?;
    job.report_lane_status(lane_status(8))?;
    job.report_lane_status(lane_status(3))?;
    job.runtime.tick_progress()?;
    job.runtime.flush()?;
    assert!(job.sink.progress().is_empty());
    assert!(job.sink.lane_statuses().is_empty());

    job.next_batch()?;
    let progress = job.sink.progress();
    let statuses = job.sink.lane_statuses();
    assert_eq!(progress.len(), 1);
    assert_eq!(progress[0].sequence, 3);
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].sequence, 4);
    assert_eq!(statuses[0].status, lane_status(3));
    assert_eq!(statuses[0].observed_at_ms, 1_000);
    assert_eq!(
        job.sink
            .seen()
            .iter()
            .filter_map(|event| match event {
                Seen::Progress(event) => Some(event.sequence),
                Seen::LaneStatus(event) => Some(event.sequence),
                _ => None,
            })
            .collect::<Vec<_>>(),
        vec![3, 4]
    );

    job.next_batch()?;
    assert_eq!(job.sink.progress().len(), 1);
    assert_eq!(job.sink.lane_statuses().len(), 1);
    job.finish()?;
    check_order(&job.sink.seen())?;
    Ok(())
}

#[test]
fn lane_status_reported_after_job_finished_is_discarded() -> TestResult {
    let mut job = recorded(ProgressUnit::Items)?;
    job.finish()?;
    job.late_lane_status(lane_status(1))?;
    job.next_batch()?;
    assert!(job.sink.lane_statuses().is_empty());
    check_order(&job.sink.seen())?;
    Ok(())
}

#[test]
fn an_unchanged_value_between_ticks_publishes_nothing() -> TestResult {
    let mut job = recorded(ProgressUnit::Items)?;
    job.report(4, Some(10))?;
    job.next_batch()?;
    assert_eq!(job.sink.progress().len(), 1);
    job.next_batch()?;
    job.report(4, Some(10))?;
    job.next_batch()?;
    assert_eq!(job.sink.progress().len(), 1);
    assert_eq!(job.runtime.snapshot(&job.id)?.sequence, 3);
    job.finish()?;
    check_order(&job.sink.seen())?;
    Ok(())
}

#[test]
fn an_unknown_total_becomes_known_with_its_remaining_amount() -> TestResult {
    let mut job = recorded(ProgressUnit::Items)?;
    job.report(2, None)?;
    job.next_batch()?;
    job.report(3, Some(10))?;
    job.next_batch()?;
    let values: Vec<_> = job
        .sink
        .progress()
        .iter()
        .map(|event| {
            (
                event.sequence,
                event.completed,
                event.total,
                event.remaining,
            )
        })
        .collect();
    assert_eq!(values, vec![(3, 2, None, None), (4, 3, Some(10), Some(7))]);
    job.finish()?;
    check_order(&job.sink.seen())?;
    Ok(())
}

#[test]
fn a_zero_total_uses_the_descriptor_unit_and_never_goes_negative() -> TestResult {
    let mut job = recorded(ProgressUnit::Bytes)?;
    job.report(0, Some(0))?;
    job.next_batch()?;
    job.report(5, Some(3))?;
    job.next_batch()?;
    let values: Vec<_> = job
        .sink
        .progress()
        .iter()
        .map(|event| (event.unit, event.completed, event.total, event.remaining))
        .collect();
    assert_eq!(
        values,
        vec![
            (ProgressUnit::Bytes, 0, Some(0), Some(0)),
            (ProgressUnit::Bytes, 5, Some(3), Some(0)),
        ]
    );
    job.finish()?;
    Ok(())
}

#[test]
fn progress_shares_the_sequence_space_and_wait_reason_of_its_state() -> TestResult {
    let mut job = recorded(ProgressUnit::Items)?;
    assert_eq!(
        job.runtime.pause(&job.id)?,
        (CancelResult::Accepted(JobState::PauseRequested), 3)
    );
    job.report(1, None)?;
    job.next_batch()?;
    let progress = job.sink.progress();
    assert_eq!(progress.len(), 1);
    // PauseRequested is 3 and the settled pause is 4.
    assert_eq!(progress[0].sequence, 5);
    assert_eq!(progress[0].wait_reason, JobWaitReason::Paused);
    assert_eq!(
        job.runtime.resume(&job.id)?,
        (CancelResult::Accepted(JobState::Running), 6)
    );
    job.finish()?;
    check_order(&job.sink.seen())?;
    Ok(())
}

#[test]
fn reports_after_the_terminal_state_are_discarded_and_logged() -> TestResult {
    let mut job = recorded(ProgressUnit::Items)?;
    job.report(1, None)?;
    job.next_batch()?;
    // Reported before the end but not yet published: job.finished supersedes it.
    job.report(2, None)?;
    job.finish()?;
    assert!(!job.runtime.late_progress_logged(&job.id)?);
    job.late_report(3)?;
    job.late_report(4)?;
    job.next_batch()?;
    job.next_batch()?;
    assert!(job.runtime.late_progress_logged(&job.id)?);
    let seen = job.sink.seen();
    check_order(&seen)?;
    let progress: Vec<_> = job
        .sink
        .progress()
        .iter()
        .map(|event| event.completed)
        .collect();
    assert_eq!(progress, vec![1]);
    assert_eq!(seen.last(), Some(&Seen::Finished(JobState::Done, 5)));
    assert_eq!(job.runtime.state(&job.id)?, JobState::Done);
    assert_eq!(job.runtime.snapshot(&job.id)?.sequence, 5);
    Ok(())
}
