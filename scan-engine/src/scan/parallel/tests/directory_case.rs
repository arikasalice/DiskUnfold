use super::*;
use du_core_contracts::scan_observation::{
    DirectoryCaseEvidence, DirectoryCaseState, FilterDecision, FilteredBatch,
};

struct CaseDir {
    step: usize,
    root: bool,
    state: Option<DirectoryCaseState>,
    root_calls: Arc<AtomicUsize>,
    child_calls: Arc<AtomicUsize>,
}
impl DirectoryObservations for CaseDir {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        match self.next_filtered(&mut |_| FilterDecision::Admit)? {
            FilteredBatch::Entries(entries) => Ok(entries),
            _ => Ok(Vec::new()),
        }
    }
    fn next_filtered(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
    ) -> Result<FilteredBatch, PortError> {
        if !self.root {
            return Ok(FilteredBatch::End);
        }
        let names: [&[u8]; 3] = [b"child", b"failed", b"excluded"];
        let Some(name) = names.get(self.step).copied() else {
            return Ok(FilteredBatch::End);
        };
        self.step += 1;
        if include(name) == FilterDecision::Exclude {
            return Ok(FilteredBatch::Progress);
        }
        Ok(FilteredBatch::Entries(vec![ObservedEntry {
            name: name.to_vec(),
            enumerated_inode: 7,
            enumerated_kind: ObservedKind::Directory,
            metadata: Ok(MetadataObservation {
                kind: ObservedKind::Directory,
                device: 1,
                inode: 7,
                size: Ok(0),
                allocated: None,
                link_count: None,
                space_identity: None,
                modified_at: None,
            }),
        }]))
    }
}
impl AnchoredDirectory for CaseDir {
    fn open_child(&self, entry: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        if entry.name == b"failed" {
            return Err(PortError {
                kind: PortErrorKind::PermissionDenied,
                reason_key: "os.denied".into(),
                os_code: None,
            });
        }
        Ok(Box::new(CaseDir {
            step: 0,
            root: false,
            state: Some(DirectoryCaseState::Failed),
            root_calls: self.root_calls.clone(),
            child_calls: self.child_calls.clone(),
        }))
    }
    fn observe_directory_case(
        &self,
        _: &dyn Fn() -> Option<du_core_contracts::Timestamp>,
    ) -> Option<DirectoryCaseEvidence> {
        if self.root {
            self.root_calls.fetch_add(1, Ordering::SeqCst);
        } else {
            self.child_calls.fetch_add(1, Ordering::SeqCst);
        }
        self.state.map(|state| DirectoryCaseEvidence {
            state,
            observed_at: du_core_contracts::Timestamp::parse("2020-09-13T12:26:40.000Z").ok(),
            reason_key: (state == DirectoryCaseState::Failed).then(|| "os.query_failed".into()),
        })
    }
}
#[test]
fn filtered_forest_retains_case_evidence_only_for_opened_directories()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let roots_seen = Arc::new(AtomicUsize::new(0));
    let children_seen = Arc::new(AtomicUsize::new(0));
    let roots = [Some(DirectoryCaseState::Sensitive), None]
        .into_iter()
        .map(|state| {
            Box::new(CaseDir {
                step: 0,
                root: true,
                state,
                root_calls: roots_seen.clone(),
                child_calls: children_seen.clone(),
            }) as Box<dyn AnchoredDirectory>
        })
        .collect();
    let context = Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    };
    let outcome = collect_open_forest_filtered(
        sink.open_forest(
            ids.next_id(),
            vec![observed_root("C:\\A", 1), observed_root("D:\\B", 2)],
        ),
        roots,
        &ids,
        &context,
        2,
        &|_, _, name| {
            if name == b"excluded" {
                FilterDecision::Exclude
            } else {
                FilterDecision::Admit
            }
        },
        DevicePlan {
            groups: vec![0, 1],
            limit: 1,
        },
    );
    let result = sink
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing result")?;
    assert!(outcome.result_version_id.is_some());
    assert_eq!(roots_seen.load(Ordering::SeqCst), 2);
    assert_eq!(children_seen.load(Ordering::SeqCst), 2);
    let expected = du_core_contracts::Timestamp::parse("2020-09-13T12:26:40.000Z")?;
    for item in result.items() {
        let reference = ItemRef {
            result_version: result.version_id().clone(),
            item_no: item.item_no,
        };
        let root = result
            .observed_root_for(&reference)
            .map_err(|e| format!("{e:?}"))?
            .0;
        let evidence = result.directory_case_evidence(&reference);
        if item.parent.is_none() && root == 0 {
            assert_eq!(
                evidence.map(|x| x.state),
                Some(DirectoryCaseState::Sensitive)
            );
        }
        if item.parent.is_none() && root == 1 {
            assert!(evidence.is_none());
        }
        if item.name == b"child" {
            assert_eq!(evidence.map(|x| x.state), Some(DirectoryCaseState::Failed));
        }
        if item.name == b"failed" {
            assert!(evidence.is_none());
        }
        if let Some(evidence) = evidence {
            assert_eq!(evidence.observed_at.as_ref(), Some(&expected));
        }
        assert_ne!(item.name, b"excluded");
    }
    Ok(())
}

