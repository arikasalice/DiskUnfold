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
fn observed_last_write_is_retained_after_live_change() -> Result<(), Box<dyn std::error::Error>> {
    use std::time::{Duration, UNIX_EPOCH};
    let fixture = tempfile::tempdir()?;
    let path = fixture.path().join("item");
    fs::write(&path, b"data")?;
    let file = fs::OpenOptions::new().write(true).open(&path)?;
    file.set_times(
        fs::FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(1_600_000_000)),
    )?;
    let mut directory = open_verified(fixture.path()).map_err(|e| format!("{e:?}"))?;
    let observed = entries(&mut directory)?
        .into_iter()
        .find(|e| e.name == name("item"))
        .ok_or("missing item")?;
    let first = observed
        .metadata
        .as_ref()
        .map_err(|e| format!("{e:?}"))?
        .modified_at
        .clone()
        .ok_or("missing time")?;
    assert_eq!(first.as_str(), "2020-09-13T12:26:40.000Z");
    file.set_times(
        fs::FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(1_700_000_000)),
    )?;
    let mut fresh = open_verified(fixture.path()).map_err(|e| format!("{e:?}"))?;
    let later = entries(&mut fresh)?
        .into_iter()
        .find(|e| e.name == name("item"))
        .ok_or("missing item")?;
    assert_ne!(
        later
            .metadata
            .map_err(|e| format!("{e:?}"))?
            .modified_at
            .as_ref(),
        Some(&first)
    );
    assert_eq!(
        observed.metadata.map_err(|e| format!("{e:?}"))?.modified_at,
        Some(first)
    );
    Ok(())
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

#[test]
fn opened_scope_ancestry_uses_verified_handle_ids() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = tempfile::tempdir()?;
    let parent = fixture.path().join("parent");
    let child = parent.join("child");
    let sibling = fixture.path().join("parent-sibling");
    fs::create_dir(&parent)?;
    fs::create_dir(&child)?;
    fs::create_dir(&sibling)?;
    let open = |path: &Path| {
        let raw: Vec<_> = path
            .as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect();
        WindowsScopeRoots
            .open_root(&raw)
            .map_err(|e| format!("{e:?}"))
    };
    let parent = open(&parent)?;
    let child = open(&child)?;
    let sibling = open(&sibling)?;
    let child_ancestors = child
        .directory
        .ancestor_file_ids()
        .ok_or("missing evidence")?;
    assert!(child_ancestors.contains(&parent.file_id));
    assert!(!child_ancestors.contains(&child.file_id));
    assert!(
        !sibling
            .directory
            .ancestor_file_ids()
            .ok_or("missing evidence")?
            .contains(&parent.file_id)
    );
    let drive = fixture
        .path()
        .ancestors()
        .last()
        .ok_or("missing drive root")?;
    let root = open(drive)?;
    assert_eq!(root.directory.ancestor_file_ids(), Some([].as_slice()));
    Ok(())
}

#[test]
fn hard_links_have_the_same_identity_and_reported_allocation()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = tempfile::tempdir()?;
    fs::write(fixture.path().join("original"), vec![42u8; 8192])?;
    fs::hard_link(
        fixture.path().join("original"),
        fixture.path().join("alias"),
    )?;
    let raw: Vec<_> = fixture
        .path()
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect();
    let mut root = WindowsScopeRoots
        .open_root(&raw)
        .map_err(|e| format!("{e:?}"))?;
    let found = entries(&mut *root.directory)?;
    let first = found
        .iter()
        .find(|e| e.name == name("original"))
        .ok_or("original missing")?
        .metadata
        .as_ref()
        .map_err(|e| format!("{e:?}"))?;
    let second = found
        .iter()
        .find(|e| e.name == name("alias"))
        .ok_or("alias missing")?
        .metadata
        .as_ref()
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(first.size, Ok(8192));
    assert!(first.allocated.is_some_and(|n| n >= 8192));
    assert_eq!(first.allocated, second.allocated);
    assert_eq!(first.link_count, Some(2));
    assert_eq!(first.link_count, second.link_count);
    assert_eq!(
        first.space_identity.as_ref().map(|id| id.volume.as_slice()),
        Some(root.volume_id.as_slice())
    );
    assert_eq!(first.space_identity, second.space_identity);
    Ok(())
}

