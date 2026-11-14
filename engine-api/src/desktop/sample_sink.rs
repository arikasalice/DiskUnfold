//! Publishes sample session events through the desktop event stream.

use std::sync::Arc;

use du_analysis_index::IngestSink;
use du_core_contracts::{
    ENVELOPE_VERSION, EventEnvelope, Id,
    sample_ipc::{SAMPLE_STATE_CHANGED, SampleStateChanged},
};
use du_job_runtime::JobIdentity;

use super::DesktopEvents;
use crate::{
    api::EngineBuilder,
    sample::{self, EmbeddedSampleLoader, SampleEventSink, SampleEvents},
    workers::Workers,
};

/// Publishes `sample.state_changed` through the desktop event stream.
pub(super) struct DesktopSampleSink(Arc<DesktopEvents>);

impl DesktopSampleSink {
    /// Wraps the desktop event source that assigns the envelope sequence.
    fn new(events: Arc<DesktopEvents>) -> Self {
        Self(events)
    }
}

impl SampleEventSink for DesktopSampleSink {
    fn publish(&self, body: SampleStateChanged, job_ref: Option<Id>) {
        let occurred_at = body.observed_at.clone();
        let Ok(body) = serde_json::to_value(&body) else {
            return;
        };
        self.0.publish_host_event(EventEnvelope {
            topic: SAMPLE_STATE_CHANGED.into(),
            contract_version: ENVELOPE_VERSION,
            seq: 0,
            correlation_id: None,
            job_ref,
            result_version: None,
            occurred_at,
            body,
        });
    }
}

/// Registers the sample use cases over the embedded bundle and the shared result `store`.
///
/// Binds the session owner to `events` so that `scan.sample` terminal events reach it.
pub(super) fn bind(
    events: &SampleEvents,
    builder: &mut EngineBuilder,
    workers: &Arc<Workers>,
    store: IngestSink,
    ids: Arc<dyn JobIdentity>,
    desktop: &Arc<DesktopEvents>,
) -> Result<(), Box<dyn std::error::Error>> {
    let loader = Arc::new(EmbeddedSampleLoader::new(store, ids));
    let sink = Arc::new(DesktopSampleSink::new(desktop.clone()));
    events.bind(sample::register(builder, workers.clone(), loader, sink)?)?;
    Ok(())
}
