use super::*;
use du_core_contracts::{
    ItemRef, OsPath, PathBytes, PathEncoding,
    analysis_ipc::{ScanStart, ScopeReview},
    job::{HistoryAppend, JobHistory, JobSummary},
    ports::{PortError, PortErrorKind},
    result::ResultVersionRead,
    result_events::{PublishedCompleteness, ResultVersionNotice},
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
        if matches!(
            event,
            JobEvent::Finished { .. } | JobEvent::ResultVersionChanged(_)
        ) {
            let _ = self.0.send(event);
        }
    }
}

struct Roots {
    file_id: AtomicUsize,
    opens: AtomicUsize,
    reads: Arc<AtomicUsize>,
    recent_observations: Arc<AtomicUsize>,
    recent_opens: Arc<AtomicUsize>,
    recent_live_handles: Arc<AtomicUsize>,
    recent_unavailable: std::sync::atomic::AtomicBool,
    read_handles: Arc<Mutex<Vec<usize>>>,
    release: Arc<Mutex<mpsc::Receiver<()>>>,
    finish_gate: Option<Arc<Mutex<mpsc::Receiver<()>>>>,
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
        recent::open_with_observer(self, path)
    }
}

struct Directory {
    handle: usize,
    read_handles: Arc<Mutex<Vec<usize>>>,
    reads: Arc<AtomicUsize>,
    release: Option<Arc<Mutex<mpsc::Receiver<()>>>>,
    finish_gate: Option<Arc<Mutex<mpsc::Receiver<()>>>>,
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

struct ScanHarness {
    engine: Engine,
    roots: Arc<Roots>,
    release: mpsc::Sender<()>,
    terminal: mpsc::Receiver<JobEvent>,
    versions: du_analysis_index::IngestSink,
    recent_scope_admissions: Option<crate::desktop::RecentScopeAdmissions>,
}

fn scan_harness() -> Result<ScanHarness, Box<dyn std::error::Error>> {
    scan_harness_with_gate(None)
}

mod fixture;
mod recent;
mod settings;
mod terminal;
mod unsupported;
use fixture::scan_harness_with_gate;
use fixture::scan_harness_with_recent_admissions;
use terminal::check_terminal_scan;

#[test]
fn reviewed_identity_is_rechecked_and_one_job_publishes_the_requested_result()
-> Result<(), Box<dyn std::error::Error>> {
    let ScanHarness {
        engine,
        roots,
        release,
        terminal,
        versions,
        ..
    } = scan_harness()?;
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
    unsupported::check_uncomposed_scope_fails_closed(&engine, &roots, &draft)?;
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
    let created_handle = roots.opens.load(Ordering::Acquire) + 1;
    let receipt: ScanStart = serde_json::from_value(start(&second.review_fingerprint)?.body)?;
    let duplicate: ScanStart = serde_json::from_value(start(&second.review_fingerprint)?.body)?;
    assert!(!receipt.existing);
    assert!(duplicate.existing);
    assert_eq!(receipt.job_ref, duplicate.job_ref);
    assert_eq!(receipt.scope_version, duplicate.scope_version);
    assert_eq!(receipt.scope_fingerprint, duplicate.scope_fingerprint);
    assert_eq!(receipt.lineage_id, duplicate.lineage_id);
    assert_ne!(receipt.scope_fingerprint, second.review_fingerprint);
    roots.file_id.store(4, Ordering::Release);
    let replacement = review(4)?;
    assert_ne!(second.review_fingerprint, replacement.review_fingerprint);
    let busy = start(&replacement.review_fingerprint)?
        .error
        .ok_or("replacement reused the active job")?;
    assert_eq!(busy.reason_key, "job.queue_full");
    release.send(())?;
    let (summary, history_recorded) = scan_finished_with_publications(&terminal)?;
    let version = check_terminal_scan(&engine, *summary, history_recorded, &receipt.job_ref)?;
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
    check_observed_details(&engine, &versions, version.clone(), path)?;
    assert_eq!(roots.reads.load(Ordering::Acquire), 2);
    assert_eq!(
        *roots
            .read_handles
            .lock()
            .map_err(|_| "handle log poisoned")?,
        vec![created_handle, created_handle]
    );

    check_replacement_submission(
        &receipt,
        start(&replacement.review_fingerprint)?.body,
        &release,
        &terminal,
        &versions,
        &version,
    )?;
    Ok(())
}

fn check_replacement_submission(
    receipt: &ScanStart,
    response: serde_json::Value,
    release: &mpsc::Sender<()>,
    terminal: &mpsc::Receiver<JobEvent>,
    versions: &du_analysis_index::IngestSink,
    first_version: &Id,
) -> Result<(), Box<dyn std::error::Error>> {
    let first = versions
        .pin_current(first_version)
        .map_err(|error| format!("{error:?}"))?;
    let next: ScanStart = serde_json::from_value(response)?;
    assert!(!next.existing);
    assert_ne!(receipt.job_ref, next.job_ref);
    assert_eq!(receipt.scope_fingerprint, next.scope_fingerprint);
    release.send(())?;
    let (summary, _) = scan_finished(terminal)?;
    assert_eq!(summary.job_id, next.job_ref);
    let second_id = summary.result_version_id.ok_or("missing second result")?;
    let second = versions
        .pin_current(&second_id)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(first.lineage_id(), second.lineage_id());
    assert!(first.lineage_id().is_some());
    assert_ne!(first.scope_version_id(), second.scope_version_id());
    assert!(versions.pin_current(first_version).is_err());
    assert_eq!(first.items().len(), 2);
    Ok(())
}

fn scan_finished(
    terminal: &mpsc::Receiver<JobEvent>,
) -> Result<(Box<JobSummary>, bool), Box<dyn std::error::Error>> {
    let delivery = scan_delivery(terminal)?;
    Ok((delivery.summary, delivery.history_recorded))
}

fn scan_finished_with_publications(
    terminal: &mpsc::Receiver<JobEvent>,
) -> Result<(Box<JobSummary>, bool), Box<dyn std::error::Error>> {
    let delivery = scan_delivery(terminal)?;
    let [partial, final_result] = delivery.notices.as_slice() else {
        return Err("expected one partial and one final publication before completion".into());
    };
    assert_eq!(partial.body.sequence, 1);
    assert_eq!(
        partial.body.completeness,
        Some(PublishedCompleteness::Partial)
    );
    assert_eq!(final_result.body.sequence, 2);
    assert_eq!(
        final_result.body.completeness,
        Some(PublishedCompleteness::Complete)
    );
    assert_eq!(partial.body.lineage_id, final_result.body.lineage_id);
    assert_eq!(
        partial.result_version,
        final_result.body.previous_result_version
    );
    assert_eq!(
        final_result.result_version,
        delivery.summary.result_version_id
    );
    Ok((delivery.summary, delivery.history_recorded))
}
struct ScanDelivery {
    summary: Box<JobSummary>,
    history_recorded: bool,
    notices: Vec<ResultVersionNotice>,
}
fn scan_delivery(
    terminal: &mpsc::Receiver<JobEvent>,
) -> Result<ScanDelivery, Box<dyn std::error::Error>> {
    let mut notices = Vec::new();
    loop {
        match terminal.recv_timeout(Duration::from_secs(5))? {
            JobEvent::ResultVersionChanged(notice) => notices.push(*notice),
            JobEvent::Finished {
                summary,
                history_recorded,
                ..
            } => {
                return Ok(ScanDelivery {
                    summary,
                    history_recorded,
                    notices,
                });
            }
            _ => return Err("missing terminal event".into()),
        }
    }
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
    assert_eq!(
        detail.body["modifiedAt"]["value"],
        "2020-01-02T03:04:05.006Z"
    );
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

mod exclusions;
mod multiple;

mod partial_status;
