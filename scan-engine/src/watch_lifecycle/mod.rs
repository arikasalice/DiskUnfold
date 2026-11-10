//! Pure, session-only watch admission and stop fencing.
//!
//! Adapter commands are returned as values. No operating-system calls, event
//! publication, filesystem access, or serialized contracts are implemented here.
//! For any rejected event or transition, the integration must fence callback
//! intake and stop claiming current observation until evidence is reconciled;
//! the adapter remains responsible for any retained registration handle.
mod compaction;
mod model;
mod registry;
mod unit;
mod unit_control;
pub use model::{
    Action, Capability, CleanupTicket, Completion, Definition, Error, Interruption, Registration,
    Snapshot, State, Status, Support,
};
pub use registry::{Registry, StartResult};
#[cfg(test)]
mod tests;
