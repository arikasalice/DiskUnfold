use super::*;

fn frame(data: &[u8], checksum: bool) -> io::Result<Vec<u8>> {
    let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), 1)?;
    encoder.include_checksum(checksum)?;
    encoder.write_all(data)?;
    encoder.finish()
}

fn envelope(frame: &[u8], limit: u64) -> Vec<u8> {
    envelope_with_count(frame, limit, 0)
}

fn envelope_with_count(frame: &[u8], limit: u64, records: u64) -> Vec<u8> {
    let mut data = b"DUSNAP\r\n".to_vec();
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&2u32.to_le_bytes());
    data.extend_from_slice(b"{}");
    data.extend_from_slice(frame);
    data.extend_from_slice(&(frame.len() as u64).to_le_bytes());
    data.extend_from_slice(&records.to_le_bytes());
    data.extend_from_slice(&limit.to_le_bytes());
    data.extend_from_slice(blake3::hash(&data).as_bytes());
    data
}

#[test]
fn verifies_decoded_record_boundaries_count_and_cancellation() -> io::Result<()> {
    let mut records = vec![1, 0, 0, 0, 1];
    records.extend_from_slice(&[1, 0, 0, 0, 0x7f]);
    let compressed = frame(&records, true)?;
    let input = envelope_with_count(&compressed, records.len() as u64, 2);
    assert!(validate_record_framing_v1(&input, || false).is_ok());
    assert!(matches!(
        validate_record_framing_v1(
            &envelope_with_count(&compressed, records.len() as u64, 1),
            || false
        ),
        Err(Error::Records)
    ));
    assert!(matches!(
        validate_record_framing_v1(&input, || true),
        Err(Error::Cancelled)
    ));
    let mut polls = 0;
    assert!(matches!(
        validate_record_framing_v1(&input, || {
            polls += 1;
            polls > 4
        }),
        Err(Error::Cancelled)
    ));
    records.push(0);
    let extra = frame(&records, true)?;
    assert!(matches!(
        validate_record_framing_v1(
            &envelope_with_count(&extra, records.len() as u64, 2),
            || false
        ),
        Err(Error::Records)
    ));
    let mut damaged = compressed;
    let last = damaged.len() - 1;
    damaged[last] ^= 1;
    assert!(matches!(
        validate_record_framing_v1(&envelope_with_count(&damaged, 10, 2), || false),
        Err(Error::Decode(_))
    ));
    Ok(())
}

#[test]
fn streams_real_compressed_data_and_drains_checksum() -> io::Result<()> {
    assert_eq!(zstd::zstd_safe::version_number(), 10_507);
    let original = b"snapshot record bytes".repeat(5000);
    let frame = frame(&original, true)?;
    let input = envelope(&frame, original.len() as u64);
    let mut output = Vec::new();
    assert_eq!(
        decode_v1(&input, &mut output, || false).ok(),
        Some(original.len() as u64)
    );
    assert_eq!(output, original);
    Ok(())
}

#[test]
fn rejects_checksum_corruption_and_exactly_one_byte_over_limit() -> io::Result<()> {
    let mut frame = frame(b"abcdefgh", true)?;
    let last = frame.len() - 1;
    frame[last] ^= 1;
    assert!(matches!(
        decode_v1(&envelope(&frame, 8), &mut Vec::new(), || false),
        Err(Error::Decode(_))
    ));
    let frame = self::frame(b"abcdefgh", false)?;
    let mut output = Vec::new();
    assert!(matches!(
        decode_v1(&envelope(&frame, 7), &mut output, || false),
        Err(Error::DecodedLimit)
    ));
    assert!(output.len() <= 7);
    Ok(())
}

