//! F02 cancellation and sealing through the platform's real directory adapter.
//!
//! The probe sequences wrapper entry, the delegate-call marker, and return independently of
//! the gate-state mutex. It marks the synchronous `job.cancel` dispatch response and later gate
//! release, but cannot observe calls between runtime flag-setting and that response.
//! Filtered enumeration is not instrumented, and the test does not launch the GUI.
//! Follow-up completion tasks:
//! - Observe the runtime flag transition to cover the remaining pre-response interval.
//! - Instrument filtered enumeration and verify its read-start behavior independently.
mod fixture;
mod gate;

use super::*;
use du_core_contracts::{
    Id, ResultEnvelope,
    analysis_ipc::{
        AnalysisJobControl, AnalysisJobStatus, JobControlOutcome, ScanStart, ScopeReview,
    },
    job::JobState,
};
use du_engine_api::api::Engine;
use du_engine_api::desktop::SharedTableLimit;
use fixture::{isolated_tempdir, native_name, scope_path};
use gate::{ObservedRoots, Probe, ReleaseOnDrop};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

type RelativePath = Vec<Vec<u8>>;
type TestResult = Result<(), Box<dyn std::error::Error>>;
const GATE_TIMEOUT: Duration = Duration::from_secs(30);
const DIRECTORY_COUNT: usize = 16;
const FILES_PER_DIRECTORY: usize = 4;

#[test]
fn native_adapter_multiworker_cancel_seals_returned_entries_once_after_gate_release() -> TestResult
{
    let fixture = isolated_tempdir()?;
    let profile = isolated_tempdir()?;
    let scope = fs::canonicalize(fixture.path())?.join("scope");
    fs::create_dir(&scope)?;
    for directory in 0..DIRECTORY_COUNT {
        let path = scope.join(format!("worker-{directory:02}"));
        fs::create_dir(&path)?;
        for file in 0..FILES_PER_DIRECTORY {
            fs::write(
                path.join(format!("item-{file:02}.bin")),
                [directory as u8, file as u8],
            )?;
        }
    }

    let probe = Arc::new(Probe::default());
    let mut native = scan_ports().ok_or("platform scan adapters unavailable")?;
    native.roots = Arc::new(ObservedRoots {
        native: native.roots,
        probe: probe.clone(),
    });
    let database = crate::startup_database::open(profile.path())?;
    let mut builder = EngineBuilder::default();
    let limit: SharedTableLimitSource = Arc::new(SharedTableLimit::default);
    let _services = register_analysis(&mut builder, native, &database, limit, None)?;
    let engine = builder.build()?;
    let _release_on_error = ReleaseOnDrop(probe.clone());

    let draft = json!({
        "includes": [scope_path(&scope)],
        "exclusions": [],
        "boundary": "current_volume",
        "includedMounts": [],
        "source": "gui"
    });
    let review: ScopeReview = serde_json::from_value(request(
        &engine,
        "scope.review",
        json!({"draft": draft, "requestSeq": 1}),
    )?)?;
    probe.arm()?;
    let start: ScanStart = serde_json::from_value(request(
        &engine,
        "scan.start",
        json!({"draft": draft, "reviewFingerprint": review.review_fingerprint}),
    )?)?;

    probe.wait_for_two_workers()?;
    let cancel: AnalysisJobControl = serde_json::from_value(request_with_response_hook(
        &engine,
        "job.cancel",
        json!({"jobRef": start.job_ref}),
        || probe.mark_cancel_response(),
    )?)?;
    assert_eq!(cancel.outcome, JobControlOutcome::Accepted);
    assert_eq!(cancel.state, JobState::CancelRequested);
    // The remaining interval starts before the response marker; adapter events use a shared sequence.
    probe.release_test_gate()?;

    let status = wait_for_cancelled(&engine, &start.job_ref)?;
    assert_eq!(status.state, JobState::Cancelled);
    let sealed_version = status
        .result_version
        .ok_or("cancelled result has no seal ID")?;
    let region =
        request(&engine, "query.region.open", json!({"kind": "browse_list"}))?["regionRef"].clone();
    let actual = browse_returned_paths(&engine, &region, &sealed_version)?;
    let snapshot = probe.snapshot()?;
    eprintln!(
        "F02_CANCEL_TRACE lock_observations_after_release={} wrapper_entries_after_reply={} adapter_dispatches_after_reply={} wrapper_entries_after_release={} adapter_dispatches_after_release={} workers_last_event_at_reply={:?}",
        snapshot.post_release_read_start_observations,
        snapshot.post_reply_wrapper_entries,
        snapshot.post_reply_adapter_dispatches,
        snapshot.post_release_wrapper_entries,
        snapshot.post_release_adapter_dispatches,
        snapshot.workers_last_event_at_cancel_reply,
    );
    for event in &snapshot.trace {
        eprintln!("F02_CANCEL_TRACE {event}");
    }
    assert_eq!(
        snapshot.started, snapshot.returned,
        "a platform adapter call did not return"
    );
    assert_eq!(
        snapshot.post_release_wrapper_entries, 0,
        "a wrapper method entered after gate release; trace: {:?}",
        snapshot.trace
    );
    assert!(snapshot.native_entries.values().all(|count| *count == 1));
    assert!(actual.values().all(|count| *count == 1));
    assert_eq!(
        actual, snapshot.native_entries,
        "sealed result differs from returned batches"
    );
    assert!(actual.len() >= 2 * FILES_PER_DIRECTORY);

    eprintln!(
        "F02_NATIVE_CANCEL_SEAL os={} arch={} workers>=2 adapter_reads={} returned_entries={} post_release_wrapper_entries={} post_release_adapter_dispatches={} post_reply_wrapper_entries={} post_reply_adapter_dispatches={} lock_observations_after_release={} observation=sequenced cancel_reply_gap=before_response_unmeasured filtered_enumeration=unmeasured gui=not_launched state=cancelled seal={sealed_version:?}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        snapshot.returned,
        actual.len(),
        snapshot.post_release_wrapper_entries,
        snapshot.post_release_adapter_dispatches,
        snapshot.post_reply_wrapper_entries,
        snapshot.post_reply_adapter_dispatches,
        snapshot.post_release_read_start_observations,
    );
    Ok(())
}

