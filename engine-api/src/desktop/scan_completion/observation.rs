//! Records one scan-completion observation after a GUI scan ends.
//!
//! The row is read from the sealed result alone, on the delivery thread at the moment the
//! job's end arrives, so a result withdrawn or replaced later is not mistaken for current.
//! The [`writer`] stores it in its own transaction off that thread, after the scan's F24
//! terminal record. A failure here never changes the scan.

use du_analysis_index::{IngestSink, ObservationSkip, ObservationStamp, SealedScanError};
use du_core_contracts::{
    Id,
    job::{JobState, JobSummary},
    scope_observation_log::{ScopeObservation, ScopeObservationWriteError},
};

pub(super) mod writer;

/// The kind of job whose completion is recorded.
const SCAN_KIND: &str = "scan.metadata";

/// Why a finished job leaves no observation. None of these is an error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Skip {
    /// Another kind of job, such as a sample load.
    NotScan,
    /// The job failed or was cancelled, or it ended without a sealed result.
    NoSealedResult,
    /// The result was withdrawn or replaced before the job's end was delivered.
    Withdrawn,
    /// The result is not from the user's storage.
    SampleSource,
    /// The result carries no collector observation time.
    NoObservedAt,
}

/// Why an observation that should exist could not be stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    /// The result store could not be read.
    ResultStore,
    /// The sealed result gave a value that cannot be stored; nothing was guessed.
    Conversion(ObservationSkip),
    /// The storage port refused the row.
    Store(ScopeObservationWriteError),
}

impl Failure {
    /// A value-free key for the diagnostic line.
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Self::ResultStore => "result_store_unavailable",
            Self::Conversion(ObservationSkip::NoFingerprint) => "fingerprint_missing",
            Self::Conversion(ObservationSkip::MalformedFingerprint) => "fingerprint_malformed",
            Self::Conversion(_) => "result_not_convertible",
            Self::Store(ScopeObservationWriteError::Busy) => "db.busy",
            Self::Store(ScopeObservationWriteError::DiskFull) => "db.disk_full",
            Self::Store(ScopeObservationWriteError::Invalid) => "value_not_storable",
            Self::Store(ScopeObservationWriteError::Storage) => "storage_failed",
        }
    }
}

/// What a finished job yields before anything is stored.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Prepared {
    /// The row to store.
    Row(Box<ScopeObservation>),
    /// The job leaves no row.
    Skipped(Skip),
    /// The job should have a row but it could not be made.
    Failed(Failure),
}

/// Makes the observation row of a finished job, if it has one.
///
/// Only a GUI scan that ended done or partial with a sealed, still-current, real result
/// that has a collector observation time has a row. This reads the result store and never
/// the database, so the delivery thread may call it. The caller supplies the row identity
/// and its creation time.
pub(crate) fn prepare(
    store: &IngestSink,
    summary: &JobSummary,
    observation_id: Id,
    created_at_ms: i64,
) -> Prepared {
    if summary.kind_id != SCAN_KIND {
        return Prepared::Skipped(Skip::NotScan);
    }
    let Some(version) = summary
        .result_version_id
        .as_ref()
        .filter(|_| matches!(summary.state, JobState::Done | JobState::Partial))
    else {
        return Prepared::Skipped(Skip::NoSealedResult);
    };
    let result = match store.sealed_scan_result(version) {
        Ok(result) => result,
        Err(SealedScanError::NotCurrent) => return Prepared::Skipped(Skip::Withdrawn),
        Err(SealedScanError::NotSealed) => return Prepared::Skipped(Skip::NoSealedResult),
        Err(SealedScanError::Unavailable) => return Prepared::Failed(Failure::ResultStore),
    };
    let stamp = ObservationStamp {
        observation_id,
        job_id: summary.job_id.clone(),
        created_at_ms,
    };
    match result.scope_observation(stamp) {
        Ok(row) => Prepared::Row(Box::new(row)),
        Err(ObservationSkip::NotRealSource) => Prepared::Skipped(Skip::SampleSource),
        Err(ObservationSkip::NoObservedAt) => Prepared::Skipped(Skip::NoObservedAt),
        Err(other) => Prepared::Failed(Failure::Conversion(other)),
    }
}

#[cfg(test)]
mod tests;
