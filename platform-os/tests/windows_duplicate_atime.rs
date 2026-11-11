#![cfg(windows)]
#![allow(unsafe_code)]
//! Isolated NTFS probes of same-handle access-time controls, not a recall guarantee.
//!
//! Microsoft documents that `SetFileTime` must receive the sentinel on the same
//! handle immediately after opening, with `FILE_WRITE_ATTRIBUTES` access:
//! <https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-setfiletime>
//! An unchanged time here cannot prove what an unsuppressed read would do, because
//! NTFS may disable or delay access-time updates. Cloud placeholder transitions
//! and non-hydrating content reads are outside the evidence of this fixture.

use std::{
    fs,
    fs::OpenOptions,
    os::windows::{
        fs::{FileExt, OpenOptionsExt},
        io::AsRawHandle,
    },
};
use windows::Win32::{
    Foundation::{FILETIME, HANDLE},
    Storage::FileSystem::{
        FILE_FLAG_OPEN_NO_RECALL, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
        FILE_READ_DATA, FILE_WRITE_ATTRIBUTES, GetFileTime, GetVolumeInformationByHandleW,
        SetFileTime,
    },
};

fn handle(file: &std::fs::File) -> HANDLE {
    HANDLE(file.as_raw_handle())
}
fn access_time(file: &std::fs::File) -> windows::core::Result<FILETIME> {
    let mut value = FILETIME::default();
    // SAFETY: The live file handle and output pointer remain valid for this call.
    unsafe {
        GetFileTime(handle(file), None, Some(&mut value), None)?;
    }
    Ok(value)
}
fn same_time(a: FILETIME, b: FILETIME) -> bool {
    a.dwHighDateTime == b.dwHighDateTime && a.dwLowDateTime == b.dwLowDateTime
}

#[test]
fn ntfs_handle_accepts_sentinel_before_fixture_read() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = tempfile::tempdir()?;
    let path = fixture.path().join("candidate");
    fs::write(&path, b"isolated fixture")?;
    let seed = OpenOptions::new()
        .access_mode(FILE_WRITE_ATTRIBUTES.0 | FILE_READ_ATTRIBUTES.0)
        .open(&path)?;
    let baseline_ticks = 132_223_104_000_000_000_u64;
    let baseline = FILETIME {
        dwLowDateTime: baseline_ticks as u32,
        dwHighDateTime: (baseline_ticks >> 32) as u32,
    };
    // SAFETY: The live test-fixture handle has FILE_WRITE_ATTRIBUTES; pointers remain valid.
    unsafe {
        SetFileTime(handle(&seed), None, Some(&baseline), None)?;
    }
    drop(seed);
    let file = OpenOptions::new()
        .access_mode(FILE_READ_DATA.0 | FILE_READ_ATTRIBUTES.0 | FILE_WRITE_ATTRIBUTES.0)
        .custom_flags((FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_OPEN_NO_RECALL).0)
        .open(&path)?;
    let suppress = FILETIME {
        dwLowDateTime: u32::MAX,
        dwHighDateTime: u32::MAX,
    };
    // SAFETY: This is the same live fixture handle that performs the subsequent read.
    // The documented sentinel is applied immediately after open, before any other operation.
    unsafe {
        SetFileTime(handle(&file), None, Some(&suppress), None)?;
    }
    let mut filesystem = [0_u16; 32];
    // SAFETY: The live handle and output buffer are valid for the duration of the call.
    unsafe {
        GetVolumeInformationByHandleW(
            handle(&file),
            None,
            None,
            None,
            None,
            Some(&mut filesystem),
        )?;
    }
    assert!(
        filesystem.starts_with(&[78, 84, 70, 83, 0]),
        "fixture must be on NTFS"
    );
    let before = access_time(&file)?;
    assert!(same_time(before, baseline));
    let mut bytes = [0_u8; 16];
    let count = file.seek_read(&mut bytes, 0)?;
    assert_eq!(&bytes[..count], b"isolated fixture");
    let after = access_time(&file)?;
    assert!(same_time(before, after));
    drop(file);
    let metadata = OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES.0)
        .open(&path)?;
    assert!(same_time(after, access_time(&metadata)?));
    // NTFS access updates can be disabled or delayed; equality alone does not
    // prove that an unsuppressed read would have changed the timestamp.
    Ok(())
}

#[test]
fn no_attribute_write_access_rejects_the_documented_suppression()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = tempfile::tempdir()?;
    let path = fixture.path().join("candidate");
    fs::write(&path, b"fixture")?;
    let file = OpenOptions::new()
        .access_mode(FILE_READ_DATA.0 | FILE_READ_ATTRIBUTES.0)
        .open(path)?;
    let suppress = FILETIME {
        dwLowDateTime: u32::MAX,
        dwHighDateTime: u32::MAX,
    };
    // SAFETY: The handle and sentinel pointer are valid; the missing right is intentional.
    assert!(unsafe { SetFileTime(handle(&file), None, Some(&suppress), None) }.is_err());
    Ok(())
}
