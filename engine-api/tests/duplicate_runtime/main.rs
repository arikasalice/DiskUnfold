//! Test-only F09 composition: scan selection, synthetic evidence, runtime gate.
//! Placeholder locality, time, change stamp and bytes below are synthetic; this
//! fixture does not claim they were observed by the platform or authorize I/O.
use du_analysis_index::{
    FileIdentity as ScanIdentity, IngestSink, Observation, SharingEvidence, SpaceObservation,
};
use du_core_contracts::{
    ByteSize, Id, ItemRef, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    Timestamp,
    duplicate_content::{
        BoundContent, ContentIdentity, ContentObservation, DuplicateContentPort,
        PlaceholderEvidence,
    },
    job::{HistoryAppend, JobHistory, JobState, JobSummary},
    ports::{PortError, PortErrorKind},
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
};
use du_duplicate_engine::{
    RegisteredHash,
    candidates::{EntryKind, FileIdentity, Placeholder, Size, Target},
    content_port::{Adapter, SelectedContent},
    fingerprint_cache::{FingerprintCache, ReuseCapabilities},
    job::{self, Input, JobReuse, PreparedJob},
    result_store::{ContentCoverage, ResultStore, ScopeEvidence},
    verification::{Snapshot, State, VerificationTarget},
};
use du_engine_api::duplicate_scope::{Selection, select_whole_scan};
use du_job_runtime::{
    CancelResult, FlowLimits, JobClock, JobEvent, JobEvents, JobIdentity, JobPriority, JobRuntime,
    Submission, SubmitResult,
};
use std::{
    collections::BTreeMap,
    num::NonZeroUsize,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

fn id(n: u8) -> Id {
    Id::from_v7_bytes([1, 2, 3, 4, 5, n, 0x70, 0, 0x80, 0, 0, 0, 0, 0, 0, n])
}
struct Ids(AtomicUsize);
impl JobIdentity for Ids {
    fn next_id(&self) -> Id {
        id(self.0.fetch_add(1, Ordering::SeqCst) as u8)
    }
}
struct Clock;
impl JobClock for Clock {
    fn now_ms(&self) -> i64 {
        100
    }
}
#[derive(Default)]
struct History {
    rows: Mutex<Vec<JobSummary>>,
    changed: Condvar,
}
impl JobHistory for History {
    fn append(&self, summary: &JobSummary) -> Result<HistoryAppend, String> {
        self.rows
            .lock()
            .map_err(|_| "history lock")?
            .push(summary.clone());
        self.changed.notify_all();
        Ok(HistoryAppend::Added)
    }
}
impl History {
    fn wait(&self, count: usize) -> Result<Vec<JobSummary>, String> {
        let rows = self.rows.lock().map_err(|_| "history lock")?;
        let (rows, _) = self
            .changed
            .wait_timeout_while(rows, Duration::from_secs(5), |rows| rows.len() < count)
            .map_err(|_| "history wait")?;
        if rows.len() < count {
            return Err("history timeout".into());
        }
        Ok(rows.clone())
    }
}
struct Events;
impl JobEvents for Events {
    fn publish(&self, _: JobEvent) {}
}
#[derive(Default)]
struct Stats {
    opens: AtomicUsize,
    bytes: AtomicUsize,
    gate: Mutex<Option<ReadGate>>,
}
struct ReadGate {
    entered: std::sync::mpsc::SyncSender<()>,
    release: std::sync::mpsc::Receiver<()>,
}
struct Port {
    selected: BTreeMap<u32, ContentIdentity>,
    stats: Arc<Stats>,
    fail: Option<u32>,
}
struct File<'a> {
    identity: ContentIdentity,
    stats: &'a Stats,
}
impl DuplicateContentPort for Port {
    fn open_bound<'a>(
        &'a mut self,
        item: &ItemRef,
        expected: &ContentIdentity,
        placeholder: PlaceholderEvidence,
    ) -> Result<Box<dyn BoundContent + 'a>, PortError> {
        self.stats.opens.fetch_add(1, Ordering::SeqCst);
        if self.fail == Some(item.item_no)
            || self.selected.get(&item.item_no) != Some(expected)
            || placeholder != PlaceholderEvidence::Local
        {
            return Err(PortError {
                kind: PortErrorKind::Io,
                reason_key: "fixture.failed".into(),
                os_code: None,
            });
        }
        Ok(Box::new(File {
            identity: expected.clone(),
            stats: &self.stats,
        }))
    }
}
impl BoundContent for File<'_> {
    fn observe(&mut self) -> Result<ContentObservation, PortError> {
        Ok(ContentObservation {
            identity: self.identity.clone(),
            size: 3,
            modified: 7,
            change_stamp: Some(9),
        })
    }
    fn read_at(&mut self, offset: u64, buffer: &mut [u8]) -> Result<usize, PortError> {
        let start = usize::try_from(offset).map_err(|_| PortError {
            kind: PortErrorKind::Io,
            reason_key: "fixture.offset".into(),
            os_code: None,
        })?;
        let remaining = b"abc".get(start..).ok_or(PortError {
            kind: PortErrorKind::Io,
            reason_key: "fixture.offset".into(),
            os_code: None,
        })?;
        let n = remaining.len().min(buffer.len());
        buffer[..n].copy_from_slice(&remaining[..n]);
        self.stats.bytes.fetch_add(n, Ordering::SeqCst);
        let gate = self
            .stats
            .gate
            .lock()
            .map_err(|_| PortError {
                kind: PortErrorKind::Io,
                reason_key: "fixture.gate".into(),
                os_code: None,
            })?
            .take();
        if n > 0
            && let Some(gate) = gate
        {
            gate.entered.send(()).map_err(|_| PortError {
                kind: PortErrorKind::Io,
                reason_key: "fixture.gate".into(),
                os_code: None,
            })?;
            gate.release
                .recv_timeout(Duration::from_secs(5))
                .map_err(|_| PortError {
                    kind: PortErrorKind::Io,
                    reason_key: "fixture.gate".into(),
                    os_code: None,
                })?;
        }
        Ok(n)
    }
}
fn scan() -> Result<(IngestSink, Selection), String> {
    let source = IngestSink::default();
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut build = source
        .open_observed(id(1), root)
        .map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    for n in 1..=2u8 {
        let path = vec![vec![b'a' + n - 1]];
        build
            .add_with_space(
                Observation {
                    path: path.clone(),
                    size: Some(SizeValue {
                        basis: SizeBasis::Logical,
                        evidence: SizeEvidence::OsReported,
                        amount: SizeAmount::Exact {
                            bytes: ByteSize::from(3),
                        },
                    }),
                },
                SpaceObservation {
                    allocated: None,
                    identity: Some(ScanIdentity {
                        volume: vec![1],
                        file: vec![n + 1],
                    }),
                    link_count: None,
                    sharing: SharingEvidence::Unknown,
                },
            )
            .map_err(|e| format!("{e:?}"))?;
        build
            .set_modified_at(0, &path, Timestamp::parse("2026-01-01T00:00:00.000Z")?)
            .map_err(|e| format!("{e:?}"))?;
        build
            .retain_enumerated_inode(
                0,
                &path,
                std::num::NonZeroU64::new(u64::from(n) + 100).ok_or("missing fixture inode")?,
            )
            .map_err(|e| format!("{e:?}"))?;
    }
    build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let selection =
        select_whole_scan(&source, &id(2), 0, || false).map_err(|e| format!("{e:?}"))?;
    Ok((source, selection))
}
struct Harness {
    source: IngestSink,
    selection: Selection,
    store: Arc<ResultStore>,
    cache: Arc<FingerprintCache>,
    runtime: Arc<JobRuntime>,
    history: Arc<History>,
    stats: Arc<Stats>,
}
impl Harness {
    fn new(live: NonZeroUsize) -> Result<Self, String> {
        let (source, selection) = scan()?;
        let store = Arc::new(ResultStore::new(live, NonZeroUsize::MIN.saturating_add(20)));
        store
            .register_source(selection.version().clone())
            .map_err(|e| format!("{e:?}"))?;
        let history = Arc::new(History::default());
        let runtime = Arc::new(
            JobRuntime::new_with_content_io_limit(
                Arc::new(Ids(AtomicUsize::new(20))),
                Arc::new(Clock),
                history.clone(),
                Arc::new(Events),
                FlowLimits::disabled(),
                1,
            )
            .map_err(|e| format!("{e:?}"))?,
        );
        runtime
            .register(job::descriptor())
            .map_err(|e| format!("{e:?}"))?;
        Ok(Self {
            source,
            selection,
            store,
            cache: Arc::new(FingerprintCache::new(NonZeroUsize::MIN.saturating_add(4))),
            runtime,
            history,
            stats: Arc::new(Stats::default()),
        })
    }
    fn prepare(&self, result: Id, fail: Option<u32>) -> Result<PreparedJob, String> {
        let scope = self.selection.scope_evidence();
        let scope = ScopeEvidence {
            roots: scope.roots.clone(),
            exclusions: scope.exclusions.clone(),
            minimum_bytes: scope.minimum_bytes,
            outside: scope.counts.outside,
            excluded: scope.counts.excluded,
            below_minimum: scope.counts.below_minimum,
            unavailable: scope.counts.unavailable,
            source_completeness: scope.source_completeness,
            observed_at: scope.observed_at.clone(),
        };
        let mut selected = Vec::new();
        let mut targets = Vec::new();
        let mut identities = BTreeMap::new();
        for file in self.selection.files() {
            let pinned = self
                .selection
                .content_evidence(&file.item)
                .ok_or("missing pinned content identity")?;
            assert_eq!(pinned.placeholder, PlaceholderEvidence::Unknown);
            let content = pinned.identity;
            assert!(content.enumerated_inode.is_some());
            let identity = FileIdentity {
                volume: content.volume.clone(),
                file: content.file.clone(),
            };
            identities.insert(file.item.item_no, content.clone());
            selected.push(SelectedContent {
                item: file.item.clone(),
                identity: content,
                // The test adapter independently supplies locality and change stamps.
                placeholder: PlaceholderEvidence::Local,
            });
            targets.push(VerificationTarget {
                target: Target {
                    item_no: file.item.item_no,
                    kind: EntryKind::File,
                    size: Size::Exact(file.size),
                    identity: Some(identity.clone()),
                    placeholder: Placeholder::Local,
                },
                snapshot: Some(Snapshot {
                    identity,
                    size: file.size,
                    modified: 7,
                    change_stamp: Some(9),
                }),
            });
        }
        let port = Port {
            selected: identities,
            stats: self.stats.clone(),
            fail,
        };
        let reader = Adapter::new_owned(port, self.selection.version(), selected)
            .map_err(|e| format!("{e:?}"))?;
        let reuse = JobReuse {
            cache: self.cache.clone(),
            capabilities: Arc::new(|_| {
                Some(ReuseCapabilities {
                    change_stamp_supported: true,
                    modified_resolution: Some(Duration::from_millis(1)),
                })
            }),
            clock: Arc::new(|| Some(100)),
        };
        job::prepare_with_reuse(
            Input {
                source: self.selection.version().clone(),
                result,
                scope,
                targets,
                reader,
            },
            RegisteredHash::resolve("blake3", Some(1)).map_err(|e| format!("{e:?}"))?,
            self.store.clone(),
            reuse,
        )
        .map_err(|e| format!("{e:?}"))
    }
    fn submit(&self, prepared: PreparedJob) -> Result<Id, String> {
        let submission = Submission {
            kind_id: "duplicate.check".into(),
            deduplication_key: None,
            body: prepared.body,
            priority: JobPriority::Normal,
            scope_summary: "fixture".into(),
            correlation_id: id(9),
            sample: false,
        };
        match self
            .runtime
            .submit_duplicate_terminal(submission, prepared.queued, prepared.commit)
            .map_err(|e| format!("{e:?}"))?
        {
            SubmitResult::Created(id) => Ok(id),
            other => Err(format!("{other:?}")),
        }
    }
}

mod cases;
