//! Handle-relative, read-only observations on a local NTFS volume.
mod recall;
mod scope;
mod space;

use du_core_contracts::{
    ports::{PortError, PortErrorKind},
    scan_observation::{
        AnchoredDirectory, DirectoryObservations, MetadataObservation, ObservedEntry, ObservedKind,
    },
};
pub use scope::WindowsScopeRoots;
use std::{
    collections::VecDeque,
    ffi::OsStr,
    fs::File,
    mem::{offset_of, size_of},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::{Component, Path, Prefix},
    sync::Arc,
};
use windows::{
    Wdk::{
        Foundation::OBJECT_ATTRIBUTES,
        Storage::FileSystem::{
            FILE_OPEN, FILE_OPEN_NO_RECALL, FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT,
            NtCreateFile,
        },
    },
    Win32::{
        Foundation::{HANDLE, OBJ_CASE_INSENSITIVE, RtlNtStatusToDosError, UNICODE_STRING},
        Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ATTRIBUTE_DIRECTORY,
            FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_NO_RECALL,
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_BOTH_DIR_INFO, FILE_LIST_DIRECTORY,
            FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
            FILE_SHARE_WRITE, FileIdBothDirectoryInfo, FileIdBothDirectoryRestartInfo,
            GETFINALPATHNAMEBYHANDLE_FLAGS, GetDriveTypeW, GetFileInformationByHandle,
            GetFileInformationByHandleEx, GetFinalPathNameByHandleW, GetVolumeInformationByHandleW,
            OPEN_EXISTING, SYNCHRONIZE, VOLUME_NAME_GUID,
        },
        System::IO::IO_STATUS_BLOCK,
    },
    core::{PCWSTR, PWSTR},
};

const BUFFER_SIZE: usize = 65536;
const BATCH_SIZE: usize = 256;

/// A verified, open directory. Names exposed by this adapter are UTF-16LE bytes.
pub struct WindowsDirectory {
    handle: File,
    volume: u64,
    volume_guid: Arc<[u8]>,
    restart: bool,
    pending: VecDeque<ObservedEntry>,
    finished: bool,
    ancestors: Option<AncestorChain>,
}

struct AncestorChain {
    // Retain the verified parent handles for the lifetime of this evidence.
    _handles: Vec<File>,
    ids: Vec<Vec<u8>>,
}

/// Review information obtained from the same handle used for collection.
pub struct DirectoryIdentity {
    /// Normalized DOS path in UTF-16, as reported at the time of review.
    pub physical_path: Vec<u16>,
    pub volume: u64,
    pub file_id: u64,
}

fn error(code: i32) -> PortError {
    let kind = match code {
        2 | 3 => PortErrorKind::NotFound,
        5 => PortErrorKind::PermissionDenied,
        _ => PortErrorKind::Io,
    };
    PortError {
        kind,
        reason_key: "os.query_failed".into(),
        os_code: Some(code),
    }
}
fn invalid() -> PortError {
    PortError {
        kind: PortErrorKind::Unsupported,
        reason_key: "path.unresolvable".into(),
        os_code: None,
    }
}
fn win_error(e: windows::core::Error) -> PortError {
    error(e.code().0 & 0xffff)
}
fn raw(file: &File) -> HANDLE {
    HANDLE(file.as_raw_handle())
}
fn bytes(name: &[u16]) -> Vec<u8> {
    name.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn wide(name: &[u8]) -> Result<Vec<u16>, PortError> {
    if name.is_empty() || !name.len().is_multiple_of(2) {
        return Err(invalid());
    }
    let result: Vec<_> = name
        .as_chunks::<2>()
        .0
        .iter()
        .map(|v| u16::from_le_bytes([v[0], v[1]]))
        .collect();
    if result
        .iter()
        .any(|v| *v == 0 || *v == b'\\' as u16 || *v == b'/' as u16 || *v == b':' as u16)
        || result == [46]
        || result == [46, 46]
    {
        return Err(invalid());
    }
    Ok(result)
}
fn info(file: &File) -> Result<BY_HANDLE_FILE_INFORMATION, PortError> {
    let mut value = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: value is writable for the duration of the call; file owns a live handle.
    unsafe { GetFileInformationByHandle(raw(file), &mut value) }.map_err(win_error)?;
    Ok(value)
}
fn identity(value: &BY_HANDLE_FILE_INFORMATION) -> u64 {
    (u64::from(value.nFileIndexHigh) << 32) | u64::from(value.nFileIndexLow)
}
fn final_path(file: &File, flags: GETFINALPATHNAMEBYHANDLE_FLAGS) -> Result<Vec<u16>, PortError> {
    let mut path = vec![0u16; 32768];
    // SAFETY: the handle is live and the buffer is writable for its full length.
    let length = unsafe { GetFinalPathNameByHandleW(raw(file), &mut path, flags) } as usize;
    if length == 0 {
        return Err(error(
            std::io::Error::last_os_error().raw_os_error().unwrap_or(0),
        ));
    }
    if length >= path.len() {
        return Err(invalid());
    }
    path.truncate(length);
    Ok(path)
}
fn kind(attributes: u32) -> ObservedKind {
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        ObservedKind::Link
    } else if attributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0 {
        ObservedKind::Directory
    } else {
        ObservedKind::File
    }
}

