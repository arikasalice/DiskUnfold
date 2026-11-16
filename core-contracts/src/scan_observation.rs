//! Read-only directory observations. Names are native bytes, not display strings.
use crate::{
    OsPath, Timestamp,
    ports::{PortError, PortErrorKind},
};
use std::sync::atomic::AtomicBool;

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
    /// Historical metadata-only observation; never authorizes content access.
    pub placeholder_observation: Option<PlaceholderObservation>,
}

/// Facts observed by the scan, not a current locality or no-recall guarantee.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaceholderObservation {
    /// The Windows metadata handle did not report the known offline or recall-on-data flags.
    WindowsNoRecallDataFlags,
}

impl From<ObservedEntry> for TimedObservedEntry {
    fn from(entry: ObservedEntry) -> Self {
        Self {
            entry,
            observed_at: None,
            placeholder_observation: None,
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

/// When a correlated adapter invoked the filter relative to metadata observation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterCallbackPhase {
    BeforeMetadata,
    AfterMetadata,
    Unknown,
}

/// One bounded result from an opt-in correlated directory enumeration attempt.
///
/// `child_id` is dense and zero-based for every direct-child record, including
/// excluded and undecidable records. It is scoped to one attempt and is not an
/// event sequence or filesystem identity. F02 assigns and transports event
/// sequences independently.
// Keep payloads inline to avoid one extra heap allocation per admitted child.
// Consequently, every event has the size of the Observation variant; any future bounded
// transport must account for the enum's full inline size per retained event.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CorrelatedDirectoryEvent {
    /// The callback decision and raw name for this record.
    FilterCallback {
        /// Dense zero-based direct-child record ID, reused for this child in this attempt.
        child_id: u64,
        raw_name: Vec<u8>,
        decision: FilterDecision,
        phase: FilterCallbackPhase,
    },
    /// The complete per-entry observation, including its timestamp and any metadata error.
    Observation {
        /// The same ID used for this direct child's filter callback.
        child_id: u64,
        entry: TimedObservedEntry,
    },
    /// A bounded call made progress but has no event to report yet.
    Progress,
    /// Actual directory EOF. The count is the number of child IDs the adapter issued,
    /// including excluded, undecidable, failed, or buffer-discarded records. It is not
    /// the number of successfully returned items or an event-sequence watermark.
    End { child_count_exclusive: u64 },
}

/// A bounded pull interface; an empty batch marks the end of this directory.
/// An error terminates the stream; callers must record the unread remainder.
pub trait DirectoryObservations: Send {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError>;

