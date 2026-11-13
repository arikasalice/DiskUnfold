//! In-process shutdown preparation for the composed desktop engine.

use std::{
    io,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use du_core_contracts::Id;
pub use du_job_runtime::ActiveJobCounts;
pub use du_job_runtime::{CancelResult, RuntimeError};
use du_job_runtime::{DeliveryFlush, ShutdownStatus, ShutdownTicket};

/// Status of the F24 delivery barrier that follows terminal job completion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryCommitStatus {
    /// Earlier job history writes and terminal notifications are still draining.
    Pending,
    /// All earlier delivery finished and every required history append succeeded.
    Complete,
    /// The caller's deadline elapsed; poll again without closing storage.
    TimedOut,
    /// Delivery stopped unexpectedly or at least one required history append failed.
    Failed,
}

/// A ticket proving the F24 delivery queue reached its shutdown barrier.
pub struct HistoryCommitBarrier {
    ticket: Option<ShutdownTicket>,
    complete: std::sync::Arc<AtomicBool>,
}

impl HistoryCommitBarrier {
    pub(crate) fn no_runtime() -> Self {
        Self {
            ticket: None,
            complete: std::sync::Arc::new(AtomicBool::new(true)),
        }
    }

    /// Polls without waiting. `Complete` is returned only after the consumer thread exits.
    pub fn poll(&self) -> HistoryCommitStatus {
        let status = match &self.ticket {
            None => HistoryCommitStatus::Complete,
            Some(ticket) => match ticket.poll() {
                ShutdownStatus::Pending => HistoryCommitStatus::Pending,
                ShutdownStatus::Complete => HistoryCommitStatus::Complete,
                ShutdownStatus::TimedOut => HistoryCommitStatus::TimedOut,
                ShutdownStatus::Failed => HistoryCommitStatus::Failed,
            },
        };
        if status == HistoryCommitStatus::Complete {
            self.complete.store(true, Ordering::Release);
        }
        status
    }

    /// Returns the failed history append count after delivery stops.
    pub fn history_failures(&self) -> Option<usize> {
        self.ticket
            .as_ref()
            .and_then(ShutdownTicket::history_failures)
    }
}

/// A bounded flush of the delivery queue that does not wait for active jobs.
///
/// Completion means the terminal history of every job that had already finished
/// when the flush began was handed to the history port. It says nothing about
/// jobs that are still running.
pub struct TerminalHistoryFlush {
    flush: Option<DeliveryFlush>,
}

impl TerminalHistoryFlush {
    pub(crate) fn no_runtime() -> Self {
        Self { flush: None }
    }

    /// Polls without waiting. `TimedOut` means the caller's deadline elapsed
    /// and the history of finished jobs may still be undelivered.
    pub fn poll(&self) -> HistoryCommitStatus {
        match self.flush.as_ref().map(DeliveryFlush::poll) {
            None | Some(ShutdownStatus::Complete) => HistoryCommitStatus::Complete,
            Some(ShutdownStatus::Pending) => HistoryCommitStatus::Pending,
            Some(ShutdownStatus::TimedOut) => HistoryCommitStatus::TimedOut,
            Some(ShutdownStatus::Failed) => HistoryCommitStatus::Failed,
        }
    }
}

/// Result of asking the composed engine to stop its observation sources.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchStopResult {
    /// An installed source was stopped.
    Stopped,
    /// No observation source is currently installed in this engine composition.
    NotInstalled,
    /// A source did not confirm that it had stopped.
    Failed,
}

/// One F24 cancellation acknowledgement returned by shutdown preparation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShutdownJobCancellation {
    /// Job reference whose cancellation was requested.
    pub job_ref: Id,
    /// Original F24 result and sequence, or the runtime error for this request.
    pub outcome: Result<(CancelResult, u64), RuntimeError>,
}

/// Results of stopping observation sources and requesting cancellation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShutdownPreparation {
    /// Whether this engine composition had a source to stop.
    pub watch_stop: WatchStopResult,
    /// Per-job cancellation result, kept in submission order.
    pub cancellations: Vec<ShutdownJobCancellation>,
}

