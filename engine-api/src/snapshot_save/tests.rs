use super::*;
use crate::desktop::DesktopEvents;
use du_analysis_index::{SampleSessionKey, ScanCompletion};
use du_core_contracts::{
    PathBytes, PathEncoding, Timestamp,
    job::{HistoryAppend, JobFlow, JobFlowError, JobHistory, JobSummary},
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
    snapshot_file::{
        PendingSnapshotFile, SnapshotFileError, SnapshotPublishError, SnapshotStoreStatus,
    },
    snapshot_index::{
        SnapshotIndexCompleteness, SnapshotIndexCursor, SnapshotIndexEntry, SnapshotIndexError,
        SnapshotIndexPage, SnapshotIndexRegisterResult,
    },
};
use du_job_runtime::{JobClock, JobEvents, JobIdentity};
use std::{
    collections::BTreeMap,
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

const CORRELATION: &str = "018f0000-0000-7000-8000-000000000001";

fn id(number: u32) -> Result<Id, Box<dyn std::error::Error>> {
    Ok(Id::parse(&format!(
        "018f0000-0000-7000-8000-{number:012x}"
    ))?)
}

fn fixed_id(number: u32) -> Id {
    let mut bytes = [0; 16];
    bytes[12..].copy_from_slice(&number.to_be_bytes());
    Id::from_v7_bytes(bytes)
}

fn root() -> ResultScopeRoot {
    ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"root"),
        },
        volume_id: vec![1, 2, 3],
        file_id: vec![4, 5, 6],
    }
}

fn observed_at() -> Result<Timestamp, Box<dyn std::error::Error>> {
    Timestamp::from_unix_millis(1_600_000_000_000).ok_or_else(|| "invalid test timestamp".into())
}

fn result_store(
    number: u32,
    sample: bool,
    lineage: bool,
    terminal: bool,
) -> Result<(IngestSink, Id), Box<dyn std::error::Error>> {
    let store = IngestSink::default();
    let version_id = id(number + 2)?;
    let mut build = if sample {
        let session = SampleSessionKey::new(id(number + 1)?);
        store
            .open_sample_forest(id(number)?, vec![root()], &session)
            .map_err(|error| format!("sample open: {error:?}"))?
    } else if lineage {
        let lineage_id = id(number + 1)?;
        let analysis_id = format!("snapshot-save-{number}");
        store
            .bind_current_analysis(&analysis_id, lineage_id.clone())
            .map_err(|error| format!("lineage bind: {error:?}"))?;
        store
            .open_forest_in_lineage(id(number)?, vec![root()], lineage_id)
            .map_err(|error| format!("lineage open: {error:?}"))?
    } else {
        store
            .open_forest(id(number)?, vec![root()])
            .map_err(|error| format!("forest open: {error:?}"))?
    };
    build
        .add_directory(Vec::new())
        .map_err(|error| format!("directory: {error:?}"))?;
    if terminal {
        build
            .seal_at(
                version_id.clone(),
                ResultCompleteness::Complete,
                Some(observed_at()?),
            )
            .map_err(|error| format!("{error:?}"))?;
        if lineage && !sample {
            assert!(
                store
                    .observe_scan_terminal(&version_id, Some(ScanCompletion::Done))
                    .map_err(|error| format!("{error:?}"))?
            );
        }
    } else {
        build
            .seal_at(
                version_id.clone(),
                ResultCompleteness::Partial,
                Some(observed_at()?),
            )
            .map_err(|error| format!("{error:?}"))?;
    }
    Ok((store, version_id))
}

fn partial_result_store(number: u32) -> Result<(IngestSink, Id), Box<dyn std::error::Error>> {
    let store = IngestSink::default();
    let lineage_id = id(number + 1)?;
    let analysis_id = format!("snapshot-save-partial-{number}");
    store
        .bind_current_analysis(&analysis_id, lineage_id.clone())
        .map_err(|error| format!("lineage bind: {error:?}"))?;
    let mut build = store
        .open_forest_in_lineage(id(number)?, vec![root()], lineage_id)
        .map_err(|error| format!("lineage open: {error:?}"))?;
    build
        .add_directory(Vec::new())
        .map_err(|error| format!("directory: {error:?}"))?;
    let version_id = id(number + 2)?;
    build
        .seal_at(
            version_id.clone(),
            ResultCompleteness::Partial,
            Some(observed_at()?),
        )
        .map_err(|error| format!("partial seal: {error:?}"))?;
    assert!(
        store
            .observe_scan_terminal(&version_id, Some(ScanCompletion::Partial))
            .map_err(|error| format!("terminal observation: {error:?}"))?
    );
    Ok((store, version_id))
}

