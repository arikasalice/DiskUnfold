use super::*;
use du_core_contracts::{
    Id,
    snapshot_file::{
        PendingSnapshotFile, SnapshotFileError, SnapshotFilePort, SnapshotStoreStatus,
    },
    snapshot_spool::{CaptureSpool, SealedSpool},
};
use std::{
    io::{self, Cursor, Read, Seek, SeekFrom, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Default, Clone)]
struct FileState {
    registered: Option<Vec<u8>>,
    available: Option<u64>,
    capture_open: bool,
}

#[derive(Clone, Default)]
struct FakePort(Arc<Mutex<FileState>>);

impl FakePort {
    fn new(bytes: Option<Vec<u8>>, available: Option<u64>) -> Self {
        Self(Arc::new(Mutex::new(FileState {
            registered: bytes,
            available,
            capture_open: false,
        })))
    }

    fn state(&self) -> FileState {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

impl SnapshotFilePort for FakePort {
    fn preflight(&self) -> Result<SnapshotStoreStatus, SnapshotFileError> {
        Ok(SnapshotStoreStatus {
            available_bytes: self.state().available,
            no_replace_register: Ok(()),
        })
    }

    fn create_pending(
        &self,
        _snapshot_id: &Id,
    ) -> Result<Box<dyn PendingSnapshotFile + '_>, SnapshotFileError> {
        Err(SnapshotFileError::Unsupported)
    }

    fn open_registered(
        &self,
        _snapshot_id: &Id,
    ) -> Result<Box<dyn Read + Send + '_>, SnapshotFileError> {
        self.state()
            .registered
            .map(|bytes| Box::new(Cursor::new(bytes)) as Box<dyn Read + Send>)
            .ok_or(SnapshotFileError::NotFound)
    }

    fn create_capture(
        &self,
        required_bytes: Option<u64>,
    ) -> Result<Box<dyn CaptureSpool<Sealed = Box<dyn SealedSpool>> + Send + '_>, SnapshotFileError>
    {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        let required = required_bytes.unwrap_or(1);
        if state
            .available
            .is_some_and(|available| available < required)
        {
            return Err(SnapshotFileError::InsufficientSpace);
        }
        state.capture_open = true;
        drop(state);
        Ok(Box::new(FakeCapture {
            state: self.0.clone(),
            bytes: Cursor::new(Vec::new()),
            sealed: false,
        }))
    }
}

struct FakeCapture {
    state: Arc<Mutex<FileState>>,
    bytes: Cursor<Vec<u8>>,
    sealed: bool,
}

impl Write for FakeCapture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bytes.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.bytes.flush()
    }
}

impl CaptureSpool for FakeCapture {
    type Sealed = Box<dyn SealedSpool>;

    fn seal(mut self: Box<Self>) -> io::Result<Self::Sealed> {
        let mut bytes = std::mem::take(&mut self.bytes);
        bytes.rewind()?;
        self.sealed = true;
        Ok(Box::new(FakeSealed {
            state: self.state.clone(),
            bytes,
        }))
    }
}

impl Drop for FakeCapture {
    fn drop(&mut self) {
        if !self.sealed {
            self.state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .capture_open = false;
        }
    }
}

struct FakeSealed {
    state: Arc<Mutex<FileState>>,
    bytes: Cursor<Vec<u8>>,
}

impl Read for FakeSealed {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.bytes.read(bytes)
    }
}

impl Seek for FakeSealed {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.bytes.seek(position)
    }
}

impl SealedSpool for FakeSealed {}

impl Drop for FakeSealed {
    fn drop(&mut self) {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .capture_open = false;
    }
}

struct TrackedStage {
    bytes: Cursor<Vec<u8>>,
    discarded: Arc<AtomicBool>,
}

impl TrackedStage {
    fn new(discarded: Arc<AtomicBool>) -> Self {
        Self {
            bytes: Cursor::new(Vec::new()),
            discarded,
        }
    }
}

impl Read for TrackedStage {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.bytes.read(bytes)
    }
}

impl Seek for TrackedStage {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.bytes.seek(position)
    }
}

impl Write for TrackedStage {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let new_len = self
            .bytes
            .position()
            .checked_add(u64::try_from(bytes.len()).map_err(io::Error::other)?)
            .ok_or(io::ErrorKind::OutOfMemory)?;
        if new_len > 1024 * 1024 {
            return Err(io::ErrorKind::OutOfMemory.into());
        }
        self.bytes.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.bytes.flush()
    }
}

impl du_snapshot_store::zstd_stream::StagingSink for TrackedStage {
    fn discard(self) -> io::Result<()> {
        self.discarded.store(true, Ordering::SeqCst);
        Ok(())
    }
}

fn sample_file() -> Result<(Arc<du_analysis_index::ResultVersion>, Id, Vec<u8>), String> {
    let source = source_result(du_core_contracts::result::ResultCompleteness::Partial)?;
    let metadata = source_metadata(false, source.observed_roots().len());
    let bytes = write_snapshot(&source, &metadata)?;
    let (snapshot_id, _) = identity()?;
    Ok((source, snapshot_id, bytes))
}

