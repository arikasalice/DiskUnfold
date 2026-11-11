//! Serializes publication of prepared results with cancellation and terminal state.
use crate::{CancelResult, JobRuntime, Publication, RuntimeError, Submission, SubmitResult};
use du_core_contracts::{
    Id,
    job::{HistoryMode, JobDescriptor, JobOutcome, JobState, ResourceClass},
};
use std::sync::{Arc, Mutex};

/// Selects the prepared result to commit at the duplicate job's terminal boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DuplicateTerminal {
    Completed,
    Cancelled,
}

pub(super) fn valid(descriptor: &JobDescriptor, publication: Option<&Publication>) -> bool {
    match (descriptor.kind_id.as_str(), publication) {
        ("query.evaluate", Some(Publication::Query(_)))
        | ("viz.compute", Some(Publication::Visualization { .. })) => {
            descriptor.history == HistoryMode::None
                && descriptor.resource_classes.as_slice() == [ResourceClass::Compute]
        }
        ("query.evaluate" | "viz.compute", _) => false,
        ("duplicate.check", Some(Publication::DuplicateTerminal { .. })) => {
            descriptor.version == 1
                && descriptor.history == HistoryMode::Summary
                && descriptor.resource_classes.as_slice() == [ResourceClass::ContentIo]
        }
        (_, publication) => publication.is_none(),
    }
}

impl JobRuntime {
    /// Waits at most `timeout` for this visualization's authoritative state.
    /// The runtime table lock is released while waiting; no publication gate is held.
    pub fn wait_viz_terminal(
        &self,
        id: &Id,
        timeout: std::time::Duration,
    ) -> Result<JobState, RuntimeError> {
        let table = self.table.lock().map_err(|_| RuntimeError::Poisoned)?;
        let job = table.jobs.get(id).ok_or(RuntimeError::NotFound)?;
        if job.descriptor.kind_id != "viz.compute" {
            return Err(RuntimeError::InvalidDescriptor);
        }
        if timeout.is_zero() || job.state.is_terminal() {
            return Ok(job.state);
        }
        let wake = job.wake.clone();
        let (table, _) = wake
            .wait_timeout_while(table, timeout, |table| {
                table
                    .jobs
                    .get(id)
                    .is_some_and(|job| !job.state.is_terminal())
            })
            .map_err(|_| RuntimeError::Poisoned)?;
        table
            .jobs
            .get(id)
            .map(|job| job.state)
            .ok_or(RuntimeError::NotFound)
    }

    /// Submits duplicate work with a prepared queued-cancellation outcome.
    ///
    /// `queued` describes the entire unprocessed scope without performing file
    /// reads. The caller prepares its cancelled candidate before submission; for
    /// a running job it prepares the final candidate after its flows drain.
    /// The callback selects and atomically commits the candidate for the given
    /// terminal state, without blocking I/O or runtime reentry. On error or panic
    /// it must commit nothing. Failure is retained in the terminal outcome;
    /// cancellation still wins under F24 and no result ID is reported.
    pub fn submit_duplicate_terminal(
        &self,
        submission: Submission,
        queued: JobOutcome,
        commit: impl FnOnce(DuplicateTerminal) -> Result<Id, String> + Send + 'static,
    ) -> Result<SubmitResult, RuntimeError> {
        if queued.completed_scope
            || queued.result_version_id.is_some()
            || queued.unprocessed_summary.is_none()
            || queued.counts.completed != 0
            || queued.counts.overwritten != 0
            || queued.counts.failed != 0
            || queued.counts.skipped != 0
        {
            return Err(RuntimeError::InvalidState);
        }
        self.submit_inner(
            submission,
            None,
            Some(Publication::DuplicateTerminal {
                queued,
                commit: Box::new(commit),
            }),
            |_| Ok(()),
        )
    }

    /// Submits a query evaluation whose candidate is published only at completion.
    ///
    /// The callback must be a bounded in-memory atomic transaction: on error or
    /// panic it must publish nothing. It must not call back into job-runtime.
    /// The body may prepare a candidate in shared private state; it must not
    /// publish it itself. Cancellation that wins the gate skips this callback;
    /// after the callback wins, cancellation observes a terminal job. Only
    /// `query.evaluate` with history disabled accepts this internal boundary.
    pub fn submit_with_publication(
        &self,
        submission: Submission,
        publish: impl FnOnce() -> Result<Id, String> + Send + 'static,
    ) -> Result<SubmitResult, RuntimeError> {
        self.submit_inner(
            submission,
            None,
            Some(Publication::Query(Box::new(publish))),
            |_| Ok(()),
        )
    }

