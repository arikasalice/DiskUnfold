//! Static DUSNAP v1 Zstandard frame boundary checks.
//!
//! This does not decompress blocks, validate checksums or records, or prove
//! that a frame with no dictionary identifier needs no external dictionary.

use crate::envelope;

const MAGIC: [u8; 4] = [0x28, 0xb5, 0x2f, 0xfd];
const MAX_WINDOW: u64 = 1 << 27;
const MAX_BLOCK: u64 = 128 * 1024;

/// Failure in outer integrity or static Zstandard framing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The DUSNAP envelope failed validation.
    Envelope(envelope::Error),
    /// The frame header or block framing is malformed or truncated.
    InvalidFrame,
    /// A nonzero dictionary identifier is not supported.
    Dictionary,
    /// The frame's window exceeds 128 MiB.
    WindowLimit,
    /// The advertised decoded size exceeds the DUSNAP footer limit.
    DecodedLimit,
    /// Bytes remain after the one frame, including additional frames.
    TrailingData,
}

/// Information visible from a frame header, without decompressing its blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameInfo {
    /// Effective window size in bytes, including the single-segment case.
    pub window_size: u64,
    /// Advertised decoded size, if the frame header includes it.
    pub content_size: Option<u64>,
}

/// Verify the DUSNAP envelope and inspect the boundaries of its one frame.
///
/// A successful result is not a valid decoded snapshot: a dictionary-free
/// decoder must still consume the entire frame, enforce output and cancellation
/// limits, verify any checksum, and parse and validate every record.
pub fn inspect_v1(bytes: &[u8]) -> Result<FrameInfo, Error> {
    let envelope = envelope::verify_v1(bytes).map_err(Error::Envelope)?;
    inspect_frame(envelope.compressed, envelope.decoded_length)
}

fn inspect_frame(bytes: &[u8], decoded_limit: u64) -> Result<FrameInfo, Error> {
    if bytes.get(..4) != Some(MAGIC.as_slice()) {
        return Err(Error::InvalidFrame);
    }
    let mut pos = 4;
    let descriptor = take(bytes, &mut pos, 1)?[0];
    if descriptor & 0x08 != 0 {
        return Err(Error::InvalidFrame);
    }
    let single = descriptor & 0x20 != 0;
    let window = if single {
        None
    } else {
        let value = take(bytes, &mut pos, 1)?[0];
        let shift = 10 + u32::from(value >> 3);
        let base = 1u64.checked_shl(shift).ok_or(Error::WindowLimit)?;
        Some(base + (base / 8) * u64::from(value & 7))
    };
    let dictionary_len = [0, 1, 2, 4][usize::from(descriptor & 3)];
    if take(bytes, &mut pos, dictionary_len)?
        .iter()
        .any(|&v| v != 0)
    {
        return Err(Error::Dictionary);
    }
    let content_len = match descriptor >> 6 {
        0 if single => 1,
        0 => 0,
        1 => 2,
        2 => 4,
        _ => 8,
    };
    let content_size = if content_len == 0 {
        None
    } else {
        let raw = take(bytes, &mut pos, content_len)?;
        let mut value = [0u8; 8];
        value[..content_len].copy_from_slice(raw);
        let size = u64::from_le_bytes(value) + if content_len == 2 { 256 } else { 0 };
        Some(size)
    };
    if content_size.is_some_and(|size| size > decoded_limit) {
        return Err(Error::DecodedLimit);
    }
    let window_size = window.or(content_size).ok_or(Error::InvalidFrame)?;
    if window_size > MAX_WINDOW {
        return Err(Error::WindowLimit);
    }
    blocks(bytes, &mut pos, window_size)?;
    if descriptor & 4 != 0 {
        take(bytes, &mut pos, 4)?;
    }
    if pos != bytes.len() {
        return Err(Error::TrailingData);
    }
    Ok(FrameInfo {
        window_size,
        content_size,
    })
}

fn blocks(bytes: &[u8], pos: &mut usize, window: u64) -> Result<(), Error> {
    loop {
        let raw = take(bytes, pos, 3)?;
        let header = u32::from_le_bytes([raw[0], raw[1], raw[2], 0]);
        let last = header & 1 != 0;
        let kind = (header >> 1) & 3;
        let length = u64::from(header >> 3);
        if kind == 3 || length > window.min(MAX_BLOCK) {
            return Err(Error::InvalidFrame);
        }
        let stored = if kind == 1 { 1 } else { length as usize };
        take(bytes, pos, stored)?;
        if last {
            return Ok(());
        }
    }
}

fn take<'a>(bytes: &'a [u8], pos: &mut usize, count: usize) -> Result<&'a [u8], Error> {
    let end = pos.checked_add(count).ok_or(Error::InvalidFrame)?;
    let value = bytes.get(*pos..end).ok_or(Error::InvalidFrame)?;
    *pos = end;
    Ok(value)
}

#[cfg(test)]
mod tests;
