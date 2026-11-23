//! Ordered verification and read-only admission for one sealed snapshot.

use crate::{
    envelope::{
        self,
        format::{self, Reader},
        spool::SealedSpool,
    },
    zstd_frame,
    zstd_stream::{self, PathRuleDescriptor, ScopeRoots, ValidatedVolumeRecord},
};
use serde_json::Value;
use std::{
    fmt,
    io::{self, Read, Seek, SeekFrom},
};

/// Non-corrupt compatibility outcome when no current filesystem facts were supplied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenCompatibility {
    /// The snapshot remains observation-only until algorithms and current path/volume facts match.
    ObservationOnly,
}

/// The failed open verification category.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenErrorKind {
    /// The envelope, metadata, or records are malformed.
    FormatInvalid,
    /// The compression or format is not supported by this build.
    FormatUnsupported,
    /// The file declares a format newer than this build.
    FormatNewer,
    /// The sealed file's integrity digest does not match.
    IntegrityFailed,
    /// The file or decoded stream exceeds its applied size limit.
    DecompressLimit,
    /// Verification was cancelled.
    Cancelled,
    /// Reading the sealed input failed.
    Io,
    /// Reading or writing the staged record stream failed.
    Staging,
}

/// Failure while opening a sealed snapshot, including any staging cleanup failure.
#[derive(Debug)]
pub struct OpenError {
    kind: OpenErrorKind,
    source: Option<io::Error>,
    cleanup: Option<io::Error>,
}

impl OpenError {
    fn new(kind: OpenErrorKind) -> Self {
        Self {
            kind,
            source: None,
            cleanup: None,
        }
    }

    fn caused_by(kind: OpenErrorKind, source: io::Error) -> Self {
        Self {
            kind,
            source: Some(source),
            cleanup: None,
        }
    }

    /// Returns the typed failure category.
    pub fn kind(&self) -> OpenErrorKind {
        self.kind
    }

    /// Returns the F07 reason key for snapshot failures, or none for cancellation and I/O.
    pub fn reason_key(&self) -> Option<&'static str> {
        Some(match self.kind {
            OpenErrorKind::FormatInvalid => "snapshot.format_invalid",
            OpenErrorKind::FormatUnsupported => "snapshot.format_unsupported",
            OpenErrorKind::FormatNewer => "snapshot.format_newer",
            OpenErrorKind::IntegrityFailed => "snapshot.integrity_failed",
            OpenErrorKind::DecompressLimit => "snapshot.decompress_limit",
            OpenErrorKind::Cancelled | OpenErrorKind::Io | OpenErrorKind::Staging => return None,
        })
    }

    /// Reports whether the staging sink also failed to discard its temporary payload.
    pub fn cleanup_failed(&self) -> bool {
        self.cleanup.is_some()
    }

    pub(crate) fn io_error_kind(&self) -> Option<io::ErrorKind> {
        self.source.as_ref().map(io::Error::kind)
    }
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "snapshot open failed: {:?}", self.kind)
    }
}

impl std::error::Error for OpenError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|error| error as &(dyn std::error::Error + 'static))
            .or_else(|| {
                self.cleanup
                    .as_ref()
                    .map(|error| error as &(dyn std::error::Error + 'static))
            })
    }
}

/// A move-only token for a snapshot whose metadata and all records passed open validation.
///
/// Its private fields prevent callers from constructing or changing verified evidence. The
/// record stream is exposed through a read/seek-only view and can be handed to the next owner by
/// moving this token.
#[must_use]
pub struct VerifiedSnapshot<W: zstd_stream::StagingSink + Read + Seek> {
    scope_roots: ScopeRoots,
    records: W,
    compatibility: OpenCompatibility,
}

impl<W: zstd_stream::StagingSink + Read + Seek> VerifiedSnapshot<W> {
    /// Returns the complete parsed metadata from the same sealed snapshot.
    pub fn metadata(&self) -> &Value {
        self.scope_roots.metadata()
    }

    /// Returns the validated source bundle, including algorithm IDs and completeness evidence.
    pub fn source_bundle(&self) -> Option<&Value> {
        self.metadata().get("source")
    }

    /// Returns the validated native scope roots in source order.
    pub fn scope_roots(&self) -> &[Vec<u8>] {
        self.scope_roots.roots()
    }

    /// Returns validated path-rule descriptors in source order.
    pub fn path_rules(&self) -> Option<&[PathRuleDescriptor]> {
        self.scope_roots.path_rules()
    }

    /// Returns the completeness value, or none when the source explicitly recorded it as unknown.
    pub fn completeness(&self) -> Option<&str> {
        self.source_bundle()?
            .get("completeness")
            .and_then(Value::as_str)
    }

    /// Returns the validated Volume record containing volume observations and root links.
    pub fn volume_record(&self) -> Option<&ValidatedVolumeRecord> {
        self.scope_roots.volume_record()
    }

    /// Returns the non-corrupt compatibility result for this open operation.
    pub fn compatibility(&self) -> OpenCompatibility {
        self.compatibility
    }

    /// Borrows the validated Volume/Item/Gap/End stream as read-only bytes.
    pub fn records(&mut self) -> impl Read + Seek + '_ {
        &mut self.records
    }

    pub(crate) fn discard_records(self) -> io::Result<()> {
        self.records.discard()
    }
}