    /// Submits a visualization computation whose model is committed on completion.
    ///
    /// The callback commits a prepared model in a bounded in-memory transaction. It
    /// must publish nothing on error or panic and must not call the job runtime.
    /// Cancellation that wins the gate skips the commit. The model reference remains
    /// in the owning cache and is recovered by repeating the fixed render request.
    pub fn submit_viz_with_publication(
        &self,
        submission: Submission,
        publish: impl FnOnce() -> Result<(), String> + Send + 'static,
    ) -> Result<SubmitResult, RuntimeError> {
        self.submit_viz_with_admission(submission, publish, |_| Ok(()))
    }

    /// Admits a new visualization subscription before workers can observe its job.
    ///
    /// `admit` runs after validation under the runtime table lock. It must be
    /// bounded, perform no I/O or runtime calls, and leave the subscription registry
    /// unchanged on error or panic. Reusing an existing job skips the callback;
    /// the caller attaches to that job under `inspect_viz_gate` instead.
    pub fn submit_viz_with_admission(
        &self,
        submission: Submission,
        publish: impl FnOnce() -> Result<(), String> + Send + 'static,
        admit: impl FnOnce(&Id) -> Result<(), RuntimeError>,
    ) -> Result<SubmitResult, RuntimeError> {
        self.submit_inner(
            submission,
            None,
            Some(Publication::Visualization {
                commit: Box::new(publish),
                retained: None,
            }),
            admit,
        )
    }

    /// Like visualization admission, retaining caller-owned deferred disposal until
    /// after the publication gate on success, failure, panic, and cancellation.
    pub fn submit_viz_with_retained_admission(
        &self,
        submission: Submission,
        retained: Arc<dyn Send + Sync>,
        publish: impl FnOnce() -> Result<(), String> + Send + 'static,
        admit: impl FnOnce(&Id) -> Result<(), RuntimeError>,
    ) -> Result<SubmitResult, RuntimeError> {
        self.submit_inner(
            submission,
            None,
            Some(Publication::Visualization {
                commit: Box::new(publish),
                retained: Some(retained),
            }),
            admit,
        )
    }

    /// Serializes a subscriber admission decision with model publication and cancellation.
    /// The caller must admit a subscriber only while the supplied state permits it.
    /// The callback must be bounded and in-memory, leave no partial change on panic,
    /// and must not call the runtime.
    pub fn inspect_viz_gate<T>(
        &self,
        id: &Id,
        inspect: impl FnOnce(JobState) -> T,
    ) -> Result<T, RuntimeError> {
        self.with_viz_gate(id, |state, _| Ok(inspect(state)))
    }

    /// Detaches a subscriber under the same gate as model publication.
    /// The callback returns true only when it removed the final active subscriber.
    /// It must be bounded and in-memory, leave no partial change on error or panic,
    /// and must not call the runtime. If the job has already finished, the callback
    /// is skipped and the terminal state is returned.
    pub fn cancel_viz_if(
        &self,
        id: &Id,
        detach: impl FnOnce() -> Result<bool, RuntimeError>,
    ) -> Result<Option<(CancelResult, u64)>, RuntimeError> {
        let mut retired = None;
        self.with_viz_gate(id, |state, sequence| {
            if state.is_terminal() {
                return Ok(Some((CancelResult::AlreadyTerminal(state), sequence)));
            }
            if detach()? {
                self.cancel_under_gate(id, &mut retired).map(Some)
            } else {
                Ok(None)
            }
        })
    }

