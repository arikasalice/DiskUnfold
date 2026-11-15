//! F02 NFR5.HD1.app-host.DD1: the end state of a metadata scan in the job runtime.
//!
//! The runtime reports a job that did not finish its scope as partial when it
//! has completed items. A scan that publishes no result reports none, so it
//! ends failed even when it counted items before it failed. A cancelled scan
//! publishes a cancelled result and ends cancelled; a scan that reads its
//! whole scope ends done.
use du_analysis_index::{
    BoundSizeRule, IngestSink,
    size_rule_registry::{DEFAULT_SIZE_RULE_ID, SizeRuleRegistryBuilder},
};
use du_core_contracts::{
    Id, OsPath, PathBytes, PathEncoding,
    job::{HistoryAppend, JobHistory, JobState, JobSummary},
    ports::{PortError, PortErrorKind},
    result::{ResultCompleteness, ResultVersionRead},
    scan_observation::{
        AnchoredDirectory, DirectoryObservations, HandleIdentity, MetadataObservation,
        ObservedEntry, ObservedKind, OpenedScopeRoot,
    },
};
use du_job_runtime::{
    FlowLimits, JobClock, JobEvent, JobEvents, JobIdentity, JobPriority, JobRuntime, Submission,
    SubmitResult,
};
use du_scan_engine::{
    scan::ResultIds,
    scan_job::{MetadataScan, descriptor},
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const ROOT: u64 = 1;
const FOLDER: u64 = 20;

fn id(n: u64) -> Id {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}")).unwrap_or_else(|_| unreachable!())
}

/// Issues fresh version IDs, or always one the store already used, which
/// makes every publication fail.
struct Ids {
    next: AtomicU64,
    reused: Option<Id>,
}

impl ResultIds for Ids {
    fn next_id(&self) -> Id {
        match &self.reused {
            Some(used) => used.clone(),
            None => id(100 + self.next.fetch_add(1, Ordering::SeqCst)),
        }
    }
}

struct Identity(AtomicU64);
impl JobIdentity for Identity {
    fn next_id(&self) -> Id {
        id(9000 + self.0.fetch_add(1, Ordering::SeqCst))
    }
}

struct Clock;
impl JobClock for Clock {
    fn now_ms(&self) -> i64 {
        4_000_000_000_000
    }
}

struct History;
impl JobHistory for History {
    fn append(&self, _: &JobSummary) -> Result<HistoryAppend, String> {
        Ok(HistoryAppend::Added)
    }
}

struct Events(Mutex<mpsc::Sender<JobEvent>>);
impl JobEvents for Events {
    fn publish(&self, event: JobEvent) {
        if let Ok(sender) = self.0.lock() {
            let _ = sender.send(event);
        }
    }
}

/// Runs once, when the folder returns its entries.
type Hook = Arc<Mutex<Option<Box<dyn FnOnce() + Send>>>>;

fn entry(name: &str, kind: ObservedKind, inode: u64) -> ObservedEntry {
    ObservedEntry {
        name: name.as_bytes().to_vec(),
        enumerated_inode: inode,
        enumerated_kind: kind.clone(),
        metadata: Ok(MetadataObservation {
            kind,
            device: 1,
            inode,
            size: Ok(3),
            allocated: None,
            link_count: None,
            space_identity: None,
            modified_at: None,
            native_modified_at: None,
        }),
    }
}

fn files() -> Vec<ObservedEntry> {
    vec![
        entry("a.txt", ObservedKind::File, 10),
        entry("b.txt", ObservedKind::File, 11),
    ]
}

fn files_and_folder() -> Vec<ObservedEntry> {
    let mut entries = files();
    entries.push(entry("folder", ObservedKind::Directory, FOLDER));
    entries
}

/// A directory that returns its entries in one batch, or fails its first
/// read. The folder holds one file.
struct Dir {
    inode: u64,
    entries: Option<Vec<ObservedEntry>>,
    fail: bool,
    hook: Hook,
}

