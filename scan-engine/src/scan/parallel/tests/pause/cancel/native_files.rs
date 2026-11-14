//! Windows filesystem observations through a test adapter, not platform-os.
use super::*;
use std::{collections::BTreeSet, fs, path::PathBuf, time::Instant};

const FILES: usize = 600;
const BATCH: usize = 256;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Result<Self, String> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let path = std::env::temp_dir().join(format!("du-f02-{}-{nonce}", std::process::id()));
        fs::create_dir(&path).map_err(|e| e.to_string())?;
        let fixture = Self(path);
        for n in 0..FILES {
            fs::write(fixture.0.join(format!("item-{n:04}")), b"x").map_err(|e| e.to_string())?;
        }
        Ok(fixture)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for n in 0..FILES {
            let _ = fs::remove_file(self.0.join(format!("item-{n:04}")));
        }
        let _ = fs::remove_dir(&self.0);
    }
}

struct WindowsFiles {
    read: fs::ReadDir,
    calls: Arc<AtomicUsize>,
    seen: Arc<Mutex<BTreeSet<Vec<u8>>>>,
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}
impl DirectoryObservations for WindowsFiles {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        let mut batch = Vec::new();
        for item in self.read.by_ref().take(BATCH) {
            let item = item.map_err(|_| fail())?;
            let meta = item.metadata().map_err(|_| fail())?;
            let name = item.file_name().to_string_lossy().into_owned();
            let index = name
                .strip_prefix("item-")
                .and_then(|v| v.parse::<u64>().ok())
                .ok_or_else(fail)?
                + 1;
            let name = native(&name);
            self.seen.lock().map_err(|_| fail())?.insert(name.clone());
            batch.push(ObservedEntry {
                name,
                // File identity is synthetic; the entries and lengths come from NTFS.
                enumerated_inode: index,
                enumerated_kind: ObservedKind::File,
                metadata: Ok(MetadataObservation {
                    kind: ObservedKind::File,
                    device: 1,
                    inode: index,
                    size: Ok(meta.len()),
                    allocated: None,
                    link_count: None,
                    space_identity: None,
                    modified_at: None,
                    native_modified_at: None,
                }),
            });
        }
        if call == 1 {
            self.entered.send(()).map_err(|_| fail())?;
            self.release.recv().map_err(|_| fail())?;
        }
        Ok(batch)
    }
}
impl AnchoredDirectory for WindowsFiles {
    fn observed_identity(&self) -> Option<du_core_contracts::scan_observation::HandleIdentity> {
        crate::scan::test_identity(self)
    }
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(fail())
    }
}

#[test]
fn windows_files_cancel_after_second_batch_preserves_partial_and_returned_entries()
-> Result<(), String> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker lock")?;
    let fixture = Fixture::new()?;
    let control = Arc::new(Control::default());
    let context = Arc::new(PauseContext(control.clone()));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::new(Mutex::new(BTreeSet::new()));
    let store = IngestSink::default();
    let build = store.open_forest(
        Ids(AtomicUsize::new(99)).next_id(),
        vec![observed_root(&fixture.0.to_string_lossy(), 1)],
    );
    let root = WindowsFiles {
        read: fs::read_dir(&fixture.0).map_err(|e| e.to_string())?,
        calls: calls.clone(),
        seen: seen.clone(),
        entered: entered_tx,
        release: release_rx,
    };
    let ctx = context.clone();
    let worker = std::thread::spawn(move || {
        let ids = Ids(AtomicUsize::new(1));
        collect_open_forest(build, vec![Box::new(root)], &ids, &*ctx, 1, &|_| true)
    });
    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let partial = loop {
        if let Some(version) = store.pin().map_err(|e| format!("{e:?}"))? {
            break version;
        }
        if Instant::now() >= deadline {
            return Err("missing partial".into());
        }
        std::thread::yield_now();
    };
    assert_eq!(partial.completeness(), ResultCompleteness::Partial);
    assert_eq!(partial.items().len(), BATCH + 1);
    {
        let mut state = control.state.lock().map_err(|_| "state")?;
        state.cancelled = true;
        control.changed.notify_all();
    }
    release_tx.send(()).map_err(|e| e.to_string())?;
    let outcome = worker.join().map_err(|_| "worker panic")?;
    assert!(!outcome.completed_scope);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let final_version = store
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing final")?;
    assert_eq!(final_version.completeness(), ResultCompleteness::Cancelled);
    assert_eq!(final_version.items().len(), BATCH * 2 + 1);
    let names: BTreeSet<_> = final_version
        .items()
        .iter()
        .filter(|i| i.parent.is_some())
        .map(|i| i.name.clone())
        .collect();
    assert_eq!(names, *seen.lock().map_err(|_| "seen")?);
    assert_eq!(partial.items().len(), BATCH + 1);
    eprintln!(
        "f02 windows files={FILES} batch={BATCH} returned={} final={} calls={}",
        names.len(),
        final_version.items().len(),
        calls.load(Ordering::SeqCst)
    );
    Ok(())
}
