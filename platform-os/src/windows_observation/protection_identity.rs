use super::{final_path, raw, scope, win_error};
use du_core_contracts::ports::PortError;
use std::{
    fs::File,
    mem::size_of,
    os::windows::{ffi::OsStrExt, io::FromRawHandle},
    path::Path,
};
use windows::{
    Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_NO_RECALL,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_INFO, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, FileIdInfo, GetFileInformationByHandleEx, OPEN_EXISTING,
        VOLUME_NAME_GUID,
    },
    core::PCWSTR,
};

/// Reads the full native identity of one path without following a reparse point.
pub(crate) fn protection_identity(path: &Path) -> Result<(Vec<u8>, Vec<u8>, u64), PortError> {
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: `wide` is NUL-terminated and remains alive until the handle is opened.
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
    .map_err(win_error)?;
    // SAFETY: CreateFileW returned a valid owned handle.
    let file = unsafe { File::from_raw_handle(handle.0) };
    let mut info = FILE_ID_INFO::default();
    // SAFETY: the buffer has the exact size and layout required for FileIdInfo.
    unsafe {
        GetFileInformationByHandleEx(
            raw(&file),
            FileIdInfo,
            (&mut info as *mut FILE_ID_INFO).cast(),
            size_of::<FILE_ID_INFO>() as u32,
        )
    }
    .map_err(win_error)?;
    let volume_guid = scope::volume_name(&final_path(&file, VOLUME_NAME_GUID)?)?;
    Ok((
        volume_guid,
        info.FileId.Identifier.to_vec(),
        info.VolumeSerialNumber,
    ))
}
