//! Snapshot file format-version lookup shared across crate boundaries (FR8.HD1.snapshot-files.DD1).
//!
//! The snapshot store reads the format version from a file header and asks a registry, filled by
//! the composition root, whether that version may be read. A version the registry does not know
//! makes only that file unreadable; the file is never rewritten and other files are not affected.

/// Stable reason key for a snapshot file whose header names a format version without a reader.
pub const SNAPSHOT_FORMAT_UNKNOWN: &str = "snapshot.format_unknown";

/// The version a file header named and no reader is registered for.
///
/// It carries the two values the design's decision consists of: the version number and the fact
/// that it is not registered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormatUnknown {
    /// Format version read from the file header.
    pub version: u16,
}

impl FormatUnknown {
    /// Returns the reason key for the error envelope built at the application boundary.
    pub const fn reason_key(self) -> &'static str {
        SNAPSHOT_FORMAT_UNKNOWN
    }
}

/// Format versions that this build may read.
///
/// Implementations are read-only and cheap; the snapshot store calls them once per opened file.
pub trait SnapshotFormatRegistry {
    /// Returns whether a reader is registered for `version`.
    fn is_registered(&self, version: u16) -> bool;
}
