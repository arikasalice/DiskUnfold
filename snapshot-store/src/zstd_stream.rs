//! Bounded, dictionary-free streaming of verified DUSNAP v1 record bytes.
//!
//! The input is borrowed; the caller owns the input buffer and output sink.
//! Decoded bytes are not validated records and must not be committed as a
//! snapshot until metadata and record validation have also succeeded.

use crate::envelope::spool::{SealedSpool, VerifiedSpool};
use std::io::{self, BufRead, BufReader, Read, Write};

use crate::{envelope, record_framing, zstd_frame};
mod item_validation;
mod scope_roots;
pub use scope_roots::{
    PathCaseSensitivity, PathNormalization, PathRuleDescriptor, PathSeparator, ScopeRoots,
};

/// An immutable, already validated Volume record retained from one sealed spool.
///
/// This is record-level evidence only; it does not establish full snapshot validity or publication.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedVolumeRecord {
    body: Vec<u8>,
    root_count: usize,
}

impl ValidatedVolumeRecord {
    /// Returns a typed view of the immutable Volume body that passed record validation.
    pub fn decode(
        &self,
    ) -> Result<crate::records::volume::Volume<'_>, crate::records::volume::Error> {
        crate::records::volume::decode(&self.body, self.root_count)
    }
}

/// A failure while verifying or streaming a DUSNAP frame.
#[derive(Debug)]
pub enum Error {
    /// The outer DUSNAP envelope failed validation.
    Envelope(envelope::Error),
    /// Static frame validation failed.
    Frame(zstd_frame::Error),
    /// The operation was cancelled before completion.
    Cancelled,
    /// The decoder rejected the input, checksum, or frame termination.
    Decode(io::Error),
    /// The sink rejected output. It may have received a partial stream.
    Sink(io::Error),
    /// Decoded output exceeded the footer's declared upper bound.
    DecodedLimit,
    /// The decoded record boundaries, order or count are invalid.
    Records,
    /// Sealed metadata does not contain valid native scope roots and encoding.
    Metadata,
    /// Discarding an unsuccessful staging sink failed.
    Discard {
        failure: Box<Error>,
        cleanup: io::Error,
    },
}

/// Decode the single frame into a caller-supplied bounded sink.
///
/// The entire borrowed envelope must already be bounded by the composition
/// layer. No compressed input copy is made. Successful completion drains the
/// frame to EOF and checks the checksum; it does not parse record payloads.
/// On cancellation, decoder or sink error, no further drain is attempted and
/// the caller must discard all partial output. Cancellation is polled between
/// frame blocks and decoder reads; an in-progress hash, read or sink write is
/// not interruptible by this function.
pub fn decode_v1<W: Write>(
    bytes: &[u8],
    sink: &mut W,
    mut cancelled: impl FnMut() -> bool,
) -> Result<u64, Error> {
    if cancelled() {
        return Err(Error::Cancelled);
    }
    let envelope = envelope::verify_v1(bytes).map_err(Error::Envelope)?;
    decode_verified(&envelope, &mut cancelled, |chunk| {
        sink.write_all(chunk).map_err(Error::Sink)
    })
}

/// Check a v1 envelope, zstd frame and record boundaries, order, and count.
///
/// Record payloads and metadata are not interpreted. A successful result is
/// not sufficient to accept or publish a snapshot. No partial records escape
/// on error or cancellation. Input is borrowed and the hash is computed once.
pub fn validate_record_framing_v1(
    bytes: &[u8],
    mut cancelled: impl FnMut() -> bool,
) -> Result<(), Error> {
    if cancelled() {
        return Err(Error::Cancelled);
    }
    let envelope = envelope::verify_v1(bytes).map_err(Error::Envelope)?;
    let mut records = record_framing::Validator::new(envelope.record_count);
    decode_verified(&envelope, &mut cancelled, |chunk| {
        records.push(chunk).map_err(|()| Error::Records)
    })?;
    records.finish().map_err(|()| Error::Records)
}

