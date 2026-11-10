//! Handle-relative exclusive snapshot staging on local NTFS.
use crate::windows_observation::{WindowsDirectory, open_verified_stable};
use du_core_contracts::{Id, ports::PortError};
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    mem::size_of,
    os::windows::io::{AsRawHandle, FromRawHandle},
    path::Path,
};
use windows::{
    Wdk::{
        Foundation::OBJECT_ATTRIBUTES,
        Storage::FileSystem::{
            FILE_CREATE, FILE_NON_DIRECTORY_FILE, FILE_OPEN_REPARSE_POINT,
            FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
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
}
impl TemporarySnapshot<'_> {
    /// Flushes and synchronizes staged bytes without registering a final filename.
    pub fn sync(&mut self) -> io::Result<()> {
        self.file.flush()?;
        self.file.sync_all()
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
        self.remove_on_close();
    }
}
