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

/// An entry and the time its metadata was actually observed, when supplied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimedObservedEntry {
    pub entry: ObservedEntry,
    pub observed_at: Option<Timestamp>,
}

impl From<ObservedEntry> for TimedObservedEntry {
    fn from(entry: ObservedEntry) -> Self {
        Self {
            entry,
            observed_at: None,
        }
    }
}

/// Filtered batch with optional per-entry observation times.
pub enum TimedFilteredBatch {
    Entries(Vec<TimedObservedEntry>),
    Progress,
    End,
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

    /// Optional timed variant. Legacy adapters preserve unknown times without calling the clock.
    fn next_batch_timed(
        &mut self,
        _clock: &dyn Fn() -> Option<Timestamp>,
    ) -> Result<Vec<TimedObservedEntry>, PortError> {
        self.next_batch()
            .map(|entries| entries.into_iter().map(TimedObservedEntry::from).collect())
    }

    /// Optional timed filtered variant, retaining the original pre-metadata boundary.
    fn next_filtered_timed(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
        _clock: &dyn Fn() -> Option<Timestamp>,
    ) -> Result<TimedFilteredBatch, PortError> {
        self.next_filtered(include).map(|batch| match batch {
            FilteredBatch::Entries(entries) => TimedFilteredBatch::Entries(
                entries.into_iter().map(TimedObservedEntry::from).collect(),
            ),
            FilteredBatch::Progress => TimedFilteredBatch::Progress,
            FilteredBatch::End => TimedFilteredBatch::End,
            FilteredBatch::Undecidable => TimedFilteredBatch::Undecidable,
        })
    }

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

/// Volume-wide OS-reported values; no physical extent identity is implied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VolumeSpace {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
}

/// A retained, verified handle that can re-observe one expected volume.
pub trait VolumeSpaceReader: Send {
    fn read(&self, expected_volume: &[u8]) -> Result<VolumeSpace, PortError>;
}

/// Name lookup mode observed on one verified directory handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectoryCaseState {
    /// The directory reported case-sensitive name lookup.
    Sensitive,
    /// The directory reported case-insensitive name lookup.
    Insensitive,
    /// The adapter queried the mode but could not interpret or obtain support for it.
    Unsupported,
    /// A query or verification failed without establishing the mode.
    Failed,
}

/// Point observation of a directory flag, independent of a path comparison key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryCaseEvidence {
    /// Outcome on this directory only, with no inherited or volume-wide meaning.
    pub state: DirectoryCaseState,
    /// Actual observation time, if the adapter captured it.
    pub observed_at: Option<Timestamp>,
    /// Diagnostic key for unsupported or failed observations, if available.
    pub reason_key: Option<String>,
}

/// A directory already opened and verified by an operating-system adapter.
///
/// Child opens must be relative to this handle, without following links or
/// reparse points. Implementations must reject a changed identity or volume.
pub trait AnchoredDirectory: DirectoryObservations {
    /// Opens the previously observed directory entry after revalidating it.
    fn open_child(&self, entry: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError>;

    /// Optionally observes name lookup mode on this handle without reopening a path.
    /// An absent observation leaves the mode unknown and does not query the clock.
    fn observe_directory_case(
        &self,
        _clock: &dyn Fn() -> Option<Timestamp>,
    ) -> Option<DirectoryCaseEvidence> {
        None
    }

    /// Retains read-only volume evidence without reopening a path.
    /// The default explicitly declines the observation without filesystem I/O.
    fn volume_space_reader(&self) -> Result<Box<dyn VolumeSpaceReader>, PortError> {
        Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "space.volume_report_unavailable".into(),
            os_code: None,
        })
    }

    /// Reads the last-write time from this verified handle without reopening it.
    /// Missing permissions or unsupported or invalid observations return `None`.
    fn observed_modified_at(&self) -> Option<Timestamp> {
        None
    }

    /// Optional timed metadata read. Existing adapters return their modification
    /// time without invoking the clock or claiming a new observation time.
    fn observed_modified_at_timed(
        &self,
        _clock: &dyn Fn() -> Option<Timestamp>,
    ) -> (Option<Timestamp>, Option<Timestamp>) {
        (self.observed_modified_at(), None)
    }

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
