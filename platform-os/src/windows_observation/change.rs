//! Read-only F11 identity lookup through the existing 128-bit file ID helper.
use super::{file_id, info, timestamps, win_error};
use du_core_contracts::{
    ByteSize, OsPath, PathEncoding,
    platform_os_change::{ChangeIdentity, ChangeItemObservation},
    ports::{EntryKind, PortError, PortErrorKind},
};
use std::{
    ffi::OsString,
    fs::File,
    os::windows::{ffi::OsStringExt, io::FromRawHandle},
    path::Path,
};
use windows::{
    Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_OFFLINE,
        FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS, FILE_ATTRIBUTE_RECALL_ON_OPEN,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_NO_RECALL,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, OPEN_EXISTING,
    },
    core::PCWSTR,
};

pub fn lookup_change_item(path: &OsPath) -> Result<ChangeItemObservation, PortError> {
    if path.encoding != PathEncoding::WindowsUtf16le {
        return Err(invalid_path());
    }
    let bytes = path.raw.bytes().map_err(|_| invalid_path())?;
    if bytes.is_empty() || !bytes.len().is_multiple_of(2) {
        return Err(invalid_path());
    }
    let (pairs, _) = bytes.as_chunks::<2>();
    let mut wide: Vec<u16> = pairs.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
    if wide.contains(&0) {
        return Err(invalid_path());
    }
    if !Path::new(&OsString::from_wide(&wide)).is_absolute() {
        return Err(invalid_path());
    }
    wide.push(0);

    // SAFETY: `wide` is NUL-terminated and remains alive for the synchronous call.
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            FILE_READ_ATTRIBUTES.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_OPEN_NO_RECALL,
            None,
        )
    }
    .map_err(classify_windows_error)?;
    // SAFETY: `CreateFileW` succeeded and transferred ownership of this handle.
    let file = unsafe { File::from_raw_handle(handle.0) };
    let facts = info(&file)?;
    let file_id = file_id(&file).ok_or_else(|| unsupported("os.query_failed"))?;
    if facts.dwVolumeSerialNumber == 0 || file_id.iter().all(|&byte| byte == 0) {
        return Err(unsupported("os.query_failed"));
    }
    let attributes = facts.dwFileAttributes;
    let placeholder = (FILE_ATTRIBUTE_OFFLINE
        | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS
        | FILE_ATTRIBUTE_RECALL_ON_OPEN)
        .0;
    let kind = if attributes & placeholder != 0 {
        EntryKind::Placeholder
    } else if attributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        EntryKind::Link
    } else if attributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0 {
        EntryKind::Directory
    } else {
        EntryKind::File
    };
    let size = (u64::from(facts.nFileSizeHigh) << 32) | u64::from(facts.nFileSizeLow);
    Ok(ChangeItemObservation {
        identity: ChangeIdentity {
            volume_id: facts.dwVolumeSerialNumber.to_le_bytes().to_vec(),
            file_id: file_id.to_vec(),
            change_token: None,
        },
        kind,
        size: Some(ByteSize::from(size)),
        modified_at: timestamps::modified_at(&file),
    })
}

fn invalid_path() -> PortError {
    PortError {
        kind: PortErrorKind::Unsupported,
        reason_key: "path.unresolvable".into(),
        os_code: None,
    }
}

fn unsupported(reason_key: &str) -> PortError {
    PortError {
        kind: PortErrorKind::Unsupported,
        reason_key: reason_key.into(),
        os_code: None,
    }
}

fn classify_windows_error(error: windows::core::Error) -> PortError {
    let mut mapped = win_error(error);
    match mapped.os_code {
        Some(32 | 33 | 170) => mapped.kind = PortErrorKind::InUse,
        Some(1 | 50) => {
            mapped.kind = PortErrorKind::Unsupported;
            mapped.reason_key = "os.unsupported".into();
        }
        _ => {}
    }
    mapped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::change_ops::SystemChangeOps;
    use du_core_contracts::{PathBytes, platform_os_change::ChangeOpsPort};
    use std::os::windows::ffi::OsStrExt;

    fn port_result<T>(result: Result<T, PortError>) -> Result<T, Box<dyn std::error::Error>> {
        result.map_err(|error| std::io::Error::other(format!("{error:?}")).into())
    }

    #[test]
    fn lookup_reads_volume_serial_and_128_bit_file_id() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("identity.txt");
        std::fs::write(&path, b"data")?;
        let bytes: Vec<u8> = path
            .as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect();
        let observed_path = OsPath {
            encoding: PathEncoding::WindowsUtf16le,
            raw: PathBytes::from_bytes(&bytes),
            display: path.to_string_lossy().into_owned(),
            lossy: false,
        };
        let observation = port_result(lookup_change_item(&observed_path))?;
        assert_eq!(observation.kind, EntryKind::File);
        assert_eq!(observation.identity.volume_id.len(), 4);
        assert_eq!(observation.identity.file_id.len(), 16);
        assert_eq!(observation.size.as_ref().map(ByteSize::get), Some(4));
        assert_eq!(
            port_result(SystemChangeOps.query_trash_outcome(
                &observed_path,
                &observation.identity,
                None,
            ))?,
            du_core_contracts::platform_os_change::TrashOutcomeQuery::NotMoved
        );
        assert!(path.exists());
        Ok(())
    }
}
