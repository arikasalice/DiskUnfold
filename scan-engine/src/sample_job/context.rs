//! Job context that stamps one observation point with the bundle's own time.
use std::{sync::Arc, time::Duration};

use du_core_contracts::{
    Timestamp,
    job::{Checkpoint, JobContext, JobCounts, JobFlow, JobFlowError},
};

/// Wraps the runtime context for one observation point.
///
/// `utc_now` reports the observation time recorded in the bundle, so item, partial and seal
/// times never come from the wall clock. Cancellation is forwarded. Flows are local no-op
/// handles: the sample source performs no I/O, so no metadata permit is needed.
pub(super) struct SampleContext {
    inner: Arc<dyn JobContext>,
    observed_at: Timestamp,
}

impl SampleContext {
    pub(super) fn new(inner: Arc<dyn JobContext>, observed_at: Timestamp) -> Self {
        Self { inner, observed_at }
    }
}

struct LocalFlow {
    inner: Arc<dyn JobContext>,
}

impl JobFlow for LocalFlow {
    fn safe_point(&self) -> Checkpoint {
        self.inner.checkpoint()
    }

    fn report_item(&self, _payload: &[u8]) -> Result<(), JobFlowError> {
        Ok(())
    }

    fn finish(self: Box<Self>, _counts: JobCounts) {}
}

impl JobContext for SampleContext {
    fn checkpoint(&self) -> Checkpoint {
        self.inner.checkpoint()
    }

    fn abort_flows(&self) {
        self.inner.abort_flows();
    }

    fn utc_now(&self) -> Option<Timestamp> {
        Some(self.observed_at.clone())
    }

    fn monotonic_now(&self) -> Option<Duration> {
        self.inner.monotonic_now()
    }

    fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
        Ok(Box::new(LocalFlow {
            inner: Arc::clone(&self.inner),
        }))
    }
}
