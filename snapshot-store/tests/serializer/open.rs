use super::*;
use du_core_contracts::snapshot_spool::SealedSpool;
use du_snapshot_store::{
    OpenCompatibility, OpenError, OpenErrorKind, VerifiedSnapshot, open, zstd_stream,
};
use std::{
    io::{Cursor, Read, Seek, SeekFrom, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use support::Decoded;

struct Sealed(Cursor<Vec<u8>>);

impl Read for Sealed {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.0.read(bytes)
    }
}

impl Seek for Sealed {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.0.seek(position)
    }
}

impl SealedSpool for Sealed {}

#[derive(Default)]
struct StageState {
    bytes: Option<Vec<u8>>,
    discards: usize,
}

struct Stage {
    bytes: Cursor<Vec<u8>>,
    state: Arc<Mutex<StageState>>,
    cancel_after_write: Option<Arc<AtomicBool>>,
}

impl Read for Stage {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.bytes.read(bytes)
    }
}

impl Seek for Stage {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.bytes.seek(position)
    }
}

impl Write for Stage {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let written = self.bytes.write(bytes)?;
        if let Some(cancelled) = &self.cancel_after_write {
            cancelled.store(true, Ordering::Release);
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("stage state poisoned"))?;
        state.bytes = Some(self.bytes.get_ref().clone());
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.bytes.flush()
    }
}

impl zstd_stream::StagingSink for Stage {
    fn discard(mut self) -> io::Result<()> {
        self.bytes.get_mut().clear();
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("stage state poisoned"))?;
        state.bytes = None;
        state.discards += 1;
        Ok(())
    }
}

fn stage() -> Result<(Stage, Arc<Mutex<StageState>>), String> {
    let state = Arc::new(Mutex::new(StageState::default()));
    Ok((
        Stage {
            bytes: Cursor::new(Vec::new()),
            state: Arc::clone(&state),
            cancel_after_write: None,
        },
        state,
    ))
}

#[test]
fn cancellation_after_staging_discards_the_decoded_payload() -> Result<(), String> {
    let result = source_result(ResultCompleteness::Partial)?;
    let source = source_metadata(false, result.observed_roots().len());
    let bytes = write_snapshot(&result, &source)?;
    let (mut sink, state) = stage()?;
    let cancelled = Arc::new(AtomicBool::new(false));
    sink.cancel_after_write = Some(Arc::clone(&cancelled));
    let result = open(Sealed(Cursor::new(bytes)), sink, || {
        cancelled.load(Ordering::Acquire)
    });
    let error = match result {
        Err(error) => error,
        Ok(_) => return Err("cancelled open completed successfully".into()),
    };
    let state = state
        .lock()
        .map_err(|_| "stage state poisoned".to_owned())?;
    if error.kind() != OpenErrorKind::Cancelled
        || error.reason_key().is_some()
        || state.bytes.is_some()
        || state.discards != 1
    {
        return Err(format!("cancellation did not discard its stage: {error:?}"));
    }
    Ok(())
}

fn open_bytes(bytes: Vec<u8>) -> Result<(VerifiedSnapshot<Stage>, Arc<Mutex<StageState>>), String> {
    let (sink, state) = stage()?;
    let result = open(Sealed(Cursor::new(bytes)), sink, || false);
    match result {
        Ok(snapshot) => Ok((snapshot, state)),
        Err(error) => Err(format!("unexpected open error: {error}")),
    }
}

fn open_error(bytes: Vec<u8>) -> Result<OpenError, String> {
    let (sink, state) = stage()?;
    let result = open(Sealed(Cursor::new(bytes)), sink, || false);
    let error = match result {
        Err(error) => error,
        Ok(_) => return Err("malformed snapshot opened successfully".into()),
    };
    let state = state
        .lock()
        .map_err(|_| "stage state poisoned".to_owned())?;
    if state.bytes.is_some() || state.discards != 1 {
        return Err("failed open left staged output behind".into());
    }
    Ok(error)
}

