#![cfg(windows)]
use du_core_contracts::scan_observation::ScopeRootPort;
use du_platform_os::{
    windows_duplicate::{ExpectedIdentity, InspectError, inspect_handle},
    windows_observation::WindowsScopeRoots,
};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    os::windows::ffi::OsStrExt,
};

fn expected(dir: &std::path::Path) -> Result<ExpectedIdentity, Box<dyn std::error::Error>> {
    let path: Vec<u8> = dir
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect();
    let mut scope = WindowsScopeRoots
        .open_root(&path)
        .map_err(|error| format!("{error:?}"))?;
    let entries = scope
        .directory
        .next_batch()
        .map_err(|error| format!("{error:?}"))?;
    let entry = entries
        .iter()
        .find(|entry| {
            entry.name
                == "item"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>()
        })
        .ok_or("missing item")?;
    let identity = entry
        .metadata
        .as_ref()
        .map_err(|error| format!("{error:?}"))?
        .space_identity
        .as_ref()
        .ok_or("no identity")?;
    Ok(ExpectedIdentity {
        volume: identity.volume.clone(),
        file: identity
            .file
            .clone()
            .try_into()
            .map_err(|_| "file ID length")?,
        enumerated_inode: entry.enumerated_inode,
    })
}

#[test]
fn metadata_only_interval_uses_one_handle_and_detects_change()
-> Result<(), Box<dyn std::error::Error>> {
    use du_platform_os::windows_duplicate::MetadataInterval;
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::Storage::FileSystem::{
        FILE_FLAG_OPEN_NO_RECALL, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
    };
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("item");
    fs::write(&path, b"abc")?;
    let identity = expected(temp.path())?;
    let file = OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES.0)
        .custom_flags((FILE_FLAG_OPEN_NO_RECALL | FILE_FLAG_OPEN_REPARSE_POINT).0)
        .open(&path)?;
    let facts = MetadataInterval::begin(&file, &identity)
        .map_err(|e| format!("{e:?}"))?
        .finish()
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(facts.size, 3);
    assert_eq!(facts.change_stamp, None);
    let interval = MetadataInterval::begin(&file, &identity).map_err(|e| format!("{e:?}"))?;
    OpenOptions::new().write(true).open(&path)?.set_len(10)?;
    assert!(matches!(
        interval.finish(),
        Err(InspectError::IdentityChanged)
    ));
    Ok(())
}

#[test]
fn restored_size_and_timestamp_cannot_prove_unchanged_content()
-> Result<(), Box<dyn std::error::Error>> {
    use du_platform_os::windows_duplicate::MetadataInterval;
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::Storage::FileSystem::{
        FILE_FLAG_OPEN_NO_RECALL, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
    };
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("item");
    fs::write(&path, b"abc")?;
    let modified = fs::metadata(&path)?.modified()?;
    let identity = expected(temp.path())?;
    let handle = OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES.0)
        .custom_flags((FILE_FLAG_OPEN_NO_RECALL | FILE_FLAG_OPEN_REPARSE_POINT).0)
        .open(&path)?;
    let interval = MetadataInterval::begin(&handle, &identity).map_err(|e| format!("{e:?}"))?;
    let mut writer = OpenOptions::new().write(true).open(&path)?;
    writer.write_all(b"xyz")?;
    writer.flush()?;
    writer.set_times(fs::FileTimes::new().set_modified(modified))?;
    let observed = interval.finish().map_err(|e| format!("{e:?}"))?;
    assert_eq!(observed.change_stamp, None);
    Ok(())
}

#[test]
fn inspects_scan_bound_same_handle_without_reading_content()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("item");
    fs::write(&path, b"abc")?;
    let identity = expected(temp.path())?;
    let file = File::open(&path)?;
    let before = inspect_handle(&file, &identity).map_err(|error| format!("{error:?}"))?;
    assert_eq!(before.size, 3);
    assert_eq!(before.change_stamp, None);
    assert_eq!(
        before,
        inspect_handle(&file, &identity).map_err(|error| format!("{error:?}"))?
    );
    OpenOptions::new().write(true).open(&path)?.set_len(10)?;
    let after = inspect_handle(&file, &identity).map_err(|error| format!("{error:?}"))?;
    assert_ne!(before, after);
    assert_eq!(after.size, 10);
    Ok(())
}

