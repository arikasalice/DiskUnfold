//! Stored state of one registered folder attempt, shared by the receiver and validation.
use crate::{
    folder_evidence_api::{
        FolderAttemptError, FolderAttemptInput, FolderAttemptStatus, FolderAttemptToken,
    },
    folder_evidence_budget::{FolderEvidenceLease, FolderScopeHandle},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EndRecord {
    pub(crate) event_watermark_exclusive: u64,
    pub(crate) child_count_exclusive: u64,
    pub(crate) outcome: Result<(), FolderAttemptError>,
}

pub(crate) struct StoredAttemptEvent {
    pub(crate) event_seq: u64,
    pub(crate) input: Option<FolderAttemptInput>,
    pub(crate) _payload_lease: Option<FolderEvidenceLease>,
}

pub(crate) struct AttemptState {
    pub(crate) token: FolderAttemptToken,
    pub(crate) status: FolderAttemptStatus,
    pub(crate) events: Vec<StoredAttemptEvent>,
    pub(crate) end: Option<EndRecord>,
    pub(crate) event_capacity_lease: Option<FolderEvidenceLease>,
    pub(crate) payload_bytes: usize,
    pub(crate) scope: FolderScopeHandle,
}
