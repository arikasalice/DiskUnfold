use super::*;

#[test]
fn sealed_multivolume_preserves_unlinked_roots_and_rejects_nonfolder_parents() -> Result<(), String>
{
    use crate::records::volume::{Observation, RootLink, Volume, VolumeEntry};
    let metadata = example_metadata(&["Lw", "L2E"])?;
    let volume = Volume {
        roots: vec![
            RootLink {
                volume_index: Some(0),
                item_number: Some(1),
            },
            RootLink {
                volume_index: Some(1),
                item_number: Some(2),
            },
        ],
        volumes: vec![
            VolumeEntry {
                id: b"a",
                basis: "native",
                start: Observation::NotAttempted,
                end: Observation::NotAttempted,
            },
            VolumeEntry {
                id: b"b",
                basis: "native",
                start: Observation::NotAttempted,
                end: Observation::NotAttempted,
            },
        ],
    }
    .encode()
    .map_err(|e| format!("{e:?}"))?;
    let make = |number, parent, kind, name: &[u8]| {
        Item {
            number,
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
        .map_err(|e| format!("{e:?}"))
    };
    let roots = [
        make(1, 0, Kind::Folder, b"@")?,
        make(2, 0, Kind::Folder, b"@")?,
    ];
    for (third, fourth, accepted) in [
        (
            make(3, 1, Kind::Folder, b"folder")?,
            make(4, 3, Kind::File, b"child")?,
            true,
        ),
        (
            make(3, 0, Kind::Folder, b"@")?,
            make(4, 3, Kind::File, b"child")?,
            true,
        ),
        (
            make(3, 1, Kind::File, b"file")?,
            make(4, 3, Kind::File, b"child")?,
            false,
        ),
        (
            make(3, 2, Kind::Link, b"link")?,
            make(4, 3, Kind::Folder, b"child")?,
            false,
        ),
    ] {
        let mut records = Vec::new();
        record(1, &volume, &mut records);
        for item in roots.iter().chain([&third, &fourth]) {
            record(2, item, &mut records);
        }
        record(127, &[], &mut records);
        let mut spool = envelope_for_records(&metadata, &records, 6)?;
        let stage = Stage::default();
        let discarded = stage.discarded.clone();
        let result = validate_records_in_sealed(&mut spool, stage, || false);
        assert_eq!(result.is_ok(), accepted);
        assert_eq!(discarded.load(Ordering::SeqCst), usize::from(!accepted));
    }
    Ok(())
}

#[test]
fn unknown_or_unrecorded_roots_remain_observational() -> Result<(), String> {
    use crate::records::volume::{RootLink, Volume};
    let cases = [
        (
            example_metadata(&[])?,
            Volume {
                roots: vec![],
                volumes: vec![],
            },
        ),
        (
            example_metadata(&["Lw"])?,
            Volume {
                roots: vec![RootLink {
                    volume_index: None,
                    item_number: None,
                }],
                volumes: vec![],
            },
        ),
        (
            example_metadata(&["Lw", "L2E"])?,
            Volume {
                roots: vec![
                    RootLink {
                        volume_index: None,
                        item_number: Some(1),
                    },
                    RootLink {
                        volume_index: None,
                        item_number: None,
                    },
                ],
                volumes: vec![],
            },
        ),
    ];
    for (metadata, volume) in cases {
        let encoded = volume.encode().map_err(|e| format!("{e:?}"))?;
        let mut records = Vec::new();
        record(1, &encoded, &mut records);
        record(
            2,
            &encoded_item(1, 0, b"@", NameEncoding::Bytes)?,
            &mut records,
        );
        record(
            2,
            &encoded_item(2, 0, b"observed", NameEncoding::Bytes)?,
            &mut records,
        );
        record(127, &[], &mut records);
        let mut spool = envelope_for_records(&metadata, &records, 4)?;
        let (stage, roots) = validate_records_in_sealed(&mut spool, Stage::default(), || false)
            .map_err(|e| format!("{e:?}"))?;
        assert_eq!(stage.bytes, records);
        assert_eq!(roots.roots().len(), volume.roots.len());
        assert_eq!(
            crate::records::volume::decode(&stage.bytes[5..5 + encoded.len()], roots.roots().len())
                .map_err(|e| format!("{e:?}"))?,
            volume
        );
    }
    Ok(())
}

#[test]
fn parent_kind_bitmap_checks_across_byte_boundaries() -> Result<(), String> {
    let metadata = example_metadata(&[])?;
    for (parent_kind, accepted) in [(Kind::Folder, true), (Kind::File, false)] {
        let mut records = Vec::new();
        record(1, &[1, 0, 0, 0, 0], &mut records);
        for number in 1..=10 {
            let item = Item {
                number,
                parent: 0,
                kind: if number == 9 {
                    parent_kind
                } else {
                    Kind::Folder
                },
                flags: 0,
                name: b"x",
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
        record(
            2,
            &encoded_item(11, 9, b"child", NameEncoding::Bytes)?,
            &mut records,
        );
        record(127, &[], &mut records);
        let mut spool = envelope_for_records(&metadata, &records, 13)?;
        let stage = Stage::default();
        let discarded = stage.discarded.clone();
        let result = validate_records_in_sealed(&mut spool, stage, || false);
        assert_eq!(result.is_ok(), accepted);
        assert_eq!(discarded.load(Ordering::SeqCst), usize::from(!accepted));
    }
    Ok(())
}
