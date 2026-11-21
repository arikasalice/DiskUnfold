use du_analysis_index::{
    FileIdentity, IngestSink, Observation, ResultVersion, SharingEvidence, SpaceObservation,
    VolumeReport, VolumeReportPoint,
};
use du_core_contracts::{
    ByteSize, Id, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    Timestamp,
    ports::{PortError, PortErrorKind},
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
    scan_observation::VolumeSpace,
    snapshot_spool::{CaptureSpool, SealedSpool},
};
use du_snapshot_store::{
    envelope,
    records::{self, NameEncoding},
    serializer::{
        AlgorithmVersion, CaseSensitivity, ConditionSummary, Normalization, PathRule,
        PathSeparator, SourceMetadata,
    },
    zstd_stream,
};
use serde_json::{Value, json};
use std::io::{self, Cursor, Read, Seek, Write};
use std::sync::Arc;

pub(super) struct Decoded {
    pub metadata: Value,
    pub records: Vec<(u8, Vec<u8>)>,
}

fn id(value: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{value:012x}"))
}

fn timestamp(value: i64) -> Result<Timestamp, String> {
    Timestamp::from_unix_millis(value).ok_or_else(|| "invalid test timestamp".into())
}

pub(super) fn native_path(value: &str) -> Vec<u8> {
    #[cfg(windows)]
    {
        value.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }
    #[cfg(not(windows))]
    {
        value.as_bytes().to_vec()
    }
}

fn component(value: &str) -> Vec<u8> {
    native_path(value)
}

fn root(path: &str, volume_id: &[u8], file_id: &[u8]) -> ResultScopeRoot {
    ResultScopeRoot {
        path: ResultNativePath {
            encoding: if cfg!(windows) {
                PathEncoding::WindowsUtf16le
            } else {
                PathEncoding::Unix
            },
            raw: PathBytes::from_bytes(&native_path(path)),
        },
        volume_id: volume_id.to_vec(),
        file_id: file_id.to_vec(),
    }
}

fn size(bytes: u64, basis: SizeBasis) -> SizeValue {
    SizeValue {
        basis,
        evidence: SizeEvidence::OsReported,
        amount: SizeAmount::Exact {
            bytes: ByteSize::from(bytes),
        },
    }
}

pub(super) fn source_result(
    completeness: ResultCompleteness,
) -> Result<Arc<ResultVersion>, String> {
    let roots = if cfg!(windows) {
        vec![
            root("C:\\alpha", &[1], &[11]),
            root("D:\\beta", &[2], &[22]),
            root("E:\\unknown", &[], &[33]),
        ]
    } else {
        vec![
            root("/alpha", &[1], &[11]),
            root("/beta", &[2], &[22]),
            root("/unknown", &[], &[33]),
        ]
    };
    let sink = IngestSink::default();
    let mut build = sink
        .open_forest(id(1)?, roots)
        .map_err(|error| format!("{error:?}"))?;
    for root_index in 0..3 {
        build
            .add_directory_at(root_index, Vec::new())
            .map_err(|error| format!("{error:?}"))?;
    }
    #[cfg(unix)]
    let raw_name = vec![0xff, 0xfe];
    #[cfg(not(unix))]
    let raw_name = "𝄞".encode_utf16().flat_map(u16::to_le_bytes).collect();
    build
        .add_with_space_at(
            0,
            Observation {
                path: vec![raw_name],
                size: Some(size(7, SizeBasis::Logical)),
            },
            SpaceObservation {
                allocated: Some(size(8, SizeBasis::Allocated)),
                identity: Some(FileIdentity {
                    volume: vec![1],
                    file: vec![0xaa, 0xbb],
                }),
                link_count: Some(1),
                sharing: SharingEvidence::Unknown,
            },
        )
        .map_err(|error| format!("{error:?}"))?;
    build
        .add_directory_at(1, vec![component("nested")])
        .map_err(|error| format!("{error:?}"))?;
    build
        .record_gap_at(
            1,
            vec![component("unread")],
            "scan.access_denied".into(),
            Some(timestamp(1_700_000_000_010)?),
        )
        .map_err(|error| format!("{error:?}"))?;
    build
        .set_volume_reports(vec![
            VolumeReport {
                volume_id: vec![1],
                start: Some(VolumeReportPoint {
                    observed_at: Some(timestamp(1_700_000_000_020)?),
                    value: Ok(VolumeSpace {
                        total_bytes: 100,
                        used_bytes: 40,
                        free_bytes: 60,
                    }),
                }),
                end: None,
            },
            VolumeReport {
                volume_id: vec![2],
                start: None,
                end: Some(VolumeReportPoint {
                    observed_at: None,
                    value: Err(PortError {
                        kind: PortErrorKind::PermissionDenied,
                        reason_key: "space.volume_denied".into(),
                        os_code: Some(5),
                    }),
                }),
            },
        ])
        .map_err(|error| format!("{error:?}"))?;
    build
        .seal_at(id(2)?, completeness, Some(timestamp(1_700_000_000_000)?))
        .map_err(|error| format!("{error:?}"))
}

