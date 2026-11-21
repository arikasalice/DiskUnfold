//! F11 identity-safe trash operations and session-only undo ports.
//! Design items: FR1.HD2.app-host.DD2, FR2.HD1.app-host.DD2,
//! FR3.HD1.app-host.DD2, FR6.HD1.app-host.DD2, FR6.HD2.app-host.DD1,
//! FR6.HD2.app-host.DD2, FR14.HD1.app-host.DD1.
//!
//! F11 FR4 requires cancellable child-enumeration batches with mount and
//! permission markers, but does not publish the batch continuation shape.
//! No new child-enumeration method is defined until that shape is specified.

use crate::{
    Id, ItemRef, OsPath, Timestamp,
    job::{HistoryMode, ResourceClass},
    ports::{EntryKind, PortError},
    values::ByteSize,
    volume::Capability,
};

/// Registered F11 change command (FR1.HD2.app-host.DD2).
pub const CHANGE_TRASH: &str = "change.trash";
/// Capability checked before any F11 OS change call (FR2.HD1.app-host.DD2).
pub const CHANGE_TRASH_IDENTITY_BOUND: &str = "change.trash_identity_bound";
/// F11 records each changed item through job history (FR1.HD2.app-host.DD3).
pub const CHANGE_TRASH_HISTORY_MODE: HistoryMode = HistoryMode::PerItem;
/// F11 uses the file-change scheduler resource class (FR1.HD2.app-host.DD3).
pub const CHANGE_TRASH_RESOURCE_CLASS: ResourceClass = ResourceClass::FileChange;

/// Identity bytes observed from one verified volume and file (FR3.HD1.app-host.DD2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeIdentity {
    /// Native volume identity bytes; never inferred from a display path.
    pub volume_id: Vec<u8>,
    /// Native file identity bytes returned by the same no-follow observation.
    pub file_id: Vec<u8>,
    /// Adapter-native change token, absent when that evidence is unavailable.
    pub change_token: Option<Vec<u8>>,
}

/// Metadata observed without following a link, tied to `identity` (FR3.HD1.app-host.DD2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeItemObservation {
    pub identity: ChangeIdentity,
    pub kind: EntryKind,
    /// None means the size was not established.
    pub size: Option<ByteSize>,
    /// None means the modification time was not established.
    pub modified_at: Option<Timestamp>,
}

/// Reads item identity and metadata without resolving a symbolic link target (FR3.HD1.app-host.DD2).
pub trait ItemIdentityPort: Send + Sync {
    /// Resolves the selected path without following links and returns facts
    /// from the same verified object observation.
    fn lookup_no_follow(&self, path: &OsPath) -> Result<ChangeItemObservation, PortError>;
}

/// Opaque reference assigned by the operating-system adapter to a trash entry (FR6.HD1.app-host.DD2).
#[derive(Clone, PartialEq, Eq)]
pub struct TrashLocationRef(String);

impl std::fmt::Debug for TrashLocationRef {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("TrashLocationRef(<opaque>)")
    }
}

impl TrashLocationRef {
    /// Stores an adapter-provided opaque value without interpreting it.
    pub fn from_adapter(value: String) -> Self {
        Self(value)
    }

    /// Returns the opaque value to the adapter that owns its meaning.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why an identity-bound trash operation was refused before moving an item (FR14.HD1.app-host.DD1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrashRefusal {
    Unsupported,
    IdentityChanged,
}

/// Result of one identity-bound trash move attempt (FR6.HD1.app-host.DD2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrashMoveOutcome {
    Moved {
        identity_after_move: ChangeIdentity,
        /// None means the OS did not provide a stable opaque trash reference.
        trash_location: Option<TrashLocationRef>,
    },
    Refused(TrashRefusal),
    OsError(PortError),
    OutcomeUnknown,
}

/// Read-only outcome after an interrupted or ambiguous trash move (FR6.HD1.app-host.DD2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrashOutcomeQuery {
    Moved {
        identity: ChangeIdentity,
        /// None means the OS did not provide a stable opaque trash reference.
        trash_location: Option<TrashLocationRef>,
    },
    NotMoved,
    OutcomeUnknown,
}

/// Canonical architecture change operations port, shared by OS adapters (FR6.HD1.app-host.DD2).
///
/// `trash_identity_bound` is supported only if the adapter binds the final
/// identity check to the actual OS move. A path recheck followed by a
/// path-based move does not satisfy this port (F11 FR14.HD1.app-host.DD1).
pub trait ChangeOpsPort: ItemIdentityPort + Send + Sync {
    /// Returns a per-volume capability and its stable reason key.
    fn capability(&self, volume_id: &[u8], capability: &str) -> Capability;

    /// Moves only the expected identity to trash; adapters must not substitute
    /// a permanent delete or a different change operation.
    fn trash_identity_bound(&self, path: &OsPath, expected: &ChangeIdentity) -> TrashMoveOutcome;

    /// Re-queries the original and trash locations without changing files.
    fn query_trash_outcome(
        &self,
        original_path: &OsPath,
        expected: &ChangeIdentity,
        prior_location: Option<&TrashLocationRef>,
    ) -> Result<TrashOutcomeQuery, PortError>;
}

/// Session-only restoration information for one trashed plan item (FR6.HD2.app-host.DD1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrashedUndoInfo {
    /// Position in the immutable plan version.
    pub item_no: u32,
    /// Version-scoped identity shown to the result-page consumer.
    pub item_ref: ItemRef,
    /// Original OS path retained only for this app session.
    pub original_path: OsPath,
    /// Identity captured before the move and verified after it.
    pub identity: ChangeIdentity,
    pub kind: EntryKind,
    /// None means the OS did not provide a reliable size.
    pub size: Option<ByteSize>,
    /// None means the OS did not provide a reliable modification time.
    pub modified_at: Option<Timestamp>,
    /// None means the item has no child fingerprint or it was unavailable.
    pub children_fingerprint: Option<String>,
    /// None means the original parent identity was unavailable.
    pub parent_identity: Option<ChangeIdentity>,
    /// None means the OS did not return an opaque trash location reference.
    pub trash_location: Option<TrashLocationRef>,
    pub moved_at: Timestamp,
}

/// App-host port for session-only undo registration and lookup (FR6.HD2.app-host.DD1).
///
/// The executor calls `register_trashed` once at job finalization. A failed or
/// over-limit registration leaves the affected items unavailable for undo and
/// does not alter their terminal outcomes. The records remain in process
/// memory for the current app session.
pub trait SessionUndoRead: Send + Sync {
    /// Registers the completed or identity-race items from one job as a batch.
    fn register_trashed(&self, job_ref: &Id, items: &[TrashedUndoInfo]) -> Result<(), PortError>;

    /// Reports whether this session still has undo information for one item.
    fn contains(&self, job_ref: &Id, item_no: u32) -> Result<bool, PortError>;

    /// Reads the undo information for one item; None means it is not registered.
    fn get_trashed(&self, job_ref: &Id, item_no: u32)
    -> Result<Option<TrashedUndoInfo>, PortError>;

    /// Removes one item after its undo operation is complete; not_found means absent.
    fn remove_trashed(&self, job_ref: &Id, item_no: u32) -> Result<(), PortError>;

    /// Clears all undo information when the application session ends.
    fn clear(&self) -> Result<(), PortError>;
}
