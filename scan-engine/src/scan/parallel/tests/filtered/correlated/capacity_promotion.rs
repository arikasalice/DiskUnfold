use super::super::*;
use super::{budget_exhaustion::RejectChildAttempts, fixtures::*};
use du_analysis_index::{IngestSink, ResultVersion, test_hooks::FolderEvidenceBudgetHold};
use du_core_contracts::scan_observation::ObservedValue;
use du_core_contracts::{
    SizeAmount, Timestamp,
    job::{Checkpoint, JobContext, JobFlow, JobFlowError},
    ports::{PortError, PortErrorKind},
    result::{ResultCompleteness, ResultVersionRead},
    result_events::{ResultVersionNotice, ResultVersionPublisher},
    scan_observation::{
        AnchoredDirectory, CorrelatedDirectoryEvent, DirectoryObservations, FilterCallbackPhase,
        FilterDecision, FilteredBatch, HandleIdentity, MetadataObservation, ObservedEntry,
        ObservedKind, TimedObservedEntry,
    },
};
use std::{
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, Instant},
};

struct TimedContext {
    inner: Context,
    started: Instant,
}

impl JobContext for TimedContext {
    fn checkpoint(&self) -> Checkpoint {
        self.inner.checkpoint()
    }

    fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
        self.inner.try_start_flow()
    }

    fn monotonic_now(&self) -> Option<Duration> {
        Some(self.started.elapsed())
    }
}

struct CapacityPromotionPublisher {
    sink: OnceLock<IngestSink>,
    release_root: Mutex<Option<Sender<()>>>,
    release_child: Mutex<Option<Sender<()>>>,
    capture_partial: Sender<Arc<ResultVersion>>,
    evidence_hold: Mutex<Option<FolderEvidenceBudgetHold>>,
    held_evidence: AtomicBool,
    captured_partial: AtomicBool,
    failed: AtomicBool,
}

impl CapacityPromotionPublisher {
    fn signal(signal: &Mutex<Option<Sender<()>>>) {
        if let Ok(mut signal) = signal.lock()
            && let Some(signal) = signal.take()
        {
            let _ = signal.send(());
        }
    }
}

impl ResultVersionPublisher for CapacityPromotionPublisher {
    fn publish_result_version(&self, _: ResultVersionNotice) {
        let Some(sink) = self.sink.get() else {
            self.failed.store(true, Ordering::Release);
            return;
        };
        let version = match sink.pin() {
            Ok(Some(version)) => version,
            _ => {
                self.failed.store(true, Ordering::Release);
                return;
            }
        };

        if version.items().iter().any(|item| item.name == b"folder")
            && !self.held_evidence.swap(true, Ordering::AcqRel)
        {
            match sink.hold_folder_evidence_budget() {
                Some(hold) => match self.evidence_hold.lock() {
                    Ok(mut slot) => *slot = Some(hold),
                    Err(_) => self.failed.store(true, Ordering::Release),
                },
                None => self.failed.store(true, Ordering::Release),
            }
            Self::signal(&self.release_root);
        }

        let has_item = |name: &[u8]| version.items().iter().any(|item| item.name == name);
        if version.completeness() == ResultCompleteness::Partial
            && has_item(b"folder")
            && has_item(b"leaf-a")
            && has_item(b"leaf-b")
            && version.folder_attempt_summaries().is_none()
            && !self.captured_partial.swap(true, Ordering::AcqRel)
        {
            if self.capture_partial.send(Arc::clone(&version)).is_err() {
                self.failed.store(true, Ordering::Release);
            }
            Self::signal(&self.release_child);
        }
    }
}

struct FolderPromotionRoot {
    step: u8,
    child: bool,
    release_root: Arc<Mutex<Receiver<()>>>,
    release_child: Arc<Mutex<Receiver<()>>>,
}

fn timeout() -> PortError {
    PortError {
        kind: PortErrorKind::Io,
        reason_key: "test.gate_timeout".into(),
        os_code: None,
    }
}

fn wait(gate: &Mutex<Receiver<()>>) -> Result<(), PortError> {
    gate.lock()
        .map_err(|_| timeout())?
        .recv_timeout(Duration::from_secs(5))
        .map(|_| ())
        .map_err(|_| timeout())
}

fn leaf(name: &[u8], inode: u64) -> TimedObservedEntry {
    ObservedEntry {
        name: name.to_vec(),
        enumerated_inode: inode,
        enumerated_kind: ObservedKind::File,
        metadata: Ok(MetadataObservation {
            kind: ObservedKind::File,
            device: 1,
            inode,
            size: Ok(4),
            allocated: ObservedValue::NotApplicable,
            link_count: ObservedValue::NotApplicable,
            space_identity: ObservedValue::NotApplicable,
            modified_at: ObservedValue::NotApplicable,
            native_modified_at: None,
        }),
    }
    .into()
}

