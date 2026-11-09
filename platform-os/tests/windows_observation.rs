#![cfg(windows)]
use du_core_contracts::scan_observation::{
    AnchoredDirectory, DirectoryObservations, ObservedKind, ScopeRootPort,
};
use du_platform_os::windows_observation::{WindowsScopeRoots, open_verified};
use std::{
    fs,
    os::windows::{ffi::OsStrExt, fs::symlink_dir},
    path::Path,
};

fn name(value: &str) -> Vec<u8> {
    value.encode_utf16().flat_map(u16::to_le_bytes).collect()
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
    let first = directory.next_batch().map_err(|e| format!("{e:?}"))?;
    assert!(first.len() <= 256);
    let mut all = first;
    all.extend(entries(&mut directory)?);
    assert_eq!(all.len(), 301);
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
    assert!(directory.open_child(child).is_ok());
    let mut wrong_volume = child.clone();
    if let Ok(meta) = &mut wrong_volume.metadata {
        meta.device ^= 1;
    }
    assert!(directory.open_child(&wrong_volume).is_err());
    assert!(open_verified(&fixture.path().join("not-present")).is_err());
    Ok(())
}

#[test]
fn replacement_or_link_is_never_opened_as_a_child() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = tempfile::tempdir()?;
    let parent = fixture.path().join("parent");
    let target = fixture.path().join("target");
    fs::create_dir(&parent)?;
    fs::create_dir(&target)?;
    fs::create_dir(parent.join("child"))?;
    fs::write(parent.join("child").join("original"), b"old")?;
    let mut directory = open_verified(&parent).map_err(|e| format!("{e:?}"))?;
    let seen = entries(&mut directory)?
        .into_iter()
        .find(|e| e.name == name("child"))
        .ok_or("missing child")?;
    let mut pinned = directory.open_child(&seen).map_err(|e| format!("{e:?}"))?;
    fs::rename(parent.join("child"), parent.join("moved"))?;
    fs::create_dir(parent.join("child"))?;
    assert!(directory.open_child(&seen).is_err());
    fs::remove_dir(parent.join("child"))?;
    symlink_dir(&target, parent.join("child"))?;
    assert!(directory.open_child(&seen).is_err());
    assert!(
        entries(&mut *pinned)?
            .iter()
            .any(|e| e.name == name("original"))
    );
    let mut fresh = open_verified(&parent).map_err(|e| format!("{e:?}"))?;
    let link = entries(&mut fresh)?
        .into_iter()
        .find(|e| e.name == name("child"))
        .ok_or("missing link")?;
    assert_eq!(
        link.metadata.as_ref().map(|m| &m.kind),
        Ok(&ObservedKind::Link)
    );
    assert!(fresh.open_child(&link).is_err());
    assert!(open_verified(Path::new(&parent).join("child").as_path()).is_err());
    Ok(())
}

#[test]
fn scope_identity_is_anchored_to_the_real_volume_and_directory()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = tempfile::tempdir()?;
    let selected = fixture.path().join("scope");
    fs::create_dir(&selected)?;
    let path = selected
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    let first = WindowsScopeRoots
        .open_root(&path)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(first.volume_id.len(), 96);
    let canonical = first.canonical_path.raw.bytes()?;
    let canonical_open = WindowsScopeRoots
        .open_root(&canonical)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(first.file_id, canonical_open.file_id);
    assert_eq!(first.volume_id, canonical_open.volume_id);
    fs::rename(&selected, fixture.path().join("old"))?;
    fs::create_dir(&selected)?;
    let second = WindowsScopeRoots
        .open_root(&path)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(first.volume_id, second.volume_id);
    assert_ne!(first.file_id, second.file_id);
    Ok(())
}
