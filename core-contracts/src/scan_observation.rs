//! Read-only directory observations. Names are native bytes, not display strings.
use crate::ports::PortError;

/// A bounded pull interface; an empty batch marks the end of this directory.
/// An error terminates the stream; callers must record the unread remainder.
pub trait DirectoryObservations: Send {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError>;
}

pub trait DirectoryObservationPort: Send + Sync {
    fn open(&self, absolute_path: &[u8]) -> Result<Box<dyn DirectoryObservations>, PortError>;
}

/// A directory already opened and verified by an operating-system adapter.
///
/// Child opens must be relative to this handle, without following links or
/// reparse points. Implementations must reject a changed identity or volume.
pub trait AnchoredDirectory: DirectoryObservations {
    /// Opens the previously observed directory entry after revalidating it.
    fn open_child(&self, entry: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObservedKind {
    File,
    Directory,
    Link,
    Other,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataObservation {
    pub kind: ObservedKind,
    pub device: u64,
    pub inode: u64,
    pub size: Result<u64, PortError>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservedEntry {
    pub name: Vec<u8>,
    /// Inode returned by directory enumeration, before the metadata query.
    pub enumerated_inode: u64,
    pub enumerated_kind: ObservedKind,
    /// Failure is retained per entry; the rest of the directory can still be read.
    pub metadata: Result<MetadataObservation, PortError>,
}
