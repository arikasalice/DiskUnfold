use super::*;
use du_core_contracts::scan_observation::{RecentScopeObservation, ScopeRootPort};
use std::{os::windows::ffi::OsStrExt, sync::atomic::AtomicBool};

#[test]
fn parses_only_terminated_nonempty_mount_entries() {
    let mounts: Vec<_> = "D:\\\0C:\\mount\\\0\0".encode_utf16().collect();
    let parsed = parse_mounts(&mounts).map(|paths| {
        paths
            .into_iter()
            .map(|path| String::from_utf16_lossy(&path))
            .collect::<Vec<_>>()
    });
    assert_eq!(parsed, Some(vec![r"C:\mount\".into(), r"D:\".into()]));
    assert!(parse_mounts(&mounts[..mounts.len() - 1]).is_none());
    assert!(parse_mounts(&[0, b'C' as u16, BACKSLASH, 0, 0]).is_none());
    assert_eq!(parse_mounts(&[0, 0]), Some(Vec::new()));
}

#[test]
fn rejects_unrecognized_saved_identity_and_honors_the_checkpoint() {
    let entry = drive_path('C');
    let volume = WindowsVolumes;
    assert!(volume.recent_volume(b"C:\\", &entry, &|| true).is_none());
    let id = guid_bytes(r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}");
    assert!(volume.recent_volume(&id, &entry, &|| false).is_none());
}

#[test]
fn native_same_open_volume_is_located_at_its_current_entry()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let raw = bytes(&temp.path().as_os_str().encode_wide().collect::<Vec<_>>());
    let (root, observer) = crate::windows_observation::WindowsScopeRoots
        .open_root_with_recent(&raw)
        .map_err(|error| format!("{error:?}"))?;
    let observer = observer
        .ok_or("recent observation is unavailable")?
        .map_err(|error| format!("{error:?}"))?;
    let RecentScopeObservation::Observed(evidence) = observer.observe(&AtomicBool::new(false))
    else {
        return Err("same-handle observation missing".into());
    };
    let matches = WindowsVolumes
        .recent_volume(&root.volume_id, &evidence.entry_path, &|| true)
        .ok_or("could not confirm the mount entries for the native volume")?;
    assert_eq!(matches.identity_kind, IDENTITY_KIND);
    assert!(
        matches
            .entries
            .iter()
            .any(|entry| entry.same_entry && entry.path == evidence.entry_path)
    );
    let saved = saved_drive(&evidence.entry_path).ok_or("missing saved drive")?;
    let old = if saved == u16::from(b'Z') { 'Y' } else { 'Z' };
    let different = WindowsVolumes
        .recent_volume(&root.volume_id, &drive_path(old), &|| true)
        .ok_or("could not confirm the mount entries against a different entry")?;
    assert!(
        different
            .entries
            .iter()
            .any(|entry| !entry.same_entry && entry.path == evidence.entry_path)
    );
    Ok(())
}

#[test]
fn native_inventory_checks_absence_of_a_distinct_guid() {
    let id = guid_bytes(r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}");
    assert_eq!(inventory_excludes(&id, &|| true), Some(true));
    assert!(inventory_excludes(&id, &|| false).is_none());
    assert_eq!(
        WindowsVolumes.recent_volume(&id, &drive_path('C'), &|| true),
        Some(RecentVolumeObservation {
            identity_kind: IDENTITY_KIND,
            entries: Vec::new()
        }),
    );
}

fn guid_bytes(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn drive_path(letter: char) -> OsPath {
    let display = format!("\\\\?\\{letter}:\\");
    OsPath {
        encoding: PathEncoding::WindowsUtf16le,
        raw: PathBytes::from_bytes(&guid_bytes(&display)),
        display,
        lossy: false,
    }
}
