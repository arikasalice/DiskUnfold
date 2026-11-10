//! In-process operating-system change notification port (F18 IF-10 and IF-11).
//!
//! Paths use the native byte encoding of the selected scope. These types are
//! internal port values, not IPC or serialized contracts.

use crate::volume::Capability;

/// Volume-specific notification capabilities and their reasons.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeCapabilities {
    pub notification: Capability,
    pub loss: Capability,
    pub path: Capability,
    pub interruption: Capability,
    pub resource_limit: Capability,
}

/// A registration request for one verified volume and entry path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatchRequest {
    pub volume: Vec<u8>,
    pub entry_path: Vec<u8>,
    /// Normalized comparison-key components within the volume.
    pub volume_path: Vec<Vec<u8>>,
    /// Nested mount paths in the same volume-relative representation.
    pub nested_mounts: Vec<Vec<Vec<u8>>>,
}

/// Adapter-owned reference. It remains valid until release completes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RegistrationRef(pub u64);

/// Coverage returned after the OS registration has completed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Registration {
    pub reference: RegistrationRef,
    pub covered: Vec<Vec<Vec<u8>>>,
    pub uncovered: Vec<Vec<Vec<u8>>>,
}

/// Neutral errors at the adapter boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchError {
    Disconnected,
    AccessLost,
    ResourceLimit,
    Other,
}

/// OS-provided hint; it is not a verified file-system observation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeHint {
    Create,
    Modify,
    Delete,
    Rename,
    Unknown,
}

/// Ordered events from one registration, timed in milliseconds by the injected clock.
/// A signal path, when present, is an absolute native path under the entry path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WatchEvent {
    Signal {
        path: Option<Vec<u8>>,
        hint: ChangeHint,
        observed_at: u64,
    },
    Loss {
        observed_at: u64,
    },
    Interrupted {
        reason: WatchError,
        observed_at: u64,
    },
}

/// A bounded, nonblocking event stream belonging to one registration.
///
/// The adapter never waits for the consumer in an OS callback. When it drops a
/// signal it retains a loss marker and reports loss on the next delivery. An
/// interruption is delivered at most once and no later signal is delivered.
pub trait WatchEvents: Send {
    /// Returns the next event, or `None` when no event is currently available.
    fn try_next(&mut self) -> Option<WatchEvent>;
}

/// Synchronous registration and release; calls must not be forcibly interrupted.
/// Failed capability queries report unsupported with an appropriate reason.
/// Nested mounts must not be registered and must be reported as uncovered.
pub trait ChangeNotifyPort: Send + Sync {
    fn capabilities(&self, volume: &[u8]) -> ChangeCapabilities;
    fn register(
        &self,
        request: &WatchRequest,
    ) -> Result<(Registration, Box<dyn WatchEvents>), WatchError>;
    /// Releasing the same reference again has no effect.
    fn release(&self, reference: RegistrationRef) -> Result<(), WatchError>;
}
