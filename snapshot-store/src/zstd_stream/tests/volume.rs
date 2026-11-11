use super::*;
use crate::records::{
    NameEncoding,
    item::{Item, Kind, Size},
};
use std::sync::atomic::Ordering;
mod q11_measurement;
mod source_metadata_tests;
mod windows_sealed_tests;
fn record(kind: u8, body: &[u8], into: &mut Vec<u8>) {
    into.extend(((body.len() + 1) as u32).to_le_bytes());
    into.push(kind);
    into.extend(body);
}
fn input(
    volume: &[u8],
    item: Option<(u32, Kind, &[u8])>,
    metadata: &str,
) -> Result<VerifiedSpool<MemorySealed>, String> {
    let mut records = Vec::new();
    record(1, volume, &mut records);
    if let Some((parent, kind, name)) = item {
        let body = Item {
            number: 1,
            parent,
            kind,
            flags: 0,
            name,
            sizes: [Size::Unknown; 3],
            modified_ms: None,
            identity: &[],
            link_count: 0,
        }
        .encode(NameEncoding::Bytes)
        .map_err(|e| format!("{e:?}"))?;
        record(2, &body, &mut records);
    }
    record(127, &[], &mut records);
    let compressed = frame(&records, true).map_err(|e| e.to_string())?;
    let mut bytes = b"DUSNAP\r\n".to_vec();
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(0u16.to_le_bytes());
    bytes.extend((metadata.len() as u32).to_le_bytes());
    bytes.extend(metadata.as_bytes());
    bytes.extend(&compressed);
    bytes.extend((compressed.len() as u64).to_le_bytes());
    bytes.extend((if item.is_some() { 3u64 } else { 2 }).to_le_bytes());
    bytes.extend((records.len() as u64).to_le_bytes());
    bytes.extend(blake3::hash(&bytes).as_bytes());
    sealed(&bytes)
}
#[test]
fn sealed_volume_semantics_and_discard() -> Result<(), String> {
    let metadata = r#"{"formatId":"dusnap","formatVersion":1,"snapshotId":"018f0000-0000-7000-8000-000000000001","createdAt":"2026-11-10T00:00:00.000Z","appVersion":"0.1.0","pathEncoding":"bytes","source":{"resultVersionId":"018f0000-0000-7000-8000-000000000002","scopeVersionId":null,"lineageId":null,"observedAt":null,"scopeMarker":null,"conditionSummary":{"state":"unknown"},"completeness":null,"algorithms":{"state":"unknown"},"pathRules":[{"separator":"/","caseSensitivity":null,"normalization":null}],"scopeRoots":["Lw"]},"volume":{"recordKind":1}}"#;
    let volume = [1, 1, 0, 0, 0, 255, 255, 1, 0, 0, 0];
    let mut spool = input(&volume, Some((0, Kind::Folder, b"@")), metadata)?;
    validate_records_in_sealed(&mut spool, Stage::default(), || false)
        .map_err(|e| format!("{e:?}"))?;
    for (v, item, meta) in [
        (
            &volume[..],
            Some((0, Kind::File, b"@".as_slice())),
            metadata,
        ),
        (
            &volume[..],
            Some((0, Kind::Folder, b"wrong".as_slice())),
            metadata,
        ),
        (&volume[..], None, metadata),
        (
            &volume[..],
            Some((0, Kind::Folder, b"@".as_slice())),
            r#"{"formatId":"dusnap","formatVersion":1,"snapshotId":"018f0000-0000-7000-8000-000000000001","createdAt":"2026-11-10T00:00:00.000Z","appVersion":"0.1.0","pathEncoding":"bytes","source":{"resultVersionId":"018f0000-0000-7000-8000-000000000002","scopeVersionId":null,"lineageId":null,"observedAt":null,"scopeMarker":null,"conditionSummary":{"state":"unknown"},"completeness":null,"algorithms":{"state":"unknown"},"pathRules":[{"separator":"/","caseSensitivity":null,"normalization":null}],"scopeRoots":["Lw"]},"volume":{"recordKind":2}}"#,
        ),
        (&volume[..volume.len() - 1], None, metadata),
    ] {
        let mut spool = input(v, item, meta)?;
        let stage = Stage::default();
        let discarded = stage.discarded.clone();
        assert!(validate_records_in_sealed(&mut spool, stage, || false).is_err());
        assert_eq!(discarded.load(Ordering::SeqCst), 1);
    }
    Ok(())
}
#[test]
fn duplicate_volume_descriptor_keys_are_rejected_before_staging() -> Result<(), String> {
    for metadata in [
        r#"{"formatId":"dusnap","formatVersion":1,"snapshotId":"018f0000-0000-7000-8000-000000000001","createdAt":"2026-11-10T00:00:00.000Z","appVersion":"0.1.0","pathEncoding":"bytes","source":{"resultVersionId":"018f0000-0000-7000-8000-000000000002","scopeVersionId":null,"lineageId":null,"observedAt":null,"scopeMarker":null,"conditionSummary":{"state":"unknown"},"completeness":null,"algorithms":{"state":"unknown"},"pathRules":[],"scopeRoots":[]},"volume":{"recordKind":2},"volume":{"recordKind":1}}"#,
        r#"{"formatId":"dusnap","formatVersion":1,"snapshotId":"018f0000-0000-7000-8000-000000000001","createdAt":"2026-11-10T00:00:00.000Z","appVersion":"0.1.0","pathEncoding":"bytes","source":{"resultVersionId":"018f0000-0000-7000-8000-000000000002","scopeVersionId":null,"lineageId":null,"observedAt":null,"scopeMarker":null,"conditionSummary":{"state":"unknown"},"completeness":null,"algorithms":{"state":"unknown"},"pathRules":[],"scopeRoots":[]},"volume":{"recordKind":2,"recordKind":1}}"#,
    ] {
        let mut spool = input(&[1, 0, 0, 0, 0], None, metadata)?;
        let stage = Stage::default();
        let discarded = stage.discarded.clone();
        assert!(matches!(
            validate_records_in_sealed(&mut spool, stage, || false),
            Err(Error::Metadata)
        ));
        assert_eq!(discarded.load(Ordering::SeqCst), 1);
    }
    Ok(())
}

