use super::*;
use crate::records::{
    NameEncoding,
    item::{Item, Kind, Size},
};
use std::sync::atomic::Ordering;
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
    let metadata =
        r#"{"pathEncoding":"bytes","source":{"scopeRoots":["Lw"]},"volume":{"recordKind":1}}"#;
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
            r#"{"pathEncoding":"bytes","source":{"scopeRoots":["Lw"]},"volume":{"recordKind":2}}"#,
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
fn volume_rejects_duplicate_and_invalid_links_with_valid_envelope_hash() -> Result<(), String> {
    let metadata = r#"{"pathEncoding":"bytes","source":{"scopeRoots":["Lw","L2E"]},"volume":{"recordKind":1}}"#;
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
    let meta = r#"{"pathEncoding":"bytes","source":{"scopeRoots":[]},"volume":{"recordKind":1}}"#;
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
#[test]
fn canonical_vectors_open_as_full_envelopes() -> Result<(), String> {
    let vectors: &[(&[u8], &str, u32)] = &[
        (
            &[1, 0, 0, 0, 0],
            r#"{"pathEncoding":"bytes","source":{"scopeRoots":[]},"volume":{"recordKind":1}}"#,
            0,
        ),
        (
            &[
                1, 2, 0, 2, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 0, 0, 0, 1, 0, 65, 0, 0, 0, 0, 1, 0, 66,
                0, 0, 0, 0,
            ],
            r#"{"pathEncoding":"bytes","source":{"scopeRoots":["Lw","L2E"]},"volume":{"recordKind":1}}"#,
            2,
        ),
        (
            &[1, 1, 0, 1, 0, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            r#"{"pathEncoding":"bytes","source":{"scopeRoots":["Lw"]},"volume":{"recordKind":1}}"#,
            0,
        ),
    ];
    for (volume, metadata, item_count) in vectors {
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
