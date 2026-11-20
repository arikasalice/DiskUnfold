//! A real desktop scan relays lane status before its terminal event.
use std::{
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

use du_core_contracts::{
    EventEnvelope, OsPath, PathBytes, PathEncoding,
    analysis_ipc::{ScanLaneStatus, ScanStart},
    ports::{PortError, PortErrorKind},
    scan_observation::{AnchoredDirectory, DirectoryObservations, OpenedScopeRoot, ScopeRootPort},
};
use serde_json::json;

use super::{CountingHistory, Entropy, NoVolumes, call};
use crate::api::{Channel, EngineBuilder};

type TestResult = Result<(), Box<dyn std::error::Error>>;
type Gate = Arc<(Mutex<bool>, Condvar)>;

struct ReleaseOnDrop(Gate);

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        let (released, wake) = &*self.0;
        if let Ok(mut released) = released.lock() {
            *released = true;
            wake.notify_all();
        }
    }
}

struct Roots(Gate);

impl ScopeRootPort for Roots {
    fn open_root(&self, path: &[u8]) -> Result<OpenedScopeRoot, PortError> {
        let second = path.first() == Some(&b'D');
        Ok(OpenedScopeRoot {
            canonical_path: OsPath {
                encoding: PathEncoding::WindowsUtf16le,
                raw: PathBytes::from_bytes(path),
                display: if second { "D:\\Fixture" } else { "C:\\Fixture" }.into(),
                lossy: false,
            },
            volume_id: vec![if second { 2 } else { 1 }],
            file_id: vec![if second { 2 } else { 1 }],
            directory: Box::new(WaitingDirectory {
                gate: self.0.clone(),
                device: if second { 2 } else { 1 },
                inode: if second { 20 } else { 10 },
                waited: false,
            }),
        })
    }
}

struct WaitingDirectory {
    gate: Gate,
    device: u64,
    inode: u64,
    waited: bool,
}

impl DirectoryObservations for WaitingDirectory {
    fn next_batch(
        &mut self,
    ) -> Result<Vec<du_core_contracts::scan_observation::ObservedEntry>, PortError> {
        if !self.waited {
            self.waited = true;
            let (released, wake) = &*self.gate;
            let mut released = released.lock().map_err(|_| port_error())?;
            while !*released {
                released = wake.wait(released).map_err(|_| port_error())?;
            }
        }
        Ok(Vec::new())
    }
}

impl AnchoredDirectory for WaitingDirectory {
    fn observed_identity(&self) -> Option<du_core_contracts::scan_observation::HandleIdentity> {
        Some(du_core_contracts::scan_observation::HandleIdentity {
            device: self.device,
            inode: self.inode,
            ancestors: Vec::new(),
        })
    }

    fn open_child(
        &self,
        _: &du_core_contracts::scan_observation::ObservedEntry,
    ) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(port_error())
    }
}

fn port_error() -> PortError {
    PortError {
        kind: PortErrorKind::Io,
        reason_key: "os.query_failed".into(),
        os_code: None,
    }
}

fn next_event(
    events: &crate::desktop::DesktopEvents,
    deadline: Instant,
) -> Result<EventEnvelope, Box<dyn std::error::Error>> {
    loop {
        if let Some(wire) = events.next_json() {
            return Ok(serde_json::from_str(&wire)?);
        }
        if Instant::now() >= deadline {
            return Err("the desktop event queue stayed empty".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn encoded_path(path: &str) -> serde_json::Value {
    let bytes: Vec<u8> = path.encode_utf16().flat_map(u16::to_le_bytes).collect();
    json!({
        "encoding": "windows_utf16le",
        "raw": PathBytes::from_bytes(&bytes),
        "display": path,
        "lossy": false
    })
}

fn is_decimal(value: &str) -> bool {
    value
        .parse::<u64>()
        .is_ok_and(|number| number.to_string() == value)
}

#[test]
fn a_desktop_scan_reports_each_storage_lane_before_job_finished() -> TestResult {
    let gate: Gate = Arc::new((Mutex::new(false), Condvar::new()));
    let _release_on_drop = ReleaseOnDrop(gate.clone());
    let mut builder = EngineBuilder::default();
    let events = crate::desktop::register(
        &mut builder,
        Arc::new(Roots(gate.clone())),
        Arc::new(NoVolumes),
        Arc::new(CountingHistory::default()),
        &Entropy,
    )?;
    let engine = builder.build()?;
    let draft = json!({
        "includes": [encoded_path("C:\\Fixture"), encoded_path("D:\\Fixture")],
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
    let fingerprint = review["body"]["reviewFingerprint"]
        .as_str()
        .ok_or_else(|| format!("scope.review failed: {review}"))?
        .to_owned();
    let started = call(
        &engine,
        "scan.start",
        Channel::Gui,
        json!({"draft": draft, "reviewFingerprint": fingerprint}),
    );
    let receipt: ScanStart = serde_json::from_value(started["body"].clone())
        .map_err(|error| format!("scan.start failed: {started}: {error}"))?;

    let deadline = Instant::now() + Duration::from_secs(10);
    let lane_event = loop {
        let event = next_event(&events, deadline)?;
        if event.topic == "scan.lane_status" && event.job_ref.as_ref() == Some(&receipt.job_ref) {
            break event;
        }
    };
    let status: ScanLaneStatus = serde_json::from_value(lane_event.body.clone())?;
    assert_eq!(
        lane_event.contract_version,
        du_core_contracts::ENVELOPE_VERSION
    );
    assert_eq!(
        status.lanes.len(),
        2,
        "one lane for each fake storage group"
    );
    assert!(status.lane_count.as_deref().is_none_or(is_decimal));
    let mut roots: Vec<u32> = status
        .lanes
        .iter()
        .flat_map(|lane| lane.root_indexes.iter().copied())
        .collect();
    roots.sort_unstable();
    assert_eq!(roots, [0, 1]);
    for lane in &status.lanes {
        assert!(lane.limit >= 1);
        assert!(lane.root_count.as_deref().is_none_or(is_decimal));
        assert!(is_decimal(&lane.remaining_directories));
        assert!(is_decimal(&lane.failures));
    }

    let (released, wake) = &*gate;
    *released.lock().map_err(|_| "scan gate lock poisoned")? = true;
    wake.notify_all();

    let mut lane_events = vec![lane_event];
    let finished = loop {
        let event = next_event(&events, deadline)?;
        if event.topic == "job.finished" && event.job_ref.as_ref() == Some(&receipt.job_ref) {
            break event;
        }
        if event.topic == "scan.lane_status" && event.job_ref.as_ref() == Some(&receipt.job_ref) {
            lane_events.push(event);
        }
    };
    assert!(lane_events.iter().all(|event| event.seq < finished.seq));

    let late_deadline = Instant::now() + Duration::from_millis(300);
    while Instant::now() < late_deadline {
        if let Some(wire) = events.next_json() {
            let event: EventEnvelope = serde_json::from_str(&wire)?;
            assert!(
                event.topic != "scan.lane_status"
                    || event.job_ref.as_ref() != Some(&receipt.job_ref),
                "lane status was queued after job.finished"
            );
        } else {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    Ok(())
}
