//! Terminal scan evidence retained for the lifetime of the current publication.

use crate::{IngestSink, QueryError, ResultVersion};
use du_core_contracts::Id;
use std::sync::Arc;

/// A successful terminal state reported by the scan's job owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanCompletion {
    Done,
    Partial,
}

/// An exact current scan publication pinned after successful completion.
#[derive(Debug)]
pub struct CompletedScan {
    version: Arc<ResultVersion>,
    completion: ScanCompletion,
}

impl CompletedScan {
    /// Keeps the admitted scan observations stable after later publications.
    pub fn version(&self) -> &Arc<ResultVersion> {
        &self.version
    }

    /// Returns the terminal job state independently of result completeness.
    pub fn completion(&self) -> ScanCompletion {
        self.completion
    }
}

/// An unconfirmed publication or an exact-version lookup failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompletedScanError {
    Unconfirmed,
    Lookup(QueryError),
}

impl IngestSink {
    /// Confirms a sealed scan from the trusted terminal event for this exact ID.
    ///
    /// The completion remains attached to the store entry even if a later scan
    /// publishes an interim version and then restores its earlier baseline.
    pub fn observe_scan_terminal(
        &self,
        version: &Id,
        completion: Option<ScanCompletion>,
    ) -> Result<bool, QueryError> {
        let mut state = self.state.lock().map_err(|_| QueryError::Unavailable)?;
        let proof = state
            .awaiting_terminal
            .remove(version)
            .and_then(|proof| proof.upgrade());
        Ok(proof
            .zip(completion)
            .is_some_and(|(proof, completion)| proof.set(completion).is_ok()))
    }

    /// Pins a scan only while its exact revision is current and confirmed complete.
    pub fn pin_completed_scan(&self, expected: &Id) -> Result<CompletedScan, CompletedScanError> {
        let state = self
            .state
            .lock()
            .map_err(|_| CompletedScanError::Lookup(QueryError::Unavailable))?;
        let entry = state
            .current_entry(expected)
            .map_err(CompletedScanError::Lookup)?;
        let completion = entry
            .completion
            .as_ref()
            .and_then(|proof| proof.get())
            .copied()
            .ok_or(CompletedScanError::Unconfirmed)?;
        Ok(CompletedScan {
            version: Arc::clone(&entry.result),
            completion,
        })
    }
}
