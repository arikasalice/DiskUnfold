//! A real scan job reports progress through the composed desktop event chain.
//!
//! The runtime sends its events to the sample tap, which relays them to the scan-completion
//! relay and then to the desktop queue. Unit tests cover each link; this one runs the chain
//! that `desktop::register` assembles (F24 IF-06 `job.progress`).

use std::{
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

use du_core_contracts::{
    EventEnvelope, OsPath, PathBytes, PathEncoding,
    analysis_ipc::ScanStart,
    ports::{PortError, PortErrorKind},
    scan_observation::{
        AnchoredDirectory, DirectoryObservations, FilterDecision, FilteredBatch,
        MetadataObservation, ObservedEntry, ObservedKind, OpenedScopeRoot, ScopeRootPort,
    },
};
use serde_json::json;

use super::{CountingHistory, Entropy, NoVolumes, call};
use crate::api::{Channel, EngineBuilder};
use du_core_contracts::scan_observation::ObservedValue;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Opens one root that holds one file and then waits for the test before it ends.
struct Roots(Arc<Mutex<mpsc::Receiver<()>>>);

impl ScopeRootPort for Roots {
    fn open_root(&self, path: &[u8]) -> Result<OpenedScopeRoot, PortError> {
        Ok(OpenedScopeRoot {
            canonical_path: OsPath {
                encoding: PathEncoding::WindowsUtf16le,
                raw: PathBytes::from_bytes(path),
                display: "C:\\Fixture".into(),
                lossy: false,
            },
            volume_id: vec![1],
            file_id: vec![1],
            directory: Box::new(Held {
                gate: self.0.clone(),
                batches: 0,
            }),
        })
    }
}

struct Held {
    gate: Arc<Mutex<mpsc::Receiver<()>>>,
    batches: u8,
}

impl DirectoryObservations for Held {
    fn next_filtered(
        &mut self,
        decide: &mut dyn FnMut(&[u8]) -> FilterDecision,
    ) -> Result<FilteredBatch, PortError> {
        match decide(&[b'f', 0]) {
            FilterDecision::Admit => self.next_batch().map(FilteredBatch::Entries),
            FilterDecision::Exclude => Ok(FilteredBatch::End),
            FilterDecision::Undecidable => Ok(FilteredBatch::Undecidable),
        }
    }

    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        self.batches += 1;
        if self.batches > 1 {
            // Keep the job running until the test saw its progress.
            self.gate
                .lock()
                .map_err(|_| io_error())?
                .recv_timeout(Duration::from_secs(10))
                .map_err(|_| io_error())?;
            return Ok(Vec::new());
        }
        Ok(vec![ObservedEntry {
            name: vec![b'f', 0],
            enumerated_inode: 2,
            enumerated_kind: ObservedKind::File,
            metadata: Ok(MetadataObservation {
                kind: ObservedKind::File,
                device: 1,
                inode: 2,
                size: Ok(64),
                allocated: ObservedValue::NotApplicable,
                link_count: ObservedValue::NotApplicable,
                space_identity: ObservedValue::NotApplicable,
                modified_at: ObservedValue::NotApplicable,
                native_modified_at: None,
            }),
        }])
    }
}

impl AnchoredDirectory for Held {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(io_error())
    }

    fn observed_identity(&self) -> Option<du_core_contracts::scan_observation::HandleIdentity> {
        Some(du_core_contracts::scan_observation::HandleIdentity {
            device: 1,
            inode: 1,
            ancestors: Vec::new(),
        })
    }
}

fn io_error() -> PortError {
    PortError {
        kind: PortErrorKind::Io,
        reason_key: "os.query_failed".into(),
        os_code: None,
    }
}

/// Reads the next desktop event within `deadline`.
fn next(
    events: &crate::desktop::DesktopEvents,
    deadline: Instant,
) -> Result<EventEnvelope, Box<dyn std::error::Error>> {
    loop {
        if let Some(wire) = events.next_json() {
            return Ok(serde_json::from_str(&wire)?);
        }
        if Instant::now() > deadline {
            return Err("the desktop queue stayed empty".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_running_scan_reports_job_progress_through_the_sample_tap() -> TestResult {
    let (release, gate) = mpsc::channel();
    let mut builder = EngineBuilder::default();
    let events = crate::desktop::register(
        &mut builder,
        Arc::new(Roots(Arc::new(Mutex::new(gate)))),
        Arc::new(NoVolumes),
        Arc::new(CountingHistory::default()),
        &Entropy,
    )?;
    let engine = builder.build()?;

    let path: Vec<u8> = "C:\\Fixture"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let draft = json!({
        "includes": [{
            "encoding": "windows_utf16le",
            "raw": PathBytes::from_bytes(&path),
            "display": "C:\\Fixture",
            "lossy": false
        }],
        "exclusions": [],
        "boundary": "current_volume",
        "includedMounts": [],
        "source": "gui"
    });
    let review = call(
        &engine,
        "scope.review",
        Channel::Gui,
        json!({"draft": draft, "requestSeq": 1}),
    );
    let fingerprint = review["body"]["reviewFingerprint"].clone();
    assert!(fingerprint.is_string(), "review failed: {review}");
    let started = call(
        &engine,
        "scan.start",
        Channel::Gui,
        json!({"draft": draft, "reviewFingerprint": fingerprint}),
    );
    let receipt: ScanStart = serde_json::from_value(started["body"].clone())
        .map_err(|error| format!("scan.start failed: {started}: {error}"))?;

    // The scan processed its one file and now waits, so the runtime ticks its progress.
    let deadline = Instant::now() + Duration::from_secs(10);
    let progress = loop {
        let event = next(&events, deadline)?;
        if event.topic == "job.progress" && event.job_ref.as_ref() == Some(&receipt.job_ref) {
            break event;
        }
    };
    assert!(progress.body["completed"].as_u64().is_some_and(|n| n >= 1));
    assert_eq!(progress.body["unit"], "items");

    release.send(())?;
    let finished = loop {
        let event = next(&events, deadline)?;
        if event.topic == "job.finished" && event.job_ref.as_ref() == Some(&receipt.job_ref) {
            break event;
        }
    };
    assert!(finished.seq > progress.seq);
    Ok(())
}
