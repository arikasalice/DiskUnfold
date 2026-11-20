//! Read-only NTFS space facts associated with a single open file handle.
use super::{identity, info, raw};
use du_core_contracts::{
    ports::{PortError, PortErrorKind},
    scan_observation::{ObservedValue, SpaceIdentity, attribute_reason},
};
use std::{fs::File, mem::size_of};
use windows::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_COMPRESSED, FILE_ATTRIBUTE_ENCRYPTED,
    FILE_ATTRIBUTE_SPARSE_FILE, FILE_COMPRESSION_INFO, FILE_ID_INFO, FILE_STANDARD_INFO,
    FileCompressionInfo, FileIdInfo, FileStandardInfo, GetFileInformationByHandleEx,
};

pub(super) struct Facts {
    pub allocated: ObservedValue<u64>,
    pub link_count: ObservedValue<u64>,
    pub identity: ObservedValue<SpaceIdentity>,
}

impl Facts {
    /// The facts of an entry that is not a file: none of them apply.
    pub(super) fn not_applicable() -> Self {
        Self {
            allocated: ObservedValue::NotApplicable,
            link_count: ObservedValue::NotApplicable,
            identity: ObservedValue::NotApplicable,
        }
    }
}

/// Reads allocation and link count without reopening the path.
/// Sparse and compressed files use the handle's physical compressed-size report;
/// encrypted files remain unknown because their allocation evidence is incomplete.
pub(super) fn read(
    file: &File,
    first: &BY_HANDLE_FILE_INFORMATION,
    volume_guid: &[u8],
) -> Result<Facts, PortError> {
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
    let file_id = file_id(file);
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
    let failed = ObservedValue::Unknown(attribute_reason::QUERY_FAILED);
    let allocated = if first.dwFileAttributes & FILE_ATTRIBUTE_ENCRYPTED.0 != 0 {
        // The allocation evidence of an encrypted file is incomplete.
        ObservedValue::Unknown(attribute_reason::UNSUPPORTED)
    } else if !queried || standard.DeletePending {
        failed
    } else if first.dwFileAttributes & special != 0 {
        ObservedValue::or_unknown(
            compression_available
                .then(|| u64::try_from(compression.CompressedFileSize).ok())
                .flatten(),
            attribute_reason::QUERY_FAILED,
        )
    } else {
        ObservedValue::or_unknown(
            u64::try_from(standard.AllocationSize).ok(),
            attribute_reason::QUERY_FAILED,
        )
    };
    let link_count = if queried
        && !standard.DeletePending
        && standard.NumberOfLinks > 0
        && standard.NumberOfLinks == first.nNumberOfLinks
        && first.nNumberOfLinks == last.nNumberOfLinks
    {
        ObservedValue::Known(u64::from(standard.NumberOfLinks))
    } else {
        failed
    };
    let file_identity = ObservedValue::or_unknown(
        file_id.map(|id| SpaceIdentity {
            volume: volume_guid.to_vec(),
            file: id.to_vec(),
        }),
        attribute_reason::QUERY_FAILED,
    );
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

// Shared by scan and duplicate metadata checks; no path is reopened.
pub(crate) fn file_id(file: &File) -> Option<[u8; 16]> {
    let mut id = FILE_ID_INFO::default();
    // SAFETY: id is writable and correctly sized; file owns a live handle.
    let available = unsafe {
        GetFileInformationByHandleEx(
            raw(file),
            FileIdInfo,
            std::ptr::addr_of_mut!(id).cast(),
            size_of::<FILE_ID_INFO>() as u32,
        )
    }
    .is_ok();
    (available && id.FileId.Identifier.iter().any(|&byte| byte != 0))
        .then_some(id.FileId.Identifier)
}
