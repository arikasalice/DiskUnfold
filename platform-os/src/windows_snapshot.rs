//! Handle-relative snapshot staging and non-replacing registration on local NTFS.
use crate::windows_observation::{WindowsDirectory, open_verified_stable};
use du_core_contracts::{Id, ports::PortError};
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    mem::{offset_of, size_of},
    os::windows::io::{AsRawHandle, FromRawHandle},
    path::Path,
};
use windows::{
    Wdk::{
        Foundation::OBJECT_ATTRIBUTES,
        Storage::FileSystem::{
            FILE_CREATE, FILE_NON_DIRECTORY_FILE, FILE_OPEN_REPARSE_POINT, FILE_RENAME_INFORMATION,
            FILE_RENAME_INFORMATION_0, FILE_SYNCHRONOUS_IO_NONALERT, FileRenameInformation,
            NtCreateFile, NtSetInformationFile,
        },
    },
    Win32::{
        Foundation::{HANDLE, OBJ_CASE_INSENSITIVE, RtlNtStatusToDosError, UNICODE_STRING},
        Storage::FileSystem::{
            DELETE, FILE_ATTRIBUTE_NORMAL, FILE_DISPOSITION_INFO, FILE_GENERIC_READ,
            FILE_GENERIC_WRITE, FILE_SHARE_MODE, FileDispositionInfo, SYNCHRONIZE,
            SetFileInformationByHandle,
        },
        System::IO::IO_STATUS_BLOCK,
    },
    core::PWSTR,
};

/// A verified local NTFS directory retained for handle-relative operations.
pub struct SnapshotDirectory(WindowsDirectory);
impl SnapshotDirectory {
    /// Verifies every component of an existing local NTFS directory without following reparse points.
    pub fn open(path: &Path) -> Result<Self, PortError> {
        open_verified_stable(path).map(Self)
    }
    /// Exclusively creates and locks an identifier-derived temporary file in this directory.
    pub fn create(&self, temporary_id: &Id) -> io::Result<TemporarySnapshot<'_>> {
        let name = format!(".tmp-{}.dusnap.part", temporary_id.as_str());
        let wide: Vec<u16> = name.encode_utf16().collect();
        let length = u16::try_from(wide.len() * 2).map_err(|_| io::ErrorKind::InvalidInput)?;
        let object_name = UNICODE_STRING {
            Length: length,
            MaximumLength: length,
            Buffer: PWSTR(wide.as_ptr().cast_mut()),
        };
        let attributes = OBJECT_ATTRIBUTES {
            Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
            RootDirectory: HANDLE(self.0.storage_handle().as_raw_handle()),
            ObjectName: &object_name,
            Attributes: OBJ_CASE_INSENSITIVE,
            SecurityDescriptor: std::ptr::null(),
            SecurityQualityOfService: std::ptr::null(),
        };
        let mut status = IO_STATUS_BLOCK::default();
        let mut handle = HANDLE::default();
        // SAFETY: the root and name remain valid; the output handle and status are writable.
        // FILE_CREATE and zero sharing make creation exclusive before any other opener can clean it.
        let result = unsafe {
            NtCreateFile(
                &mut handle,
                FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE | SYNCHRONIZE,
                &attributes,
                &mut status,
                None,
                FILE_ATTRIBUTE_NORMAL,
                FILE_SHARE_MODE(0),
                FILE_CREATE,
                FILE_NON_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
                None,
                0,
            )
        };
        if result.0 < 0 {
            // SAFETY: translating a status value does not access memory or filesystem state.
            return Err(io::Error::from_raw_os_error(
                unsafe { RtlNtStatusToDosError(result) } as i32,
            ));
        }
        // SAFETY: successful creation transfers ownership of a valid handle.
        let file = unsafe { File::from_raw_handle(handle.0) };
        let temporary = TemporarySnapshot {
            _directory: self,
            file,
            registered: false,
        };
        if let Err(error) = temporary.file.try_lock() {
            return Err(error.into());
        }
        Ok(temporary)
    }
}