#[test]
#[allow(unsafe_code)]
fn offline_fixture_is_reported_unavailable_without_metadata()
-> Result<(), Box<dyn std::error::Error>> {
    use windows::{
        Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_OFFLINE, SetFileAttributesW,
        },
        core::PCWSTR,
    };
    let fixture = tempfile::tempdir()?;
    let path = fixture.path().join("offline");
    fs::write(&path, b"fixture")?;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: the temporary fixture path is NUL-terminated and remains alive.
    unsafe {
        SetFileAttributesW(PCWSTR(wide.as_ptr()), FILE_ATTRIBUTE_OFFLINE)?;
    }
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let mut root = open_verified(fixture.path()).map_err(|e| format!("{e:?}"))?;
        let found = entries(&mut root)?;
        let entry = found
            .iter()
            .find(|e| e.name == name("offline"))
            .ok_or("missing offline entry")?;
        let failure = entry
            .metadata
            .as_ref()
            .err()
            .ok_or("offline entry was observed")?;
        assert_eq!(
            failure.kind,
            du_core_contracts::ports::PortErrorKind::Unsupported
        );
        assert_eq!(failure.reason_key, "os.unsupported");
        Ok(())
    })();
    // SAFETY: restore the attribute on the same isolated fixture for cleanup.
    unsafe {
        SetFileAttributesW(PCWSTR(wide.as_ptr()), FILE_ATTRIBUTE_NORMAL)?;
    }
    result
}

#[test]
#[allow(unsafe_code)]
fn offline_scope_component_is_rejected_before_directory_open()
-> Result<(), Box<dyn std::error::Error>> {
    use windows::{
        Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_OFFLINE, SetFileAttributesW,
        },
        core::PCWSTR,
    };
    let fixture = tempfile::tempdir()?;
    let path = fixture.path().join("offline-dir");
    fs::create_dir(&path)?;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: the temporary fixture path is NUL-terminated and remains alive.
    unsafe {
        SetFileAttributesW(PCWSTR(wide.as_ptr()), FILE_ATTRIBUTE_OFFLINE)?;
    }
    let result = open_verified(&path);
    // SAFETY: restore the attribute on the same isolated fixture for cleanup.
    unsafe {
        SetFileAttributesW(PCWSTR(wide.as_ptr()), FILE_ATTRIBUTE_NORMAL)?;
    }
    let failure = result.err().ok_or("offline scope accepted")?;
    assert_eq!(
        failure.kind,
        du_core_contracts::ports::PortErrorKind::Unsupported
    );
    assert!(open_verified(&path).is_ok());
    Ok(())
}

#[test]
#[allow(unsafe_code)]
fn scope_non_ascii_case_and_available_short_alias_resolve_to_same_id()
-> Result<(), Box<dyn std::error::Error>> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf};
    use windows::{Win32::Storage::FileSystem::GetShortPathNameW, core::PCWSTR};
    let fixture = tempfile::tempdir()?;
    let actual = fixture.path().join("ÉcoleLongDirectoryName");
    fs::create_dir(&actual)?;
    let expected = open_verified(&actual)
        .map_err(|e| format!("{e:?}"))?
        .identity()
        .map_err(|e| format!("{e:?}"))?;
    let case_variant = fixture.path().join("éCOLELONGDIRECTORYNAME");
    let varied = open_verified(&case_variant)
        .map_err(|e| format!("{e:?}"))?
        .identity()
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        (expected.volume, expected.file_id),
        (varied.volume, varied.file_id)
    );
    let wide: Vec<u16> = actual.as_os_str().encode_wide().chain([0]).collect();
    let mut buffer = vec![0u16; 32768];
    // SAFETY: input is NUL-terminated and the output buffer is writable.
    let size = unsafe { GetShortPathNameW(PCWSTR(wide.as_ptr()), Some(&mut buffer)) } as usize;
    if size > 0 && size < buffer.len() {
        let short = PathBuf::from(OsString::from_wide(&buffer[..size]));
        let short_component = short.file_name().map(|name| name.to_string_lossy());
        eprintln!(
            "short final component uses tilde: {}",
            short_component.is_some_and(|name| name.contains('~'))
        );
        let short_id = open_verified(&short)
            .map_err(|e| format!("{e:?}"))?
            .identity()
            .map_err(|e| format!("{e:?}"))?;
        assert_eq!(
            (expected.volume, expected.file_id),
            (short_id.volume, short_id.file_id)
        );
    } else {
        eprintln!("short-path lookup unavailable");
    }
    Ok(())
}

