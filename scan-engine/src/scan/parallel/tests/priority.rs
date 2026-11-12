use super::*;
use du_core_contracts::{
    job::{JobCounts, JobFlow},
    result::ResultVersionRead,
    scan_observation::OpenedScopeRoot,
};
use std::{
    sync::{Condvar, mpsc},
    time::Duration,
};

type TestError = Box<dyn std::error::Error>;

struct PinnedArea {
    version: Arc<du_analysis_index::ResultVersion>,
    item: ItemRef,
    components: Vec<Vec<u8>>,
}

struct Fixture {
    job: crate::scan_job::MetadataScan,
    handle: ScanControlHandle,
    store: IngestSink,
    context: Arc<SingleFlowContext>,
    open_gate: Arc<Gate>,
    visit_gate: Arc<Gate>,
    open_entered: mpsc::Receiver<()>,
    visit_entered: mpsc::Receiver<()>,
    visits: mpsc::Receiver<&'static str>,
    scope: Id,
    lineage: Id,
}

struct Gate {
    released: Mutex<bool>,
    changed: Condvar,
    entered: mpsc::Sender<()>,
    signalled: AtomicBool,
}

impl Gate {
    fn block(&self) -> Result<(), PortError> {
        if !self.signalled.swap(true, Ordering::AcqRel) {
            self.entered.send(()).map_err(|_| test_io_error())?;
        }
        let mut released = self.released.lock().map_err(|_| test_io_error())?;
        while !*released {
            released = self.changed.wait(released).map_err(|_| test_io_error())?;
        }
        Ok(())
    }

    fn release(&self) -> Result<(), Box<dyn std::error::Error>> {
        *self.released.lock().map_err(|_| "gate lock poisoned")? = true;
        self.changed.notify_all();
        Ok(())
    }
}

struct PriorityRoot {
    sent: bool,
    gate: Arc<Gate>,
    visit_gate: Arc<Gate>,
    visits: mpsc::Sender<&'static str>,
}

impl DirectoryObservations for PriorityRoot {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        if std::mem::replace(&mut self.sent, true) {
            return Ok(Vec::new());
        }
        Ok(vec![
            directory_entry(b"A\0", 11),
            directory_entry(b"B\0", 12),
        ])
    }
}

impl AnchoredDirectory for PriorityRoot {
    fn open_child(&self, entry: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        if entry.name == b"A\0" {
            self.gate.block()?;
            Ok(Box::new(PriorityChild {
                visited: false,
                label: "A",
                gate: Some(self.visit_gate.clone()),
                visits: self.visits.clone(),
            }))
        } else {
            Ok(Box::new(PriorityChild {
                visited: false,
                label: "B",
                gate: None,
                visits: self.visits.clone(),
            }))
        }
    }
}

struct PriorityChild {
    visited: bool,
    label: &'static str,
    gate: Option<Arc<Gate>>,
    visits: mpsc::Sender<&'static str>,
}

impl DirectoryObservations for PriorityChild {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        if !std::mem::replace(&mut self.visited, true) {
            if let Some(gate) = &self.gate {
                gate.block()?;
            }
            self.visits.send(self.label).map_err(|_| test_io_error())?;
        }
        Ok(Vec::new())
    }
}

impl AnchoredDirectory for PriorityChild {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(test_io_error())
    }
}

struct SingleFlowContext {
    occupied: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
}

struct SingleFlow(Arc<AtomicBool>);

impl JobFlow for SingleFlow {
    fn report_item(&self, _: &[u8]) -> Result<(), JobFlowError> {
        Ok(())
    }

    fn finish(self: Box<Self>, _: JobCounts) {
        self.0.store(false, Ordering::Release);
    }
}

impl JobContext for SingleFlowContext {
    fn checkpoint(&self) -> Checkpoint {
        if self.cancelled.load(Ordering::Acquire) {
            Checkpoint::Cancel
        } else {
            Checkpoint::Continue
        }
    }

    fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(JobFlowError::Cancelled);
        }
        self.occupied
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Box::new(SingleFlow(self.occupied.clone())) as Box<dyn JobFlow>)
            .map_err(|_| JobFlowError::Busy)
    }
}