#[test]
fn volume_rejects_duplicate_and_invalid_links_with_valid_envelope_hash() -> Result<(), String> {
    let metadata = r#"{"formatId":"dusnap","formatVersion":1,"snapshotId":"018f0000-0000-7000-8000-000000000001","createdAt":"2026-11-10T00:00:00.000Z","appVersion":"0.1.0","pathEncoding":"bytes","source":{"resultVersionId":"018f0000-0000-7000-8000-000000000002","scopeVersionId":null,"lineageId":null,"observedAt":null,"scopeMarker":null,"conditionSummary":{"state":"unknown"},"completeness":null,"algorithms":{"state":"unknown"},"pathRules":[{"separator":"/","caseSensitivity":null,"normalization":null},{"separator":"/","caseSensitivity":null,"normalization":null}],"scopeRoots":["Lw","L2E"]},"volume":{"recordKind":1}}"#;
    let cases: &[&[u8]] = &[
        &[1, 2, 0, 0, 0, 255, 255, 1, 0, 0, 0, 255, 255, 1, 0, 0, 0], // duplicate item
        &[1, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 0, 0, 0, 0],     // volume out of range
        &[
            1, 2, 0, 2, 0, 255, 255, 0, 0, 0, 0, 255, 255, 0, 0, 0, 0, 1, 0, 65, 0, 0, 0, 0, 1, 0,
            65, 0, 0, 0, 0,
        ], // duplicate ID
        &[1, 2, 0, 0, 0, 255, 255, 0, 0, 0, 0, 255, 255, 0, 0, 0, 0, 0], // trailing byte
        &[
            1, 2, 0, 1, 0, 255, 255, 0, 0, 0, 0, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 3, 0,
        ], // unknown status
    ];
    for volume in cases {
        let mut spool = input(volume, None, metadata)?;
        let stage = Stage::default();
        let discarded = stage.discarded.clone();
        assert!(matches!(
            validate_records_in_sealed(&mut spool, stage, || false),
            Err(Error::Records)
        ));
        assert_eq!(discarded.load(Ordering::SeqCst), 1);
    }
    Ok(())
}
#[test]
fn cancelled_semantic_validation_discards_stage() -> Result<(), String> {
    let meta = r#"{"formatId":"dusnap","formatVersion":1,"snapshotId":"018f0000-0000-7000-8000-000000000001","createdAt":"2026-11-10T00:00:00.000Z","appVersion":"0.1.0","pathEncoding":"bytes","source":{"resultVersionId":"018f0000-0000-7000-8000-000000000002","scopeVersionId":null,"lineageId":null,"observedAt":null,"scopeMarker":null,"conditionSummary":{"state":"unknown"},"completeness":null,"algorithms":{"state":"unknown"},"pathRules":[],"scopeRoots":[]},"volume":{"recordKind":1}}"#;
    let mut spool = input(&[1, 0, 0, 0, 0], None, meta)?;
    let stage = Stage::default();
    let discarded = stage.discarded.clone();
    assert!(matches!(
        validate_records_in_sealed(&mut spool, stage, || true),
        Err(Error::Cancelled)
    ));
    assert_eq!(discarded.load(Ordering::SeqCst), 1);
    Ok(())
}
// Condition summary semantics and catalog extensions remain outside this check.
const SOURCE_EXAMPLE: &str = r#"{"resultVersionId":"018f0000-0000-7000-8000-000000000002","scopeVersionId":null,"lineageId":null,"observedAt":null,"scopeMarker":null,"conditionSummary":{"state":"unknown"},"completeness":null,"algorithms":{"state":"unknown"},"pathRules":[]}"#;
fn example_metadata(roots: &[&str]) -> Result<String, String> {
    let mut source: serde_json::Value =
        serde_json::from_str(SOURCE_EXAMPLE).map_err(|e| e.to_string())?;
    source["scopeRoots"] = serde_json::json!(roots);
    source["pathRules"] = serde_json::json!(
        roots
            .iter()
            .map(|_| serde_json::json!({
                "separator":"/", "caseSensitivity":null, "normalization":null
            }))
            .collect::<Vec<_>>()
    );
    Ok(serde_json::json!({
        "formatId":"dusnap", "formatVersion":1,
        "snapshotId":"018f0000-0000-7000-8000-000000000001",
        "createdAt":"2026-11-10T00:00:00.000Z", "appVersion":"0.1.0",
        "source":source, "volume":{"recordKind":1}, "pathEncoding":"bytes"
    })
    .to_string())
}

