//! Conservative IF-11 handoff into an already registered F18 lifecycle.
//!
//! No path comparison is available here. Every change therefore marks the whole
//! watch unit and does not assert which item changed. The caller owns the
//! registration, retains rejected inputs, and publishes every returned action.
use du_core_contracts::change_notify::{WatchError, WatchEvent};
pub use du_scan_engine::watch_coalescer::GapReason;
pub use du_scan_engine::watch_lifecycle::{
    Action, Capability, Completion, Definition, Registration, Registry, State, Support,
};
use du_scan_engine::{
    watch_coalescer::{ChangeKind, Signal},
    watch_lifecycle::{Error, Interruption},
};

/// Deliver a port event to its bound lifecycle attempt and generation.
pub fn deliver(
    registry: &mut Registry,
    watch_ref: [u8; 16],
    token: u64,
    generation: u64,
    event: &WatchEvent,
) -> Result<Vec<Action>, Error> {
    match event {
        WatchEvent::Signal { observed_at, .. } => registry.signal(
            watch_ref,
            token,
            generation,
            &Signal {
                path: None,
                directory_subtree: true,
                kind: ChangeKind::Unknown,
                observed_at: *observed_at,
            },
        ),
        WatchEvent::Loss { observed_at } => {
            registry.signal_lost(watch_ref, token, generation, None, *observed_at)
        }
        WatchEvent::Interrupted {
            reason,
            observed_at,
        } => registry.interrupted(
            watch_ref,
            token,
            generation,
            match reason {
                WatchError::Disconnected => Interruption::Disconnected,
                WatchError::AccessLost => Interruption::AccessLost,
                WatchError::Other | WatchError::ResourceLimit => Interruption::Other,
            },
            *observed_at,
        ),
    }
}
