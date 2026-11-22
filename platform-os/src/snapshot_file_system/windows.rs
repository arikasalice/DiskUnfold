use super::*;
use du_core_contracts::{
    snapshot_file::{SnapshotFileSystem, SnapshotFileSystemTempFile},
    snapshot_spool::{CaptureSpool, SealedSpool},
};

#[cfg(windows)]
impl SnapshotFileSystem for NativeSnapshotFileSystem {
    fn open_or_create_snapshots(
        &self,
    ) -> Result<Box<dyn SnapshotFileDirectory + '_>, SnapshotFileError> {
        use std::fs;

        match fs::create_dir(self.snapshots_path()) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(SnapshotFileError::StoreUnavailable),
        }
        let directory = crate::windows_snapshot::SnapshotDirectory::open(&self.snapshots_path())
            .map_err(|_| SnapshotFileError::StoreUnavailable)?;
        let directory_path = self.snapshots_path();
        let available_bytes = windows_available_bytes(&directory_path);
        Ok(Box::new(WindowsSnapshotDirectory {
            directory,
            available_bytes,
        }))
    }
}

#[cfg(windows)]
struct WindowsSnapshotDirectory {
    directory: crate::windows_snapshot::SnapshotDirectory,
    available_bytes: Option<u64>,
}

#[cfg(windows)]
impl SnapshotFileDirectory for WindowsSnapshotDirectory {
    fn available_bytes(&self) -> Option<u64> {
        self.available_bytes
    }

    fn check_no_replace_register(&self) -> Result<(), SnapshotFileError> {
        // SnapshotDirectory::open already verifies a stable local NTFS directory.
        Ok(())
    }

    fn create_exclusive_temp(
        &self,
        snapshot_id: &du_core_contracts::Id,
    ) -> Result<Box<dyn SnapshotFileSystemTempFile>, SnapshotFileError> {
        let temporary = self
            .directory
            .create(snapshot_id)
            .map_err(|error| SnapshotFileError::WriteFailed(error.kind()))?;
        Ok(Box::new(temporary))
    }

    fn open_final(
        &self,
        snapshot_id: &du_core_contracts::Id,
    ) -> std::io::Result<Box<dyn std::io::Read + Send>> {
        self.directory
            .open_final(snapshot_id)
            .map(|file| Box::new(file) as _)
    }

    fn create_capture(
        &self,
        required_bytes: Option<u64>,
    ) -> Result<Box<dyn CaptureSpool<Sealed = Box<dyn SealedSpool>> + Send>, SnapshotFileError>
    {
        check_capture_space(self.available_bytes, required_bytes)?;
        let capture_id = random_capture_id()?;
        self.directory
            .capture(&capture_id)
            .map(|capture| Box::new(capture) as _)
            .map_err(|error| SnapshotFileError::WriteFailed(error.kind()))
    }

    fn sync_folder(&self) -> std::io::Result<()> {
        self.directory.sync_folder()
    }
}

#[cfg(windows)]
fn windows_available_bytes(path: &std::path::Path) -> Option<u64> {
    use ::windows::{Win32::Storage::FileSystem::GetDiskFreeSpaceExW, core::PCWSTR};
    use std::os::windows::ffi::OsStrExt;

    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    let mut available = 0_u64;
    // SAFETY: `wide` is NUL-terminated and `available` is a valid output pointer.
    unsafe { GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&mut available), None, None) }
        .ok()
        .map(|()| available)
}

#[cfg(windows)]
impl SnapshotFileSystemTempFile for crate::windows_snapshot::TemporarySnapshot {
    fn sync_file(&mut self) -> std::io::Result<()> {
        self.sync()
    }

    fn register(&mut self, snapshot_id: &du_core_contracts::Id) -> std::io::Result<()> {
        self.register_after_sync(snapshot_id)
    }
}

fn random_capture_id() -> Result<du_core_contracts::Id, SnapshotFileError> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes)
        .map_err(|_| SnapshotFileError::WriteFailed(std::io::ErrorKind::Other))?;
    Ok(du_core_contracts::Id::from_v7_bytes(bytes))
}
