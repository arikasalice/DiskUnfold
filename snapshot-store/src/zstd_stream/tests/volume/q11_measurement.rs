//! Reproducible measurements, not a production writer or space reservation rule.
use super::*;
use crate::records::volume::{Observation, RootLink, Volume, VolumeEntry};

fn item(number: u32, parent: u32, name: &[u8], encoding: NameEncoding) -> Result<Vec<u8>, String> {
    Item {
        number,
        parent,
        kind: if parent == 0 {
            Kind::Folder
        } else {
            Kind::File
        },
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

fn measure(
    label: &str,
    count: u32,
    name_len: usize,
    random: bool,
    unicode: bool,
    windows: bool,
) -> Result<(), String> {
    let encoding = if windows {
        NameEncoding::WindowsUtf16Le
    } else {
        NameEncoding::Bytes
    };
    let metadata = if windows {
        let roots: Vec<String> = ["C:\\", "D:\\"]
            .iter()
            .map(|root| {
                let bytes: Vec<u8> = root.encode_utf16().flat_map(u16::to_le_bytes).collect();
                du_core_contracts::PathBytes::from_bytes(&bytes)
                    .as_str()
                    .to_owned()
            })
            .collect();
        let mut value: serde_json::Value =
            serde_json::from_str(&example_metadata(&[&roots[0], &roots[1]])?)
                .map_err(|e| e.to_string())?;
        value["pathEncoding"] = "utf16le".into();
        for rule in value["source"]["pathRules"]
            .as_array_mut()
            .ok_or("path rules")?
        {
            rule["separator"] = "\\".into();
        }
        value.to_string()
    } else {
        example_metadata(&["Lw", "L2E"])?
    };
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
                id: b"volume-a",
                basis: "native",
                start: Observation::NotAttempted,
                end: Observation::NotAttempted,
            },
            VolumeEntry {
                id: b"volume-b",
                basis: "native",
                start: Observation::NotAttempted,
                end: Observation::NotAttempted,
            },
        ],
    }
    .encode()
    .map_err(|e| format!("{e:?}"))?;
    let mut records = Vec::new();
    record(1, &volume, &mut records);
    let root: &[u8] = if windows { b"@\0" } else { b"@" };
    record(2, &item(1, 0, root, encoding)?, &mut records);
    record(2, &item(2, 0, root, encoding)?, &mut records);
    let mut seed = 0x5eed_1234_5678_9abcu64;
    for n in 0..count {
        let name = if unicode {
            "한".repeat(name_len / 3).into_bytes()
        } else if random {
            (0..name_len)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
                        [(seed % 62) as usize]
                })
                .collect()
        } else {
            vec![b'a'; name_len]
        };
        let name = if windows {
            String::from_utf8(name)
                .map_err(|e| e.to_string())?
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect()
        } else {
            name
        };
        record(2, &item(n + 3, 1 + n % 2, &name, encoding)?, &mut records);
    }
    record(127, &[], &mut records);
    let compressed = frame(&records, true).map_err(|e| e.to_string())?;
    let mut envelope = b"DUSNAP\r\n".to_vec();
    envelope.extend(1u16.to_le_bytes());
    envelope.extend(0u16.to_le_bytes());
    envelope.extend((metadata.len() as u32).to_le_bytes());
    envelope.extend(metadata.as_bytes());
    envelope.extend(&compressed);
    envelope.extend((compressed.len() as u64).to_le_bytes());
    envelope.extend((u64::from(count) + 4).to_le_bytes());
    envelope.extend((records.len() as u64).to_le_bytes());
    envelope.extend(blake3::hash(&envelope).as_bytes());
    assert_eq!(envelope.len(), 72 + metadata.len() + compressed.len());
    let mut spool = sealed(&envelope)?;
    let (stage, _) = validate_records_in_sealed(&mut spool, Stage::default(), || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(stage.bytes, records);
    let last = envelope.len() - 1;
    envelope[last] ^= 1;
    assert!(sealed(&envelope).is_err());
    println!(
        "{label},items={},metadata={},records={},compressed={},sealed={}",
        count + 2,
        metadata.len(),
        records.len(),
        compressed.len(),
        envelope.len()
    );
    Ok(())
}

#[test]
fn q11_fixture_measurements() -> Result<(), String> {
    measure("small", 0, 0, false, false, false)?;
    measure("large_repeated", 10_000, 64, false, false, false)?;
    measure("large_random_names", 10_000, 64, true, false, false)?;
    measure("long_utf8", 100, 1023, false, true, false)?;
    measure("windows_utf16_random", 10_000, 64, true, false, true)?;
    assert!(item(3, 1, b"bad/name", NameEncoding::Bytes).is_err());
    assert!(item(3, 1, &vec![b'a'; 1024], NameEncoding::Bytes).is_ok());
    assert!(item(3, 1, &vec![b'a'; 1025], NameEncoding::Bytes).is_err());
    Ok(())
}
