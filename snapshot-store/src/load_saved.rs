//! Read-only loading of a registered snapshot file into an isolated result.

use crate::{
    envelope::spool::{self, Error as CaptureError},
    load::{self, LoadError},
    open::{self, OpenError, OpenErrorKind},
    zstd_frame,
    zstd_stream::StagingSink,
};
use du_analysis_index::ResultVersion;
use du_core_contracts::{
    Id,
    snapshot_file::{SnapshotFileError, SnapshotFilePort},
};
use serde_json::Value;
use std::{
    fmt,
    io::{self, ErrorKind, Read, Seek},
    sync::Arc,
};

/// Typed failure category returned when a registered snapshot cannot be loaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadSavedErrorKind {
    /// No registered final file exists for the requested identifier.
    NotFound,
    /// Reading the registered source or sealed capture failed; the kind is diagnostic only.
    ReadFailed(ErrorKind),
    /// The snapshot volume has insufficient available space for the known copy size.
    InsufficientSpace,
    /// Creating or writing the private capture failed; the kind is diagnostic only.
    WriteFailed(ErrorKind),
    /// The copied file does not satisfy the DUSNAP structure or metadata contract.
    FormatInvalid,
    /// The copied file's integrity digest or expected byte count does not match.
    IntegrityFailed,
    /// The file or decoded stream exceeds an applied size limit.
    DecompressLimit,
    /// The file uses a format not supported by this build.
    FormatUnsupported,
    /// The file declares a format newer than this build.
    FormatNewer,
    /// The operation was cancelled.
    Cancelled,
    /// Staged output failed or could not be discarded.
    Staging,
    /// The snapshot store is unavailable.
    StoreUnavailable,
}

/// Failure while capturing, validating, or loading one saved snapshot.
#[derive(Debug)]
pub struct LoadSavedError {
    kind: LoadSavedErrorKind,
    cleanup_failed: bool,
}

impl LoadSavedError {
    /// Returns the typed category and any diagnostic-only I/O kind.
    pub fn kind(&self) -> LoadSavedErrorKind {
        self.kind
    }

    /// Returns the stable F07 reason key, or none for cancellation and diagnostic I/O failures.
    pub fn reason_key(&self) -> Option<&'static str> {
        Some(match self.kind {
            LoadSavedErrorKind::NotFound => "snapshot.not_found",
            LoadSavedErrorKind::InsufficientSpace => "snapshot.insufficient_space",
            LoadSavedErrorKind::WriteFailed(_) => "snapshot.write_failed",
            LoadSavedErrorKind::FormatInvalid => "snapshot.format_invalid",
            LoadSavedErrorKind::IntegrityFailed => "snapshot.integrity_failed",
            LoadSavedErrorKind::DecompressLimit => "snapshot.decompress_limit",
            LoadSavedErrorKind::FormatUnsupported => "snapshot.format_unsupported",
            LoadSavedErrorKind::FormatNewer => "snapshot.format_newer",
            LoadSavedErrorKind::StoreUnavailable => "snapshot.store_unavailable",
            LoadSavedErrorKind::ReadFailed(_)
            | LoadSavedErrorKind::Cancelled
            | LoadSavedErrorKind::Staging => return None,
        })
    }

    /// Reports whether discarding staged output also failed.
    pub fn cleanup_failed(&self) -> bool {
        self.cleanup_failed
    }
}

impl fmt::Display for LoadSavedError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "saved snapshot load failed: {:?}", self.kind)
    }
}

impl std::error::Error for LoadSavedError {}