#[test]
#[allow(unsafe_code)]
fn sparse_file_reports_physical_allocation_without_reading_content()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::{
        Foundation::HANDLE,
        System::{IO::DeviceIoControl, Ioctl::FSCTL_SET_SPARSE},
    };
    let fixture = tempfile::tempdir()?;
    let path = fixture.path().join("sparse");
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    let mut returned = 0u32;
    // SAFETY: the fixture file handle and return pointer are valid for this synchronous call.
    unsafe {
        DeviceIoControl(
            HANDLE(file.as_raw_handle()),
            FSCTL_SET_SPARSE,
            None,
            0,
            None,
            0,
            Some(&mut returned),
            None,
        )?;
    }
    file.set_len(1024 * 1024)?;
    drop(file);
    let mut root = open_verified(fixture.path()).map_err(|e| format!("{e:?}"))?;
    let found = entries(&mut root)?;
    let meta = found
        .iter()
        .find(|e| e.name == name("sparse"))
        .ok_or("missing")?
        .metadata
        .as_ref()
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(meta.size, Ok(1024 * 1024));
    assert_eq!(meta.allocated, Some(0));
    Ok(())
}

#[test]
fn observes_current_system_drive() -> Result<(), Box<dyn std::error::Error>> {
    use du_core_contracts::{ports::PortErrorKind, volume::VolumePort};
    use du_platform_os::volume::WindowsVolumes;
    let system = std::env::var_os("SystemDrive").ok_or("missing SystemDrive")?;
    let mut wide: Vec<u16> = system.encode_wide().collect();
    wide.push(92);
    let path: Vec<u8> = wide.into_iter().flat_map(u16::to_le_bytes).collect();
    let result = WindowsVolumes.enumerate();
    let expected = result
        .mounts
        .map_err(|e| format!("{e:?}"))?
        .into_iter()
        .find(|m| m.mount_path == path)
        .and_then(|m| m.volume_id)
        .ok_or("missing system drive identity")?;
    WindowsVolumes
        .path_access(&path)
        .map_err(|e| format!("{e:?}"))?;
    let capacity = WindowsVolumes
        .observe(&path, Some(&expected), &|| true)
        .map_err(|e| format!("{e:?}"))?
        .capacity
        .ok_or("missing capacity observation")?
        .map_err(|e| format!("{e:?}"))?;
    assert!(capacity.total_bytes >= capacity.available_bytes);
    let without_capacity = WindowsVolumes
        .observe(&path, Some(&expected), &|| false)
        .map_err(|e| format!("{e:?}"))?;
    assert!(without_capacity.capacity.is_none());
    let mut different = expected;
    different.push(0);
    let attempted = std::cell::Cell::new(false);
    let replacement = WindowsVolumes.observe(&path, Some(&different), &|| {
        attempted.set(true);
        true
    });
    assert_eq!(
        replacement.err().map(|e| e.kind),
        Some(PortErrorKind::IdentityChanged)
    );
    assert!(!attempted.get());
    Ok(())
}
