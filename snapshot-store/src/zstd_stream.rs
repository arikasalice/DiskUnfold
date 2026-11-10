//! Bounded, dictionary-free streaming of verified DUSNAP v1 record bytes.
//!
//! The input is borrowed; the caller owns the input buffer and output sink.
//! Decoded bytes are not validated records and must not be committed as a
//! snapshot until metadata and record validation have also succeeded.

use std::io::{self, Read, Write};

use crate::{envelope, zstd_frame};

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
    zstd_frame::inspect_frame_cancellable(
        envelope.compressed,
        envelope.decoded_length,
        &mut cancelled,
    )
    .map_err(|e| {
        if e == zstd_frame::Error::Cancelled {
            Error::Cancelled
        } else {
            Error::Frame(e)
        }
    })?;
    let mut decoder = zstd::stream::read::Decoder::with_buffer(envelope.compressed)
        .map_err(Error::Decode)?
        .single_frame();
    decoder.window_log_max(27).map_err(Error::Decode)?;
    let mut total = 0u64;
    let mut buffer = [0u8; 16 * 1024];
    loop {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let available = envelope.decoded_length - total;
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
        sink.write_all(&buffer[..count]).map_err(Error::Sink)?;
        total += count as u64;
    }
    decoder.finish_frame().map_err(Error::Decode)?;
    if !decoder.get_ref().is_empty() {
        return Err(Error::Frame(zstd_frame::Error::TrailingData));
    }
    Ok(total)
}

#[cfg(test)]
mod tests;