fn decode_verified(
    envelope: &envelope::Envelope<'_>,
    cancelled: &mut impl FnMut() -> bool,
    mut consume: impl FnMut(&[u8]) -> Result<(), Error>,
) -> Result<u64, Error> {
    zstd_frame::inspect_frame_cancellable(envelope.compressed, envelope.decoded_length, cancelled)
        .map_err(|e| {
            if e == zstd_frame::Error::Cancelled {
                Error::Cancelled
            } else {
                Error::Frame(e)
            }
        })?;
    decode_reader(
        envelope.compressed,
        envelope.compressed.len() as u64,
        envelope.decoded_length,
        cancelled,
        &mut consume,
    )
}

/// An owned temporary output discarded on unsuccessful decoding or validation.
///
/// Implementations must discard all staged bytes when `discard` succeeds.
/// Returning a sink after success does not authorize publishing it: payload
/// semantics and all other snapshot checks still have to pass.
pub trait StagingSink: Write {
    fn discard(self) -> io::Result<()>;
}

/// Decode and validate record framing from the same verified sealed spool.
///
/// On every failure or cancellation, the owned sink is discarded. If cleanup
/// fails, both the original error and cleanup error are returned. Success
/// returns the staged sink, not a published snapshot; metadata and individual
/// record payloads still require validation. The verified spool remains open.
pub fn validate_and_decode_sealed<S: SealedSpool, W: StagingSink>(
    spool: &mut VerifiedSpool<S>,
    sink: W,
    cancelled: impl FnMut() -> bool,
) -> Result<W, Error> {
    decode_sealed(spool, sink, None, None, false, cancelled).map(|(sink, _)| sink)
}

/// Stages records after checking framing and Item bodies against the verified spool.
///
/// Use the path encoding established by the sealed metadata validator. The
/// caller still needs to validate Volume and Gap bodies before publication.
/// On failure or cancellation, this consumes and discards the staging sink.
pub fn validate_items_in_sealed<S: SealedSpool, W: StagingSink>(
    spool: &mut VerifiedSpool<S>,
    sink: W,
    encoding: crate::records::NameEncoding,
    cancelled: impl FnMut() -> bool,
) -> Result<W, Error> {
    decode_sealed(spool, sink, Some(encoding), None, false, cancelled).map(|(sink, _)| sink)
}

/// Checks framing and Item and Gap bodies using the verified sealed spool.
/// Path encoding and scope roots are read from this same sealed spool's metadata.
/// Volume semantics and full snapshot validation remain outstanding.
/// Duplicate object keys are rejected. The trusted capture-time metadata
/// validator must still validate all remaining required metadata fields.
/// Failure or cancellation consumes and discards the staging sink.
pub fn validate_items_and_gaps_in_sealed<S: SealedSpool, W: StagingSink>(
    spool: &mut VerifiedSpool<S>,
    sink: W,
    cancelled: impl FnMut() -> bool,
) -> Result<(W, ScopeRoots), Error> {
    validate_scoped_records(spool, sink, false, cancelled)
}

/// Validate Volume, Item and Gap bodies using metadata from the same sealed copy.
/// The v1 format, snapshot identity, and source identity and time fields are
/// checked before staging. Other metadata and publication checks remain.
/// Any error or cancellation discards the staging sink.
pub fn validate_records_in_sealed<S: SealedSpool, W: StagingSink>(
    spool: &mut VerifiedSpool<S>,
    sink: W,
    cancelled: impl FnMut() -> bool,
) -> Result<(W, ScopeRoots), Error> {
    validate_scoped_records(spool, sink, true, cancelled)
}

fn validate_scoped_records<S: SealedSpool, W: StagingSink>(
    spool: &mut VerifiedSpool<S>,
    sink: W,
    require_volume: bool,
    cancelled: impl FnMut() -> bool,
) -> Result<(W, ScopeRoots), Error> {
    let mut roots = match scope_roots::parse(spool.metadata(), require_volume) {
        Ok(roots) => roots,
        Err(()) => {
            return match sink.discard() {
                Ok(()) => Err(Error::Metadata),
                Err(cleanup) => Err(Error::Discard {
                    failure: Box::new(Error::Metadata),
                    cleanup,
                }),
            };
        }
    };
    let (sink, volume_body) = decode_sealed(
        spool,
        sink,
        Some(roots.encoding()),
        Some(roots.roots().len()),
        require_volume,
        cancelled,
    )?;
    if let Some(body) = volume_body {
        roots.set_validated_volume_record(ValidatedVolumeRecord {
            body,
            root_count: roots.roots().len(),
        });
    }
    Ok((sink, roots))
}

