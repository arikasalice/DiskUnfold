use super::*;

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

#[test]
fn preserves_order_and_first_language() -> io::Result<()> {
    let expected = wide("ko-KR\0en-US\0\0");
    let actual = read_with(|buffer| {
        if let Some(buffer) = buffer {
            buffer.copy_from_slice(&expected);
        }
        Ok((
            2,
            u32::try_from(expected.len()).map_err(|_| invalid_data())?,
        ))
    })?;
    assert_eq!(actual, ["ko-KR", "en-US"]);
    Ok(())
}

#[test]
fn empty_and_malformed_os_responses_are_not_promoted_to_a_language() -> io::Result<()> {
    assert!(parse(&[0, 0], 0)?.is_empty());
    for (buffer, count) in [
        (vec![], 0),
        (wide("ko-KR\0"), 1),
        (wide("ko-KR\0en-US\0\0"), 1),
        (vec![0xd800, 0, 0], 1),
        (wide("\0ko\0\0"), 1),
    ] {
        assert_eq!(
            parse(&buffer, count).err().map(|error| error.kind()),
            Some(io::ErrorKind::InvalidData)
        );
    }
    Ok(())
}

#[test]
fn retries_a_resized_list_and_preserves_other_errors() -> io::Result<()> {
    let expected = wide("en-US\0\0");
    let mut reads = 0;
    let result = read_with(|buffer| {
        if let Some(buffer) = buffer {
            reads += 1;
            if reads == 1 {
                return Err(io::Error::from_raw_os_error(INSUFFICIENT_BUFFER));
            }
            buffer.copy_from_slice(&expected);
        }
        Ok((
            1,
            u32::try_from(expected.len()).map_err(|_| invalid_data())?,
        ))
    })?;
    assert_eq!(result, ["en-US"]);
    assert_eq!(reads, 2);
    let failure = read_with(|_| Err(io::Error::from_raw_os_error(5)))
        .err()
        .ok_or_else(invalid_data)?;
    assert_eq!(failure.raw_os_error(), Some(5));
    let too_large = read_with(|_| Ok((0, u32::MAX)))
        .err()
        .ok_or_else(invalid_data)?;
    assert_eq!(too_large.kind(), io::ErrorKind::InvalidData);
    Ok(())
}

#[test]
fn repeated_resize_is_bounded() {
    let mut reads = 0;
    let error = read_with(|buffer| {
        if buffer.is_some() {
            reads += 1;
            Err(io::Error::from_raw_os_error(INSUFFICIENT_BUFFER))
        } else {
            Ok((1, 2))
        }
    });
    assert_eq!(reads, ATTEMPTS);
    assert_eq!(
        error.err().and_then(|e| e.raw_os_error()),
        Some(INSUFFICIENT_BUFFER)
    );
}

#[cfg(windows)]
#[test]
fn windows_user_ui_language_is_readable() -> io::Result<()> {
    let tags = preferred_ui_languages()?;
    assert!(!tags.is_empty());
    assert!(
        tags.iter()
            .all(|tag| !tag.is_empty() && !tag.contains('\0'))
    );
    Ok(())
}