#[test]
fn serial_collector_records_evidence_from_opened_root_and_child()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let roots_seen = Arc::new(AtomicUsize::new(0));
    let children_seen = Arc::new(AtomicUsize::new(0));
    let root = CaseDir {
        step: 0,
        root: true,
        state: Some(DirectoryCaseState::Insensitive),
        root_calls: roots_seen.clone(),
        child_calls: children_seen.clone(),
    };
    let outcome = crate::scan::collect(
        &sink,
        ids.next_id(),
        Box::new(root),
        &ids,
        || Checkpoint::Continue,
        |path| path.last().is_none_or(|name| name != b"excluded"),
    );
    assert!(outcome.result_version_id.is_some());
    let result = sink
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing result")?;
    assert_eq!(roots_seen.load(Ordering::SeqCst), 1);
    assert_eq!(children_seen.load(Ordering::SeqCst), 1);
    for item in result.items() {
        let reference = ItemRef {
            result_version: result.version_id().clone(),
            item_no: item.item_no,
        };
        let evidence = result.directory_case_evidence(&reference);
        if item.parent.is_none() {
            assert_eq!(
                evidence.map(|x| x.state),
                Some(DirectoryCaseState::Insensitive)
            );
        }
        if item.name == b"child" {
            assert_eq!(evidence.map(|x| x.state), Some(DirectoryCaseState::Failed));
        }
        if item.name == b"failed" {
            assert!(evidence.is_none());
        }
        assert_ne!(item.name, b"excluded");
    }
    Ok(())
}

struct Cancelled;
impl JobContext for Cancelled {
    fn checkpoint(&self) -> Checkpoint {
        Checkpoint::Cancel
    }
    fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
        Err(JobFlowError::Cancelled)
    }
}
#[test]
fn cancelled_before_collection_does_not_observe_root_case() -> Result<(), Box<dyn std::error::Error>>
{
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let roots_seen = Arc::new(AtomicUsize::new(0));
    let children_seen = Arc::new(AtomicUsize::new(0));
    let root = CaseDir {
        step: 0,
        root: true,
        state: Some(DirectoryCaseState::Sensitive),
        root_calls: roots_seen.clone(),
        child_calls: children_seen.clone(),
    };
    let outcome = collect_parallel(
        &sink,
        ids.next_id(),
        Box::new(root),
        &ids,
        &Cancelled,
        1,
        &|_| true,
    );
    assert!(!outcome.completed_scope);
    assert_eq!(roots_seen.load(Ordering::SeqCst), 0);
    assert_eq!(children_seen.load(Ordering::SeqCst), 0);
    Ok(())
}

struct CancelDuringCase {
    cancelled: Arc<AtomicBool>,
}
impl JobContext for CancelDuringCase {
    fn checkpoint(&self) -> Checkpoint {
        if self.cancelled.load(Ordering::SeqCst) {
            Checkpoint::Cancel
        } else {
            Checkpoint::Continue
        }
    }
    fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
        Ok(Box::new(Flow {
            completed: Arc::new(AtomicUsize::new(0)),
            abandoned: Arc::new(AtomicUsize::new(0)),
            finished: false,
        }))
    }
}
struct CaseThenCancel(Arc<AtomicBool>);
impl DirectoryObservations for CaseThenCancel {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Err(PortError {
            kind: PortErrorKind::Io,
            reason_key: "unexpected.read".into(),
            os_code: None,
        })
    }
}
impl AnchoredDirectory for CaseThenCancel {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "unexpected.open".into(),
            os_code: None,
        })
    }
    fn observe_directory_case(
        &self,
        _: &dyn Fn() -> Option<du_core_contracts::Timestamp>,
    ) -> Option<DirectoryCaseEvidence> {
        self.0.store(true, Ordering::SeqCst);
        Some(DirectoryCaseEvidence {
            state: DirectoryCaseState::Sensitive,
            observed_at: None,
            reason_key: None,
        })
    }
}
#[test]
fn case_observation_returned_during_cancellation_is_retained()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let cancelled = Arc::new(AtomicBool::new(false));
    let context = CancelDuringCase {
        cancelled: cancelled.clone(),
    };
    let outcome = collect_parallel(
        &sink,
        ids.next_id(),
        Box::new(CaseThenCancel(cancelled)),
        &ids,
        &context,
        1,
        &|_| true,
    );
    assert!(!outcome.completed_scope);
    let result = sink
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing result")?;
    assert_eq!(result.completeness(), ResultCompleteness::Cancelled);
    let root = result
        .items()
        .iter()
        .find(|item| item.parent.is_none())
        .ok_or("missing root")?;
    let reference = ItemRef {
        result_version: result.version_id().clone(),
        item_no: root.item_no,
    };
    assert_eq!(
        result.directory_case_evidence(&reference).map(|e| e.state),
        Some(DirectoryCaseState::Sensitive)
    );
    Ok(())
}

#[test]
fn case_query_failure_does_not_change_collection_counts_or_completeness()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "lock")?;
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let root = CaseDir {
        step: 0,
        root: false,
        state: Some(DirectoryCaseState::Failed),
        root_calls: Arc::new(AtomicUsize::new(0)),
        child_calls: Arc::new(AtomicUsize::new(0)),
    };
    let context = Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    };
    let outcome = collect_parallel(
        &sink,
        ids.next_id(),
        Box::new(root),
        &ids,
        &context,
        1,
        &|_| true,
    );
    assert_eq!(outcome.counts.failed, 0);
    assert!(outcome.completed_scope);
    let result = sink
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing result")?;
    assert_eq!(result.completeness(), ResultCompleteness::Complete);
    let root = result
        .items()
        .iter()
        .find(|item| item.parent.is_none())
        .ok_or("missing root")?;
    let reference = ItemRef {
        result_version: result.version_id().clone(),
        item_no: root.item_no,
    };
    assert_eq!(
        result.directory_case_evidence(&reference).map(|e| e.state),
        Some(DirectoryCaseState::Failed)
    );
    Ok(())
}