#[test]
fn registered_file_round_trips_as_an_immutable_snapshot_result() -> Result<(), String> {
    use du_core_contracts::{data_source::DataSource, result::ResultVersionRead};

    let (source, snapshot_id, bytes) = sample_file()?;
    let port = FakePort::new(Some(bytes.clone()), Some(1_000_000));
    let discarded = Arc::new(AtomicBool::new(false));
    let size = u64::try_from(bytes.len()).map_err(|error| error.to_string())?;
    let loaded = du_snapshot_store::load_saved(
        &port,
        &snapshot_id,
        Some(size),
        TrackedStage::new(discarded.clone()),
        || false,
    )
    .map_err(|error| error.to_string())?;

    assert_eq!(loaded.data_source(), DataSource::Snapshot);
    assert_eq!(loaded.snapshot_id(), Some(&snapshot_id));
    assert_eq!(loaded.items().len(), source.items().len());
    assert_eq!(loaded.observed_roots(), source.observed_roots());
    assert_eq!(loaded.scope_gaps().len(), source.scope_gaps().len());
    assert_eq!(loaded.completeness(), source.completeness());
    assert_eq!(loaded.observed_at(), source.observed_at());
    assert!(discarded.load(Ordering::SeqCst));
    let state = port.state();
    assert!(!state.capture_open);
    assert_eq!(state.registered, Some(bytes));
    Ok(())
}

#[test]
fn missing_snapshot_is_not_found_and_discards_staging() -> Result<(), String> {
    let (_, snapshot_id, _) = sample_file()?;
    let port = FakePort::new(None, Some(1_000_000));
    let discarded = Arc::new(AtomicBool::new(false));
    let error = du_snapshot_store::load_saved(
        &port,
        &snapshot_id,
        None,
        TrackedStage::new(discarded.clone()),
        || false,
    )
    .err()
    .ok_or("missing snapshot unexpectedly loaded")?;
    assert_eq!(
        error.kind(),
        du_snapshot_store::LoadSavedErrorKind::NotFound
    );
    assert_eq!(error.reason_key(), Some("snapshot.not_found"));
    assert!(discarded.load(Ordering::SeqCst));
    assert!(!port.state().capture_open);
    Ok(())
}

#[test]
fn corrupted_snapshot_and_wrong_index_size_discard_the_capture() -> Result<(), String> {
    let (_, snapshot_id, mut bytes) = sample_file()?;
    let original = bytes.clone();
    *bytes.last_mut().ok_or("snapshot unexpectedly empty")? ^= 1;
    let port = FakePort::new(Some(bytes), Some(1_000_000));
    let discarded = Arc::new(AtomicBool::new(false));
    let error = du_snapshot_store::load_saved(
        &port,
        &snapshot_id,
        None,
        TrackedStage::new(discarded.clone()),
        || false,
    )
    .err()
    .ok_or("corrupted snapshot unexpectedly loaded")?;
    assert_eq!(error.reason_key(), Some("snapshot.integrity_failed"));
    assert!(discarded.load(Ordering::SeqCst));
    assert!(!port.state().capture_open);

    let port = FakePort::new(Some(original.clone()), Some(1_000_000));
    let discarded = Arc::new(AtomicBool::new(false));
    let size = u64::try_from(original.len()).map_err(|error| error.to_string())?;
    let error = du_snapshot_store::load_saved(
        &port,
        &snapshot_id,
        Some(size + 1),
        TrackedStage::new(discarded.clone()),
        || false,
    )
    .err()
    .ok_or("wrong index size unexpectedly loaded")?;
    assert_eq!(error.reason_key(), Some("snapshot.integrity_failed"));
    assert!(discarded.load(Ordering::SeqCst));
    assert!(!port.state().capture_open);
    assert_eq!(port.state().registered, Some(original));
    Ok(())
}

#[test]
fn cancellation_and_insufficient_space_leave_no_capture() -> Result<(), String> {
    let (_, snapshot_id, bytes) = sample_file()?;
    let size = u64::try_from(bytes.len()).map_err(|error| error.to_string())?;
    let port = FakePort::new(Some(bytes.clone()), Some(1_000_000));
    let discarded = Arc::new(AtomicBool::new(false));
    let mut polls = 0;
    let error = du_snapshot_store::load_saved(
        &port,
        &snapshot_id,
        Some(size),
        TrackedStage::new(discarded.clone()),
        || {
            polls += 1;
            polls >= 3
        },
    )
    .err()
    .ok_or("cancelled load unexpectedly succeeded")?;
    assert_eq!(error.reason_key(), None);
    assert_eq!(
        error.kind(),
        du_snapshot_store::LoadSavedErrorKind::Cancelled
    );
    assert!(discarded.load(Ordering::SeqCst));
    assert!(!port.state().capture_open);
    assert_eq!(port.state().registered, Some(bytes.clone()));

    let port = FakePort::new(Some(bytes), Some(0));
    let discarded = Arc::new(AtomicBool::new(false));
    let error = du_snapshot_store::load_saved(
        &port,
        &snapshot_id,
        Some(size),
        TrackedStage::new(discarded.clone()),
        || false,
    )
    .err()
    .ok_or("insufficient space unexpectedly succeeded")?;
    assert_eq!(error.reason_key(), Some("snapshot.insufficient_space"));
    assert!(discarded.load(Ordering::SeqCst));
    assert!(!port.state().capture_open);
    Ok(())
}
