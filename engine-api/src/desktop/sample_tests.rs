//! The desktop composition exposes the sample use cases and their events.

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use du_core_contracts::{
    CallEnvelope, ENVELOPE_VERSION, EventEnvelope, Id,
    job::{HistoryAppend, JobHistory, JobSummary},
    ports::{PortError, PortErrorKind, Random},
    sample_ipc::{SampleSession, SampleSessionState, SampleStateChanged},
    scan_observation::{OpenedScopeRoot, ScopeRootPort},
    volume::{
        Capability, CapabilityState, VolumeCapabilities, VolumeCapacity, VolumeEnumeration,
        VolumePort,
    },
};
use serde_json::json;

use crate::api::{CallerCtx, Channel, Engine, EngineBuilder};

struct NoRoots;

impl ScopeRootPort for NoRoots {
    fn open_root(&self, _: &[u8]) -> Result<OpenedScopeRoot, PortError> {
        Err(unsupported())
    }
}

struct NoVolumes;

impl VolumePort for NoVolumes {
    fn enumerate(&self) -> VolumeEnumeration {
        let none = Capability {
            state: CapabilityState::Unsupported,
            reason_key: Some("os.unsupported"),
        };
        VolumeEnumeration {
            mounts: Ok(Vec::new()),
            capabilities: VolumeCapabilities {
                enumerate: none.clone(),
                identity: none.clone(),
                capacity: none.clone(),
                path_access: none.clone(),
                path_compare_rule: none.clone(),
                mount_boundary: none.clone(),
                alias: none,
            },
        }
    }

    fn path_access(&self, _: &[u8]) -> Result<(), PortError> {
        Err(unsupported())
    }

    fn capacity(&self, _: &[u8]) -> Result<VolumeCapacity, PortError> {
        Err(unsupported())
    }
}

#[derive(Default)]
struct CountingHistory(AtomicUsize);

impl JobHistory for CountingHistory {
    fn append(&self, _: &JobSummary) -> Result<HistoryAppend, String> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(HistoryAppend::Added)
    }
}

struct Entropy;

impl Random for Entropy {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), PortError> {
        bytes.fill(7);
        Ok(())
    }
}

fn unsupported() -> PortError {
    PortError {
        kind: PortErrorKind::Unsupported,
        reason_key: "os.unsupported".into(),
        os_code: None,
    }
}

fn call(
    engine: &Engine,
    name: &str,
    channel: Channel,
    body: serde_json::Value,
) -> serde_json::Value {
    let id = Id::from_v7_bytes(*uuid::Uuid::now_v7().as_bytes());
    let reply = engine.call(
        CallEnvelope {
            name: name.into(),
            contract_version: ENVELOPE_VERSION,
            correlation_id: id.clone(),
            body,
        },
        &CallerCtx::new(channel, id, None),
    );
    json!({"error": reply.error.map(|e| e.reason_key), "body": reply.body})
}

#[test]
fn the_desktop_serves_the_sample_session_and_publishes_its_events()
-> Result<(), Box<dyn std::error::Error>> {
    let history = Arc::new(CountingHistory::default());
    let mut builder = EngineBuilder::default();
    let events = super::register(
        &mut builder,
        Arc::new(NoRoots),
        Arc::new(NoVolumes),
        history.clone(),
        &Entropy,
    )?;
    let engine = builder.build()?;
    assert!(
        call(&engine, "sample.open", Channel::Mcp, json!({}))["error"] == "command.not_provided"
    );
    let opened = call(&engine, "sample.open", Channel::Gui, json!({}));
    let session: SampleSession = serde_json::from_value(opened["body"].clone())?;
    assert_eq!(session.state, SampleSessionState::Preparing);

    let mut samples = Vec::new();
    let mut finished = false;
    let deadline = Instant::now() + Duration::from_secs(10);
    while samples.len() < 2 || !finished {
        if Instant::now() > deadline {
            return Err("sample events did not arrive".into());
        }
        let Some(json) = events.next_json() else {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        };
        let event: EventEnvelope = serde_json::from_str(&json)?;
        finished |= event.topic == "job.finished" && event.job_ref == session.job_ref;
        if event.topic == "sample.state_changed" {
            samples.push(serde_json::from_value::<SampleStateChanged>(event.body)?);
        }
    }
    assert_eq!(samples[0].sequence, 1);
    assert_eq!(samples[1].sequence, 2);
    let active = call(
        &engine,
        "sample.status",
        Channel::Gui,
        json!({"sessionRef": session.session_ref.as_str()}),
    );
    assert_eq!(active["body"]["state"], "active");
    assert_eq!(
        active["body"]["observations"].as_array().map(Vec::len),
        Some(2)
    );
    assert_eq!(history.0.load(Ordering::Relaxed), 0);
    Ok(())
}