#[derive(Default)]
struct IndexState {
    active: Option<Id>,
    rows: Vec<SnapshotIndexRow>,
    volumes: Vec<SnapshotIndexVolume>,
    baseline_at: Option<i64>,
    list_seq: i64,
    lookups: usize,
    fail_register: bool,
}

#[derive(Clone, Default)]
struct MemoryIndex(Arc<Mutex<IndexState>>);

impl SnapshotIndexPort for MemoryIndex {
    fn register(
        &mut self,
        row: &SnapshotIndexRow,
        volumes: &[SnapshotIndexVolume],
        baseline_pinned_at_ms: Option<i64>,
    ) -> Result<SnapshotIndexRegisterResult, SnapshotIndexError> {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if state.fail_register {
            return Err(SnapshotIndexError::Storage);
        }
        state.rows.push(row.clone());
        state.volumes.extend_from_slice(volumes);
        state.baseline_at = baseline_pinned_at_ms;
        state.list_seq += 1;
        Ok(SnapshotIndexRegisterResult {
            snapshot_id: row.snapshot_id.clone(),
            list_seq: state.list_seq,
            inserted: true,
        })
    }

    fn find_active_by_source(
        &mut self,
        source_version_id: &Id,
    ) -> Result<Option<Id>, SnapshotIndexError> {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        state.lookups += 1;
        Ok(state.active.clone().or_else(|| {
            state
                .rows
                .iter()
                .find(|row| row.source_version_id == *source_version_id)
                .map(|row| row.snapshot_id.clone())
        }))
    }

    fn list_active(
        &mut self,
        _: usize,
        _: Option<&SnapshotIndexCursor>,
    ) -> Result<SnapshotIndexPage, SnapshotIndexError> {
        let state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        Ok(SnapshotIndexPage {
            rows: state
                .rows
                .iter()
                .cloned()
                .map(|row| SnapshotIndexEntry {
                    row,
                    volumes: state.volumes.clone(),
                    pinned: state.baseline_at.is_some(),
                })
                .collect(),
            list_seq: state.list_seq,
            next_cursor: None,
        })
    }
}

#[derive(Default)]
struct FileState {
    available: Option<u64>,
    unsupported: bool,
    fail_write: bool,
    fail_publish: bool,
    registered_not_durable: bool,
    capacity_queries: usize,
    create_calls: usize,
    temporary_count: usize,
    write_calls: usize,
    finals: BTreeMap<String, Vec<u8>>,
}

#[derive(Clone, Default)]
struct MemoryFiles(Arc<Mutex<FileState>>);

impl MemoryFiles {
    fn configure(&self, update: impl FnOnce(&mut FileState)) {
        update(&mut self.0.lock().unwrap_or_else(|error| error.into_inner()));
    }

    fn state(&self) -> FileStateSnapshot {
        let state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        FileStateSnapshot {
            capacity_queries: state.capacity_queries,
            create_calls: state.create_calls,
            temporary_count: state.temporary_count,
            write_calls: state.write_calls,
            finals: state.finals.clone(),
        }
    }
}

struct FileStateSnapshot {
    capacity_queries: usize,
    create_calls: usize,
    temporary_count: usize,
    write_calls: usize,
    finals: BTreeMap<String, Vec<u8>>,
}

impl SnapshotFilePort for MemoryFiles {
    fn preflight(&self) -> Result<SnapshotStoreStatus, SnapshotFileError> {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        state.capacity_queries += 1;
        Ok(SnapshotStoreStatus {
            available_bytes: state.available,
            no_replace_register: if state.unsupported {
                Err(SnapshotFileError::Unsupported)
            } else {
                Ok(())
            },
        })
    }

    fn create_pending(
        &self,
        snapshot_id: &Id,
    ) -> Result<Box<dyn PendingSnapshotFile + '_>, SnapshotFileError> {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        state.create_calls += 1;
        if state.unsupported {
            return Err(SnapshotFileError::Unsupported);
        }
        state.temporary_count += 1;
        drop(state);
        Ok(Box::new(MemoryPending {
            state: self.0.clone(),
            name: format!("{}.dusnap", snapshot_id.as_str()),
            bytes: Vec::new(),
            closed: false,
        }))
    }

    fn open_registered(
        &self,
        _snapshot_id: &Id,
    ) -> Result<Box<dyn std::io::Read + Send + '_>, SnapshotFileError> {
        Err(SnapshotFileError::Unsupported)
    }

    fn create_capture(
        &self,
        _required_bytes: Option<u64>,
    ) -> Result<
        Box<
            dyn du_core_contracts::snapshot_spool::CaptureSpool<
                    Sealed = Box<dyn du_core_contracts::snapshot_spool::SealedSpool>,
                > + Send
                + '_,
        >,
        SnapshotFileError,
    > {
        Err(SnapshotFileError::Unsupported)
    }
}