    /// Selects correlated enumeration before consuming entries for this attempt.
    ///
    /// A successful return selects this stream for the entire attempt. Callers must not
    /// mix it with `next_batch`, `next_filtered`, or their timed variants; after any
    /// stream error they must fail the attempt rather than bridge to a legacy port.
    /// Implementations that cannot provide this capability return `Unsupported` before
    /// enumeration I/O. Only that result permits fallback to the legacy path; other
    /// errors fail the attempt. Existing adapters inherit `Unsupported` unchanged. The
    /// attempt state remains on this directory between calls. F02 owns attempt framing,
    /// the event sequence and its watermark, pending-state clearing, per-child final
    /// classification (including boundaries and revisits), and attempt completion. The
    /// adapter owns dense child IDs, the EOF child count, filter callback facts,
    /// per-entry observations, and stream errors.
    ///
    /// The selection performs no enumeration I/O. Each subsequent step uses the
    /// directory lease for that call, which its owner releases before transporting the
    /// event. A successful selection prohibits mixing with legacy enumeration, even
    /// after an error or cancellation; end and failure leave the attempt terminal. A
    /// retry is a new attempt with a new child-ID scope. Only `Unsupported` from this
    /// method permits legacy fallback.
    fn begin_correlated_filtered(&mut self) -> Result<(), PortError> {
        Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "os.unsupported".into(),
            os_code: None,
        })
    }

    /// Advances the selected attempt by one bounded event using the same stable raw-name
    /// predicate for its lifetime.
    ///
    /// Each OS enumeration record receives the next dense, zero-based `child_id` before
    /// filtering or metadata observation, including records filtered as `Exclude` or
    /// `Undecidable`. Reuse that ID for every event about the record; IDs have no meaning
    /// outside this attempt. Emit one `FilterCallback` for each predicate invocation.
    /// `Exclude` has no later observation. `Undecidable` remains unresolved. A
    /// per-entry metadata error belongs in `Observation.entry.metadata`; an error
    /// returned by this method is a stream failure and must not be bridged to a legacy
    /// method. Once issued, an ID remains counted if its record later fails or its
    /// pending buffer is discarded.
    ///
    /// Return `End { child_count_exclusive }` only at successful actual EOF, after all
    /// adapter events for issued IDs have been returned to F02. The count is the total
    /// number of IDs issued, including excluded, undecidable, failed, and
    /// buffer-discarded records; it is not a count of successful observations. It defines
    /// the range `[0, child_count_exclusive)`. Do not return a valid `End` after ID
    /// overflow, parse or I/O failure, cancellation, or loss of a required adapter fact.
    /// F02 transports this count separately from its independently assigned
    /// `event_watermark_exclusive`; neither value can substitute for the other. If F02
    /// clears pending data or loses data during allocation or transport, it must preserve
    /// affected IDs as unresolved before forwarding `End`, or fail the whole attempt.
    /// F03 must independently require every ID in the count range to be Seen and have a
    /// terminal outcome. An `Exclude` callback is itself terminal and needs no later
    /// `Final` event; only an `Admit` callback requires corresponding `Final` evidence.
    /// `Undecidable`, missing facts, and pending clears remain unresolved and cannot
    /// establish successful attempt completion. A matching count alone does not prove
    /// the filesystem was fully observed or unchanged during enumeration.
    ///
    /// `observed_at: None` and a `None` clock result mean the time is unknown. Do not
    /// synthesize a scan or completion time. These point observations do not prove an
    /// atomic snapshot or rule out concurrent filesystem changes. A failed or cancelled
    /// attempt cannot be resumed in place; retry with a new attempt and do not combine
    /// its IDs or events with the prior attempt.
    ///
    /// This default returns `Unsupported` without I/O for compatibility with existing
    /// implementations. If selection succeeded, an error here fails that attempt;
    /// callers must not fall back to legacy enumeration.
    fn next_correlated_event(
        &mut self,
        _include: &mut dyn FnMut(&[u8]) -> FilterDecision,
        _clock: &dyn Fn() -> Option<Timestamp>,
    ) -> Result<CorrelatedDirectoryEvent, PortError> {
        Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "os.unsupported".into(),
            os_code: None,
        })
    }

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

/// Volume and file identity of one verified directory handle.
///
/// Values are adapter-scoped and comparable only with the `device` and
/// `inode` that the same adapter reports in [`MetadataObservation`].
///
/// On Windows the pair is the 32-bit volume serial number and the 64-bit file
/// index. Two volumes that report the same serial number cannot be told
/// apart, and a file ID that needs 128 bits cannot be represented.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandleIdentity {
    /// Volume of the handle in the adapter's `MetadataObservation::device` space.
    pub device: u64,
    /// File identity of the handle on `device`, never zero when reported.
    pub inode: u64,
    /// Same-volume ancestor identities verified while opening this handle, in
    /// the `inode` space. Empty when the adapter retained none. The Windows
    /// adapter omits retained ancestor IDs that are not exactly 8 bytes.
    pub ancestors: Vec<u64>,
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

    /// Identity of this handle in the `MetadataObservation` identity space.
    ///
    /// The collector reads it once for every scope root, in root order and
    /// under a metadata flow, before any root is enumerated, and compares child
    /// volumes with it. It may perform filesystem I/O on this handle.
    ///
    /// `None` means the identity is unsupported or could not be verified. The
    /// collector then records the root as an unread range, keeps the scan
    /// result incomplete and does not enter its child directories. A wrapper
    /// must forward the value of the handle it wraps.
    fn observed_identity(&self) -> Option<HandleIdentity>;
}

