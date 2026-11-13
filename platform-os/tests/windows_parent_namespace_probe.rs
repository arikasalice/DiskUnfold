#![cfg(windows)]
//! NTFS experiments only: parent sharing does not guard its child namespace.
use std::{
    fs::{self, File, OpenOptions},
    mem::{offset_of, size_of},
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    path::Path,
};
use windows::{
    Wdk::Storage::FileSystem::{
        FILE_ID_BOTH_DIR_INFORMATION, FileIdBothDirectoryInformation, NtQueryDirectoryFile,
    },
    Win32::{
        Foundation::{HANDLE, UNICODE_STRING},
        Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, GetFileInformationByHandle,
        },
        System::IO::IO_STATUS_BLOCK,
    },
    core::PWSTR,
};

fn parent(path: &Path, share: u32) -> std::io::Result<File> {
    OpenOptions::new()
        .read(true)
        .share_mode(share)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0)
        .open(path)
}

#[allow(unsafe_code)]
fn parent_id(file: &File) -> Result<(u32, u64), Box<dyn std::error::Error>> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: file owns a live handle and info is writable for the call.
    unsafe {
        GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info)?;
    }
    Ok((
        info.dwVolumeSerialNumber,
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    ))
}

type LiteralResult = Result<Option<(u64, Vec<u16>)>, Box<dyn std::error::Error>>;

#[allow(unsafe_code)]
fn literal(file: &File, name: &str) -> LiteralResult {
    let wanted: Vec<u16> = name.encode_utf16().collect();
    assert!(!wanted.iter().any(|c| matches!(*c, 34 | 42 | 60 | 62 | 63)));
    let len = u16::try_from(wanted.len() * 2)?;
    let pattern = UNICODE_STRING {
        Length: len,
        MaximumLength: len,
        Buffer: PWSTR(wanted.as_ptr().cast_mut()),
    };
    let mut buffer = vec![0u64; 8192];
    let mut status = IO_STATUS_BLOCK::default();
    // SAFETY: file is a live synchronous directory handle; pattern and the aligned
    // output buffer remain valid throughout this synchronous query.
    let result = unsafe {
        NtQueryDirectoryFile(
            HANDLE(file.as_raw_handle()),
            None,
            None,
            None,
            &mut status,
            buffer.as_mut_ptr().cast(),
            (buffer.len() * 8) as u32,
            FileIdBothDirectoryInformation,
            true,
            Some(&pattern),
            true,
        )
    };
    if result.0 as u32 == 0xc000000f || result.0 as u32 == 0x80000006 {
        return Ok(None);
    }
    if result.0 < 0 {
        return Err(format!("directory query failed: {:#x}", result.0).into());
    }
    let header = offset_of!(FILE_ID_BOTH_DIR_INFORMATION, FileName);
    if status.Information < size_of::<FILE_ID_BOTH_DIR_INFORMATION>()
        || status.Information > buffer.len() * 8
    {
        return Err("short directory record".into());
    }
    // SAFETY: the fixed prefix was checked above and the buffer is aligned.
    let item =
        unsafe { std::ptr::read_unaligned(buffer.as_ptr().cast::<FILE_ID_BOTH_DIR_INFORMATION>()) };
    let bytes = item.FileNameLength as usize;
    if !bytes.is_multiple_of(2)
        || header
            .checked_add(bytes)
            .is_none_or(|end| end > status.Information)
    {
        return Err("invalid name length".into());
    }
    // SAFETY: the validated name fits in the initialized output buffer; read_unaligned
    // avoids assuming any additional alignment of the variable-length name.
    let name = (0..bytes / 2)
        .map(|i| unsafe {
            std::ptr::read_unaligned(
                buffer
                    .as_ptr()
                    .cast::<u8>()
                    .add(header + i * 2)
                    .cast::<u16>(),
            )
        })
        .collect();
    Ok(Some((item.FileId as u64, name)))
}

#[test]
fn fresh_queries_see_child_changes_despite_no_delete_share_on_parent()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let guard = parent(root, 3)?;
    let expected = parent_id(&guard)?;
    let fresh = || -> Result<File, Box<dyn std::error::Error>> {
        let file = parent(root, 7)?;
        assert_eq!(parent_id(&file)?, expected);
        Ok(file)
    };
    fs::create_dir(root.join("PrivateFolder"))?;
    if !root.join("privatefolder").exists() {
        eprintln!("Case-sensitive test directory; case-insensitive probe skipped");
        return Ok(());
    }
    let (original, name) = literal(&fresh()?, "privatefolder")?.ok_or("missing original")?;
    assert_eq!(name, "PrivateFolder".encode_utf16().collect::<Vec<_>>());
    fs::rename(root.join("PrivateFolder"), root.join("PRIVATEFOLDER"))?;
    let (renamed, name) = literal(&fresh()?, "privatefolder")?.ok_or("missing rename")?;
    assert_eq!(renamed, original);
    assert_eq!(name, "PRIVATEFOLDER".encode_utf16().collect::<Vec<_>>());
    fs::rename(root.join("PRIVATEFOLDER"), root.join("old"))?;
    fs::create_dir(root.join("PrivateFolder"))?;
    let (replacement, _) = literal(&fresh()?, "privatefolder")?.ok_or("missing replacement")?;
    assert_ne!(replacement, original);
    assert!(literal(&fresh()?, "absent")?.is_none());
    fs::create_dir(root.join("ABSENT"))?;
    assert!(literal(&fresh()?, "absent")?.is_some());
    Ok(())
}
