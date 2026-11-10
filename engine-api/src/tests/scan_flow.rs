use super::*;
use du_core_contracts::{
    ItemRef, OsPath, PathBytes, PathEncoding,
    analysis_ipc::{ScanStart, ScopeReview},
    job::{HistoryAppend, JobHistory, JobSummary},
    ports::{PortError, PortErrorKind},
    result::ResultVersionRead,
    scan_observation::{
        AnchoredDirectory, DirectoryObservations, MetadataObservation, ObservedEntry, ObservedKind,
        OpenedScopeRoot, ScopeRootPort,
    },
};
use du_job_runtime::{FlowLimits, JobClock, JobEvent, JobEvents, JobIdentity, JobRuntime};
use std::{
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

struct Ids {
    next: AtomicUsize,
    fallback: Id,
}

impl JobIdentity for Ids {
    fn next_id(&self) -> Id {
        let next = self.next.fetch_add(1, Ordering::Relaxed);
        Id::parse(&format!("018f0000-0000-7000-8000-{next:012x}"))
            .unwrap_or_else(|_| self.fallback.clone())
    }
}

struct Clock;

impl JobClock for Clock {
    fn now_ms(&self) -> i64 {
        1_000
    }
}

struct History;

impl JobHistory for History {
    fn append(&self, _: &JobSummary) -> Result<HistoryAppend, String> {
        Ok(HistoryAppend::Added)
    }
}

struct Events(mpsc::Sender<JobEvent>);

impl JobEvents for Events {
    fn publish(&self, event: JobEvent) {
        if matches!(event, JobEvent::Finished { .. }) {
            let _ = self.0.send(event);
        }
    }
}

struct Roots {
    file_id: AtomicUsize,
    opens: AtomicUsize,
    reads: Arc<AtomicUsize>,
    release: Arc<Mutex<mpsc::Receiver<()>>>,
}

impl ScopeRootPort for Roots {
    fn open_root(&self, path: &[u8]) -> Result<OpenedScopeRoot, PortError> {
        self.opens.fetch_add(1, Ordering::Relaxed);
        Ok(OpenedScopeRoot {
            canonical_path: OsPath {
                encoding: PathEncoding::WindowsUtf16le,
                raw: PathBytes::from_bytes(path),
                display: "C:\\Fixture".into(),
                lossy: false,
            },
            volume_id: vec![1],
            file_id: self.file_id.load(Ordering::Acquire).to_le_bytes().to_vec(),
            directory: Box::new(Directory {
                reads: self.reads.clone(),
                release: Some(self.release.clone()),
            }),
        })
    }
}

struct Directory {
    reads: Arc<AtomicUsize>,
    release: Option<Arc<Mutex<mpsc::Receiver<()>>>>,
}

impl DirectoryObservations for Directory {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        let Some(release) = self.release.take() else {
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
            }),
        }])
    }
}

impl AnchoredDirectory for Directory {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(io_error())
    }
}

fn io_error() -> PortError {
    PortError {
        kind: PortErrorKind::Io,
        reason_key: "os.query_failed".into(),
        os_code: None,
    }
}

