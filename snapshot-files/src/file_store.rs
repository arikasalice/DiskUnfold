use du_core_contracts::{
    Id,
    snapshot_file::{
        PendingSnapshotFile, SnapshotFileDirectory, SnapshotFileError, SnapshotFilePort,
        SnapshotFileSystem, SnapshotFileSystemTempFile, SnapshotPublishError, SnapshotStoreStatus,
    },
};
use std::io::{self, Write};

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
mod tests {
    use super::*;
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex},
    };

    #[derive(Clone, Default)]
    struct State {
        available: bool,
        free_bytes: Option<u64>,
        supported: bool,
        fail_write: bool,
        register_error: Option<io::ErrorKind>,
        folder_sync_error: Option<io::ErrorKind>,
        temporaries: BTreeMap<String, Vec<u8>>,
        finals: BTreeMap<String, Vec<u8>>,
        writes: usize,
    }

    #[derive(Clone, Default)]
    struct FakeFileSystem(Arc<Mutex<State>>);

    impl FakeFileSystem {
        fn with_state(update: impl FnOnce(&mut State)) -> Self {
            let mut state = State {
                available: true,
                free_bytes: Some(1_000_000),
                supported: true,
                ..State::default()
            };
            update(&mut state);
            Self(Arc::new(Mutex::new(state)))
        }

        fn snapshot(&self) -> State {
            self.0
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone()
        }
    }

    impl SnapshotFileSystem for FakeFileSystem {
        fn open_or_create_snapshots(
            &self,
        ) -> Result<Box<dyn SnapshotFileDirectory + '_>, SnapshotFileError> {
            let available = self
                .0
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .available;
            if !available {
                return Err(SnapshotFileError::StoreUnavailable);
            }
            Ok(Box::new(FakeDirectory(self.0.clone())))
        }
    }

    struct FakeDirectory(Arc<Mutex<State>>);

    impl SnapshotFileDirectory for FakeDirectory {
        fn available_bytes(&self) -> Option<u64> {
            self.0
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .free_bytes
        }

        fn check_no_replace_register(&self) -> Result<(), SnapshotFileError> {
            let supported = self
                .0
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .supported;
            if supported {
                Ok(())
            } else {
                Err(SnapshotFileError::Unsupported)
            }
        }

        fn create_exclusive_temp(
            &self,
            snapshot_id: &Id,
        ) -> Result<Box<dyn SnapshotFileSystemTempFile>, SnapshotFileError> {
            let key = snapshot_id.as_str().to_owned();
            let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
            if state.temporaries.contains_key(&key) {
                return Err(SnapshotFileError::WriteFailed(io::ErrorKind::AlreadyExists));
            }
            state.temporaries.insert(key.clone(), Vec::new());
            drop(state);
            Ok(Box::new(FakeTemporary {
                state: self.0.clone(),
                key,
                bytes: Vec::new(),
                registered: false,
            }))
        }

        fn sync_folder(&self) -> io::Result<()> {
            let error = self
                .0
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .folder_sync_error;
            match error {
                Some(kind) => Err(io::Error::from(kind)),
                None => Ok(()),
            }
        }
    }

    struct FakeTemporary {
        state: Arc<Mutex<State>>,
        key: String,
        bytes: Vec<u8>,
        registered: bool,
    }

    impl SnapshotFileSystemTempFile for FakeTemporary {
        fn sync_file(&mut self) -> io::Result<()> {
            Ok(())
        }

        fn register(&mut self, snapshot_id: &Id) -> io::Result<()> {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            if let Some(kind) = state.register_error {
                return Err(kind.into());
            }
            let final_name = snapshot_id.as_str().to_owned();
            if state.finals.contains_key(&final_name) {
                return Err(io::ErrorKind::AlreadyExists.into());
            }
            state.finals.insert(final_name, self.bytes.clone());
            state.temporaries.remove(&self.key);
            self.registered = true;
            Ok(())
        }
    }

    impl Write for FakeTemporary {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state.writes += 1;
            if state.fail_write {
                return Err(io::ErrorKind::Other.into());
            }
            self.bytes.extend_from_slice(bytes);
            state
                .temporaries
                .insert(self.key.clone(), self.bytes.clone());
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Drop for FakeTemporary {
        fn drop(&mut self) {
            if !self.registered {
                self.state
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .temporaries
                    .remove(&self.key);
            }
        }
    }

    fn id() -> Result<Id, String> {
        Id::parse("018f0000-0000-7000-8000-000000000080")
    }

    #[test]
    fn publish_registers_the_complete_file() -> Result<(), Box<dyn std::error::Error>> {
        let filesystem = FakeFileSystem::with_state(|_| {});
        let state = filesystem.clone();
        let store = SnapshotFileStore::new(filesystem);
        let mut pending = store.create_pending(&id()?)?;
        pending.write_all(b"snapshot")?;
        pending.publish().map_err(|error| format!("{error:?}"))?;
        let state = state.snapshot();
        assert!(state.temporaries.is_empty());
        assert_eq!(
            state.finals.get(id()?.as_str()).map(Vec::as_slice),
            Some(&b"snapshot"[..])
        );
        Ok(())
    }

    #[test]
    fn existing_final_file_is_preserved_and_reported() -> Result<(), Box<dyn std::error::Error>> {
        let filename = id()?;
        let filesystem = FakeFileSystem::with_state(|state| {
            state
                .finals
                .insert(filename.as_str().to_owned(), b"original".to_vec());
        });
        let state = filesystem.clone();
        let store = SnapshotFileStore::new(filesystem);
        let mut pending = store.create_pending(&filename)?;
        pending.write_all(b"replacement")?;
        assert!(matches!(
            pending.publish(),
            Err(SnapshotPublishError::WriteFailed(
                io::ErrorKind::AlreadyExists
            ))
        ));
        let state = state.snapshot();
        assert!(state.temporaries.is_empty());
        assert_eq!(
            state.finals.get(filename.as_str()).map(Vec::as_slice),
            Some(&b"original"[..])
        );
        Ok(())
    }

    #[test]
    fn publish_write_failure_removes_temp_without_creating_final()
    -> Result<(), Box<dyn std::error::Error>> {
        let filename = id()?;
        let filesystem = FakeFileSystem::with_state(|state| {
            state.register_error = Some(io::ErrorKind::Other);
        });
        let state = filesystem.clone();
        let store = SnapshotFileStore::new(filesystem);
        let mut pending = store.create_pending(&filename)?;
        pending.write_all(b"staged")?;
        assert!(matches!(
            pending.publish(),
            Err(SnapshotPublishError::WriteFailed(io::ErrorKind::Other))
        ));
        let state = state.snapshot();
        assert!(!state.temporaries.contains_key(filename.as_str()));
        assert!(!state.finals.contains_key(filename.as_str()));
        Ok(())
    }

    #[test]
    fn unsupported_filesystem_fails_before_creating_or_writing_a_temp_file()
    -> Result<(), Box<dyn std::error::Error>> {
        let filesystem = FakeFileSystem::with_state(|state| state.supported = false);
        let state = filesystem.clone();
        let store = SnapshotFileStore::new(filesystem);
        assert_eq!(
            store.create_pending(&id()?).err(),
            Some(SnapshotFileError::Unsupported)
        );
        let state = state.snapshot();
        assert!(state.temporaries.is_empty());
        assert_eq!(state.writes, 0);
        Ok(())
    }

    #[test]
    fn preflight_reports_capacity_and_capability_without_creating_a_temp_file()
    -> Result<(), Box<dyn std::error::Error>> {
        let filesystem = FakeFileSystem::with_state(|state| state.free_bytes = Some(1234));
        let state = filesystem.clone();
        let store = SnapshotFileStore::new(filesystem);
        let status = store.preflight()?;
        assert_eq!(status.available_bytes, Some(1234));
        assert_eq!(status.no_replace_register, Ok(()));
        let state = state.snapshot();
        assert!(state.temporaries.is_empty());
        assert_eq!(state.writes, 0);
        Ok(())
    }

    #[test]
    fn preflight_reports_unsupported_registration_without_creating_a_temp_file()
    -> Result<(), Box<dyn std::error::Error>> {
        let filesystem = FakeFileSystem::with_state(|state| state.supported = false);
        let state = filesystem.clone();
        let store = SnapshotFileStore::new(filesystem);
        let status = store.preflight()?;
        assert_eq!(
            status.no_replace_register,
            Err(SnapshotFileError::Unsupported)
        );
        let state = state.snapshot();
        assert!(state.temporaries.is_empty());
        assert_eq!(state.writes, 0);
        Ok(())
    }

    #[test]
    fn unavailable_store_is_reported() -> Result<(), Box<dyn std::error::Error>> {
        let filesystem = FakeFileSystem::with_state(|state| state.available = false);
        let store = SnapshotFileStore::new(filesystem);
        assert_eq!(
            store.create_pending(&id()?).err(),
            Some(SnapshotFileError::StoreUnavailable)
        );
        Ok(())
    }

    #[test]
    fn write_failure_removes_the_temporary_file() -> Result<(), Box<dyn std::error::Error>> {
        let filesystem = FakeFileSystem::with_state(|state| state.fail_write = true);
        let state = filesystem.clone();
        let store = SnapshotFileStore::new(filesystem);
        let mut pending = store.create_pending(&id()?)?;
        assert_eq!(
            pending.write(b"partial").err().map(|error| error.kind()),
            Some(io::ErrorKind::Other)
        );
        assert!(state.snapshot().temporaries.is_empty());
        drop(pending);
        assert!(state.snapshot().temporaries.is_empty());
        Ok(())
    }

    #[test]
    fn dropping_an_unpublished_file_removes_the_temp() -> Result<(), Box<dyn std::error::Error>> {
        let filesystem = FakeFileSystem::with_state(|_| {});
        let state = filesystem.clone();
        let store = SnapshotFileStore::new(filesystem);
        let mut pending = store.create_pending(&id()?)?;
        pending.write_all(b"partial")?;
        assert_eq!(state.snapshot().temporaries.len(), 1);
        drop(pending);
        assert!(state.snapshot().temporaries.is_empty());
        Ok(())
    }

    #[test]
    fn folder_sync_failure_reports_registered_not_durable() -> Result<(), Box<dyn std::error::Error>>
    {
        let filename = id()?;
        let filesystem = FakeFileSystem::with_state(|state| {
            state.folder_sync_error = Some(io::ErrorKind::Other);
        });
        let state = filesystem.clone();
        let store = SnapshotFileStore::new(filesystem);
        let mut pending = store.create_pending(&filename)?;
        pending.write_all(b"registered")?;
        assert!(matches!(
            pending.publish(),
            Err(SnapshotPublishError::RegisteredNotDurable(
                io::ErrorKind::Other
            ))
        ));
        let state = state.snapshot();
        assert!(state.temporaries.is_empty());
        assert_eq!(
            state.finals.get(filename.as_str()).map(Vec::as_slice),
            Some(&b"registered"[..])
        );
        Ok(())
    }
}
