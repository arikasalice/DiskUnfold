use super::*;
use std::io::Cursor;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct Capture {
    bytes: Vec<u8>,
    cleanup: Arc<AtomicUsize>,
    sealed: bool,
}
struct Sealed {
    bytes: Cursor<Vec<u8>>,
    cleanup: Arc<AtomicUsize>,
}
impl Drop for Capture {
    fn drop(&mut self) {
        if !self.sealed {
            self.cleanup.fetch_add(1, Ordering::SeqCst);
        }
    }
}
impl Drop for Sealed {
    fn drop(&mut self) {
        self.cleanup.fetch_add(1, Ordering::SeqCst);
    }
}
impl Write for Capture {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        let n = b.len().min(3);
        self.bytes.extend_from_slice(&b[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl CaptureSpool for Capture {
    type Sealed = Sealed;
    fn seal(mut self) -> io::Result<Sealed> {
        self.sealed = true;
        Ok(Sealed {
            bytes: Cursor::new(std::mem::take(&mut self.bytes)),
            cleanup: self.cleanup.clone(),
        })
    }
}
impl Read for Sealed {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        self.bytes.read(b)
    }
}
impl Seek for Sealed {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        self.bytes.seek(from)
    }
}
impl SealedSpool for Sealed {}
fn capture(cleanup: &Arc<AtomicUsize>) -> Capture {
    Capture {
        bytes: Vec::new(),
        cleanup: cleanup.clone(),
        sealed: false,
    }
}
fn frame() -> Vec<u8> {
    vec![0x28, 0xb5, 0x2f, 0xfd, 0x20, 3, 25, 0, 0, b'a', b'b', b'c']
}
fn fixture(frame: &[u8]) -> Vec<u8> {
    let metadata = br#"{"formatId":"dusnap","formatVersion":1}"#;
    let mut data = b"DUSNAP\r\n".to_vec();
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&(metadata.len() as u32).to_le_bytes());
    data.extend_from_slice(metadata);
    data.extend_from_slice(frame);
    data.extend_from_slice(&(frame.len() as u64).to_le_bytes());
    data.extend_from_slice(&2u64.to_le_bytes());
    data.extend_from_slice(&3u64.to_le_bytes());
    data.extend_from_slice(blake3::hash(&data).as_bytes());
    data
}

