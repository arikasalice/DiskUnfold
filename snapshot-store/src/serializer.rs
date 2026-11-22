//! Seals one pinned analysis result as a DUSNAP v1 stream.

mod result_records;

use crate::{
    envelope, record_framing,
    records::{NameEncoding, gap::Gap},
    source_admission,
};
use du_analysis_index::ResultVersion;
use du_core_contracts::{
    Id, Timestamp,
    result::{ResultCompleteness, ResultScopeRoot, ResultVersionRead},
};
use serde_json::{Value, json};
use std::io::{self, Write};

use result_records::{item_record, path_encoding, root_items, validate_source, volume_records};

/// Snapshot identity and application version supplied by the save owner.
pub struct SnapshotIdentity<'a> {
    /// UUIDv7 allocated by the save owner.
    pub snapshot_id: &'a Id,
    /// Capture time allocated by the save owner.
    pub created_at: &'a Timestamp,
    /// Non-empty application version text.
    pub app_version: &'a str,
}

/// A path separator preserved in the source metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathSeparator {
    /// Slash-separated native path.
    Slash,
    /// Backslash-separated native path.
    Backslash,
}

/// Known case-sensitivity evidence for one scope root.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseSensitivity {
    /// Names are case-sensitive.
    Sensitive,
    /// Names are case-insensitive.
    Insensitive,
}

/// Known Unicode normalization evidence for one scope root.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Normalization {
    /// No normalization is applied.
    None,
    /// Names are compared after NFC normalization.
    Nfc,
    /// Names are compared after NFD normalization.
    Nfd,
}

/// Path comparison evidence for the scope root at the same index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathRule {
    /// Native separator observed for this root.
    pub separator: PathSeparator,
    /// Unknown when `None`.
    pub case_sensitivity: Option<CaseSensitivity>,
    /// Unknown when `None`.
    pub normalization: Option<Normalization>,
}

/// Whether the source supplied a complete condition summary.
#[derive(Clone, Debug, PartialEq)]
pub enum ConditionSummary {
    /// Caller-supplied, non-sensitive condition entries in their original order.
    ///
    /// Entries must not contain secrets, file contents, approvals, or undo data.
    Known(Vec<Value>),
    /// The source did not establish a condition summary.
    Unknown,
}

/// One caller-supplied algorithm identity used by the source result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlgorithmVersion {
    /// Algorithm category.
    pub kind: String,
    /// Stable algorithm identifier.
    pub id: String,
    /// Positive one-based algorithm version.
    pub version: u32,
}

/// Caller-supplied source metadata that cannot be recovered from the pinned result.
pub struct SourceMetadata {
    /// Known entries or an explicit unknown state.
    pub condition_summary: ConditionSummary,
    /// One rule for each observed scope root, in producer order.
    pub path_rules: Vec<PathRule>,
    /// Algorithm identities in caller order.
    pub algorithms: Vec<AlgorithmVersion>,
}

/// Facts copied from the sealed v1 bytes and their completed footer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotWriteReport {
    /// Total registered file size, including the footer digest.
    pub body_size: u64,
    /// BLAKE3 digest stored as the v1 footer.
    pub body_hash: [u8; 32],
    /// Exact JSON text serialized for `source.conditionSummary`.
    pub condition_summary_json: String,
    /// Canonical 32-byte scope marker, or empty when absent.
    pub scope_marker: Vec<u8>,
    /// Completeness value serialized in the metadata.
    pub completeness: du_core_contracts::snapshot_index::SnapshotIndexCompleteness,
    /// DUSNAP format version serialized in the header and metadata.
    pub format_version: u16,
    /// Volume identifiers serialized in the `Volume` record order.
    pub volume_ids: Vec<Vec<u8>>,
}

