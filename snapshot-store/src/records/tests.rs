use super::*;

fn item<'a>(number: u32, parent: u32, name: &'a [u8]) -> Record<'a> {
    Record::Item {
        number,
        parent,
        name,
        identity: &[],
    }
}

#[test]
fn accepts_empty_and_complete_streams_with_unchanged_gap_data() -> Result<(), ValidationError> {
    let mut empty = Validator::new(NameEncoding::Bytes, 1)?;
    empty.accept(Record::Volume, 1, 5)?;
    empty.accept(Record::End, 1, 5)?;
    empty.finish()?;
    let mut validator = Validator::new(NameEncoding::Bytes, MAX_FIELD_BYTES)?;
    validator.accept(Record::Volume, 1, 5)?;
    let original = b"a\xff\\name \"";
    let record = item(1, 0, original);
    validator.accept(record, 32, 36)?;
    assert_eq!(record, item(1, 0, original));
    validator.accept(item(2, 1, b"child"), 20, 24)?;
    let gap = Record::Gap {
        item: 2,
        reason_key: "scan.path_denied",
    };
    validator.accept(gap, 20, 24)?;
    assert_eq!(
        gap,
        Record::Gap {
            item: 2,
            reason_key: "scan.path_denied"
        }
    );
    validator.accept(Record::End, 1, 5)?;
    validator.finish()
}

#[test]
fn enforces_record_order_numbering_and_parent_without_advancing_on_failure()
-> Result<(), ValidationError> {
    let mut v = Validator::new(NameEncoding::Bytes, 10)?;
    assert_eq!(
        v.accept(item(1, 0, b"x"), 1, 5),
        Err(ValidationError::RecordOrder)
    );
    v.accept(Record::Volume, 1, 5)?;
    assert_eq!(
        v.accept(Record::Volume, 1, 5),
        Err(ValidationError::RecordOrder)
    );
    assert_eq!(
        v.accept(item(2, 0, b"x"), 1, 5),
        Err(ValidationError::ItemNumber)
    );
    assert_eq!(
        v.accept(item(1, 1, b"x"), 1, 5),
        Err(ValidationError::Parent)
    );
    v.accept(item(1, 0, b"x"), 1, 5)?;
    assert_eq!(
        v.accept(
            Record::Gap {
                item: 2,
                reason_key: "scan.path_denied"
            },
            1,
            5
        ),
        Err(ValidationError::GapItem)
    );
    v.accept(
        Record::Gap {
            item: 1,
            reason_key: "scan.path_denied",
        },
        1,
        5,
    )?;
    assert_eq!(
        v.accept(item(2, 1, b"y"), 1, 5),
        Err(ValidationError::RecordOrder)
    );
    v.accept(Record::End, 1, 5)?;
    assert_eq!(
        v.accept(Record::End, 1, 5),
        Err(ValidationError::RecordOrder)
    );
    v.finish()
}

#[test]
fn raw_names_validate_units_without_utf_conversion() -> Result<(), ValidationError> {
    let mut windows = Validator::new(NameEncoding::WindowsUtf16Le, 1)?;
    windows.accept(Record::Volume, 1, 5)?;
    windows.accept(item(1, 0, &[0, 0xd8, b' ', 0]), 1, 5)?;
    for invalid in [&[0x41][..], &[0, 0], &[47, 0], &[92, 0], &[]] {
        assert_eq!(
            windows.accept(item(2, 0, invalid), 1, 5),
            Err(ValidationError::Name)
        );
    }
    windows.accept(item(2, 0, &[0x41, 0]), 1, 5)?;
    let mut unix = Validator::new(NameEncoding::Bytes, 1)?;
    unix.accept(Record::Volume, 1, 5)?;
    unix.accept(item(1, 0, &[0xff, b'\\']), 1, 5)?;
    for invalid in [&[0][..], b"/"] {
        assert_eq!(
            unix.accept(item(2, 0, invalid), 1, 5),
            Err(ValidationError::Name)
        );
    }
    Ok(())
}

#[test]
fn applies_published_length_bounds_and_requires_end() -> Result<(), ValidationError> {
    assert!(matches!(
        Validator::new(NameEncoding::Bytes, 0),
        Err(ValidationError::MetadataLength)
    ));
    assert!(matches!(
        Validator::new(NameEncoding::Bytes, MAX_FIELD_BYTES + 1),
        Err(ValidationError::MetadataLength)
    ));
    let mut v = Validator::new(NameEncoding::Bytes, 1)?;
    assert_eq!(
        v.accept(Record::Volume, MAX_FIELD_BYTES + 1, 1),
        Err(ValidationError::RecordLength)
    );
    v.accept(Record::Volume, MAX_FIELD_BYTES, MAX_DECODED_BYTES)?;
    assert_eq!(
        v.accept(Record::End, 1, 1),
        Err(ValidationError::DecodedLimit)
    );
    v.accept(Record::End, 1, 0)?;
    v.finish()?;
    let mut v = Validator::new(NameEncoding::Bytes, 1)?;
    v.accept(Record::Volume, 1, 5)?;
    let long = vec![b'a'; MAX_NAME_BYTES + 1];
    assert_eq!(
        v.accept(item(1, 0, &long), 1, 5),
        Err(ValidationError::Name)
    );
    let valid = vec![b'a'; MAX_NAME_BYTES];
    v.accept(item(1, 0, &valid), 1, 5)?;
    let identity = [0; MAX_IDENTITY_BYTES + 1];
    assert_eq!(
        v.accept(
            Record::Item {
                number: 2,
                parent: 0,
                name: b"x",
                identity: &identity
            },
            1,
            5
        ),
        Err(ValidationError::IdentityLength)
    );
    assert_eq!(v.finish(), Err(ValidationError::MissingEnd));
    Ok(())
}