#[test]
fn sealed_identity_and_observation_metadata_are_checked_before_staging() -> Result<(), String> {
    let valid: serde_json::Value =
        serde_json::from_str(&example_metadata(&[])?).map_err(|e| e.to_string())?;
    let mut cases = Vec::new();
    for key in [
        "formatId",
        "formatVersion",
        "snapshotId",
        "createdAt",
        "appVersion",
    ] {
        let mut value = valid.clone();
        value.as_object_mut().ok_or("object")?.remove(key);
        cases.push(value);
    }
    for key in [
        "resultVersionId",
        "scopeVersionId",
        "lineageId",
        "observedAt",
    ] {
        let mut value = valid.clone();
        value["source"].as_object_mut().ok_or("source")?.remove(key);
        cases.push(value);
    }
    for (key, bad) in [
        ("resultVersionId", serde_json::json!(null)),
        ("scopeVersionId", serde_json::json!("not-an-id")),
        ("lineageId", serde_json::json!(42)),
        ("observedAt", serde_json::json!("2026-02-30T00:00:00.000Z")),
    ] {
        let mut value = valid.clone();
        value["source"][key] = bad;
        cases.push(value);
    }
    for (key, bad) in [
        ("formatId", serde_json::json!("other")),
        ("formatVersion", serde_json::json!(1.0)),
        (
            "snapshotId",
            serde_json::json!("018F0000-0000-7000-8000-000000000001"),
        ),
        ("createdAt", serde_json::json!("2026-11-10T00:00:00Z")),
        ("appVersion", serde_json::json!("")),
        ("label", serde_json::json!(null)),
    ] {
        let mut value = valid.clone();
        value[key] = bad;
        cases.push(value);
    }
    for metadata in cases {
        let mut spool = input(&[1, 0, 0, 0, 0], None, &metadata.to_string())?;
        let stage = Stage::default();
        let discarded = stage.discarded.clone();
        assert!(matches!(
            validate_records_in_sealed(&mut spool, stage, || false),
            Err(Error::Metadata)
        ));
        assert_eq!(discarded.load(Ordering::SeqCst), 1);
    }
    let mut spool = input(&[1, 0, 0, 0, 0], None, &valid.to_string())?;
    validate_records_in_sealed(&mut spool, Stage::default(), || false)
        .map_err(|e| format!("{e:?}"))?;
    let mut known = valid.clone();
    known["source"]["scopeVersionId"] = serde_json::json!("018f0000-0000-7000-8000-000000000004");
    known["source"]["lineageId"] = serde_json::json!("018f0000-0000-7000-8000-000000000005");
    known["source"]["observedAt"] = serde_json::json!("2026-11-10T00:00:00.000Z");
    known["futureField"] = serde_json::json!({"future": true});
    let mut spool = input(&[1, 0, 0, 0, 0], None, &known.to_string())?;
    validate_records_in_sealed(&mut spool, Stage::default(), || false)
        .map_err(|e| format!("{e:?}"))?;
    let duplicate = valid.to_string().replacen(
        "\"resultVersionId\":",
        "\"resultVersionId\":null,\"resultVersionId\":",
        1,
    );
    let mut spool = input(&[1, 0, 0, 0, 0], None, &duplicate)?;
    assert!(matches!(
        validate_records_in_sealed(&mut spool, Stage::default(), || false),
        Err(Error::Metadata)
    ));
    Ok(())
}
#[test]
fn canonical_vectors_open_as_full_envelopes() -> Result<(), String> {
    let vectors: &[(&[u8], &[&str], u32)] = &[
        (&[1, 0, 0, 0, 0], &[], 0),
        (
            &[
                1, 2, 0, 2, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 0, 0, 0, 1, 0, 65, 0, 0, 0, 0, 1, 0, 66,
                0, 0, 0, 0,
            ],
            &["Lw", "L2E"],
            2,
        ),
        (
            &[1, 1, 0, 1, 0, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            &["Lw"],
            0,
        ),
    ];
    for (volume, roots, item_count) in vectors {
        let metadata = example_metadata(roots)?;
        let mut records = Vec::new();
        record(1, volume, &mut records);
        for number in 1..=*item_count {
            let item = Item {
                number,
                parent: 0,
                kind: Kind::Folder,
                flags: 0,
                name: b"@",
                sizes: [Size::Unknown; 3],
                modified_ms: None,
                identity: &[],
                link_count: 0,
            };
            record(
                2,
                &item
                    .encode(NameEncoding::Bytes)
                    .map_err(|e| format!("{e:?}"))?,
                &mut records,
            );
        }
        let root_count = u16::from_le_bytes([volume[1], volume[2]]);
        if root_count > 0 {
            let gap = crate::records::gap::Gap {
                root_index: 0,
                path: vec![b"missing"],
                reason_key: "scan.denied",
                observed_at_ms: None,
            };
            record(
                3,
                &gap.encode(NameEncoding::Bytes, usize::from(root_count))
                    .map_err(|e| format!("{e:?}"))?,
                &mut records,
            );
        }
        record(127, &[], &mut records);
        let compressed = frame(&records, true).map_err(|e| e.to_string())?;
        let mut bytes = b"DUSNAP\r\n".to_vec();
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(0u16.to_le_bytes());
        bytes.extend((metadata.len() as u32).to_le_bytes());
        bytes.extend(metadata.as_bytes());
        bytes.extend(&compressed);
        bytes.extend((compressed.len() as u64).to_le_bytes());
        bytes.extend((u64::from(*item_count) + 2 + u64::from(root_count > 0)).to_le_bytes());
        bytes.extend((records.len() as u64).to_le_bytes());
        bytes.extend(blake3::hash(&bytes).as_bytes());
        let mut spool = sealed(&bytes)?;
        let (stage, _) = validate_records_in_sealed(&mut spool, Stage::default(), || false)
            .map_err(|e| format!("{e:?}"))?;
        assert_eq!(stage.bytes, records);
    }
    Ok(())
}
fn envelope_for_records(
    metadata: &str,
    records: &[u8],
    count: u64,
) -> Result<VerifiedSpool<MemorySealed>, String> {
    let compressed = frame(records, true).map_err(|e| e.to_string())?;
    let mut bytes = b"DUSNAP\r\n".to_vec();
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(0u16.to_le_bytes());
    bytes.extend((metadata.len() as u32).to_le_bytes());
    bytes.extend(metadata.as_bytes());
    bytes.extend(&compressed);
    bytes.extend((compressed.len() as u64).to_le_bytes());
    bytes.extend(count.to_le_bytes());
    bytes.extend((records.len() as u64).to_le_bytes());
    bytes.extend(blake3::hash(&bytes).as_bytes());
    sealed(&bytes)
}
fn encoded_item(
    number: u32,
    parent: u32,
    name: &[u8],
    encoding: NameEncoding,
) -> Result<Vec<u8>, String> {
    Item {
        number,
        parent,
        kind: Kind::Folder,
        flags: 0,
        name,
        sizes: [Size::Unknown; 3],
        modified_ms: None,
        identity: &[],
        link_count: 0,
    }
    .encode(encoding)
    .map_err(|e| format!("{e:?}"))
}
#[test]
fn rejects_root_bound_to_actual_child_item() -> Result<(), String> {
    let metadata = example_metadata(&["Lw"])?;
    let mut records = Vec::new();
    record(1, &[1, 1, 0, 0, 0, 255, 255, 2, 0, 0, 0], &mut records);
    record(
        2,
        &encoded_item(1, 0, b"root", NameEncoding::Bytes)?,
        &mut records,
    );
    record(
        2,
        &encoded_item(2, 1, b"@", NameEncoding::Bytes)?,
        &mut records,
    );
    record(127, &[], &mut records);
    let mut spool = envelope_for_records(&metadata, &records, 4)?;
    let stage = Stage::default();
    let discarded = stage.discarded.clone();
    assert!(matches!(
        validate_records_in_sealed(&mut spool, stage, || false),
        Err(Error::Records)
    ));
    assert_eq!(discarded.load(Ordering::SeqCst), 1);
    Ok(())
}
#[test]
fn windows_root_placeholder_is_exact_utf16le() -> Result<(), String> {
    let root: Vec<u8> = "C:\\".encode_utf16().flat_map(u16::to_le_bytes).collect();
    let encoded = du_core_contracts::PathBytes::from_bytes(&root)
        .as_str()
        .to_owned();
    let mut metadata: serde_json::Value =
        serde_json::from_str(&example_metadata(&[&encoded])?).map_err(|e| e.to_string())?;
    metadata["pathEncoding"] = "utf16le".into();
    metadata["source"]["pathRules"][0]["separator"] = "\\".into();
    for (name, accepted) in [(&[b'@', 0][..], true), (&[b'x', 0][..], false)] {
        let mut records = Vec::new();
        record(1, &[1, 1, 0, 0, 0, 255, 255, 1, 0, 0, 0], &mut records);
        record(
            2,
            &encoded_item(1, 0, name, NameEncoding::WindowsUtf16Le)?,
            &mut records,
        );
        record(127, &[], &mut records);
        let mut spool = envelope_for_records(&metadata.to_string(), &records, 3)?;
        let stage = Stage::default();
        let discarded = stage.discarded.clone();
        let result = validate_records_in_sealed(&mut spool, stage, || false);
        assert_eq!(result.is_ok(), accepted);
        assert_eq!(discarded.load(Ordering::SeqCst), usize::from(!accepted));
    }
    Ok(())
}
