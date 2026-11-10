//! Synthetic Windows multi-root sealed staging and cancellation checks.
use super::*;

fn fixture() -> Result<(String, Vec<u8>), String> {
    let roots: Vec<Vec<u8>> = ["C:\\", "D:\\folder"]
        .iter()
        .map(|s| s.encode_utf16().flat_map(u16::to_le_bytes).collect())
        .collect();
    let encoded: Vec<String> = roots
        .iter()
        .map(|r| {
            du_core_contracts::PathBytes::from_bytes(r)
                .as_str()
                .to_owned()
        })
        .collect();
    let mut metadata: serde_json::Value =
        serde_json::from_str(&example_metadata(&[&encoded[0], &encoded[1]])?)
            .map_err(|e| e.to_string())?;
    metadata["pathEncoding"] = "utf16le".into();
    metadata["source"]["pathRules"] = serde_json::json!([
        {"separator":"\\","caseSensitivity":"insensitive","normalization":null},
        {"separator":"\\","caseSensitivity":"sensitive","normalization":"NFC"}
    ]);
    let mut records = Vec::new();
    record(
        1,
        &[1, 2, 0, 0, 0, 255, 255, 1, 0, 0, 0, 255, 255, 2, 0, 0, 0],
        &mut records,
    );
    for n in 1..=2 {
        record(
            2,
            &encoded_item(n, 0, &[b'@', 0], NameEncoding::WindowsUtf16Le)?,
            &mut records,
        );
    }
    record(127, &[], &mut records);
    Ok((metadata.to_string(), records))
}

#[test]
fn windows_multi_root_rules_and_staging_failures() -> Result<(), String> {
    let (metadata, records) = fixture()?;
    let mut spool = envelope_for_records(&metadata, &records, 4)?;
    let (previous, roots) = validate_records_in_sealed(&mut spool, Stage::default(), || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(previous.bytes, records);
    assert_eq!(roots.roots().len(), 2);
    let mut invalid: serde_json::Value =
        serde_json::from_str(&metadata).map_err(|e| e.to_string())?;
    invalid["source"]["pathRules"][1]["separator"] = "/".into();
    let mut spool = envelope_for_records(&invalid.to_string(), &records, 4)?;
    let stage = Stage::default();
    let discarded = stage.discarded.clone();
    assert!(matches!(
        validate_records_in_sealed(&mut spool, stage, || false),
        Err(Error::Metadata)
    ));
    assert_eq!(discarded.load(Ordering::SeqCst), 1);
    assert_eq!(previous.bytes, records);
    let mut spool = envelope_for_records(&metadata, &records, 4)?;
    let stage = Stage {
        fail_write: true,
        ..Stage::default()
    };
    let discarded = stage.discarded.clone();
    assert!(matches!(
        validate_records_in_sealed(&mut spool, stage, || false),
        Err(Error::Sink(_))
    ));
    assert_eq!(discarded.load(Ordering::SeqCst), 1);
    assert_eq!(previous.bytes, records);
    Ok(())
}

#[test]
fn cancellation_at_final_staging_boundary_discards_result() -> Result<(), String> {
    let (metadata, records) = fixture()?;
    let mut spool = envelope_for_records(&metadata, &records, 4)?;
    let mut checks = 0;
    validate_records_in_sealed(&mut spool, Stage::default(), || {
        checks += 1;
        false
    })
    .map_err(|e| format!("{e:?}"))?;
    assert!(checks >= 5, "final cancellation check was not reached");
    let mut spool = envelope_for_records(&metadata, &records, 4)?;
    let stage = Stage::default();
    let discarded = stage.discarded.clone();
    let mut current = 0;
    assert!(matches!(
        validate_records_in_sealed(&mut spool, stage, || {
            current += 1;
            current == checks
        }),
        Err(Error::Cancelled)
    ));
    assert_eq!(discarded.load(Ordering::SeqCst), 1);
    Ok(())
}
