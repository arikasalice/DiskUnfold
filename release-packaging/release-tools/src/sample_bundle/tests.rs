use super::*;

fn bundle(header_json: &str, payload: &[u8]) -> Vec<u8> {
    let mut out = b"DUSAMPLE".to_vec();
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(header_json.len() as u32).to_le_bytes());
    out.extend_from_slice(header_json.as_bytes());
    out.extend_from_slice(payload);
    out
}

fn header(digest: &str) -> String {
    format!(
        r#"{{"bundleVersion":"1.2.3","digestAlgorithm":"blake3","digest":"{digest}","declaredItemCount":7,"scenarioIds":["a"]}}"#
    )
}

fn good() -> Vec<u8> {
    let payload = b"{}";
    bundle(&header(&blake3::hash(payload).to_hex()), payload)
}

#[test]
fn a_valid_bundle_reports_its_version_and_digest() {
    let record = verify_sample_bundle(&good());
    assert_eq!(
        record,
        Ok(SampleBundleRecord {
            bundle_version: "1.2.3".into(),
            format_version: 1,
            declared_item_count: 7,
            blake3: blake3::hash(b"{}").to_hex().to_string(),
        })
    );
}

#[test]
fn damaged_headers_and_digests_fail() {
    let mut wrong_magic = good();
    wrong_magic[0] = b'X';
    assert_eq!(
        verify_sample_bundle(&wrong_magic),
        Err(SampleBundleError::NotABundle)
    );
    assert_eq!(
        verify_sample_bundle(b"DUSAMPLE"),
        Err(SampleBundleError::NotABundle)
    );

    let mut other_format = good();
    other_format[8] = 2;
    assert_eq!(
        verify_sample_bundle(&other_format),
        Err(SampleBundleError::UnsupportedFormat(2))
    );

    let mut too_long = good();
    too_long[10..14].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        verify_sample_bundle(&too_long),
        Err(SampleBundleError::HeaderLength)
    );

    let broken_header = bundle("{not json", b"{}");
    assert_eq!(
        verify_sample_bundle(&broken_header),
        Err(SampleBundleError::HeaderJson)
    );

    let upper = header(&blake3::hash(b"{}").to_hex().to_uppercase());
    assert_eq!(
        verify_sample_bundle(&bundle(&upper, b"{}")),
        Err(SampleBundleError::DigestField)
    );

    let mut tampered = good();
    let last = tampered.len() - 1;
    tampered[last] ^= 1;
    assert_eq!(
        verify_sample_bundle(&tampered),
        Err(SampleBundleError::DigestMismatch)
    );
}

#[test]
fn the_committed_embedded_bundle_verifies() -> Result<(), Box<dyn std::error::Error>> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scan-engine/assets/sample/bundle.v1.bin"
    );
    let record = verify_sample_bundle(&std::fs::read(path)?)?;
    assert_eq!(record.bundle_version, "1.0.0");
    assert_eq!(record.blake3.len(), 64);
    Ok(())
}
