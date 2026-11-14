//! Fake root and directory adapters used by the scan admission tests.
use super::*;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};
use std::time::Duration;

pub(super) struct Roots {
    pub(super) file_id: AtomicUsize,
    pub(super) opens: AtomicUsize,
    pub(super) reads: Arc<AtomicUsize>,
    pub(super) recent_observations: Arc<AtomicUsize>,
    pub(super) recent_opens: Arc<AtomicUsize>,
    pub(super) recent_live_handles: Arc<AtomicUsize>,
    pub(super) recent_unavailable: std::sync::atomic::AtomicBool,
    pub(super) read_handles: Arc<Mutex<Vec<usize>>>,
    pub(super) release: Arc<Mutex<mpsc::Receiver<()>>>,
    pub(super) finish_gate: Option<Arc<Mutex<mpsc::Receiver<()>>>>,
    pub(super) priority: Option<Arc<PriorityState>>,
}

pub(super) struct PriorityState {
    pub(super) open_entered: mpsc::Sender<()>,
    pub(super) visit_entered: mpsc::Sender<()>,
    pub(super) visit_release: Arc<Mutex<mpsc::Receiver<()>>>,
    pub(super) visits: mpsc::Sender<&'static str>,
    pub(super) completed_b: mpsc::Sender<()>,
}

impl ScopeRootPort for Roots {
    fn open_root(&self, path: &[u8]) -> Result<OpenedScopeRoot, PortError> {
        let handle = self.opens.fetch_add(1, Ordering::Relaxed) + 1;
        if path.first() == Some(&b'Z') {
            return Err(PortError {
                kind: PortErrorKind::PermissionDenied,
                reason_key: "scan.path_denied".into(),
                os_code: None,
            });
        }
        let identity = self.file_id.load(Ordering::Acquire);
        let identity = if path.first() == Some(&b'D') {
            identity + 100
        } else {
            identity
        };
        let priority = (path.first() == Some(&b'P'))
            .then(|| self.priority.clone())
            .flatten();
        Ok(OpenedScopeRoot {
            canonical_path: OsPath {
                encoding: PathEncoding::WindowsUtf16le,
                raw: PathBytes::from_bytes(path),
                display: "C:\\Fixture".into(),
                lossy: false,
            },
            volume_id: vec![1],
            file_id: identity.to_le_bytes().to_vec(),
            directory: Box::new(Directory {
                handle,
                read_handles: self.read_handles.clone(),
                reads: self.reads.clone(),
                release: Some(self.release.clone()),
                finish_gate: self.finish_gate.clone(),
                priority,
                priority_label: None,
                priority_sent: false,
            }),
        })
    }

    fn open_root_with_recent(
        &self,
        path: &[u8],
    ) -> Result<
        (
            OpenedScopeRoot,
            du_core_contracts::scan_observation::OptionalRecentScopeObserver,
        ),
        PortError,
    > {
        super::recent::open_with_observer(self, path)
    }
}

struct Directory {
    handle: usize,
    read_handles: Arc<Mutex<Vec<usize>>>,
    reads: Arc<AtomicUsize>,
    release: Option<Arc<Mutex<mpsc::Receiver<()>>>>,
    finish_gate: Option<Arc<Mutex<mpsc::Receiver<()>>>>,
    priority: Option<Arc<PriorityState>>,
    priority_label: Option<&'static str>,
    priority_sent: bool,
}

