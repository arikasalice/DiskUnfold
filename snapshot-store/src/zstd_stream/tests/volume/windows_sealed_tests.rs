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
        {"separator":"\\","caseSensitivity":null,"normalization":null},
        {"separator":"\\","caseSensitivity":"sensitive","normalization":"NFC"}
    ]);
    let mut records = Vec::new();
    let volume = crate::records::volume::Volume {
        roots: vec![
            crate::records::volume::RootLink {
                volume_index: Some(0),
                item_number: Some(1),
            },
            crate::records::volume::RootLink {
                volume_index: None,
                item_number: Some(2),
            },
        ],
        volumes: vec![crate::records::volume::VolumeEntry {
            id: b"windows-volume-0",
            basis: "native",
            start: crate::records::volume::Observation::NotAttempted,
            end: crate::records::volume::Observation::NotAttempted,
        }],
    };
    let volume_body = volume.encode().map_err(|e| format!("{e:?}"))?;
    record(1, &volume_body, &mut records);
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

fn expected_windows_roots() -> Vec<Vec<u8>> {
    ["C:\\", "D:\\folder"]
        .iter()
        .map(|path| path.encode_utf16().flat_map(u16::to_le_bytes).collect())
        .collect()
}

fn assert_windows_path_rules(roots: &crate::zstd_stream::ScopeRoots) -> Result<(), String> {
    let path_rules = roots.path_rules().ok_or("validated path rules missing")?;
    assert_eq!(path_rules.len(), 2);
    assert_eq!(
        path_rules[0].separator(),
        crate::zstd_stream::PathSeparator::Backslash
    );
    assert_eq!(path_rules[0].case_sensitivity(), None);
    assert_eq!(path_rules[0].normalization(), None);
    assert_eq!(
        path_rules[1].case_sensitivity(),
        Some(crate::zstd_stream::PathCaseSensitivity::Sensitive)
    );
    assert_eq!(
        path_rules[1].normalization(),
        Some(crate::zstd_stream::PathNormalization::Nfc)
    );
    assert_eq!(
        path_rules[0].original(),
        &serde_json::json!({
            "separator":"\\",
            "caseSensitivity":null,
            "normalization":null
        })
    );
    Ok(())
}

fn assert_windows_volume_membership(roots: &crate::zstd_stream::ScopeRoots) -> Result<(), String> {
    let volume = roots
        .volume_record()
        .ok_or("validated Volume record missing")?
        .decode()
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        volume.roots,
        vec![
            crate::records::volume::RootLink {
                volume_index: Some(0),
                item_number: Some(1),
            },
            crate::records::volume::RootLink {
                volume_index: None,
                item_number: Some(2),
            },
        ]
    );
    assert_eq!(volume.volumes.len(), 1);
    assert_eq!(volume.volumes[0].id, b"windows-volume-0");
    Ok(())
}