fn decode_sealed<S: SealedSpool, W: StagingSink>(
    spool: &mut VerifiedSpool<S>,
    mut sink: W,
    encoding: Option<crate::records::NameEncoding>,
    root_count: Option<usize>,
    validate_volume: bool,
    mut cancelled: impl FnMut() -> bool,
) -> Result<(W, Option<Vec<u8>>), Error> {
    let result = (|| {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let compressed_length = spool.compressed_length();
        let decoded_length = spool.decoded_length();
        let mut records = record_framing::Validator::new(spool.record_count());
        let mut items =
            encoding.map(|encoding| item_validation::Validator::new(encoding, root_count));
        if validate_volume && let Some(items) = &mut items {
            items.require_volume();
        }
        let reader = spool.compressed_reader().map_err(Error::Decode)?;
        decode_reader(
            reader,
            compressed_length,
            decoded_length,
            &mut cancelled,
            &mut |chunk| {
                records.push(chunk).map_err(|()| Error::Records)?;
                if let Some(items) = &mut items {
                    items.push(chunk).map_err(|()| Error::Records)?;
                }
                sink.write_all(chunk).map_err(Error::Sink)
            },
        )?;
        records.finish().map_err(|()| Error::Records)?;
        let volume_body = match items {
            Some(items) => items.finish().map_err(|()| Error::Records)?,
            None => None,
        };
        // Do not return a staged result when cancellation arrived during final checks.
        if cancelled() {
            return Err(Error::Cancelled);
        }
        Ok(volume_body)
    })();
    match result {
        Ok(volume_body) => Ok((sink, volume_body)),
        Err(failure) => match sink.discard() {
            Ok(()) => Err(failure),
            Err(cleanup) => Err(Error::Discard {
                failure: Box::new(failure),
                cleanup,
            }),
        },
    }
}

struct Counting<R> {
    input: R,
    bytes: u64,
}
impl<R: Read> Read for Counting<R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let n = self.input.read(output)?;
        self.bytes = self
            .bytes
            .checked_add(n as u64)
            .ok_or_else(|| io::Error::other("compressed byte count overflow"))?;
        Ok(n)
    }
}

fn decode_reader<R: Read>(
    input: R,
    compressed_length: u64,
    decoded_length: u64,
    cancelled: &mut impl FnMut() -> bool,
    consume: &mut impl FnMut(&[u8]) -> Result<(), Error>,
) -> Result<u64, Error> {
    let reader = BufReader::new(Counting { input, bytes: 0 });
    let mut decoder = zstd::stream::read::Decoder::with_buffer(reader)
        .map_err(Error::Decode)?
        .single_frame();
    decoder.window_log_max(27).map_err(Error::Decode)?;
    let mut total = 0u64;
    let mut buffer = [0u8; 16 * 1024];
    loop {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let available = decoded_length - total;
        let read_size = if available == 0 {
            1
        } else {
            available.min(buffer.len() as u64) as usize
        };
        let count = decoder
            .read(&mut buffer[..read_size])
            .map_err(Error::Decode)?;
        if count == 0 {
            break;
        }
        if available == 0 {
            return Err(Error::DecodedLimit);
        }
        consume(&buffer[..count])?;
        total += count as u64;
    }
    decoder.finish_frame().map_err(Error::Decode)?;
    if cancelled() {
        return Err(Error::Cancelled);
    }
    if !decoder
        .get_mut()
        .fill_buf()
        .map_err(Error::Decode)?
        .is_empty()
    {
        return Err(Error::Frame(zstd_frame::Error::TrailingData));
    }
    if decoder.get_ref().get_ref().bytes != compressed_length {
        return Err(Error::Decode(io::Error::from(io::ErrorKind::UnexpectedEof)));
    }
    Ok(total)
}

#[cfg(test)]
mod tests;
