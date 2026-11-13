//! Identity-bound, read-only content access for duplicate verification.
//!
//! Unlike `ports::ContentReadPort`, this port binds reads and observations to
//! one selected file identity. Implementations must fail before content I/O if
//! they cannot prevent placeholder download and access-time updates.
use crate::{ItemRef, ports::PortError};

/// Identity recorded by the scan, including the enumeration discriminator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentIdentity {
    /// Native volume identity bytes.
    pub volume: Vec<u8>,
    /// Native file identity bytes.
    pub file: Vec<u8>,
    /// Nonzero enumeration discriminator, or `None` when not observed.
    pub enumerated_inode: Option<u64>,
}

/// Pre-open evidence about whether accessing data would require a download.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaceholderEvidence {
    /// The observation indicates that data is local.
    Local,
    /// Data access requires a download and must not be attempted.
    DownloadRequired,
    /// The observation cannot establish whether a download is needed.
    Unknown,
}

/// State of the same open handle used for content reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentObservation {
    /// Identity of the same open handle used for reads.
    pub identity: ContentIdentity,
    /// Logical size in bytes.
    pub size: u64,
    /// Adapter-native modification time, compared only within the same adapter.
    pub modified: u64,
    /// Absent when a reliable change stamp is unavailable.
    pub change_stamp: Option<u64>,
}

/// A selected file whose observations and bounded reads use the same handle.
pub trait BoundContent {
    /// Observes identity and metadata without replacing the content handle.
    fn observe(&mut self) -> Result<ContentObservation, PortError>;
    /// Reads at most the supplied buffer length at the requested offset.
    fn read_at(&mut self, offset: u64, buffer: &mut [u8]) -> Result<usize, PortError>;
}

/// Opens a selected item only when both no-download and no-atime guarantees hold.
pub trait DuplicateContentPort {
    /// Opens the selected identity or rejects it before any unsafe content I/O.
    fn open_bound<'a>(
        &'a mut self,
        item: &ItemRef,
        expected: &ContentIdentity,
        placeholder: PlaceholderEvidence,
    ) -> Result<Box<dyn BoundContent + 'a>, PortError>;
}