#[test]
fn reviewed_identity_is_rechecked_and_one_job_publishes_the_requested_result()
-> Result<(), Box<dyn std::error::Error>> {
    let ids = Arc::new(Ids {
        next: AtomicUsize::new(1),
        fallback: Id::parse("018f0000-0000-7000-8000-000000000000")?,
    });
    let (release, blocked) = mpsc::channel();
    let (send, terminal) = mpsc::channel();
    let roots = Arc::new(Roots {
        file_id: AtomicUsize::new(1),
        opens: AtomicUsize::new(0),
        reads: Arc::new(AtomicUsize::new(0)),
        release: Arc::new(Mutex::new(blocked)),
    });
    let clock = Arc::new(Clock);
    let runtime = Arc::new(JobRuntime::new_with_limits(
        ids.clone(),
        clock.clone(),
        Arc::new(History),
        Arc::new(Events(send)),
        FlowLimits {
            metadata_io: 2,
            queued_items: 4,
            item_bytes: 1024,
        },
    )?);
    let mut builder = EngineBuilder::default();
    crate::jobs::register(&mut builder, runtime.clone());
    crate::scope::register_review(&mut builder, roots.clone(), clock);
    let workers = Arc::new(crate::workers::Workers::start(runtime)?);
    let versions = crate::scan::register(&mut builder, workers, roots.clone(), ids)?;
    let engine = builder.build()?;
    let gui = |name, body| call(&engine, name, Channel::Gui, ENVELOPE_VERSION, body);
    let path: Vec<u8> = "C:\\Fixture"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let draft = json!({
        "includes": [{
            "encoding": "windows_utf16le",
            "raw": PathBytes::from_bytes(&path),
            "display": "untrusted label",
            "lossy": true
        }],
        "exclusions": [],
        "boundary": "current_volume",
        "includedMounts": [],
        "source": "gui"
    });
    // Until multi-root collection and result identity are composed, reject the
    // entire scope before opening any path or submitting a partial scan.
    let mut other = draft["includes"][0].clone();
    let other_path: Vec<u8> = "D:\\Other"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    other["raw"] = json!(PathBytes::from_bytes(&other_path));
    for second in [draft["includes"][0].clone(), other] {
        let mut multi = draft.clone();
        multi["includes"] = json!([draft["includes"][0], second]);
        let rejected_review = gui("scope.review", json!({"draft": multi, "requestSeq": 0}))?;
        assert_eq!(
            rejected_review
                .error
                .ok_or("multi-root review unexpectedly succeeded")?
                .reason_key,
            "scope.target_unavailable"
        );
        let rejected_start = gui(
            "scan.start",
            json!({"draft": multi, "reviewFingerprint": "unreviewed"}),
        )?;
        assert_eq!(
            rejected_start
                .error
                .ok_or("multi-root scan unexpectedly started")?
                .reason_key,
            "scope.target_unavailable"
        );
    }
    assert_eq!(roots.reads.load(Ordering::Acquire), 0);
    assert_eq!(roots.opens.load(Ordering::Acquire), 0);
    let review = |seq| -> Result<ScopeReview, Box<dyn std::error::Error>> {
        Ok(serde_json::from_value(
            gui(
                "scope.review",
                json!({
                    "draft": draft, "requestSeq": seq
                }),
            )?
            .body,
        )?)
    };
    let first = review(1)?;
    assert_eq!(first.scope.includes[0].display, "C:\\Fixture");
    assert_eq!(first.review_fingerprint, review(2)?.review_fingerprint);
    assert_eq!(first.review_fingerprint.len(), 64);
    let start = |fingerprint: &str| {
        gui(
            "scan.start",
            json!({
                "draft": draft, "reviewFingerprint": fingerprint
            }),
        )
    };
    roots.file_id.store(3, Ordering::Release);
    assert_eq!(
        start(&first.review_fingerprint)?
            .error
            .ok_or("expected stale error")?
            .reason_key,
        "scope.changed"
    );
    assert_eq!(roots.reads.load(Ordering::Acquire), 0);

    let second = review(3)?;
    let receipt: ScanStart = serde_json::from_value(start(&second.review_fingerprint)?.body)?;
    let duplicate: ScanStart = serde_json::from_value(start(&second.review_fingerprint)?.body)?;
    assert!(!receipt.existing);
    assert!(duplicate.existing);
    assert_eq!(receipt.job_ref, duplicate.job_ref);
    assert_eq!(receipt.scope_version, duplicate.scope_version);
    assert_ne!(receipt.scope_fingerprint, second.review_fingerprint);
    roots.file_id.store(4, Ordering::Release);
    let replacement = review(4)?;
    assert_ne!(second.review_fingerprint, replacement.review_fingerprint);
    let busy = start(&replacement.review_fingerprint)?
        .error
        .ok_or("replacement reused the active job")?;
    assert_eq!(busy.reason_key, "job.queue_full");
    release.send(())?;
    let JobEvent::Finished {
        summary,
        history_recorded,
        ..
    } = terminal.recv_timeout(Duration::from_secs(5))?
    else {
        return Err("missing terminal event".into());
    };
    assert_eq!(summary.job_id, receipt.job_ref);
    assert_eq!(summary.scope_summary, "metadata: 1 root");
    assert!(history_recorded);
    let version = summary.result_version_id.ok_or("missing result version")?;
    let response = gui(
        "space.total",
        json!({
            "resultVersion": version,
            "target": {"kind": "scope_root"},
            "requestSeq": 6
        }),
    )?;
    assert_eq!(response.body["resultVersion"], json!(version));
    assert_eq!(response.body["sizes"][0]["size"]["bytes"], "64");
    check_observed_details(&engine, &versions, version, path)?;
    assert_eq!(roots.reads.load(Ordering::Acquire), 2);

    let next: ScanStart = serde_json::from_value(start(&replacement.review_fingerprint)?.body)?;
    assert!(!next.existing);
    assert_ne!(receipt.job_ref, next.job_ref);
    assert_eq!(receipt.scope_fingerprint, next.scope_fingerprint);
    release.send(())?;
    let JobEvent::Finished { summary, .. } = terminal.recv_timeout(Duration::from_secs(5))? else {
        return Err("missing replacement terminal event".into());
    };
    assert_eq!(summary.job_id, next.job_ref);
    Ok(())
}

fn check_observed_details(
    engine: &Engine,
    versions: &du_analysis_index::IngestSink,
    version: Id,
    path: Vec<u8>,
) -> Result<(), Box<dyn std::error::Error>> {
    let gui = |name, body| call(engine, name, Channel::Gui, ENVELOPE_VERSION, body);
    let pinned = versions
        .pin_current(&version)
        .map_err(|error| format!("{error:?}"))?;
    let observed = pinned.observed_root().ok_or("missing observed root")?;
    assert_eq!(observed.path.raw.bytes()?, path);
    assert_eq!(observed.volume_id, [1]);
    assert_eq!(observed.file_id, 3usize.to_le_bytes());
    assert_eq!(
        pinned.observed_at().map(|time| time.as_str()),
        Some("1970-01-01T00:00:01.000Z")
    );
    let detail = gui(
        "browse.detail",
        json!({
            "resultVersion": version,
            "item": { "resultVersion": version, "itemNo": 1 }
        }),
    )?;
    assert_eq!(detail.body["displayPath"]["value"], "C:\\Fixture\\f");
    assert_eq!(detail.body["logicalSize"]["value"]["bytes"], "64");
    assert_eq!(detail.body["allocatedSize"]["state"], "unknown");
    assert_eq!(detail.body["modifiedAt"]["state"], "unknown");
    assert_eq!(detail.body["observationOnly"], true);
    assert!(!detail.body.to_string().contains("\"raw\""));
    let missing = gui(
        "browse.detail",
        json!({
            "resultVersion": version,
            "item": { "resultVersion": version, "itemNo": 3 }
        }),
    )?;
    assert_eq!(
        missing.error.ok_or("missing browse error")?.reason_key,
        "ref.unavailable"
    );
    let child = pinned
        .native_path(&ItemRef {
            result_version: version,
            item_no: 1,
        })
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(child.raw.bytes()?, [path, vec![b'\\', 0, b'f', 0]].concat());
    Ok(())
}
