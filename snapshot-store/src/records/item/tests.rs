use super::*;

fn example() -> Item<'static> {
    Item {
        number: 1,
        parent: 0,
        kind: Kind::File,
        flags: 0x8123,
        name: b"file",
        sizes: [Size::Exact(0), Size::LowerBound(u64::MAX), Size::Unknown],
        modified_ms: None,
        identity: &[0, 255],
        link_count: u32::MAX,
    }
}

#[test]
fn preserves_item_values_and_matches_wire_layout() -> Result<(), Error> {
    let item = example();
    let encoded = item.encode(NameEncoding::Bytes)?;
    let mut expected = vec![1, 0, 0, 0, 0, 0, 0, 0, 1, 0x23, 0x81, 4, 0];
    expected.extend(b"file");
    expected.push(0);
    expected.extend(0_u64.to_le_bytes());
    expected.push(1);
    expected.extend(u64::MAX.to_le_bytes());
    expected.push(2);
    expected.extend(i64::MIN.to_le_bytes());
    expected.extend([2, 0, 255]);
    expected.extend(u32::MAX.to_le_bytes());
    assert_eq!(encoded, expected);
    assert_eq!(decode(&encoded, NameEncoding::Bytes)?, item);
    Ok(())
}

#[test]
fn rejects_truncation_trailing_and_invalid_discriminants() -> Result<(), Error> {
    let encoded = example().encode(NameEncoding::Bytes)?;
    for end in 0..encoded.len() {
        assert!(decode(&encoded[..end], NameEncoding::Bytes).is_err());
    }
    let mut invalid = encoded.clone();
    invalid.push(0);
    assert_eq!(
        decode(&invalid, NameEncoding::Bytes),
        Err(Error::TrailingBytes)
    );
    invalid = encoded.clone();
    invalid[8] = 5;
    assert_eq!(
        decode(&invalid, NameEncoding::Bytes),
        Err(Error::InvalidKind)
    );
    invalid = encoded;
    invalid[17] = 3;
    assert_eq!(
        decode(&invalid, NameEncoding::Bytes),
        Err(Error::InvalidSizeState)
    );
    Ok(())
}

#[test]
fn validates_names_identity_numbers_and_timestamp() {
    let mut item = example();
    item.name = &[b'a', 0, b'\\', 0];
    assert_eq!(
        item.encode(NameEncoding::WindowsUtf16Le),
        Err(Error::InvalidName)
    );
    item.name = &[b'a', 0, b'b', 0];
    assert!(item.encode(NameEncoding::WindowsUtf16Le).is_ok());
    item.name = b"valid";
    item.identity = &[0; 33];
    assert_eq!(item.encode(NameEncoding::Bytes), Err(Error::IdentityLength));
    item.identity = &[];
    item.parent = 1;
    assert_eq!(item.encode(NameEncoding::Bytes), Err(Error::InvalidNumber));
    item.parent = 0;
    item.modified_ms = Some(i64::MIN);
    assert_eq!(
        item.encode(NameEncoding::Bytes),
        Err(Error::InvalidTimestamp)
    );
}
