use super::*;
use du_core_contracts::{
    Timestamp,
    scan_observation::{FilterDecision, TimedFilteredBatch, TimedObservedEntry},
};

struct TimedRoot {
    step: usize,
    probes: Arc<AtomicUsize>,
    captured: Arc<Mutex<Option<Timestamp>>>,
    mismatch_time: Arc<Mutex<Option<Timestamp>>>,
}
impl DirectoryObservations for TimedRoot {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Ok(Vec::new())
    }
    fn next_filtered_timed(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
        clock: &dyn Fn() -> Option<Timestamp>,
    ) -> Result<TimedFilteredBatch, PortError> {
        self.step += 1;
        let name: &[u8] = match self.step {
            1 => b"skip",
            2 => b"keep",
            3 => b"bad",
            4 => b"mismatch",
            5 => b"foreign",
            6 => b"missing",
            _ => return Ok(TimedFilteredBatch::End),
        };
        if include(name) == FilterDecision::Exclude {
            return Ok(TimedFilteredBatch::Progress);
        }
        self.probes.fetch_add(1, Ordering::SeqCst);
        let mut root = SmallRoot {
            fail: false,
            done: false,
            name: name.to_vec(),
        };
        let mut entry = root.next_batch()?.remove(0);
        if let Ok(meta) = &mut entry.metadata {
            if name != b"missing" {
                meta.space_identity = Some(du_core_contracts::scan_observation::SpaceIdentity {
                    volume: if name == b"foreign" { vec![2] } else { vec![1] },
                    file: vec![7],
                });
            }
            meta.native_modified_at = std::num::NonZeroU64::new(123_456_789)
                .map(du_core_contracts::scan_observation::NativeModifiedTime::WindowsFiletime);
        }
        if name == b"bad" {
            entry.metadata = Err(PortError {
                kind: PortErrorKind::PermissionDenied,
                reason_key: "os.denied".into(),
                os_code: None,
            });
        }
        if name == b"mismatch"
            && let Ok(meta) = &mut entry.metadata
        {
            meta.inode += 1;
        }
        let observed_at = if entry.metadata.is_ok() {
            clock()
        } else {
            None
        };
        if name == b"keep" && observed_at.is_some() {
            *self.captured.lock().map_err(|_| PortError {
                kind: PortErrorKind::Io,
                reason_key: "clock.lock".into(),
                os_code: None,
            })? = observed_at.clone();
        }
        if name == b"mismatch" {
            *self.mismatch_time.lock().map_err(|_| PortError {
                kind: PortErrorKind::Io,
                reason_key: "clock.lock".into(),
                os_code: None,
            })? = observed_at.clone();
        }
        Ok(TimedFilteredBatch::Entries(vec![TimedObservedEntry {
            entry,
            observed_at,
        }]))
    }
}
impl AnchoredDirectory for TimedRoot {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "os.unsupported".into(),
            os_code: None,
        })
    }
}
struct Clocked {
    ticks: AtomicUsize,
}
impl JobContext for Clocked {
    fn checkpoint(&self) -> Checkpoint {
        Checkpoint::Continue
    }
    fn utc_now(&self) -> Option<Timestamp> {
        Timestamp::from_unix_millis(self.ticks.fetch_add(1, Ordering::SeqCst) as i64)
    }
    fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
        Ok(Box::new(Flow {
            completed: Arc::new(AtomicUsize::new(0)),
            abandoned: Arc::new(AtomicUsize::new(0)),
            finished: false,
        }))
    }
}
#[test]
fn injected_clock_is_recorded_at_fake_observation_not_publication()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker lock")?;
    let ids = Ids(AtomicUsize::new(1));
    let store = IngestSink::default();
    let probes = Arc::new(AtomicUsize::new(0));
    let captured = Arc::new(Mutex::new(None));
    let mismatch_time = Arc::new(Mutex::new(None));
    let roots: Vec<Box<dyn AnchoredDirectory>> = vec![Box::new(TimedRoot {
        step: 0,
        probes: probes.clone(),
        captured: captured.clone(),
        mismatch_time: mismatch_time.clone(),
    })];
    let context = Clocked {
        ticks: AtomicUsize::new(10),
    };
    let result = collect_open_forest_filtered(
        store.open_forest(ids.next_id(), vec![observed_root("C:\\A", 1)]),
        roots,
        &ids,
        &context,
        1,
        &|_, _, name| {
            if name == b"skip" {
                FilterDecision::Exclude
            } else {
                FilterDecision::Admit
            }
        },
        DevicePlan {
            groups: vec![0],
            limit: 1,
        },
    );
    assert!(result.result_version_id.is_some());
    let version = store
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing result")?;
    let keep = version
        .items()
        .iter()
        .find(|item| item.name == b"keep")
        .ok_or("missing keep")?;
    let bad = version
        .items()
        .iter()
        .find(|item| item.name == b"bad")
        .ok_or("missing failure")?;
    let measured = captured
        .lock()
        .map_err(|_| "clock lock")?
        .clone()
        .ok_or("missing observed time")?;
    assert_eq!(version.item_observed_at(keep.item_no), Some(&measured));
    assert_eq!(
        version.item_native_modified_at(keep.item_no),
        std::num::NonZeroU64::new(123_456_789)
            .map(du_core_contracts::scan_observation::NativeModifiedTime::WindowsFiletime)
    );
    assert_eq!(version.item_observed_at(bad.item_no), None);
    assert_eq!(version.item_native_modified_at(bad.item_no), None);
    let reference = |item: &du_core_contracts::result::ResultItem| ItemRef {
        result_version: version.version_id().clone(),
        item_no: item.item_no,
    };
    assert_eq!(
        version.item_enumerated_inode(&reference(keep)),
        Ok(std::num::NonZeroU64::new(7))
    );
    assert_eq!(version.item_enumerated_inode(&reference(bad)), Ok(None));
    assert_missing_enumeration(&version)?;
    assert_ne!(
        version.item_observed_at(keep.item_no),
        version.observed_at()
    );
    let mismatch = version
        .items()
        .iter()
        .find(|item| item.name == b"mismatch")
        .ok_or("missing mismatch")?;
    assert_eq!(version.item_observed_at(mismatch.item_no), None);
    assert_eq!(version.item_native_modified_at(mismatch.item_no), None);
    assert_eq!(
        version.item_enumerated_inode(&reference(mismatch)),
        Ok(None)
    );
    assert!(!version.items().iter().any(|item| item.name == b"skip"));
    let uncertain = version
        .scan_issues()
        .iter()
        .find(|issue| issue.relative == vec![b"mismatch".to_vec()])
        .ok_or("missing mismatch issue")?;
    assert_eq!(
        uncertain.observed_at,
        *mismatch_time.lock().map_err(|_| "clock lock")?
    );
    assert!(uncertain.observed_at.is_some());
    let failure = version
        .scan_issues()
        .iter()
        .find(|issue| issue.relative == vec![b"bad".to_vec()])
        .ok_or("missing failed issue")?;
    assert_eq!(failure.observed_at, None);
    assert_eq!(probes.load(Ordering::SeqCst), 5);
    assert_version_replacement(&store, &ids, &version, &reference(keep))?;
    assert!(!version.items().iter().any(|item| item.name == b"skip"));
    Ok(())
}