/// The identity and canonical path of a scope root obtained from its open handle.
pub struct OpenedScopeRoot {
    pub canonical_path: OsPath,
    pub volume_id: Vec<u8>,
    pub file_id: Vec<u8>,
    pub directory: Box<dyn AnchoredDirectory>,
}

/// One optional point observation from the same originally opened scope handle.
/// Identity bytes are adapter-owned, not a persisted format or proof of a current
/// correspondence. Paths retain their native encoding and raw bytes.
#[derive(Debug, PartialEq, Eq)]
pub struct RecentScopeEvidence {
    pub volume_id: Vec<u8>,
    pub entry_path: OsPath,
    pub relative_path: OsPath,
    pub file_id: Vec<u8>,
}

/// A cooperative observation result; cancellation contains no partial evidence.
#[derive(Debug, PartialEq, Eq)]
pub enum RecentScopeObservation {
    Observed(RecentScopeEvidence),
    Cancelled,
    Unavailable(PortError),
}

/// Deferred read-only observation bound to the same initially opened root.
/// Cancellation is cooperative and does not interrupt an in-flight OS call.
/// The caller must recheck cancellation when it publishes the observation.
pub trait RecentScopeObserver: Send {
    fn observe(&self, cancelled: &AtomicBool) -> RecentScopeObservation;
}

/// `None` means unsupported; an error means optional observer creation failed.
/// Neither condition invalidates a successfully opened collection root.
pub type OptionalRecentScopeObserver = Option<Result<Box<dyn RecentScopeObserver>, PortError>>;

/// Opens a verified read-only directory for review or later collection.
pub trait ScopeRootPort: Send + Sync {
    fn open_root(&self, absolute_path: &[u8]) -> Result<OpenedScopeRoot, PortError>;

    /// Opts into an optional observer bound to this same opened root. Existing
    /// implementations open once and provide no observer by default. Creating
    /// an observer may block; this method does not impose an admission deadline.
    fn open_root_with_recent(
        &self,
        absolute_path: &[u8],
    ) -> Result<(OpenedScopeRoot, OptionalRecentScopeObserver), PortError> {
        Ok((self.open_root(absolute_path)?, None))
    }
}

/// The kind of an enumerated entry, as reported by the adapter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObservedKind {
    File,
    Directory,
    Link,
    /// A directory entry at which another volume is mounted.
    ///
    /// An adapter reports it only when its own observation of the entry
    /// establishes the mount; an entry it cannot tell apart from a link stays a
    /// `Link`. The entry follows the rule for another volume in
    /// [`MetadataObservation`]: `device` and `inode` may identify the mounted
    /// volume and its root, and the size, times, allocation, link count and
    /// space identity are not reported. Enumeration may have reported the same
    /// entry as a `Directory` with the identity of the covered directory, so
    /// the collector compares neither with the metadata. It records the entry
    /// as a directory outside the boundary and never enters it.
    MountPoint,
    Other,
    Unknown,
}

/// Native modification evidence retained without display-time rounding.
///
/// The representation does not assert the filesystem's effective timestamp
/// resolution or authorize a content open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeModifiedTime {
    /// Windows FILETIME ticks of 100 ns since 1601-01-01 UTC, nonzero.
    WindowsFiletime(std::num::NonZeroU64),
}

/// Metadata observed for one enumerated entry through a relative handle.
///
/// `device` may differ from the volume of the enumerated parent directory.
/// Such an entry carries its own `device`, `inode` and `kind` only: `size`
/// is an error, and the times, allocation, link count and space identity are
/// `None`. When the parent's volume is known, the collector classifies the
/// entry outside the boundary. It never enters such an entry and never uses a
/// size reported for it. A [`ObservedKind::MountPoint`] follows the same rule
/// and is outside the boundary whether or not the parent's volume is known.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataObservation {
    pub kind: ObservedKind,
    /// Volume of the entry; see the type documentation for another volume.
    pub device: u64,
    pub inode: u64,
    pub size: Result<u64, PortError>,
    /// Last write time observed on the verified handle, when available.
    pub modified_at: Option<Timestamp>,
    /// Native value from the same verified metadata handle, when available.
    pub native_modified_at: Option<NativeModifiedTime>,
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
