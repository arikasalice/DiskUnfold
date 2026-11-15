//! Records one scan-completion observation after a GUI scan ends.
//!
//! The row is read from the sealed result alone and written in its own transaction,
//! after the scan's F24 terminal record. A failure here never changes the scan.

use du_analysis_index::{IngestSink, ObservationSkip, ObservationStamp, SealedScanError};
use du_core_contracts::{
    Id,
    job::{JobState, JobSummary},
    scope_observation_log::{
        ScopeObservationAppend, ScopeObservationLog, ScopeObservationWriteError,
    },
};

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

/// What happened to one finished job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// A new row was stored.
    Recorded,
    /// The job already had its row.
    AlreadyRecorded,
    /// The job leaves no row.
    Skipped(Skip),
    /// The job should have a row but it was not stored.
    Failed(Failure),
}

/// Records the observation of a finished job, if it has one.
///
/// Only a GUI scan that ended done or partial with a sealed, still-current, real result
/// that has a collector observation time is recorded. The caller supplies the row identity
/// and the write time.
pub(crate) fn record(
    store: &IngestSink,
    log: &dyn ScopeObservationLog,
    summary: &JobSummary,
    observation_id: Id,
    created_at_ms: i64,
) -> Outcome {
    if summary.kind_id != SCAN_KIND {
        return Outcome::Skipped(Skip::NotScan);
    }
    let Some(version) = summary
        .result_version_id
        .as_ref()
        .filter(|_| matches!(summary.state, JobState::Done | JobState::Partial))
    else {
        return Outcome::Skipped(Skip::NoSealedResult);
    };
    let result = match store.sealed_scan_result(version) {
        Ok(result) => result,
        Err(SealedScanError::NotCurrent) => return Outcome::Skipped(Skip::Withdrawn),
        Err(SealedScanError::NotSealed) => return Outcome::Skipped(Skip::NoSealedResult),
        Err(SealedScanError::Unavailable) => return Outcome::Failed(Failure::ResultStore),
    };
    let stamp = ObservationStamp {
        observation_id,
        job_id: summary.job_id.clone(),
        created_at_ms,
    };
    let row = match result.scope_observation(stamp) {
        Ok(row) => row,
        Err(ObservationSkip::NotRealSource) => return Outcome::Skipped(Skip::SampleSource),
        Err(ObservationSkip::NoObservedAt) => return Outcome::Skipped(Skip::NoObservedAt),
        Err(other) => return Outcome::Failed(Failure::Conversion(other)),
    };
    match log.append(&row) {
        Ok(ScopeObservationAppend::Added) => Outcome::Recorded,
        Ok(ScopeObservationAppend::AlreadyRecorded) => Outcome::AlreadyRecorded,
        Err(error) => Outcome::Failed(Failure::Store(error)),
    }
}

#[cfg(test)]
mod tests;
