//! Shared wire values, envelopes, and narrow operating system ports.
pub mod analysis_ipc;
pub mod browse_ipc;
mod envelopes;
pub mod job;
pub mod ports;
pub mod result;
pub mod scan_observation;
pub mod settings;
mod values;
pub mod visualization;
pub mod volume;
pub use envelopes::*;
pub use values::*;