/// A serializer failure. The caller must discard any output already written on error.
#[derive(Debug)]
pub enum Error {
    /// The pinned result was cancelled and is not eligible for snapshot save.
    Cancelled,
    /// Snapshot persistence was requested for a sample result.
    SampleSource,
    /// Supplied metadata conflicts with the pinned result or the v1 metadata contract.
    InvalidMetadata,
    /// The pinned result cannot be represented by the existing v1 record codecs.
    InvalidResult,
    /// A record exceeds the existing v1 bounds or violates its codec contract.
    InvalidRecord,
    /// An envelope or decoded-stream size limit was exceeded.
    Limit,
    /// Compression or output I/O failed; output may be partial.
    Io(io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Cancelled => "cancelled results cannot be saved",
            Self::SampleSource => source_admission::SnapshotSourceRejected::REASON,
            Self::InvalidMetadata => "snapshot source metadata is invalid",
            Self::InvalidResult => "pinned result is invalid for snapshot serialization",
            Self::InvalidRecord => "pinned result contains an unrepresentable snapshot record",
            Self::Limit => "snapshot size limit exceeded",
            Self::Io(_) => "snapshot serialization I/O failed",
        })
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

/// Writes a complete, sealed v1 snapshot for one immutable result version.
///
/// Result facts are read from `ResultVersion`; the caller supplies condition, path,
/// algorithm, and identity metadata. The caller must discard partial output on error.
pub fn write_v1<W: Write>(
    version: &ResultVersion,
    identity: SnapshotIdentity<'_>,
    source: &SourceMetadata,
    output: &mut W,
) -> Result<(), Error> {
    write_v1_cancellable(version, identity, source, output, || false).map(|_| ())
}

