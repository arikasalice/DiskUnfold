//! Job submission used by the sample session.

use du_core_contracts::Id;
use du_job_runtime::{RuntimeError, Submission, SubmitResult};

use crate::workers::Workers;

/// The slice of the job runtime that the sample session needs.
pub(crate) trait SampleJobs: Send + Sync {
    /// Submits a job and wakes a worker when it was newly created.
    fn submit(&self, submission: Submission) -> Result<SubmitResult, RuntimeError>;

    /// Requests cancellation; an unknown or finished job is not an error here.
    fn cancel(&self, job_ref: &Id);
}

impl SampleJobs for Workers {
    fn submit(&self, submission: Submission) -> Result<SubmitResult, RuntimeError> {
        let result = self.runtime.submit(submission)?;
        if matches!(result, SubmitResult::Created(_)) {
            self.notify();
        }
        Ok(result)
    }

    fn cancel(&self, job_ref: &Id) {
        let _ = self.runtime.cancel(job_ref);
    }
}