/// Atomic GUI-quit admission result produced before gateway closure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShutdownAdmission {
    /// No new jobs can be submitted; shutdown may continue.
    Accepted {
        /// Nonterminal `change.*` jobs present when admission closed.
        active_change_jobs: usize,
    },
    /// The request did not change runtime state and needs user confirmation.
    ConfirmRequired {
        /// Nonterminal `change.*` jobs present at the unchanged admission cut.
        active_change_jobs: usize,
    },
    /// Job admission was already closed by a previous accepted request.
    AlreadyClosing,
}

impl ShutdownPreparation {
    pub(crate) fn no_runtime() -> Self {
        Self {
            watch_stop: WatchStopResult::NotInstalled,
            cancellations: Vec::new(),
        }
    }
}

/// Failure reported by the engine shutdown hooks.
#[derive(Debug)]
pub enum ShutdownError {
    /// Job-runtime state or cancellation failed.
    Runtime(RuntimeError),
    /// A shutdown coordination mutex was poisoned.
    Poisoned,
    /// Worker shutdown could not complete.
    WorkerClose(String),
    /// Workers cannot be stopped before an accepted shutdown closes admission.
    AdmissionOpen,
    /// Workers cannot be stopped before all prior history delivery commits.
    HistoryBarrierPending,
    /// A worker close was attempted while jobs were still nonterminal.
    ActiveJobs,
}

impl std::fmt::Display for ShutdownError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Runtime(error) => write!(f, "job-runtime shutdown failed: {error}"),
            Self::Poisoned => f.write_str("engine shutdown state is poisoned"),
            Self::WorkerClose(error) => write!(f, "job workers could not be stopped: {error}"),
            Self::AdmissionOpen => f.write_str("job admission is still open"),
            Self::HistoryBarrierPending => {
                f.write_str("history delivery has not reached its shutdown barrier")
            }
            Self::ActiveJobs => f.write_str("jobs are still active during worker shutdown"),
        }
    }
}

impl std::error::Error for ShutdownError {}

/// Job operations grouped to keep shutdown hook construction small.
pub(crate) struct ShutdownJobHooks {
    prepare: Box<dyn Fn() -> Result<ShutdownPreparation, RuntimeError> + Send + Sync>,
    list_active_jobs: Box<dyn Fn() -> Result<Vec<Id>, RuntimeError> + Send + Sync>,
    active_counts: Option<Box<dyn Fn() -> Result<ActiveJobCounts, RuntimeError> + Send + Sync>>,
    begin_flush: Option<FlushHook>,
}

type FlushHook = Box<dyn Fn(Duration) -> Result<DeliveryFlush, RuntimeError> + Send + Sync>;

impl ShutdownJobHooks {
    pub(crate) fn new(
        prepare: impl Fn() -> Result<ShutdownPreparation, RuntimeError> + Send + Sync + 'static,
        list_active_jobs: impl Fn() -> Result<Vec<Id>, RuntimeError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            prepare: Box::new(prepare),
            list_active_jobs: Box::new(list_active_jobs),
            active_counts: None,
            begin_flush: None,
        }
    }

    /// Adds the count of nonterminal jobs and the delivery flush, which a
    /// composition without a job runtime leaves out.
    pub(crate) fn with_runtime_reads(
        mut self,
        active_counts: impl Fn() -> Result<ActiveJobCounts, RuntimeError> + Send + Sync + 'static,
        begin_flush: impl Fn(Duration) -> Result<DeliveryFlush, RuntimeError> + Send + Sync + 'static,
    ) -> Self {
        self.active_counts = Some(Box::new(active_counts));
        self.begin_flush = Some(Box::new(begin_flush));
        self
    }
}

/// Composition-time hooks installed by the desktop service registration.
pub(crate) struct ShutdownHooks {
    close_admission: Box<dyn Fn(bool) -> Result<ShutdownAdmission, RuntimeError> + Send + Sync>,
    operations: ShutdownJobHooks,
    begin_history_barrier:
        Box<dyn Fn(Duration) -> Result<ShutdownTicket, RuntimeError> + Send + Sync>,
    close_workers: Box<dyn Fn() -> io::Result<()> + Send + Sync>,
    admission_closed: AtomicBool,
    history_barrier_complete: std::sync::Arc<AtomicBool>,
    prepared: Mutex<Option<ShutdownPreparation>>,
    workers_closed: Mutex<bool>,
}

