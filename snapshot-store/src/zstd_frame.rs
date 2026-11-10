//! Static DUSNAP v1 Zstandard frame boundary checks.
//!
//! This does not decompress blocks, validate checksums or records, or prove
//! that a frame with no dictionary identifier needs no external dictionary.

use crate::envelope;
use std::io::Read;

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
    /// The caller cancelled frame inspection.
    Cancelled,
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
    inspect_frame_cancellable(bytes, decoded_limit, &mut || false)
}

/// Read failures remain distinct from malformed or cancelled frames.
#[derive(Debug)]
pub(crate) enum ReadError {
    Frame(Error),
    Io(std::io::Error),
}

trait Input {
    fn read(
        &mut self,
        bytes: &mut [u8],
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<(), ReadError>;
    fn skip(&mut self, length: usize, cancelled: &mut dyn FnMut() -> bool)
    -> Result<(), ReadError>;
    fn remaining(&self) -> u64;
}

struct SliceInput<'a>(&'a [u8]);
impl Input for SliceInput<'_> {
    fn read(
        &mut self,
        bytes: &mut [u8],
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<(), ReadError> {
        checkpoint(cancelled)?;
        let source = self
            .0
            .get(..bytes.len())
            .ok_or(ReadError::Frame(Error::InvalidFrame))?;
        bytes.copy_from_slice(source);
        self.0 = &self.0[bytes.len()..];
        Ok(())
    }
    fn skip(
        &mut self,
        length: usize,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<(), ReadError> {
        checkpoint(cancelled)?;
        self.0 = self
            .0
            .get(length..)
            .ok_or(ReadError::Frame(Error::InvalidFrame))?;
        Ok(())
    }
    fn remaining(&self) -> u64 {
        self.0.len() as u64
    }
}

struct ReaderInput<'a, R>(&'a mut std::io::Take<R>);
impl<R: std::io::Read> Input for ReaderInput<'_, R> {
    fn read(
        &mut self,
        mut bytes: &mut [u8],
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<(), ReadError> {
        while !bytes.is_empty() {
            checkpoint(cancelled)?;
            match self.0.read(bytes) {
                Ok(0) => return Err(ReadError::Frame(Error::InvalidFrame)),
                Ok(n) => bytes = &mut bytes[n..],
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => return Err(ReadError::Io(error)),
            }
        }
        Ok(())
    }
    fn skip(
        &mut self,
        mut length: usize,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<(), ReadError> {
        let mut buffer = [0; 16 * 1024];
        while length > 0 {
            let n = length.min(buffer.len());
            self.read(&mut buffer[..n], cancelled)?;
            length -= n;
        }
        Ok(())
    }
    fn remaining(&self) -> u64 {
        self.0.limit()
    }
}

fn checkpoint(cancelled: &mut dyn FnMut() -> bool) -> Result<(), ReadError> {
    if cancelled() {
        Err(ReadError::Frame(Error::Cancelled))
    } else {
        Ok(())
    }
}

pub(crate) fn inspect_frame_cancellable(
    bytes: &[u8],
    decoded_limit: u64,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<FrameInfo, Error> {
    match inspect(&mut SliceInput(bytes), decoded_limit, cancelled) {
        Ok(info) => Ok(info),
        Err(ReadError::Frame(error)) => Err(error),
        Err(ReadError::Io(_)) => Err(Error::InvalidFrame),
    }
}

pub(crate) fn inspect_reader<R: std::io::Read>(
    reader: &mut std::io::Take<R>,
    decoded_limit: u64,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<FrameInfo, ReadError> {
    inspect(&mut ReaderInput(reader), decoded_limit, cancelled)
}

fn inspect(
    input: &mut impl Input,
    decoded_limit: u64,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<FrameInfo, ReadError> {
    let mut fixed = [0; 5];
    input.read(&mut fixed, cancelled)?;
    if fixed[..4] != MAGIC || fixed[4] & 0x08 != 0 {
        return Err(ReadError::Frame(Error::InvalidFrame));
    }
    let descriptor = fixed[4];
    let single = descriptor & 0x20 != 0;
    let window = if single {
        None
    } else {
        let mut raw = [0];
        input.read(&mut raw, cancelled)?;
        let shift = 10 + u32::from(raw[0] >> 3);
        let base = 1u64
            .checked_shl(shift)
            .ok_or(ReadError::Frame(Error::WindowLimit))?;
        Some(base + (base / 8) * u64::from(raw[0] & 7))
    };
    let dictionary_len = [0, 1, 2, 4][usize::from(descriptor & 3)];
    let mut dictionary = [0; 4];
    input.read(&mut dictionary[..dictionary_len], cancelled)?;
    if dictionary[..dictionary_len].iter().any(|v| *v != 0) {
        return Err(ReadError::Frame(Error::Dictionary));
    }
    let content_len = match descriptor >> 6 {
        0 if single => 1,
        0 => 0,
        1 => 2,
        2 => 4,
        _ => 8,
    };
    let mut raw = [0; 8];
    input.read(&mut raw[..content_len], cancelled)?;
    let content_size = (content_len != 0)
        .then(|| u64::from_le_bytes(raw) + if content_len == 2 { 256 } else { 0 });
    if content_size.is_some_and(|size| size > decoded_limit) {
        return Err(ReadError::Frame(Error::DecodedLimit));
    }
    let window_size = window
        .or(content_size)
        .ok_or(ReadError::Frame(Error::InvalidFrame))?;
    if window_size > MAX_WINDOW {
        return Err(ReadError::Frame(Error::WindowLimit));
    }
    blocks(input, window_size, cancelled)?;
    if descriptor & 4 != 0 {
        input.skip(4, cancelled)?;
    }
    if input.remaining() != 0 {
        return Err(ReadError::Frame(Error::TrailingData));
    }
    Ok(FrameInfo {
        window_size,
        content_size,
    })
}

fn blocks(
    input: &mut impl Input,
    window: u64,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<(), ReadError> {
    loop {
        let mut raw = [0; 3];
        input.read(&mut raw, cancelled)?;
        let header = u32::from_le_bytes([raw[0], raw[1], raw[2], 0]);
        let kind = (header >> 1) & 3;
        let length = u64::from(header >> 3);
        if kind == 3 || length > window.min(MAX_BLOCK) {
            return Err(ReadError::Frame(Error::InvalidFrame));
        }
        let stored = if kind == 1 { 1 } else { length as usize };
        input.skip(stored, cancelled)?;
        if header & 1 != 0 {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests;