/// Opens a registered file, copies it into a private locked spool, verifies that
/// copy, and loads it as an immutable snapshot result.
///
/// `expected_size`, when supplied from the index row, is compared with the full
/// number of bytes copied. The provided staging sink must bound decoded output
/// without retaining a potentially multi-gigabyte stream in memory. The source
/// file is opened read-only and never modified. The private capture is removed
/// on success, cancellation, and every failure; reconciliation must not report
/// or auto-delete a leftover capture as `snapshot.orphan_file`.
///
/// The returned `Arc<ResultVersion>` has `DataSource::Snapshot`. Native Windows
/// behavior is covered by the platform adapter test; Linux and macOS native
/// behavior is unverified on hosts that do not run those operating systems.
pub fn load_saved<W>(
    port: &dyn SnapshotFilePort,
    snapshot_id: &Id,
    expected_size: Option<u64>,
    staging: W,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Arc<ResultVersion>, LoadSavedError>
where
    W: StagingSink + Read + Seek,
{
    if cancelled() {
        return Err(discard(staging, LoadSavedErrorKind::Cancelled));
    }
    let source = match port.open_registered(snapshot_id) {
        Ok(source) => source,
        Err(error) => return Err(discard(staging, file_error(error))),
    };
    let capture = match port.create_capture(expected_size) {
        Ok(capture) => capture,
        Err(error) => return Err(discard(staging, file_error(error))),
    };
    let mut source = CountingReader::new(source);
    let verified = match spool::capture_v1(
        &mut source,
        capture,
        None,
        |metadata, version| metadata_matches(metadata, version, snapshot_id),
        &mut cancelled,
    ) {
        Ok(verified) => verified,
        Err(error) => {
            let kind = capture_error(error, source.version());
            return Err(discard(staging, kind));
        }
    };
    if expected_size.is_some_and(|expected| expected != source.bytes_read()) {
        drop(verified);
        return Err(discard(staging, LoadSavedErrorKind::IntegrityFailed));
    }
    let opened = match open::open(verified, staging, &mut cancelled) {
        Ok(opened) => opened,
        Err(error) => return Err(open_error(error)),
    };
    load::load(opened, &mut cancelled).map_err(load_error)
}

struct CountingReader<'a> {
    reader: Box<dyn Read + Send + 'a>,
    bytes_read: u64,
    prefix: [u8; 10],
    prefix_len: usize,
}

impl<'a> CountingReader<'a> {
    fn new(reader: Box<dyn Read + Send + 'a>) -> Self {
        Self {
            reader,
            bytes_read: 0,
            prefix: [0; 10],
            prefix_len: 0,
        }
    }

    fn bytes_read(&self) -> u64 {
        self.bytes_read
    }

    fn version(&self) -> Option<u16> {
        (self.prefix_len >= 10).then(|| u16::from_le_bytes([self.prefix[8], self.prefix[9]]))
    }
}

impl Read for CountingReader<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let count = self.reader.read(bytes)?;
        let prefix_count = (self.prefix.len() - self.prefix_len).min(count);
        self.prefix[self.prefix_len..self.prefix_len + prefix_count]
            .copy_from_slice(&bytes[..prefix_count]);
        self.prefix_len += prefix_count;
        self.bytes_read = self.bytes_read.saturating_add(count as u64);
        Ok(count)
    }
}

fn metadata_matches(metadata: &str, version: u16, requested_id: &Id) -> bool {
    version == crate::envelope::DUSNAP_FORMAT_VERSION
        && serde_json::from_str::<Value>(metadata).is_ok_and(|value| {
            value.get("formatId").and_then(Value::as_str) == Some("dusnap")
                && value.get("formatVersion").and_then(Value::as_u64) == Some(u64::from(version))
                && value.get("snapshotId").and_then(Value::as_str) == Some(requested_id.as_str())
        })
}

fn capture_error(error: CaptureError, version: Option<u16>) -> LoadSavedErrorKind {
    match error {
        CaptureError::Cancelled => LoadSavedErrorKind::Cancelled,
        CaptureError::Read(error) => LoadSavedErrorKind::ReadFailed(error.kind()),
        CaptureError::Io(error) => LoadSavedErrorKind::WriteFailed(error.kind()),
        CaptureError::Format(error) => format_error(error, version),
        CaptureError::Frame(error) => frame_error(error),
        CaptureError::Metadata => LoadSavedErrorKind::FormatInvalid,
        CaptureError::IndexIntegrity => LoadSavedErrorKind::IntegrityFailed,
    }
}

