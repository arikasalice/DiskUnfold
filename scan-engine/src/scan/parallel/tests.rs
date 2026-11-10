use super::super::collect_parallel;
use super::*;
use du_analysis_index::IngestSink;
use du_core_contracts::{
    ItemRef, OsPath, PathBytes, PathEncoding,
    job::{JobBody, JobFlowError},
    ports::{PortError, PortErrorKind},
    result::{ResultNativePath, ResultScopeRoot, ResultVersionRead},
    scan_observation::{DirectoryObservations, MetadataObservation, OpenedScopeRoot},
};
use std::sync::{Arc, atomic::AtomicUsize};
mod disconnect;
mod filtered;
mod item_times;
mod pause;
mod root_times;
mod volume_reports;

static WORKER_TEST_LOCK: Mutex<()> = Mutex::new(());

struct Ids(AtomicUsize);
impl ResultIds for Ids {
    fn next_id(&self) -> Id {
        let n = self.0.fetch_add(1, Ordering::SeqCst);
        Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}")).unwrap_or_else(|_| unreachable!())
    }
}

struct SmallRoot {
    fail: bool,
    done: bool,
    name: Vec<u8>,
}
impl DirectoryObservations for SmallRoot {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        if self.fail {
            return Err(PortError {
                kind: PortErrorKind::PermissionDenied,
                reason_key: "os.denied".into(),
                os_code: None,
            });
        }
        if std::mem::replace(&mut self.done, true) {
            return Ok(Vec::new());
        }
        Ok(vec![ObservedEntry {
            name: self.name.clone(),
            enumerated_inode: 7,
            enumerated_kind: ObservedKind::File,
            metadata: Ok(MetadataObservation {
                kind: ObservedKind::File,
                device: 1,
                inode: 7,
                size: Ok(5),
                allocated: None,
                link_count: None,
                space_identity: None,
                modified_at: None,
            }),
        }])
    }
}
impl AnchoredDirectory for SmallRoot {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "os.unsupported".into(),
            os_code: None,
        })
    }
}

fn native(value: &str) -> Vec<u8> {
    value.encode_utf16().flat_map(u16::to_le_bytes).collect()
}
fn observed_root(path: &str, volume: u8) -> ResultScopeRoot {
    ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::WindowsUtf16le,
            raw: PathBytes::from_bytes(&native(path)),
        },
        volume_id: vec![volume],
        file_id: vec![1],
    }
}

#[test]
fn forest_keeps_same_names_separate_and_records_failed_root()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker test lock")?;
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let roots = vec![
        observed_root("C:\\A", 1),
        observed_root("C:\\B", 1),
        observed_root("D:\\C", 2),
    ];
    let opened = sink.open_forest(ids.next_id(), roots.clone());
    let dirs = [false, false, true]
        .into_iter()
        .map(|fail| {
            Box::new(SmallRoot {
                fail,
                done: false,
                name: native("same.txt"),
            }) as Box<dyn AnchoredDirectory>
        })
        .collect();
    let ctx = Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    };
    let result = collect_open_forest(opened, dirs, &ids, &ctx, 1, &|_| true);
    let version = sink
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing result")?;
    assert_eq!(
        result.result_version_id.as_ref(),
        Some(version.version_id())
    );
    assert_eq!(version.observed_roots(), roots);
    assert_eq!(version.completeness(), ResultCompleteness::Missing);
    assert_eq!(version.scope_gaps().len(), 1);
    assert_eq!(version.scope_gaps()[0].root_index, 2);
    assert_eq!(version.scope_gaps()[0].reason_key, "scan.path_denied");
    assert_eq!(version.scan_issues().len(), 1);
    assert_eq!(version.scan_issues()[0].root_index, 2);
    assert_eq!(
        version.scan_issues()[0].stage,
        du_core_contracts::scan_failures::ScanFailureStage::Enumerate
    );
    let files: Vec<_> = version
        .items()
        .iter()
        .filter(|item| item.name == native("same.txt"))
        .collect();
    assert_eq!(files.len(), 2);
    let indices: Vec<_> = files
        .iter()
        .map(|item| {
            version
                .observed_root_for(&ItemRef {
                    result_version: version.version_id().clone(),
                    item_no: item.item_no,
                })
                .map(|(index, _)| index)
        })
        .collect::<Result<_, _>>()
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(indices, [0, 1]);
    Ok(())
}