pub(super) fn empty_result() -> Result<Arc<ResultVersion>, String> {
    let path = if cfg!(windows) {
        root("C:\\empty", &[9], &[10])
    } else {
        root("/empty", &[9], &[10])
    };
    let sink = IngestSink::default();
    let mut build = sink
        .open_forest(id(3)?, vec![path])
        .map_err(|error| format!("{error:?}"))?;
    build
        .add_directory_at(0, Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    build
        .seal(id(4)?, ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))
}

pub(super) fn source_metadata(known: bool, root_count: usize) -> SourceMetadata {
    let separator = if cfg!(windows) {
        PathSeparator::Backslash
    } else {
        PathSeparator::Slash
    };
    SourceMetadata {
        condition_summary: if known {
            ConditionSummary::Known(vec![json!({"filter": "included"})])
        } else {
            ConditionSummary::Unknown
        },
        path_rules: (0..root_count)
            .map(|index| PathRule {
                separator,
                case_sensitivity: if known {
                    Some(if index == 1 {
                        CaseSensitivity::Insensitive
                    } else {
                        CaseSensitivity::Sensitive
                    })
                } else {
                    None
                },
                normalization: if known {
                    Some(if index == 0 {
                        Normalization::Nfc
                    } else {
                        Normalization::None
                    })
                } else {
                    None
                },
            })
            .collect(),
        algorithms: vec![AlgorithmVersion {
            kind: "aggregation".into(),
            id: "size.subtree".into(),
            version: 1,
        }],
    }
}

pub(super) fn identity() -> Result<(Id, Timestamp), String> {
    Ok((id(8)?, timestamp(1_700_000_001_000)?))
}

pub(super) fn decoded(bytes: &[u8]) -> Result<Decoded, String> {
    zstd_stream::validate_record_framing_v1(bytes, || false)
        .map_err(|error| format!("{error:?}"))?;
    let envelope = envelope::verify_v1(bytes).map_err(|error| format!("{error:?}"))?;
    let metadata: Value =
        serde_json::from_str(envelope.metadata).map_err(|error| error.to_string())?;
    let mut records = Vec::new();
    let decoded_length = zstd_stream::decode_v1(bytes, &mut records, || false)
        .map_err(|error| format!("{error:?}"))?;
    if decoded_length != envelope.decoded_length {
        return Err("decoded length differs from envelope".into());
    }
    let mut stream = records.as_slice();
    let mut framed = Vec::new();
    while !stream.is_empty() {
        let length_bytes: [u8; 4] = stream
            .get(..4)
            .ok_or("truncated test frame")?
            .try_into()
            .map_err(|_| "truncated test frame")?;
        let length = u32::from_le_bytes(length_bytes) as usize;
        stream = &stream[4..];
        let record = stream.get(..length).ok_or("truncated test body")?;
        let (&kind, body) = record.split_first().ok_or("empty test record")?;
        framed.push((kind, body.to_vec()));
        stream = &stream[length..];
    }
    Ok(Decoded {
        metadata,
        records: framed,
    })
}

#[derive(Default)]
struct MemorySpool(Cursor<Vec<u8>>);

impl Read for MemorySpool {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.0.read(bytes)
    }
}

impl Seek for MemorySpool {
    fn seek(&mut self, position: io::SeekFrom) -> io::Result<u64> {
        self.0.seek(position)
    }
}

impl Write for MemorySpool {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl CaptureSpool for MemorySpool {
    type Sealed = Self;

    fn seal(self) -> io::Result<Self::Sealed> {
        Ok(self)
    }
}

impl SealedSpool for MemorySpool {}

#[derive(Default)]
struct Stage(Vec<u8>);

impl Write for Stage {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl zstd_stream::StagingSink for Stage {
    fn discard(self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn validate_sealed(bytes: &[u8]) -> Result<zstd_stream::ScopeRoots, String> {
    let mut source = Cursor::new(bytes);
    let mut spool = envelope::spool::capture_v1(
        &mut source,
        MemorySpool::default(),
        None,
        |_, _| true,
        || false,
    )
    .map_err(|error| format!("{error:?}"))?;
    let (stage, roots) =
        zstd_stream::validate_records_in_sealed(&mut spool, Stage::default(), || false)
            .map_err(|error| format!("{error:?}"))?;
    if stage.0.is_empty() {
        return Err("validated record stream was empty".into());
    }
    Ok(roots)
}

pub(super) fn encoding() -> NameEncoding {
    if cfg!(windows) {
        NameEncoding::WindowsUtf16Le
    } else {
        NameEncoding::Bytes
    }
}

pub(super) fn root_placeholder() -> &'static [u8] {
    if cfg!(windows) { &[b'@', 0] } else { b"@" }
}

pub(super) fn snapshot_size(amount: &SizeAmount) -> records::item::Size {
    match amount {
        SizeAmount::Exact { bytes } => records::item::Size::Exact(bytes.get()),
        SizeAmount::LowerBound { bytes } => records::item::Size::LowerBound(bytes.get()),
        SizeAmount::Unknown => records::item::Size::Unknown,
    }
}
