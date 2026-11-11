//! Candidate preparation and injected content identity verification.
//!
//! Real content reads require an injected identity-bound port.
pub mod candidates;
pub mod content_port;

mod content_hash;
pub use content_hash::{RegisteredHash, RegisteredOutcome};
pub mod verification;

mod read_measurement;

mod verification_result;

/// Session-owned immutable duplicate verification results.
pub mod result_store;

/// Job producer for a pinned, injected duplicate verification scope.
pub mod job;

mod size_evidence;

mod verification_input;

/// Bounded session fingerprint evidence, separate from duplicate verdicts.
pub mod fingerprint_cache;

mod verification_ports;
mod verification_read;
