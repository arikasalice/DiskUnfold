#![cfg(windows)]
use du_core_contracts::scan_observation::AnchoredDirectory;
use du_platform_os::windows_observation::open_verified;
use std::fs;

#[test]
fn root_last_write_comes_from_the_verified_directory_handle()
-> Result<(), Box<dyn std::error::Error>> {
    use std::{
        os::windows::fs::OpenOptionsExt,
        time::{Duration, UNIX_EPOCH},
    };
    use windows::Win32::Storage::FileSystem::{FILE_FLAG_BACKUP_SEMANTICS, FILE_WRITE_ATTRIBUTES};

    let fixture = tempfile::tempdir()?;
    let path = fixture.path().join("root");
    let set_time = |seconds| {
        fs::OpenOptions::new()
            .access_mode(FILE_WRITE_ATTRIBUTES.0)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0)
            .open(&path)?
            .set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(seconds)))
    };
    fs::create_dir(&path)?;
    set_time(1_600_000_000)?;
    let root = open_verified(&path).map_err(|e| format!("{e:?}"))?;
    fs::rename(&path, fixture.path().join("old"))?;
    fs::create_dir(&path)?;
    set_time(1_700_000_000)?;
    let replacement = open_verified(&path).map_err(|e| format!("{e:?}"))?;
    let original_time = root.observed_modified_at().ok_or("original time missing")?;
    let replacement_time = replacement
        .observed_modified_at()
        .ok_or("replacement time missing")?;
    assert_eq!(original_time.as_str(), "2020-09-13T12:26:40.000Z");
    assert_eq!(replacement_time.as_str(), "2023-11-14T22:13:20.000Z");
    Ok(())
}