impl Dir {
    fn root(entries: Vec<ObservedEntry>) -> Self {
        Self {
            inode: ROOT,
            entries: Some(entries),
            fail: false,
            hook: Hook::default(),
        }
    }
}

impl DirectoryObservations for Dir {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        if self.fail {
            return Err(PortError {
                kind: PortErrorKind::PermissionDenied,
                reason_key: "os.denied".into(),
                os_code: None,
            });
        }
        let batch = self.entries.take().unwrap_or_default();
        if self.inode == FOLDER && !batch.is_empty() {
            let hook = self.hook.lock().ok().and_then(|mut hook| hook.take());
            if let Some(hook) = hook {
                hook();
            }
        }
        Ok(batch)
    }
}

impl AnchoredDirectory for Dir {
    fn observed_identity(&self) -> Option<HandleIdentity> {
        Some(HandleIdentity {
            device: 1,
            inode: self.inode,
            ancestors: Vec::new(),
        })
    }

    fn open_child(&self, child: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Ok(Box::new(Dir {
            inode: child.enumerated_inode,
            entries: Some(vec![entry("inner.txt", ObservedKind::File, 30)]),
            fail: false,
            hook: Arc::clone(&self.hook),
        }))
    }
}

fn opened(root: Dir) -> OpenedScopeRoot {
    let raw: Vec<u8> = "C:\\R".encode_utf16().flat_map(u16::to_le_bytes).collect();
    OpenedScopeRoot {
        canonical_path: OsPath {
            encoding: PathEncoding::WindowsUtf16le,
            raw: PathBytes::from_bytes(&raw),
            display: "C:\\R".into(),
            lossy: false,
        },
        volume_id: vec![1],
        file_id: vec![1],
        directory: Box::new(root),
    }
}

struct Options {
    /// Publish with a version ID the store already used.
    reuse: bool,
    /// Cancel the job when the folder returns its entries.
    cancel_in_folder: bool,
}

/// Runs `root` as a metadata scan job in a runtime and returns its terminal
/// summary and the result store, which already holds an older result.
fn run_scan(
    root: Dir,
    options: Options,
) -> Result<(JobSummary, IngestSink), Box<dyn std::error::Error>> {
    run_scan_with_rule(root, options, None)
}

/// Like [`run_scan`], binding `rule` to the job when one is given.
fn run_scan_with_rule(
    root: Dir,
    options: Options,
    rule: Option<BoundSizeRule>,
) -> Result<(JobSummary, IngestSink), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let mut previous = sink.open(id(1)).map_err(|e| format!("{e:?}"))?;
    previous
        .add_directory(Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    let used = previous
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?
        .version_id()
        .clone();
    let ids = Arc::new(Ids {
        next: AtomicU64::new(0),
        reused: options.reuse.then_some(used),
    });
    let (sender, events) = mpsc::channel();
    let runtime = Arc::new(JobRuntime::new_with_limits(
        Arc::new(Identity(AtomicU64::new(0))),
        Arc::new(Clock),
        Arc::new(History),
        Arc::new(Events(Mutex::new(sender))),
        FlowLimits {
            metadata_io: 4,
            queued_items: 4,
            item_bytes: 1024,
        },
    )?);
    runtime.register(descriptor())?;
    let hook = Arc::clone(&root.hook);
    let mut scan = MetadataScan::new_roots(vec![opened(root)], sink.clone(), id(3), ids);
    if let Some(rule) = rule {
        scan = scan.with_size_rule(rule);
    }
    let created = runtime.submit(Submission {
        kind_id: "scan.metadata".into(),
        deduplication_key: None,
        body: Arc::new(scan),
        priority: JobPriority::Normal,
        scope_summary: "scan".into(),
        correlation_id: id(4),
        sample: false,
    })?;
    let SubmitResult::Created(job) = created else {
        return Err("expected a created job".into());
    };
    if options.cancel_in_folder {
        let (runtime, job) = (Arc::clone(&runtime), job.clone());
        let cancel: Box<dyn FnOnce() + Send> = Box::new(move || {
            let _ = runtime.cancel(&job);
        });
        *hook.lock().map_err(|_| "hook")? = Some(cancel);
    }
    assert_eq!(runtime.run_next()?, Some(job.clone()));
    loop {
        if let JobEvent::Finished { summary, .. } = events.recv_timeout(Duration::from_secs(5))?
            && summary.job_id == job
        {
            assert_eq!(runtime.state(&job)?, summary.state);
            return Ok((*summary, sink));
        }
    }
}