impl ShutdownHooks {
    pub(crate) fn new(
        close_admission: impl Fn(bool) -> Result<ShutdownAdmission, RuntimeError>
        + Send
        + Sync
        + 'static,
        operations: ShutdownJobHooks,
        begin_history_barrier: impl Fn(Duration) -> Result<ShutdownTicket, RuntimeError>
        + Send
        + Sync
        + 'static,
        close_workers: impl Fn() -> io::Result<()> + Send + Sync + 'static,
    ) -> Self {
        Self {
            close_admission: Box::new(close_admission),
            operations,
            begin_history_barrier: Box::new(begin_history_barrier),
            close_workers: Box::new(close_workers),
            admission_closed: AtomicBool::new(false),
            history_barrier_complete: std::sync::Arc::new(AtomicBool::new(false)),
            prepared: Mutex::new(None),
            workers_closed: Mutex::new(false),
        }
    }

    pub(crate) fn close_admission(
        &self,
        confirmed: bool,
    ) -> Result<ShutdownAdmission, ShutdownError> {
        let admission = (self.close_admission)(confirmed).map_err(ShutdownError::Runtime)?;
        if matches!(
            admission,
            ShutdownAdmission::Accepted { .. } | ShutdownAdmission::AlreadyClosing
        ) {
            self.admission_closed.store(true, Ordering::Release);
        }
        Ok(admission)
    }

    pub(crate) fn prepare(&self) -> Result<ShutdownPreparation, ShutdownError> {
        if !self.admission_closed.load(Ordering::Acquire) {
            return Err(ShutdownError::AdmissionOpen);
        }
        let mut prepared = self.prepared.lock().map_err(|_| ShutdownError::Poisoned)?;
        if let Some(report) = prepared.as_ref() {
            return Ok(report.clone());
        }
        let report = (self.operations.prepare)().map_err(ShutdownError::Runtime)?;
        *prepared = Some(report.clone());
        Ok(report)
    }

    pub(crate) fn list_active_jobs(&self) -> Result<Vec<Id>, ShutdownError> {
        if !self.admission_closed.load(Ordering::Acquire) {
            return Err(ShutdownError::AdmissionOpen);
        }
        (self.operations.list_active_jobs)().map_err(ShutdownError::Runtime)
    }

    pub(crate) fn begin_history_barrier(
        &self,
        timeout: Duration,
    ) -> Result<HistoryCommitBarrier, ShutdownError> {
        let ticket = (self.begin_history_barrier)(timeout).map_err(ShutdownError::Runtime)?;
        Ok(HistoryCommitBarrier {
            ticket: Some(ticket),
            complete: self.history_barrier_complete.clone(),
        })
    }

    /// Counts nonterminal jobs; callable before admission closes.
    pub(crate) fn active_job_counts(&self) -> Result<ActiveJobCounts, ShutdownError> {
        self.operations
            .active_counts
            .as_ref()
            .map_or(Ok(ActiveJobCounts::default()), |count| {
                count().map_err(ShutdownError::Runtime)
            })
    }

    pub(crate) fn begin_terminal_history_flush(
        &self,
        timeout: Duration,
    ) -> Result<TerminalHistoryFlush, ShutdownError> {
        match &self.operations.begin_flush {
            None => Ok(TerminalHistoryFlush::no_runtime()),
            Some(begin) => Ok(TerminalHistoryFlush {
                flush: Some(begin(timeout).map_err(ShutdownError::Runtime)?),
            }),
        }
    }