#[test]
fn captures_once_and_reads_same_sealed_bytes_after_source_changes() -> Result<(), String> {
    let data = fixture(&frame());
    let digest = blake3::hash(&data[..data.len() - 32]);
    let cleanup = Arc::new(AtomicUsize::new(0));
    let mut source = Cursor::new(data);
    let mut verified = capture_v1(
        &mut source,
        capture(&cleanup),
        Some(*digest.as_bytes()),
        |_, _| true,
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    source.get_mut().fill(0);
    let mut compressed = Vec::new();
    verified
        .compressed_reader()
        .map_err(|e| e.to_string())?
        .read_to_end(&mut compressed)
        .map_err(|e| e.to_string())?;
    assert_eq!(compressed, frame());
    assert_eq!(verified.record_count(), 2);
    assert_eq!(verified.decoded_length(), 3);
    assert_eq!(cleanup.load(Ordering::SeqCst), 0);
    drop(verified);
    assert_eq!(cleanup.load(Ordering::SeqCst), 1);
    Ok(())
}
#[test]
fn compression_precedes_integrity_and_catalog_digest_is_checked() {
    let cleanup = Arc::new(AtomicUsize::new(0));
    let mut invalid = fixture(&frame());
    let metadata_len =
        u32::from_le_bytes([invalid[12], invalid[13], invalid[14], invalid[15]]) as usize;
    invalid[16 + metadata_len] ^= 1;
    assert!(matches!(
        capture_v1(
            &mut Cursor::new(invalid),
            capture(&cleanup),
            None,
            |_, _| true,
            || false
        ),
        Err(Error::Frame(FrameError::InvalidFrame))
    ));
    assert!(matches!(
        capture_v1(
            &mut Cursor::new(fixture(&frame())),
            capture(&cleanup),
            Some([0; 32]),
            |_, _| true,
            || false
        ),
        Err(Error::IndexIntegrity)
    ));
    let mut bad_digest = fixture(&frame());
    let last = bad_digest.len() - 1;
    bad_digest[last] ^= 1;
    assert!(matches!(
        capture_v1(
            &mut Cursor::new(bad_digest),
            capture(&cleanup),
            None,
            |_, _| true,
            || false
        ),
        Err(Error::Format(FormatError::IntegrityFailed))
    ));
    assert_eq!(cleanup.load(Ordering::SeqCst), 3);
}
#[test]
fn truncation_growth_and_cancellation_cleanup() {
    let cleanup = Arc::new(AtomicUsize::new(0));
    let data = fixture(&frame());
    for end in 0..data.len() {
        assert!(
            capture_v1(
                &mut Cursor::new(&data[..end]),
                capture(&cleanup),
                None,
                |_, _| true,
                || false
            )
            .is_err()
        );
    }
    assert_eq!(cleanup.load(Ordering::SeqCst), data.len());
    let mut dest = Vec::new();
    assert!(matches!(
        copy(&mut Cursor::new(vec![7; 73]), &mut dest, 72, &mut || false),
        Err(Error::Format(FormatError::FileLimit))
    ));
    assert_eq!(dest.len(), 72);
    let mut polls = 0;
    assert!(matches!(
        capture_v1(
            &mut Cursor::new(data),
            capture(&cleanup),
            None,
            |_, _| true,
            || {
                polls += 1;
                polls > 4
            }
        ),
        Err(Error::Cancelled)
    ));
    assert_eq!(
        cleanup.load(Ordering::SeqCst),
        1 + 16 + br#"{"formatId":"dusnap","formatVersion":1}"#.len() + frame().len() + 56
    );
}
#[test]
fn malformed_metadata_and_frame_trailing_bytes_are_rejected_at_appropriate_stages() {
    let cleanup = Arc::new(AtomicUsize::new(0));
    let mut trailing = frame();
    trailing.push(0);
    assert!(matches!(
        capture_v1(
            &mut Cursor::new(fixture(&trailing)),
            capture(&cleanup),
            None,
            |_, _| true,
            || false
        ),
        Err(Error::Frame(FrameError::TrailingData))
    ));
    let mut invalid = fixture(&frame());
    invalid[16] = 0xff;
    assert!(matches!(
        capture_v1(
            &mut Cursor::new(invalid),
            capture(&cleanup),
            None,
            |_, _| true,
            || false
        ),
        Err(Error::Format(FormatError::MetadataEncoding))
    ));
}

#[test]
fn metadata_validator_is_required_before_compression_and_digest() {
    let cleanup = Arc::new(AtomicUsize::new(0));
    let mut invalid_frame = frame();
    invalid_frame[0] = 0;
    let result = capture_v1(
        &mut Cursor::new(fixture(&invalid_frame)),
        capture(&cleanup),
        None,
        |metadata, version| metadata == "never" && version == 1,
        || false,
    );
    assert!(matches!(result, Err(Error::Metadata)));
    assert_eq!(cleanup.load(Ordering::SeqCst), 1);
}

#[test]
fn unknown_version_precedes_v1_fields_and_never_calls_metadata() {
    let cleanup = Arc::new(AtomicUsize::new(0));
    let mut data = fixture(&frame());
    data[8..10].copy_from_slice(&2u16.to_le_bytes());
    data[10..12].copy_from_slice(&1u16.to_le_bytes());
    let last = data.len() - 1;
    data[last] ^= 1;
    let mut calls = 0;
    let result = capture_v1(
        &mut Cursor::new(data),
        capture(&cleanup),
        None,
        |_, _| {
            calls += 1;
            true
        },
        || false,
    );
    assert!(matches!(
        result,
        Err(Error::Format(FormatError::UnsupportedVersion))
    ));
    assert_eq!(calls, 0);
    assert_eq!(cleanup.load(Ordering::SeqCst), 1);
}

#[test]
fn forged_metadata_length_is_rejected_before_metadata_is_read() {
    let cleanup = Arc::new(AtomicUsize::new(0));
    let mut data = fixture(&frame());
    data[12..16].copy_from_slice(&100u32.to_le_bytes());
    let mut calls = 0;
    let result = capture_v1(
        &mut Cursor::new(data),
        capture(&cleanup),
        None,
        |_, _| {
            calls += 1;
            true
        },
        || false,
    );
    assert!(matches!(
        result,
        Err(Error::Format(FormatError::LengthMismatch))
    ));
    assert_eq!(calls, 0);
    assert_eq!(cleanup.load(Ordering::SeqCst), 1);
}