fn counts(summary: &JobSummary) -> (u64, u64, u64) {
    let counts = &summary.counts;
    (counts.completed, counts.failed, counts.unprocessed)
}

#[test]
fn a_scan_without_a_result_ends_failed_even_after_counting_items() -> TestResult {
    // The root and two files were counted before publication failed. Before
    // the rule the job ended partial with completed 3.
    let options = Options {
        reuse: true,
        cancel_in_folder: false,
    };
    let (summary, _) = run_scan(Dir::root(files()), options)?;
    assert_eq!(summary.state, JobState::Failed);
    assert_eq!(summary.result_version_id, None);
    assert_eq!(
        summary.failure_reason_key.as_deref(),
        Some("support.internal_error")
    );
    assert_eq!(counts(&summary), (0, 0, 3));

    // The first root read fails: the root is the only failure.
    let mut root = Dir::root(Vec::new());
    root.fail = true;
    let options = Options {
        reuse: false,
        cancel_in_folder: false,
    };
    let (summary, _) = run_scan(root, options)?;
    assert_eq!(summary.state, JobState::Failed);
    assert_eq!(summary.result_version_id, None);
    assert_eq!(counts(&summary), (0, 1, 0));
    Ok(())
}

#[test]
fn a_cancelled_scan_with_a_published_result_ends_cancelled() -> TestResult {
    let options = Options {
        reuse: false,
        cancel_in_folder: true,
    };
    let (summary, sink) = run_scan(Dir::root(files_and_folder()), options)?;
    assert_eq!(summary.state, JobState::Cancelled);
    let version = summary
        .result_version_id
        .as_ref()
        .ok_or("a cancelled scan keeps its result")?;
    let result = sink.pin_current(version).map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.completeness(), ResultCompleteness::Cancelled);
    // The result keeps what was read, so its counts stay completed.
    assert!(summary.counts.completed >= 4, "{:?}", summary.counts);
    assert_eq!(summary.counts.failed, 0);
    Ok(())
}

#[test]
fn a_scan_that_reads_its_whole_scope_ends_done() -> TestResult {
    let options = Options {
        reuse: false,
        cancel_in_folder: false,
    };
    let (summary, sink) = run_scan(Dir::root(files_and_folder()), options)?;
    assert_eq!(summary.state, JobState::Done);
    let version = summary.result_version_id.as_ref().ok_or("missing result")?;
    let result = sink.pin_current(version).map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.completeness(), ResultCompleteness::Complete);
    // The root, two files, the folder and its file.
    assert_eq!(counts(&summary), (5, 0, 0));
    Ok(())
}

#[test]
fn a_bound_size_rule_is_reported_by_the_sealed_result() -> TestResult {
    let mut registry = SizeRuleRegistryBuilder::new();
    registry.register_defaults().map_err(|e| format!("{e:?}"))?;
    let registry = registry.build().map_err(|e| format!("{e:?}"))?;
    let rule = registry
        .latest(DEFAULT_SIZE_RULE_ID)
        .ok_or("default rule missing")?;
    let options = Options {
        reuse: false,
        cancel_in_folder: false,
    };
    let (summary, sink) = run_scan_with_rule(Dir::root(files_and_folder()), options, Some(rule))?;
    // A fresh build accepts the rule, so the job is not failed by the binding.
    assert_eq!(summary.state, JobState::Done);
    let version = summary.result_version_id.as_ref().ok_or("missing result")?;
    let result = sink.pin_current(version).map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.size_rule(), rule.identity());
    Ok(())
}