    fn with_viz_gate<T>(
        &self,
        id: &Id,
        operation: impl FnOnce(JobState, u64) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        let gate = self
            .publication_gate(id)?
            .ok_or(RuntimeError::InvalidState)?;
        let _guard = gate.lock().map_err(|_| RuntimeError::Poisoned)?;
        let (state, sequence) = {
            let table = self.table.lock().map_err(|_| RuntimeError::Poisoned)?;
            let job = table.jobs.get(id).ok_or(RuntimeError::NotFound)?;
            if job.descriptor.kind_id != "viz.compute" {
                return Err(RuntimeError::InvalidDescriptor);
            }
            (job.state, job.sequence)
        };
        // Keep the guard outside the unwind boundary so a subscriber failure
        // cannot prevent completion or cancellation of the physical job.
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| operation(state, sequence)))
            .map_err(|_| RuntimeError::InvalidState)?
    }

    pub(super) fn publication_gate(&self, id: &Id) -> Result<Option<Arc<Mutex<()>>>, RuntimeError> {
        let table = self.table.lock().map_err(|_| RuntimeError::Poisoned)?;
        Ok(table
            .jobs
            .get(id)
            .ok_or(RuntimeError::NotFound)?
            .publication_gate
            .clone())
    }

    pub(super) fn finish_with_publication(
        &self,
        id: &Id,
        mut outcome: JobOutcome,
        gate: Option<Arc<Mutex<()>>>,
        mut publication: Option<Publication>,
    ) -> Result<(), RuntimeError> {
        if publication.is_none() {
            return self.finish(id, outcome).map(|_| ());
        }
        let gate = gate.ok_or(RuntimeError::InvalidState)?;
        // Keep this ownership outside the callback's unwind boundary and alive
        // until after the gate on every exit, including `finish` errors.
        let retained = match publication.as_mut() {
            Some(Publication::Visualization { retained, .. }) => retained.take(),
            _ => None,
        };
        let guard = gate.lock().map_err(|_| RuntimeError::Poisoned)?;
        let cancelled = {
            let table = self.table.lock().map_err(|_| RuntimeError::Poisoned)?;
            let job = table.jobs.get(id).ok_or(RuntimeError::NotFound)?;
            job.cancel.0.load(std::sync::atomic::Ordering::Acquire)
        };
        outcome.result_version_id = None;
        if let Some(Publication::DuplicateTerminal { .. }) = publication.as_ref() {
            if (cancelled || outcome.completed_scope)
                && let Some(Publication::DuplicateTerminal { commit, .. }) = publication.take()
            {
                outcome = commit_duplicate(
                    outcome,
                    commit,
                    if cancelled {
                        DuplicateTerminal::Cancelled
                    } else {
                        DuplicateTerminal::Completed
                    },
                );
            }
            self.finish(id, outcome)?;
            return Ok(());
        }
        if !cancelled
            && outcome.completed_scope
            && let Some(publish) = publication.take()
        {
            let viz = matches!(&publish, Publication::Visualization { .. });
            let commit = move || match publish {
                Publication::Query(commit) => commit().map(Some),
                Publication::Visualization { commit, .. } => commit().map(|()| None),
                Publication::DuplicateTerminal { .. } => Err("support.internal_error".into()),
            };
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(commit)) {
                Ok(Ok(reference)) => outcome.result_version_id = reference,
                Ok(Err(reason)) => {
                    if viz {
                        outcome.counts = Default::default();
                    }
                    outcome.completed_scope = false;
                    outcome.failure_reason_key = Some(reason);
                    outcome.result_version_id = None;
                }
                Err(_) => {
                    if viz {
                        outcome.counts = Default::default();
                    }
                    outcome.completed_scope = false;
                    outcome.failure_reason_key = Some("support.internal_error".into());
                    outcome.result_version_id = None;
                }
            }
        }
        self.finish(id, outcome)?;
        drop(guard);
        drop(publication);
        drop(retained);
        Ok(())
    }
}

pub(super) fn commit_duplicate(
    mut outcome: JobOutcome,
    commit: Box<dyn FnOnce(DuplicateTerminal) -> Result<Id, String> + Send>,
    terminal: DuplicateTerminal,
) -> JobOutcome {
    outcome.result_version_id = None;
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| commit(terminal))) {
        Ok(Ok(id)) => outcome.result_version_id = Some(id),
        Ok(Err(reason)) => {
            outcome.completed_scope = false;
            outcome.failure_reason_key = Some(reason);
        }
        Err(_) => {
            outcome.completed_scope = false;
            outcome.failure_reason_key = Some("support.internal_error".into());
        }
    }
    outcome
}
