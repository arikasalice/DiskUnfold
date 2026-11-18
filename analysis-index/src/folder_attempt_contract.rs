//! DRAFT, test-only model of the unresolved F02 to F03 directory-attempt handoff.
//! Identifier scopes and evidence ownership here are candidates, not a published contract.
//! F02 assigns event watermarks independently and carries the adapter EOF child count
//! without deriving it from admitted observations.
use crate::folder_marks::BoundaryKind;
use du_core_contracts::scan_observation::{FilterDecision, ObservedKind};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct DirectoryAttemptKey {
    pub(super) build_id: u64,
    pub(super) root_index: u32,
    pub(super) raw_parent: Vec<Vec<u8>>,
    pub(super) attempt_id: u64,
}

/// Provenance of the filter callback relative to metadata collection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CallbackPhase {
    BeforeMetadata,
    AfterMetadata,
    Unknown,
}

/// Optional attempt capability; omission is interpreted as Unknown for old producers.
/// `StableChildId` is synthetic test input here, not proof. Production must obtain
/// this capability from an approved adapter-owned contract and its validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChildCorrelationCapability {
    Unknown,
    StableChildId,
}

/// Draft scopes: callback_id is dense per attempt; child_id is dense per direct-child set.
/// event_seq is separate and orders every non-End transport frame for one attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CallbackSeen {
    pub(super) key: DirectoryAttemptKey,
    pub(super) event_seq: u64,
    pub(super) callback_id: u64,
    /// Zero-based per attempt, stable across callback and final only with an approved
    /// producer correlation guarantee; None means no proven join.
    pub(super) child_id: Option<u64>,
    pub(super) raw_name: Vec<u8>,
    /// Exclude is terminal here and is not followed by a ChildFinal frame.
    pub(super) decision: FilterDecision,
    pub(super) phase: CallbackPhase,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum DirectoryChildFinal {
    /// A selected child with its recorded kind and, for an entered directory, its attempt.
    Recorded {
        kind: ObservedKind,
        child_attempt_id: Option<u64>,
    },
    /// A selected directory whose traversal stopped at a known scope boundary.
    /// It remains distinct from an unread gap.
    Boundary { kind: BoundaryKind },
    /// A selected directory rejected as a revisit; this is not an unread gap.
    Revisit,
    /// The producer could not classify the direct child safely.
    Unresolved,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum AttemptEventPayload {
    Started {
        correlation: Option<ChildCorrelationCapability>,
    },
    CallbackSeen {
        callback_id: u64,
        child_id: Option<u64>,
        raw_name: Vec<u8>,
        decision: FilterDecision,
        phase: CallbackPhase,
    },
    ChildFinal {
        child_id: u64,
        classification: DirectoryChildFinal,
    },
    Progress,
    PendingCleared {
        child_id: u64,
    },
    MetadataError,
    TransportFailure,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SequencedAttemptEvent {
    /// Zero-based ordinal for every non-End frame in this attempt; not a child ID.
    pub(super) event_seq: u64,
    pub(super) payload: AttemptEventPayload,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum DirectoryAttemptEvent {
    Started {
        key: DirectoryAttemptKey,
        event_seq: u64,
        correlation: Option<ChildCorrelationCapability>,
    },
    CallbackSeen(CallbackSeen),
    ChildFinal {
        key: DirectoryAttemptKey,
        event_seq: u64,
        child_id: u64,
        classification: DirectoryChildFinal,
    },
    Progress {
        key: DirectoryAttemptKey,
        event_seq: u64,
    },
    PendingCleared {
        key: DirectoryAttemptKey,
        event_seq: u64,
        child_id: u64,
    },
    MetadataError {
        key: DirectoryAttemptKey,
        event_seq: u64,
    },
    TransportFailure {
        key: DirectoryAttemptKey,
        event_seq: u64,
    },
    Cancelled {
        key: DirectoryAttemptKey,
        event_seq: u64,
    },
    /// End is a barrier, not a numbered frame. Its watermark counts prior frames.
    /// This count copies the adapter's EOF child_count_exclusive, including excluded
    /// and undecidable IDs; it is never derived from admitted observations.
    End {
        key: DirectoryAttemptKey,
        event_watermark_exclusive: u64,
        callback_watermark_exclusive: u64,
        child_count_exclusive: u64,
    },
}