struct MemoryPending {
    state: Arc<Mutex<FileState>>,
    name: String,
    bytes: Vec<u8>,
    closed: bool,
}

impl MemoryPending {
    fn close_temp(&mut self) {
        if !self.closed {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state.temporary_count = state.temporary_count.saturating_sub(1);
            self.closed = true;
        }
    }
}

impl Write for MemoryPending {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let fail = {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state.write_calls += 1;
            state.fail_write
        };
        if fail {
            return Err(io::Error::other("injected write failure"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl PendingSnapshotFile for MemoryPending {
    fn publish(mut self: Box<Self>) -> Result<(), SnapshotPublishError> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.fail_publish {
            return Err(SnapshotPublishError::WriteFailed(io::ErrorKind::Other));
        }
        state.finals.insert(self.name.clone(), self.bytes.clone());
        let not_durable = state.registered_not_durable;
        state.temporary_count = state.temporary_count.saturating_sub(1);
        self.closed = true;
        if not_durable {
            Err(SnapshotPublishError::RegisteredNotDurable(
                io::ErrorKind::Other,
            ))
        } else {
            Ok(())
        }
    }
}

impl Drop for MemoryPending {
    fn drop(&mut self) {
        self.close_temp();
    }
}

struct TestIds(AtomicUsize);

impl JobIdentity for TestIds {
    fn next_id(&self) -> Id {
        let number = self.0.fetch_add(1, Ordering::Relaxed) + 1;
        fixed_id(number as u32)
    }
}

struct TestClock;

impl JobClock for TestClock {
    fn now_ms(&self) -> i64 {
        1_700_000_000_000
    }
}

struct TestHistory;

impl JobHistory for TestHistory {
    fn append(&self, _: &JobSummary) -> Result<HistoryAppend, String> {
        Ok(HistoryAppend::Added)
    }
}

struct TestEvents;

impl JobEvents for TestEvents {
    fn publish(&self, _: du_job_runtime::JobEvent) {}
}

struct TestRig {
    command: SaveCommand<MemoryIndex>,
    runtime: Arc<JobRuntime>,
    events: Arc<DesktopEvents>,
}

fn rig(
    store: IngestSink,
    index: MemoryIndex,
    files: MemoryFiles,
    primary_window: bool,
) -> Result<TestRig, Box<dyn std::error::Error>> {
    let runtime = Arc::new(JobRuntime::new(
        Arc::new(TestIds(AtomicUsize::new(100))),
        Arc::new(TestClock),
        Arc::new(TestHistory),
        Arc::new(TestEvents),
    )?);
    runtime.register(JobDescriptor {
        kind_id: KIND.into(),
        version: 1,
        display_name_key: "job.snapshot_save".into(),
        controls: vec![JobControl::Cancel, JobControl::SetPriority],
        safe_points: vec!["batch_end".into()],
        progress_unit: ProgressUnit::Bytes,
        resource_classes: vec![ResourceClass::Output],
        history: HistoryMode::Summary,
    })?;
    let events = Arc::new(DesktopEvents::default());
    events.begin_document_generation();
    events.mark_document_ready();
    let context = Arc::new(SaveContext {
        store,
        index: Arc::new(Mutex::new(index.clone())),
        files: Arc::new(files.clone()),
        queue: Arc::new(SaveQueue {
            workers: Arc::new(crate::workers::Workers::idle(runtime.clone())),
            runtime: runtime.clone(),
            events: events.clone(),
        }),
    });
    Ok(TestRig {
        command: SaveCommand {
            context,
            primary_window,
        },
        runtime,
        events,
    })
}

fn caller() -> Result<CallerCtx, Box<dyn std::error::Error>> {
    Ok(CallerCtx::new(Channel::Gui, Id::parse(CORRELATION)?, None))
}

fn request(version: Id, pin: bool) -> SnapshotSaveRequest {
    SnapshotSaveRequest {
        result_version_id: version,
        pin,
    }
}

fn reason(error: EngineError) -> String {
    error.into_envelope(fixed_id(1)).reason_key
}

fn no_op_submission(version: &Id) -> Submission {
    struct Noop;
    impl JobBody for Noop {
        fn kind_id(&self) -> &str {
            KIND
        }
        fn run(&self, _: Arc<dyn JobContext>) -> JobOutcome {
            outcome(true, JobCounts::default(), None)
        }
    }
    Submission {
        kind_id: KIND.into(),
        deduplication_key: Some(deduplication_key(version)),
        body: Arc::new(Noop),
        priority: JobPriority::Normal,
        scope_summary: "snapshot.save".into(),
        scope_roots: Vec::new(),
        correlation_id: fixed_id(999),
        sample: false,
    }
}

mod admission;
mod save;