    pub(crate) fn close_workers(&self) -> Result<(), ShutdownError> {
        if !self.admission_closed.load(Ordering::Acquire) {
            return Err(ShutdownError::AdmissionOpen);
        }
        if !self.history_barrier_complete.load(Ordering::Acquire) {
            return Err(ShutdownError::HistoryBarrierPending);
        }
        let mut closed = self
            .workers_closed
            .lock()
            .map_err(|_| ShutdownError::Poisoned)?;
        if *closed {
            return Ok(());
        }
        (self.close_workers)().map_err(|error| ShutdownError::WorkerClose(error.to_string()))?;
        *closed = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    struct Identity;
    impl du_job_runtime::JobIdentity for Identity {
        fn next_id(&self) -> Id {
            Id::from_v7_bytes([1; 16])
        }
    }
    struct Clock;
    impl du_job_runtime::JobClock for Clock {
        fn now_ms(&self) -> i64 {
            1
        }
    }
    struct History;
    impl du_core_contracts::job::JobHistory for History {
        fn append(
            &self,
            _: &du_core_contracts::job::JobSummary,
        ) -> Result<du_core_contracts::job::HistoryAppend, String> {
            Ok(du_core_contracts::job::HistoryAppend::Added)
        }
    }
    struct Events;
    impl du_job_runtime::JobEvents for Events {
        fn publish(&self, _: du_job_runtime::JobEvent) {}
    }

    #[test]
    fn preparation_and_worker_close_are_idempotent() -> Result<(), Box<dyn std::error::Error>> {
        let runtime = std::sync::Arc::new(du_job_runtime::JobRuntime::new(
            std::sync::Arc::new(Identity),
            std::sync::Arc::new(Clock),
            std::sync::Arc::new(History),
            std::sync::Arc::new(Events),
        )?);
        let prepare_calls = Arc::new(AtomicUsize::new(0));
        let close_calls = Arc::new(AtomicUsize::new(0));
        let prepare_count = prepare_calls.clone();
        let close_count = close_calls.clone();
        let list_runtime = runtime.clone();
        let barrier_runtime = runtime.clone();
        let job_hooks = ShutdownJobHooks::new(
            move || {
                prepare_count.fetch_add(1, Ordering::AcqRel);
                Ok(ShutdownPreparation::no_runtime())
            },
            move || {
                list_runtime
                    .list(None)
                    .map(|rows| rows.into_iter().map(|row| row.job_id).collect())
            },
        );
        let hooks = ShutdownHooks::new(
            |_| {
                Ok(ShutdownAdmission::Accepted {
                    active_change_jobs: 0,
                })
            },
            job_hooks,
            move |timeout| barrier_runtime.begin_shutdown(timeout),
            move || {
                close_count.fetch_add(1, Ordering::AcqRel);
                Ok(())
            },
        );

        assert!(matches!(hooks.prepare(), Err(ShutdownError::AdmissionOpen)));
        assert!(matches!(
            hooks.close_admission(true)?,
            ShutdownAdmission::Accepted {
                active_change_jobs: 0
            }
        ));
        let first = hooks.prepare()?;
        assert_eq!(hooks.prepare()?, first);
        assert!(matches!(
            hooks.close_workers(),
            Err(ShutdownError::HistoryBarrierPending)
        ));
        let barrier = hooks.begin_history_barrier(Duration::from_secs(1))?;
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while barrier.poll() == HistoryCommitStatus::Pending {
            if std::time::Instant::now() >= deadline {
                return Err("history barrier did not finish".into());
            }
            std::thread::yield_now();
        }
        assert_eq!(barrier.poll(), HistoryCommitStatus::Complete);
        hooks.close_workers()?;
        hooks.close_workers()?;
        assert_eq!(prepare_calls.load(Ordering::Acquire), 1);
        assert_eq!(close_calls.load(Ordering::Acquire), 1);
        Ok(())
    }

    #[test]
    fn public_engine_cannot_prepare_shutdown_before_admission_closes()
    -> Result<(), Box<dyn std::error::Error>> {
        let prepare_calls = Arc::new(AtomicUsize::new(0));
        let prepare_count = prepare_calls.clone();
        let job_hooks = ShutdownJobHooks::new(
            move || {
                prepare_count.fetch_add(1, Ordering::AcqRel);
                Ok(ShutdownPreparation::no_runtime())
            },
            || Ok(Vec::new()),
        );
        let hooks = ShutdownHooks::new(
            |_| {
                Ok(ShutdownAdmission::Accepted {
                    active_change_jobs: 0,
                })
            },
            job_hooks,
            |_| Err(RuntimeError::InvalidState),
            || Ok(()),
        );
        let mut builder = crate::api::EngineBuilder::default();
        builder.on_shutdown_hooks(hooks);
        let engine = builder.build()?;

        assert!(matches!(
            engine.prepare_shutdown(),
            Err(ShutdownError::AdmissionOpen)
        ));
        assert_eq!(prepare_calls.load(Ordering::Acquire), 0);
        assert_eq!(
            engine.close_job_admission(false)?,
            ShutdownAdmission::Accepted {
                active_change_jobs: 0
            }
        );
        assert_eq!(
            engine.prepare_shutdown()?,
            ShutdownPreparation::no_runtime()
        );
        assert_eq!(prepare_calls.load(Ordering::Acquire), 1);
        Ok(())
    }
}
