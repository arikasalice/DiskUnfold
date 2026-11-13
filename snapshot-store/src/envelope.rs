//! Read-only DUSNAP v1 envelope and integrity inspection.
//!
//! Inspection alone is not complete snapshot validation. It does not parse
//! JSON, inspect zstd frames, or decompress records. An unkeyed integrity hash
//! does not establish the origin or authenticity of a file.

pub mod spool;

const HEADER_BYTES: u64 = 16;
const FOOTER_BYTES: u64 = 56;
const HASH_BYTES: usize = 32;
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_METADATA_BYTES: u64 = 65_536;
const MAX_DECODED_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAGIC: &[u8; 8] = b"DUSNAP\r\n";

/// The only DUSNAP envelope format version this crate reads and writes (F15 release gate reads it).
pub const DUSNAP_FORMAT_VERSION: u16 = 1;

/// A structural error found while reading one immutable input buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Truncated,
    FileLimit,
    Magic,
    Flags,
    MetadataLength,
    MetadataEncoding,
    LengthMismatch,
    DecodedLimit,
    IntegrityFailed,
    UnsupportedVersion,
}

/// Structural information only; callers must still run all open validations.
pub struct Envelope<'a> {
    pub version: u16,
    pub metadata: &'a str,
    pub compressed: &'a [u8],
    pub record_count: u64,
    pub decoded_length: u64,
    expected_digest: &'a [u8; HASH_BYTES],
    bytes_before_digest: &'a [u8],
}

/// Inspects a single input buffer without allocating or modifying it.
///
/// Integer fields are little-endian. This checks fixed boundaries and UTF-8
/// metadata, but does not assert that the metadata is valid JSON, the body is
/// one valid zstd frame, or the digest is correct. Those checks are separate.
fn inspect(bytes: &[u8]) -> Result<Envelope<'_>, Error> {
    let file_len = u64::try_from(bytes.len()).map_err(|_| Error::FileLimit)?;
    check_file_length(file_len)?;
    if bytes.get(..8) != Some(MAGIC.as_slice()) {
        return Err(Error::Magic);
    }
    let version = read_u16(bytes, 8)?;
    if version != DUSNAP_FORMAT_VERSION {
        return Err(Error::UnsupportedVersion);
    }
    if read_u16(bytes, 10)? != 0 {
        return Err(Error::Flags);
    }
    let metadata_len = u64::from(read_u32(bytes, 12)?);
    if !(1..=MAX_METADATA_BYTES).contains(&metadata_len) {
        return Err(Error::MetadataLength);
    }
    let footer = bytes
        .len()
        .checked_sub(FOOTER_BYTES as usize)
        .ok_or(Error::Truncated)?;
    let compressed_length = read_u64(bytes, footer)?;
    let record_count = read_u64(bytes, footer + 8)?;
    let decoded_length = read_u64(bytes, footer + 16)?;
    if decoded_length > MAX_DECODED_BYTES {
        return Err(Error::DecodedLimit);
    }
    let body_start = layout(file_len, metadata_len, compressed_length)?;
    let metadata_end = usize::try_from(body_start).map_err(|_| Error::LengthMismatch)?;
    let metadata = std::str::from_utf8(
        bytes
            .get(HEADER_BYTES as usize..metadata_end)
            .ok_or(Error::LengthMismatch)?,
    )
    .map_err(|_| Error::MetadataEncoding)?;
    let compressed = bytes
        .get(metadata_end..footer)
        .ok_or(Error::LengthMismatch)?;
    let digest_start = footer + 24;
    let expected_digest = bytes
        .get(digest_start..)
        .and_then(|digest| digest.try_into().ok())
        .ok_or(Error::Truncated)?;
    Ok(Envelope {
        version,
        metadata,
        compressed,
        record_count,
        decoded_length,
        expected_digest,
        bytes_before_digest: &bytes[..digest_start],
    })
}

fn check_file_length(file_len: u64) -> Result<(), Error> {
    if file_len < HEADER_BYTES + FOOTER_BYTES {
        return Err(Error::Truncated);
    }
    if file_len > MAX_FILE_BYTES {
        return Err(Error::FileLimit);
    }
    Ok(())
}

fn layout(file_len: u64, metadata_len: u64, compressed_len: u64) -> Result<u64, Error> {
    let body_start = HEADER_BYTES
        .checked_add(metadata_len)
        .ok_or(Error::LengthMismatch)?;
    let expected = body_start
        .checked_add(compressed_len)
        .and_then(|length| length.checked_add(FOOTER_BYTES))
        .ok_or(Error::LengthMismatch)?;
    if expected != file_len {
        return Err(Error::LengthMismatch);
    }
    Ok(body_start)
}

fn array<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], Error> {
    let end = offset.checked_add(N).ok_or(Error::Truncated)?;
    bytes
        .get(offset..end)
        .and_then(|value| value.try_into().ok())
        .ok_or(Error::Truncated)
}
fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, Error> {
    Ok(u16::from_le_bytes(array(bytes, offset)?))
}
fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, Error> {
    Ok(u32::from_le_bytes(array(bytes, offset)?))
}
fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, Error> {
    Ok(u64::from_le_bytes(array(bytes, offset)?))
}

/// Checks the v1 envelope and its BLAKE3 digest without modifying the input.
///
/// This checks only outer integrity; it does not validate the metadata JSON,
/// zstd frame, decoded records, or file origin.
pub fn verify_v1(bytes: &[u8]) -> Result<Envelope<'_>, Error> {
    let envelope = inspect(bytes)?;
    if blake3::hash(envelope.bytes_before_digest).as_bytes() != envelope.expected_digest {
        return Err(Error::IntegrityFailed);
    }
    Ok(envelope)
}

#[cfg(test)]
mod tests;
