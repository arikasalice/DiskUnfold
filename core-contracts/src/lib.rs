//! Shared wire values, envelopes, and narrow operating system ports.
pub mod analysis_ipc;
pub mod browse_ipc;
mod envelopes;
pub mod job;
pub mod ports;
pub mod result;
pub mod result_events;
pub mod scan_failures;
pub mod scan_observation;
pub mod settings;
pub mod snapshot_spool;
mod values;
pub mod visualization;
pub mod volume;
pub mod volume_gap_ipc;
pub use envelopes::*;
pub use values::*;
