use super::*;

fn fixture(metadata: &[u8], body: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&(metadata.len() as u32).to_le_bytes());
    bytes.extend_from_slice(metadata);
    bytes.extend_from_slice(body);
    bytes.extend_from_slice(&(body.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&0x0102030405060708u64.to_le_bytes());
    bytes.extend_from_slice(&123u64.to_le_bytes());
    bytes.extend_from_slice(blake3::hash(&bytes).as_bytes());
    bytes
}

#[test]
fn reads_little_endian_fields_after_integrity_check() -> Result<(), Error> {
    let bytes = fixture(br#"{"formatVersion":1}"#, b"body-bytes");
    let view = verify_v1(&bytes)?;
    assert_eq!(view.version, 1);
    assert_eq!(view.metadata, r#"{"formatVersion":1}"#);
    assert_eq!(view.compressed, b"body-bytes");
    assert_eq!(view.record_count, 0x0102030405060708);
    assert_eq!(view.decoded_length, 123);
    assert_eq!(view.bytes_before_digest, &bytes[..bytes.len() - 32]);
    Ok(())
}

#[test]
fn verifies_fixed_v1_blake3_vector_and_rejects_body_only_digest() {
    // Independently generated with the Python BLAKE3 implementation.
    let digest = [
        0xbb, 0xb1, 0x28, 0x86, 0x6d, 0x20, 0x46, 0x1d, 0x61, 0xa4, 0x8d, 0x50, 0xd5, 0x48, 0x22,
        0x11, 0x6c, 0x74, 0xd3, 0xa6, 0x98, 0xf1, 0xf8, 0x1b, 0x5c, 0x94, 0x13, 0x80, 0x56, 0xa2,
        0xbf, 0xb3,
    ];
    let mut bytes = fixture(b"{}", b"body");
    let digest_start = bytes.len() - HASH_BYTES;
    bytes[digest_start..].copy_from_slice(&digest);
    assert!(verify_v1(&bytes).is_ok());
    bytes[digest_start..].copy_from_slice(blake3::hash(b"body").as_bytes());
    assert!(matches!(verify_v1(&bytes), Err(Error::IntegrityFailed)));
}

#[test]
fn catches_truncation_extension_and_forged_lengths() {
    let original = fixture(b"{}", b"body");
    for end in 0..original.len() {
        assert!(
            inspect(&original[..end]).is_err(),
            "accepted truncated input {end}"
        );
    }
    let mut trailing = original.clone();
    trailing.push(0);
    assert!(inspect(&trailing).is_err());
    let mut forged = original.clone();
    let footer = forged.len() - FOOTER_BYTES as usize;
    forged[footer..footer + 8].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(matches!(inspect(&forged), Err(Error::LengthMismatch)));
    assert_eq!(
        layout(u64::MAX, MAX_METADATA_BYTES, u64::MAX),
        Err(Error::LengthMismatch)
    );
}

#[test]
fn detects_tampering_and_never_changes_input() {
    let original = fixture(b"{}", b"body");
    for offset in [
        16,
        18,
        original.len() - 48,
        original.len() - 40,
        original.len() - 32,
        original.len() - 1,
    ] {
        let mut changed = original.clone();
        changed[offset] ^= 1;
        let before = changed.clone();
        assert!(matches!(verify_v1(&changed), Err(Error::IntegrityFailed)));
        assert_eq!(changed, before);
    }
}

#[test]
fn rejects_unregistered_version() {
    let mut bytes = fixture(b"{}", b"body");
    bytes[8..10].copy_from_slice(&2u16.to_le_bytes());
    assert!(matches!(verify_v1(&bytes), Err(Error::UnsupportedVersion)));
}

#[test]
fn checks_header_metadata_and_decoded_bounds() {
    assert_eq!(check_file_length(71), Err(Error::Truncated));
    assert_eq!(check_file_length(72), Ok(()));
    assert_eq!(check_file_length(MAX_FILE_BYTES), Ok(()));
    assert_eq!(check_file_length(MAX_FILE_BYTES + 1), Err(Error::FileLimit));
    let mut bytes = fixture(b"{}", b"body");
    bytes[0] ^= 1;
    assert!(matches!(inspect(&bytes), Err(Error::Magic)));
    bytes = fixture(b"{}", b"body");
    bytes[10] = 1;
    assert!(matches!(inspect(&bytes), Err(Error::Flags)));
    bytes = fixture(b"{}", b"body");
    bytes[12..16].copy_from_slice(&0u32.to_le_bytes());
    assert!(matches!(inspect(&bytes), Err(Error::MetadataLength)));
    bytes[12..16].copy_from_slice(&65_537u32.to_le_bytes());
    assert!(matches!(inspect(&bytes), Err(Error::MetadataLength)));
    bytes = fixture(&[0xff], b"body");
    assert!(matches!(inspect(&bytes), Err(Error::MetadataEncoding)));
    bytes = fixture(b"{}", b"body");
    let footer = bytes.len() - FOOTER_BYTES as usize;
    bytes[footer + 16..footer + 24].copy_from_slice(&(MAX_DECODED_BYTES + 1).to_le_bytes());
    assert!(matches!(inspect(&bytes), Err(Error::DecodedLimit)));
    bytes[footer + 16..footer + 24].copy_from_slice(&MAX_DECODED_BYTES.to_le_bytes());
    assert!(inspect(&bytes).is_ok());
}