#[test]
fn cancels_and_propagates_partial_sink_failure() -> io::Result<()> {
    let input = envelope(&frame(&vec![42; 100_000], true)?, 100_000);
    let mut calls = 0;
    let mut output = Vec::new();
    assert!(matches!(
        decode_v1(&input, &mut output, || {
            calls += 1;
            calls > 5
        }),
        Err(Error::Cancelled)
    ));
    struct ShortSink {
        remaining: usize,
    }
    impl Write for ShortSink {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.remaining == 0 {
                return Err(io::Error::other("sink full"));
            }
            let len = bytes.len().min(self.remaining);
            self.remaining -= len;
            Ok(len)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut sink = ShortSink { remaining: 100 };
    assert!(matches!(
        decode_v1(&input, &mut sink, || false),
        Err(Error::Sink(_))
    ));
    assert_eq!(sink.remaining, 0);
    Ok(())
}

#[test]
fn rejects_multiple_frames_and_truncated_frame() -> io::Result<()> {
    let frame = frame(b"data", false)?;
    let mut twice = frame.clone();
    twice.extend_from_slice(&frame);
    assert!(matches!(
        decode_v1(&envelope(&twice, 8), &mut Vec::new(), || false),
        Err(Error::Frame(zstd_frame::Error::TrailingData))
    ));
    assert!(matches!(
        decode_v1(
            &envelope(&frame[..frame.len() - 1], 4),
            &mut Vec::new(),
            || false
        ),
        Err(Error::Frame(zstd_frame::Error::InvalidFrame))
    ));
    Ok(())
}

struct MemoryCapture(Vec<u8>);
impl Write for MemoryCapture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl crate::envelope::spool::CaptureSpool for MemoryCapture {
    type Sealed = MemorySealed;
    fn seal(self) -> io::Result<Self::Sealed> {
        Ok(MemorySealed(std::io::Cursor::new(self.0)))
    }
}

struct MemorySealed(std::io::Cursor<Vec<u8>>);
impl Read for MemorySealed {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.0.read(bytes)
    }
}
impl std::io::Seek for MemorySealed {
    fn seek(&mut self, position: std::io::SeekFrom) -> io::Result<u64> {
        self.0.seek(position)
    }
}
impl crate::envelope::spool::SealedSpool for MemorySealed {}