#[test]
fn windows_multi_root_metadata_and_volume_links_are_preserved() -> Result<(), String> {
    let (metadata, records) = fixture()?;
    let mut spool = envelope_for_records(&metadata, &records, 4)?;
    let (staged, roots) = validate_records_in_sealed(&mut spool, Stage::default(), || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(staged.bytes, records);
    assert_eq!(roots.roots(), expected_windows_roots());
    assert_windows_path_rules(&roots)?;
    assert_windows_volume_membership(&roots)?;
    Ok(())
}

#[test]
fn windows_invalid_path_rule_discards_staging() -> Result<(), String> {
    let (metadata, records) = fixture()?;
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
    Ok(())
}

#[test]
fn windows_staging_write_failure_discards_partial_output() -> Result<(), String> {
    let (metadata, records) = fixture()?;
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
    Ok(())
}

#[test]
fn windows_missing_path_rules_discards_staging() -> Result<(), String> {
    let (metadata, records) = fixture()?;
    let mut metadata: serde_json::Value =
        serde_json::from_str(&metadata).map_err(|e| e.to_string())?;
    metadata
        .get_mut("source")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or("source")?
        .remove("pathRules");
    let mut spool = envelope_for_records(&metadata.to_string(), &records, 4)?;
    let stage = Stage::default();
    let discarded = stage.discarded.clone();
    assert!(matches!(
        validate_records_in_sealed(&mut spool, stage, || false),
        Err(Error::Metadata)
    ));
    assert_eq!(discarded.load(Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn windows_metadata_outputs_are_owned_from_the_same_sealed_copy() -> Result<(), String> {
    let (mut metadata, records) = fixture()?;
    let mut spool = envelope_for_records(&metadata, &records, 4)?;
    metadata.clear();
    let (staged, roots) = validate_records_in_sealed(&mut spool, Stage::default(), || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(staged.bytes, records);
    let expected_roots: Vec<Vec<u8>> = ["C:\\", "D:\\folder"]
        .iter()
        .map(|path| path.encode_utf16().flat_map(u16::to_le_bytes).collect())
        .collect();
    assert_eq!(roots.roots(), expected_roots);
    assert_eq!(roots.path_rules().map(|rules| rules.len()), Some(2));
    let volume = roots
        .volume_record()
        .ok_or("validated Volume record missing")?
        .decode()
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(volume.volumes[0].id, b"windows-volume-0");
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

// This reads a synthetic envelope from a real Windows temporary file. It does
// not exercise the unimplemented product writer or catalog database.
#[cfg(windows)]
#[test]
fn windows_file_multivolume_sealed_read_and_index_digest() -> Result<(), String> {
    use crate::envelope::spool::{Error as CaptureError, capture_v1};
    use crate::records::volume::{Observation, RootLink, Volume, VolumeEntry};
    use std::fs::{File, OpenOptions, remove_file};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct FixtureFile(std::path::PathBuf);
    impl Drop for FixtureFile {
        fn drop(&mut self) {
            let _ = remove_file(&self.0);
        }
    }
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "diskunfold-f07-{}-{unique}.dusnap",
        std::process::id()
    ));
    let (metadata, _) = fixture()?;
    let volumes = Volume {
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
                id: b"first",
                basis: "native",
                start: Observation::NotAttempted,
                end: Observation::NotAttempted,
            },
            VolumeEntry {
                id: b"second",
                basis: "native",
                start: Observation::NotAttempted,
                end: Observation::NotAttempted,
            },
        ],
    };
    let body = volumes.encode().map_err(|e| format!("{e:?}"))?;
    let mut records = Vec::new();
    record(1, &body, &mut records);
    for n in 1..=2 {
        record(
            2,
            &encoded_item(n, 0, &[b'@', 0], NameEncoding::WindowsUtf16Le)?,
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
    bytes.extend(4u64.to_le_bytes());
    bytes.extend((records.len() as u64).to_le_bytes());
    let digest = *blake3::hash(&bytes).as_bytes();
    bytes.extend(digest);
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    let path = FixtureFile(path);
    output.write_all(&bytes).map_err(|e| e.to_string())?;
    output.sync_all().map_err(|e| e.to_string())?;
    drop(output);
    let mut input = File::open(&path.0).map_err(|e| e.to_string())?;
    let mut spool = capture_v1(
        &mut input,
        MemoryCapture(Vec::new()),
        Some(digest),
        |_, _| true,
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let (staged, roots) = validate_records_in_sealed(&mut spool, Stage::default(), || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(staged.bytes, records);
    assert_eq!(roots.roots().len(), 2);
    assert_eq!(
        crate::records::volume::decode(&staged.bytes[5..5 + body.len()], 2),
        Ok(volumes)
    );

    drop(input);
    let mut input = File::open(&path.0).map_err(|e| e.to_string())?;
    assert!(matches!(
        capture_v1(
            &mut input,
            MemoryCapture(Vec::new()),
            Some([0; 32]),
            |_, _| true,
            || false
        ),
        Err(CaptureError::IndexIntegrity)
    ));
    drop(input);
    assert_eq!(std::fs::read(&path.0).map_err(|e| e.to_string())?, bytes);
    remove_file(&path.0).map_err(|e| e.to_string())?;
    Ok(())
}