fn directory_entry(name: &[u8], inode: u64) -> ObservedEntry {
    ObservedEntry {
        name: name.to_vec(),
        enumerated_inode: inode,
        enumerated_kind: ObservedKind::Directory,
        metadata: Ok(MetadataObservation {
            kind: ObservedKind::Directory,
            device: 1,
            inode,
            size: Ok(0),
            allocated: None,
            link_count: None,
            space_identity: None,
            modified_at: None,
            native_modified_at: None,
        }),
    }
}

fn test_io_error() -> PortError {
    PortError {
        kind: PortErrorKind::Io,
        reason_key: "os.query_failed".into(),
        os_code: None,
    }
}

fn opened_root(directory: PriorityRoot) -> OpenedScopeRoot {
    OpenedScopeRoot {
        canonical_path: OsPath {
            encoding: PathEncoding::WindowsUtf16le,
            raw: PathBytes::from_bytes(&native("C:\\root")),
            display: "C:\\root".into(),
            lossy: false,
        },
        volume_id: vec![1],
        file_id: vec![1],
        directory: Box::new(directory),
    }
}

fn pinned_area(store: &IngestSink) -> Result<PinnedArea, TestError> {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let version = loop {
        if let Some(version) = store
            .pin()
            .map_err(|error| format!("pin current result: {error:?}"))?
            && version.items().iter().any(|item| {
                item.kind == du_core_contracts::result::ResultItemKind::Directory
                    && item.name == native("B")
            })
        {
            break version;
        }
        if std::time::Instant::now() >= deadline {
            return Err("missing current partial result with B directory".into());
        }
        std::thread::yield_now();
    };
    assert_eq!(version.completeness(), ResultCompleteness::Partial);
    let item = version
        .items()
        .iter()
        .find(|item| {
            item.kind == du_core_contracts::result::ResultItemKind::Directory
                && item.name == native("B")
        })
        .ok_or("missing selectable B directory")?;
    let item_ref = ItemRef {
        result_version: version.version_id().clone(),
        item_no: item.item_no,
    };
    let (root, _) = version
        .observed_root_for(&item_ref)
        .map_err(|error| format!("resolve observed root: {error:?}"))?;
    assert_eq!(root, 0);
    let (_, parts) = version
        .observed_path_parts(&item_ref)
        .map_err(|error| format!("resolve observed path: {error:?}"))?;
    let parts = parts.map(<[u8]>::to_vec).collect();
    Ok(PinnedArea {
        version,
        item: item_ref,
        components: parts,
    })
}

fn scan_fixture() -> Result<Fixture, TestError> {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (visit_entered_tx, visit_entered_rx) = mpsc::channel();
    let (visits_tx, visits_rx) = mpsc::channel();
    let gate = Arc::new(Gate {
        released: Mutex::new(false),
        changed: Condvar::new(),
        entered: entered_tx,
        signalled: AtomicBool::new(false),
    });
    let visit_gate = Arc::new(Gate {
        released: Mutex::new(false),
        changed: Condvar::new(),
        entered: visit_entered_tx,
        signalled: AtomicBool::new(false),
    });
    let ids = Arc::new(Ids(AtomicUsize::new(20)));
    let scope = ids.next_id();
    let lineage = ids.next_id();
    let store = IngestSink::default();
    store
        .bind_current_analysis("priority-test-scope", lineage.clone())
        .map_err(|error| format!("bind test lineage: {error:?}"))?;
    let job = crate::scan_job::MetadataScan::new_roots(
        vec![opened_root(PriorityRoot {
            sent: false,
            gate: gate.clone(),
            visit_gate: visit_gate.clone(),
            visits: visits_tx,
        })],
        store.clone(),
        scope.clone(),
        ids,
    )
    .with_priority_control()
    .with_lineage(lineage.clone());
    let handle = job
        .control_handle()
        .ok_or("priority control was not enabled")?;
    let context = Arc::new(SingleFlowContext {
        occupied: Arc::new(AtomicBool::new(false)),
        cancelled: Arc::new(AtomicBool::new(false)),
    });
    Ok(Fixture {
        job,
        handle,
        store,
        context,
        open_gate: gate,
        visit_gate,
        open_entered: entered_rx,
        visit_entered: visit_entered_rx,
        visits: visits_rx,
        scope,
        lineage,
    })
}

