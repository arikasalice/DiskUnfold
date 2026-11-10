//! Metadata-only checks on an already-open Windows file handle.
//!
//! This does not open paths or provide a content reader. Callers must not treat
//! successful metadata inspection as proof that content reads avoid recall or
//! access-time changes. No duplicate-verification adapter is composed here.
use crate::windows_observation;
use du_core_contracts::duplicate_content::{
    BoundContent, ContentIdentity, DuplicateContentPort, PlaceholderEvidence,
};
use du_core_contracts::ports::{PortError, PortErrorKind};
use std::{fs::File, io, os::windows::io::AsRawHandle};
use windows::Win32::{
    Foundation::HANDLE,
    Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_OFFLINE,
        FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS, FILE_ATTRIBUTE_REPARSE_POINT,
        GetVolumeInformationByHandleW, VOLUME_NAME_GUID,
    },
};

/// File identity evidence recorded on the selected scan observation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpectedIdentity {
    /// Volume GUID name encoded as UTF-16LE, without a terminating NUL.
    pub volume: Vec<u8>,
    /// NTFS 128-bit file identifier reported by the scan.
    pub file: [u8; 16],
    /// Nonzero file index from the parent directory enumeration.
    pub enumerated_inode: u64,
}

/// Facts from one opened file, suitable for explicit before/after comparison.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandleFacts {
    /// Scan-bound identity observed on this handle.
    pub identity: ExpectedIdentity,
    /// Logical file length in bytes.
    pub size: u64,
    /// Native FILETIME in 100 ns ticks; no time-zone or millisecond rounding is applied.
    pub last_write_ticks: u64,
    /// A reliable change stamp is not currently obtained by this helper.
    pub change_stamp: Option<u64>,
}

/// Why the metadata-only inspection cannot identify an ordinary local file.
#[derive(Debug)]
pub enum InspectError {
    /// Windows could not read a required fact.
    Io(io::Error),
    /// The file or filesystem does not support this inspection.
    Unsupported,
    /// Required identity or metadata changed.
    IdentityChanged,
}

/// The default Windows content port refuses all opens until no-recall and
/// no-access-time-update guarantees can both be established.
pub struct WindowsDuplicateContent;

impl DuplicateContentPort for WindowsDuplicateContent {
    fn open_bound<'a>(
        &'a mut self,
        _item: &du_core_contracts::ItemRef,
        _expected: &ContentIdentity,
        _placeholder: PlaceholderEvidence,
    ) -> Result<Box<dyn BoundContent + 'a>, PortError> {
        Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "os.content_safety_unverified".into(),
            os_code: None,
        })
    }
}
fn invalid_attributes(value: u32) -> bool {
    value
        & (FILE_ATTRIBUTE_DIRECTORY
            | FILE_ATTRIBUTE_REPARSE_POINT
            | FILE_ATTRIBUTE_OFFLINE
            | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
            .0
        != 0
}
fn observation_error(error: PortError) -> InspectError {
    match error.kind {
        PortErrorKind::IdentityChanged => InspectError::IdentityChanged,
        PortErrorKind::Unsupported => InspectError::Unsupported,
        _ => InspectError::Io(error.os_code.map_or_else(
            || io::Error::other(error.reason_key),
            io::Error::from_raw_os_error,
        )),
    }
}
fn get_info(file: &File) -> Result<BY_HANDLE_FILE_INFORMATION, InspectError> {
    windows_observation::info(file).map_err(observation_error)
}
fn volume_guid(file: &File) -> Result<Vec<u8>, InspectError> {
    let path =
        windows_observation::final_path(file, VOLUME_NAME_GUID).map_err(observation_error)?;
    windows_observation::volume_name(&path).map_err(observation_error)
}
fn file_identifier(file: &File) -> Result<[u8; 16], InspectError> {
    windows_observation::file_id(file).ok_or(InspectError::Unsupported)
}
fn is_ntfs(file: &File) -> bool {
    let mut fs = [0u16; 32];
    // SAFETY: the handle and supplied filesystem-name buffer remain valid.
    unsafe {
        GetVolumeInformationByHandleW(
            HANDLE(file.as_raw_handle()),
            None,
            None,
            None,
            None,
            Some(&mut fs),
        )
    }
    .is_ok()
        && fs.starts_with(&[78, 84, 70, 83, 0])
}
fn raw_index(info: &BY_HANDLE_FILE_INFORMATION) -> u64 {
    windows_observation::identity(info)
}
fn last_write(info: &BY_HANDLE_FILE_INFORMATION) -> u64 {
    (u64::from(info.ftLastWriteTime.dwHighDateTime) << 32)
        | u64::from(info.ftLastWriteTime.dwLowDateTime)
}

/// Checks a live handle against scan identity and returns its current metadata.
///
/// This never opens a path or reads file content. The caller must independently
/// establish that the handle was opened without following a replaced path or
/// recalling a placeholder. A missing change stamp is represented as `None`.
pub fn inspect_handle(
    file: &File,
    expected: &ExpectedIdentity,
) -> Result<HandleFacts, InspectError> {
    let first = get_info(file)?;
    if invalid_attributes(first.dwFileAttributes) || !is_ntfs(file) {
        return Err(InspectError::Unsupported);
    }
    if last_write(&first) == 0 {
        return Err(InspectError::Unsupported);
    }
    let actual = ExpectedIdentity {
        volume: volume_guid(file)?,
        file: file_identifier(file)?,
        enumerated_inode: raw_index(&first),
    };
    if actual.enumerated_inode == 0 || &actual != expected {
        return Err(InspectError::IdentityChanged);
    }
    let second = get_info(file)?;
    if invalid_attributes(second.dwFileAttributes)
        || raw_index(&second) != actual.enumerated_inode
        || first.dwVolumeSerialNumber != second.dwVolumeSerialNumber
        || first.nFileSizeHigh != second.nFileSizeHigh
        || first.nFileSizeLow != second.nFileSizeLow
        || last_write(&first) != last_write(&second)
    {
        return Err(InspectError::IdentityChanged);
    }
    Ok(HandleFacts {
        identity: actual,
        size: (u64::from(second.nFileSizeHigh) << 32) | u64::from(second.nFileSizeLow),
        last_write_ticks: last_write(&second),
        change_stamp: None,
    })
}
