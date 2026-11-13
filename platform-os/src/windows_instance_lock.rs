//! Exclusive GUI ownership of one verified Windows data directory.
//!
//! The guard is a migration prerequisite, not GUI activation or an app startup
//! implementation. All cooperating GUI processes must use this same lock name.
use crate::windows_observation::{WindowsDirectory, open_verified_stable};
use du_core_contracts::ports::PortError;
use std::{
    fs::File,
    io,
    mem::size_of,
    os::windows::io::{AsRawHandle, FromRawHandle},
    path::Path,
};
use windows::{
    Wdk::{
        Foundation::OBJECT_ATTRIBUTES,
        Storage::FileSystem::{
            FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_IF, FILE_OPEN_NO_RECALL,
            FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT, NTCREATEFILE_CREATE_DISPOSITION,
            NtCreateFile,
        },
    },
    Win32::{
        Foundation::{HANDLE, OBJ_CASE_INSENSITIVE, RtlNtStatusToDosError, UNICODE_STRING},
        Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL,
            FILE_ATTRIBUTE_REPARSE_POINT, FILE_GENERIC_READ, FILE_SHARE_MODE, FILE_SHARE_READ,
            FILE_SHARE_WRITE, GetFileInformationByHandle, SYNCHRONIZE,
        },
        System::IO::IO_STATUS_BLOCK,
    },
    core::PWSTR,
};

/// Acquisition failure; no migration should start on any error.
#[derive(Debug)]
pub enum InstanceLockError {
    /// The supplied directory or its ancestor chain could not be verified.
    Directory(PortError),
    /// The lock is already held or another opener has incompatible sharing.
    Busy,
    /// The sentinel is not a single-link ordinary file.
    InvalidSentinel,
    /// The database path is a link or does not identify an ordinary file.
    InvalidDatabase,
    /// Native filesystem operation failed.
    Io(io::Error),
}

/// Holds the sentinel and its verified ancestor chain until dropped.
///
/// This covers local NTFS directories accepted by the existing stable path
/// verifier. It does not prevent non-cooperating processes from editing the DB.
pub struct WindowsInstanceLock {
    _sentinel: File,
    _directory: WindowsDirectory,
}

impl WindowsInstanceLock {
    /// Acquires the per-data-directory guard without following reparse points.
    /// The directory must already exist and remain stable for the guard's life.
    pub fn acquire(path: &Path) -> Result<Self, InstanceLockError> {
        let directory = open_verified_stable(path).map_err(InstanceLockError::Directory)?;
        let sentinel = open_child(
            &directory,
            ".diskunfold.instance.lock",
            FILE_OPEN_IF,
            FILE_SHARE_MODE(0),
        )?;
        if !single_link_file(&sentinel)? {
            return Err(InstanceLockError::InvalidSentinel);
        }
        Ok(Self {
            _sentinel: sentinel,
            _directory: directory,
        })
    }

    /// Pins the existing database against rename or deletion while permitting SQLite I/O.
    /// An absent file is reported separately so the caller can publish a new database.
    pub fn pin_database(&self) -> Result<Option<File>, InstanceLockError> {
        pin_existing_database(&self._directory)
    }
}

/// Pins an existing single-link database for a reader without taking the GUI lock.
/// Keep the directory and returned handle alive for the duration of SQLite access.
pub fn pin_existing_database(
    directory: &WindowsDirectory,
) -> Result<Option<File>, InstanceLockError> {
    match open_child(
        directory,
        crate::application_data::DATABASE_FILE,
        FILE_OPEN,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
    ) {
        Ok(file) if single_link_file(&file)? => Ok(Some(file)),
        Ok(_) => Err(InstanceLockError::InvalidDatabase),
        Err(InstanceLockError::Io(error)) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn open_child(
    directory: &WindowsDirectory,
    name: &str,
    disposition: NTCREATEFILE_CREATE_DISPOSITION,
    sharing: FILE_SHARE_MODE,
) -> Result<File, InstanceLockError> {
    let name: Vec<u16> = name.encode_utf16().collect();
    let length = (name.len() * 2) as u16;
    let object_name = UNICODE_STRING {
        Length: length,
        MaximumLength: length,
        Buffer: PWSTR(name.as_ptr().cast_mut()),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: HANDLE(directory.storage_handle().as_raw_handle()),
        ObjectName: &object_name,
        Attributes: OBJ_CASE_INSENSITIVE,
        SecurityDescriptor: std::ptr::null(),
        SecurityQualityOfService: std::ptr::null(),
    };
    let mut status = IO_STATUS_BLOCK::default();
    let mut handle = HANDLE::default();
    // SAFETY: The verified root, UTF-16 name and writable outputs remain alive.
    // The fixed child name is resolved relative to the retained directory handle.
    let result = unsafe {
        NtCreateFile(
            &mut handle,
            FILE_GENERIC_READ | SYNCHRONIZE,
            &attributes,
            &mut status,
            None,
            FILE_ATTRIBUTE_NORMAL,
            sharing,
            disposition,
            FILE_NON_DIRECTORY_FILE
                | FILE_OPEN_REPARSE_POINT
                | FILE_OPEN_NO_RECALL
                | FILE_SYNCHRONOUS_IO_NONALERT,
            None,
            0,
        )
    };
    if result.0 < 0 {
        // SAFETY: Converting the returned status does not access process memory.
        let code = unsafe { RtlNtStatusToDosError(result) } as i32;
        return Err(if code == 32 || code == 33 {
            InstanceLockError::Busy
        } else {
            InstanceLockError::Io(io::Error::from_raw_os_error(code))
        });
    }
    // SAFETY: A successful call transfers one owned file handle.
    Ok(unsafe { File::from_raw_handle(handle.0) })
}

fn single_link_file(file: &File) -> Result<bool, InstanceLockError> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: The handle is live and the output structure is writable.
    unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }.map_err(
        |error| {
            InstanceLockError::Io(io::Error::from_raw_os_error(
                (error.code().0 as u32 & 0xffff) as i32,
            ))
        },
    )?;
    Ok(
        info.dwFileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT.0 | FILE_ATTRIBUTE_DIRECTORY.0) == 0
            && info.nNumberOfLinks == 1,
    )
}

#[cfg(test)]
mod tests;