#[test]
fn metadata_job_accepts_reviewed_roots_in_one_build() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker test lock")?;
    let ids = Arc::new(Ids(AtomicUsize::new(1)));
    let sink = IngestSink::default();
    let roots = ["C:\\A", "D:\\B"]
        .into_iter()
        .enumerate()
        .map(|(index, path)| OpenedScopeRoot {
            canonical_path: OsPath {
                encoding: PathEncoding::WindowsUtf16le,
                raw: PathBytes::from_bytes(&native(path)),
                display: path.into(),
                lossy: false,
            },
            volume_id: vec![index as u8 + 1],
            file_id: vec![1],
            directory: Box::new(SmallRoot {
                fail: false,
                done: false,
                name: native("same.txt"),
            }),
        })
        .collect();
    let scope = ids.next_id();
    let job = crate::scan_job::MetadataScan::new_roots(roots, sink.clone(), scope.clone(), ids);
    let context = Arc::new(Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    });
    let outcome = job.run(context);
    let result = sink
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing result")?;
    assert_eq!(result.scope_version_id(), &scope);
    assert_eq!(result.observed_roots().len(), 2);
    assert_eq!(
        outcome.result_version_id.as_ref(),
        Some(result.version_id())
    );
    assert_eq!(result.completeness(), ResultCompleteness::Complete);
    Ok(())
}

#[test]
fn all_roots_failing_does_not_publish_a_result() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker test lock")?;
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let roots = vec![observed_root("C:\\A", 1), observed_root("D:\\B", 2)];
    let opened = sink.open_forest(ids.next_id(), roots);
    let dirs = (0..2)
        .map(|_| {
            Box::new(SmallRoot {
                fail: true,
                done: false,
                name: Vec::new(),
            }) as Box<dyn AnchoredDirectory>
        })
        .collect();
    let ctx = Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    };
    let result = collect_open_forest(opened, dirs, &ids, &ctx, 2, &|_| true);
    assert!(result.result_version_id.is_none());
    assert!(sink.pin().map_err(|e| format!("{e:?}"))?.is_none());
    Ok(())
}

#[test]
fn cancelled_forest_reports_unprocessed_root_count_without_error_gaps()
-> Result<(), Box<dyn std::error::Error>> {
    struct Cancelled;
    impl JobContext for Cancelled {
        fn checkpoint(&self) -> Checkpoint {
            Checkpoint::Cancel
        }
        fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
            Err(JobFlowError::Cancelled)
        }
    }
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let roots = vec![observed_root("C:\\A", 1), observed_root("D:\\B", 2)];
    let opened = sink.open_forest(ids.next_id(), roots);
    let dirs = (0..2)
        .map(|_| {
            Box::new(SmallRoot {
                fail: false,
                done: false,
                name: Vec::new(),
            }) as Box<dyn AnchoredDirectory>
        })
        .collect();
    let outcome = collect_open_forest(opened, dirs, &ids, &Cancelled, 2, &|_| true);
    assert_eq!(
        outcome.unprocessed_summary.as_deref(),
        Some("unprocessed_roots=2")
    );
    let result = sink
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing result")?;
    assert_eq!(result.completeness(), ResultCompleteness::Cancelled);
    assert!(result.scope_gaps().is_empty());
    Ok(())
}

#[test]
fn cancellation_after_one_root_releases_flow_and_counts_only_unfinished_roots()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker test lock")?;
    struct StopRoot(Arc<AtomicBool>);
    impl DirectoryObservations for StopRoot {
        fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
            self.0.store(true, Ordering::Release);
            Ok(Vec::new())
        }
    }
    impl AnchoredDirectory for StopRoot {
        fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
            Err(PortError {
                kind: PortErrorKind::Unsupported,
                reason_key: "os.unsupported".into(),
                os_code: None,
            })
        }
    }
    struct StopContext {
        stop: Arc<AtomicBool>,
        done: Arc<AtomicUsize>,
        abandoned: Arc<AtomicUsize>,
    }
    impl JobContext for StopContext {
        fn checkpoint(&self) -> Checkpoint {
            if self.stop.load(Ordering::Acquire) {
                Checkpoint::Cancel
            } else {
                Checkpoint::Continue
            }
        }
        fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
            Ok(Box::new(Flow {
                completed: self.done.clone(),
                abandoned: self.abandoned.clone(),
                finished: false,
            }))
        }
    }
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let roots = vec![observed_root("C:\\A", 1), observed_root("D:\\B", 2)];
    let opened = sink.open_forest(ids.next_id(), roots);
    let stop = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicUsize::new(0));
    let abandoned = Arc::new(AtomicUsize::new(0));
    let dirs: Vec<Box<dyn AnchoredDirectory>> = vec![
        Box::new(StopRoot(stop.clone())),
        Box::new(SmallRoot {
            fail: false,
            done: false,
            name: native("later"),
        }),
    ];
    let context = StopContext {
        stop,
        done: done.clone(),
        abandoned: abandoned.clone(),
    };
    let outcome = collect_open_forest(opened, dirs, &ids, &context, 1, &|_| true);
    assert_eq!(
        outcome.unprocessed_summary.as_deref(),
        Some("unprocessed_roots=1")
    );
    assert_eq!(done.load(Ordering::Acquire), 2);
    assert_eq!(abandoned.load(Ordering::Acquire), 0);
    let result = sink
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing result")?;
    assert_eq!(result.completeness(), ResultCompleteness::Cancelled);
    assert!(result.scope_gaps().is_empty());
    Ok(())
}
struct Context {
    completed: Arc<AtomicUsize>,
    abandoned: Arc<AtomicUsize>,
}
struct Flow {
    completed: Arc<AtomicUsize>,
    abandoned: Arc<AtomicUsize>,
    finished: bool,
}
impl JobFlow for Flow {
    fn report_item(&self, _: &[u8]) -> Result<(), JobFlowError> {
        Ok(())
    }
    fn finish(mut self: Box<Self>, _: JobCounts) {
        self.finished = true;
        self.completed.fetch_add(1, Ordering::SeqCst);
    }
}
impl Drop for Flow {
    fn drop(&mut self) {
        if !self.finished {
            self.abandoned.fetch_add(1, Ordering::SeqCst);
        }
    }
}
impl JobContext for Context {
    fn checkpoint(&self) -> Checkpoint {
        Checkpoint::Continue
    }
    fn try_start_flow(&self) -> Result<Box<dyn JobFlow>, JobFlowError> {
        Ok(Box::new(Flow {
            completed: self.completed.clone(),
            abandoned: self.abandoned.clone(),
            finished: false,
        }))
    }
}
struct Root {
    complete: bool,
    reads: Arc<AtomicUsize>,
}
impl DirectoryObservations for Root {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        if std::mem::replace(&mut self.complete, true) {
            return Ok(Vec::new());
        }
        Ok((1..=8192)
            .map(|n| ObservedEntry {
                name: format!("f{n}").into_bytes(),
                enumerated_inode: n,
                enumerated_kind: ObservedKind::File,
                metadata: Ok(MetadataObservation {
                    kind: ObservedKind::File,
                    device: 1,
                    inode: n,
                    size: Ok(1),
                    allocated: None,
                    link_count: None,
                    space_identity: None,
                    modified_at: None,
                }),
            })
            .collect())
    }
}
impl AnchoredDirectory for Root {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(PortError {
            kind: PortErrorKind::Unsupported,
            reason_key: "os.unsupported".into(),
            os_code: None,
        })
    }
}

