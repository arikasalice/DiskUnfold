//! A real desktop scan relays lane status before its terminal event.
use std::{
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

use du_core_contracts::{
    EventEnvelope, Id, OsPath, PathBytes, PathEncoding,
    analysis_ipc::{ScanLaneStatus, ScanStart},
    ports::{PortError, PortErrorKind},
    scan_observation::{
        AnchoredDirectory, DirectoryObservations, ObservedEntry, OpenedScopeRoot, ScopeRootPort,
    },
};
use serde_json::json;

use super::{CountingHistory, Entropy, NoVolumes, call};
use crate::api::{Channel, EngineBuilder};

type TestResult = Result<(), Box<dyn std::error::Error>>;
type Gate = Arc<(Mutex<bool>, Condvar)>;

struct ReleaseOnDrop([Gate; 2]);

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        for gate in &self.0 {
            let (released, wake) = &**gate;
            if let Ok(mut released) = released.lock() {
                *released = true;
                wake.notify_all();
            }
        }
    }
}

struct Roots([Gate; 2]);

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
                gate: self.0[if second { 1 } else { 0 }].clone(),
                device: if second { 2 } else { 1 },
                inode: if second { 20 } else { 10 },
                denied: !second,
                waited: false,
            }),
        })
    }
}

struct WaitingDirectory {
    gate: Gate,
    device: u64,
    inode: u64,
    denied: bool,
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
            if self.denied {
                return Ok(vec![ObservedEntry {
                    name: b"denied\0".to_vec(),
                    enumerated_inode: 11,
                    enumerated_kind: du_core_contracts::scan_observation::ObservedKind::Directory,
                    metadata: Ok(du_core_contracts::scan_observation::MetadataObservation {
                        kind: du_core_contracts::scan_observation::ObservedKind::Directory,
                        device: self.device,
                        inode: 11,
                        size: Ok(0),
                        modified_at:
                            du_core_contracts::scan_observation::ObservedValue::NotApplicable,
                        native_modified_at: None,
                        allocated:
                            du_core_contracts::scan_observation::ObservedValue::NotApplicable,
                        link_count:
                            du_core_contracts::scan_observation::ObservedValue::NotApplicable,
                        space_identity:
                            du_core_contracts::scan_observation::ObservedValue::NotApplicable,
                    }),
                }]);
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
        Err(PortError {
            kind: PortErrorKind::PermissionDenied,
            reason_key: "scan.path_denied".into(),
            os_code: None,
        })
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

fn next_lane_status(
    events: &crate::desktop::DesktopEvents,
    job_ref: &Id,
    deadline: Instant,
) -> Result<(EventEnvelope, ScanLaneStatus), Box<dyn std::error::Error>> {
    loop {
        let event = next_event(events, deadline)?;
        if event.topic == "scan.lane_status" && event.job_ref.as_ref() == Some(job_ref) {
            let status = serde_json::from_value(event.body.clone())?;
            return Ok((event, status));
        }
    }
}

fn next_failure_lane_status(
    events: &crate::desktop::DesktopEvents,
    job_ref: &Id,
    deadline: Instant,
) -> Result<(EventEnvelope, ScanLaneStatus), Box<dyn std::error::Error>> {
    loop {
        let (event, status) = next_lane_status(events, job_ref, deadline)?;
        if status
            .lanes
            .iter()
            .any(|lane| lane.failures.parse::<u64>().is_ok_and(|count| count > 0))
        {
            return Ok((event, status));
        }
    }
}

fn wait_for_finish(
    events: &crate::desktop::DesktopEvents,
    job_ref: &Id,
    deadline: Instant,
    mut status: ScanLaneStatus,
    mut lane_sequences: Vec<u64>,
) -> Result<(EventEnvelope, ScanLaneStatus, Vec<u64>), Box<dyn std::error::Error>> {
    loop {
        let event = next_event(events, deadline)?;
        if event.job_ref.as_ref() != Some(job_ref) {
            continue;
        }
        if event.topic == "job.finished" {
            return Ok((event, status, lane_sequences));
        }
        if event.topic == "scan.lane_status" {
            status = serde_json::from_value(event.body.clone())?;
            lane_sequences.push(event.seq);
        }
    }
}

fn assert_filtered_failure_counts(
    engine: &crate::api::Engine,
    job_ref: &Id,
    status: &ScanLaneStatus,
) -> TestResult {
    let job_status = call(
        engine,
        "job.status",
        Channel::Gui,
        json!({"jobRef":job_ref}),
    );
    let version = job_status["body"]["resultVersion"]
        .as_str()
        .ok_or("finished job has no result version")?;
    let mut counted = 0_u64;
    let mut listed = 0_u64;
    for lane in &status.lanes {
        counted += lane.failures.parse::<u64>()?;
        let page = call(
            engine,
            "scan.failures",
            Channel::Gui,
            json!({
                "resultVersion":version,
                "rootIndexes":lane.root_indexes,
                "failuresOnly":true
            }),
        );
        assert!(page["error"].is_null());
        listed += page["body"]["totalCount"]
            .as_u64()
            .ok_or("filtered failure count missing")?;
        if lane.failures != "0" {
            assert_eq!(page["body"]["rows"][0]["reasonKey"], "scan.path_denied");
        }
    }
    assert_eq!(counted, 1);
    assert_eq!(listed, 1);
    assert_eq!(counted, listed);
    Ok(())
}

fn assert_no_late_lane_status(events: &crate::desktop::DesktopEvents, job_ref: &Id) -> TestResult {
    let deadline = Instant::now() + Duration::from_millis(300);
    while Instant::now() < deadline {
        if let Some(wire) = events.next_json() {
            let event: EventEnvelope = serde_json::from_str(&wire)?;
            assert!(
                event.topic != "scan.lane_status" || event.job_ref.as_ref() != Some(job_ref),
                "lane status was queued after job.finished"
            );
        } else {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    Ok(())
}

#[test]
fn a_finished_scan_lane_failures_match_filtered_problem_rows() -> TestResult {
    let gate_c: Gate = Arc::new((Mutex::new(false), Condvar::new()));
    let gate_d: Gate = Arc::new((Mutex::new(false), Condvar::new()));
    let _release_on_drop = ReleaseOnDrop([gate_c.clone(), gate_d.clone()]);
    let mut builder = EngineBuilder::default();
    let events = crate::desktop::register(
        &mut builder,
        Arc::new(Roots([gate_c.clone(), gate_d.clone()])),
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
    let (lane_event, status) = next_lane_status(&events, &receipt.job_ref, deadline)?;
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

    release(&gate_c)?;
    let failed_lane_event = next_failure_lane_status(&events, &receipt.job_ref, deadline)?;
    release(&gate_d)?;

    let (failed_lane_event, failed_status) = failed_lane_event;
    let (finished, latest_status, lane_sequences) = wait_for_finish(
        &events,
        &receipt.job_ref,
        deadline,
        failed_status,
        vec![lane_event.seq, failed_lane_event.seq],
    )?;
    assert!(
        lane_sequences
            .iter()
            .all(|sequence| *sequence < finished.seq)
    );

    assert_filtered_failure_counts(&engine, &receipt.job_ref, &latest_status)?;
    assert_no_late_lane_status(&events, &receipt.job_ref)?;
    Ok(())
}

fn release(gate: &Gate) -> Result<(), Box<dyn std::error::Error>> {
    let (released, wake) = &**gate;
    *released.lock().map_err(|_| "scan gate lock poisoned")? = true;
    wake.notify_all();
    Ok(())
}
