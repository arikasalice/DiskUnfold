//! Bounded capture into an injected immutable spool and outer v1 verification.
//!
//! Spool implementations are trusted composition-boundary adapters: they must
//! enforce their documented locking, immutability, and cleanup guarantees.
use super::{
    Error as FormatError, FOOTER_BYTES, MAGIC, MAX_DECODED_BYTES, MAX_FILE_BYTES,
    MAX_METADATA_BYTES, check_file_length, layout, read_u16, read_u32, read_u64,
};
use crate::zstd_frame::Error as FrameError;
pub use du_core_contracts::snapshot_spool::{CaptureSpool, SealedSpool};
use std::io::{self, Read, Seek, SeekFrom, Write};

/// Failure while capturing or inspecting; no verified spool is returned.
#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Cancelled,
    Format(FormatError),
    Frame(FrameError),
    /// The injected metadata validator rejected the captured metadata.
    Metadata,
    /// The stored catalog digest does not match the captured file digest.
    IndexIntegrity,
}

/// Validated outer container in the same sealed spool used during verification.
/// This is not a validated snapshot: payload, record semantics, and metadata
/// beyond the injected metadata checks still require validation before publication.
pub struct VerifiedSpool<S: SealedSpool> {
    spool: S,
    /// UTF-8 metadata, validated by the injected metadata parser.
    metadata: String,
    compressed_offset: u64,
    compressed_length: u64,
    record_count: u64,
    decoded_length: u64,
}
impl<S: SealedSpool> VerifiedSpool<S> {
    /// Returns the validated metadata text without permitting mutation.
    pub fn metadata(&self) -> &str {
        &self.metadata
    }
    /// Returns the captured compressed body length.
    pub fn compressed_length(&self) -> u64 {
        self.compressed_length
    }
    /// Returns the declared record count; records have not yet been validated.
    pub fn record_count(&self) -> u64 {
        self.record_count
    }
    /// Returns the declared decoded length limit.
    pub fn decoded_length(&self) -> u64 {
        self.decoded_length
    }
    /// Opens only the captured compressed range on the same sealed spool.
    pub fn compressed_reader(&mut self) -> io::Result<CompressedReader<'_, S>> {
        self.spool.seek(SeekFrom::Start(self.compressed_offset))?;
        Ok(CompressedReader {
            inner: (&mut self.spool).take(self.compressed_length),
        })
    }
}

/// A read-only view of the verified compressed range.
pub struct CompressedReader<'a, S: SealedSpool> {
    inner: io::Take<&'a mut S>,
}
impl<S: SealedSpool> Read for CompressedReader<'_, S> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.inner.read(bytes)
    }
}

/// Sequentially copies the source once, seals the destination, and verifies it.
///
/// `validate_metadata` is a trusted metadata parser that must validate JSON,
/// required fields, and agreement with the header without selecting a codec.
/// `index_digest` is required by the caller for catalog-owned snapshots and is
/// absent for external inputs. The copy is capped at 4 GiB by bytes actually
/// read, with an additional read to detect growth past the limit. Cancellation
/// is checked between I/O operations. A failed or cancelled call drops its spool.
pub fn capture_v1<R: Read, W: CaptureSpool>(
    source: &mut R,
    mut spool: W,
    index_digest: Option<[u8; 32]>,
    validate_metadata: impl FnOnce(&str, u16) -> bool,
    mut cancelled: impl FnMut() -> bool,
) -> Result<VerifiedSpool<W::Sealed>, Error> {
    copy(source, &mut spool, MAX_FILE_BYTES, &mut cancelled)?;
    check_cancel(&mut cancelled)?;
    let sealed = spool.seal().map_err(Error::Io)?;
    verify(sealed, index_digest, validate_metadata, &mut cancelled)
}