fn format_error(error: crate::envelope::Error, version: Option<u16>) -> LoadSavedErrorKind {
    use crate::envelope::Error as FormatError;
    match error {
        FormatError::IntegrityFailed => LoadSavedErrorKind::IntegrityFailed,
        FormatError::FileLimit | FormatError::DecodedLimit => LoadSavedErrorKind::DecompressLimit,
        FormatError::UnsupportedVersion
            if version.is_some_and(|v| v > crate::envelope::DUSNAP_FORMAT_VERSION) =>
        {
            LoadSavedErrorKind::FormatNewer
        }
        FormatError::UnsupportedVersion => LoadSavedErrorKind::FormatUnsupported,
        FormatError::Truncated
        | FormatError::Magic
        | FormatError::Flags
        | FormatError::MetadataLength
        | FormatError::MetadataEncoding
        | FormatError::LengthMismatch => LoadSavedErrorKind::FormatInvalid,
    }
}

fn frame_error(error: zstd_frame::Error) -> LoadSavedErrorKind {
    match error {
        zstd_frame::Error::DecodedLimit => LoadSavedErrorKind::DecompressLimit,
        zstd_frame::Error::Dictionary | zstd_frame::Error::WindowLimit => {
            LoadSavedErrorKind::FormatUnsupported
        }
        zstd_frame::Error::Cancelled => LoadSavedErrorKind::Cancelled,
        zstd_frame::Error::InvalidFrame | zstd_frame::Error::TrailingData => {
            LoadSavedErrorKind::FormatInvalid
        }
        zstd_frame::Error::Envelope(error) => format_error(error, None),
    }
}

fn open_error(error: OpenError) -> LoadSavedError {
    let kind = match error.kind() {
        OpenErrorKind::FormatInvalid => LoadSavedErrorKind::FormatInvalid,
        OpenErrorKind::FormatUnsupported => LoadSavedErrorKind::FormatUnsupported,
        OpenErrorKind::FormatNewer => LoadSavedErrorKind::FormatNewer,
        OpenErrorKind::IntegrityFailed => LoadSavedErrorKind::IntegrityFailed,
        OpenErrorKind::DecompressLimit => LoadSavedErrorKind::DecompressLimit,
        OpenErrorKind::Cancelled => LoadSavedErrorKind::Cancelled,
        OpenErrorKind::Io => {
            LoadSavedErrorKind::ReadFailed(error.io_error_kind().unwrap_or(ErrorKind::Other))
        }
        OpenErrorKind::Staging => LoadSavedErrorKind::Staging,
    };
    LoadSavedError {
        kind,
        cleanup_failed: error.cleanup_failed(),
    }
}

fn load_error(error: LoadError) -> LoadSavedError {
    let (kind, cleanup_failed) = match error {
        LoadError::Cancelled => (LoadSavedErrorKind::Cancelled, false),
        LoadError::InvalidSnapshot | LoadError::Build(_) => {
            (LoadSavedErrorKind::FormatInvalid, false)
        }
        LoadError::Io(error) => (LoadSavedErrorKind::ReadFailed(error.kind()), false),
        LoadError::Cleanup(_) => (LoadSavedErrorKind::Staging, true),
        LoadError::Discard { failure, .. } => (load_error(*failure).kind, true),
    };
    LoadSavedError {
        kind,
        cleanup_failed,
    }
}

fn file_error(error: SnapshotFileError) -> LoadSavedErrorKind {
    match error {
        SnapshotFileError::NotFound => LoadSavedErrorKind::NotFound,
        SnapshotFileError::ReadFailed(kind) => LoadSavedErrorKind::ReadFailed(kind),
        SnapshotFileError::InsufficientSpace => LoadSavedErrorKind::InsufficientSpace,
        SnapshotFileError::WriteFailed(kind) => LoadSavedErrorKind::WriteFailed(kind),
        SnapshotFileError::StoreUnavailable | SnapshotFileError::Unsupported => {
            LoadSavedErrorKind::StoreUnavailable
        }
    }
}

fn discard<W: StagingSink>(staging: W, kind: LoadSavedErrorKind) -> LoadSavedError {
    LoadSavedError {
        kind,
        cleanup_failed: staging.discard().is_err(),
    }
}