#[test]
fn replaced_path_is_rejected_for_the_original_scan_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("item");
    fs::write(&path, b"old")?;
    let identity = expected(temp.path())?;
    let old_handle = File::open(&path)?;
    fs::rename(&path, temp.path().join("old"))?;
    let mut replacement = File::create(&path)?;
    replacement.write_all(b"new")?;
    assert!(matches!(
        inspect_handle(&replacement, &identity),
        Err(InspectError::IdentityChanged)
    ));
    assert_eq!(
        inspect_handle(&old_handle, &identity)
            .map_err(|error| format!("{error:?}"))?
            .size,
        3
    );
    Ok(())
}

#[test]
fn rejects_a_directory_handle_before_identity_matching() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
    let temp = tempfile::tempdir()?;
    let handle = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0)
        .open(temp.path())?;
    let impossible = ExpectedIdentity {
        volume: vec![],
        file: [0; 16],
        enumerated_inode: 0,
    };
    assert!(matches!(
        inspect_handle(&handle, &impossible),
        Err(InspectError::Unsupported)
    ));
    Ok(())
}

#[test]
fn rejects_a_reparse_handle_without_reading_its_target() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::windows::fs::{OpenOptionsExt, symlink_file};
    use windows::Win32::Storage::FileSystem::{
        FILE_FLAG_OPEN_NO_RECALL, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
    };
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("item");
    fs::write(&path, b"target")?;
    let identity = expected(temp.path())?;
    let link = temp.path().join("link");
    match symlink_file(&path, &link) {
        Ok(()) => {}
        Err(error) if error.raw_os_error() == Some(1314) => {
            eprintln!("symlink creation unavailable: privilege not held");
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    }
    let handle = OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES.0)
        .custom_flags((FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_OPEN_NO_RECALL).0)
        .open(&link)?;
    assert!(matches!(
        inspect_handle(&handle, &identity),
        Err(InspectError::Unsupported)
    ));
    Ok(())
}

#[test]
fn rejects_mismatched_volume_file_and_enumerated_identity() -> Result<(), Box<dyn std::error::Error>>
{
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("item");
    fs::write(&path, b"data")?;
    let good = expected(temp.path())?;
    let file = File::open(&path)?;
    let mut wrong_volume = good.clone();
    wrong_volume.volume.push(0);
    let mut wrong_file = good.clone();
    wrong_file.file[0] ^= 1;
    let mut wrong_inode = good;
    wrong_inode.enumerated_inode ^= 1;
    for wrong in [&wrong_volume, &wrong_file, &wrong_inode] {
        assert!(matches!(
            inspect_handle(&file, wrong),
            Err(InspectError::IdentityChanged)
        ));
    }
    Ok(())
}

#[test]
fn scan_native_time_matches_identity_checked_handle_facts() -> Result<(), Box<dyn std::error::Error>>
{
    use du_core_contracts::scan_observation::NativeModifiedTime;
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("item");
    fs::write(&path, b"abc")?;
    let identity = expected(temp.path())?;
    let raw: Vec<u8> = temp
        .path()
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect();
    let mut scope = WindowsScopeRoots
        .open_root(&raw)
        .map_err(|error| format!("{error:?}"))?;
    let entries = scope
        .directory
        .next_batch()
        .map_err(|error| format!("{error:?}"))?;
    let name: Vec<u8> = "item".encode_utf16().flat_map(u16::to_le_bytes).collect();
    let entry = entries
        .iter()
        .find(|entry| entry.name == name)
        .ok_or("missing item")?;
    let metadata = entry
        .metadata
        .as_ref()
        .map_err(|error| format!("{error:?}"))?;
    let native = metadata
        .native_modified_at
        .ok_or("missing native timestamp")?;
    assert!(metadata.modified_at.is_some());
    let facts =
        inspect_handle(&File::open(&path)?, &identity).map_err(|error| format!("{error:?}"))?;
    let NativeModifiedTime::WindowsFiletime(ticks) = native;
    assert_eq!(ticks.get(), facts.last_write_ticks);
    Ok(())
}
