//! Ports for staging and durably registering immutable snapshot files.

use crate::{
    Id,
    snapshot_spool::{CaptureSpool, SealedSpool},
};
use std::io::{self, ErrorKind, Read, Write};

/// A failure during private snapshot-file operations.
///
/// `StoreUnavailable` and `Unsupported` map to `snapshot.store_unavailable`.
/// `NotFound` maps to `snapshot.not_found`; `InsufficientSpace` maps to
/// `snapshot.insufficient_space`; `WriteFailed` maps to `snapshot.write_failed`.
/// `ReadFailed` is diagnostic only and must not be mapped to `snapshot.write_failed`.
/// The contained `ErrorKind` values are diagnostic only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotFileError {
    /// The configured snapshot directory could not be opened or created.
    StoreUnavailable,
    /// The directory's filesystem cannot register files without replacement.
    Unsupported,
    /// The final identifier-derived snapshot name does not exist.
    NotFound,
    /// Reading the opened final snapshot failed; the error kind is diagnostic only.
    ReadFailed(ErrorKind),
    /// The snapshot volume has less than the requested free space.
    InsufficientSpace,
    /// Staging failed before registration.
    WriteFailed(ErrorKind),
}

impl std::fmt::Display for SnapshotFileError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StoreUnavailable => formatter.write_str("snapshot store is unavailable"),
            Self::Unsupported => formatter.write_str("snapshot store lacks required operations"),
            Self::NotFound => formatter.write_str("registered snapshot was not found"),
            Self::ReadFailed(_) => formatter.write_str("registered snapshot read failed"),
            Self::InsufficientSpace => {
                formatter.write_str("snapshot volume has insufficient space")
            }
            Self::WriteFailed(_) => formatter.write_str("snapshot file staging failed"),
        }
    }
}

impl std::error::Error for SnapshotFileError {}

/// The outcome of publishing a staged snapshot file.
///
/// `WriteFailed` means no final name was registered. It maps to
/// `snapshot.write_failed`, including when the final name already exists; the
/// contained `ErrorKind` is diagnostic only.
/// `RegisteredNotDurable` means the final name was registered, but syncing the
/// snapshot directory failed. Startup reconciliation must report the file as
/// `snapshot.orphan_file` rather than deleting it. Callers must not add an index
/// row for this result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotPublishError {
    /// Registration did not happen; the temporary file is removed on drop.
    WriteFailed(ErrorKind),
    /// Registration happened, but the directory entry was not confirmed durable.
    RegisteredNotDurable(ErrorKind),
}

impl std::fmt::Display for SnapshotPublishError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WriteFailed(_) => formatter.write_str("snapshot file registration failed"),
            Self::RegisteredNotDurable(_) => {
                formatter.write_str("snapshot file registration durability is unknown")
            }
        }
    }
}

impl std::error::Error for SnapshotPublishError {}

/// Provides private snapshot-file operations used by local snapshot saves and reads.
pub trait SnapshotFilePort: Send + Sync {
    /// Opens or creates the private snapshots folder and checks its current save capability.
    ///
    /// The folder uses the platform's permission rules, including mode `0700` on Unix.
    /// Failure to open or create it returns `StoreUnavailable`. The result never exposes
    /// a configured path. `available_bytes: None` means space is unknown and admission
    /// proceeds; a later write failure is reported as `snapshot.write_failed`. An `Ok`
    /// `no_replace_register` result is only a pre-check and does not guarantee later
    /// registration will succeed.
    fn preflight(&self) -> Result<SnapshotStoreStatus, SnapshotFileError>;

    /// Creates an exclusively locked temporary file before any snapshot bytes are written.
    fn create_pending(
        &self,
        snapshot_id: &Id,
    ) -> Result<Box<dyn PendingSnapshotFile + '_>, SnapshotFileError>;

    /// Opens only the registered final name for reading.
    ///
    /// Implementations never open `.tmp-*.part` names. The returned handle is
    /// bound to the opened file even if its directory entry is later replaced.
    /// Missing names return `NotFound`; open errors return `ReadFailed` with a
    /// diagnostic-only error kind. Errors from subsequent reads remain I/O errors.
    /// Neither the handle nor diagnostics expose a filesystem path.
    fn open_registered(
        &self,
        snapshot_id: &Id,
    ) -> Result<Box<dyn Read + Send + '_>, SnapshotFileError>;

