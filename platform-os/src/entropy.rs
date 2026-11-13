//! Operating-system cryptographic randomness for injected capabilities.
use du_core_contracts::ports::{PortError, PortErrorKind, Random};

/// Fills buffers from the operating system's cryptographic random source.
pub struct OsRandom;

impl Random for OsRandom {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), PortError> {
        getrandom::fill(bytes).map_err(|error| PortError {
            kind: PortErrorKind::Io,
            reason_key: "os.random_unavailable".into(),
            os_code: error.raw_os_error(),
        })
    }
}