fn assert_version_replacement(
    store: &IngestSink,
    ids: &Ids,
    version: &du_analysis_index::ResultVersion,
    item: &ItemRef,
) -> Result<(), Box<dyn std::error::Error>> {
    let wrong = ItemRef {
        result_version: Id::from_v7_bytes([9; 16]),
        item_no: item.item_no,
    };
    assert!(version.item_enumerated_inode(&wrong).is_err());
    let mut replacement = store.open(ids.next_id()).map_err(|e| format!("{e:?}"))?;
    replacement
        .add_directory(vec![])
        .map_err(|e| format!("{e:?}"))?;
    let replacement = replacement
        .seal(
            ids.next_id(),
            du_core_contracts::result::ResultCompleteness::Complete,
        )
        .map_err(|e| format!("{e:?}"))?;
    assert!(replacement.item_enumerated_inode(item).is_err());
    assert_eq!(
        version.item_enumerated_inode(item),
        Ok(std::num::NonZeroU64::new(7))
    );
    Ok(())
}

fn assert_missing_enumeration(
    version: &du_analysis_index::ResultVersion,
) -> Result<(), Box<dyn std::error::Error>> {
    for name in [b"foreign".as_slice(), b"missing".as_slice()] {
        let item = version
            .items()
            .iter()
            .find(|item| item.name == name)
            .ok_or("missing fixture")?;
        let reference = ItemRef {
            result_version: version.version_id().clone(),
            item_no: item.item_no,
        };
        assert_eq!(version.item_enumerated_inode(&reference), Ok(None));
    }
    Ok(())
}

#[test]
fn legacy_adapter_does_not_fabricate_item_time() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker lock")?;
    let ids = Ids(AtomicUsize::new(1));
    let store = IngestSink::default();
    let roots: Vec<Box<dyn AnchoredDirectory>> = vec![Box::new(SmallRoot {
        fail: false,
        done: false,
        name: b"legacy".to_vec(),
    })];
    let context = Clocked {
        ticks: AtomicUsize::new(10),
    };
    let result = collect_open_forest(
        store.open_forest(ids.next_id(), vec![observed_root("C:\\A", 1)]),
        roots,
        &ids,
        &context,
        1,
        &|_| true,
    );
    assert!(result.result_version_id.is_some());
    let version = store
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing")?;
    let item = version
        .items()
        .iter()
        .find(|item| item.name == b"legacy")
        .ok_or("missing item")?;
    assert_eq!(version.item_observed_at(item.item_no), None);
    Ok(())
}

struct CachedModifiedRoot;
impl DirectoryObservations for CachedModifiedRoot {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Ok(Vec::new())
    }
}
impl AnchoredDirectory for CachedModifiedRoot {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "os.unsupported".into(),
            os_code: None,
        })
    }
    fn observed_modified_at(&self) -> Option<Timestamp> {
        Timestamp::from_unix_millis(123)
    }
}
#[test]
fn legacy_root_modified_time_does_not_read_observation_clock() {
    let calls = AtomicUsize::new(0);
    let (modified, observed) = CachedModifiedRoot.observed_modified_at_timed(&|| {
        calls.fetch_add(1, Ordering::SeqCst);
        Timestamp::from_unix_millis(456)
    });
    assert_eq!(modified, Timestamp::from_unix_millis(123));
    assert_eq!(observed, None);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