/// A temporary file held exclusively until registration or cleanup.
pub struct TemporarySnapshot<'a> {
    _directory: &'a SnapshotDirectory,
    file: File,
    registered: bool,
}
impl TemporarySnapshot<'_> {
    /// Flushes and synchronizes staged bytes without registering a final filename.
    pub fn sync(&mut self) -> io::Result<()> {
        self.file.flush()?;
        self.file.sync_all()
    }
    /// Synchronizes and renames the held file within its current parent, without replacement.
    ///
    /// The temporary file and verified directory remain exclusively held until the
    /// native rename completes. No path-based source reopen or copy fallback is used.
    pub fn register(mut self, snapshot_id: &Id) -> io::Result<()> {
        self.sync()?;
        let name: Vec<u16> = format!("{}.dusnap", snapshot_id.as_str())
            .encode_utf16()
            .collect();
        let name_bytes = name
            .len()
            .checked_mul(2)
            .ok_or(io::ErrorKind::InvalidInput)?;
        let size = size_of::<FILE_RENAME_INFORMATION>()
            .checked_add(name_bytes)
            .ok_or(io::ErrorKind::InvalidInput)?;
        let length = u32::try_from(size).map_err(|_| io::ErrorKind::InvalidInput)?;
        let mut buffer = vec![0u64; size.div_ceil(size_of::<u64>())];
        let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
        let mut status = IO_STATUS_BLOCK::default();
        // SAFETY: the aligned buffer holds the fixed header and complete UTF-16 name;
        // this synchronous file handle remains exclusively held through the call.
        let result = unsafe {
            (*info).Anonymous = FILE_RENAME_INFORMATION_0 {
                ReplaceIfExists: false,
            };
            // Native FileRenameInformation interprets a simple name and null root
            // relative to the held file's current parent (not process CWD).
            (*info).RootDirectory = HANDLE::default();
            (*info).FileNameLength = name_bytes as u32;
            std::ptr::copy_nonoverlapping(
                name.as_ptr(),
                buffer
                    .as_mut_ptr()
                    .cast::<u8>()
                    .add(offset_of!(FILE_RENAME_INFORMATION, FileName))
                    .cast::<u16>(),
                name.len(),
            );
            NtSetInformationFile(
                HANDLE(self.file.as_raw_handle()),
                &mut status,
                info.cast(),
                length,
                FileRenameInformation,
            )
        };
        if result.0 < 0 {
            // SAFETY: translating the returned NTSTATUS reads no application memory.
            return Err(io::Error::from_raw_os_error(
                unsafe { RtlNtStatusToDosError(result) } as i32,
            ));
        }
        self.registered = true;
        Ok(())
    }
    fn remove_on_close(&mut self) {
        let info = FILE_DISPOSITION_INFO { DeleteFile: true };
        // SAFETY: the file has DELETE access and info lives for the duration of the call.
        let _ = unsafe {
            SetFileInformationByHandle(
                HANDLE(self.file.as_raw_handle()),
                FileDispositionInfo,
                (&raw const info).cast(),
                size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        };
    }
}
impl Read for TemporarySnapshot<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.file.read(bytes)
    }
}
impl Seek for TemporarySnapshot<'_> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.file.seek(position)
    }
}
impl Write for TemporarySnapshot<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.file.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}
impl Drop for TemporarySnapshot<'_> {
    fn drop(&mut self) {
        if !self.registered {
            self.remove_on_close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    fn file_id(file: &File) -> io::Result<(u32, u32, u32)> {
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: the live file handle and writable information buffer are valid.
        unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }
            .map_err(io::Error::other)?;
        Ok((
            info.dwVolumeSerialNumber,
            info.nFileIndexHigh,
            info.nFileIndexLow,
        ))
    }

    #[test]
    fn registration_keeps_the_held_file_identity() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let root = SnapshotDirectory::open(dir.path()).map_err(|e| format!("{e:?}"))?;
        let source = Id::parse("018f0000-0000-7000-8000-000000000080")?;
        let target = Id::parse("018f0000-0000-7000-8000-000000000081")?;
        let mut staged = root.create(&source)?;
        staged.write_all(b"same handle")?;
        let before = file_id(&staged.file)?;
        staged.register(&target)?;
        let registered = File::open(dir.path().join(format!("{}.dusnap", target.as_str())))?;
        assert_eq!(before, file_id(&registered)?);
        Ok(())
    }
}
