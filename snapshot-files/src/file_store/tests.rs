use super::*;
use std::{
    collections::BTreeMap,
    io::{Cursor, Seek},
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

    fn open_final(&self, snapshot_id: &Id) -> io::Result<Box<dyn Read + Send>> {
        let bytes = self
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .finals
            .get(snapshot_id.as_str())
            .cloned()
            .ok_or(io::ErrorKind::NotFound)?;
        Ok(Box::new(Cursor::new(bytes)))
    }

    fn create_capture(
        &self,
        required_bytes: Option<u64>,
    ) -> Result<Box<dyn CaptureSpool<Sealed = Box<dyn SealedSpool>> + Send>, SnapshotFileError>
    {
        let available = self.available_bytes();
        if let (Some(required), Some(available)) = (required_bytes, available)
            && available < required
        {
            return Err(SnapshotFileError::InsufficientSpace);
        }
        Ok(Box::new(MemoryCapture(Cursor::new(Vec::new()))))
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

struct MemoryCapture(Cursor<Vec<u8>>);

impl Write for MemoryCapture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl CaptureSpool for MemoryCapture {
    type Sealed = Box<dyn SealedSpool>;

    fn seal(mut self: Box<Self>) -> io::Result<Self::Sealed> {
        self.0.rewind()?;
        Ok(Box::new(MemorySealed(self.0)))
    }
}

struct MemorySealed(Cursor<Vec<u8>>);

impl Read for MemorySealed {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.0.read(bytes)
    }
}

impl Seek for MemorySealed {
    fn seek(&mut self, position: io::SeekFrom) -> io::Result<u64> {
        self.0.seek(position)
    }
}

impl SealedSpool for MemorySealed {}

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
fn reads_only_the_registered_final_name_and_checks_capture_capacity()
-> Result<(), Box<dyn std::error::Error>> {
    let filename = id()?;
    let filesystem = FakeFileSystem::with_state(|state| {
        state
            .finals
            .insert(filename.as_str().to_owned(), b"registered".to_vec());
    });
    let store = SnapshotFileStore::new(filesystem);
    let mut reader = store.open_registered(&filename)?;
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    assert_eq!(bytes, b"registered");

    assert_eq!(
        store.create_capture(Some(1_000_001)).err(),
        Some(SnapshotFileError::InsufficientSpace)
    );
    let mut capture = store.create_capture(Some(1))?;
    capture.write_all(b"copy")?;
    let mut sealed = capture.seal()?;
    let mut copied = Vec::new();
    sealed.read_to_end(&mut copied)?;
    assert_eq!(copied, b"copy");
    Ok(())
}

#[test]
fn opening_a_missing_registered_snapshot_is_not_a_write_error()
-> Result<(), Box<dyn std::error::Error>> {
    let store = SnapshotFileStore::new(FakeFileSystem::with_state(|_| {}));
    assert_eq!(
        store.open_registered(&id()?).err(),
        Some(SnapshotFileError::NotFound)
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
fn folder_sync_failure_reports_registered_not_durable() -> Result<(), Box<dyn std::error::Error>> {
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
