//! Test-only hooks for dependent crates, compiled with the `test-hooks` feature.
use crate::{IngestSink, folder_evidence_budget::FolderEvidenceLease};

/// Holds all remaining folder-evidence budget of a sink until it is dropped.
#[must_use = "the budget is released when the hold is dropped"]
pub struct FolderEvidenceBudgetHold {
    _lease: FolderEvidenceLease,
}

impl IngestSink {
    /// Reserves the remaining folder-evidence budget so the next evidence snapshot fails
    /// with `CapacityExceeded` and the version publishes without folder evidence.
    ///
    /// Returns `None` when nothing remains to reserve.
    pub fn hold_folder_evidence_budget(&self) -> Option<FolderEvidenceBudgetHold> {
        self.folder_evidence_budget
            .reserve_remaining()
            .map(|lease| FolderEvidenceBudgetHold { _lease: lease })
    }
}