fn copy<R: Read, W: Write>(
    source: &mut R,
    output: &mut W,
    max: u64,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<(), Error> {
    let mut total = 0_u64;
    let mut buffer = [0u8; 16 * 1024];
    loop {
        check_cancel(cancelled)?;
        let remaining = max - total;
        let read_len = if remaining == 0 {
            1
        } else {
            remaining.min(buffer.len() as u64) as usize
        };
        let count = match source.read(&mut buffer[..read_len]) {
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            other => other.map_err(Error::Io)?,
        };
        if count == 0 {
            break;
        }
        if remaining == 0 {
            return Err(Error::Format(FormatError::FileLimit));
        }
        let mut pending = &buffer[..count];
        while !pending.is_empty() {
            check_cancel(cancelled)?;
            match output.write(pending) {
                Ok(0) => return Err(Error::Io(io::Error::from(io::ErrorKind::WriteZero))),
                Ok(n) => pending = &pending[n..],
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(Error::Io(e)),
            }
        }
        total += count as u64;
    }
    check_file_length(total).map_err(Error::Format)
}

fn verify<S: SealedSpool>(
    mut spool: S,
    index_digest: Option<[u8; 32]>,
    validate_metadata: impl FnOnce(&str, u16) -> bool,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<VerifiedSpool<S>, Error> {
    check_cancel(cancelled)?;
    let len = spool.seek(SeekFrom::End(0)).map_err(Error::Io)?;
    check_file_length(len).map_err(Error::Format)?;
    spool.seek(SeekFrom::Start(0)).map_err(Error::Io)?;
    let mut header = [0; 16];
    read_exact(&mut spool, &mut header, cancelled)?;
    if &header[..8] != MAGIC {
        return Err(Error::Format(FormatError::Magic));
    }
    let version = read_u16(&header, 8).map_err(Error::Format)?;
    if version != 1 {
        return Err(Error::Format(FormatError::UnsupportedVersion));
    }
    if read_u16(&header, 10).map_err(Error::Format)? != 0 {
        return Err(Error::Format(FormatError::Flags));
    }
    let metadata_len = u64::from(read_u32(&header, 12).map_err(Error::Format)?);
    if !(1..=MAX_METADATA_BYTES).contains(&metadata_len) {
        return Err(Error::Format(FormatError::MetadataLength));
    }
    spool
        .seek(SeekFrom::Start(len - FOOTER_BYTES))
        .map_err(Error::Io)?;
    let mut footer = [0; 56];
    read_exact(&mut spool, &mut footer, cancelled)?;
    let compressed_length = read_u64(&footer, 0).map_err(Error::Format)?;
    let record_count = read_u64(&footer, 8).map_err(Error::Format)?;
    let decoded_length = read_u64(&footer, 16).map_err(Error::Format)?;
    let compressed_offset = layout(len, metadata_len, compressed_length).map_err(Error::Format)?;
    if decoded_length > MAX_DECODED_BYTES {
        return Err(Error::Format(FormatError::DecodedLimit));
    }
    spool.seek(SeekFrom::Start(16)).map_err(Error::Io)?;
    let mut metadata = vec![0; metadata_len as usize];
    read_exact(&mut spool, &mut metadata, cancelled)?;
    let metadata =
        String::from_utf8(metadata).map_err(|_| Error::Format(FormatError::MetadataEncoding))?;
    if !validate_metadata(&metadata, version) {
        return Err(Error::Metadata);
    }
    spool
        .seek(SeekFrom::Start(compressed_offset))
        .map_err(Error::Io)?;
    crate::zstd_frame::inspect_reader(
        &mut (&mut spool).take(compressed_length),
        decoded_length,
        cancelled,
    )
    .map_err(|error| match error {
        crate::zstd_frame::ReadError::Frame(FrameError::Cancelled) => Error::Cancelled,
        crate::zstd_frame::ReadError::Frame(error) => Error::Frame(error),
        crate::zstd_frame::ReadError::Io(error) => Error::Io(error),
    })?;
    digest(&mut spool, len, &footer[24..], index_digest, cancelled)?;
    // Do not return the sealed handle if cancellation arrived during final verification.
    check_cancel(cancelled)?;
    Ok(VerifiedSpool {
        spool,
        metadata,
        compressed_offset,
        compressed_length,
        record_count,
        decoded_length,
    })
}

fn digest<R: Read + Seek>(
    spool: &mut R,
    len: u64,
    expected: &[u8],
    index: Option<[u8; 32]>,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<(), Error> {
    spool.seek(SeekFrom::Start(0)).map_err(Error::Io)?;
    let mut left = len - 32;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0; 16 * 1024];
    while left > 0 {
        let n = left.min(buffer.len() as u64) as usize;
        read_exact(spool, &mut buffer[..n], cancelled)?;
        hasher.update(&buffer[..n]);
        left -= n as u64;
    }
    check_cancel(cancelled)?;
    let actual = hasher.finalize();
    if actual.as_bytes() != expected {
        return Err(Error::Format(FormatError::IntegrityFailed));
    }
    if index.is_some_and(|value| &value != actual.as_bytes()) {
        return Err(Error::IndexIntegrity);
    }
    Ok(())
}

fn read_exact<R: Read>(
    reader: &mut R,
    mut bytes: &mut [u8],
    cancelled: &mut impl FnMut() -> bool,
) -> Result<(), Error> {
    while !bytes.is_empty() {
        check_cancel(cancelled)?;
        match reader.read(bytes) {
            Ok(0) => return Err(Error::Format(FormatError::Truncated)),
            Ok(n) => bytes = &mut bytes[n..],
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(Error::Io(e)),
        }
    }
    Ok(())
}
fn check_cancel(cancelled: &mut impl FnMut() -> bool) -> Result<(), Error> {
    if cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
