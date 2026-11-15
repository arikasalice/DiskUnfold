//! Bind the desktop job runtime and worker pool to engine shutdown hooks.

use std::sync::Arc;

use du_job_runtime::JobRuntime;

use super::scan_completion::ScanEvents;
use crate::{
    shutdown::{
        ShutdownAdmission, ShutdownHooks, ShutdownJobCancellation, ShutdownJobHooks,
        ShutdownPreparation, WatchStopResult,
    },
    workers::Workers,
};

pub(super) fn hooks(
    runtime: Arc<JobRuntime>,
    workers: Arc<Workers>,
    scan_events: Arc<ScanEvents>,
) -> ShutdownHooks {
    let admission_runtime = runtime.clone();
    let prepare_runtime = runtime.clone();
    let list_runtime = runtime.clone();
    let count_runtime = runtime.clone();
    let flush_runtime = runtime.clone();
    let history_runtime = runtime;
    let job_hooks = ShutdownJobHooks::new(
        move || {
            let cancellations = prepare_runtime
                .cancel_all_for_shutdown()?
                .into_iter()
                .map(|(job_ref, outcome)| ShutdownJobCancellation { job_ref, outcome })
                .collect();
            Ok(ShutdownPreparation {
                // No OS watcher is installed by the current desktop composition.
                watch_stop: WatchStopResult::NotInstalled,
                cancellations,
            })
        },
        move || {
            list_runtime.list(None).map(|rows| {
                rows.into_iter()
                    .filter(|row| !row.snapshot.state.is_terminal())
                    .map(|row| row.job_id)
                    .collect()
            })
        },
    )
    .with_runtime_reads(
        move || count_runtime.active_job_counts(),
        move |timeout| flush_runtime.begin_delivery_flush(timeout),
    );
    ShutdownHooks::new(
        move |confirmed| {
            let admission = admission_runtime.close_admission_for_shutdown(confirmed)?;
            Ok(match admission {
                du_job_runtime::AdmissionClose::Accepted { active_change_jobs } => {
                    ShutdownAdmission::Accepted { active_change_jobs }
                }
                du_job_runtime::AdmissionClose::ConfirmRequired { active_change_jobs } => {
                    ShutdownAdmission::ConfirmRequired { active_change_jobs }
                }
                du_job_runtime::AdmissionClose::AlreadyClosed => ShutdownAdmission::AlreadyClosing,
            })
        },
        job_hooks,
        move |timeout| history_runtime.begin_shutdown(timeout),
        // Every job has ended and its terminal event is delivered, so no further observation
        // row can be queued; the worker writes the queued ones before the database closes.
        move || {
            workers.close_and_join()?;
            scan_events.close_observations()
        },
    )
}

impl super::DesktopEvents {
    /// Reports whether queued events remain undelivered, for shutdown flushing.
    ///
    /// A poisoned queue reports no pending events so a failed queue cannot hold
    /// the process open.
    pub fn has_pending_events(&self) -> bool {
        self.0
            .lock()
            .is_ok_and(|pending| !pending.queue.is_empty() || pending.delivery_resync.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use du_core_contracts::{
        Id, SizeEvidence,
        job::{HistoryAppend, JobSummary},
        result::ResultCompleteness,
        scope_observation_log::{
            ObservedAmount, ObservedSize, SCOPE_FINGERPRINT_FORMAT_V1, ScopeObservation,
            ScopeObservationAppend, ScopeObservationLog, ScopeObservationOrigin,
            ScopeObservationWriteError,
        },
    };
    use du_job_runtime::{JobClock, JobEvent, JobEvents, JobIdentity};
    use std::{sync::Mutex, time::Duration};

    struct Identity;
    impl JobIdentity for Identity {
        fn next_id(&self) -> Id {
            Id::from_v7_bytes([1; 16])
        }
    }
    struct Clock;
    impl JobClock for Clock {
        fn now_ms(&self) -> i64 {
            1
        }
    }
    struct History;
    impl du_core_contracts::job::JobHistory for History {
        fn append(&self, _: &JobSummary) -> Result<HistoryAppend, String> {
            Ok(HistoryAppend::Added)
        }
    }
    struct Events;
    impl JobEvents for Events {
        fn publish(&self, _: JobEvent) {}
    }

    /// Stores rows slowly, so only a closing step that waits for the worker sees them all.
    #[derive(Default)]
    struct SlowLog(Mutex<Vec<Id>>);
    impl ScopeObservationLog for SlowLog {
        fn append(
            &self,
            observation: &ScopeObservation,
        ) -> Result<ScopeObservationAppend, ScopeObservationWriteError> {
            std::thread::sleep(Duration::from_millis(20));
            self.0
                .lock()
                .map_err(|_| ScopeObservationWriteError::Storage)?
                .push(observation.job_id.clone());
            Ok(ScopeObservationAppend::Added)
        }
    }

    fn row(n: u8) -> ScopeObservation {
        let size = ObservedSize {
            amount: ObservedAmount::Exact(1),
            evidence: SizeEvidence::Measured,
        };
        ScopeObservation {
            observation_id: Id::from_v7_bytes([n.wrapping_add(100); 16]),
            scope_fingerprint: [7; 32],
            fingerprint_format: SCOPE_FINGERPRINT_FORMAT_V1,
            observed_at_ms: 1_000,
            job_id: Id::from_v7_bytes([n; 16]),
            origin: ScopeObservationOrigin::Scan,
            completeness: ResultCompleteness::Complete,
            size_rule_id: "size.agg.subtree-unique".into(),
            size_rule_revision: 1,
            logical: size.clone(),
            allocated: size.clone(),
            unique_physical: size,
            created_at_ms: 2_000,
        }
    }

    #[test]
    fn closing_the_job_workers_also_writes_the_queued_observations()
    -> Result<(), Box<dyn std::error::Error>> {
        let runtime = Arc::new(JobRuntime::new(
            Arc::new(Identity),
            Arc::new(Clock),
            Arc::new(History),
            Arc::new(Events),
        )?);
        let workers = Arc::new(Workers::start(runtime.clone())?);
        let scan_events = Arc::new(ScanEvents::new(Arc::new(
            super::super::DesktopEvents::default(),
        )));
        let log = Arc::new(SlowLog::default());
        scan_events.bind_observation_log(log.clone())?;
        let hooks = hooks(runtime.clone(), workers, scan_events.clone());
        for n in 1..=3 {
            scan_events.queue_observation_for_test(row(n))?;
        }
        hooks.close_admission(true)?;
        let barrier = hooks.begin_history_barrier(Duration::from_secs(5))?;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while barrier.poll() == crate::shutdown::HistoryCommitStatus::Pending {
            assert!(
                std::time::Instant::now() < deadline,
                "history barrier stuck"
            );
            std::thread::yield_now();
        }
        hooks.close_workers()?;
        // The database may close now: every queued row is stored and the worker has ended.
        let stored = log.0.lock().map_err(|_| "log poisoned")?.clone();
        assert_eq!(stored.len(), 3);
        assert_eq!(Arc::strong_count(&log), 1);
        assert!(scan_events.queue_observation_for_test(row(4)).is_err());
        hooks.close_workers()?;
        Ok(())
    }
}
