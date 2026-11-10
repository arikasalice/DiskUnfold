use super::*;

fn frame(data: &[u8], checksum: bool) -> io::Result<Vec<u8>> {
    let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), 1)?;
    encoder.include_checksum(checksum)?;
    encoder.write_all(data)?;
    encoder.finish()
}

fn envelope(frame: &[u8], limit: u64) -> Vec<u8> {
    let mut data = b"DUSNAP\r\n".to_vec();
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&2u32.to_le_bytes());
    data.extend_from_slice(b"{}");
    data.extend_from_slice(frame);
    data.extend_from_slice(&(frame.len() as u64).to_le_bytes());
    data.extend_from_slice(&0u64.to_le_bytes());
    data.extend_from_slice(&limit.to_le_bytes());
    data.extend_from_slice(blake3::hash(&data).as_bytes());
    data
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
