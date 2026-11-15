//! What a scan-completion observation means.
//!
//! The row is read from one sealed result version only: the scope-wide totals, the
//! version's completeness and observation time, and the size rule that produced them.
//! Nothing here looks at the file system again.

use std::sync::Arc;

use du_core_contracts::{
    Id, SizeAmount, SizeBasis, SizeValue,
    result::ResultVersionRead,
    scope_observation_log::{
        ObservedAmount, ObservedSize, SCOPE_FINGERPRINT_FORMAT_V1, ScopeObservation,
        ScopeObservationOrigin,
    },
};

use crate::{IngestSink, ResultVersion, origin::ResultOrigin};

/// Values the recorder supplies for the new row.
#[derive(Clone, Debug)]
pub struct ObservationStamp {
    /// UUID v7 identifier of the new row.
    pub observation_id: Id,
    /// The scan job that produced the sealed result.
    pub job_id: Id,
    /// Time the row is written, in Unix milliseconds.
    pub created_at_ms: i64,
}

/// Why a result version yields no observation. None of these is a failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservationSkip {
    /// The result was not observed from the user's storage.
    NotRealSource,
    /// The collector supplied no observation time. The job's end time is not a substitute.
    NoObservedAt,
    /// The result carries no scope fingerprint.
    NoFingerprint,
    /// The fingerprint is not 64 lowercase hexadecimal characters.
    MalformedFingerprint,
    /// A scope total is recorded for a different size basis than its column.
    InconsistentBasis,
}

/// Why a version cannot be taken as the sealed result of a finished scan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SealedScanError {
    /// The version is not the current result of its lineage: it was withdrawn, replaced, or
    /// never existed.
    NotCurrent,
    /// The version is an interim publication of a scan that did not seal it.
    NotSealed,
    /// The result store could not be read.
    Unavailable,
}

/// Converts a scope fingerprint in its text form to the 32 bytes stored for it.
///
/// Only 64 lowercase hexadecimal characters are accepted. Anything else returns `None`
/// and is never repaired or guessed.
pub fn fingerprint_from_hex(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    let mut bytes = [0u8; 32];
    let (pairs, _) = text.as_bytes().as_chunks::<2>();
    for (byte, [high, low]) in bytes.iter_mut().zip(pairs) {
        *byte = (digit(*high)? << 4) | digit(*low)?;
    }
    Some(bytes)
}

fn digit(character: u8) -> Option<u8> {
    match character {
        b'0'..=b'9' => Some(character - b'0'),
        b'a'..=b'f' => Some(character - b'a' + 10),
        _ => None,
    }
}

impl ResultVersion {
    /// Builds the observation row for this sealed scan result.
    ///
    /// The scope fingerprint is the one frozen into the result when the scan was admitted,
    /// converted to its 32-byte form for key format one. The totals are the scope-wide
    /// aggregate with their original state and evidence: an unknown total stays unknown,
    /// a lower bound stays a lower bound, and a partial result keeps its completeness.
    ///
    /// # Errors
    /// Returns the reason when this result must not produce a row.
    pub fn scope_observation(
        &self,
        stamp: ObservationStamp,
    ) -> Result<ScopeObservation, ObservationSkip> {
        if !self.data_source.is_real() {
            return Err(ObservationSkip::NotRealSource);
        }
        let observed_at = self.observed_at().ok_or(ObservationSkip::NoObservedAt)?;
        let fingerprint = self
            .scope_fingerprint()
            .ok_or(ObservationSkip::NoFingerprint)?;
        let scope_fingerprint =
            fingerprint_from_hex(fingerprint).ok_or(ObservationSkip::MalformedFingerprint)?;
        let total = self.scope_total();
        let rule = self.size_rule();
        Ok(ScopeObservation {
            observation_id: stamp.observation_id,
            scope_fingerprint,
            fingerprint_format: SCOPE_FINGERPRINT_FORMAT_V1,
            observed_at_ms: observed_at.unix_millis(),
            job_id: stamp.job_id,
            origin: match self.origin {
                ResultOrigin::Scan => ScopeObservationOrigin::Scan,
                ResultOrigin::Incremental => ScopeObservationOrigin::Incremental,
            },
            completeness: self.completeness(),
            size_rule_id: rule.id.to_owned(),
            size_rule_revision: rule.revision,
            logical: observed(&total.logical, SizeBasis::Logical)?,
            allocated: observed(&total.allocated, SizeBasis::Allocated)?,
            unique_physical: observed(&total.unique_physical, SizeBasis::UniquePhysical)?,
            created_at_ms: stamp.created_at_ms,
        })
    }
}

fn observed(value: &SizeValue, basis: SizeBasis) -> Result<ObservedSize, ObservationSkip> {
    if value.basis != basis {
        return Err(ObservationSkip::InconsistentBasis);
    }
    Ok(ObservedSize {
        amount: match &value.amount {
            SizeAmount::Exact { bytes } => ObservedAmount::Exact(bytes.get()),
            SizeAmount::LowerBound { bytes } => ObservedAmount::LowerBound(bytes.get()),
            SizeAmount::Unknown => ObservedAmount::Unknown,
        },
        evidence: value.evidence.clone(),
    })
}

impl IngestSink {
    /// Returns the sealed scan result for the exact `version` of a finished job.
    ///
    /// The result must still be the current one of its lineage. A withdrawn or replaced
    /// result, and an interim publication that the scan never sealed, are refused, so a
    /// caller never records totals the user can no longer see.
    pub fn sealed_scan_result(&self, version: &Id) -> Result<Arc<ResultVersion>, SealedScanError> {
        let state = self
            .state
            .lock()
            .map_err(|_| SealedScanError::Unavailable)?;
        let entry = state
            .current_entry(version)
            .map_err(|_| SealedScanError::NotCurrent)?;
        if !entry.sealed {
            return Err(SealedScanError::NotSealed);
        }
        Ok(Arc::clone(&entry.result))
    }
}
