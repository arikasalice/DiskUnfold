//! Serializes publication of prepared results with cancellation and terminal state.
use crate::{CancelResult, JobRuntime, Publication, RuntimeError, Submission, SubmitResult};
use du_core_contracts::{
    Id,
    job::{HistoryMode, JobDescriptor, JobOutcome, JobState, ResourceClass},
};
use std::sync::{Arc, Mutex};

pub(super) fn valid(descriptor: &JobDescriptor, publication: Option<&Publication>) -> bool {
    match (descriptor.kind_id.as_str(), publication) {
        ("query.evaluate", Some(Publication::Query(_)))
        | ("viz.compute", Some(Publication::Visualization(_))) => {
            descriptor.history == HistoryMode::None
                && descriptor.resource_classes.as_slice() == [ResourceClass::Compute]
        }
        ("query.evaluate" | "viz.compute", _) => false,
        (_, publication) => publication.is_none(),
    }
}

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
        self.submit_inner(
            submission,
            None,
            Some(Publication::Query(Box::new(publish))),
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
        self.submit_inner(
            submission,
            None,
            Some(Publication::Visualization(Box::new(publish))),
        )
    }

    /// Serializes a subscriber admission decision with model publication and cancellation.
    /// The caller must admit a subscriber only while the supplied state permits it.
    /// The callback must be bounded and in-memory and must not call the runtime.
    pub fn inspect_viz_gate<T>(
        &self,
        id: &Id,
        inspect: impl FnOnce(JobState) -> T,
    ) -> Result<T, RuntimeError> {
        self.with_viz_gate(id, |state, _| Ok(inspect(state)))
    }

    /// Detaches a subscriber under the same gate as model publication.
    /// The callback returns true only when it removed the final active subscriber.
    /// It must be bounded and in-memory and must not call the runtime. If the job
    /// has already finished, the callback is skipped and the terminal state is returned.
    pub fn cancel_viz_if(
        &self,
        id: &Id,
        detach: impl FnOnce() -> Result<bool, RuntimeError>,
    ) -> Result<Option<(CancelResult, u64)>, RuntimeError> {
        self.with_viz_gate(id, |state, sequence| {
            if state.is_terminal() {
                return Ok(Some((CancelResult::AlreadyTerminal(state), sequence)));
            }
            if detach()? {
                self.cancel_under_gate(id).map(Some)
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
        operation(state, sequence)
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
            let commit = move || match publish {
                Publication::Query(commit) => commit().map(Some),
                Publication::Visualization(commit) => commit().map(|()| None),
            };
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(commit)) {
                Ok(Ok(reference)) => outcome.result_version_id = reference,
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
