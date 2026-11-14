//! The job-finished tap that tells the sample session when its load job ends.

use std::sync::{Arc, OnceLock};

use du_core_contracts::{Id, job::HistoryChanged};
use du_job_runtime::{JobEvent, JobEvents};

use super::{KIND, SampleSessions};

/// Wraps the runtime's event receiver and tells the sample session when its load job ends.
///
/// Every event, including the `scan.sample` terminal one, still reaches the downstream.
pub struct SampleEvents {
    downstream: Arc<dyn JobEvents>,
    sessions: OnceLock<Arc<SampleSessions>>,
}

impl SampleEvents {
    /// Chains in front of `downstream`.
    pub fn new(downstream: Arc<dyn JobEvents>) -> Self {
        Self {
            downstream,
            sessions: OnceLock::new(),
        }
    }

    /// Binds the session owner once, before any sample job can be submitted.
    pub(crate) fn bind(&self, sessions: Arc<SampleSessions>) -> Result<(), &'static str> {
        self.sessions
            .set(sessions)
            .map_err(|_| "sample session owner already bound")
    }
}

impl JobEvents for SampleEvents {
    fn publish(&self, event: JobEvent) {
        if let JobEvent::Finished { summary, .. } = &event
            && summary.kind_id == KIND
            && let Some(sessions) = self.sessions.get()
        {
            sessions.finished(summary.job_id.clone(), summary.state);
        }
        self.downstream.publish(event);
    }

    fn publish_history_changed(&self, change: HistoryChanged) {
        self.downstream.publish_history_changed(change);
    }

    fn retire_job(&self, id: &Id) {
        self.downstream.retire_job(id);
    }
}
