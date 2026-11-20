//! Immutable, bounded summaries bound to one result version.
use crate::{
    folder_evidence_api::{FolderAttemptInput, FolderAttemptStatus},
    folder_evidence_budget::FolderEvidenceLease,
};

/// Summary of one directory attempt in a pinned result version.
///
/// `EndedUnproven` records transport and child-terminal consistency only. It does not prove
/// filesystem-wide observation, absence of concurrent changes, or exact size completeness.
/// Parent rows do not imply that linked descendant attempts have ended; each attempt has its own row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FolderAttemptSummary {
    pub attempt_id: u64,
    pub root_index: u32,
    pub folder_item: u32,
    pub status: FolderAttemptStatus,
    pub event_watermark_exclusive: Option<u64>,
    pub child_count_exclusive: Option<u64>,
}

#[derive(Debug)]
pub(crate) struct FolderEvidenceSnapshot {
    summaries: Vec<FolderAttemptSummary>,
    events: Vec<PinnedFolderEvent>,
    _attempt_leases: Vec<FolderEvidenceLease>,
    _overhead_lease: FolderEvidenceLease,
}

#[derive(Debug)]
pub(crate) struct PinnedFolderEvent {
    pub(crate) attempt_id: u64,
    pub(crate) _event_seq: u64,
    /// `None` when the receiver did not keep the event's input.
    pub(crate) input: Option<FolderAttemptInput>,
}

impl FolderEvidenceSnapshot {
    pub(crate) fn new(
        summaries: Vec<FolderAttemptSummary>,
        events: Vec<PinnedFolderEvent>,
        attempt_leases: Vec<FolderEvidenceLease>,
        overhead_lease: FolderEvidenceLease,
    ) -> Self {
        Self {
            summaries,
            events,
            _attempt_leases: attempt_leases,
            _overhead_lease: overhead_lease,
        }
    }

    pub(crate) fn summaries(&self) -> &[FolderAttemptSummary] {
        &self.summaries
    }

    pub(crate) fn events(&self) -> &[PinnedFolderEvent] {
        &self.events
    }
}
