//! Binds terminal scan delivery to the store that owns its immutable results.

use du_analysis_index::{CompletedScan, CompletedScanError, IngestSink, ScanCompletion};
use du_core_contracts::{
    Id,
    job::{HistoryChanged, JobState},
    scope_observation_log::ScopeObservationLog,
};
use du_job_runtime::{JobEvent, JobEvents};
use std::sync::{Arc, OnceLock};

mod observation;
mod progress;
pub(crate) use progress::ProgressEvents;

/// Read-only admission for a current scan proposed as a comparison source.
pub struct CompletedScanSource(IngestSink);

impl CompletedScanSource {
    /// Pins the requested current result only after its F24 terminal delivery.
    pub fn admit(&self, result_version: &Id) -> Result<CompletedScan, CompletedScanError> {
        self.0.pin_completed_scan(result_version)
    }
}

pub(crate) struct ScanEvents {
    downstream: Arc<dyn ProgressEvents>,
    source: OnceLock<IngestSink>,
    observations: OnceLock<Arc<dyn ScopeObservationLog>>,
}

impl ScanEvents {
    /// Wraps the runtime's event sink, which also relays batched job progress.
    pub(crate) fn new(downstream: Arc<impl ProgressEvents + 'static>) -> Self {
        Self {
            downstream,
            source: OnceLock::new(),
            observations: OnceLock::new(),
        }
    }

    /// Binds the result store before any scan can be dispatched from the engine.
    pub(crate) fn bind(&self, source: IngestSink) -> Result<CompletedScanSource, &'static str> {
        self.source
            .set(source.clone())
            .map_err(|_| "scan completion source already bound")?;
        Ok(CompletedScanSource(source))
    }

    /// Binds the log that receives one observation per finished GUI scan.
    ///
    /// Without a log, nothing is recorded; the CLI never binds one.
    pub(crate) fn bind_observation_log(
        &self,
        log: Arc<dyn ScopeObservationLog>,
    ) -> Result<(), &'static str> {
        self.observations
            .set(log)
            .map_err(|_| "scope observation log already bound")
    }

    /// Records the scan's observation after its terminal record, reporting failures only
    /// as a diagnostic so the scan result and job state stay as they are.
    fn record_observation(&self, summary: &du_core_contracts::job::JobSummary) {
        let (Some(store), Some(log)) = (self.source.get(), self.observations.get()) else {
            return;
        };
        let id = Id::from_v7_bytes(*uuid::Uuid::now_v7().as_bytes());
        if let observation::Outcome::Failed(failure) =
            observation::record(store, log.as_ref(), summary, id, super::utc_now_ms())
        {
            eprintln!("scope_observation.record_failed: {}", failure.reason());
        }
    }
}

impl JobEvents for ScanEvents {
    fn publish(&self, event: JobEvent) {
        if let JobEvent::Finished { summary, .. } = &event
            && summary.kind_id == "scan.metadata"
            && let Some(version) = &summary.result_version_id
            && let Some(source) = self.source.get()
        {
            let completion = match summary.state {
                JobState::Done => Some(ScanCompletion::Done),
                JobState::Partial => Some(ScanCompletion::Partial),
                _ => None,
            };
            let _ = source.observe_scan_terminal(version, completion);
        }
        if let JobEvent::Finished { summary, .. } = &event {
            self.record_observation(summary);
        }
        self.downstream.publish(event);
    }

    fn publish_history_changed(&self, change: HistoryChanged) {
        self.downstream.publish_history_changed(change);
    }

    fn publish_progress(&self, event: du_job_runtime::JobProgressEvent) {
        self.downstream.relay_progress(event);
    }

    fn retire_job(&self, id: &Id) {
        self.downstream.retire_job(id);
    }
}
