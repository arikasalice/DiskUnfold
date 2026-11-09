//! Read-only NTFS space facts associated with a single open file handle.
use super::{identity, info, raw};
use du_core_contracts::{
    ports::{PortError, PortErrorKind},
    scan_observation::SpaceIdentity,
};
use std::{fs::File, mem::size_of};
use windows::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_COMPRESSED, FILE_ATTRIBUTE_ENCRYPTED,
    FILE_ATTRIBUTE_SPARSE_FILE, FILE_COMPRESSION_INFO, FILE_ID_INFO, FILE_STANDARD_INFO,
    FileCompressionInfo, FileIdInfo, FileStandardInfo, GetFileInformationByHandleEx,
};

#[derive(Default)]
pub(super) struct Facts {
    pub allocated: Option<u64>,
    pub link_count: Option<u64>,
    pub identity: Option<SpaceIdentity>,
}

/// Reads allocation and link count without reopening the path.
/// Sparse and compressed files use the handle's physical compressed-size report;
/// encrypted files remain unknown because their allocation evidence is incomplete.
pub(super) fn read(file: &File, first: &BY_HANDLE_FILE_INFORMATION) -> Result<Facts, PortError> {
    let mut standard = FILE_STANDARD_INFO::default();
    // SAFETY: the live handle and writable, correctly sized structure remain valid.
    let queried = unsafe {
        GetFileInformationByHandleEx(
            raw(file),
            FileStandardInfo,
            std::ptr::addr_of_mut!(standard).cast(),
            size_of::<FILE_STANDARD_INFO>() as u32,
        )
    }
    .is_ok();
    let mut id = FILE_ID_INFO::default();
    // SAFETY: id is writable and has the required size; file retains the handle.
    let identity_available = unsafe {
        GetFileInformationByHandleEx(
            raw(file),
            FileIdInfo,
            std::ptr::addr_of_mut!(id).cast(),
            size_of::<FILE_ID_INFO>() as u32,
        )
    }
    .is_ok();
    let special = (FILE_ATTRIBUTE_COMPRESSED | FILE_ATTRIBUTE_SPARSE_FILE).0;
    let mut compression = FILE_COMPRESSION_INFO::default();
    let compression_available = if first.dwFileAttributes & special != 0 {
        // SAFETY: compression is writable with the required size and file is live.
        unsafe {
            GetFileInformationByHandleEx(
                raw(file),
                FileCompressionInfo,
                std::ptr::addr_of_mut!(compression).cast(),
                size_of::<FILE_COMPRESSION_INFO>() as u32,
            )
        }
        .is_ok()
    } else {
        false
    };
    let last = info(file)?;
    let logical = (u64::from(first.nFileSizeHigh) << 32) | u64::from(first.nFileSizeLow);
    if !consistent(first, &last)
        || (queried && u64::try_from(standard.EndOfFile).ok() != Some(logical))
    {
        return Err(PortError {
            kind: PortErrorKind::IdentityChanged,
            reason_key: "os.query_failed".into(),
            os_code: None,
        });
    }
    let allocated = if first.dwFileAttributes & FILE_ATTRIBUTE_ENCRYPTED.0 != 0
        || !queried
        || standard.DeletePending
    {
        None
    } else if first.dwFileAttributes & special != 0 {
        compression_available
            .then(|| u64::try_from(compression.CompressedFileSize).ok())
            .flatten()
    } else {
        u64::try_from(standard.AllocationSize).ok()
    };
    let link_count = if queried
        && !standard.DeletePending
        && standard.NumberOfLinks > 0
        && standard.NumberOfLinks == first.nNumberOfLinks
        && first.nNumberOfLinks == last.nNumberOfLinks
    {
        Some(u64::from(standard.NumberOfLinks))
    } else {
        None
    };
    let file_identity = (identity_available && id.FileId.Identifier.iter().any(|&byte| byte != 0))
        .then(|| SpaceIdentity {
            volume: id.VolumeSerialNumber.to_le_bytes().to_vec(),
            file: id.FileId.Identifier.to_vec(),
        });
    Ok(Facts {
        allocated,
        link_count,
        identity: file_identity,
    })
}

fn consistent(first: &BY_HANDLE_FILE_INFORMATION, last: &BY_HANDLE_FILE_INFORMATION) -> bool {
    identity(first) == identity(last)
        && first.dwVolumeSerialNumber == last.dwVolumeSerialNumber
        && first.dwFileAttributes == last.dwFileAttributes
        && first.nFileSizeHigh == last.nFileSizeHigh
        && first.nFileSizeLow == last.nFileSizeLow
        && first.ftLastWriteTime.dwHighDateTime == last.ftLastWriteTime.dwHighDateTime
        && first.ftLastWriteTime.dwLowDateTime == last.ftLastWriteTime.dwLowDateTime
}
