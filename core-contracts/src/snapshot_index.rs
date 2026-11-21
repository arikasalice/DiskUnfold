//! Snapshot metadata rows and the narrow index port used by snapshot-store.

use crate::Id;

/// Where the sealed snapshot came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotIndexSourceKind {
    /// Captured by this application.
    Local,
    /// Imported from another source.
    Imported,
}

/// Completeness recorded in the sealed metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotIndexCompleteness {
    /// All requested observations were available.
    Complete,
    /// Some observations were unavailable.
    Partial,
    /// The observation was missing.
    Missing,
}

/// Lifecycle state of one snapshot index row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotIndexState {
    /// The index row refers to a registered snapshot file.
    Active,
    /// The user deleted the snapshot and its deletion record exists.
    Deleted,
    /// The snapshot file is no longer present.
    Removed,
}

/// Kind assigned to a volume identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotVolumeIdentityKind {
    /// A Windows volume GUID.
    Guid,
    /// A UUID identity.
    Uuid,
    /// A filesystem UUID.
    FsUuid,
    /// No identity kind has been assigned.
    None,
}

/// Capability of the volume identity observation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotVolumeIdentityCapability {
    /// The identity is supported by the current policy.
    Supported,
    /// The identity is only partially supported.
    Partial,
    /// The identity is unsupported.
    Unsupported,
    /// The identity capability is unknown.
    Unknown,
}

/// Snapshot row data supplied for registration and returned by an active list.
///
/// The sealed condition JSON and scope marker are preserved as provided. Debug
/// output redacts those values because they can contain user-specific data.
#[derive(Clone, PartialEq, Eq)]
pub struct SnapshotIndexRow {
    /// UUID v7 for this snapshot index row.
    pub snapshot_id: Id,
    /// Snapshot file name, without a filesystem path.
    pub file_name: String,
    /// UUID v7 of the sealed result version.
    pub source_version_id: Id,
    /// UUID v7 of the source lineage; a missing lineage must be reported upstream.
    pub lineage_id: Id,
    /// Origin of the snapshot.
    pub source_kind: SnapshotIndexSourceKind,
    /// Observation time in Unix milliseconds, absent when unknown.
    pub observed_at_ms: Option<i64>,
    /// Index row creation time in Unix milliseconds.
    pub created_at_ms: i64,
    /// Scope marker bytes, empty when sealed metadata had no marker.
    pub scope_label: Vec<u8>,
    /// Sealed `conditionSummary` JSON text, preserved as-is.
    pub condition_summary: String,
    /// Completeness from the sealed metadata, absent when unknown.
    pub completeness: Option<SnapshotIndexCompleteness>,
    /// Snapshot format version.
    pub format_version: u16,
    /// Sealed body size in bytes.
    pub body_size: i64,
    /// BLAKE3 digest of the sealed body.
    pub body_hash: [u8; 32],
    /// Lifecycle state; registration accepts only `Active` rows.
    pub state: SnapshotIndexState,
    /// Time of the last failed verification, when present.
    pub verify_failed_at_ms: Option<i64>,
    /// Last update time in Unix milliseconds.
    pub updated_at_ms: i64,
    /// Monotonic row version.
    pub row_version: i64,
}

impl std::fmt::Debug for SnapshotIndexRow {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SnapshotIndexRow")
            .field("snapshot_id", &self.snapshot_id)
            .field("file_name", &self.file_name)
            .field("source_version_id", &self.source_version_id)
            .field("lineage_id", &self.lineage_id)
            .field("source_kind", &self.source_kind)
            .field("observed_at_ms", &self.observed_at_ms)
            .field("created_at_ms", &self.created_at_ms)
            .field("scope_label", &"<redacted>")
            .field("condition_summary", &"<redacted>")
            .field("completeness", &self.completeness)
            .field("format_version", &self.format_version)
            .field("body_size", &self.body_size)
            .field("body_hash", &self.body_hash)
            .field("state", &self.state)
            .field("verify_failed_at_ms", &self.verify_failed_at_ms)
            .field("updated_at_ms", &self.updated_at_ms)
            .field("row_version", &self.row_version)
            .finish()
    }
}