impl DirectoryObservations for FolderPromotionRoot {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Err(timeout())
    }

    fn next_filtered(
        &mut self,
        _: &mut dyn FnMut(&[u8]) -> FilterDecision,
    ) -> Result<FilteredBatch, PortError> {
        Err(timeout())
    }

    fn begin_correlated_filtered(&mut self) -> Result<(), PortError> {
        Ok(())
    }

    fn next_correlated_event(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
        _: &dyn Fn() -> Option<Timestamp>,
    ) -> Result<CorrelatedDirectoryEvent, PortError> {
        self.step = self.step.saturating_add(1);
        let event = match (self.child, self.step) {
            (false, 1) => CorrelatedDirectoryEvent::FilterCallback {
                child_id: 0,
                raw_name: b"folder".to_vec(),
                decision: include(b"folder"),
                phase: FilterCallbackPhase::BeforeMetadata,
            },
            (false, 2) => CorrelatedDirectoryEvent::Observation {
                child_id: 0,
                entry: directory_observation(b"folder"),
            },
            (false, 3) => {
                wait(&self.release_root)?;
                CorrelatedDirectoryEvent::End {
                    child_count_exclusive: 1,
                }
            }
            (true, 1) => CorrelatedDirectoryEvent::FilterCallback {
                child_id: 0,
                raw_name: b"leaf-a".to_vec(),
                decision: include(b"leaf-a"),
                phase: FilterCallbackPhase::BeforeMetadata,
            },
            (true, 2) => CorrelatedDirectoryEvent::Observation {
                child_id: 0,
                entry: leaf(b"leaf-a", 8),
            },
            (true, 3) => CorrelatedDirectoryEvent::FilterCallback {
                child_id: 1,
                raw_name: b"leaf-b".to_vec(),
                decision: include(b"leaf-b"),
                phase: FilterCallbackPhase::BeforeMetadata,
            },
            (true, 4) => CorrelatedDirectoryEvent::Observation {
                child_id: 1,
                entry: leaf(b"leaf-b", 9),
            },
            (true, 5) => {
                wait(&self.release_child)?;
                CorrelatedDirectoryEvent::End {
                    child_count_exclusive: 2,
                }
            }
            _ => return Err(timeout()),
        };
        Ok(event)
    }
}

impl AnchoredDirectory for FolderPromotionRoot {
    fn observed_identity(&self) -> Option<HandleIdentity> {
        crate::scan::test_identity(self)
    }

    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Ok(Box::new(Self {
            step: 0,
            child: true,
            release_root: Arc::clone(&self.release_root),
            release_child: Arc::clone(&self.release_child),
        }))
    }
}

#[test]
fn exhausted_snapshot_with_rejected_child_attempt_keeps_partial_folder_unpromoted()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = worker_test_lock();
    let _reject = RejectChildAttempts::enable();
    let ids = Ids(AtomicUsize::new(1));
    let (release_root_tx, release_root_rx) = mpsc::channel();
    let (release_child_tx, release_child_rx) = mpsc::channel();
    let (capture_tx, capture_rx) = mpsc::channel();
    let publisher = Arc::new(CapacityPromotionPublisher {
        sink: OnceLock::new(),
        release_root: Mutex::new(Some(release_root_tx)),
        release_child: Mutex::new(Some(release_child_tx)),
        capture_partial: capture_tx,
        evidence_hold: Mutex::new(None),
        held_evidence: AtomicBool::new(false),
        captured_partial: AtomicBool::new(false),
        failed: AtomicBool::new(false),
    });
    let sink = IngestSink::with_version_publisher(publisher.clone());
    publisher
        .sink
        .set(sink.clone())
        .map_err(|_| "publisher sink was already bound")?;
    let lineage = ids.next_id();
    sink.bind_current_analysis("f02-folder-promotion-capacity", lineage.clone())
        .map_err(|error| format!("bind analysis lineage: {error:?}"))?;
    let build = sink.open_forest_in_lineage(
        ids.next_id(),
        vec![super::super::super::observed_root("C:\\R0", 1)],
        lineage,
    );
    let root = FolderPromotionRoot {
        step: 0,
        child: false,
        release_root: Arc::new(Mutex::new(release_root_rx)),
        release_child: Arc::new(Mutex::new(release_child_rx)),
    };
    let context = TimedContext {
        inner: Context {
            completed: Arc::new(AtomicUsize::new(0)),
            abandoned: Arc::new(AtomicUsize::new(0)),
        },
        started: Instant::now(),
    };
    let outcome = crate::scan::parallel::collect_open_forest_filtered(
        build,
        vec![Box::new(root)],
        &ids,
        &context,
        1,
        &|_, _, _| FilterDecision::Admit,
        crate::scan::parallel::DevicePlan {
            groups: vec![0],
            limit: 1,
        },
    );
    assert!(outcome.result_version_id.is_some());
    let partial = capture_rx
        .recv_timeout(Duration::from_secs(8))
        .map_err(|error| {
            let rejected = crate::scan::parallel::publisher::REJECTED_CHILD_ATTEMPTS
                .with(std::cell::Cell::get);
            format!(
                "evidence-less partial was not captured (held={}, captured={}, failed={}, child_rejections={rejected}): {error}",
                publisher.held_evidence.load(Ordering::Acquire),
                publisher.captured_partial.load(Ordering::Acquire),
                publisher.failed.load(Ordering::Acquire)
            )
        })?;
    assert!(publisher.held_evidence.load(Ordering::Acquire));
    assert!(publisher.captured_partial.load(Ordering::Acquire));
    assert!(!publisher.failed.load(Ordering::Acquire));
    assert!(
        crate::scan::parallel::publisher::REJECTED_CHILD_ATTEMPTS.with(std::cell::Cell::get) >= 1
    );
    assert_eq!(partial.completeness(), ResultCompleteness::Partial);
    assert!(partial.folder_attempt_summaries().is_none());
    assert!(partial.items().iter().any(|item| item.name == b"folder"));
    assert!(partial.items().iter().any(|item| item.name == b"leaf-a"));
    assert!(partial.items().iter().any(|item| item.name == b"leaf-b"));
    let folder = partial
        .items()
        .iter()
        .find(|item| item.name == b"folder")
        .ok_or("missing folder result item")?;
    let aggregate = partial
        .space(folder.item_no)
        .ok_or("missing folder aggregate")?;
    assert_eq!(aggregate.completeness, ResultCompleteness::Partial);
    assert!(matches!(
        &aggregate.logical.amount,
        SizeAmount::LowerBound { .. }
    ));
    assert!(partial.scan_issues().is_empty());
    Ok(())
}