#[test]
fn live_priority_reorders_existing_work_from_the_current_partial_result() -> Result<(), TestError> {
    let _guard = WORKER_TEST_LOCK
        .lock()
        .map_err(|_| "worker lock poisoned")?;
    let Fixture {
        job,
        handle,
        store,
        context,
        open_gate,
        visit_gate,
        open_entered,
        visit_entered,
        visits,
        scope,
        lineage,
    } = scan_fixture()?;
    assert_eq!(
        handle.prioritize(0, &[native("B")]),
        Err(ScanControlError::NotActive)
    );
    let worker_context = context.clone();
    let worker = std::thread::spawn(move || job.run(worker_context));
    if let Err(error) = open_entered.recv_timeout(Duration::from_secs(5)) {
        let result = worker.join().map_err(|_| "scan worker panicked")?;
        return Err(format!("gate was not reached ({error}); outcome {result:?}").into());
    }

    let area = pinned_area(&store)?;
    assert_eq!(area.version.scope_version_id(), &scope);
    assert_eq!(area.version.lineage_id(), Some(&lineage));
    assert_eq!(
        handle.prioritize(1, &area.components),
        Err(ScanControlError::OutOfScope)
    );
    let accepted = handle
        .prioritize(0, &area.components)
        .map_err(|error| format!("prioritize region: {error:?}"))?;
    assert_eq!(
        accepted,
        ScanPriorityOutcome::Accepted {
            remaining_directories: 1
        }
    );
    assert_eq!(
        handle
            .prioritize(0, &area.components)
            .map_err(|error| format!("repeat priority: {error:?}"))?,
        ScanPriorityOutcome::NoEffect {
            same_region: true,
            remaining_directories: 1,
        }
    );

    open_gate.release()?;
    assert_eq!(visits.recv_timeout(Duration::from_secs(5))?, "B");
    visit_entered.recv_timeout(Duration::from_secs(5))?;
    assert_eq!(
        handle
            .prioritize(0, &area.components)
            .map_err(|error| format!("completed area request: {error:?}"))?,
        ScanPriorityOutcome::NoEffect {
            same_region: false,
            remaining_directories: 0,
        }
    );
    visit_gate.release()?;
    assert_eq!(visits.recv_timeout(Duration::from_secs(5))?, "A");
    let outcome = worker.join().map_err(|_| "scan worker panicked")?;
    assert!(outcome.completed_scope);
    assert_eq!(
        handle.prioritize(0, &area.components),
        Err(ScanControlError::AlreadyTerminal)
    );
    assert_eq!(area.item.result_version, area.version.version_id().clone());
    Ok(())
}

#[test]
fn cancellation_releases_the_job_bound_priority_handle() -> Result<(), TestError> {
    let _guard = WORKER_TEST_LOCK
        .lock()
        .map_err(|_| "worker lock poisoned")?;
    let Fixture {
        job,
        handle,
        store,
        context,
        open_gate,
        open_entered,
        ..
    } = scan_fixture()?;
    let worker_context = context.clone();
    let worker = std::thread::spawn(move || job.run(worker_context));
    open_entered.recv_timeout(Duration::from_secs(5))?;
    let area = pinned_area(&store)?;
    assert!(matches!(
        handle
            .prioritize(0, &area.components)
            .map_err(|error| format!("prioritize region: {error:?}"))?,
        ScanPriorityOutcome::Accepted { .. }
    ));
    context.cancelled.store(true, Ordering::Release);
    open_gate.release()?;
    let outcome = worker.join().map_err(|_| "scan worker panicked")?;
    assert!(!outcome.completed_scope);
    assert_eq!(
        handle.prioritize(0, &area.components),
        Err(ScanControlError::AlreadyTerminal)
    );
    Ok(())
}

mod queue;
