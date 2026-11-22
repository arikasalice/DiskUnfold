use du_core_contracts::{
    Id,
    snapshot_file::{
        PendingSnapshotFile, SnapshotFileDirectory, SnapshotFileError, SnapshotFilePort,
        SnapshotFileSystem, SnapshotFileSystemTempFile, SnapshotPublishError, SnapshotStoreStatus,
    },
    snapshot_spool::{CaptureSpool, SealedSpool},
};
use std::io::{self, Read, Write};

/// Implements the snapshot file port using an injected handle-based filesystem.
pub struct SnapshotFileStore {
    file_system: Box<dyn SnapshotFileSystem>,
}

impl SnapshotFileStore {
    /// Creates a store backed by the supplied operating-system filesystem adapter.
    pub fn new(file_system: impl SnapshotFileSystem + 'static) -> Self {
        Self {
            file_system: Box::new(file_system),
        }
    }
}

impl SnapshotFilePort for SnapshotFileStore {
    fn preflight(&self) -> Result<SnapshotStoreStatus, SnapshotFileError> {
        let directory = self.file_system.open_or_create_snapshots()?;
        let available_bytes = directory.available_bytes();
        let no_replace_register = directory.check_no_replace_register();
        Ok(SnapshotStoreStatus {
            available_bytes,
            no_replace_register,
        })
    }

    fn create_pending(
        &self,
        snapshot_id: &Id,
    ) -> Result<Box<dyn PendingSnapshotFile + '_>, SnapshotFileError> {
        let directory = self.file_system.open_or_create_snapshots()?;
        directory.check_no_replace_register()?;
        let file = directory.create_exclusive_temp(snapshot_id)?;
        Ok(Box::new(PendingSnapshot {
            directory,
            file: Some(file),
            snapshot_id: snapshot_id.clone(),
        }))
    }

    fn open_registered(
        &self,
        snapshot_id: &Id,
    ) -> Result<Box<dyn Read + Send + '_>, SnapshotFileError> {
        let directory = self.file_system.open_or_create_snapshots()?;
        directory
            .open_final(snapshot_id)
            .map_err(|error| match error.kind() {
                io::ErrorKind::NotFound => SnapshotFileError::NotFound,
                kind => SnapshotFileError::ReadFailed(kind),
            })
    }

    fn create_capture(
        &self,
        required_bytes: Option<u64>,
    ) -> Result<Box<dyn CaptureSpool<Sealed = Box<dyn SealedSpool>> + Send + '_>, SnapshotFileError>
    {
        let directory = self.file_system.open_or_create_snapshots()?;
        directory.create_capture(required_bytes)
    }
}

struct PendingSnapshot<'a> {
    directory: Box<dyn SnapshotFileDirectory + 'a>,
    file: Option<Box<dyn SnapshotFileSystemTempFile>>,
    snapshot_id: Id,
}

/// Combines registration and folder synchronization, mapping a sync failure after
/// successful registration to `SnapshotPublishError::RegisteredNotDurable`.
impl PendingSnapshotFile for PendingSnapshot<'_> {
    fn publish(mut self: Box<Self>) -> Result<(), SnapshotPublishError> {
        let mut file = self.file.take().ok_or(SnapshotPublishError::WriteFailed(
            io::ErrorKind::InvalidInput,
        ))?;
        file.sync_file()
            .map_err(|error| SnapshotPublishError::WriteFailed(error.kind()))?;
        file.register(&self.snapshot_id)
            .map_err(|error| SnapshotPublishError::WriteFailed(error.kind()))?;
        self.directory
            .sync_folder()
            .map_err(|error| SnapshotPublishError::RegisteredNotDurable(error.kind()))
    }
}

impl Write for PendingSnapshot<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let result = self
            .file
            .as_mut()
            .ok_or_else(|| io::Error::from(io::ErrorKind::BrokenPipe))?
            .write(bytes);
        if result.is_err() {
            self.file.take();
        }
        result
    }

    fn flush(&mut self) -> io::Result<()> {
        let result = self
            .file
            .as_mut()
            .ok_or_else(|| io::Error::from(io::ErrorKind::BrokenPipe))?
            .flush();
        if result.is_err() {
            self.file.take();
        }
        result
    }
}

#[cfg(test)]
mod tests;
