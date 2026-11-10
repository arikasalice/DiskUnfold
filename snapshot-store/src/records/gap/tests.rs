use super::*;

#[test]
fn wire_roundtrip_multiple_roots_and_root_itself() -> Result<(), Error> {
    let g = Gap {
        root_index: 1,
        path: vec![b"a\xff", b"child"],
        reason_key: "unknown.reason-1",
        observed_at_ms: Some(-42),
    };
    let encoded = g.encode(NameEncoding::Bytes, 2)?;
    assert_eq!(&encoded[..10], &[1, 0, 0, 0, 2, 0, 2, 0, b'a', 255]);
    assert_eq!(decode(&encoded, NameEncoding::Bytes, 2)?, g);
    let root = Gap {
        root_index: 0,
        path: vec![],
        reason_key: "x",
        observed_at_ms: None,
    };
    let bytes = root.encode(NameEncoding::Bytes, 1)?;
    assert_eq!(
        bytes,
        [0, 0, 0, 0, 0, 0, 1, 0, b'x', 0, 0, 0, 0, 0, 0, 0, 128]
    );
    assert_eq!(decode(&bytes, NameEncoding::Bytes, 1)?, root);
    assert_eq!(
        decode(&bytes, NameEncoding::Bytes, 0),
        Err(Error::RootIndex)
    );
    Ok(())
}

#[test]
fn raw_windows_names_and_invalid_components() -> Result<(), Error> {
    let g = Gap {
        root_index: 0,
        path: vec![&[0x00, 0xd8], &[b'a', 0]],
        reason_key: "X_1",
        observed_at_ms: Some(0),
    };
    let bytes = g.encode(NameEncoding::WindowsUtf16Le, 1)?;
    assert_eq!(decode(&bytes, NameEncoding::WindowsUtf16Le, 1)?, g);
    for name in [&[][..], b".", b"..", b"/", b"\0"] {
        assert_eq!(
            Gap {
                path: vec![name],
                ..g.clone()
            }
            .encode(NameEncoding::Bytes, 1),
            Err(Error::Name)
        );
    }
    for name in [
        &[b'.', 0][..],
        &[b'.', 0, b'.', 0],
        &[b'\\', 0],
        b"a",
        &[0, 0],
    ] {
        assert_eq!(
            Gap {
                path: vec![name],
                ..g.clone()
            }
            .encode(NameEncoding::WindowsUtf16Le, 1),
            Err(Error::Name)
        );
    }
    Ok(())
}

#[test]
fn rejects_every_truncation_extra_bytes_keys_and_limits() -> Result<(), Error> {
    let g = Gap {
        root_index: 0,
        path: vec![b"path"],
        reason_key: "scan.missing",
        observed_at_ms: Some(0),
    };
    let bytes = g.encode(NameEncoding::Bytes, 1)?;
    for n in 0..bytes.len() {
        assert!(decode(&bytes[..n], NameEncoding::Bytes, 1).is_err(), "{n}");
    }
    let mut extra = bytes.clone();
    extra.push(0);
    assert_eq!(
        decode(&extra, NameEncoding::Bytes, 1),
        Err(Error::TrailingBytes)
    );
    for key in ["", "a b", "é", "a/b"] {
        assert_eq!(
            Gap {
                reason_key: key,
                ..g.clone()
            }
            .encode(NameEncoding::Bytes, 1),
            Err(Error::ReasonKey)
        );
    }
    assert_eq!(
        Gap {
            observed_at_ms: Some(i64::MIN),
            ..g.clone()
        }
        .encode(NameEncoding::Bytes, 1),
        Err(Error::Timestamp)
    );
    assert_eq!(
        Gap {
            observed_at_ms: Some(i64::MAX),
            ..g.clone()
        }
        .encode(NameEncoding::Bytes, 1),
        Err(Error::Timestamp)
    );
    let mut invalid_time = bytes;
    let time_offset = invalid_time.len() - 8;
    invalid_time[time_offset..].copy_from_slice(&i64::MAX.to_le_bytes());
    assert_eq!(
        decode(&invalid_time, NameEncoding::Bytes, 1),
        Err(Error::Timestamp)
    );
    let too_long = vec![b'a'; 1025];
    assert_eq!(
        Gap {
            path: vec![&too_long],
            ..g.clone()
        }
        .encode(NameEncoding::Bytes, 1),
        Err(Error::Name)
    );
    let huge = vec![b'a'; 65_536];
    assert_eq!(
        Gap {
            reason_key: std::str::from_utf8(&huge).map_err(|_| Error::ReasonKey)?,
            ..g
        }
        .encode(NameEncoding::Bytes, 1),
        Err(Error::RecordLength)
    );
    Ok(())
}
