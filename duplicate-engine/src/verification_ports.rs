//! Identity-bound verification ports and hash strategy.
use crate::{candidates::FileIdentity, verification_input::Snapshot};
/// Purpose-separated hash invocations must not reuse each other's digest domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HashPurpose {
    Sample,
    Full,
}

/// Read failure or a failed identity-bound open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadError {
    Failed,
    IdentityChanged,
}

/// An opened file bound to its identity; reads must not follow a replacement path.
pub trait OpenFile {
    /// Reads metadata from the same open file used for content.
    fn snapshot(&mut self) -> Result<Snapshot, ReadError>;
    /// Reads at most `buffer.len()` bytes from the specified file offset.
    fn read_at(&mut self, offset: u64, buffer: &mut [u8]) -> Result<usize, ReadError>;
}

/// Opens only the selected item and fails if its identity no longer matches.
///
/// The adapter must prevent placeholder downloads and access-time updates, or fail
/// closed when it cannot provide those guarantees.
pub trait ReadPort {
    fn open_bound<'a>(
        &'a mut self,
        item_no: u32,
        expected: &FileIdentity,
    ) -> Result<Box<dyn OpenFile + 'a>, ReadError>;
}

/// Incremental digest state, scoped to one item and hash purpose.
pub trait Digest {
    fn update(&mut self, bytes: &[u8]);
    fn finish(self: Box<Self>) -> Vec<u8>;
}

/// A pinned registered strategy; production composition must provide a trusted implementation.
pub trait HashStrategy {
    fn id(&self) -> &str;
    fn revision(&self) -> u32;
    fn collision_resistant(&self) -> bool;
    /// Initializes a digest for the requested purpose and the confirmed file size.
    fn start(&self, purpose: HashPurpose, approved_size: u64) -> Box<dyn Digest>;
}

/// Strategies fixed at admission for sample filtering and final full-file verification.
pub struct Strategies<'a> {
    pub sample: &'a dyn HashStrategy,
    pub full: &'a dyn HashStrategy,
}