fn relative(parent: &File, name: &[u16], directory: bool) -> Result<File, PortError> {
    let length =
        u16::try_from(name.len().checked_mul(2).ok_or_else(invalid)?).map_err(|_| invalid())?;
    let object_name = UNICODE_STRING {
        Length: length,
        MaximumLength: length,
        Buffer: PWSTR(name.as_ptr().cast_mut()),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: raw(parent),
        ObjectName: &object_name,
        Attributes: OBJ_CASE_INSENSITIVE,
        SecurityDescriptor: std::ptr::null(),
        SecurityQualityOfService: std::ptr::null(),
    };
    let mut status = IO_STATUS_BLOCK::default();
    let mut handle = HANDLE::default();
    let access = FILE_READ_ATTRIBUTES
        | SYNCHRONIZE
        | if directory {
            FILE_LIST_DIRECTORY
        } else {
            Default::default()
        };
    // FILE_DIRECTORY_FILE and FILE_OPEN_NO_RECALL are not a documented
    // compatible combination. Verify directory kind from the opened handle.
    let options = FILE_OPEN_REPARSE_POINT | FILE_OPEN_NO_RECALL | FILE_SYNCHRONOUS_IO_NONALERT;
    // SAFETY: attributes references a valid name and live parent; NtCreateFile writes
    // the handle and status synchronously. Only read-attribute/list access is requested.
    let result = unsafe {
        NtCreateFile(
            &mut handle,
            access,
            &attributes,
            &mut status,
            None,
            Default::default(),
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_OPEN,
            options,
            None,
            0,
        )
    };
    if result.0 < 0 {
        // SAFETY: conversion does not dereference pointers or access filesystem state.
        return Err(error(unsafe { RtlNtStatusToDosError(result) } as i32));
    }
    // SAFETY: a successful NtCreateFile returned a uniquely owned handle.
    Ok(unsafe { File::from_raw_handle(handle.0) })
}

fn verified_directory(
    parent: &File,
    name: &[u16],
    volume: u64,
    expected: Option<u64>,
) -> Result<File, PortError> {
    let file = relative(parent, name, true)?;
    let value = info(&file)?;
    recall::check_handle_attributes(value.dwFileAttributes)?;
    if kind(value.dwFileAttributes) != ObservedKind::Directory
        || u64::from(value.dwVolumeSerialNumber) != volume
        || identity(&value) == 0
        || expected.is_some_and(|id| id != identity(&value))
    {
        return Err(invalid());
    }
    Ok(file)
}

