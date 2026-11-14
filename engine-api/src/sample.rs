//! F32 sample experience: one engine session over an embedded bundle (GUI only).
//!
//! The session module reads nothing outside the ports below and job submission.

use std::sync::Arc;

use du_analysis_index::SampleSessionKey;
use du_core_contracts::{
    Id,
    job::JobBody,
    sample_ipc::{SampleObservationRef, SampleStateChanged},
};
use du_job_runtime::RuntimeError;

use crate::{api::EngineBuilder, workers::Workers};

mod commands;
mod embedded;
mod events;
mod jobs;
mod session;
#[cfg(test)]
mod tests;

pub use embedded::EmbeddedSampleLoader;
pub use events::SampleEvents;
pub(crate) use jobs::SampleJobs;
pub(crate) use session::SampleSessions;

/// Job kind of the sample load job.
pub const KIND: &str = du_scan_engine::sample_job::SAMPLE_JOB_KIND;

/// Facts about a bundle that passed validation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SampleBundleInfo {
    /// Version label of the embedded bundle; part of the load job's deduplication key.
    pub bundle_version: String,
}

/// Owns the embedded bundle and the sample results built from it.
pub trait SampleLoader: Send + Sync {
    /// Validates the embedded bundle; `Err` carries a `sample.*` unavailable reason key.
    ///
    /// Called before any job is submitted, so an invalid bundle never reaches the runtime.
    fn open(&self) -> Result<SampleBundleInfo, &'static str>;

    /// Builds the `scan.sample` job body for one session.
    ///
    /// The body records each sealed observation into the returned report before it finishes.
    fn job(&self, session: &SampleSessionKey) -> (Arc<dyn JobBody>, Arc<dyn SampleLoadReport>);

    /// Releases every result version of the session and returns how many were released.
    ///
    /// Repeating the call for a released session returns 0.
    fn release(&self, session: &SampleSessionKey) -> u64;
}

/// Observations sealed by one load job.
pub trait SampleLoadReport: Send + Sync {
    /// Returns the sealed observations in bundle order; read after the job finished as done.
    fn observations(&self) -> Vec<SampleObservationRef>;
}

/// Receives each committed `sample.state_changed` body, in sequence order.
pub trait SampleEventSink: Send + Sync {
    /// Delivers one event; the session state is committed before this call.
    fn publish(&self, body: SampleStateChanged, job_ref: Option<Id>);
}

/// Registers the scan-engine job descriptor and the four GUI-only sample use cases.
///
/// Returns the session owner, which must be bound to the runtime's [`SampleEvents`] so
/// that `scan.sample` terminal events reach it.
pub(crate) fn register(
    builder: &mut EngineBuilder,
    workers: Arc<Workers>,
    loader: Arc<dyn SampleLoader>,
    events: Arc<dyn SampleEventSink>,
) -> Result<Arc<SampleSessions>, RuntimeError> {
    workers
        .runtime
        .register(du_scan_engine::sample_job::descriptor())?;
    Ok(register_with(builder, workers, loader, events))
}

fn register_with(
    builder: &mut EngineBuilder,
    jobs: Arc<dyn SampleJobs>,
    loader: Arc<dyn SampleLoader>,
    events: Arc<dyn SampleEventSink>,
) -> Arc<SampleSessions> {
    let sessions = Arc::new(SampleSessions::new(jobs, loader, events));
    commands::register(builder, &sessions);
    sessions
}
