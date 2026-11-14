//! Relays batched job progress from the runtime to the desktop event queue
//! as the F24 IF-06 `job.progress` event.
use crate::{desktop::DesktopEvents, jobs::wait_reason};
use du_core_contracts::{
    ENVELOPE_VERSION, EventEnvelope, Timestamp,
    analysis_ipc::{AnalysisJobProgress, JOB_PROGRESS},
};
use du_job_runtime::{JobEvents, JobProgressEvent};

/// A runtime event sink that can also relay batched job progress.
pub(crate) trait ProgressEvents: JobEvents {
    /// Relays one progress value on the delivery thread, in job-sequence order.
    /// The default hands the value to [`JobEvents::publish_progress`].
    fn relay_progress(&self, event: JobProgressEvent) {
        self.publish_progress(event);
    }
}

impl ProgressEvents for DesktopEvents {
    fn relay_progress(&self, event: JobProgressEvent) {
        if let Some(event) = envelope(event) {
            self.enqueue(event);
        }
    }
}

/// Maps a runtime value to the `job.progress` envelope; the relay batches it.
fn envelope(event: JobProgressEvent) -> Option<EventEnvelope> {
    let observed_at = Timestamp::from_unix_millis(event.observed_at_ms)?;
    let body = AnalysisJobProgress {
        sequence: event.sequence,
        unit: event.unit,
        completed: event.completed,
        total: event.total,
        total_known: event.total.is_some(),
        wait_reason: wait_reason(event.wait_reason),
        observed_at: observed_at.clone(),
    };
    Some(EventEnvelope {
        topic: JOB_PROGRESS.into(),
        contract_version: ENVELOPE_VERSION,
        seq: 0,
        correlation_id: None,
        job_ref: Some(event.job_id),
        // The optional latest partial result version is not carried yet.
        result_version: None,
        occurred_at: observed_at,
        body: serde_json::to_value(body).ok()?,
    })
}

#[cfg(test)]
mod tests;
