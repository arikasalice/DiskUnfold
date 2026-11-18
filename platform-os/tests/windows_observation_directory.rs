#![cfg(windows)]
use du_core_contracts::scan_observation::{
    AnchoredDirectory, DirectoryObservations, FilterDecision, FilteredBatch, ObservedKind,
};
use du_platform_os::windows_observation::{open_verified, open_verified_stable};
use std::{fs, os::windows::fs::symlink_dir};

fn name(value: &str) -> Vec<u8> {
    value.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn link_creation_is_unavailable(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::Unsupported
    ) || error.raw_os_error() == Some(1314)
}

fn entries(
    directory: &mut dyn DirectoryObservations,
) -> Result<Vec<du_core_contracts::scan_observation::ObservedEntry>, String> {
    let mut result = Vec::new();
    loop {
        let batch = directory.next_batch().map_err(|e| format!("{e:?}"))?;
        if batch.is_empty() {
            return Ok(result);
        }
        result.extend(batch);
    }
}

#[test]
fn opens_ntfs_fixture_and_reads_bounded_batches() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = tempfile::tempdir()?;
    fs::create_dir(fixture.path().join("folder"))?;
    for n in 0..300 {
        fs::write(fixture.path().join(format!("file{n}")), b"abc")?;
    }
    let mut directory = open_verified(fixture.path()).map_err(|e| format!("{e:?}"))?;
    let review = directory.identity().map_err(|e| format!("{e:?}"))?;
    assert_ne!(review.file_id, 0);
    assert!(!review.physical_path.is_empty());
    let mut batches = Vec::new();
    loop {
        let batch = directory.next_batch().map_err(|e| format!("{e:?}"))?;
        if batch.is_empty() {
            break;
        }
        assert!(batch.len() <= 256);
        batches.push(batch);
    }
    assert!(
        batches.len() > 1,
        "301 entries require multiple nonempty batches"
    );
    let all: Vec<_> = batches.into_iter().flatten().collect();
    assert_eq!(all.len(), 301);
    let expected: std::collections::BTreeSet<_> = (0..300)
        .map(|n| name(&format!("file{n}")))
        .chain([name("folder")])
        .collect();
    let observed: std::collections::BTreeSet<_> =
        all.iter().map(|entry| entry.name.clone()).collect();
    assert_eq!(observed, expected);
    let file = all
        .iter()
        .find(|e| e.name == name("file0"))
        .ok_or("missing file")?;
    assert_eq!(
        file.metadata.as_ref().map(|m| (&m.kind, &m.size)),
        Ok((&ObservedKind::File, &Ok(3)))
    );
    let child = all
        .iter()
        .find(|e| e.name == name("folder"))
        .ok_or("missing folder")?;
    let mut empty = directory.open_child(child).map_err(|e| format!("{e:?}"))?;
    assert!(empty.next_batch().map_err(|e| format!("{e:?}"))?.is_empty());
    assert!(empty.next_batch().map_err(|e| format!("{e:?}"))?.is_empty());
    let mut wrong_volume = child.clone();
    if let Ok(meta) = &mut wrong_volume.metadata {
        meta.device ^= 1;
    }
    assert!(directory.open_child(&wrong_volume).is_err());
    assert!(open_verified(&fixture.path().join("not-present")).is_err());
    Ok(())
}

#[test]
fn stable_handle_reads_an_empty_scope() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = tempfile::tempdir()?;
    let mut directory = open_verified_stable(fixture.path()).map_err(|e| format!("{e:?}"))?;
    assert!(
        directory
            .next_batch()
            .map_err(|e| format!("{e:?}"))?
            .is_empty()
    );
    assert!(
        directory
            .next_batch()
            .map_err(|e| format!("{e:?}"))?
            .is_empty()
    );
    Ok(())
}

#[test]
fn filters_names_from_a_real_directory() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = tempfile::tempdir()?;
    for value in ["keep-a", "drop-a", "keep-b", "drop-b"] {
        fs::write(fixture.path().join(value), b"fixture")?;
    }
    let keep_prefix = name("keep-");
    let mut directory = open_verified(fixture.path()).map_err(|e| format!("{e:?}"))?;
    let mut include = |entry: &[u8]| {
        if entry.starts_with(&keep_prefix) {
            FilterDecision::Admit
        } else {
            FilterDecision::Exclude
        }
    };
    let mut kept = Vec::new();
    loop {
        match directory
            .next_filtered(&mut include)
            .map_err(|error| format!("{error:?}"))?
        {
            FilteredBatch::Entries(entries) => {
                kept.extend(entries.into_iter().map(|entry| entry.name));
            }
            FilteredBatch::Progress => {}
            FilteredBatch::End => break,
            FilteredBatch::Undecidable => return Err("fixture filter became undecidable".into()),
        }
    }
    kept.sort();
    assert_eq!(kept, [name("keep-a"), name("keep-b")]);
    Ok(())
}

#[test]
fn reports_a_link_without_opening_its_target() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = tempfile::tempdir()?;
    let scope = fixture.path().join("scope");
    let target = fixture.path().join("target");
    fs::create_dir(&scope)?;
    fs::create_dir(&target)?;
    fs::write(target.join("target-only"), vec![7u8; 4096])?;
    let link_path = scope.join("to-target");
    if let Err(error) = symlink_dir(&target, &link_path) {
        if link_creation_is_unavailable(&error) {
            eprintln!("skipped Windows symbolic-link case after creation failed: {error}");
            return Ok(());
        }
        return Err(error.into());
    }

    let mut root = open_verified(&scope).map_err(|e| format!("{e:?}"))?;
    let entries = entries(&mut root)?;
    assert_eq!(entries.len(), 1);
    let link = entries.first().ok_or("missing link")?;
    assert_eq!(link.name, name("to-target"));
    assert_eq!(link.enumerated_kind, ObservedKind::Link);
    let observed = link.metadata.as_ref().map_err(|e| format!("{e:?}"))?;
    assert_eq!(observed.kind, ObservedKind::Link);
    // `observed.size` is the adapter's Windows reparse-handle observation.
    // Do not infer target traversal from zero or equate it with symlink_metadata.
    assert!(root.open_child(link).is_err());
    assert_eq!(fs::read(target.join("target-only"))?.len(), 4096);
    Ok(())
}