/// Opens one already locked sealed copy and returns a move-only verified token.
///
/// Header, frame, integrity, metadata and decoded records are checked against this same spool.
/// `sink` is discarded on every failure or cancellation. A successful token exposes its staged
/// records only through `Read` and `Seek`. The sink must read and seek the bytes it just accepted,
/// and `discard` must remove all of its partial output.
pub fn open<S, W>(
    sealed: S,
    sink: W,
    mut cancelled: impl FnMut() -> bool,
) -> Result<VerifiedSnapshot<W>, OpenError>
where
    S: SealedSpool,
    W: zstd_stream::StagingSink + Read + Seek,
{
    let mut spool = match envelope::spool::verify_for_open(sealed, &mut cancelled) {
        Ok(spool) => spool,
        Err(error) => return Err(discard(sink, map_spool_error(error))),
    };

    let reader = format::reader_for(spool.format_version());
    if reader.is_none() {
        let kind = if spool.format_version() > envelope::DUSNAP_FORMAT_VERSION {
            OpenErrorKind::FormatNewer
        } else {
            OpenErrorKind::FormatUnsupported
        };
        return Err(discard(sink, OpenError::new(kind)));
    }
    if reader != Some(Reader::V1) {
        return Err(discard(
            sink,
            OpenError::new(OpenErrorKind::FormatUnsupported),
        ));
    }

    let (mut staged, scope_roots) =
        match zstd_stream::validate_records_in_sealed(&mut spool, sink, &mut cancelled) {
            Ok(validated) => validated,
            Err(error) => return Err(map_stream_error(error)),
        };
    if cancelled() {
        return Err(discard(staged, OpenError::new(OpenErrorKind::Cancelled)));
    }
    if let Err(error) = staged.seek(SeekFrom::Start(0)) {
        return Err(discard(
            staged,
            OpenError::caused_by(OpenErrorKind::Staging, error),
        ));
    }

    Ok(VerifiedSnapshot {
        scope_roots,
        records: staged,
        compatibility: OpenCompatibility::ObservationOnly,
    })
}

fn discard<W: zstd_stream::StagingSink>(sink: W, mut failure: OpenError) -> OpenError {
    if let Err(cleanup) = sink.discard() {
        failure.cleanup = Some(cleanup);
    }
    failure
}

fn map_spool_error(error: envelope::spool::Error) -> OpenError {
    match error {
        envelope::spool::Error::Io(error) => OpenError::caused_by(OpenErrorKind::Io, error),
        envelope::spool::Error::Read(error) => OpenError::caused_by(OpenErrorKind::Io, error),
        envelope::spool::Error::Cancelled => OpenError::new(OpenErrorKind::Cancelled),
        envelope::spool::Error::Format(error) => map_envelope_error(error),
        envelope::spool::Error::Frame(error) => map_frame_error(error),
        envelope::spool::Error::Metadata => OpenError::new(OpenErrorKind::FormatInvalid),
        envelope::spool::Error::IndexIntegrity => OpenError::new(OpenErrorKind::IntegrityFailed),
    }
}

fn map_stream_error(error: zstd_stream::Error) -> OpenError {
    match error {
        zstd_stream::Error::Envelope(error) => map_envelope_error(error),
        zstd_stream::Error::Frame(error) => map_frame_error(error),
        zstd_stream::Error::Cancelled => OpenError::new(OpenErrorKind::Cancelled),
        zstd_stream::Error::Decode(error) => {
            let kind = match error.kind() {
                io::ErrorKind::InvalidData | io::ErrorKind::UnexpectedEof => {
                    OpenErrorKind::FormatInvalid
                }
                _ => OpenErrorKind::Io,
            };
            OpenError::caused_by(kind, error)
        }
        zstd_stream::Error::Sink(error) => OpenError::caused_by(OpenErrorKind::Staging, error),
        zstd_stream::Error::DecodedLimit => OpenError::new(OpenErrorKind::DecompressLimit),
        zstd_stream::Error::Records | zstd_stream::Error::Metadata => {
            OpenError::new(OpenErrorKind::FormatInvalid)
        }
        zstd_stream::Error::Discard { failure, cleanup } => {
            let mut mapped = map_stream_error(*failure);
            mapped.cleanup = Some(cleanup);
            mapped
        }
    }
}

fn map_frame_error(error: zstd_frame::Error) -> OpenError {
    match error {
        zstd_frame::Error::Envelope(error) => map_envelope_error(error),
        zstd_frame::Error::Dictionary | zstd_frame::Error::WindowLimit => {
            OpenError::new(OpenErrorKind::FormatUnsupported)
        }
        zstd_frame::Error::DecodedLimit => OpenError::new(OpenErrorKind::DecompressLimit),
        zstd_frame::Error::Cancelled => OpenError::new(OpenErrorKind::Cancelled),
        zstd_frame::Error::InvalidFrame | zstd_frame::Error::TrailingData => {
            OpenError::new(OpenErrorKind::FormatInvalid)
        }
    }
}

fn map_envelope_error(error: envelope::Error) -> OpenError {
    match error {
        envelope::Error::FileLimit | envelope::Error::DecodedLimit => {
            OpenError::new(OpenErrorKind::DecompressLimit)
        }
        envelope::Error::IntegrityFailed => OpenError::new(OpenErrorKind::IntegrityFailed),
        envelope::Error::UnsupportedVersion => OpenError::new(OpenErrorKind::FormatUnsupported),
        envelope::Error::Truncated
        | envelope::Error::Magic
        | envelope::Error::Flags
        | envelope::Error::MetadataLength
        | envelope::Error::MetadataEncoding
        | envelope::Error::LengthMismatch => OpenError::new(OpenErrorKind::FormatInvalid),
    }
}