fn assert_reason(bytes: Vec<u8>, reason: &str, kind: OpenErrorKind) -> Result<(), String> {
    let error = open_error(bytes)?;
    if error.reason_key() != Some(reason) || error.kind() != kind || error.cleanup_failed() {
        return Err(format!("unexpected open error: {error:?}"));
    }
    Ok(())
}

fn stream(bytes: &[u8]) -> Result<(String, u64, Vec<u8>), String> {
    let envelope = envelope::verify_v1(bytes).map_err(|error| format!("{error:?}"))?;
    let metadata = envelope.metadata.to_owned();
    let record_count = envelope.record_count;
    let mut decoded = Vec::new();
    zstd_stream::decode_v1(bytes, &mut decoded, || false).map_err(|error| format!("{error:?}"))?;
    Ok((metadata, record_count, decoded))
}

fn records(mut decoded: &[u8]) -> Result<Vec<(u8, Vec<u8>)>, String> {
    let mut output = Vec::new();
    while !decoded.is_empty() {
        let length = decoded
            .get(..4)
            .ok_or("truncated test record length")?
            .try_into()
            .map(u32::from_le_bytes)
            .map_err(|_| "truncated test record length")? as usize;
        decoded = &decoded[4..];
        let frame = decoded.get(..length).ok_or("truncated test record")?;
        let (&kind, body) = frame.split_first().ok_or("empty test record")?;
        output.push((kind, body.to_vec()));
        decoded = &decoded[length..];
    }
    Ok(output)
}

fn encode_records(records: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let mut output = Vec::new();
    for (kind, body) in records {
        output.extend_from_slice(
            &u32::try_from(body.len() + 1)
                .unwrap_or(u32::MAX)
                .to_le_bytes(),
        );
        output.push(*kind);
        output.extend_from_slice(body);
    }
    output
}

fn compress(decoded: &[u8]) -> Result<Vec<u8>, String> {
    let mut encoder =
        zstd::stream::write::Encoder::new(Vec::new(), 0).map_err(|error| error.to_string())?;
    encoder
        .write_all(decoded)
        .map_err(|error| error.to_string())?;
    encoder.finish().map_err(|error| error.to_string())
}

fn wrap_version(
    version: u16,
    metadata: &str,
    compressed: &[u8],
    record_count: u64,
    decoded_length: u64,
) -> Vec<u8> {
    let mut output = Vec::new();
    output.extend_from_slice(b"DUSNAP\r\n");
    output.extend_from_slice(&version.to_le_bytes());
    output.extend_from_slice(&0_u16.to_le_bytes());
    output.extend_from_slice(&(metadata.len() as u32).to_le_bytes());
    output.extend_from_slice(metadata.as_bytes());
    output.extend_from_slice(compressed);
    output.extend_from_slice(&(compressed.len() as u64).to_le_bytes());
    output.extend_from_slice(&record_count.to_le_bytes());
    output.extend_from_slice(&decoded_length.to_le_bytes());
    let digest = blake3::hash(&output);
    output.extend_from_slice(digest.as_bytes());
    output
}

fn wrap(metadata: &str, compressed: &[u8], record_count: u64, decoded_length: u64) -> Vec<u8> {
    wrap_version(
        envelope::DUSNAP_FORMAT_VERSION,
        metadata,
        compressed,
        record_count,
        decoded_length,
    )
}