fn request(engine: &Engine, name: &str, body: Value) -> Result<Value, Box<dyn std::error::Error>> {
    request_with_response_hook(engine, name, body, || Ok(()))
}

fn request_with_response_hook(
    engine: &Engine,
    name: &str,
    body: Value,
    after_response: impl FnOnce() -> Result<(), String>,
) -> Result<Value, Box<dyn std::error::Error>> {
    let correlation_id = Id::from_v7_bytes([42; 16]);
    let message = json!({
        "command": "du_call",
        "args": {"envelope": {
            "name": name,
            "contractVersion": 1,
            "correlationId": correlation_id,
            "body": body
        }}
    });
    let response = crate::ipc::dispatch(engine, &message.to_string()).ok_or("no IPC response")?;
    after_response().map_err(std::io::Error::other)?;
    let envelope: ResultEnvelope = serde_json::from_str(&response)?;
    if let Some(error) = envelope.error {
        return Err(format!("{name}: {error:?}").into());
    }
    Ok(envelope.body)
}

fn wait_for_cancelled(
    engine: &Engine,
    job: &Id,
) -> Result<AnalysisJobStatus, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + GATE_TIMEOUT;
    loop {
        let status: AnalysisJobStatus =
            serde_json::from_value(request(engine, "job.get", json!({"jobRef": job}))?)?;
        if status.state == JobState::Cancelled {
            return Ok(status);
        }
        if status.state.is_terminal() || Instant::now() >= deadline {
            return Err(format!("scan ended before cancelled seal: {:?}", status.state).into());
        }
        thread::yield_now();
    }
}

fn browse_returned_paths(
    engine: &Engine,
    region: &Value,
    version: &Id,
) -> Result<BTreeMap<RelativePath, usize>, Box<dyn std::error::Error>> {
    let mut actual = BTreeMap::new();
    let root = browse_page(engine, region, version, 1, None)?;
    let mut folders = Vec::new();
    for row in root["page"]["rows"].as_array().ok_or("missing root rows")? {
        let name = row_name(row)?;
        let path = vec![native_name(&name)];
        *actual.entry(path.clone()).or_default() += 1;
        folders.push((path, row["item"].clone()));
    }
    if folders.len() != DIRECTORY_COUNT {
        return Err(format!("root returned {} fixture directories", folders.len()).into());
    }
    for (index, (parent, item)) in folders.iter().enumerate() {
        let page = browse_page(engine, region, version, index as u32 + 2, Some(item))?;
        for row in page["page"]["rows"]
            .as_array()
            .ok_or("missing child rows")?
        {
            let mut path = parent.clone();
            path.push(native_name(&row_name(row)?));
            *actual.entry(path).or_default() += 1;
        }
    }
    Ok(actual)
}

fn browse_page(
    engine: &Engine,
    region: &Value,
    version: &Id,
    request_seq: u32,
    location: Option<&Value>,
) -> Result<Value, Box<dyn std::error::Error>> {
    let mut body = json!({
        "regionRef": region,
        "viewKind": "list",
        "requestSeq": request_seq,
        "resultVersion": version,
        "pageSize": 128
    });
    if let Some(location) = location {
        body["location"] = location.clone();
    }
    let opened = request(engine, "browse.open", body)?;
    if opened["kind"] != "opened"
        || opened["page"]["resultVersion"] != json!(version)
        || opened["page"]["completeness"] != "cancelled"
        || !opened["page"]["nextCursor"].is_null()
    {
        return Err("sealed cancelled version did not open as one complete page".into());
    }
    Ok(opened)
}

fn row_name(row: &Value) -> Result<String, Box<dyn std::error::Error>> {
    if row["displayName"]["state"] != "present" {
        return Err("observed fixture name is not displayable".into());
    }
    row["displayName"]["value"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "missing display name".into())
}