#[test]
fn ready_roots_take_turns_even_when_one_has_many_frames() {
    let frame = |root_index| Frame {
        root_index,
        path: Vec::new(),
        directory: Box::new(Root {
            complete: true,
            reads: Arc::new(AtomicUsize::new(0)),
        }),
        entries: VecDeque::new(),
        deferred: VecDeque::new(),
        root: false,
        first_batch: false,
        root_published: false,
        batch_pending: false,
    };
    let mut queue = Queue {
        ready: VecDeque::from([frame(0), frame(0), frame(1), frame(2), frame(0), frame(1)]),
        last_root: None,
        remaining: 6,
        root_remaining: vec![3, 2, 1],
        root_groups: vec![0, 1, 2],
        active_groups: vec![0; 3],
        device_limit: 10,
        started: true,
    };
    let mut selected = Vec::new();
    while let Some((frame, _)) = queue.pop_fair() {
        selected.push(frame.root_index);
    }
    assert_eq!(selected, [0, 1, 2, 0, 1, 0]);
}

mod cadence;
mod device;
mod panic_boundary;

#[test]
fn failed_second_spawn_closes_channels_and_finishes_all_flows()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = WORKER_TEST_LOCK.lock().map_err(|_| "worker test lock")?;
    let ids = Ids(AtomicUsize::new(1));
    let sink = IngestSink::default();
    let mut previous = sink.open(ids.next_id()).map_err(|e| format!("{e:?}"))?;
    previous
        .add_directory(Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    let baseline = previous
        .seal(ids.next_id(), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let context = Context {
        completed: Arc::new(AtomicUsize::new(0)),
        abandoned: Arc::new(AtomicUsize::new(0)),
    };
    TEST_WORKER_STARTED.store(false, Ordering::Release);
    FAIL_SPAWN_AT.store(1, Ordering::Release);
    let reads = Arc::new(AtomicUsize::new(0));
    let result = collect_parallel(
        &sink,
        ids.next_id(),
        Box::new(Root {
            complete: false,
            reads: reads.clone(),
        }),
        &ids,
        &context,
        3,
        &|_| true,
    );
    FAIL_SPAWN_AT.store(usize::MAX, Ordering::Release);
    assert!(TEST_WORKER_STARTED.load(Ordering::Acquire));
    assert_eq!(reads.load(Ordering::Relaxed), 0);
    assert!(result.result_version_id.is_none());
    assert_eq!(context.completed.load(Ordering::SeqCst), 4);
    assert_eq!(context.abandoned.load(Ordering::SeqCst), 0);
    assert_eq!(
        sink.pin()
            .map_err(|e| format!("{e:?}"))?
            .ok_or("missing baseline")?
            .version_id(),
        baseline.version_id()
    );
    Ok(())
}