fn wrap_records(metadata: &str, records: &[(u8, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let decoded = encode_records(records);
    let compressed = compress(&decoded)?;
    Ok(wrap(
        metadata,
        &compressed,
        records.len() as u64,
        decoded.len() as u64,
    ))
}

fn assert_matches_writer(bytes: Vec<u8>, expected: &Decoded) -> Result<(), String> {
    let (_, _, expected_records) = stream(&bytes)?;
    let (mut snapshot, state) = open_bytes(bytes)?;
    if snapshot.metadata() != &expected.metadata
        || snapshot.source_bundle() != Some(&expected.metadata["source"])
        || snapshot.compatibility() != OpenCompatibility::ObservationOnly
        || snapshot.volume_record().is_none()
    {
        return Err("verified metadata differs from the serialized source".into());
    }
    let mut actual_records = Vec::new();
    snapshot
        .records()
        .read_to_end(&mut actual_records)
        .map_err(|error| error.to_string())?;
    if actual_records != expected_records {
        return Err("verified record stream differs from the serialized source".into());
    }
    let state = state
        .lock()
        .map_err(|_| "stage state poisoned".to_owned())?;
    if state.discards != 0 || state.bytes.as_deref() != Some(expected_records.as_slice()) {
        return Err("successful open did not retain its staged records".into());
    }
    Ok(())
}

#[test]
fn opens_empty_result_from_write_v1() -> Result<(), String> {
    let result = empty_result(None)?;
    let source = source_metadata(true, result.observed_roots().len());
    let bytes = write_snapshot(&result, &source)?;
    assert_matches_writer(bytes.clone(), &decoded(&bytes)?)
}

#[test]
fn opens_multiroot_gap_and_unknown_volume_identity_from_write_v1() -> Result<(), String> {
    let result = source_result(ResultCompleteness::Partial)?;
    let source = source_metadata(false, result.observed_roots().len());
    let bytes = write_snapshot(&result, &source)?;
    let expected = decoded(&bytes)?;
    assert_matches_writer(bytes.clone(), &expected)?;

    let (snapshot, _) = open_bytes(bytes)?;
    if snapshot.scope_roots().len() != 3
        || snapshot.path_rules().map(<[_]>::len) != Some(3)
        || snapshot.completeness() != Some("partial")
    {
        return Err("multi-root metadata was not retained".into());
    }
    let volume = snapshot
        .volume_record()
        .ok_or("validated Volume record missing")?
        .decode()
        .map_err(|error| format!("{error:?}"))?;
    if volume.volumes.len() != 2
        || volume.roots.len() != 3
        || volume.roots[2].volume_index.is_some()
        || expected
            .records
            .iter()
            .filter(|record| record.0 == 3)
            .count()
            != 1
    {
        return Err("volume links, unknown identity, or gap record changed".into());
    }
    Ok(())
}

#[test]
fn reports_registered_codec_failures_after_outer_verification() -> Result<(), String> {
    let result = empty_result(None)?;
    let source = source_metadata(true, result.observed_roots().len());
    let valid = write_snapshot(&result, &source)?;
    let (metadata, count, decoded) = stream(&valid)?;
    let compressed = envelope::verify_v1(&valid)
        .map_err(|error| format!("{error:?}"))?
        .compressed
        .to_vec();
    let metadata: Value = serde_json::from_str(&metadata).map_err(|error| error.to_string())?;
    for (version, reason, kind) in [
        (
            0_u16,
            "snapshot.format_unsupported",
            OpenErrorKind::FormatUnsupported,
        ),
        (2_u16, "snapshot.format_newer", OpenErrorKind::FormatNewer),
    ] {
        let mut versioned = metadata.clone();
        versioned["formatVersion"] = serde_json::json!(version);
        let versioned = serde_json::to_string(&versioned).map_err(|error| error.to_string())?;
        assert_reason(
            wrap_version(
                version,
                &versioned,
                &compressed,
                count,
                decoded.len() as u64,
            ),
            reason,
            kind,
        )?;
    }
    Ok(())
}

#[test]
fn rejects_each_invalid_open_step_and_discards_staging() -> Result<(), String> {
    let result = source_result(ResultCompleteness::Partial)?;
    let source = source_metadata(false, result.observed_roots().len());
    let valid = write_snapshot(&result, &source)?;
    let (metadata, count, decoded) = stream(&valid)?;
    let base_records = records(&decoded)?;

    let mut forged_length = valid.clone();
    let footer = forged_length.len() - 56;
    let length = u64::from_le_bytes(
        forged_length[footer..footer + 8]
            .try_into()
            .map_err(|_| "missing footer length")?,
    );
    forged_length[footer..footer + 8].copy_from_slice(&(length + 1).to_le_bytes());
    let digest = blake3::hash(&forged_length[..forged_length.len() - 32]);
    let digest_start = forged_length.len() - 32;
    forged_length[digest_start..].copy_from_slice(digest.as_bytes());
    assert_reason(
        forged_length,
        "snapshot.format_invalid",
        OpenErrorKind::FormatInvalid,
    )?;

    let mut bad_hash = valid.clone();
    let last = bad_hash.len() - 1;
    bad_hash[last] ^= 1;
    assert_reason(
        bad_hash,
        "snapshot.integrity_failed",
        OpenErrorKind::IntegrityFailed,
    )?;

    let base_envelope = envelope::verify_v1(&valid).map_err(|error| format!("{error:?}"))?;
    let mut bad_compression = base_envelope.compressed.to_vec();
    let descriptor = *bad_compression.get(4).ok_or("missing zstd descriptor")?;
    let dictionary_position = if descriptor & 0x20 != 0 { 5 } else { 6 };
    bad_compression[4] |= 1;
    bad_compression.insert(dictionary_position, 1);
    let mut compression_boundary = wrap(&metadata, &bad_compression, count, decoded.len() as u64);
    assert_reason(
        compression_boundary.clone(),
        "snapshot.format_unsupported",
        OpenErrorKind::FormatUnsupported,
    )?;
    let footer = compression_boundary.len() - 56;
    let forged = u64::from_le_bytes(
        compression_boundary[footer..footer + 8]
            .try_into()
            .map_err(|_| "missing footer length")?,
    );
    compression_boundary[footer..footer + 8].copy_from_slice(&(forged + 1).to_le_bytes());
    let digest = blake3::hash(&compression_boundary[..compression_boundary.len() - 32]);
    let digest_start = compression_boundary.len() - 32;
    compression_boundary[digest_start..].copy_from_slice(digest.as_bytes());
    assert_reason(
        compression_boundary,
        "snapshot.format_unsupported",
        OpenErrorKind::FormatUnsupported,
    )?;

    let missing_volume: Vec<_> = base_records.iter().skip(1).cloned().collect();
    assert_reason(
        wrap_records(&metadata, &missing_volume)?,
        "snapshot.format_invalid",
        OpenErrorKind::FormatInvalid,
    )?;

    let mut duplicate_volume = base_records.clone();
    duplicate_volume.insert(1, base_records[0].clone());
    assert_reason(
        wrap_records(&metadata, &duplicate_volume)?,
        "snapshot.format_invalid",
        OpenErrorKind::FormatInvalid,
    )?;

    let mut truncated_body = base_records.clone();
    truncated_body
        .get_mut(1)
        .ok_or("Item record missing from test fixture")?
        .1
        .pop();
    assert_reason(
        wrap_records(&metadata, &truncated_body)?,
        "snapshot.format_invalid",
        OpenErrorKind::FormatInvalid,
    )?;

    let mut wrong_root = base_records.clone();
    wrong_root[0].1[7..11].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_reason(
        wrap_records(&metadata, &wrong_root)?,
        "snapshot.format_invalid",
        OpenErrorKind::FormatInvalid,
    )?;

    let compressed = compress(&decoded)?;
    assert_reason(
        wrap(&metadata, &compressed, count, decoded.len() as u64 - 1),
        "snapshot.decompress_limit",
        OpenErrorKind::DecompressLimit,
    )?;

    assert_reason(
        wrap(&metadata, &compressed, count, 4_294_967_297_u64),
        "snapshot.decompress_limit",
        OpenErrorKind::DecompressLimit,
    )
}
