//! Serializes query-result publication with cancellation and terminal state.
use crate::{JobRuntime, Publication, RuntimeError, Submission, SubmitResult};
use du_core_contracts::{Id, job::JobOutcome};
use std::sync::{Arc, Mutex};

impl JobRuntime {
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
        self.submit_inner(submission, None, Some(Box::new(publish)))
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
        let guard = gate.lock().map_err(|_| RuntimeError::Poisoned)?;
        let cancelled = {
            let table = self.table.lock().map_err(|_| RuntimeError::Poisoned)?;
            let job = table.jobs.get(id).ok_or(RuntimeError::NotFound)?;
            job.cancel.0.load(std::sync::atomic::Ordering::Acquire)
        };
        outcome.result_version_id = None;
        if !cancelled
            && outcome.completed_scope
            && let Some(publish) = publication.take()
        {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(publish)) {
                Ok(Ok(reference)) => outcome.result_version_id = Some(reference),
                Ok(Err(reason)) => {
                    outcome.completed_scope = false;
                    outcome.failure_reason_key = Some(reason);
                    outcome.result_version_id = None;
                }
                Err(_) => {
                    outcome.completed_scope = false;
                    outcome.failure_reason_key = Some("support.internal_error".into());
                    outcome.result_version_id = None;
                }
            }
        }
        self.finish(id, outcome)?;
        drop(guard);
        drop(publication);
        Ok(())
    }
}