    /// Creates one private, exclusively locked capture for this operation.
    ///
    /// The capture is removed when dropped, including after cancellation or
    /// failure. When `required_bytes` is known, insufficient available space
    /// returns `InsufficientSpace` before the capture is created. A later write
    /// error is `WriteFailed`; `None` means the size is unknown and the adapter
    /// checks for any available space when it can.
    fn create_capture(
        &self,
        required_bytes: Option<u64>,
    ) -> Result<Box<dyn CaptureSpool<Sealed = Box<dyn SealedSpool>> + Send + '_>, SnapshotFileError>;
}

/// Snapshot-store facts collected without creating a pending snapshot file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotStoreStatus {
    /// Available bytes on the snapshot volume, or `None` when unknown.
    pub available_bytes: Option<u64>,
    /// Result of checking no-replace file registration support.
    pub no_replace_register: Result<(), SnapshotFileError>,
}

/// A staged snapshot file that is removed if it is dropped before publication.
pub trait PendingSnapshotFile: Write {
    /// Registers the file without replacement and confirms the directory entry is durable.
    ///
    /// On `Ok`, the file is registered and durable. On `WriteFailed`, nothing
    /// was registered. On `RegisteredNotDurable`, the final file is registered
    /// but directory durability could not be confirmed; callers must not add an
    /// index row, and startup reconciliation reports `snapshot.orphan_file`.
    fn publish(self: Box<Self>) -> Result<(), SnapshotPublishError>;
}

/// Opens or creates the private snapshots folder without exposing a path to feature code.
pub trait SnapshotFileSystem: Send + Sync {
    /// Returns a handle-relative view of the snapshots folder.
    fn open_or_create_snapshots(
        &self,
    ) -> Result<Box<dyn SnapshotFileDirectory + '_>, SnapshotFileError>;
}

/// Handle-relative operations on the snapshots folder.
pub trait SnapshotFileDirectory: Send + Sync {
    /// Returns available bytes on this directory's volume, or `None` when unknown.
    fn available_bytes(&self) -> Option<u64> {
        None
    }

    /// Confirms no-replace registration support before the caller writes snapshot bytes.
    /// `Ok` is only a pre-check and does not guarantee a later registration succeeds;
    /// registration failure is still reported as `WriteFailed`.
    fn check_no_replace_register(&self) -> Result<(), SnapshotFileError>;

    /// Exclusively creates and locks the identifier-derived temporary file.
    fn create_exclusive_temp(
        &self,
        snapshot_id: &Id,
    ) -> Result<Box<dyn SnapshotFileSystemTempFile>, SnapshotFileError>;

    /// Opens only `<snapshot_id>.dusnap` relative to this held directory.
    fn open_final(&self, snapshot_id: &Id) -> io::Result<Box<dyn Read + Send>>;

    /// Creates one private capture file after checking known free-space needs.
    fn create_capture(
        &self,
        required_bytes: Option<u64>,
    ) -> Result<Box<dyn CaptureSpool<Sealed = Box<dyn SealedSpool>> + Send>, SnapshotFileError>;

    /// Synchronizes the folder after successful registration. An error does not undo
    /// registration; the final name remains registered.
    fn sync_folder(&self) -> io::Result<()>;
}

/// An opaque temporary file handle retained through registration or cleanup.
///
/// Dropping an unregistered file removes its temporary name. Successful
/// registration prevents that cleanup from removing the final name.
pub trait SnapshotFileSystemTempFile: Write + Send {
    /// Flushes the file's staged bytes to stable storage.
    fn sync_file(&mut self) -> io::Result<()>;

    /// Registers this handle under the final identifier without replacing a name.
    ///
    /// If the final name already exists, it is preserved and this call fails. This
    /// operation never replaces a name. An error guarantees this handle was not
    /// registered under the final name.
    fn register(&mut self, snapshot_id: &Id) -> io::Result<()>;
}
