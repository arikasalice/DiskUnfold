//! The loader backed by the bundle embedded in the executable.

use std::sync::{Arc, Mutex, PoisonError};

use du_analysis_index::{IngestSink, SampleSessionKey};
use du_core_contracts::{
    Id,
    job::{JobBody, JobContext, JobCounts, JobOutcome},
    sample_ipc::{
        SAMPLE_BUNDLE_INVALID, SAMPLE_FORMAT_UNSUPPORTED, SAMPLE_LOAD_FAILED, SAMPLE_MISSING,
        SampleObservationRef,
    },
};
use du_job_runtime::JobIdentity;
use du_scan_engine::{
    sample_bundle::{BundleFailure, ValidatedBundle, embedded::EmbeddedSampleBundle},
    sample_job::{SAMPLE_JOB_KIND, SampleLoad, SampleLoadReport as LoadRecord, SampleLoadTarget},
    scan::ResultIds,
};

use super::{SampleBundleInfo, SampleLoadReport, SampleLoader};

/// Maps a bundle validation failure to the `sample.*` reason the session reports.
///
/// This is the single place where scan-engine failures become unavailable reason keys.
pub(crate) fn unavailable_reason(failure: BundleFailure) -> &'static str {
    match failure {
        BundleFailure::Missing => SAMPLE_MISSING,
        BundleFailure::UnsupportedFormat => SAMPLE_FORMAT_UNSUPPORTED,
        BundleFailure::Corrupt => SAMPLE_BUNDLE_INVALID,
    }
}

/// Loads the embedded bundle into sample result versions of the shared result store.
///
/// `open` validates the bundle and keeps it for the jobs of the session; every reset
/// validates again through `open`.
pub struct EmbeddedSampleLoader {
    sink: IngestSink,
    ids: Arc<IdSource>,
    validated: Mutex<Option<Arc<ValidatedBundle>>>,
}

struct IdSource(Arc<dyn JobIdentity>);

impl ResultIds for IdSource {
    fn next_id(&self) -> Id {
        self.0.next_id()
    }
}

impl EmbeddedSampleLoader {
    /// Builds a loader that publishes into `sink`, the store the scan use cases share.
    pub fn new(sink: IngestSink, ids: Arc<dyn JobIdentity>) -> Self {
        Self {
            sink,
            ids: Arc::new(IdSource(ids)),
            validated: Mutex::new(None),
        }
    }

    fn validated(&self) -> Result<Arc<ValidatedBundle>, &'static str> {
        let mut slot = self
            .validated
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(bundle) = slot.as_ref() {
            return Ok(bundle.clone());
        }
        let bundle = Arc::new(EmbeddedSampleBundle::validate().map_err(unavailable_reason)?);
        *slot = Some(bundle.clone());
        Ok(bundle)
    }
}

impl SampleLoader for EmbeddedSampleLoader {
    fn open(&self) -> Result<SampleBundleInfo, &'static str> {
        let bundle = Arc::new(EmbeddedSampleBundle::validate().map_err(unavailable_reason)?);
        let bundle_version = bundle.header().bundle_version.clone();
        *self
            .validated
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(bundle);
        Ok(SampleBundleInfo { bundle_version })
    }

    fn job(&self, session: &SampleSessionKey) -> (Arc<dyn JobBody>, Arc<dyn SampleLoadReport>) {
        let record = Arc::new(LoadRecord::default());
        let report: Arc<dyn SampleLoadReport> = Arc::new(Report(record.clone()));
        let Ok(bundle) = self.validated() else {
            // `open` just validated the bundle, so this is unreachable in practice.
            return (Arc::new(FailedLoad), report);
        };
        let target = SampleLoadTarget {
            sink: self.sink.clone(),
            session: session.clone(),
            scope_version_id: self.ids.next_id(),
            ids: self.ids.clone(),
        };
        let body: Arc<dyn JobBody> = Arc::new(SampleLoad::new(bundle, target, record));
        (body, report)
    }

    fn release(&self, session: &SampleSessionKey) -> u64 {
        self.sink.release_sample_session(session).unwrap_or(0)
    }
}

struct Report(Arc<LoadRecord>);

impl SampleLoadReport for Report {
    fn observations(&self) -> Vec<SampleObservationRef> {
        self.0
            .observations()
            .into_iter()
            .map(
                |(index, result_version, observed_at)| SampleObservationRef {
                    index,
                    result_version,
                    observed_at,
                },
            )
            .collect()
    }
}

/// Stands in for the load body when no validated bundle is available; it only fails.
struct FailedLoad;

impl JobBody for FailedLoad {
    fn kind_id(&self) -> &str {
        SAMPLE_JOB_KIND
    }

    fn run(&self, _: Arc<dyn JobContext>) -> JobOutcome {
        JobOutcome {
            counts: JobCounts::default(),
            completed_scope: false,
            failure_reason_key: Some(SAMPLE_LOAD_FAILED.into()),
            unprocessed_summary: None,
            result_version_id: None,
        }
    }
}
