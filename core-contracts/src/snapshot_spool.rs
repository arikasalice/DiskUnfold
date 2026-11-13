//! Operating system storage boundary for immutable snapshot verification input.

use std::io::{self, Read, Seek, Write};

/// A private, exclusive capture destination removed on drop or failure.
pub trait CaptureSpool: Write {
    /// The same bytes after the write capability has been consumed.
    type Sealed: SealedSpool;

    /// Consumes the writer and retains the lock over the captured bytes.
    ///
    /// Other requests and processes must not be able to change the capture.
    /// The implementation must remove it if sealing fails.
    fn seal(self) -> io::Result<Self::Sealed>;
}

/// Read-only access to the same locked bytes until the capture is removed on drop.
///
/// Implementations must keep the lock and must never expose a writer.
pub trait SealedSpool: Read + Seek {}
