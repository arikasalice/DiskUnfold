//! Test-only hooks for dependent crates, compiled with the `test-hooks` feature.
//!
//! These hooks exist only for tests. Production code must never call them, and no production
//! crate may enable the `test-hooks` feature in its normal dependencies; enable it only from
//! `[dev-dependencies]`.
use crate::{IngestSink, folder_evidence_budget::FolderEvidenceLease};

/// Holds all remaining folder-evidence budget of a sink until it is dropped.
///
/// Test only: production code must not create or hold it.
#[must_use = "the budget is released when the hold is dropped"]
pub struct FolderEvidenceBudgetHold {
    _lease: FolderEvidenceLease,
}

impl IngestSink {
    /// Reserves the remaining folder-evidence budget so the next evidence snapshot fails
    /// with `CapacityExceeded` and the version publishes without folder evidence.
    ///
    /// Test only: production code must not call this. It starves every build of this sink.
    ///
    /// Returns `None` when nothing remains to reserve.
    pub fn hold_folder_evidence_budget(&self) -> Option<FolderEvidenceBudgetHold> {
        self.folder_evidence_budget
            .reserve_remaining()
            .map(|lease| FolderEvidenceBudgetHold { _lease: lease })
    }
}