/// Writes a sealed v1 snapshot and checks cancellation between complete records.
///
/// The caller must discard partial output on any error. A successful report is
/// derived from the same metadata and body records that were written.
pub fn write_v1_cancellable<W: Write>(
    version: &ResultVersion,
    identity: SnapshotIdentity<'_>,
    source: &SourceMetadata,
    output: &mut W,
    mut cancelled: impl FnMut() -> bool,
) -> Result<SnapshotWriteReport, Error> {
    if version.completeness() == ResultCompleteness::Cancelled {
        return Err(Error::Cancelled);
    }
    if source_admission::admit_snapshot_source(version.data_source()).is_err() {
        return Err(Error::SampleSource);
    }

    let roots = version.observed_roots();
    let items = version.items();
    let encoding = path_encoding(roots)?;
    validate_source(&identity, source, roots, encoding)?;
    let root_items = root_items(items, roots.len())?;
    let root_links_and_volumes = volume_records(version, roots, &root_items)?;
    let volume_ids = root_links_and_volumes
        .volumes
        .iter()
        .map(|volume| volume.id.to_vec())
        .collect();
    let volume_body = root_links_and_volumes
        .encode()
        .map_err(|_| Error::InvalidRecord)?;
    let metadata = metadata_bytes(version, &identity, source, roots, encoding)?;
    if metadata.is_empty() || metadata.len() as u64 > envelope::MAX_METADATA_BYTES {
        return Err(Error::Limit);
    }

    let record_count = u64::try_from(items.len())
        .ok()
        .and_then(|count| count.checked_add(version.scope_gaps().len() as u64))
        .and_then(|count| count.checked_add(2))
        .ok_or(Error::Limit)?;
    let mut digest_writer = DigestWriter::new(output);
    let mut header = Vec::with_capacity(envelope::HEADER_BYTES as usize);
    header.extend_from_slice(envelope::MAGIC);
    header.extend(envelope::DUSNAP_FORMAT_VERSION.to_le_bytes());
    header.extend(0_u16.to_le_bytes());
    header.extend(
        u32::try_from(metadata.len())
            .map_err(|_| Error::Limit)?
            .to_le_bytes(),
    );
    digest_writer.write_all(&header).map_err(Error::Io)?;
    digest_writer.write_all(&metadata).map_err(Error::Io)?;
    let compressed_start = digest_writer.written;

    let mut encoder = zstd::stream::write::Encoder::new(digest_writer, 0).map_err(Error::Io)?;
    encoder.include_checksum(true).map_err(Error::Io)?;
    let mut framing = record_framing::Validator::new(record_count);
    let mut decoded_length = 0_u64;
    write_record(
        &mut encoder,
        &mut framing,
        &mut decoded_length,
        1,
        &volume_body,
        &mut cancelled,
    )?;
    for item_source in items {
        let record = item_record(version, item_source, roots)?;
        let body = record.encode(encoding).map_err(|_| Error::InvalidRecord)?;
        write_record(
            &mut encoder,
            &mut framing,
            &mut decoded_length,
            2,
            &body,
            &mut cancelled,
        )?;
    }
    for gap_source in version.scope_gaps() {
        let root_index =
            usize::try_from(gap_source.root_index).map_err(|_| Error::InvalidResult)?;
        if roots.get(root_index).is_none() {
            return Err(Error::InvalidResult);
        }
        let path = gap_source.path.iter().map(Vec::as_slice).collect();
        let body = Gap {
            root_index: gap_source.root_index,
            path,
            reason_key: &gap_source.reason_key,
            observed_at_ms: gap_source.observed_at.as_ref().map(Timestamp::unix_millis),
        }
        .encode(encoding, roots.len())
        .map_err(|_| Error::InvalidRecord)?;
        write_record(
            &mut encoder,
            &mut framing,
            &mut decoded_length,
            3,
            &body,
            &mut cancelled,
        )?;
    }
    write_record(
        &mut encoder,
        &mut framing,
        &mut decoded_length,
        0x7f,
        &[],
        &mut cancelled,
    )?;
    framing.finish().map_err(|()| Error::InvalidRecord)?;

    let mut digest_writer = encoder.finish().map_err(Error::Io)?;
    let compressed_length = digest_writer
        .written
        .checked_sub(compressed_start)
        .ok_or(Error::InvalidResult)?;
    let mut footer = Vec::with_capacity(24);
    footer.extend(compressed_length.to_le_bytes());
    footer.extend(record_count.to_le_bytes());
    footer.extend(decoded_length.to_le_bytes());
    digest_writer.write_all(&footer).map_err(Error::Io)?;
    let DigestWriter {
        inner,
        hasher,
        written,
    } = digest_writer;
    let body_hash = *hasher.finalize().as_bytes();
    inner.write_all(&body_hash).map_err(Error::Io)?;
    let condition_summary_json = match &source.condition_summary {
        ConditionSummary::Known(entries) => {
            serde_json::to_string(&json!({"state": "known", "entries": entries}))
        }
        ConditionSummary::Unknown => serde_json::to_string(&json!({"state": "unknown"})),
    }
    .map_err(|_| Error::InvalidMetadata)?;
    let scope_marker = version
        .scope_fingerprint()
        .filter(|fingerprint| {
            fingerprint.len() == 64
                && fingerprint
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .and_then(|fingerprint| {
            let (pairs, _) = fingerprint.as_bytes().as_chunks::<2>();
            pairs
                .iter()
                .map(|pair| {
                    let digits = std::str::from_utf8(pair).map_err(|_| Error::InvalidMetadata)?;
                    u8::from_str_radix(digits, 16).map_err(|_| Error::InvalidMetadata)
                })
                .collect::<Result<Vec<_>, _>>()
                .ok()
        })
        .unwrap_or_default();
    Ok(SnapshotWriteReport {
        body_size: written.checked_add(32).ok_or(Error::Limit)?,
        body_hash,
        condition_summary_json,
        scope_marker,
        completeness: match version.completeness() {
            ResultCompleteness::Complete => {
                du_core_contracts::snapshot_index::SnapshotIndexCompleteness::Complete
            }
            ResultCompleteness::Partial => {
                du_core_contracts::snapshot_index::SnapshotIndexCompleteness::Partial
            }
            ResultCompleteness::Missing => {
                du_core_contracts::snapshot_index::SnapshotIndexCompleteness::Missing
            }
            ResultCompleteness::Cancelled => return Err(Error::Cancelled),
        },
        format_version: envelope::DUSNAP_FORMAT_VERSION,
        volume_ids,
    })
}

fn metadata_bytes(
    version: &ResultVersion,
    identity: &SnapshotIdentity<'_>,
    source: &SourceMetadata,
    roots: &[ResultScopeRoot],
    encoding: NameEncoding,
) -> Result<Vec<u8>, Error> {
    let condition_summary = match &source.condition_summary {
        ConditionSummary::Known(entries) => json!({"state": "known", "entries": entries}),
        ConditionSummary::Unknown => json!({"state": "unknown"}),
    };
    let path_rules: Vec<Value> = source
        .path_rules
        .iter()
        .map(|rule| {
            json!({
                "separator": match rule.separator {
                    PathSeparator::Slash => "/",
                    PathSeparator::Backslash => "\\",
                },
                "caseSensitivity": rule.case_sensitivity.map(|value| match value {
                    CaseSensitivity::Sensitive => "sensitive",
                    CaseSensitivity::Insensitive => "insensitive",
                }),
                "normalization": rule.normalization.map(|value| match value {
                    Normalization::None => "none",
                    Normalization::Nfc => "NFC",
                    Normalization::Nfd => "NFD",
                }),
            })
        })
        .collect();
    let algorithms: Vec<Value> = source
        .algorithms
        .iter()
        .map(|algorithm| {
            json!({
                "kind": algorithm.kind,
                "id": algorithm.id,
                "version": algorithm.version,
            })
        })
        .collect();
    let scope_roots: Vec<&str> = roots.iter().map(|root| root.path.raw.as_str()).collect();
    let value = json!({
        "formatId": "dusnap",
        "formatVersion": envelope::DUSNAP_FORMAT_VERSION,
        "snapshotId": identity.snapshot_id.as_str(),
        "createdAt": identity.created_at.to_string(),
        "appVersion": identity.app_version,
        "pathEncoding": match encoding {
            NameEncoding::Bytes => "bytes",
            NameEncoding::WindowsUtf16Le => "utf16le",
        },
        "source": {
            "resultVersionId": version.version_id().as_str(),
            "scopeVersionId": version.scope_version_id().as_str(),
            "lineageId": version.lineage_id().map(Id::as_str),
            "observedAt": version.observed_at().map(ToString::to_string),
            "scopeMarker": version.scope_fingerprint().filter(|fingerprint| {
                fingerprint.len() == 64
                    && fingerprint.bytes().all(|byte| {
                        byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
                    })
            }),
            "conditionSummary": condition_summary,
            "completeness": match version.completeness() {
                ResultCompleteness::Complete => "complete",
                ResultCompleteness::Partial => "partial",
                ResultCompleteness::Missing => "missing",
                ResultCompleteness::Cancelled => return Err(Error::Cancelled),
            },
            "algorithms": {"state": "known", "entries": algorithms},
            "pathRules": path_rules,
            "scopeRoots": scope_roots,
        },
        "volume": {"recordKind": 1},
    });
    serde_json::to_vec(&value).map_err(|_| Error::InvalidMetadata)
}

fn write_record<W: Write>(
    output: &mut W,
    framing: &mut record_framing::Validator,
    decoded_length: &mut u64,
    kind: u8,
    body: &[u8],
    cancelled: &mut impl FnMut() -> bool,
) -> Result<(), Error> {
    if cancelled() {
        return Err(Error::Cancelled);
    }
    let length =
        u32::try_from(body.len().checked_add(1).ok_or(Error::Limit)?).map_err(|_| Error::Limit)?;
    let mut frame = Vec::with_capacity(body.len().saturating_add(5));
    frame.extend(length.to_le_bytes());
    frame.push(kind);
    frame.extend_from_slice(body);
    framing.push(&frame).map_err(|()| Error::InvalidRecord)?;
    let next = decoded_length
        .checked_add(u64::try_from(frame.len()).map_err(|_| Error::Limit)?)
        .filter(|length| *length <= envelope::MAX_DECODED_BYTES)
        .ok_or(Error::Limit)?;
    output.write_all(&frame).map_err(Error::Io)?;
    *decoded_length = next;
    Ok(())
}

struct DigestWriter<W> {
    inner: W,
    hasher: blake3::Hasher,
    written: u64,
}

impl<W> DigestWriter<W> {
    fn new(inner: W) -> Self {
        Self {
            inner,
            hasher: blake3::Hasher::new(),
            written: 0,
        }
    }
}

impl<W: Write> Write for DigestWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        let hashed_limit = envelope::MAX_FILE_BYTES - 32;
        let remaining = hashed_limit.saturating_sub(self.written);
        if remaining == 0 {
            return Err(io::Error::other("snapshot file limit exceeded"));
        }
        let length = bytes.len().min(remaining.min(bytes.len() as u64) as usize);
        let written = self.inner.write(&bytes[..length])?;
        self.hasher.update(&bytes[..written]);
        self.written = self.written.saturating_add(written as u64);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
