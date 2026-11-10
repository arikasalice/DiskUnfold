//! Read-only directory observations. Names are native bytes, not display strings.
use crate::{
    OsPath, Timestamp,
    ports::{PortError, PortErrorKind},
};

/// A bounded filtered enumeration step; `Progress` is not end of directory.
pub enum FilteredBatch {
    Entries(Vec<ObservedEntry>),
    Progress,
    End,
    /// The predicate could not safely decide; the collection must be withdrawn.
    Undecidable,
}

/// A decision made before opening the enumerated item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterDecision {
    Admit,
    Exclude,
    Undecidable,
}

/// A bounded pull interface; an empty batch marks the end of this directory.
/// An error terminates the stream; callers must record the unread remainder.
pub trait DirectoryObservations: Send {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError>;

    /// Applies a stable raw-name decision before opening any item metadata handle.
    /// Each call processes bounded work; `Progress` requires another call. The
    /// predicate must remain the same for this directory's lifetime. Adapters
    /// that cannot enforce this boundary must return `Unsupported` without I/O.
    fn next_filtered(
        &mut self,
        _include: &mut dyn FnMut(&[u8]) -> FilterDecision,
    ) -> Result<FilteredBatch, PortError> {
        Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "os.unsupported".into(),
            os_code: None,
        })
    }
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

    /// IDs of verified same-volume ancestors, excluding this directory.
    /// `None` means unavailable; `Some(&[])` confirms a volume root. Evidence
    /// applies only while this anchored directory and its supporting handles live.
    fn ancestor_file_ids(&self) -> Option<&[Vec<u8>]> {
        None
    }
}

/// The identity and canonical path of a scope root obtained from its open handle.
pub struct OpenedScopeRoot {
    pub canonical_path: OsPath,
    pub volume_id: Vec<u8>,
    pub file_id: Vec<u8>,
    pub directory: Box<dyn AnchoredDirectory>,
}

/// Opens a verified read-only directory for review or later collection.
pub trait ScopeRootPort: Send + Sync {
    fn open_root(&self, absolute_path: &[u8]) -> Result<OpenedScopeRoot, PortError>;
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
    /// Last write time observed on the verified handle, when available.
    pub modified_at: Option<Timestamp>,
    /// OS-reported allocation for the observed file, or `None` when unverified.
    pub allocated: Option<u64>,
    /// OS-reported total hard-link count, or `None` when unverified.
    /// This does not establish whether physical extents are shared.
    pub link_count: Option<u64>,
    /// OS-provided identity for physical accounting, when independently available.
    pub space_identity: Option<SpaceIdentity>,
}

/// An operating-system volume and file identity, scoped to the observation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpaceIdentity {
    pub volume: Vec<u8>,
    pub file: Vec<u8>,
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