/// Opens an absolute local drive path after verifying NTFS and every component.
///
/// The returned handle can be passed directly to the scan collector. It is not
/// safe to substitute a path reopened after review for this verified handle.
pub fn open_verified(path: &Path) -> Result<WindowsDirectory, PortError> {
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return Err(invalid());
    };
    let (Prefix::Disk(drive) | Prefix::VerbatimDisk(drive)) = prefix.kind() else {
        return Err(invalid());
    };
    if components.next() != Some(Component::RootDir) {
        return Err(invalid());
    }
    let root = [u16::from(drive), b':' as u16, b'\\' as u16, 0];
    // SAFETY: root is NUL-terminated and remains valid throughout these calls.
    if unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) } != 3 {
        return Err(invalid());
    }
    let handle = unsafe {
        CreateFileW(
            PCWSTR(root.as_ptr()),
            (FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE).0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_OPEN_NO_RECALL,
            None,
        )
    }
    .map_err(win_error)?;
    // SAFETY: CreateFileW succeeded and transferred ownership of this handle.
    let mut file = unsafe { File::from_raw_handle(handle.0) };
    let root_info = info(&file)?;
    recall::check_handle_attributes(root_info.dwFileAttributes)?;
    if kind(root_info.dwFileAttributes) != ObservedKind::Directory || identity(&root_info) == 0 {
        return Err(invalid());
    }
    let canonical = final_path(&file, FILE_NAME_NORMALIZED)?;
    let expected = [
        92u16,
        92,
        63,
        92,
        u16::from(drive.to_ascii_uppercase()),
        58,
        92,
    ];
    if canonical.len() != expected.len()
        || !canonical
            .iter()
            .zip(expected)
            .all(|(a, b)| (*a == b) || (*a >= 97 && *a <= 122 && *a - 32 == b))
    {
        return Err(invalid());
    }
    let volume = u64::from(root_info.dwVolumeSerialNumber);
    let volume_guid = scope::volume_name(&final_path(&file, VOLUME_NAME_GUID)?)?.into();
    let mut filesystem = [0u16; 32];
    // SAFETY: the writable name buffer and open handle are valid for the call.
    unsafe {
        GetVolumeInformationByHandleW(raw(&file), None, None, None, None, Some(&mut filesystem))
    }
    .map_err(win_error)?;
    if !filesystem.starts_with(&[78, 84, 70, 83, 0]) {
        return Err(invalid());
    }
    let mut ancestors = AncestorChain {
        _handles: Vec::new(),
        ids: Vec::new(),
    };
    let mut parent_id = identity(&root_info);
    for component in components {
        let Component::Normal(name) = component else {
            return Err(invalid());
        };
        let name: Vec<_> = OsStr::new(name).encode_wide().collect();
        wide(&bytes(&name))?;
        let observed = recall::lookup(&file, &name)?;
        let next = recall::observe_if_safe(observed.attributes, || {
            verified_directory(&file, &name, volume, Some(observed.inode))
        })?;
        ancestors.ids.push(parent_id.to_le_bytes().to_vec());
        parent_id = observed.inode;
        ancestors._handles.push(file);
        file = next;
    }
    Ok(WindowsDirectory {
        handle: file,
        volume,
        volume_guid,
        restart: true,
        pending: VecDeque::new(),
        finished: false,
        ancestors: Some(ancestors),
    })
}

impl WindowsDirectory {
    /// Reads the current handle identity and physical path without reopening it.
    pub fn identity(&self) -> Result<DirectoryIdentity, PortError> {
        let value = info(&self.handle)?;
        Ok(DirectoryIdentity {
            physical_path: final_path(&self.handle, FILE_NAME_NORMALIZED)?,
            volume: u64::from(value.dwVolumeSerialNumber),
            file_id: identity(&value),
        })
    }

    fn observe(&self, name: Vec<u16>, inode: u64, attributes: u32) -> ObservedEntry {
        let metadata = recall::observe_if_safe(attributes, || {
            wide(&bytes(&name))
                .and_then(|name| relative(&self.handle, &name, false))
                .and_then(|file| {
                    let value = info(&file)?;
                    recall::check_handle_attributes(value.dwFileAttributes)?;
                    if u64::from(value.dwVolumeSerialNumber) != self.volume {
                        return Err(invalid());
                    }
                    let size =
                        (u64::from(value.nFileSizeHigh) << 32) | u64::from(value.nFileSizeLow);
                    let facts = if kind(value.dwFileAttributes) == ObservedKind::File {
                        space::read(&file, &value, &self.volume_guid)?
                    } else {
                        space::Facts::default()
                    };
                    Ok(MetadataObservation {
                        kind: kind(value.dwFileAttributes),
                        device: self.volume,
                        inode: identity(&value),
                        size: Ok(size),
                        allocated: facts.allocated,
                        link_count: facts.link_count,
                        space_identity: facts.identity,
                    })
                })
        });
        ObservedEntry {
            name: bytes(&name),
            enumerated_inode: inode,
            enumerated_kind: kind(attributes),
            metadata,
        }
    }