/// One snapshot-to-volume row. Raw observed identity bytes are never interpreted or merged.
#[derive(Clone, PartialEq, Eq)]
pub struct SnapshotIndexVolume {
    /// Zero-based position in the sealed snapshot; 65535 is reserved and rejected.
    pub volume_index: u16,
    /// Raw observed identity bytes, or `None` when the identity is unknown.
    pub observed_volume_id: Option<Vec<u8>>,
    /// Identity kind recorded for this row.
    pub identity_kind: SnapshotVolumeIdentityKind,
    /// Identity capability recorded for this row.
    pub identity_capability: SnapshotVolumeIdentityCapability,
}

impl std::fmt::Debug for SnapshotIndexVolume {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SnapshotIndexVolume")
            .field("volume_index", &self.volume_index)
            .field(
                "observed_volume_id",
                &self.observed_volume_id.as_ref().map(|_| "<redacted>"),
            )
            .field("identity_kind", &self.identity_kind)
            .field("identity_capability", &self.identity_capability)
            .finish()
    }
}

/// A row and its volumes as returned by an active snapshot list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotIndexEntry {
    /// Snapshot index metadata.
    pub row: SnapshotIndexRow,
    /// Volume metadata stored with this snapshot.
    pub volumes: Vec<SnapshotIndexVolume>,
    /// Whether this snapshot is the current baseline.
    pub pinned: bool,
}

/// Position after which an active snapshot page should continue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotIndexCursor {
    /// List sequence observed by the page that produced this cursor.
    pub list_seq: i64,
    /// Observation time of the last listed row; `None` sorts after known times.
    pub observed_at_ms: Option<i64>,
    /// UUID v7 of the last listed row.
    pub snapshot_id: Id,
}

/// One page from the active snapshot index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotIndexPage {
    /// Rows ordered by observation time descending, unknown times last, then ID ascending.
    pub rows: Vec<SnapshotIndexEntry>,
    /// List sequence read in the same transaction as these rows.
    pub list_seq: i64,
    /// Cursor for another page, when more active rows remain.
    pub next_cursor: Option<SnapshotIndexCursor>,
}

/// Result of registering one snapshot row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotIndexRegisterResult {
    /// New or already-active snapshot ID.
    pub snapshot_id: Id,
    /// List sequence after the operation; unchanged for an already-active source.
    pub list_seq: i64,
    /// Whether this call inserted the row and advanced the list sequence.
    pub inserted: bool,
}

/// Why the snapshot index operation failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotIndexError {
    /// The database predates the snapshot index migration.
    UnsupportedSchema,
    /// The row, volume list or page limit cannot be stored or queried.
    InvalidInput,
    /// SQLite remained busy through the configured five-second timeout; no write was retried.
    Busy,
    /// A cursor was created against an older list sequence.
    StaleCursor,
    /// Stored values violate the snapshot index contract.
    Corrupt,
    /// A storage operation failed, including sequence overflow.
    Storage,
}

impl std::fmt::Display for SnapshotIndexError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::UnsupportedSchema => "snapshot index schema is unavailable",
            Self::InvalidInput => "snapshot index input is invalid",
            Self::Busy => "snapshot index database is busy",
            Self::StaleCursor => "snapshot index cursor is stale",
            Self::Corrupt => "snapshot index data is corrupt",
            Self::Storage => "snapshot index storage failed",
        })
    }
}

impl std::error::Error for SnapshotIndexError {}

/// Narrow storage interface used by snapshot-store for registration and active listing.
pub trait SnapshotIndexPort {
    /// Registers the active row, volumes and optional baseline atomically.
    ///
    /// A repeated active `source_version_id` returns the existing snapshot ID and
    /// current list sequence without changing the database. A successful new row
    /// advances the sequence exactly once. Implementations must not retry a busy
    /// operation; they return `Busy` after their configured timeout.
    fn register(
        &mut self,
        row: &SnapshotIndexRow,
        volumes: &[SnapshotIndexVolume],
        baseline_pinned_at_ms: Option<i64>,
    ) -> Result<SnapshotIndexRegisterResult, SnapshotIndexError>;

    /// Finds the active snapshot for a sealed result version.
    fn find_active_by_source(
        &mut self,
        source_version_id: &Id,
    ) -> Result<Option<Id>, SnapshotIndexError>;

    /// Lists at most 1,000 active rows and rejects cursors from an older list sequence.
    fn list_active(
        &mut self,
        limit: usize,
        after: Option<&SnapshotIndexCursor>,
    ) -> Result<SnapshotIndexPage, SnapshotIndexError>;
}
