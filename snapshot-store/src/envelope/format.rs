//! Reader selection by the format version in a snapshot file header (FR8.HD1.snapshot-files.DD1).
//!
//! Only the first ten bytes of a file (magic and version) are read to choose a reader. A file
//! whose version is newer than this build reads, or that no registry entry covers, is reported
//! as `snapshot.format_unknown` before any other header field is trusted. Nothing here writes.

use du_core_contracts::snapshot_format::{
    FormatUnknown, SNAPSHOT_FORMAT_UNKNOWN, SnapshotFormatRegistry,
};

use super::{DUSNAP_FORMAT_VERSION, Envelope, Error, MAGIC, read_u16, verify_v1};

/// Bytes of the header the version decision needs: the magic and the version field.
pub const VERSION_PREFIX_BYTES: usize = 10;

/// The reader module that handles one format version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reader {
    /// DUSNAP version 1 (`verify_v1`).
    V1,
}

/// Reader modules by version. A new format version adds its module and one line here, and one
/// registration where the composition root assembles the registry.
const READERS: &[(u16, Reader)] = &[(DUSNAP_FORMAT_VERSION, Reader::V1)];

/// Why a snapshot file could not be opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenError {
    /// The header is not a DUSNAP header, or the version-specific reader found the file invalid.
    Unreadable(Error),
    /// The header names a version without a registered reader; only this file is unreadable.
    FormatUnknown(FormatUnknown),
}

impl OpenError {
    /// Returns the published reason key, or `None` when F07 defines the key for the failure.
    pub fn reason_key(&self) -> Option<&'static str> {
        match self {
            Self::FormatUnknown(unknown) => Some(unknown.reason_key()),
            Self::Unreadable(error) => error.reason_key(),
        }
    }
}

impl Error {
    /// Returns `snapshot.format_unknown` for an unsupported version. Other structural failures
    /// have no key in the F15 design; F07's snapshot reasons apply to them.
    pub fn reason_key(self) -> Option<&'static str> {
        (self == Self::UnsupportedVersion).then_some(SNAPSHOT_FORMAT_UNKNOWN)
    }
}

/// Reads the format version from the first bytes of a file.
///
/// Fails with `Truncated` when fewer than ten bytes are given and with `Magic` when the file is
/// not a DUSNAP file. No other field is checked.
pub fn header_format_version(prefix: &[u8]) -> Result<u16, Error> {
    match prefix.get(..MAGIC.len()) {
        Some(magic) if magic == MAGIC.as_slice() => read_u16(prefix, MAGIC.len()),
        Some(_) => Err(Error::Magic),
        None => Err(Error::Truncated),
    }
}

/// Returns the reader for `version` when this build has one.
pub fn reader_for(version: u16) -> Option<Reader> {
    READERS
        .iter()
        .find_map(|(registered, reader)| (*registered == version).then_some(*reader))
}

/// Chooses the reader for a file from its header and the registry.
///
/// A version is readable only when the registry lists it and this build has a reader for it.
pub fn select_reader(
    prefix: &[u8],
    registry: &dyn SnapshotFormatRegistry,
) -> Result<Reader, OpenError> {
    let version = header_format_version(prefix).map_err(OpenError::Unreadable)?;
    let unknown = OpenError::FormatUnknown(FormatUnknown { version });
    if !registry.is_registered(version) {
        return Err(unknown);
    }
    reader_for(version).ok_or(unknown)
}

/// Opens one file's bytes: selects the reader by header version and runs its outer verification.
///
/// The input is never modified.
pub fn open_verified<'a>(
    bytes: &'a [u8],
    registry: &dyn SnapshotFormatRegistry,
) -> Result<Envelope<'a>, OpenError> {
    match select_reader(bytes, registry)? {
        Reader::V1 => verify_v1(bytes).map_err(OpenError::Unreadable),
    }
}

#[cfg(test)]
mod tests;