#[derive(Default)]
struct Stage {
    bytes: Vec<u8>,
    discarded: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    fail_write: bool,
    fail_discard: bool,
}
impl Write for Stage {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.fail_write {
            return Err(io::Error::other("sink failure"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl StagingSink for Stage {
    fn discard(self) -> io::Result<()> {
        self.discarded
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.fail_discard {
            Err(io::Error::other("cleanup failure"))
        } else {
            Ok(())
        }
    }
}
fn sealed(data: &[u8]) -> Result<VerifiedSpool<MemorySealed>, String> {
    crate::envelope::spool::capture_v1(
        &mut std::io::Cursor::new(data),
        MemoryCapture(Vec::new()),
        None,
        |_, _| true,
        || false,
    )
    .map_err(|e| format!("{e:?}"))
}
fn valid_records() -> Vec<u8> {
    vec![1, 0, 0, 0, 1, 1, 0, 0, 0, 0x7f]
}

#[test]
fn sealed_framing_uses_captured_input_and_returns_only_successful_stage() -> Result<(), String> {
    let records = valid_records();
    let compressed = frame(&records, true).map_err(|e| e.to_string())?;
    let mut source = envelope_with_count(&compressed, records.len() as u64, 2);
    let mut spool = sealed(&source)?;
    source.fill(0);
    let stage = validate_and_decode_sealed(&mut spool, Stage::default(), || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(stage.bytes, records);
    assert_eq!(stage.discarded.load(std::sync::atomic::Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn sealed_rejects_checksum_record_errors_and_discards_stage() -> Result<(), String> {
    let records = valid_records();
    let mut compressed = frame(&records, true).map_err(|e| e.to_string())?;
    let last = compressed.len() - 1;
    compressed[last] ^= 1;
    let mut spool = sealed(&envelope_with_count(&compressed, records.len() as u64, 2))?;
    let stage = Stage::default();
    let dropped = stage.discarded.clone();
    assert!(matches!(
        validate_and_decode_sealed(&mut spool, stage, || false),
        Err(Error::Decode(_))
    ));
    assert_eq!(dropped.load(std::sync::atomic::Ordering::SeqCst), 1);
    let compressed = frame(&records, false).map_err(|e| e.to_string())?;
    let mut spool = sealed(&envelope_with_count(&compressed, records.len() as u64, 1))?;
    let stage = Stage::default();
    let dropped = stage.discarded.clone();
    assert!(matches!(
        validate_and_decode_sealed(&mut spool, stage, || false),
        Err(Error::Records)
    ));
    assert_eq!(dropped.load(std::sync::atomic::Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn sealed_cap_cancellation_and_cleanup_failure_are_reported() -> Result<(), String> {
    let records = valid_records();
    let compressed = frame(&records, false).map_err(|e| e.to_string())?;
    let mut spool = sealed(&envelope_with_count(
        &compressed,
        records.len() as u64 - 1,
        2,
    ))?;
    let stage = Stage::default();
    let dropped = stage.discarded.clone();
    assert!(matches!(
        validate_and_decode_sealed(&mut spool, stage, || false),
        Err(Error::DecodedLimit)
    ));
    assert_eq!(dropped.load(std::sync::atomic::Ordering::SeqCst), 1);
    let mut spool = sealed(&envelope_with_count(&compressed, records.len() as u64, 2))?;
    let stage = Stage::default();
    let dropped = stage.discarded.clone();
    assert!(matches!(
        validate_and_decode_sealed(&mut spool, stage, || true),
        Err(Error::Cancelled)
    ));
    assert_eq!(dropped.load(std::sync::atomic::Ordering::SeqCst), 1);
    let stage = Stage {
        fail_write: true,
        fail_discard: true,
        ..Stage::default()
    };
    let dropped = stage.discarded.clone();
    assert!(
        matches!(validate_and_decode_sealed(&mut spool, stage, || false), Err(Error::Discard { failure, .. }) if matches!(*failure, Error::Sink(_)))
    );
    assert_eq!(dropped.load(std::sync::atomic::Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn shared_decoder_checks_short_reads_and_trailing_bytes() -> io::Result<()> {
    let records = valid_records();
    let compressed = frame(&records, true)?;
    let mut sink = Vec::new();
    assert!(
        decode_reader(
            &compressed[..compressed.len() - 1],
            compressed.len() as u64,
            records.len() as u64,
            &mut || false,
            &mut |bytes| {
                sink.extend_from_slice(bytes);
                Ok(())
            }
        )
        .is_err()
    );
    let mut trailing = compressed;
    trailing.push(0);
    assert!(matches!(
        decode_reader(
            trailing.as_slice(),
            trailing.len() as u64,
            records.len() as u64,
            &mut || false,
            &mut |_| Ok(())
        ),
        Err(Error::Frame(zstd_frame::Error::TrailingData))
    ));
    Ok(())
}

#[test]
fn shared_decoder_accepts_repeated_short_reads_and_cancels_midstream() -> io::Result<()> {
    struct Short<'a>(&'a [u8]);
    impl Read for Short<'_> {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            let n = out.len().min(self.0.len()).min(2);
            out[..n].copy_from_slice(&self.0[..n]);
            self.0 = &self.0[n..];
            Ok(n)
        }
    }
    let original = vec![42; 100_000];
    let compressed = frame(&original, true)?;
    let mut output = Vec::new();
    assert_eq!(
        decode_reader(
            Short(&compressed),
            compressed.len() as u64,
            original.len() as u64,
            &mut || false,
            &mut |chunk| {
                output.extend_from_slice(chunk);
                Ok(())
            }
        )
        .ok(),
        Some(original.len() as u64)
    );
    assert_eq!(output, original);
    let mut calls = 0;
    assert!(matches!(
        decode_reader(
            Short(&compressed),
            compressed.len() as u64,
            original.len() as u64,
            &mut || {
                calls += 1;
                calls > 2
            },
            &mut |_| Ok(())
        ),
        Err(Error::Cancelled)
    ));
    Ok(())
}

#[test]
fn sealed_item_validation_rejects_bad_payload_and_discards_stage() -> Result<(), String> {
    use crate::records::{
        NameEncoding,
        item::{Item, Kind, Size},
    };
    let item = Item {
        number: 1,
        parent: 0,
        kind: Kind::File,
        flags: 0,
        name: b"x",
        sizes: [Size::Unknown; 3],
        modified_ms: None,
        identity: &[],
        link_count: 0,
    };
    let body = item
        .encode(NameEncoding::Bytes)
        .map_err(|e| format!("{e:?}"))?;
    let make = |body: &[u8]| {
        let mut records = vec![1, 0, 0, 0, 1];
        records.extend_from_slice(&((body.len() + 1) as u32).to_le_bytes());
        records.push(2);
        records.extend_from_slice(body);
        records.extend_from_slice(&[1, 0, 0, 0, 127]);
        records
    };
    let records = make(&body);
    let compressed = frame(&records, true).map_err(|e| e.to_string())?;
    let mut spool = sealed(&envelope_with_count(&compressed, records.len() as u64, 3))?;
    let stage =
        validate_items_in_sealed(&mut spool, Stage::default(), NameEncoding::Bytes, || false)
            .map_err(|e| format!("{e:?}"))?;
    assert_eq!(stage.bytes, records);
    for mutation in [0, 1, 2] {
        let mut invalid = body.clone();
        match mutation {
            0 => invalid[8] = 0,
            1 => invalid[0] = 2,
            _ => invalid[13] = b'/',
        }
        let records = make(&invalid);
        let compressed = frame(&records, true).map_err(|e| e.to_string())?;
        let mut spool = sealed(&envelope_with_count(&compressed, records.len() as u64, 3))?;
        let stage = Stage::default();
        let discarded = stage.discarded.clone();
        assert!(matches!(
            validate_items_in_sealed(&mut spool, stage, NameEncoding::Bytes, || false),
            Err(Error::Records)
        ));
        assert_eq!(discarded.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
    Ok(())
}

mod gap;

mod volume;