impl DirectoryObservations for Directory {
    fn next_filtered(
        &mut self,
        decide: &mut dyn FnMut(&[u8]) -> du_core_contracts::scan_observation::FilterDecision,
    ) -> Result<du_core_contracts::scan_observation::FilteredBatch, PortError> {
        use du_core_contracts::scan_observation::{FilterDecision, FilteredBatch};
        if self.release.is_none() {
            return Ok(FilteredBatch::End);
        }
        match decide(&[b'f', 0]) {
            FilterDecision::Admit => self.next_batch().map(FilteredBatch::Entries),
            FilterDecision::Exclude => {
                self.release.take();
                Ok(FilteredBatch::Progress)
            }
            FilterDecision::Undecidable => Ok(FilteredBatch::Undecidable),
        }
    }

    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        self.read_handles
            .lock()
            .map_err(|_| io_error())?
            .push(self.handle);
        self.reads.fetch_add(1, Ordering::Relaxed);
        if let Some(priority) = &self.priority {
            if let Some(label) = self.priority_label {
                if !std::mem::replace(&mut self.priority_sent, true) {
                    if label == "A" {
                        priority.visit_entered.send(()).map_err(|_| io_error())?;
                        priority
                            .visit_release
                            .lock()
                            .map_err(|_| io_error())?
                            .recv_timeout(Duration::from_secs(5))
                            .map_err(|_| io_error())?;
                    }
                    priority.visits.send(label).map_err(|_| io_error())?;
                }
                return Ok(Vec::new());
            }
            if !std::mem::replace(&mut self.priority_sent, true) {
                return Ok(vec![
                    priority_entry(b"A\0", 11, ObservedKind::Directory),
                    priority_entry(b"B\0", 12, ObservedKind::Directory),
                    priority_entry(b"F\0", 13, ObservedKind::File),
                ]);
            }
            return Ok(Vec::new());
        }
        let Some(release) = self.release.take() else {
            if let Some(gate) = self.finish_gate.take() {
                gate.lock()
                    .map_err(|_| io_error())?
                    .recv_timeout(Duration::from_secs(5))
                    .map_err(|_| io_error())?;
            }
            return Ok(Vec::new());
        };
        release
            .lock()
            .map_err(|_| io_error())?
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| io_error())?;
        Ok(vec![ObservedEntry {
            name: vec![b'f', 0],
            enumerated_inode: 2,
            enumerated_kind: ObservedKind::File,
            metadata: Ok(MetadataObservation {
                kind: ObservedKind::File,
                device: 1,
                inode: 2,
                size: Ok(64),
                allocated: None,
                link_count: None,
                space_identity: None,
                modified_at: du_core_contracts::Timestamp::parse("2020-01-02T03:04:05.006Z").ok(),
                native_modified_at: std::num::NonZeroU64::new(132_224_078_450_060_001)
                    .map(du_core_contracts::scan_observation::NativeModifiedTime::WindowsFiletime),
            }),
        }])
    }
}

impl AnchoredDirectory for Directory {
    fn open_child(&self, entry: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        if let Some(priority) = &self.priority {
            let (label, wait) = if entry.name == b"A\0" {
                ("A", true)
            } else if entry.name == b"B\0" {
                ("B", false)
            } else {
                return Err(io_error());
            };
            if wait {
                priority.open_entered.send(()).map_err(|_| io_error())?;
                if let Some(release) = &self.release {
                    release
                        .lock()
                        .map_err(|_| io_error())?
                        .recv_timeout(Duration::from_secs(5))
                        .map_err(|_| io_error())?;
                }
            }
            return Ok(Box::new(Self {
                handle: self.handle,
                read_handles: self.read_handles.clone(),
                reads: self.reads.clone(),
                release: self.release.clone(),
                finish_gate: None,
                priority: self.priority.clone(),
                priority_label: Some(label),
                priority_sent: false,
            }));
        }
        Err(io_error())
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        if self.priority_label == Some("B")
            && let Some(priority) = &self.priority
        {
            let _ = priority.completed_b.send(());
        }
    }
}

fn priority_entry(name: &[u8], inode: u64, kind: ObservedKind) -> ObservedEntry {
    ObservedEntry {
        name: name.to_vec(),
        enumerated_inode: inode,
        enumerated_kind: kind.clone(),
        metadata: Ok(MetadataObservation {
            kind,
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

fn io_error() -> PortError {
    PortError {
        kind: PortErrorKind::Io,
        reason_key: "os.query_failed".into(),
        os_code: None,
    }
}
