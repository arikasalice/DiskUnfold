//! Reproducible measurements, not a production writer or space reservation rule.
use super::*;
use crate::records::volume::{Observation, RootLink, Volume, VolumeEntry};
use std::time::Instant;

struct CountSink {
    bytes: u64,
    limit: u64,
}
impl Write for CountSink {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(data.len() as u64)
            .filter(|n| *n <= self.limit)
            .ok_or_else(|| io::Error::other("count limit"))?;
        Ok(data.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn scheduled<W: Write>(data: &[u8], output: W, flush: bool) -> io::Result<W> {
    let mut encoder = zstd::stream::write::Encoder::new(output, 1)?;
    encoder.include_checksum(true)?;
    for (index, chunk) in data.chunks(8192).enumerate() {
        encoder.write_all(chunk)?;
        if flush && index % 4 == 3 {
            encoder.flush()?;
        }
    }
    encoder.finish()
}

fn compare_preflight(label: &str, records: &[u8], flush: bool) -> Result<(), String> {
    let start = Instant::now();
    let count = scheduled(
        records,
        CountSink {
            bytes: 0,
            limit: u64::MAX,
        },
        flush,
    )
    .map_err(|e| e.to_string())?;
    let count_us = start.elapsed().as_micros();
    let start = Instant::now();
    let written = scheduled(records, Vec::new(), flush).map_err(|e| e.to_string())?;
    let write_us = start.elapsed().as_micros();
    assert_eq!(count.bytes, written.len() as u64);
    let repeated = scheduled(records, Vec::new(), flush).map_err(|e| e.to_string())?;
    assert_eq!(written, repeated);
    println!(
        "preflight,{label},flush={flush},bytes={},count_us={count_us},vec_us={write_us}",
        count.bytes
    );
    Ok(())
}

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

enum FixtureNames {
    RepeatedAscii(usize),
    SeededAscii(usize),
    RepeatedKorean(usize),
    UniqueSiblings,
}

fn measure(
    label: &str,
    count: u32,
    names: FixtureNames,
    encoding: NameEncoding,
) -> Result<(), String> {
    let windows = encoding == NameEncoding::WindowsUtf16Le;
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
        let name = match names {
            FixtureNames::RepeatedAscii(bytes) => vec![b'a'; bytes],
            FixtureNames::SeededAscii(bytes) => (0..bytes)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
                        [(seed % 62) as usize]
                })
                .collect(),
            FixtureNames::RepeatedKorean(characters) => "한".repeat(characters).into_bytes(),
            FixtureNames::UniqueSiblings => format!("common-prefix-{n:08}").into_bytes(),
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
    compare_preflight(label, &records, false)?;
    compare_preflight(label, &records, true)?;
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
    measure(
        "small",
        0,
        FixtureNames::RepeatedAscii(0),
        NameEncoding::Bytes,
    )?;
    measure(
        "large_repeated",
        10_000,
        FixtureNames::RepeatedAscii(64),
        NameEncoding::Bytes,
    )?;
    measure(
        "large_random_names",
        10_000,
        FixtureNames::SeededAscii(64),
        NameEncoding::Bytes,
    )?;
    measure(
        "long_utf8",
        100,
        FixtureNames::RepeatedKorean(341),
        NameEncoding::Bytes,
    )?;
    measure(
        "windows_utf16_random",
        10_000,
        FixtureNames::SeededAscii(64),
        NameEncoding::WindowsUtf16Le,
    )?;
    measure(
        "unique_siblings",
        10_000,
        FixtureNames::UniqueSiblings,
        NameEncoding::Bytes,
    )?;
    assert!(item(3, 1, b"bad/name", NameEncoding::Bytes).is_err());
    assert!(item(3, 1, &vec![b'a'; 1024], NameEncoding::Bytes).is_ok());
    assert!(item(3, 1, &vec![b'a'; 1025], NameEncoding::Bytes).is_err());
    assert!(
        scheduled(
            b"failure injection",
            CountSink { bytes: 0, limit: 0 },
            false
        )
        .is_err()
    );
    Ok(())
}

#[test]
#[ignore = "large synthetic measurement; run explicitly"]
fn q11_million_measurement() -> Result<(), String> {
    measure(
        "unique_siblings_million",
        1_000_000,
        FixtureNames::UniqueSiblings,
        NameEncoding::Bytes,
    )
}