    fn fill(&mut self) -> Result<(), PortError> {
        // u64 backing storage provides the alignment required by the Windows records.
        let mut buffer = vec![0u64; BUFFER_SIZE / size_of::<u64>()];
        let class = if self.restart {
            FileIdBothDirectoryRestartInfo
        } else {
            FileIdBothDirectoryInfo
        };
        self.restart = false;
        // SAFETY: buffer is aligned, writable and has the byte length supplied.
        let result = unsafe {
            GetFileInformationByHandleEx(
                raw(&self.handle),
                class,
                buffer.as_mut_ptr().cast(),
                BUFFER_SIZE as u32,
            )
        };
        if let Err(e) = result {
            let failure = win_error(e);
            if failure.os_code == Some(18) {
                self.finished = true;
                return Ok(());
            }
            return Err(failure);
        }
        let mut offset = 0usize;
        loop {
            let header = offset_of!(FILE_ID_BOTH_DIR_INFO, FileName);
            if offset
                .checked_add(size_of::<FILE_ID_BOTH_DIR_INFO>())
                .is_none_or(|end| end > BUFFER_SIZE)
            {
                return Err(invalid());
            }
            // SAFETY: the bounds above cover the fixed record and the buffer is aligned;
            // unaligned reading also tolerates malformed offsets from the OS.
            let item = unsafe {
                std::ptr::read_unaligned(
                    buffer
                        .as_ptr()
                        .cast::<u8>()
                        .add(offset)
                        .cast::<FILE_ID_BOTH_DIR_INFO>(),
                )
            };
            let len = usize::try_from(item.FileNameLength).map_err(|_| invalid())?;
            if len % 2 != 0
                || offset
                    .checked_add(header)
                    .and_then(|v| v.checked_add(len))
                    .is_none_or(|end| end > BUFFER_SIZE)
            {
                return Err(invalid());
            }
            let mut name = Vec::with_capacity(len / 2);
            for pos in (0..len).step_by(2) {
                // SAFETY: the complete name range was bounds checked above.
                let value = unsafe {
                    std::ptr::read_unaligned(
                        buffer
                            .as_ptr()
                            .cast::<u8>()
                            .add(offset + header + pos)
                            .cast::<u16>(),
                    )
                };
                name.push(value);
            }
            if name != [46] && name != [46, 46] {
                self.pending
                    .push_back(self.observe(name, item.FileId as u64, item.FileAttributes));
            }
            if item.NextEntryOffset == 0 {
                break;
            }
            let next = usize::try_from(item.NextEntryOffset).map_err(|_| invalid())?;
            if next < header + len {
                return Err(invalid());
            }
            offset = offset.checked_add(next).ok_or_else(invalid)?;
        }
        Ok(())
    }
}

impl DirectoryObservations for WindowsDirectory {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        while self.pending.is_empty() && !self.finished {
            self.fill()?;
        }
        let count = self.pending.len().min(BATCH_SIZE);
        Ok(self.pending.drain(..count).collect())
    }
}

impl AnchoredDirectory for WindowsDirectory {
    fn open_child(&self, entry: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        let meta = entry.metadata.as_ref().map_err(|e| e.clone())?;
        if meta.kind != ObservedKind::Directory
            || meta.device != self.volume
            || meta.inode == 0
            || entry.enumerated_inode != meta.inode
        {
            return Err(invalid());
        }
        let name = wide(&entry.name)?;
        let handle = verified_directory(&self.handle, &name, self.volume, Some(meta.inode))?;
        Ok(Box::new(Self {
            handle,
            volume: self.volume,
            volume_guid: self.volume_guid.clone(),
            restart: true,
            pending: VecDeque::new(),
            finished: false,
            ancestors: None,
        }))
    }

    fn ancestor_file_ids(&self) -> Option<&[Vec<u8>]> {
        self.ancestors.as_ref().map(|chain| chain.ids.as_slice())
    }
}
