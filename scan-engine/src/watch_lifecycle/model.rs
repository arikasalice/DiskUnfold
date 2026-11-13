use crate::watch_coalescer::{Batch, Gap, Key};

/// A user-selected, verified scope target. IDs and normalized paths are supplied
/// by the scope owner, rather than discovered or resolved by the lifecycle core.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Definition {
    pub watch_ref: [u8; 16],
    pub scope_revision: [u8; 16],
    pub target: Key,
    /// Native path for the adapter to open, supplied by the verified scope.
    pub entry_path: Vec<u8>,
    /// Normalized path within the identified volume.
    pub volume_path: Key,
    /// Nested mount paths which the adapter must not register.
    pub nested_mounts: Vec<Key>,
    pub volume: Vec<u8>,
    pub baseline_end: Option<u64>,
}
/// Capability support independent of operating system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Support {
    Supported,
    Partial,
    Unsupported,
}
/// The relevant five IF-10 change-notification capability values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capability {
    pub notification: Support,
    pub loss: Support,
    pub interruption: Support,
    pub path: Support,
    pub resource_limit: Support,
}
impl Capability {
    pub(super) fn degraded(self) -> bool {
        self.notification == Support::Partial
            || self.loss != Support::Supported
            || self.interruption != Support::Supported
    }
    pub(super) fn gap_for_all(self) -> bool {
        self.loss != Support::Supported || self.interruption != Support::Supported
    }
}
/// Result of an IF-10 registration. A handle is opaque to the lifecycle core.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Registration {
    /// The adapter acquired a registration, with any uncovered subtrees.
    Acquired {
        handle: u64,
        uncovered: Vec<Key>,
    },
    /// The root could not be watched because of a resource limit.
    ResourceLimit,
    /// Registration failed without an acquired handle.
    Disconnected,
    AccessLost,
    Other,
}
/// Neutral interruption classifications reported by the adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interruption {
    Disconnected,
    AccessLost,
    Other,
}
/// Completion of a requested adapter removal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Completion {
    Removed,
    Failed,
}
/// Adapter-owned registration which still requires explicit cleanup.
/// The lifecycle retains the identity until the adapter confirms removal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CleanupTicket {
    pub token: u64,
    pub generation: u64,
    pub handle: u64,
}
/// Published lifecycle states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Starting,
    Active,
    Degraded,
    Lost,
    Unsupported,
    Stopping,
    Stopped,
}
/// A status event shares a monotonic per-watch sequence with change batches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub watch_ref: [u8; 16],
    pub state: State,
    pub previous: Option<State>,
    pub reason: Option<&'static str>,
    pub generation: u64,
    pub sequence: u64,
    pub observed_at: u64,
    pub last_observed: Option<u64>,
}
/// F33-specific internal snapshot, captured under the unit owner at one cut.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub definition: Definition,
    pub state: State,
    pub generation: u64,
    pub sequence: u64,
    pub last_change_sequence: u64,
    pub last_observed: Option<u64>,
    pub observed_at: u64,
    pub gaps: Vec<Gap>,
}
/// Ordered effects to be interpreted by a future adapter/dispatcher.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Start the adapter registration. Token is a per-session attempt, not generation.
    Register {
        token: u64,
        generation: u64,
        definition: Definition,
    },
    /// Release this acquired adapter handle exactly once.
    Remove {
        token: u64,
        generation: u64,
        handle: u64,
    },
    /// Publish an internal change batch.
    Changes(Batch),
    /// Publish an internal status event.
    Status(Status),
}
/// Rejected operations retain their input and previous evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    DuplicateReference,
    ScopeChanged,
    Capacity,
    NotFound,
    InvalidState,
    InvalidDefinition,
    GapCapacity,
    ClockRegressed,
    CounterExhausted,
    Coalescer(crate::watch_coalescer::Error),
}
impl From<crate::watch_coalescer::Error> for Error {
    fn from(error: crate::watch_coalescer::Error) -> Self {
        Self::Coalescer(error)
    }
}
