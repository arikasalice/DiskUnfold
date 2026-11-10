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
