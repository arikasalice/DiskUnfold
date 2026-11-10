use super::*;

fn raw_frame(data: &[u8]) -> Vec<u8> {
    let mut frame = MAGIC.to_vec();
    frame.extend_from_slice(&[0x20, data.len() as u8]);
    let header = ((data.len() as u32) << 3) | 1;
    frame.extend_from_slice(&header.to_le_bytes()[..3]);
    frame.extend_from_slice(data);
    frame
}

fn envelope(frame: &[u8], decoded: u64) -> Vec<u8> {
    let mut data = b"DUSNAP\r\n".to_vec();
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&2u32.to_le_bytes());
    data.extend_from_slice(b"{}");
    data.extend_from_slice(frame);
    data.extend_from_slice(&(frame.len() as u64).to_le_bytes());
    data.extend_from_slice(&0u64.to_le_bytes());
    data.extend_from_slice(&decoded.to_le_bytes());
    data.extend_from_slice(blake3::hash(&data).as_bytes());
    data
}

#[test]
fn accepts_one_frame_after_envelope_integrity() {
    let frame = raw_frame(b"abc");
    let data = envelope(&frame, 3);
    let before = data.clone();
    assert_eq!(
        inspect_v1(&data),
        Ok(FrameInfo {
            window_size: 3,
            content_size: Some(3)
        })
    );
    assert_eq!(data, before);
    let mut corrupted = data;
    let last = corrupted.len() - 1;
    corrupted[last] ^= 1;
    assert!(matches!(
        inspect_v1(&corrupted),
        Err(Error::Envelope(envelope::Error::IntegrityFailed))
    ));
}

#[test]
fn rejects_multiple_skippable_and_truncated_frames() {
    let frame = raw_frame(b"abc");
    let mut multiple = frame.clone();
    multiple.extend_from_slice(&frame);
    assert_eq!(inspect_frame(&multiple, 6), Err(Error::TrailingData));
    assert_eq!(
        inspect_frame(&[0x50, 0x2a, 0x4d, 0x18, 0, 0, 0, 0], 0),
        Err(Error::InvalidFrame)
    );
    for end in 0..frame.len() {
        assert!(
            inspect_frame(&frame[..end], 3).is_err(),
            "truncation at {end}"
        );
    }
    let mut unfinished = frame;
    unfinished[6] &= !1;
    assert_eq!(inspect_frame(&unfinished, 3), Err(Error::InvalidFrame));
}

#[test]
fn checks_dictionary_window_and_advertised_output_limits() {
    let mut dict = MAGIC.to_vec();
    dict.extend_from_slice(&[0x21, 1, 0, 1, 0, 0]);
    assert_eq!(inspect_frame(&dict, 0), Err(Error::Dictionary));
    dict[5] = 0;
    assert!(inspect_frame(&dict, 0).is_ok());
    let mut max_window = MAGIC.to_vec();
    max_window.extend_from_slice(&[0, 0x88, 1, 0, 0]);
    assert_eq!(
        inspect_frame(&max_window, 0),
        Ok(FrameInfo {
            window_size: 1 << 27,
            content_size: None
        })
    );
    max_window[5] = 0x89;
    assert_eq!(inspect_frame(&max_window, 0), Err(Error::WindowLimit));
    assert_eq!(
        inspect_frame(&raw_frame(b"abc"), 2),
        Err(Error::DecodedLimit)
    );
    assert!(matches!(
        inspect_v1(&envelope(&raw_frame(b""), (1u64 << 32) + 1)),
        Err(Error::Envelope(envelope::Error::DecodedLimit))
    ));
    let mut huge_single = MAGIC.to_vec();
    huge_single.extend_from_slice(&[0xa0, 1, 0, 0, 8]);
    huge_single.extend_from_slice(&[1, 0, 0]);
    assert_eq!(
        inspect_frame(&huge_single, 1 << 32),
        Err(Error::WindowLimit)
    );
}

#[test]
fn rejects_reserved_block_and_missing_checksum() {
    let mut frame = raw_frame(b"abc");
    frame[6] |= 6;
    assert_eq!(inspect_frame(&frame, 3), Err(Error::InvalidFrame));
    let mut checksum = raw_frame(b"abc");
    checksum[4] |= 4;
    assert_eq!(inspect_frame(&checksum, 3), Err(Error::InvalidFrame));
    checksum.extend_from_slice(&[0, 0, 0, 0]);
    assert!(inspect_frame(&checksum, 3).is_ok()); // The checksum is not verified here.
}

#[test]
fn shared_slice_and_reader_parser_match_frame_boundaries() {
    use std::io::Read;
    let valid = raw_frame(b"abc");
    let mut trailing = valid.clone();
    trailing.push(0);
    let mut dictionary = valid.clone();
    dictionary[4] |= 1;
    dictionary.insert(5, 1);
    for bytes in [
        valid.clone(),
        valid[..valid.len() - 1].to_vec(),
        trailing,
        dictionary,
        vec![0; 5],
    ] {
        let slice = inspect_frame_cancellable(&bytes, 100, &mut || false);
        let mut input = bytes.as_slice().take(bytes.len() as u64);
        let stream = inspect_reader(&mut input, 100, &mut || false).map_err(|error| match error {
            ReadError::Frame(e) => e,
            ReadError::Io(_) => Error::InvalidFrame,
        });
        assert_eq!(slice, stream);
    }
}

#[test]
fn reader_parser_preserves_io_and_cancellation() {
    use std::io::Read;
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("broken"))
        }
    }
    assert!(matches!(
        inspect_reader(&mut Broken.take(100), 100, &mut || false),
        Err(ReadError::Io(_))
    ));
    assert!(matches!(
        inspect_reader(
            &mut raw_frame(b"abc").as_slice().take(100),
            100,
            &mut || true
        ),
        Err(ReadError::Frame(Error::Cancelled))
    ));
}
